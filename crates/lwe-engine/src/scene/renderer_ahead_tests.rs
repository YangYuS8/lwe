//! CPU lookahead uses the existing mailboxes; presentation still owns GPU state.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn timed_source() -> Result<Arc<lwe_scene::TimedAtlasSource>> {
    let mut bytes = b"TEXV0005\0TEXI0001\0".to_vec();
    for value in [4_u32, 7, 8, 4, 8, 4, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"TEXB0004\0");
    for value in [1_u32, u32::MAX, 0, 1, 8, 4, 0, 0, 32] {
        bytes.extend(value.to_le_bytes());
    }
    for color in [0xf800_u16, 0x07e0_u16] {
        let mut block = [0_u8; 16];
        block[0] = 255;
        block[1] = 255;
        block[8..10].copy_from_slice(&color.to_le_bytes());
        bytes.extend(block);
    }
    bytes.extend(b"TEXS0003\0");
    for value in [2_u32, 2, 2] {
        bytes.extend(value.to_le_bytes());
    }
    for x in [0_f32, 4.] {
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(0.1_f32.to_le_bytes());
        for value in [x, 0., 2., 0., 0., 2.] {
            bytes.extend(value.to_le_bytes());
        }
    }
    let budget = super::super::timed_runtime::scene_texture_memory_budget();
    let reservation = budget.reserve_bytes(bytes.capacity())?;
    Ok(Arc::new(lwe_scene::TimedAtlasSource::from_budgeted_bytes(
        bytes,
        reservation,
        lwe_scene::ResourceOrigin::Project,
        &mut lwe_scene::TextureInspectionBudget::default(),
        budget,
    )?))
}

fn fixture(timed: bool, scripted: bool) -> Result<PreparedScene> {
    let mut scene = super::sharing_tests::fixture(false, [255, 0, 0, 255].repeat(4));
    scene.summary.animated = true;
    scene.layers[0].blend = Blend::Replace;
    if timed {
        let source = timed_source()?;
        let texture = &mut scene.textures[0];
        texture.fingerprint = source.encoded_sha256();
        texture.rgba.clear();
        texture.width = 8;
        texture.height = 4;
        texture.image_width = 8;
        texture.image_height = 4;
        texture.source_format = 4;
        texture.source_flags = 7;
        texture.timed = Some(source);
    }
    let mut accumulation = super::super::shader::image();
    accumulation.fragment = "#version 130\nin vec2 v_UV;out vec4 lwe_FragColor;uniform sampler2D g_Texture0;uniform sampler2D g_Texture1;void main(){lwe_FragColor=mix(texture(g_Texture1,v_UV),texture(g_Texture0,v_UV),0.5);}".into();
    scene.layers[0].stages.push(PreparedStage {
        targets: BTreeMap::from([("history".into(), 1.), ("work".into(), 1.)]),
        persistent_targets: std::collections::BTreeSet::from(["history".into()]),
        passes: vec![
            PreparedPass {
                label: "ahead fixture accumulation".into(),
                shader: Some(accumulation),
                kind: super::super::PreparedPassKind::Render,
                textures: vec![
                    TextureBinding::Input,
                    TextureBinding::History("history".into()),
                ],
                target: Some("work".into()),
                blend: Blend::Replace,
                constants: BTreeMap::new(),
            },
            PreparedPass {
                label: "ahead fixture history copy".into(),
                shader: None,
                kind: super::super::PreparedPassKind::Copy {
                    source: TextureBinding::Named("work".into()),
                },
                textures: Vec::new(),
                target: Some("history".into()),
                blend: Blend::Replace,
                constants: BTreeMap::new(),
            },
            PreparedPass {
                label: "ahead fixture combine".into(),
                shader: Some(super::super::shader::image()),
                kind: super::super::PreparedPassKind::Render,
                textures: vec![TextureBinding::Named("work".into())],
                target: None,
                blend: Blend::Replace,
                constants: BTreeMap::new(),
            },
        ],
    });
    if scripted {
        use super::super::dynamic_script::{Binding, LayerState, PreparedScripts};
        scene.scripts = Some(PreparedScripts::prepare(
            vec![Binding {
                layer: 1,
                property: "color".into(),
                source: "const audio=engine.registerAudioBuffers(64);let count=0;export function update(){count+=1;return new Vec3(audio.average[0],engine.runtime,Math.min(count/10,1));}".into(),
                input: serde_json::json!([1., 1., 1.]),
                script_properties: serde_json::json!({}),
            }],
            vec![LayerState {
                handle: 1,
                origin: [1., 1., 0.],
                scale: [1.; 3],
                angles: [0.; 3],
                color: [1.; 3],
                alpha: 1.,
                visible: true,
                text: None,
            }],
            serde_json::json!({}),
        )?.into());
        scene.script_layers.insert(1, 0);
    }
    Ok(scene)
}

