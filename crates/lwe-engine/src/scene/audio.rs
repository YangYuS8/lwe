//! CPU-prepared, immutable Scene audio and its reversible runtime ownership.
//! No package reads, extraction, or media paths are delegated to libmpv.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use lwe_scene::{SceneAssets, SceneResource};
use serde_json::{Map, Value};

use super::unsupported;
use crate::mpv::{MpvAudioPlayer, SceneAudioFormat};

const MAX_SOUNDS: usize = 8;
const MAX_CLIP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_AUDIO_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ID3V2_BYTES: u64 = 1024 * 1024;
const READ_CHUNK: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct PreparedAudio {
    clip: Arc<AudioClip>,
    pub(crate) volume: f64,
}

impl std::fmt::Debug for PreparedAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedAudio")
            .field("bytes", &self.clip.bytes)
            .field("volume", &self.volume)
            .finish()
    }
}

impl PreparedAudio {
    pub(crate) fn clip_identity(&self) -> usize {
        Arc::as_ptr(&self.clip) as usize
    }

    pub(crate) fn payload_bytes(&self) -> u64 {
        self.clip.bytes
    }
}

struct AudioClip {
    source: Arc<File>,
    bytes: u64,
    format: SceneAudioFormat,
}

/// Call only from CPU preparation, after the project-property resolver. This
/// subset admits ordinary, initially audible single-file FLAC or MP3 loops.
/// Multi-track selection, random intervals, scripts, and spatial sound remain
/// explicit capability failures.
pub(crate) fn prepare_audio(assets: &SceneAssets, scene: &Value) -> Result<Vec<PreparedAudio>> {
    let objects = scene
        .get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| unsupported("audio", "Scene audio needs an object array"))?;
    let mut declarations = Vec::new();
    let mut resources = BTreeMap::<String, (SceneResource, SceneAudioFormat)>::new();
    let mut total = 0_u64;
    for object in objects
        .iter()
        .filter(|object| object.get("sound").is_some())
    {
        ensure!(
            declarations.len() < MAX_SOUNDS,
            "Scene sound layer limit exceeded"
        );
        let (name, format, volume) = declaration(object)?;
        if !resources.contains_key(name) {
            let resource = assets.resource(name)?;
            ensure!(
                resource.len() <= MAX_CLIP_BYTES,
                "Scene audio clip byte limit exceeded"
            );
            total = total
                .checked_add(resource.len())
                .context("Scene audio byte total overflow")?;
            ensure!(
                total <= MAX_AUDIO_BYTES,
                "Scene audio total byte limit exceeded"
            );
            if format == SceneAudioFormat::Flac {
                validate_flac(&resource)?;
            }
            resources.insert(name.to_string(), (resource, format));
        }
        declarations.push((name.to_string(), volume));
    }
    // Check declarations and resource size limits before copying. Duplicate
    // sound paths share one sealed anonymous descriptor.
    let clips = resources
        .into_iter()
        .map(|(name, (resource, format))| {
            let bytes = resource.len();
            let source = sealed_source(&resource)?;
            if format == SceneAudioFormat::Mp3 {
                validate_mp3(&source, bytes)?;
            }
            Ok((
                name,
                Arc::new(AudioClip {
                    source: Arc::new(source),
                    bytes,
                    format,
                }),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(declarations
        .into_iter()
        .map(|(name, volume)| PreparedAudio {
            clip: clips[&name].clone(),
            volume,
        })
        .collect())
}

fn declaration(object: &Value) -> Result<(&str, SceneAudioFormat, f64)> {
    let object = object
        .as_object()
        .ok_or_else(|| unsupported("audio", "Invalid sound object"))?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "id" | "name"
                | "sound"
                | "playbackmode"
                | "volume"
                | "mintime"
                | "maxtime"
                | "startsilent"
                | "muteineditor"
                | "solid"
                | "disablepropagation"
        ) {
            return Err(unsupported(
                "audio",
                format!("Unsupported sound field {key}"),
            ));
        }
    }
    if !object
        .get("id")
        .is_some_and(|value| value.as_u64().is_some())
        || !object.get("name").is_none_or(Value::is_string)
    {
        return Err(unsupported("audio", "Invalid sound identity"));
    }
    let sounds = object
        .get("sound")
        .and_then(Value::as_array)
        .ok_or_else(|| unsupported("audio", "Sound must be a single-file array"))?;
    if sounds.len() != 1 || object.get("playbackmode").and_then(Value::as_str) != Some("loop") {
        return Err(unsupported(
            "audio",
            "Only a single-file loop sound is supported",
        ));
    }
    let format_error = || unsupported("audio", "Only FLAC and bounded MP3 sounds are supported");
    let name = sounds[0].as_str().ok_or_else(format_error)?;
    let format = match name.rsplit_once('.').map(|(_, extension)| extension) {
        Some("flac") => SceneAudioFormat::Flac,
        Some(extension) if extension.eq_ignore_ascii_case("mp3") => SceneAudioFormat::Mp3,
        _ => return Err(format_error()),
    };
    require_bool(object, "startsilent", false)?;
    // The native sound descriptor's editor bit does not mute wallpaper
    // playback. Validate the author flag without changing playback policy.
    if object
        .get("muteineditor")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(unsupported("audio", "Invalid sound editor mute flag"));
    }
    require_bool(object, "disablepropagation", false)?;
    require_bool(object, "solid", true)?;
    // The verified native single-file loop branch sets loop=true/countdown=0;
    // only the random mode consumes these intervals. Still validate the fields.
    let minimum = finite_number(object, "mintime", 1.0)?;
    let maximum = finite_number(object, "maxtime", 5.0)?;
    if minimum < 0.0 || maximum < minimum || maximum > 120.0 {
        return Err(unsupported("audio", "Unsupported sound timing interval"));
    }
    let volume = finite_number(object, "volume", 1.0)?;
    if !(0.0..=1.0).contains(&volume) {
        return Err(unsupported(
            "audio",
            "Sound volume must be between zero and one",
        ));
    }
    Ok((name, format, volume))
}

