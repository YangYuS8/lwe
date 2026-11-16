//! Opt-in GPU acceptance using an unchanged user-owned Yakuza package.
//! Captured pixels and package resources stay outside the Git workspace.

use super::super::spectrum::SpectrumSnapshot;
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const WIDTH: i32 = 960;
const HEIGHT: i32 = 540;
const SCRIPT_WAIT: Duration = Duration::from_secs(3);

fn bind(target: &RenderTarget) {
    unsafe {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target.fbo);
        gl::BindFramebuffer(gl::READ_FRAMEBUFFER, target.fbo);
        gl::ReadBuffer(gl::COLOR_ATTACHMENT0);
    }
}

fn read_pixels(target: &RenderTarget) -> Result<Vec<u8>> {
    bind(target);
    let mut pixels = vec![0; (target.texture.width * target.texture.height * 4) as usize];
    unsafe {
        gl::ReadPixels(
            0,
            0,
            target.texture.width,
            target.texture.height,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            pixels.as_mut_ptr().cast(),
        );
    }
    check_gl("reading original Yakuza GPU acceptance pixels")?;
    Ok(pixels)
}

fn spectrum(values: [f32; 64]) -> SpectrumSnapshot {
    SpectrumSnapshot {
        script_average: values,
        ..Default::default()
    }
}

fn close(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() < 0.001
}

fn bars_match(renderer: &SceneRenderer, audio: [f32; 64], visible: bool) -> bool {
    if !renderer.scripts_ready || renderer.script_layers.len() != 64 {
        return false;
    }
    renderer
        .script_layers
        .values()
        .enumerate()
        .all(|(band, index)| {
            let layer = &renderer.layers[*index];
            close(layer.origin[0], 1065.165 + band as f32 * 30.0)
                && close(layer.origin[1], 90.0)
                && close(layer.scale[0], 5.0)
                && close(layer.scale[1], audio[band] * 50.0)
                && close(layer.angle, 0.0)
                && renderer.hidden_layers.contains(index) != visible
        })
}