fn pixels(target: &RenderTarget) -> Result<Vec<u8>> {
    Ok(frozen_history::FrozenHistory::capture(
        target,
        super::super::timed_runtime::scene_texture_memory_budget(),
    )?
    .pixels()
    .to_vec())
}

fn wait(mut progress: impl FnMut() -> Result<bool>) -> Result<()> {
    let start = Instant::now();
    while !progress()? {
        ensure!(
            start.elapsed() < Duration::from_secs(5),
            "Ahead fixture did not progress"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

fn draw(renderer: &mut SceneRenderer, size: i32, time: f64, force: bool) -> Result<Vec<u8>> {
    wait(|| {
        if force {
            renderer.render_forced(size, size, time)
        } else {
            renderer.render(size, size, time)
        }
    })?;
    renderer.report_presented();
    pixels(
        renderer
            .front
            .as_ref()
            .context("Ahead fixture has no presented cache")?,
    )
}

fn start(scene: &PreparedScene, pool: &Rc<TexturePool>) -> Result<SceneRenderer> {
    let mut renderer = SceneRenderer::initialize_shared(scene, Rc::clone(pool))?;
    draw(&mut renderer, 2, 0., false)?;
    renderer.update(0.)?;
    Ok(renderer)
}

fn ahead(renderer: &mut SceneRenderer, time: f64, timed: bool) -> Result<()> {
    wait(|| {
        renderer.prepare_ahead(time, timed)?;
        Ok(renderer.packet.is_some() && (!timed || renderer.job_requested))
    })
}

fn audio() -> super::super::spectrum::SpectrumSnapshot {
    let mut audio = super::super::spectrum::SpectrumSnapshot::default();
    audio.script_average[0] = 0.25;
    audio.left[0] = 0.8;
    audio
}

#[derive(Debug, PartialEq)]
struct Accepted {
    seq: u64,
    time: u64,
    next_seq: u64,
    spectrum: super::super::spectrum::SpectrumSnapshot,
    texture_frames: Vec<Option<usize>>,
    texture_keys: Vec<Option<TextureKey>>,
    tint: [f32; 4],
    text: BTreeMap<usize, String>,
    front_id: u32,
    cache: Vec<u8>,
    history: Vec<u8>,
    resident_bytes: u64,
}

fn accepted(renderer: &SceneRenderer) -> Result<Accepted> {
    let front = renderer
        .front
        .as_ref()
        .context("Ahead fixture has no front")?;
    Ok(Accepted {
        seq: renderer.accepted_seq,
        time: renderer.accepted_time.to_bits(),
        next_seq: renderer.next_seq,
        spectrum: renderer.accepted_spectrum,
        texture_frames: renderer.textures.iter().map(|view| view.frame).collect(),
        texture_keys: renderer.textures.iter().map(|view| view.key).collect(),
        tint: renderer.layers[0].tint,
        text: renderer.accepted_text.clone(),
        front_id: front.texture.id,
        cache: pixels(front)?,
        history: pixels(&renderer.layers[0].stages[1].targets["history"])?,
        resident_bytes: renderer.texture_pool.resident_bytes()?,
    })
}

#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn ahead_retains_gpu_state_and_one_vm_time_audio_tag() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let mut renderer = start(&fixture(true, true)?, &pool)?;
    let before = accepted(&renderer)?;
    let gl_before = GlState::capture();
    let gate = Arc::new(AtomicBool::new(false));
    renderer.texture_worker = Some(super::super::timed_runtime::TimedWorker::held(gate)?);
    let submitted_audio = audio();
    renderer.last_spectrum = submitted_audio;
    ahead(&mut renderer, 0.125, true)?;
    let packet = renderer.packet.clone().unwrap();
    assert_eq!((packet.seq, packet.time), (1, 0.125));
    assert_eq!(packet.states[0].color, [0.25, 0.125, 0.2]);
    renderer.last_spectrum = Default::default();
    for _ in 0..5 {
        renderer.prepare_ahead(1., true)?;
    }
    assert_eq!(renderer.packet.as_ref(), Some(&packet));
    assert_eq!(renderer.time, 0.125);
    assert_eq!(renderer.requested_spectrum, submitted_audio);
    assert_eq!(renderer.packet_spectrum, submitted_audio);
    assert_eq!(GlState::capture(), gl_before);
    assert_eq!(accepted(&renderer)?, before);
    assert!(renderer.ahead_timed && renderer.job_requested);
    renderer.cleanup();
    pool.drain_retired()?;
    Ok(())
}

#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn held_ahead_completion_is_installed_only_by_gated_render() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let mut renderer = start(&fixture(true, true)?, &pool)?;
    let before = accepted(&renderer)?;
    let gate = Arc::new(AtomicBool::new(false));
    renderer.texture_worker = Some(super::super::timed_runtime::TimedWorker::held(Arc::clone(
        &gate,
    ))?);
    let submitted_audio = audio();
    renderer.last_spectrum = submitted_audio;
    ahead(&mut renderer, 0.125, true)?;
    assert!(!renderer.render_internal(2, 2, 0.175, false, Some(Default::default()))?);
    assert_eq!(accepted(&renderer)?, before);
    gate.store(true, Ordering::Release);
    renderer.prepare_ahead(1., true)?;
    assert_eq!(accepted(&renderer)?, before);
    let next = draw(&mut renderer, 2, 0.175, false)?;
    assert_eq!((renderer.accepted_seq, renderer.accepted_time), (1, 0.125));
    assert_eq!(renderer.accepted_spectrum, submitted_audio);
    assert_eq!(renderer.textures[0].frame, Some(1));
    assert_eq!(renderer.layers[0].tint[..3], [0.25, 0.125, 0.2]);
    assert_ne!(next, before.cache);
    assert!(!renderer.ahead_timed && !renderer.job_requested);
    renderer.cleanup();
    pool.drain_retired()?;
    Ok(())
}

#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn pause_resize_and_resume_keep_ahead_packet_and_accepted_pixels() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let mut renderer = start(&fixture(true, true)?, &pool)?;
    let before = accepted(&renderer)?;
    renderer.last_spectrum = audio();
    ahead(&mut renderer, 0.125, true)?;
    let packet = renderer.packet.clone();
    renderer.pause();
    assert!(!renderer.job_requested && !renderer.ahead_timed);
    assert!(renderer.texture_worker.is_none());
    renderer.prepare_ahead(0.5, true)?;
    assert_eq!(renderer.packet, packet);
    assert_eq!(accepted(&renderer)?, before);
    assert_eq!(draw(&mut renderer, 2, 0.5, true)?, before.cache);
    let resized = draw(&mut renderer, 4, 0.5, true)?;
    assert!(
        resized
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel == &before.cache[..4])
    );
    assert_eq!(renderer.accepted_seq, before.seq);
    assert_eq!(renderer.accepted_time.to_bits(), before.time);
    assert_eq!(renderer.packet, packet);
    renderer.resume();
    draw(&mut renderer, 4, 0.75, false)?;
    assert_eq!((renderer.accepted_seq, renderer.accepted_time), (1, 0.125));
    assert_eq!(renderer.accepted_spectrum, audio());
    ahead(&mut renderer, 1., false)?;
    let following = renderer.packet.as_ref().unwrap();
    assert_eq!(following.seq, 2);
    assert!(following.time >= 0.125);
    renderer.cleanup();
    pool.drain_retired()?;
    Ok(())
}