fn require_bool(object: &Map<String, Value>, key: &str, expected: bool) -> Result<()> {
    if !object
        .get(key)
        .is_none_or(|value| value.as_bool() == Some(expected))
    {
        return Err(unsupported(
            "audio",
            format!("Unsupported sound setting {key}"),
        ));
    }
    Ok(())
}

fn finite_number(object: &Map<String, Value>, key: &str, default: f64) -> Result<f64> {
    match object.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite())
            .ok_or_else(|| {
                unsupported(
                    "audio",
                    format!("Sound {key} must be a resolved finite number"),
                )
            }),
    }
}

fn validate_flac(resource: &SceneResource) -> Result<()> {
    ensure!(resource.len() >= 42, "Scene FLAC resource is truncated");
    let header = resource.read_range(0, 42, 42)?;
    ensure!(
        &header[..4] == b"fLaC" && header[4] & 0x7f == 0 && header[5..8] == [0, 0, 34],
        "Scene audio requires a FLAC STREAMINFO header"
    );
    let packed = u64::from_be_bytes(header[18..26].try_into().unwrap());
    let rate = packed >> 44;
    let channels = ((packed >> 41) & 7) + 1;
    let bits = ((packed >> 36) & 31) + 1;
    let samples = packed & ((1_u64 << 36) - 1);
    ensure!(
        (8_000..=192_000).contains(&rate)
            && channels <= 2
            && (8..=32).contains(&bits)
            && samples > 0
            && samples <= rate * 6 * 3600,
        "Unsupported FLAC stream dimensions or duration"
    );
    Ok(())
}

fn mp3_error(detail: &'static str) -> anyhow::Error {
    unsupported("audio", detail)
}