/// Both a complete GPU draw and the expected asynchronously applied snapshot
/// are required. A pending worker's false return can never pass this check.
fn render_bars(
    renderer: &mut SceneRenderer,
    target: &RenderTarget,
    time: f64,
    audio: [f32; 64],
    visible: bool,
) -> Result<Vec<u8>> {
    let started = Instant::now();
    loop {
        bind(target);
        let drawn = renderer.render_internal(
            target.texture.width,
            target.texture.height,
            time,
            false,
            Some(spectrum(audio)),
        )?;
        if drawn {
            renderer.report_presented();
        }
        if drawn && bars_match(renderer, audio, visible) {
            return read_pixels(target);
        }
        ensure!(
            started.elapsed() < SCRIPT_WAIT,
            "Original Yakuza did not draw the expected complete 64-band script frame"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn redraw(
    renderer: &mut SceneRenderer,
    target: &RenderTarget,
    time: f64,
    audio: [f32; 64],
) -> Result<Vec<u8>> {
    bind(target);
    ensure!(
        renderer.render_internal(
            target.texture.width,
            target.texture.height,
            time,
            true,
            Some(spectrum(audio)),
        )?,
        "Original Yakuza forced GPU redraw produced no frame"
    );
    renderer.report_presented();
    read_pixels(target)
}

fn difference(left: &[u8], right: &[u8]) -> Value {
    assert_eq!(left.len(), right.len());
    let mut changed_pixels = 0_u64;
    let mut absolute_rgb_difference = 0_u64;
    let mut maximum_channel_difference = 0_u8;
    for (left, right) in left
        .as_chunks::<4>()
        .0
        .iter()
        .zip(right.as_chunks::<4>().0.iter())
    {
        let mut changed = false;
        for channel in 0..3 {
            let delta = left[channel].abs_diff(right[channel]);
            changed |= delta != 0;
            maximum_channel_difference = maximum_channel_difference.max(delta);
            absolute_rgb_difference += u64::from(delta);
        }
        changed_pixels += u64::from(changed);
    }
    json!({
        "changed_pixels":changed_pixels,
        "absolute_rgb_difference":absolute_rgb_difference,
        "maximum_channel_difference":maximum_channel_difference,
    })
}

struct Report {
    path: Option<PathBuf>,
    frames: Option<PathBuf>,
    value: Value,
}

impl Report {
    fn new() -> Result<Self> {
        let path = std::env::var_os("LWE_YAKUZA_GL_REPORT").map(PathBuf::from);
        Self::for_path(path)
    }

    fn live_audio() -> Result<Self> {
        let path = std::env::var_os("LWE_YAKUZA_LIVE_AUDIO_GL_REPORT")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("LWE_YAKUZA_GL_REPORT").map(|path| {
                    let path = PathBuf::from(path);
                    path.with_file_name(format!(
                        "{}-live-audio.json",
                        path.file_stem().unwrap_or_default().to_string_lossy()
                    ))
                })
            });
        let mut report = Self::for_path(path)?;
        report.value["scope"] = json!(
            "Unchanged user-owned Yakuza package, real PulseAudio/PipeWire capture from an owned silent null-sink monitor, independent 64-band FFT average, original SceneScript worker and native offscreen bar GPU pixels; excludes visible desktop assignment, default-device capture, Windows A/B parity and suspend"
        );
        Ok(report)
    }

    fn for_path(path: Option<PathBuf>) -> Result<Self> {
        let frames = if let Some(path) = &path {
            ensure!(path.is_absolute(), "GPU report path must be absolute");
            let parent = path.parent().context("GPU report has no parent")?;
            std::fs::create_dir_all(parent)?;
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .context("No workspace root")?
                .canonicalize()?;
            ensure!(
                !parent.canonicalize()?.starts_with(workspace),
                "Original Yakuza image diagnostics must stay outside the Git workspace"
            );
            let directory = parent.join(format!(
                "{}-frames",
                path.file_stem()
                    .context("No report stem")?
                    .to_string_lossy()
            ));
            std::fs::create_dir_all(&directory)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
            }
            Some(directory)
        } else {
            None
        };
        Ok(Self {
            path,
            frames,
            value: json!({
                "scope":"Unchanged user-owned Yakuza package, native offscreen OpenGL draw and injected 64-band average spectrum; excludes real desktop application, desktop audio capture, Windows A/B parity and suspend",
                "cases":[],
                "comparisons":{},
            }),
        })
    }

    fn frame(&self, name: &str, target: &RenderTarget, pixels: &[u8]) -> Result<()> {
        if let Some(directory) = &self.frames {
            let path = directory.join(format!("{name}.png"));
            let image = image::RgbaImage::from_raw(
                target.texture.width as u32,
                target.texture.height as u32,
                pixels.to_vec(),
            )
            .context("Invalid original Yakuza acceptance pixel buffer")?;
            image::imageops::flip_vertical(&image).save(&path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(())
    }

    fn compare(&mut self, name: &str, left: &[u8], right: &[u8], changes: bool) -> Result<()> {
        let diff = difference(left, right);
        let count = diff["changed_pixels"]
            .as_u64()
            .context("Invalid pixel count")?;
        self.value["comparisons"][name] = diff;
        ensure!(
            if changes { count > 0 } else { count == 0 },
            "Original Yakuza GPU comparison {name}: {count} changed pixels"
        );
        Ok(())
    }

    fn save(&mut self, outcome: &Result<()>) -> Result<()> {
        self.value["passed"] = Value::Bool(outcome.is_ok());
        if let Err(error) = outcome {
            self.value["error"] = Value::String(format!("{error:#}"));
        }
        if let Some(path) = &self.path {
            std::fs::write(path, serde_json::to_vec_pretty(&self.value)?)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        println!("{}", serde_json::to_string(&self.value)?);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
struct BarTransform {
    origin: [f32; 2],
    scale: [f32; 2],
    angle: f32,
    tint: [f32; 4],
    hidden: bool,
}

fn bar_transforms(renderer: &SceneRenderer) -> Vec<BarTransform> {
    renderer
        .script_layers
        .values()
        .map(|index| {
            let layer = &renderer.layers[*index];
            BarTransform {
                origin: layer.origin,
                scale: layer.scale,
                angle: layer.angle,
                tint: layer.tint,
                hidden: renderer.hidden_layers.contains(index),
            }
        })
        .collect()
}

fn check_case(
    prepared: &PreparedScene,
    name: &str,
    visible: bool,
    report: &mut Report,
) -> Result<()> {
    let plan = prepared
        .scripts
        .as_ref()
        .context("Original Yakuza script plan is missing")?;
    ensure!(
        plan.created_layers().len() == 63,
        "Original Yakuza must create 63 additional bars"
    );
    ensure!(
        prepared.script_layers.len() == 64,
        "Original Yakuza must retain 64 script layer handles"
    );
    ensure!(
        plan.uses_audio(),
        "Original Yakuza audio registration is missing"
    );
    ensure!(
        prepared.canvas == [3840.0, 2160.0],
        "Original Yakuza canvas dimensions changed"
    );
    let indices = prepared
        .script_layers
        .values()
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        indices.len() == 64,
        "Original Yakuza script handles alias image layers"
    );
    for index in &indices {
        ensure!(
            prepared.layers[*index].size == [4.0, 4.0],
            "Original Yakuza bar autosize must retain the 4x4 texture geometry"
        );
    }
    report.value["cases"]
        .as_array_mut()
        .expect("cases array")
        .push(json!({
            "name":name,
            "canvas":prepared.canvas,
            "layers":prepared.summary().layer_count,
            "passes":prepared.summary().pass_count,
            "script_handles":64,
            "created_layers":63,
            "bars_visible":visible,
            "estimated_gpu_bytes":estimated_gpu_bytes(prepared)?,
        }));
    let mut renderer = SceneRenderer::initialize(prepared)?;
    let target = RenderTarget::new(WIDTH, HEIGHT)?;
    let silent = render_bars(&mut renderer, &target, 0.0, [0.0; 64], visible)?;
    ensure!(
        silent
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] != [0, 0, 0]),
        "Original Yakuza complete scene is unexpectedly blank"
    );
    report.frame(&format!("{name}-original-silence"), &target, &silent)?;
    let ramp = std::array::from_fn(|band| (band + 1) as f32 / 64.0);
    let active = render_bars(&mut renderer, &target, 0.02, ramp, visible)?;
    report.frame(&format!("{name}-original-spectrum"), &target, &active)?;

    // Freeze an actual presented whole scene, queue another input without
    // consuming its output, and ensure pause/forced resize preserve the image.
    let frozen_transforms = bar_transforms(&renderer);
    // Queue through the renderer's tagged protocol so the worker and renderer
    // retain the same sequence, requested time and spectrum while paused.
    renderer.update(renderer.last_input.context("Missing scene clock")? + 0.01)?;
    renderer.last_spectrum = spectrum([0.125; 64]);
    ensure!(
        !renderer.prepare_packet(false)? && renderer.vm_requested,
        "Original Yakuza did not queue its next tagged script tick"
    );
    let frozen_time = renderer.time;
    renderer.pause();
    thread::sleep(Duration::from_millis(20));
    let paused = redraw(&mut renderer, &target, 10.0, [0.0; 64])?;
    report.compare(
        &format!("{name}-pause-preserves-presented-frame"),
        &active,
        &paused,
        false,
    )?;
    ensure!(
        bar_transforms(&renderer) == frozen_transforms,
        "Pausing consumed an unpresented script update"
    );
    ensure!(renderer.time == frozen_time, "Pausing advanced Scene time");
    // Repeat while the producer has time to finish its queued input. A single
    // redraw could pass merely because the worker had not published yet.
    for frame in 0..20 {
        thread::sleep(Duration::from_millis(1));
        let repeated = redraw(
            &mut renderer,
            &target,
            10.1 + f64::from(frame) * 0.01,
            [0.0; 64],
        )?;
        ensure!(
            repeated == active,
            "A later paused redraw consumed asynchronous script output"
        );
        ensure!(
            bar_transforms(&renderer) == frozen_transforms,
            "A later paused redraw changed script transforms"
        );
    }
    let resized_target = RenderTarget::new(1200, 675)?;
    let resized = redraw(&mut renderer, &resized_target, 11.0, [1.0; 64])?;
    report.frame(&format!("{name}-paused-resized"), &resized_target, &resized)?;
    let restored = redraw(&mut renderer, &target, 12.0, [0.0; 64])?;
    report.compare(
        &format!("{name}-paused-resize-back-preserves-pixels"),
        &active,
        &restored,
        false,
    )?;
    ensure!(
        bar_transforms(&renderer) == frozen_transforms,
        "Paused resize changed authored script transforms"
    );

    // The original particles and animated background are deliberately absent
    // from this one comparison, so changed pixels can only come from bars.
    renderer.resume();
    renderer.draw_order.retain(|object| matches!(object, super::super::DrawObject::Image(index) if indices.contains(index)));
    renderer.clear = [0.0, 0.0, 0.0, 1.0];
    let silent_bars = render_bars(&mut renderer, &target, 12.02, [0.0; 64], visible)?;
    ensure!(
        silent_bars
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [0, 0, 0]),
        "Zero-scale bars emitted visible GPU pixels"
    );
    let active_bars = render_bars(&mut renderer, &target, 12.04, ramp, visible)?;
    report.frame(
        &format!("{name}-isolated-bars-silence"),
        &target,
        &silent_bars,
    )?;
    report.frame(
        &format!("{name}-isolated-bars-spectrum"),
        &target,
        &active_bars,
    )?;
    report.compare(
        &format!("{name}-isolated-audio-bars"),
        &silent_bars,
        &active_bars,
        visible,
    )?;
    if !visible {
        ensure!(
            renderer.hidden_layers.len() == 64,
            "av=0 did not hide every script-created bar"
        );
    }
    renderer.pause();
    for index in &indices {
        renderer.layers[*index].tint = [0.0, 0.5, 0.0, 1.0];
    }
    renderer.cache_valid = false;
    let green = redraw(&mut renderer, &target, 13.0, ramp)?;
    report.frame(&format!("{name}-isolated-bars-green-tint"), &target, &green)?;
    ensure!(
        green
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[0] == 0 && pixel[2] == 0),
        "Original genericimage2 card ignored the authored green tint"
    );
    ensure!(
        green.as_chunks::<4>().0.iter().any(|pixel| pixel[1] > 0) == visible,
        "Original card tint visibility does not match its script control"
    );
    for index in &indices {
        renderer.layers[*index].tint[3] = 0.0;
    }
    renderer.cache_valid = false;
    let transparent = redraw(&mut renderer, &target, 14.0, ramp)?;
    report.compare(
        &format!("{name}-original-card-alpha-zero"),
        &silent_bars,
        &transparent,
        false,
    )?;
    for index in &indices {
        renderer.layers[*index].tint[3] = 1.0;
        renderer.hidden_layers.insert(*index);
    }
    renderer.cache_valid = false;
    let hidden = redraw(&mut renderer, &target, 15.0, ramp)?;
    report.compare(
        &format!("{name}-original-card-hidden"),
        &silent_bars,
        &hidden,
        false,
    )?;
    renderer.cleanup();
    ensure!(
        renderer.gpu_payload_bytes() == 0,
        "Yakuza cleanup retained logical GPU payload"
    );
    ensure!(
        renderer.scripts.is_none(),
        "Yakuza cleanup retained its script worker"
    );
    drop(renderer);

    // Repeated initialization/drop uses the same prepared package and current
    // GL context; every generation must create its own ready worker and draw.
    for cycle in 0..2 {
        let mut renderer = SceneRenderer::initialize(prepared)?;
        render_bars(&mut renderer, &target, 0.0, [0.0; 64], visible)?;
        renderer.cleanup();
        ensure!(
            renderer.scripts.is_none() && renderer.gpu_payload_bytes() == 0,
            "Yakuza restart cycle {cycle} retained renderer resources"
        );
    }
    Ok(())
}