#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn frozen_rollback_replays_accepted_epoch_before_consuming_ahead_packet() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let mut old = start(&fixture(true, true)?, &pool)?;
    old.last_spectrum = audio();
    draw(&mut old, 2, 0.075, false)?;
    let before = accepted(&old)?;
    ahead(&mut old, 0.125, true)?;
    let packet = old.packet.clone();
    old.freeze_for_rollback()?;
    assert!(!old.job_requested && !old.ahead_timed && old.texture_worker.is_none());
    assert_eq!(old.packet, packet);
    assert_eq!(
        old.frozen_histories[&(0, 1, "history".into())].pixels(),
        before.history
    );
    old.prepare_ahead(0.5, true)?;
    assert_eq!(old.packet, packet);
    let mut candidate_scene = fixture(false, false)?;
    candidate_scene.textures[0].rgba = [0, 255, 0, 255].repeat(4);
    let mut candidate = start(&candidate_scene, &pool)?;
    assert_ne!(draw(&mut candidate, 2, 0.1, false)?, before.cache);
    let mut invalid = candidate_scene;
    invalid.layers[0].stages[0].passes[0]
        .shader
        .as_mut()
        .unwrap()
        .fragment = "not GLSL".into();
    assert!(SceneRenderer::initialize_shared(&invalid, Rc::clone(&pool)).is_err());
    assert_eq!(draw(&mut old, 2, 0.5, true)?, before.cache);
    candidate.cleanup();
    old.thaw(false);
    let replay = draw(&mut old, 4, 0.5, true)?;
    assert!(
        replay
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel == &before.cache[..4])
    );
    assert_eq!(
        pixels(&old.layers[0].stages[1].targets["history"])?,
        before.history
    );
    assert_eq!(
        (old.accepted_seq, old.accepted_time.to_bits()),
        (before.seq, before.time)
    );
    assert_eq!(old.packet, packet);
    draw(&mut old, 4, 0.5, false)?;
    assert_eq!((old.accepted_seq, old.accepted_time), (2, 0.125));
    old.cleanup();
    pool.drain_retired()?;
    Ok(())
}