fn syncsafe_size(bytes: [u8; 4]) -> Option<u64> {
    bytes.into_iter().try_fold(0, |value, byte| {
        (byte < 0x80).then_some((value << 7) | u64::from(byte))
    })
}

fn read_id3v2(input: &mut BufReader<&File>, header: [u8; 10], bytes: u64) -> Result<u64> {
    let version = header[3];
    ensure!(
        matches!(version, 3 | 4) && header[4..6] == [0, 0],
        mp3_error("Unsupported ID3v2 version or flags")
    );
    let mut remaining = syncsafe_size(header[6..10].try_into().unwrap())
        .filter(|size| *size <= MAX_ID3V2_BYTES && *size < bytes - 10)
        .ok_or_else(|| mp3_error("Invalid ID3v2 tag size"))?;
    let next_audio = 10 + remaining;
    while remaining >= 10 {
        let mut frame = [0; 10];
        input.read_exact(&mut frame)?;
        remaining -= 10;
        if frame[0] == 0 {
            ensure!(frame == [0; 10], mp3_error("Invalid ID3v2 padding"));
            break;
        }
        ensure!(
            frame[..4]
                .iter()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
                && frame[8..10] == [0, 0],
            mp3_error("Unsupported ID3v2 frame name or flags")
        );
        let encoded_size = frame[4..8].try_into().unwrap();
        let size = if version == 3 {
            Some(u64::from(u32::from_be_bytes(encoded_size)))
        } else {
            syncsafe_size(encoded_size)
        }
        .filter(|size| *size > 0 && *size <= remaining)
        .ok_or_else(|| mp3_error("Invalid ID3v2 frame length"))?;
        input.seek_relative(size as i64)?;
        remaining -= size;
    }
    let mut padding = [0; 4096];
    while remaining != 0 {
        let count = remaining.min(padding.len() as u64) as usize;
        input.read_exact(&mut padding[..count])?;
        ensure!(
            padding[..count].iter().all(|byte| *byte == 0),
            mp3_error("Invalid ID3v2 padding")
        );
        remaining -= count as u64;
    }
    Ok(next_audio)
}