fn acceptance(project: &Path, assets: &Path, report: &mut Report) -> Result<()> {
    let properties = PreparedScene::property_definitions(project, Some(assets))?;
    let picture = properties
        .iter()
        .find(|property| property.name == "pic")
        .context("Original Yakuza picture combo is missing")?;
    let audio = properties
        .iter()
        .find(|property| property.name == "av")
        .context("Original Yakuza audio-bar combo is missing")?;
    let super::super::PropertyValue::Combo(default_picture) = &picture.value else {
        bail!("Original Yakuza picture property is not a combo")
    };
    let super::super::PropertyValue::Combo(default_audio) = &audio.value else {
        bail!("Original Yakuza audio-bar property is not a combo")
    };
    ensure!(
        picture.options.len() == 4 && audio.options.len() == 2,
        "Original Yakuza combo declarations changed"
    );
    ensure!(
        default_audio == "av",
        "Original Yakuza default bars are no longer enabled"
    );
    report.value["properties"] = serde_json::to_value(&properties)?;
    let original = PreparedScene::prepare(project, Some(assets))?;
    check_case(&original, "default", true, report)?;
    drop(original);
    for (index, option) in picture.options.iter().enumerate() {
        if &option.value != default_picture {
            let overrides =
                BTreeMap::from([("pic".to_owned(), Value::String(option.value.clone()))]);
            let prepared =
                PreparedScene::prepare_with_properties(project, Some(assets), &overrides)?;
            check_case(&prepared, &format!("picture-{index}"), true, report)?;
        }
    }
    let disabled = audio
        .options
        .iter()
        .find(|option| option.value != "av")
        .context("Original Yakuza disabled-bars option is missing")?;
    let overrides = BTreeMap::from([("av".to_owned(), Value::String(disabled.value.clone()))]);
    let prepared = PreparedScene::prepare_with_properties(project, Some(assets), &overrides)?;
    check_case(&prepared, "bars-disabled", false, report)?;
    report.value["worker_cleanup_cycles"] = json!(15);
    Ok(())
}