#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn frozen_timed_hierarchy_replays_accepted_pose_and_history_before_pending_epoch() -> Result<()> {
    use super::super::dynamic_script::{Binding, LayerState, PreparedScripts};
    use serde_json::json;

    let mut scene = fixture(true, false)?;
    scene.canvas = [4., 4.];
    scene.layers[0].stages[0].passes[0].shader =
        Some(super::super::shader::image_material_variant(
            &BTreeMap::from([("SPRITESHEET".into(), 1)]),
            true,
        )?);
    let objects = [
        json!({"id":11,"image":"models/image.json","parent":29,"origin":"0 0 0"}),
        json!({"id":29,"origin":"1 2 0"}),
    ];
    let graph =
        super::super::hierarchy::prepare(&objects, &[2].into_iter().collect(), scene.canvas)?;
    let state = |handle, origin| LayerState {
        handle,
        origin,
        scale: [1.; 3],
        angles: [0.; 3],
        color: [1.; 3],
        alpha: 1.,
        visible: true,
        text: None,
    };
    let scripts = PreparedScripts::prepare(
        vec![Binding {
            layer: 2,
            property: "origin".into(),
            source: "let n=0;export function update(){n+=1;return new Vec3(n===1?1:3,2,0);}".into(),
            input: json!([1., 2., 0.]),
            script_properties: json!({}),
        }],
        vec![state(1, [0.; 3]), state(2, [1., 2., 0.])],
        json!({}),
    )?;
    scene.hierarchy = Some(Arc::new(super::super::SceneHierarchy {
        initial: graph.resolve(scripts.initial_states())?,
        graph,
        layers: vec![(0, 0)],
    }));
    scene.scripts = Some(Arc::new(scripts));

    let _context = super::sharing_tests::Offscreen::new()?;
    let output = RenderTarget::new(8, 8)?;
    unsafe { gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, output.fbo) };
    let pool = TexturePool::new()?;
    let mut renderer = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    let pose = |renderer: &SceneRenderer| renderer.layers[0].world_2d.unwrap().origin;
    let sample = |rgba: &[u8], size: usize, x: usize| {
        let offset = (size * (size / 2) + x) * 4;
        [rgba[offset], rgba[offset + 1], rgba[offset + 2]]
    };
    draw(&mut renderer, 4, 0., false)?;
    renderer.update(0.)?;
    let before = accepted(&renderer)?;
    assert_eq!(pose(&renderer), [1., 2.]);
    assert_eq!(before.texture_frames, [Some(0)]);
    let red = sample(&before.cache, 4, 1);
    assert!(red[0] > 64 && red[1..] == [0, 0]);
    assert_eq!(sample(&before.cache, 4, 3), [0; 3]);

    renderer.texture_worker = Some(super::super::timed_runtime::TimedWorker::held(Arc::new(
        AtomicBool::new(false),
    ))?);
    ahead(&mut renderer, 0.125, true)?;
    let pending = renderer
        .packet
        .clone()
        .context("No next hierarchy packet")?;
    assert_eq!((pending.seq, pending.time), (1, 0.125));
    assert!(
        pending
            .states
            .iter()
            .any(|s| s.handle == 2 && s.origin == [3., 2., 0.])
    );
    assert!(!renderer.render(4, 4, 0.125)?);
    assert_eq!(pose(&renderer), [1., 2.]);
    assert_eq!(accepted(&renderer)?, before);

    renderer.freeze_for_rollback()?;
    assert_eq!(renderer.packet.as_ref(), Some(&pending));
    assert!(renderer.texture_worker.is_none() && renderer.textures[0].frame.is_none());
    assert_eq!(
        renderer.frozen_histories[&(0, 1, "history".into())].pixels(),
        before.history
    );
    renderer.thaw(true);
    let replay = draw(&mut renderer, 8, 0.5, true)?;
    assert_eq!(pose(&renderer), [1., 2.]);
    assert_eq!(renderer.accepted_seq, before.seq);
    assert_eq!(renderer.textures[0].frame, Some(0));
    assert_eq!(renderer.packet.as_ref(), Some(&pending));
    assert_eq!(
        pixels(&renderer.layers[0].stages[1].targets["history"])?,
        before.history
    );
    assert_eq!(sample(&replay, 8, 2), red);
    assert_eq!(sample(&replay, 8, 6), [0; 3]);

    renderer.resume();
    let resumed = draw(&mut renderer, 8, 0.75, false)?;
    assert_eq!((renderer.accepted_seq, renderer.accepted_time), (1, 0.125));
    assert_eq!(renderer.textures[0].frame, Some(1));
    assert_eq!(pose(&renderer), [3., 2.]);
    assert_eq!(sample(&resumed, 8, 2), [0; 3]);
    assert!(sample(&resumed, 8, 6)[1] > 64);
    assert_ne!(
        pixels(&renderer.layers[0].stages[1].targets["history"])?,
        before.history
    );
    renderer.cleanup();
    pool.drain_retired()?;
    assert_eq!(pool.resident_bytes()?, 0);
    Ok(())
}