fn validate_mp3(source: &File, bytes: u64) -> Result<()> {
    ensure!(bytes >= 10, mp3_error("Truncated MP3 resource"));
    let mut header = [0; 10];
    let mut input = BufReader::with_capacity(READ_CHUNK, source);
    input.read_exact(&mut header)?;
    let mut offset = if &header[..3] == b"ID3" {
        read_id3v2(&mut input, header, bytes)?
    } else {
        input.rewind()?;
        0
    };

    const BITRATES: [u64; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    const RATES: [u64; 4] = [44_100, 48_000, 32_000, 0];
    let mut stream = None;
    let mut frames = 0;
    while offset < bytes {
        let mut header = [0; 4];
        let remaining = bytes - offset;
        ensure!(remaining >= 4, mp3_error("Truncated MP3 frame"));
        input.read_exact(&mut header)?;
        if remaining == 128 && &header[..3] == b"TAG" {
            // ID3v1 is valid only after the final complete MP3 frame.
            break;
        }
        let header = u32::from_be_bytes(header);
        let bitrate = BITRATES[((header >> 12) & 15) as usize];
        let rate = RATES[((header >> 10) & 3) as usize];
        ensure!(
            header & 0xfffe_0000 == 0xfffa_0000 && bitrate != 0 && rate != 0 && header & 3 != 2,
            mp3_error("Invalid or unsupported MPEG-1 Layer III frame")
        );
        let mono = (header >> 6) & 3 == 3;
        let expected = stream.get_or_insert((rate, mono));
        ensure!(
            *expected == (rate, mono),
            mp3_error("MP3 stream format changes between frames")
        );
        let length = 144_000 * bitrate / rate + u64::from(header >> 9 & 1);
        ensure!(
            length <= remaining,
            mp3_error("Truncated MP3 frame payload")
        );
        frames += 1;
        ensure!(
            frames * 1152 <= rate * 6 * 3600,
            mp3_error("MP3 duration exceeds the Scene audio limit")
        );
        input.seek_relative((length - 4) as i64)?;
        offset += length;
    }
    ensure!(
        frames >= 2,
        mp3_error("MP3 needs at least two complete frames")
    );
    input.rewind()?;
    Ok(())
}

fn sealed_source(resource: &SceneResource) -> Result<File> {
    let name = CString::new("lwe-scene-audio")?;
    let descriptor =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    ensure!(
        descriptor >= 0,
        "Unable to create prepared Scene audio: {}",
        std::io::Error::last_os_error()
    );
    let mut source = unsafe { File::from_raw_fd(descriptor) };
    let mut offset = 0;
    while offset < resource.len() {
        let count = (resource.len() - offset).min(READ_CHUNK as u64) as usize;
        source
            .write_all(&resource.read_range(offset, count, READ_CHUNK)?)
            .context("Copy prepared Scene audio")?;
        offset += count as u64;
    }
    source
        .seek(SeekFrom::Start(0))
        .context("Rewind prepared Scene audio")?;
    ensure!(
        unsafe { libc::fchmod(source.as_raw_fd(), 0o400) } == 0,
        "Unable to protect prepared Scene audio"
    );
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    ensure!(
        unsafe { libc::fcntl(source.as_raw_fd(), libc::F_ADD_SEALS, seals) } == 0,
        "Unable to seal prepared Scene audio"
    );
    Ok(source)
}

/// Candidate players are always paused and muted. The owner activates them only
/// after atomic assignment persistence and deactivates the previous generation
/// first. Keeping a deactivated previous player preserves its rollback timer.
pub(crate) struct SceneAudio {
    players: Vec<(MpvAudioPlayer, f64)>,
    active: bool,
    paused: bool,
    master_volume: f64,
}

impl SceneAudio {
    pub(crate) fn new(prepared: &[PreparedAudio]) -> Result<Self> {
        let players = prepared
            .iter()
            .map(|sound| {
                Ok((
                    MpvAudioPlayer::new(sound.clip.source.clone(), sound.clip.format)?,
                    sound.volume,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            players,
            active: false,
            paused: true,
            master_volume: 1.0,
        })
    }

    pub(crate) fn poll_ready(&mut self) -> Result<bool> {
        let mut ready = true;
        for (player, _) in &mut self.players {
            ready &= player.poll_ready()?;
        }
        Ok(ready)
    }

    pub(crate) fn activate(&mut self, paused: bool, master_volume: f64) -> Result<()> {
        ensure!(self.poll_ready()?, "Candidate Scene audio is not ready");
        ensure!(
            master_volume.is_finite() && (0.0..=1.0).contains(&master_volume),
            "Invalid Scene audio master volume"
        );
        self.active = true;
        self.paused = paused;
        self.master_volume = master_volume;
        self.apply_policy()
    }

    pub(crate) fn deactivate(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        self.apply_policy()
    }

    pub(crate) fn set_paused(&mut self, paused: bool) -> Result<()> {
        if self.paused == paused {
            return Ok(());
        }
        self.paused = paused;
        self.apply_policy()
    }

    pub(crate) fn set_master_volume(&mut self, volume: f64) -> Result<()> {
        ensure!(
            volume.is_finite() && (0.0..=1.0).contains(&volume),
            "Invalid Scene audio master volume"
        );
        if self.master_volume == volume {
            return Ok(());
        }
        self.master_volume = volume;
        self.apply_policy()
    }

    fn apply_policy(&mut self) -> Result<()> {
        let mut failure = None;
        for (player, volume) in &mut self.players {
            if let Err(error) = player.set_policy(
                self.active,
                self.paused,
                layer_gain(*volume, self.master_volume),
            ) {
                failure.get_or_insert(error);
            }
        }
        if let Some(error) = failure {
            self.active = false;
            for (player, _) in &mut self.players {
                let _ = player.set_policy(false, true, 0.0);
            }
            return Err(error);
        }
        Ok(())
    }
}

fn layer_gain(volume: f64, master_volume: f64) -> f64 {
    // Native sound updates square object volume before multiplying global gain.
    volume * volume * master_volume
}

pub(crate) fn prepared_audio_bytes(prepared: &[PreparedAudio]) -> u64 {
    let mut clips = Vec::new();
    let mut total = 0;
    for sound in prepared {
        if !clips.iter().any(|clip| Arc::ptr_eq(clip, &sound.clip)) {
            total += sound.clip.bytes;
            clips.push(sound.clip.clone());
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::FileExt;
    use std::path::PathBuf;

    struct AuthoredResources(PathBuf);
    impl AuthoredResources {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "lwe-authored-audio-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn streaminfo(&self, name: &str, size: u64) {
            let mut bytes = vec![0_u8; 42];
            bytes[..8].copy_from_slice(b"fLaC\x00\x00\x00\x22");
            let packed = (48_000_u64 << 44) | (1_u64 << 41) | (23_u64 << 36) | 48_000;
            bytes[18..26].copy_from_slice(&packed.to_be_bytes());
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut file = File::create(path).unwrap();
            file.write_all(&bytes).unwrap();
            file.set_len(size.max(42)).unwrap();
        }
        fn write(&self, name: &str, bytes: &[u8]) {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        fn assets(&self) -> SceneAssets {
            SceneAssets::open(&self.0, None).unwrap()
        }
    }
    impl Drop for AuthoredResources {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sound(id: u64, name: &str, volume: f64) -> Value {
        json!({"id":id,"name":"Authored sound","sound":[name],"playbackmode":"loop","volume":volume,"mintime":1.0,"maxtime":5.0,"startsilent":false,"muteineditor":false,"solid":true,"disablepropagation":false})
    }

    fn mp3_frames() -> Vec<u8> {
        // Two structurally complete MPEG-1 Layer III frames. The separate
        // opt-in test exercises the libmpv decoder with actual audio payloads.
        let mut frames = vec![0; 417 + 418];
        frames[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
        frames[417..421].copy_from_slice(&[0xff, 0xfb, 0x92, 0x00]);
        frames
    }

    #[test]
    fn mp3_loop_accepts_bounded_tags_and_editor_mute_without_changing_volume() {
        let resources = AuthoredResources::new();
        let mut object = sound(1, "sounds/authored.MP3", 0.5);
        object["muteineditor"] = json!(true);
        let scene = json!({"objects":[object.clone(), object]});
        let raw = mp3_frames();
        for version in [None, Some(3), Some(4)] {
            let mut bytes = Vec::new();
            if let Some(version) = version {
                bytes.extend([b'I', b'D', b'3', version, 0, 0, 0, 0, 0, 16]);
                bytes.extend(b"TSSE\x00\x00\x00\x03\x00\x00\x00OK\x00\x00\x00");
            }
            bytes.extend(&raw);
            if version == Some(3) {
                bytes.extend(b"TAG");
                bytes.resize(bytes.len() + 125, 0);
            }
            resources.write("sounds/authored.MP3", &bytes);
            let prepared = prepare_audio(&resources.assets(), &scene).unwrap();
            assert_eq!(prepared[0].clip.format, SceneAudioFormat::Mp3);
            assert_eq!(prepared[0].volume, 0.5);
            assert_eq!(prepared[0].clip_identity(), prepared[1].clip_identity());
            assert_eq!(prepared_audio_bytes(&prepared), bytes.len() as u64);
        }
    }

    #[test]
    fn mp3_rejects_unknown_contents_free_bitrate_and_incomplete_streams() {
        let resources = AuthoredResources::new();
        let scene = json!({"objects":[sound(1,"input.mp3",1.0)]});
        let raw = mp3_frames();
        for (byte, value) in [(1, 0xf3), (2, 0x00), (419, 0x94), (420, 0xc0)] {
            let mut changed = raw.clone();
            changed[byte] = value;
            resources.write("input.mp3", &changed);
            assert!(prepare_audio(&resources.assets(), &scene).is_err());
        }
        let mut oversized_tag = b"ID3\x04\x00\x00\x00\x00\x7f\x7f".to_vec();
        oversized_tag.extend(&raw);
        let mut bad_tag = b"ID3\x03\x00\x00\x00\x00\x00\x0aTSSE\x00\x00\x00\x00\x00\x00".to_vec();
        bad_tag.extend(&raw);
        let mut trailing = raw.clone();
        trailing.extend(b"unexpected");
        for bytes in [
            raw[..417].to_vec(),
            raw[..raw.len() - 1].to_vec(),
            oversized_tag,
            bad_tag,
            trailing,
            b"fLaCthis-is-not-mp3".to_vec(),
        ] {
            resources.write("input.mp3", &bytes);
            assert!(prepare_audio(&resources.assets(), &scene).is_err());
        }
    }

    #[test]
    #[ignore = "requires LWE_TEST_SCENE_MP3 with a real audio file; ao=null, no desktop/backend"]
    fn actual_mp3_source_decodes_with_a_null_output() -> Result<()> {
        use std::time::{Duration, Instant};

        let path =
            PathBuf::from(std::env::var_os("LWE_TEST_SCENE_MP3").context("Missing MP3 fixture")?);
        let metadata = std::fs::metadata(&path)?;
        ensure!(
            metadata.is_file() && metadata.len() <= MAX_CLIP_BYTES,
            "Invalid MP3 fixture"
        );
        let resources = AuthoredResources::new();
        std::fs::copy(path, resources.0.join("source.mp3"))?;
        let mut object = sound(1, "source.mp3", 0.5);
        object["muteineditor"] = json!(true);
        let prepared = prepare_audio(&resources.assets(), &json!({"objects":[object]}))?;
        let clip = &prepared[0].clip;
        let weak = Arc::downgrade(&clip.source);
        let mut player = MpvAudioPlayer::new_null_output(Arc::clone(&clip.source), clip.format)?;
        drop(prepared);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !player.poll_ready()? {
            ensure!(Instant::now() < deadline, "Prepared MP3 did not load");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(player.property("current-ao").is_none_or(|ao| ao == "null"));
        assert_eq!(player.property("file-format").as_deref(), Some("mp3"));
        assert_eq!(player.property("pause").as_deref(), Some("yes"));
        assert_eq!(player.property("mute").as_deref(), Some("yes"));
        player.set_policy(true, false, 0.0)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            player.poll_ready()?;
            let time = player
                .property("time-pos")
                .and_then(|time| time.parse::<f64>().ok());
            if time.is_some_and(|time| time >= 0.025) {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Prepared MP3 did not play on null output"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(player.property("current-ao").as_deref(), Some("null"));
        assert_eq!(player.property("mute").as_deref(), Some("yes"));
        player.set_policy(false, true, 0.0)?;
        drop(player);
        assert!(weak.upgrade().is_none());
        Ok(())
    }

    #[test]
    fn preparation_preserves_declared_volumes_and_seals_shared_payload() {
        let resources = AuthoredResources::new();
        resources.streaminfo("sounds/authored.flac", 42);
        let prepared = prepare_audio(&resources.assets(), &json!({"objects":[sound(1,"sounds/authored.flac",1.0),sound(2,"sounds/authored.flac",0.0)]})).unwrap();
        assert_eq!(prepared.len(), 2);
        assert_eq!((prepared[0].volume, prepared[1].volume), (1.0, 0.0));
        assert_eq!(prepared[0].clip_identity(), prepared[1].clip_identity());
        assert_eq!(prepared_audio_bytes(&prepared), 42);
        assert_eq!(prepared[0].payload_bytes(), 42);
        let source = &prepared[0].clip.source;
        let required =
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        assert_eq!(
            unsafe { libc::fcntl(source.as_raw_fd(), libc::F_GET_SEALS) } & required,
            required
        );
        assert_eq!(
            source.write_at(&[0], 0).unwrap_err().raw_os_error(),
            Some(libc::EPERM)
        );
        assert!(source.set_len(0).is_err());
    }

    #[test]
    fn selectors_scripts_unknown_fields_and_unevaluated_properties_stay_blocked() {
        let resources = AuthoredResources::new();
        resources.streaminfo("sounds/authored.flac", 42);
        let good = sound(1, "sounds/authored.flac", 1.0);
        let mut cases = Vec::new();
        for (key, value) in [
            ("playbackmode", json!("random")),
            ("startsilent", json!(true)),
            ("volume", json!({"user":"unevaluated","value":1})),
            ("volume", json!(-1)),
            ("spatialization", json!(true)),
            ("script", json!("never execute")),
            (
                "sound",
                json!(["sounds/authored.flac", "sounds/authored.flac"]),
            ),
        ] {
            let mut object = good.clone();
            object[key] = value;
            cases.push(object);
        }
        for object in cases {
            assert!(prepare_audio(&resources.assets(), &json!({"objects":[object]})).is_err());
        }
        let unsafe_path = sound(1, "../authored.flac", 1.0);
        let error =
            prepare_audio(&resources.assets(), &json!({"objects":[unsafe_path]})).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<lwe_scene::SceneAssetError>(),
            Some(lwe_scene::SceneAssetError::UnsafePath { .. })
        ));
    }

    #[test]
    fn resource_and_layer_budgets_reject_before_payload_copy() {
        let resources = AuthoredResources::new();
        resources.streaminfo("oversize.flac", MAX_CLIP_BYTES + 1);
        assert!(
            prepare_audio(
                &resources.assets(),
                &json!({"objects":[sound(1,"oversize.flac",1.0)]})
            )
            .unwrap_err()
            .to_string()
            .contains("clip byte limit")
        );
        let mut objects = Vec::new();
        for index in 0..3 {
            let name = format!("clip-{index}.flac");
            resources.streaminfo(&name, 45 * 1024 * 1024);
            objects.push(sound(index, &name, 1.0));
        }
        assert!(
            prepare_audio(&resources.assets(), &json!({"objects":objects}))
                .unwrap_err()
                .to_string()
                .contains("total byte limit")
        );
        resources.streaminfo("small.flac", 42);
        let objects: Vec<_> = (0..9)
            .map(|index| sound(index, "small.flac", 1.0))
            .collect();
        assert!(
            prepare_audio(&resources.assets(), &json!({"objects":objects}))
                .unwrap_err()
                .to_string()
                .contains("layer limit")
        );
    }

    #[test]
    fn empty_scene_audio_is_ready_without_creating_a_player() {
        let mut audio = SceneAudio::new(&[]).unwrap();
        assert!(audio.poll_ready().unwrap());
        audio.activate(false, 1.0).unwrap();
        audio.set_paused(true).unwrap();
        audio.deactivate().unwrap();
        assert!(audio.players.is_empty());
    }

    #[test]
    fn native_sound_volume_curve_squares_the_layer_before_global_gain() {
        assert_eq!(layer_gain(1.0, 1.0), 1.0);
        assert_eq!(layer_gain(0.0, 1.0), 0.0);
        assert_eq!(layer_gain(0.5, 1.0), 0.25);
        assert_eq!(layer_gain(0.5, 0.5), 0.125);
    }
}
