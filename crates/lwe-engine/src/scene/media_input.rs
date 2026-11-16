//! Read-only MPRIS snapshots shared by active SceneScript workers.

use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex, RwLock, TryLockError, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, Thread},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use dbus::{
    Message,
    arg::{ArgType, Iter},
    blocking::{BlockingSender, Connection},
    strings::BusName,
};

const BUS_SERVICE: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const PLAYER_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const POLL_INTERVAL: Duration = Duration::from_millis(750);
const CALL_TIMEOUT: Duration = Duration::from_millis(150);
const MAX_BUS_NAMES: usize = 8192;
const MAX_PLAYERS: usize = 8;
const MAX_PROPERTIES: usize = 128;
const MAX_FIELD_BYTES: usize = 512;
const MAX_LIST_ITEMS: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Playback {
    #[default]
    Stopped,
    Paused,
    Playing,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct MediaSnapshot {
    pub player: Option<String>,
    pub track_id: Option<String>,
    pub playback: Playback,
    pub title: String,
    pub artist: String,
    pub album_title: String,
    pub album_artist: String,
    pub genres: String,
}

#[derive(Default)]
enum Readback {
    #[default]
    Pending,
    Ready(MediaSnapshot),
    Unavailable(&'static str),
}

#[derive(Default)]
struct Publication {
    state: RwLock<Readback>,
    stop: AtomicBool,
}

struct Worker {
    publication: Arc<Publication>,
    thread: Thread,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.publication.stop.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

static ACTIVE: Mutex<Weak<Worker>> = Mutex::new(Weak::new());

#[derive(Clone)]
pub(super) struct MediaFeed(Arc<Worker>);

impl MediaFeed {
    pub(super) fn acquire() -> Result<Self> {
        let mut active = ACTIVE
            .lock()
            .map_err(|_| anyhow!("MPRIS media worker registry is unavailable"))?;
        if let Some(worker) = active.upgrade() {
            return Ok(Self(worker));
        }
        let publication = Arc::new(Publication::default());
        let reader = Arc::clone(&publication);
        let thread = thread::Builder::new()
            .name("lwe-scene-mpris".into())
            .spawn(move || poll(reader))
            .context("Cannot start the MPRIS media worker")?;
        let worker = Arc::new(Worker {
            publication,
            thread: thread.thread().clone(),
        });
        *active = Arc::downgrade(&worker);
        Ok(Self(worker))
    }

    /// Pending or a contested publication leaves the consumer's state intact.
    /// An empty ready snapshot represents a successful poll with no players.
    pub(super) fn latest(&self) -> Result<Option<MediaSnapshot>> {
        match self.0.publication.state.try_read() {
            Ok(readback) => match &*readback {
                Readback::Pending => Ok(None),
                Readback::Ready(snapshot) => Ok(Some(snapshot.clone())),
                Readback::Unavailable(reason) => Err(anyhow!(*reason)),
            },
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => {
                Err(anyhow!("MPRIS media publication is unavailable"))
            }
        }
    }
}

fn poll(publication: Arc<Publication>) {
    let mut connection = None;
    while !publication.stop.load(Ordering::Acquire) {
        if connection.is_none() {
            connection = open_session_bus().ok();
        }
        let readback = match connection.as_ref() {
            Some(bus) => match poll_players(bus, &publication.stop) {
                Ok(snapshot) => Readback::Ready(snapshot),
                Err(_) => Readback::Unavailable("MPRIS media state is unavailable"),
            },
            None => Readback::Unavailable("MPRIS session bus is unavailable"),
        };
        if connection
            .as_ref()
            .is_some_and(|bus| !bus.channel().is_connected())
        {
            connection = None;
        }
        if publication.stop.load(Ordering::Acquire) {
            break;
        }
        let Ok(mut state) = publication.state.write() else {
            break;
        };
        *state = readback;
        drop(state);
        thread::park_timeout(POLL_INTERVAL);
    }
}

fn open_session_bus() -> Result<Connection> {
    // Use only an already advertised local session bus. An absent or unusable
    // address must not trigger D-Bus autolaunch or a system or remote connection.
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .context("MPRIS session bus has no local address")?;
    ensure!(
        address.len() <= 4096
            && !address.contains([';', '\0'])
            && (address.starts_with("unix:path=") || address.starts_with("unix:abstract=")),
        "MPRIS session bus address is unsupported"
    );
    Ok(Connection::new_address(&address)?)
}

fn call_message(destination: &str, path: &str, interface: &str, method: &str) -> Result<Message> {
    let destination = BusName::new(destination).map_err(|_| anyhow!("Invalid MPRIS bus name"))?;
    let mut message = Message::new_method_call(destination, path, interface, method)
        .map_err(|_| anyhow!("Cannot construct a read-only MPRIS request"))?;
    message.set_auto_start(false);
    Ok(message)
}

fn player_names(bus: &Connection) -> Result<BTreeSet<String>> {
    let message = call_message(BUS_SERVICE, BUS_PATH, BUS_SERVICE, "ListNames")?;
    let reply = bus.send_with_reply_and_block(message, CALL_TIMEOUT)?;
    let mut names = reply
        .iter_init()
        .recurse(ArgType::Array)
        .context("Invalid D-Bus name list")?;
    let mut players = BTreeSet::new();
    for _ in 0..MAX_BUS_NAMES {
        if names.arg_type() == ArgType::Invalid {
            return Ok(players);
        }
        let name = names.get::<&str>().context("Invalid D-Bus name")?;
        if name.starts_with(PLAYER_PREFIX) && name.len() <= 255 {
            players.insert(name.to_owned());
            ensure!(players.len() <= MAX_PLAYERS, "MPRIS player limit exceeded");
        }
        names.next();
    }
    ensure!(
        names.arg_type() == ArgType::Invalid,
        "D-Bus name count exceeds the MPRIS limit"
    );
    Ok(players)
}

fn poll_players(bus: &Connection, stop: &AtomicBool) -> Result<MediaSnapshot> {
    let mut best = None;
    let mut unavailable = false;
    for name in player_names(bus)? {
        if stop.load(Ordering::Acquire) {
            break;
        }
        let message = call_message(
            &name,
            PLAYER_PATH,
            "org.freedesktop.DBus.Properties",
            "GetAll",
        )?
        .append1(PLAYER_INTERFACE);
        match bus.send_with_reply_and_block(message, CALL_TIMEOUT) {
            Ok(reply) => match snapshot(&name, &reply) {
                Ok(current) => {
                    if best
                        .as_ref()
                        .is_none_or(|best: &MediaSnapshot| best.playback < current.playback)
                    {
                        let playing = current.playback == Playback::Playing;
                        best = Some(current);
                        if playing {
                            break;
                        }
                    }
                }
                Err(_) => unavailable = true,
            },
            Err(error)
                if matches!(
                    error.name(),
                    Some(
                        "org.freedesktop.DBus.Error.NameHasNoOwner"
                            | "org.freedesktop.DBus.Error.ServiceUnknown"
                    )
                ) => {}
            Err(_) => unavailable = true,
        }
    }
    if let Some(best) = best {
        return Ok(best);
    }
    ensure!(!unavailable, "No active MPRIS player answered");
    Ok(MediaSnapshot::default())
}

fn snapshot(name: &str, reply: &Message) -> Result<MediaSnapshot> {
    let mut snapshot = MediaSnapshot {
        player: Some(name.to_owned()),
        ..Default::default()
    };
    let mut playback = None;
    visit_dict(reply.iter_init(), |key, mut value| {
        match key {
            "PlaybackStatus" => {
                playback = Some(match value.get::<&str>() {
                    Some("Playing") => Playback::Playing,
                    Some("Paused") => Playback::Paused,
                    Some("Stopped") => Playback::Stopped,
                    _ => bail!("Invalid MPRIS playback state"),
                });
            }
            "Metadata" => visit_dict(value, |key, mut value| {
                match key {
                    "mpris:trackid" => {
                        snapshot.track_id = value
                            .get::<dbus::Path<'_>>()
                            .filter(|path| path.len() <= MAX_FIELD_BYTES)
                            .map(|path| path.to_string());
                    }
                    "xesam:title" => snapshot.title = text(value),
                    "xesam:artist" => snapshot.artist = text_list(value),
                    "xesam:album" => snapshot.album_title = text(value),
                    "xesam:albumArtist" => snapshot.album_artist = text_list(value),
                    "xesam:genre" => snapshot.genres = text_list(value),
                    _ => {}
                }
                Ok(())
            })?,
            _ => {}
        }
        Ok(())
    })?;
    snapshot.playback = playback.context("MPRIS player has no playback state")?;
    Ok(snapshot)
}

fn visit_dict<'a>(
    mut value: Iter<'a>,
    mut visit: impl FnMut(&str, Iter<'a>) -> Result<()>,
) -> Result<()> {
    ensure!(
        value.arg_type() == ArgType::Array && &*value.signature() == "a{sv}",
        "Invalid MPRIS dictionary"
    );
    let mut array = value
        .recurse(ArgType::Array)
        .context("Invalid MPRIS dictionary")?;
    for _ in 0..MAX_PROPERTIES {
        if array.arg_type() == ArgType::Invalid {
            return Ok(());
        }
        let mut entry = array
            .recurse(ArgType::DictEntry)
            .context("Invalid MPRIS entry")?;
        let key = entry.read::<&str>()?;
        let value = entry
            .recurse(ArgType::Variant)
            .context("Invalid MPRIS variant")?;
        visit(key, value)?;
        array.next();
    }
    ensure!(
        array.arg_type() == ArgType::Invalid,
        "MPRIS metadata entry limit exceeded"
    );
    Ok(())
}

fn bounded_text(text: &str) -> String {
    text_prefix(text, MAX_FIELD_BYTES)
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

fn text_prefix(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn text(mut value: Iter<'_>) -> String {
    value.get::<&str>().map(bounded_text).unwrap_or_default()
}

fn text_list(mut value: Iter<'_>) -> String {
    let mut output = String::new();
    let Some(mut values) = value.recurse(ArgType::Array) else {
        return output;
    };
    for _ in 0..MAX_LIST_ITEMS {
        let Some(value) = values.get::<&str>() else {
            break;
        };
        let value = bounded_text(value);
        if !value.is_empty() {
            let separator = if output.is_empty() { "" } else { ", " };
            let remaining = MAX_FIELD_BYTES - output.len();
            if remaining <= separator.len() {
                break;
            }
            let prefix = text_prefix(&value, remaining - separator.len());
            if prefix.is_empty() {
                break;
            }
            output.push_str(separator);
            output.push_str(prefix);
        }
        values.next();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::arg::{PropMap, RefArg, Variant};

    fn variant(value: impl RefArg + 'static) -> Variant<Box<dyn RefArg>> {
        Variant(Box::new(value))
    }

    #[test]
    fn player_metadata_uses_typed_bounded_plain_text() {
        let properties = PropMap::from([
            ("PlaybackStatus".into(), variant("Playing".to_string())),
            (
                "Metadata".into(),
                variant(PropMap::from([
                    (
                        "mpris:trackid".into(),
                        variant(dbus::Path::new("/org/mpris/MediaPlayer2/Track/One").unwrap()),
                    ),
                    (
                        "xesam:title".into(),
                        variant("Prefix\n".to_string() + &"🌙".repeat(200)),
                    ),
                    (
                        "xesam:artist".into(),
                        variant(vec!["First".to_string(), "Second".to_string()]),
                    ),
                    ("xesam:album".into(), variant("Record".to_string())),
                    (
                        "xesam:albumArtist".into(),
                        variant(vec!["Lead".to_string()]),
                    ),
                    ("xesam:genre".into(), variant(vec!["Synth".to_string(); 64])),
                    (
                        "mpris:artUrl".into(),
                        variant("https://example.invalid/unrequested".to_string()),
                    ),
                ])),
            ),
        ]);
        let message = Message::new_signal(PLAYER_PATH, PLAYER_INTERFACE, "Fixture")
            .unwrap()
            .append1(properties);
        let result = snapshot("org.mpris.MediaPlayer2.fixture", &message).unwrap();
        assert_eq!(result.playback, Playback::Playing);
        assert_eq!(
            result.track_id.as_deref(),
            Some("/org/mpris/MediaPlayer2/Track/One")
        );
        assert_eq!(
            result.player.as_deref(),
            Some("org.mpris.MediaPlayer2.fixture")
        );
        assert!(result.title.starts_with("Prefix🌙"));
        assert!(result.title.len() <= MAX_FIELD_BYTES && !result.title.contains('\n'));
        assert_eq!(result.artist, "First, Second");
        assert_eq!(result.album_title, "Record");
        assert_eq!(result.album_artist, "Lead");
        assert_eq!(result.genres.split(", ").count(), MAX_LIST_ITEMS);
    }

    #[test]
    fn queries_disable_activation_and_malformed_status_is_not_no_player() {
        let name = "org.mpris.MediaPlayer2.fixture";
        let request = call_message(
            name,
            PLAYER_PATH,
            "org.freedesktop.DBus.Properties",
            "GetAll",
        )
        .unwrap();
        assert!(!request.get_auto_start());
        for properties in [
            PropMap::new(),
            PropMap::from([("PlaybackStatus".into(), variant("Unknown".to_string()))]),
            PropMap::from([("PlaybackStatus".into(), variant(7u32))]),
        ] {
            let reply = Message::new_signal(PLAYER_PATH, PLAYER_INTERFACE, "Fixture")
                .unwrap()
                .append1(properties);
            assert!(snapshot(name, &reply).is_err());
        }
    }

    #[test]
    #[ignore = "requires the isolated dbus-run-session MPRIS fixture"]
    fn private_bus_publishes_shared_player_changes_and_removal() -> Result<()> {
        ensure!(
            std::env::var("LWE_TEST_PRIVATE_MPRIS").as_deref() == Ok("1"),
            "Start the fixture on a private session bus"
        );
        let first = MediaFeed::acquire()?;
        let second = MediaFeed::acquire()?;
        assert!(Arc::ptr_eq(&first.0, &second.0));
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let wait_for = |feed: &MediaFeed, matches: &dyn Fn(&MediaSnapshot) -> bool| -> Result<()> {
            loop {
                if feed.latest()?.as_ref().is_some_and(matches) {
                    return Ok(());
                }
                ensure!(
                    std::time::Instant::now() < deadline,
                    "Private MPRIS snapshot did not arrive"
                );
                thread::sleep(Duration::from_millis(5));
            }
        };
        wait_for(&first, &|snapshot| {
            snapshot.player.as_deref() == Some("org.mpris.MediaPlayer2.lwe_fixture")
                && snapshot.track_id.as_deref() == Some("/org/mpris/MediaPlayer2/Track/One")
                && snapshot.playback == Playback::Playing
                && snapshot.title == "First fixture track"
                && snapshot.artist == "Fixture artist, Second artist"
        })?;
        drop(first);
        wait_for(&second, &|snapshot| {
            snapshot.playback == Playback::Paused
                && snapshot.title == "Second fixture track"
                && snapshot.track_id.as_deref() == Some("/org/mpris/MediaPlayer2/Track/Two")
        })?;
        wait_for(&second, &|snapshot| snapshot == &MediaSnapshot::default())?;
        Ok(())
    }
}