#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn topology_cancel_releases_ahead_permit_and_preserves_gated_job() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let scene = fixture(true, true)?;
    let mut first = start(&scene, &pool)?;
    let mut second = start(&scene, &pool)?;
    let gate = Arc::new(AtomicBool::new(false));
    first.texture_worker = Some(super::super::timed_runtime::TimedWorker::held(gate)?);
    ahead(&mut first, 0.125, true)?;
    let first_packet = first.packet.clone();
    ahead(&mut second, 0.125, false)?;
    assert!(!second.job_requested);
    assert!(!second.render(2, 2, 0.125)?);
    assert!(
        !second.job_requested,
        "Ahead owner did not retain the producer permit"
    );
    first.cancel_ahead_timed();
    assert_eq!(first.packet, first_packet);
    assert!(!first.job_requested && first.texture_worker.is_none());
    let second_gate = Arc::new(AtomicBool::new(false));
    second.texture_worker = Some(super::super::timed_runtime::TimedWorker::held(Arc::clone(
        &second_gate,
    ))?);
    wait(|| {
        assert!(!second.render(2, 2, 0.125)?);
        Ok(second.job_requested)
    })?;
    assert!(!second.ahead_timed);
    second.cancel_ahead_timed();
    second.pause();
    assert!(second.job_requested && second.texture_worker.is_some());
    second.resume();
    second_gate.store(true, Ordering::Release);
    draw(&mut second, 2, 0.125, false)?;
    assert_eq!((second.accepted_seq, second.accepted_time), (1, 0.125));
    first.cleanup();
    second.cleanup();
    pool.drain_retired()?;
    Ok(())
}

#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn ahead_refuses_startup_unacknowledged_swap_and_shader_only_animation() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let mut renderer = SceneRenderer::initialize_shared(&fixture(true, true)?, Rc::clone(&pool))?;
    renderer.prepare_ahead(0.5, true)?;
    assert!(renderer.packet.is_none() && !renderer.vm_requested && !renderer.job_requested);
    assert_eq!(renderer.last_input, None);
    wait(|| renderer.render(2, 2, 0.))?;
    assert!(renderer.awaiting_swap && !renderer.drawn);
    renderer.prepare_ahead(0.5, true)?;
    assert!(renderer.packet.is_none() && !renderer.vm_requested && !renderer.job_requested);
    assert_eq!(renderer.next_seq, 1);
    renderer.report_presented();
    renderer.cleanup();
    let mut shader = start(&fixture(false, false)?, &pool)?;
    let before = accepted(&shader)?;
    let clock = (shader.time, shader.last_input, shader.rebase_time);
    shader.prepare_ahead(3., true)?;
    assert!(shader.packet.is_none() && !shader.vm_requested && !shader.job_requested);
    assert_eq!((shader.time, shader.last_input, shader.rebase_time), clock);
    assert_eq!(accepted(&shader)?, before);
    shader.cleanup();
    pool.drain_retired()?;
    Ok(())
}
