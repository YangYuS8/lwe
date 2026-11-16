//! Opt-in original default graph, immutable source identities and actual pixels.
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hashes(project: &std::path::Path) -> Result<serde_json::Value> {
    Ok(
        json!({"project.json":hash(&std::fs::read(project.join("project.json"))?),"scene.pkg":hash(&std::fs::read(project.join("scene.pkg"))?)}),
    )
}
fn read(target: &RenderTarget) -> Result<Vec<u8>> {
    let mut pixels = vec![0; (target.texture.width * target.texture.height * 4) as usize];
    unsafe {
        gl::BindFramebuffer(gl::READ_FRAMEBUFFER, target.fbo);
        gl::ReadBuffer(gl::COLOR_ATTACHMENT0);
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
    check_gl("reading original animated Scene pixels")?;
    Ok(pixels)
}
fn draw(
    renderer: &mut SceneRenderer,
    target: &RenderTarget,
    time: f64,
    force: bool,
) -> Result<Vec<u8>> {
    let started = Instant::now();
    loop {
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target.fbo);
        }
        let drawn = if force {
            renderer.render_forced(target.texture.width, target.texture.height, time)?
        } else {
            let mut spectrum = super::super::spectrum::SpectrumSnapshot::default();
            spectrum.script_average[0] = 0.8;
            spectrum.script_average[63] = 0.8;
            spectrum.left.fill(0.5);
            spectrum.right.fill(0.5);
            spectrum.left32.fill(0.5);
            spectrum.right32.fill(0.5);
            spectrum.script_left.fill(0.5);
            spectrum.script_right.fill(0.5);
            renderer.render_internal(
                target.texture.width,
                target.texture.height,
                time,
                false,
                Some(spectrum),
            )?
        };
        if drawn {
            renderer.report_presented();
            return read(target);
        }
        ensure!(
            started.elapsed() < Duration::from_secs(20),
            "Original complete frame did not become ready"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn driver(name: u32) -> String {
    unsafe {
        let ptr = gl::GetString(name);
        if ptr.is_null() {
            return "unavailable".into();
        }
        std::ffi::CStr::from_ptr(ptr.cast())
            .to_string_lossy()
            .into_owned()
    }
}
#[test]
#[ignore = "requires explicit original animated Scene assets and opt-in surfaceless software EGL; no desktop backend"]
fn original_miku_default_eight_frames_three_histories_and_cached_rollback() -> Result<()> {
    let project = PathBuf::from(
        std::env::var_os("LWE_MIKU_TEST_PROJECT").context("Set LWE_MIKU_TEST_PROJECT")?,
    );
    let assets = PathBuf::from(
        std::env::var_os("LWE_SCENE_TEST_ASSETS").context("Set LWE_SCENE_TEST_ASSETS")?,
    );
    let report =
        PathBuf::from(std::env::var_os("LWE_MIKU_GL_REPORT").context("Set LWE_MIKU_GL_REPORT")?);
    ensure!(report.is_absolute(), "Private report path must be absolute");
    std::fs::create_dir_all(report.parent().context("Report parent unavailable")?)?;
    let before = hashes(&project)?;
    let prepared = PreparedScene::prepare(&project, Some(&assets))?;
    ensure!(
        prepared.summary().animated,
        "Timed scene is not scheduled as animated"
    );
    let timed = prepared
        .textures
        .iter()
        .filter_map(|t| t.timed.as_ref())
        .collect::<Vec<_>>();
    ensure!(
        timed.len() == 2 && timed.iter().all(|t| t.frames().len() == 8),
        "Original timed source graph changed"
    );
    let sources=timed.iter().map(|s|json!({"encoded_sha256":format!("{:x}",Sha256::digest(s.encoded_bytes())),"encoded_capacity":s.encoded_capacity(),"raw_capacity":s.raw_capacity(),"framing_capacity":s.retained_capacity_bytes()-s.encoded_capacity()-s.raw_capacity(),"retained_capacity":s.retained_capacity_bytes(),"image_info_extra":s.metadata().image_info_extra,"frames":s.frames().iter().map(|f|json!({"rect":[f.rect.x,f.rect.y,f.rect.width,f.rect.height],"duration":f.duration_seconds})).collect::<Vec<_>>() })).collect::<Vec<_>>();
    let _context = super::sharing_tests::Offscreen::new()?;
    let driver = json!({"vendor":driver(gl::VENDOR),"renderer":driver(gl::RENDERER),"version":driver(gl::VERSION)});
    let pool = TexturePool::new()?;
    let output = RenderTarget::new(2160, 1440)?;
    let output_plan = prepared.resource_plan_for_output(2160, 1440)?;
    output_plan.preflight(0, 0)?;
    let mut renderer = SceneRenderer::initialize_shared(&prepared, Rc::clone(&pool))?;
    let late_enabled = std::env::var("LWE_MIKU_LATE_COMPOSITE").as_deref() == Ok("1");
    renderer.set_canvas_replacement_allowed(late_enabled);

    let history_ids = renderer
        .layers
        .iter()
        .flat_map(|l| &l.stages)
        .flat_map(|s| s.persistent.iter().map(|name| s.targets[name].texture.id))
        .collect::<std::collections::BTreeSet<_>>();
    ensure!(
        history_ids.len() == 3,
        "Original independent history graph changed"
    );
    // VM initialization is deliberately silent/time zero. Consume it first,
    // then a tagged CPU spectrum makes both authored timed layers visible.
    draw(&mut renderer, &output, 0., false)?;
    renderer.update(0.)?;
    let duration = timed[0].frames()[0].duration_seconds;
    let mut frames = Vec::new();
    let mut last = Vec::new();
    for index in 0..8 {
        let time = (index as f64 + 0.25) * duration;
        let before_late = renderer.canvas_replacements;
        let pixels = draw(&mut renderer, &output, time, false)?;
        let late_used = renderer.canvas_replacements == before_late + 1;
        ensure!(
            late_used == late_enabled,
            "Private late Color28 branch did not match opt-in at frame {index}"
        );

        let timed_alphas = prepared.layers.iter().enumerate().filter(|(_, l)| l.stages.iter().flat_map(|s| &s.passes).flat_map(|p| &p.textures).any(|b| matches!(b,TextureBinding::Static(i) if prepared.textures[*i].timed.is_some()))).map(|(i,_)| renderer.layers[i].tint[3]).collect::<Vec<_>>();
        ensure!(
            timed_alphas.len() == 2 && timed_alphas.iter().all(|a| *a > 0.),
            "Original animated layers remained hidden: {timed_alphas:?}"
        );
        let selected = renderer
            .textures
            .iter()
            .filter(|v| v.timed)
            .map(|v| v.frame)
            .collect::<Vec<_>>();
        ensure!(
            selected == vec![Some(index), Some(index)],
            "Selected timed frame does not match accepted time: {selected:?}"
        );
        let mut histories = Vec::new();
        for (layer_index, layer) in renderer.layers.iter().enumerate() {
            for (stage_index, stage) in layer.stages.iter().enumerate() {
                for name in &stage.persistent {
                    let target = &stage.targets[name];
                    let pixels = read(target)?;
                    histories.push(json!({"layer":layer_index,"stage":stage_index,"target":name,"dimensions":[target.texture.width,target.texture.height],"pixel_sha256":hash(&pixels),"nontransparent_pixels":pixels.as_chunks::<4>().0.iter().filter(|p| p[3]>0).count()}));
                }
            }
        }
        ensure!(
            histories.len() == 3
                && histories
                    .iter()
                    .all(|h| h["nontransparent_pixels"].as_u64().unwrap() > 0),
            "Original histories contain no visible accumulated input"
        );
        frames.push(json!({"seq":renderer.accepted_seq,"scene_time":renderer.accepted_time,"selected":selected,"timed_alphas":timed_alphas,"histories":histories,"pixel_sha256":hash(&pixels),"gpu_reserved_bytes":renderer.gpu_payload_bytes(),"pool_resident_bytes":pool.resident_bytes()?,"private_late_color28_used":late_used}));
        ensure!(
            renderer.gpu_payload_bytes() <= MAX_SCENE_GPU_BYTES,
            "Full default graph exceeded unchanged cap"
        );
        last = pixels;
    }
    ensure!(
        frames
            .iter()
            .map(|f| f["pixel_sha256"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1,
        "Original timed output never changed"
    );
    renderer.pause();
    let paused = draw(&mut renderer, &output, 8., true)?;
    ensure!(paused == last, "Paused redraw changed accepted output");
    renderer.freeze_for_rollback()?;
    let frozen = renderer.allocation_plan()?;
    ensure!(
        frozen.work.is_empty()
            && renderer
                .textures
                .iter()
                .filter(|t| t.timed)
                .all(|t| t.clamp.is_none()),
        "Frozen generation retained timed inputs/work"
    );
    std::fs::write(
        &report,
        serde_json::to_vec_pretty(
            &json!({"status":"partial: original eight visible frames/pause passed; transition preflight pending", "sources":sources,"frames":frames,"source_before":before,"source_now":hashes(&project)?,"driver":driver,"history_count":history_ids.len(),"current_gpu_reserved_bytes":renderer.gpu_payload_bytes(),"frozen_gpu_reserved_bytes":frozen.total(0)?,"cpu_usage":format!("{:?}",super::super::timed_runtime::scene_texture_memory_budget().usage())}),
        )?,
    )?;
    let candidate_prepared = PreparedScene::prepare(&project, Some(&assets))?;
    let mut transition = frozen.clone();
    transition.merge(&candidate_prepared.resource_plan_for_output(2160, 1440)?)?;
    let transition_total = transition.total(pool.retired_bytes()?)?;
    let frozen_history_cpu_bytes = renderer
        .frozen_histories
        .values()
        .map(|h| h.capacity_bytes())
        .sum::<usize>();
    let legacy_gpu_history_transition_bytes = transition_total
        .checked_add(renderer.history_reservation)
        .context("Legacy transition estimate overflow")?;
    eprintln!(
        "original current/candidate union={transition_total}, frozen={}, candidate={} work={:?}",
        frozen.total(0)?,
        candidate_prepared
            .resource_plan_for_output(2160, 1440)?
            .total(0)?,
        transition.work
    );
    transition
        .preflight(pool.retired_bytes()?, 0)
        .with_context(|| format!("Original transition union {transition_total} bytes"))?;
    let mut candidate = SceneRenderer::initialize_shared(&candidate_prepared, Rc::clone(&pool))?;
    let candidate_pixels = draw(&mut candidate, &output, 0., false)?;
    ensure!(
        !candidate_pixels.is_empty(),
        "Candidate did not complete first frame"
    );
    let restored = draw(&mut renderer, &output, 9., true)?;
    ensure!(
        restored == last,
        "Candidate overwrote frozen rollback pixels"
    );
    let transition_bytes = transition.total(0)?;
    candidate.cleanup();
    drop(candidate);
    drop(candidate_prepared);
    renderer.thaw(false);
    let resumed = draw(&mut renderer, &output, 9., false)?;
    ensure!(!resumed.is_empty(), "Restored generation did not rehydrate");
    let cpu = super::super::timed_runtime::scene_texture_memory_budget().usage();
    renderer.cleanup();
    let late_swaps = renderer.canvas_replacements;
    drop(renderer);
    pool.drain_retired()?;
    ensure!(
        pool.resident_bytes()? == 0,
        "Original cleanup retained GPU leases"
    );
    let summary = prepared.summary();
    let graph=prepared.layers.iter().enumerate().map(|(i,l)| json!({"layer":i,"size":l.size,"stages":l.stages.iter().map(|s|json!({"targets":s.targets,"persistent_targets":s.persistent_targets,"passes":s.passes.iter().map(|p|json!({"label":p.label,"kind":format!("{:?}",p.kind),"target":p.target})).collect::<Vec<_>>()})).collect::<Vec<_>>()})).collect::<Vec<_>>();
    drop(prepared);
    let cleaned = super::super::timed_runtime::scene_texture_memory_budget().usage();
    let after = hashes(&project)?;
    ensure!(before == after, "Original assets changed during test");
    let value = json!({"private_late_color28":{"enabled":late_enabled,"swaps":late_swaps},"scope":"Original full-default CPU graph and software EGL offscreen pixels; no desktop application, hardware GPU, Windows renderer parity or physical sleep claim","summary":{"layers":summary.layer_count,"effects":summary.effect_count,"passes":summary.pass_count},"graph":graph,"cpu_ledger_scope":"production reservations; excludes test-only output/history readback Vecs, allocator overhead and RSS", "source_before":before,"source_after":after,"driver":driver,"sources":sources,"history_count":history_ids.len(),"frames":frames,"paused_exact":true,"frozen_rollback_exact":true,"transition_reserved_bytes":transition_bytes,"frozen_history_cpu_bytes":frozen_history_cpu_bytes,"legacy_gpu_history_transition_bytes":legacy_gpu_history_transition_bytes,"gpu_limit_bytes":MAX_SCENE_GPU_BYTES,"cpu_charged_bytes":cpu.charged_bytes,"cpu_peak_reserved_bytes":cpu.peak_charged_bytes,"cpu_after_cleanup":cleaned.charged_bytes,"pool_after_cleanup":pool.resident_bytes()?});
    std::fs::write(report, serde_json::to_vec_pretty(&value)?)?;
    Ok(())
}

// These helpers are opt-in evidence only. Playback owns its child process;
// neither the host server nor its device defaults are modified.
struct PrivatePlayback(std::process::Child);
impl PrivatePlayback {
    fn new(path: &std::path::Path, sink: &str) -> Result<Self> {
        Ok(Self(
            std::process::Command::new("paplay")
                .arg(format!("--device={sink}"))
                .arg(path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .context("Start owned PCM on the private sink")?,
        ))
    }
}
impl Drop for PrivatePlayback {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct AlphaOracle {
    handle: usize,
    bands: usize,
    frequency: usize,
    smoothing: f64,
    minimum: f64,
    maximum: f64,
    initial: f64,
    smooth: f64,
}
struct CapturedFrames {
    previous_time: f64,
    alphas: Vec<AlphaOracle>,
    trace: Vec<serde_json::Value>,
}
impl CapturedFrames {
    fn original(path: &std::path::Path) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let alphas = value["scripts"]
            .as_array()
            .context("Original binding evidence is missing")?
            .iter()
            .filter(|b| b["path"].as_str().is_some_and(|p| p.ends_with("/alpha")))
            .map(|b| {
                let path = b["path"].as_str().unwrap();
                let handle = path
                    .split('/')
                    .nth(2)
                    .context("Missing object index")?
                    .parse::<usize>()?
                    + 1;
                let wrapper = &b["wrapper"];
                let p = &wrapper["scriptproperties"];
                let source = wrapper["script"]
                    .as_str()
                    .context("Missing authored script")?;
                let bands = if source.contains("AUDIO_RESOLUTION_16") {
                    16
                } else {
                    64
                };
                Ok(AlphaOracle {
                    handle,
                    bands,
                    frequency: p["frequency"].as_u64().context("Missing frequency")? as usize,
                    smoothing: p["smoothing"].as_f64().context("Missing smoothing")?,
                    minimum: p["minvalue"].as_f64().context("Missing minimum")?,
                    maximum: p["maxvalue"].as_f64().context("Missing maximum")?,
                    initial: wrapper["value"].as_f64().context("Missing initial alpha")?,
                    smooth: 0.,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            alphas.len() == 4
                && alphas.iter().any(|a| a.bands == 16)
                && alphas.iter().any(|a| a.bands == 64),
            "Original 16/64 alpha binding oracle changed"
        );
        Ok(Self {
            previous_time: 0.,
            alphas,
            trace: Vec::new(),
        })
    }
    fn record(&mut self, renderer: &SceneRenderer, pixels: &[u8]) -> Result<()> {
        let dt = renderer.accepted_time - self.previous_time;
        let audio = renderer.accepted_spectrum;
        let mut alphas = Vec::new();
        for oracle in &mut self.alphas {
            let input = if oracle.bands == 16 {
                (audio.left[oracle.frequency] + audio.right[oracle.frequency]) * 0.5
            } else {
                audio.script_average[oracle.frequency]
            };
            // Original JS operates in f64 and returns an f32 LayerState alpha.
            if renderer.accepted_seq > 0 {
                oracle.smooth = (oracle.smooth
                    + (f64::from(input) - oracle.smooth) * (dt * oracle.smoothing).min(1.))
                .min(1.);
            }
            let expected = (oracle.initial
                * (oracle.smooth * (oracle.maximum - oracle.minimum) + oracle.minimum))
                as f32;
            let layer = *renderer
                .script_layers
                .get(&oracle.handle)
                .context("Original script layer missing")?;
            let actual = renderer.layers[layer].tint[3];
            ensure!(
                (actual - expected).abs() < 0.00002,
                "Captured PCM/VM mismatch handle{} seq{} time{} dt{} input{} alpha{} expected{}",
                oracle.handle,
                renderer.accepted_seq,
                renderer.accepted_time,
                dt,
                input,
                actual,
                expected
            );
            alphas.push(json!({"handle":oracle.handle,"bands":oracle.bands,"frequency":oracle.frequency,"pcm_average":input,"expected_alpha":expected,"actual_alpha":actual}));
        }
        self.previous_time = renderer.accepted_time;
        self.trace.push(json!({"seq":renderer.accepted_seq,"time":renderer.accepted_time,"delta":dt,"left16":audio.left.to_vec(),"right16":audio.right.to_vec(),"left32":audio.left32.to_vec(),"right32":audio.right32.to_vec(),"left64":audio.script_left.to_vec(),"right64":audio.script_right.to_vec(),"average64":audio.script_average.to_vec(),"alphas":alphas,"pixel_sha256":hash(pixels)}));
        Ok(())
    }
}
fn captured_frame(
    renderer: &mut SceneRenderer,
    target: &RenderTarget,
    clock: Instant,
    frames: &mut CapturedFrames,
) -> Result<Vec<u8>> {
    let started = Instant::now();
    loop {
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target.fbo);
        }
        if renderer.render(
            target.texture.width,
            target.texture.height,
            clock.elapsed().as_secs_f64(),
        )? {
            renderer.report_presented();
            let pixels = read(target)?;
            frames.record(renderer, &pixels)?;
            return Ok(pixels);
        }
        ensure!(
            started.elapsed() < Duration::from_secs(20),
            "Original captured frame did not become ready"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn wait_capture(mut ready: impl FnMut() -> bool, description: &str) -> Result<()> {
    let start = Instant::now();
    while !ready() {
        ensure!(start.elapsed() < Duration::from_secs(6), "{description}");
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
fn captured_shader_probe(renderer: &SceneRenderer) -> Result<serde_json::Value> {
    let (stage, pass) = renderer
        .layers
        .iter()
        .flat_map(|l| &l.stages)
        .flat_map(|s| s.passes.iter().map(move |p| (s, p)))
        .find(|(_, p)| p.label.contains("Simple_Audio_Bars"))
        .context("Original SimpleBars pass missing")?;
    let program = pass
        .program
        .as_ref()
        .context("Original SimpleBars program missing")?;
    let input = renderer
        .black
        .as_ref()
        .context("Original renderer cleaned up")?;
    let inputs = TextureInputs {
        textures: &renderer.textures,
        black: input,
        full_frame: Some(input),
        replay: false,
    };
    let resolved = pass
        .textures
        .iter()
        .enumerate()
        .map(|(slot, binding)| {
            resolve_binding(binding, stage, Some(input), &inputs, &pass.label, slot)
        })
        .collect::<Result<Vec<_>>>()?;
    let target = RenderTarget::new(384, 216)?;
    let state = GlState::capture();
    state.begin();
    let draw_probe = |audio: &super::super::spectrum::SpectrumSnapshot| -> Result<Vec<u8>> {
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target.fbo);
            gl::Viewport(0, 0, 384, 216);
            gl::ClearColor(0., 0., 0., 0.);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::BindVertexArray(renderer.vao);
            gl::Disable(gl::BLEND);
        }
        bind_pass(program, pass, &resolved, 0., audio, [1.; 4])?;
        program.matrix(&IDENTITY);
        unsafe {
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
        }
        check_gl("original SimpleBars fixed-input capture probe")?;
        read(&target)
    };
    let silent = draw_probe(&Default::default())?;
    let active = draw_probe(&renderer.accepted_spectrum)?;
    let mut uniforms = Vec::new();
    for (name, values) in [
        (
            "g_AudioSpectrum16Left",
            renderer.accepted_spectrum.left.as_slice(),
        ),
        (
            "g_AudioSpectrum16Right",
            renderer.accepted_spectrum.right.as_slice(),
        ),
        (
            "g_AudioSpectrum32Left",
            renderer.accepted_spectrum.left32.as_slice(),
        ),
        (
            "g_AudioSpectrum32Right",
            renderer.accepted_spectrum.right32.as_slice(),
        ),
        (
            "g_AudioSpectrum64Left",
            renderer.accepted_spectrum.script_left.as_slice(),
        ),
        (
            "g_AudioSpectrum64Right",
            renderer.accepted_spectrum.script_right.as_slice(),
        ),
    ] {
        if let Some(count) = program.array_lengths.get(name) {
            let mut actual = Vec::new();
            for (i, expected) in values.iter().take(*count as usize).enumerate() {
                let name = CString::new(format!("{name}[{i}]"))?;
                let location = unsafe { gl::GetUniformLocation(program.id, name.as_ptr()) };
                let mut value = 0.;
                unsafe {
                    gl::GetUniformfv(program.id, location, &mut value);
                }
                ensure!(
                    (value - expected).abs() < 0.000001,
                    "Original shader uniform did not use accepted PCM"
                );
                actual.push(value);
            }
            uniforms.push(json!({"name":name,"actual":actual}));
        }
    }
    check_gl("querying captured original shader uniforms")?;
    let changed = silent
        .as_chunks::<4>()
        .0
        .iter()
        .zip(active.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    ensure!(
        changed > 0 && !uniforms.is_empty(),
        "Real PCM did not change original SimpleBars GPU pixels at fixed input/time"
    );
    Ok(
        json!({"scope":"original unchanged SimpleBars shader, fixed black input/time/geometry; no effect omitted from the separate full-default draw","uniforms":uniforms,"silent_sha256":hash(&silent),"active_sha256":hash(&active),"changed_pixels":changed}),
    )
}

#[test]
#[ignore = "requires explicit private Pulse server/sink, original assets and opt-in surfaceless EGL; no physical output"]
fn original_miku_private_capture_drives_16_64_vm_and_original_shader_pixels() -> Result<()> {
    use super::super::spectrum::tests::{owned_capture_sources, pactl};
    ensure!(
        std::env::var("LWE_MIKU_PRIVATE_CAPTURE").as_deref() == Ok("1"),
        "Private capture opt-in required"
    );
    let server = std::env::var("PULSE_SERVER").context("Set the owned private PULSE_SERVER")?;
    ensure!(
        server.starts_with("unix:/tmp/lwe-miku-audio-"),
        "Capture requires the isolated owned audio server"
    );
    let sink = std::env::var("LWE_MIKU_AUDIO_SINK").context("Set private audio sink")?;
    ensure!(
        sink == "lwe-miku-private" && pactl(&["get-default-sink"]) == sink,
        "Only the owned silent default sink is allowed"
    );
    let source_default = pactl(&["get-default-source"]);
    let monitor = format!("{sink}.monitor");
    let sources: serde_json::Value =
        serde_json::from_str(&pactl(&["--format=json", "list", "sources"]))?;
    let monitor_index = sources
        .as_array()
        .context("Pulse source list malformed")?
        .iter()
        .find(|s| s["name"].as_str() == Some(monitor.as_str()))
        .context("Private monitor missing")?["index"]
        .as_u64()
        .context("Private monitor index missing")? as u32;
    ensure!(
        owned_capture_sources().is_empty(),
        "This process already owns a capture stream"
    );
    let project =
        PathBuf::from(std::env::var_os("LWE_MIKU_TEST_PROJECT").context("Set original project")?);
    let assets =
        PathBuf::from(std::env::var_os("LWE_SCENE_TEST_ASSETS").context("Set builtin assets")?);
    let report = PathBuf::from(
        std::env::var_os("LWE_MIKU_CAPTURE_REPORT").context("Set private capture report")?,
    );
    let wave =
        PathBuf::from(std::env::var_os("LWE_MIKU_AUDIO_WAV").context("Set owned stereo PCM")?);
    let bindings = PathBuf::from(
        std::env::var_os("LWE_ORIGINAL_SCRIPT_BINDINGS").context("Set original wrapper oracle")?,
    );
    ensure!(
        report.is_absolute() && wave.is_absolute(),
        "Private evidence paths must be absolute"
    );
    let before = hashes(&project)?;
    let prepared = PreparedScene::prepare(&project, Some(&assets))?;
    ensure!(
        prepared.summary().layer_count == 10
            && prepared.summary().effect_count == 5
            && prepared.summary().pass_count == 25,
        "Original full default graph changed"
    );
    let _context = super::sharing_tests::Offscreen::new()?;
    let driver = json!({"vendor":driver(gl::VENDOR),"renderer":driver(gl::RENDERER),"version":driver(gl::VERSION)});
    let pool = TexturePool::new()?;
    let output = RenderTarget::new(2160, 1440)?;
    let mut renderer = SceneRenderer::initialize_shared(&prepared, Rc::clone(&pool))?;
    let probe = renderer
        .spectrum
        .as_ref()
        .context("Original graph has no capture consumer")?
        .capture_probe();
    renderer.activate_analysis(false);
    wait_capture(
        || probe.connected() && owned_capture_sources() == vec![monitor_index],
        "Production default capture did not connect exclusively to private monitor",
    )?;
    let mut frames = CapturedFrames::original(&bindings)?;
    let clock = Instant::now();
    let silence = captured_frame(&mut renderer, &output, clock, &mut frames)?;
    let playback = PrivatePlayback::new(&wave, &sink)?;
    let started = Instant::now();
    let active = loop {
        let pixels = captured_frame(&mut renderer, &output, clock, &mut frames)?;
        if renderer.accepted_time > 0.001
            && renderer.accepted_spectrum.script_average[0] > 0.1
            && renderer.accepted_spectrum.script_average[63] > 0.1
        {
            break pixels;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(15),
            "Private bass/treble did not reach original 64-band VM"
        );
    };
    ensure!(
        active != silence,
        "Whole original graph did not change with real captured input"
    );
    let timed_alphas = frames.trace.last().unwrap()["alphas"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["frequency"] == 0 || a["frequency"] == 63)
        .map(|a| a["actual_alpha"].as_f64().unwrap())
        .collect::<Vec<_>>();
    ensure!(
        timed_alphas.len() == 2 && timed_alphas.iter().all(|a| *a > 0.),
        "Real PCM left original timed layers invisible"
    );
    let shader_probe = captured_shader_probe(&renderer)?;
    // A separate owned 750Hz PCM excites the author's 16-band frequency7.
    let mid_path = report.with_file_name("owned-audio16-band7.wav");
    let payload = 48000u32 * 8 * 4;
    let mut bytes = Vec::with_capacity(payload as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(payload + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&48000u32.to_le_bytes());
    bytes.extend_from_slice(&192000u32.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&payload.to_le_bytes());
    for i in 0..48000 * 8 {
        let sample = ((std::f64::consts::TAU * 750. * i as f64 / 48000.).sin()
            * 0.3
            * f64::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&sample.to_le_bytes());
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(&mid_path, &bytes)?;
    let mid_hash = hash(&bytes);
    drop(bytes);
    let mid = PrivatePlayback::new(&mid_path, &sink)?;
    let started = Instant::now();
    let active16 = loop {
        let pixels = captured_frame(&mut renderer, &output, clock, &mut frames)?;
        let row = frames.trace.last().unwrap()["alphas"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["bands"] == 16)
            .unwrap();
        if row["pcm_average"].as_f64().unwrap() > 0.1
            && row["actual_alpha"].as_f64().unwrap() > 0.21
        {
            break pixels;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(12),
            "Private mid tone did not drive original 16-band alpha"
        );
    };
    renderer.pause();
    wait_capture(
        || !probe.connected() && owned_capture_sources().is_empty(),
        "Paused capture did not disconnect",
    )?;
    ensure!(
        draw(&mut renderer, &output, clock.elapsed().as_secs_f64(), true)? == active16,
        "Paused full-default pixels changed with disconnected capture"
    );
    renderer.resume();
    wait_capture(
        || probe.connected() && owned_capture_sources() == vec![monitor_index],
        "Resumed private capture did not reconnect",
    )?;
    captured_frame(&mut renderer, &output, clock, &mut frames)?;
    drop((playback, mid));
    let started = Instant::now();
    loop {
        captured_frame(&mut renderer, &output, clock, &mut frames)?;
        if renderer
            .accepted_spectrum
            .script_average
            .iter()
            .chain(&renderer.accepted_spectrum.left)
            .chain(&renderer.accepted_spectrum.right)
            .all(|v| *v < 0.01)
        {
            break;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(20),
            "Stopped owned PCM did not release captured spectrum"
        );
    }
    renderer.cleanup();
    drop(renderer);
    drop(prepared);
    wait_capture(
        || probe.released() && owned_capture_sources().is_empty(),
        "Cleanup retained private capture worker",
    )?;
    wait_capture(
        || {
            super::super::timed_runtime::scene_texture_memory_budget()
                .usage()
                .charged_bytes
                == 0
        },
        "Cleanup retained CPU frame worker resources",
    )?;
    pool.drain_retired()?;
    ensure!(
        pool.resident_bytes()? == 0
            && super::super::timed_runtime::scene_texture_memory_budget()
                .usage()
                .charged_bytes
                == 0,
        "Capture cleanup retained CPU/GPU resource owners"
    );
    ensure!(
        pactl(&["get-default-sink"]) == sink && pactl(&["get-default-source"]) == source_default,
        "Private default devices changed"
    );
    let after = hashes(&project)?;
    ensure!(
        before == after,
        "Original project changed during live capture"
    );
    std::fs::write(
        report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"scope":"owned private Pulse PCM -> production default capture -> accepted 16/32/64 spectrum -> unchanged original 16/64 VM alpha and full default graph/three histories; separate fixed-input original shader causal probe; no physical output/Windows parity/suspend","driver":driver,"source_before":before,"source_after":after,"pcm_sha256":hash(&std::fs::read(wave)?),"mid_pcm_sha256":mid_hash,"monitor_index":monitor_index,"trace":frames.trace,"shader_probe":shader_probe,"paused_pixels_exact":true,"pause_disconnected":true,"resume_reconnected":true,"capture_cleanup":true,"defaults_unchanged":true,"pool_after_cleanup":pool.resident_bytes()?}),
        )?,
    )?;
    Ok(())
}

#[test]
#[ignore = "requires private original Miku assets and explicit CPU-only option oracle; no EGL/backend"]
fn original_miku_static_and_poof_options_keep_authored_visibility_and_scripts() -> Result<()> {
    ensure!(
        std::env::var("LWE_MIKU_OPTIONS_ORACLE").as_deref() == Ok("1"),
        "CPU option oracle requires opt-in"
    );
    let project =
        PathBuf::from(std::env::var_os("LWE_MIKU_TEST_PROJECT").context("Set original project")?);
    let builtin =
        PathBuf::from(std::env::var_os("LWE_SCENE_TEST_ASSETS").context("Set builtin assets")?);
    let report = PathBuf::from(
        std::env::var_os("LWE_MIKU_OPTIONS_REPORT").context("Set private option report")?,
    );
    let before = hashes(&project)?;
    let assets = lwe_scene::SceneAssets::open(&project, Some(&builtin))?;
    let metadata: serde_json::Value =
        serde_json::from_slice(&assets.read("project.json", 1024 * 1024)?)?;
    let raw: serde_json::Value = serde_json::from_slice(&assets.read(
        metadata["file"].as_str().context("Scene file missing")?,
        16 * 1024 * 1024,
    )?)?;
    let objects = raw["objects"]
        .as_array()
        .context("Original objects missing")?;
    let handle = |id: u64| -> Result<usize> {
        Ok(objects
            .iter()
            .position(|o| o["id"].as_u64() == Some(id))
            .context("Original object missing")?
            + 1)
    };
    let static_handle = handle(50)?;
    let main_handle = handle(120)?;
    ensure!(
        objects[static_handle - 1]["image"] == objects[main_handle - 1]["image"],
        "Original static and Main models differ"
    );
    let mut cases = Vec::new();
    for option in ["0", "2"] {
        let overrides = BTreeMap::from([
            ("glitch".to_owned(), json!(option)),
            ("framerate".to_owned(), json!(true)),
        ]);
        let prepared =
            PreparedScene::prepare_with_properties(&project, Some(&builtin), &overrides)?;
        let summary = prepared.summary();
        ensure!(
            summary.layer_count == if option == "0" { 11 } else { 10 }
                && summary.effect_count == 5
                && summary.pass_count == if option == "0" { 26 } else { 25 },
            "Original option graph omitted authored resources/effects"
        );
        ensure!(
            prepared
                .textures
                .iter()
                .filter(|t| t.timed.is_some())
                .count()
                == 2,
            "Hidden script-owned timed resources were dropped"
        );
        let plan = prepared
            .scripts
            .as_ref()
            .context("Original script plan missing")?;
        let initial = plan.initial_states();
        let static_state = initial
            .iter()
            .find(|s| s.handle == static_handle)
            .context("Static state missing")?;
        ensure!(
            static_state.visible == (option == "0") && static_state.alpha == 1.,
            "Original Static50 condition/alpha changed"
        );
        let mut initial_rows = Vec::new();
        for (id, alpha) in [(120, 1.), (67, 0.), (69, 0.), (85, 0.2)] {
            let h = handle(id)?;
            let state = initial
                .iter()
                .find(|s| s.handle == h)
                .context("Original controlled state missing")?;
            ensure!(
                !state.visible && (state.alpha - alpha).abs() < 0.000001,
                "Option altered original initial alpha/visibility for{id}"
            );
            ensure!(
                prepared.script_layers.contains_key(&h),
                "Hidden authored layer{id} no longer retained"
            );
            initial_rows.push(json!({"object_id":id,"handle":h,"prepared_index":prepared.script_layers[&h],"visible":state.visible,"alpha":state.alpha}));
        }
        let main_index = prepared.script_layers[&main_handle];
        let primary = |layer: &PreparedLayer| -> Result<usize> {
            match layer.stages[0].passes[0].textures.first() {
                Some(TextureBinding::Static(i)) => Ok(*i),
                _ => bail!("Original Main primary texture changed"),
            }
        };
        let main_texture = primary(&prepared.layers[main_index])?;
        let static_texture = if option == "0" {
            let layer = &prepared.layers[main_index - 1];
            let index = primary(layer)?;
            ensure!(
                layer.tint[3] == 1.
                    && prepared.textures[index].timed.is_none()
                    && !prepared.textures[index].rgba().is_empty(),
                "Static50 did not keep its actual immutable pixels/alpha"
            );
            ensure!(
                prepared.textures[index].fingerprint == prepared.textures[main_texture].fingerprint,
                "Static50 did not use the original shared Main asset"
            );
            Some(index)
        } else {
            None
        };
        let worker = super::super::dynamic_script::ScriptWorker::spawn(plan)?;
        let take = || -> Result<super::super::dynamic_script::FrameOutput> {
            let started = Instant::now();
            loop {
                if let Some(output) = worker.take_frame()? {
                    return Ok(output);
                }
                ensure!(
                    started.elapsed() < Duration::from_secs(5),
                    "Original CPU worker timeout"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        let first = take()?;
        ensure!(
            first.seq == 0 && first.time == 0. && first.states == initial,
            "Worker initialization differs from prepared option snapshot"
        );
        let mut audio = super::super::spectrum::SpectrumSnapshot::default();
        audio.script_average[0] = 0.4;
        audio.script_average[1] = 1.;
        audio.script_average[63] = 0.8;
        audio.left[7] = 0.8;
        audio.right[7] = 0.2;
        let started = Instant::now();
        while !worker.submit_frame(1, 0.2, audio)? {
            ensure!(
                started.elapsed() < Duration::from_secs(5),
                "Original CPU tick submission timeout"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let active = take()?;
        ensure!(
            active.seq == 1 && active.time == 0.2,
            "Original tagged tick identity changed"
        );
        let mut active_rows = Vec::new();
        for (id, alpha) in [(120, 0.), (67, 0.26), (69, 0.519999984), (85, 0.574999995)] {
            let h = handle(id)?;
            let state = active
                .states
                .iter()
                .find(|s| s.handle == h)
                .context("Updated original state missing")?;
            ensure!(
                !state.visible && (f64::from(state.alpha) - alpha).abs() < 0.000001,
                "Hidden option layer{id} did not keep original 16/64 audio callback/visibility"
            );
            active_rows.push(json!({"object_id":id,"visible":state.visible,"alpha":state.alpha}));
        }
        let text = active
            .states
            .iter()
            .find(|s| s.handle == handle(106).unwrap())
            .context("Original text state missing")?;
        ensure!(
            text.visible && text.text.is_some(),
            "Framerate option did not keep authored text/script"
        );
        let budget = prepared.resource_plan_for_output(1920, 1080)?;
        budget.preflight(0, 0)?;
        cases.push(json!({"glitch":option,"overrides":overrides,"summary":{"layers":summary.layer_count,"effects":summary.effect_count,"passes":summary.pass_count,"animated":summary.animated},"static50":{"visible":static_state.visible,"alpha":static_state.alpha,"prepared_texture":static_texture,"main_fingerprint":format!("{:x}",Sha256::digest(prepared.textures[main_texture].rgba()))},"initial_controlled":initial_rows,"active_controlled":active_rows,"worker_seq0_matches_prepared":true,"active_seq":active.seq,"active_time":active.time,"timed_sources_retained":2,"text_visible":text.visible,"logical_gpu_plan1920x1080":budget.total(0)?}));
    }
    let after = hashes(&project)?;
    ensure!(
        before == after,
        "Original assets changed during CPU option oracle"
    );
    std::fs::write(
        report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"scope":"actual original PreparedScene and tagged worker CPU options; no GL/native pixel, capture or30FPS proof","source_before":before,"source_after":after,"cases":cases}),
        )?,
    )?;
    Ok(())
}