fn wait_until(mut predicate: impl FnMut() -> bool, message: &str) -> Result<()> {
    let started = Instant::now();
    while !predicate() {
        ensure!(started.elapsed() < Duration::from_secs(6), "{message}");
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn live_frame(
    renderer: &mut SceneRenderer,
    target: &RenderTarget,
    clock: Instant,
    sound: bool,
) -> Result<(Vec<u8>, SpectrumSnapshot)> {
    let started = Instant::now();
    loop {
        bind(target);
        let drawn = renderer.render_internal(
            target.texture.width,
            target.texture.height,
            clock.elapsed().as_secs_f64(),
            false,
            None,
        )?;
        if drawn {
            renderer.report_presented();
        }
        let captured = renderer.accepted_spectrum;
        let peak = captured
            .script_average
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        let level_matches = if sound { peak > 0.35 } else { peak < 0.0001 };
        let transforms_match = renderer.scripts_ready
            && renderer
                .script_layers
                .values()
                .enumerate()
                .all(|(band, index)| {
                    let layer = &renderer.layers[*index];
                    close(layer.origin[0], 1065.165 + band as f32 * 30.0)
                        && close(layer.scale[0], 5.0)
                        && (layer.scale[1] - captured.script_average[band] * 50.0).abs() < 0.02
                        && !renderer.hidden_layers.contains(index)
                });
        if drawn && level_matches && transforms_match {
            let pixels = read_pixels(target)?;
            let lit = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[..3] != [0, 0, 0]);
            if lit == sound {
                return Ok((pixels, captured));
            }
        }
        ensure!(
            started.elapsed() < Duration::from_secs(6),
            "Private monitor did not drive the expected original 64-bar GPU frame"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn live_audio_acceptance(project: &Path, assets: &Path, report: &mut Report) -> Result<()> {
    use super::super::spectrum::Spectrum;
    use super::super::spectrum::tests::{OwnedPlayback, OwnedSink, owned_capture_sources, pactl};

    let default_sink = pactl(&["get-default-sink"]);
    let default_source = pactl(&["get-default-source"]);
    ensure!(
        owned_capture_sources().is_empty(),
        "Test process already has a Scene monitor capture stream"
    );
    let sink = OwnedSink::new();
    let monitor = format!("{}.monitor", sink.name);
    let sources: Value = serde_json::from_str(&pactl(&["--format=json", "list", "sources"]))?;
    let monitor_index = sources
        .as_array()
        .context("Invalid Pulse sources")?
        .iter()
        .find(|source| source["name"].as_str() == Some(monitor.as_str()))
        .context("Owned null-sink monitor source is missing")?["index"]
        .as_u64()
        .context("Missing private monitor index")? as u32;
    let prepared = PreparedScene::prepare(project, Some(assets))?;
    ensure!(
        prepared.script_layers.len() == 64,
        "Original Yakuza 64-bar plan is missing"
    );
    let mut renderer = SceneRenderer::initialize(&prepared)?;
    let spectrum = Spectrum::private_monitor(monitor);
    let probe = spectrum.capture_probe();
    renderer.spectrum = Some(spectrum);
    renderer.activate_analysis(false);
    let clock = Instant::now();
    let target = RenderTarget::new(WIDTH, HEIGHT)?;
    let indices = renderer
        .script_layers
        .values()
        .copied()
        .collect::<BTreeSet<_>>();
    renderer.draw_order.retain(|object| matches!(object, super::super::DrawObject::Image(index) if indices.contains(index)));
    renderer.clear = [0.0, 0.0, 0.0, 1.0];
    wait_until(|| probe.connected(), "Private monitor did not connect")?;
    ensure!(
        owned_capture_sources() == vec![monitor_index],
        "Scene capture is not routed exclusively to the owned silent sink monitor"
    );
    let (silence, initial_snapshot) = live_frame(&mut renderer, &target, clock, false)?;
    let playback = OwnedPlayback::new(&sink);
    let (active, active_snapshot) = live_frame(&mut renderer, &target, clock, true)?;
    report.frame("live-monitor-silence", &target, &silence)?;
    report.frame("live-monitor-active", &target, &active)?;
    report.compare(
        "real-monitor-64band-worker-drives-original-bars",
        &silence,
        &active,
        true,
    )?;
    ensure!(
        owned_capture_sources() == vec![monitor_index],
        "Active capture changed to a different source"
    );
    let active_transforms = bar_transforms(&renderer);
    renderer.pause();
    ensure!(
        renderer
            .spectrum
            .as_ref()
            .context("No private capture")?
            .snapshot()
            == SpectrumSnapshot::default(),
        "Paused monitor snapshot is not immediately silent"
    );
    wait_until(
        || !probe.connected() && owned_capture_sources().is_empty(),
        "Paused private capture did not disconnect",
    )?;
    let paused = redraw(
        &mut renderer,
        &target,
        clock.elapsed().as_secs_f64(),
        [0.0; 64],
    )?;
    report.compare(
        "real-monitor-pause-retains-presented-bars",
        &active,
        &paused,
        false,
    )?;
    ensure!(
        bar_transforms(&renderer) == active_transforms,
        "Pausing changed original bar transforms"
    );
    renderer.resume();
    let (resumed, resumed_snapshot) = live_frame(&mut renderer, &target, clock, true)?;
    report.frame("live-monitor-resumed", &target, &resumed)?;
    report.compare(
        "real-monitor-resume-restores-responsive-bars",
        &silence,
        &resumed,
        true,
    )?;
    ensure!(
        probe.connected() && owned_capture_sources() == vec![monitor_index],
        "Resumed capture did not reconnect to the private monitor"
    );
    drop(playback);
    let (released, silent_snapshot) = live_frame(&mut renderer, &target, clock, false)?;
    report.frame("live-monitor-silence-released", &target, &released)?;
    report.compare(
        "real-monitor-stopped-playback-releases-bars",
        &silence,
        &released,
        false,
    )?;
    renderer.cleanup();
    ensure!(
        renderer.scripts.is_none()
            && renderer.spectrum.is_none()
            && renderer.gpu_payload_bytes() == 0,
        "Live Yakuza cleanup retained script, capture or GPU resources"
    );
    ensure!(
        probe.released() && !probe.connected(),
        "Private monitor capture worker ownership was retained after cleanup"
    );
    wait_until(
        || owned_capture_sources().is_empty(),
        "Cleanup retained a Pulse capture stream",
    )?;
    drop(renderer);
    drop(sink);
    ensure!(
        pactl(&["get-default-sink"]) == default_sink
            && pactl(&["get-default-source"]) == default_source,
        "Private monitor test changed the default audio devices"
    );
    report.value["capture"] = json!({
        "script_bands":64,
        "initial_average":initial_snapshot.script_average.to_vec(),
        "active_average":active_snapshot.script_average.to_vec(),
        "resumed_average":resumed_snapshot.script_average.to_vec(),
        "released_average":silent_snapshot.script_average.to_vec(),
        "original_bar_transforms_from_real_capture":active_transforms.iter().map(|bar| json!({"origin":bar.origin,"scale":bar.scale})).collect::<Vec<_>>(),
        "source_was_owned_sink_monitor":true,
        "speaker_output":false,
        "pause_disconnected_capture":true,
        "resume_reconnected_capture":true,
        "silence_released_original_bars":true,
        "capture_worker_joined":true,
        "script_worker_released":true,
        "owned_sink_unloaded":true,
        "default_sink_unchanged":true,
        "default_source_unchanged":true,
    });
    Ok(())
}

fn offscreen(report: &mut Report, run: impl FnOnce(&mut Report) -> Result<()>) -> Result<()> {
    use khronos_egl as egl;
    let instance = unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required()? };
    let display = unsafe { instance.get_display(egl::DEFAULT_DISPLAY) }
        .context("No offscreen EGL display")?;
    instance.initialize(display)?;
    instance.bind_api(egl::OPENGL_API)?;
    let config = instance
        .choose_first_config(
            display,
            &[
                egl::SURFACE_TYPE,
                egl::PBUFFER_BIT,
                egl::RENDERABLE_TYPE,
                egl::OPENGL_BIT,
                egl::RED_SIZE,
                8,
                egl::GREEN_SIZE,
                8,
                egl::BLUE_SIZE,
                8,
                egl::ALPHA_SIZE,
                8,
                egl::NONE,
            ],
        )?
        .context("No EGL pbuffer configuration")?;
    let context = instance.create_context(
        display,
        config,
        None,
        &[
            egl::CONTEXT_MAJOR_VERSION,
            3,
            egl::CONTEXT_MINOR_VERSION,
            0,
            egl::NONE,
        ],
    )?;
    let surface = instance.create_pbuffer_surface(
        display,
        config,
        &[egl::WIDTH, 2, egl::HEIGHT, 2, egl::NONE],
    )?;
    instance.make_current(display, Some(surface), Some(surface), Some(context))?;
    gl::load_with(|name| {
        instance
            .get_proc_address(name)
            .map(|function| function as *const std::ffi::c_void)
            .unwrap_or(std::ptr::null())
    });
    let driver_string = |name| -> Result<String> {
        let raw = unsafe { gl::GetString(name) };
        ensure!(!raw.is_null(), "OpenGL driver string is unavailable");
        Ok(unsafe { std::ffi::CStr::from_ptr(raw.cast()) }
            .to_string_lossy()
            .into_owned())
    };
    report.value["gl_driver"] = json!({
        "vendor":driver_string(gl::VENDOR)?,
        "renderer":driver_string(gl::RENDERER)?,
        "version":driver_string(gl::VERSION)?,
    });
    let result = run(report);
    let save = report.save(&result);
    instance.make_current(display, None, None, None)?;
    instance.destroy_surface(display, surface)?;
    instance.destroy_context(display, context)?;
    instance.terminate(display)?;
    save?;
    result
}

#[test]
#[ignore = "requires explicit original Yakuza package and EGL/OpenGL; offscreen GPU pixels only"]
fn original_yakuza_picture_combos_script_bars_and_paused_frames() -> Result<()> {
    let project =
        PathBuf::from(std::env::var_os("LWE_YAKUZA_PROJECT").context("Set LWE_YAKUZA_PROJECT")?);
    let assets =
        PathBuf::from(std::env::var_os("LWE_YAKUZA_ASSETS").context("Set LWE_YAKUZA_ASSETS")?);
    let mut report = Report::new()?;
    offscreen(&mut report, |report| acceptance(&project, &assets, report))
}

#[test]
#[ignore = "requires LWE_REAL_DESKTOP_TESTS=1, original Yakuza assets, EGL, PulseAudio/PipeWire, pactl and paplay; private silent monitor only"]
fn original_yakuza_private_monitor_drives_script_bars_and_gpu_pixels() -> Result<()> {
    ensure!(
        std::env::var("LWE_REAL_DESKTOP_TESTS").as_deref() == Ok("1"),
        "Set LWE_REAL_DESKTOP_TESTS=1 for private live-audio GPU acceptance"
    );
    let project =
        PathBuf::from(std::env::var_os("LWE_YAKUZA_PROJECT").context("Set LWE_YAKUZA_PROJECT")?);
    let assets =
        PathBuf::from(std::env::var_os("LWE_YAKUZA_ASSETS").context("Set LWE_YAKUZA_ASSETS")?);
    let mut report = Report::live_audio()?;
    offscreen(&mut report, |report| {
        live_audio_acceptance(&project, &assets, report)
    })
}
