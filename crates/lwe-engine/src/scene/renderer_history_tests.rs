//! Stateful history and presentation protocol with authored offscreen pixels.
use super::*;
use std::time::{Duration, Instant};

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
    check_gl("reading authored history pixels")?;
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
        let rendered = if force {
            renderer.render_forced(target.texture.width, target.texture.height, time)?
        } else {
            renderer.render(target.texture.width, target.texture.height, time)?
        };
        if rendered {
            renderer.report_presented();
            return read(target);
        }
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "Authored frame did not become ready"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn history_fixture(color: [u8; 4]) -> PreparedScene {
    let mut scene = super::sharing_tests::fixture(false, color.repeat(4));
    scene.layers[0].blend = Blend::Replace;
    scene.summary.animated = true;
    let mut accumulation = super::super::shader::image();
    accumulation.fragment="#version 130\nin vec2 v_UV;out vec4 lwe_FragColor;uniform sampler2D g_Texture0;uniform sampler2D g_Texture1;void main(){lwe_FragColor=mix(texture(g_Texture1,v_UV),texture(g_Texture0,v_UV),0.5);}".into();
    let pass =
        |label: &str, shader: Option<super::super::shader::Shader>, kind, textures, target| {
            PreparedPass {
                label: label.into(),
                shader,
                kind,
                textures,
                constants: BTreeMap::new(),
                target,
                blend: Blend::Replace,
            }
        };
    scene.layers[0].stages.push(PreparedStage {
        targets: BTreeMap::from([("history".into(), 1.), ("work".into(), 1.)]),
        persistent_targets: std::collections::BTreeSet::from(["history".into()]),
        passes: vec![
            pass(
                "authored accumulation",
                Some(accumulation),
                crate::scene::PreparedPassKind::Render,
                vec![
                    TextureBinding::Input,
                    TextureBinding::History("history".into()),
                ],
                Some("work".into()),
            ),
            pass(
                "authored history copy",
                None,
                crate::scene::PreparedPassKind::Copy {
                    source: TextureBinding::Named("work".into()),
                },
                Vec::new(),
                Some("history".into()),
            ),
            pass(
                "authored history combine",
                Some(super::super::shader::image()),
                crate::scene::PreparedPassKind::Render,
                vec![TextureBinding::Named("work".into())],
                None,
            ),
        ],
    });
    scene
}
#[test]
fn work_plan_keeps_base_input_distinct_from_accumulation_target() -> Result<()> {
    let scene = history_fixture([255, 0, 0, 255]);
    let plan = resource_plan(&scene)?;
    ensure!(
        plan.work.len() == 3,
        "Authored history reserves a third image for future input reads"
    );
    ensure!(
        plan.private_bytes == 20,
        "Only black plus one unique history are retained"
    );
    Ok(())
}
#[test]
#[ignore = "requires opt-in surfaceless software EGL; no desktop backend"]
fn history_copy_pause_resize_and_frozen_rollback_keep_actual_pixels() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let mut red = history_fixture([255, 0, 0, 255]);
    // An intervening effect ends in the same pooled image that the following
    // accumulation writes. Its input must be moved without another allocation.
    red.layers[0].stages.insert(
        1,
        PreparedStage {
            targets: BTreeMap::new(),
            persistent_targets: Default::default(),
            passes: vec![PreparedPass {
                label: "authored intervening effect".into(),
                shader: Some(super::super::shader::image()),
                kind: super::super::PreparedPassKind::Render,
                textures: vec![TextureBinding::Input],
                constants: BTreeMap::new(),
                target: None,
                blend: Blend::Replace,
            }],
        },
    );
    ensure!(
        resource_plan(&red)?.work.len() == 3,
        "Input relocation must fit the three preflighted work images"
    );
    let green = history_fixture([0, 255, 0, 255]);
    let output = RenderTarget::new(2, 2)?;
    let mut old = SceneRenderer::initialize_shared(&red, Rc::clone(&pool))?;
    let first = draw(&mut old, &output, 0., false)?;
    ensure!(
        first
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| (127..=128).contains(&p[0]) && p[1..3] == [0, 0] && p[3] == p[0]),
        "First accumulation differs: {:?}",
        &first[..4]
    );
    let second = draw(&mut old, &output, 0.2, false)?;
    ensure!(
        second
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| (191..=192).contains(&p[0]) && p[1..3] == [0, 0] && p[3] == p[0]),
        "Second accumulation differs: {:?}",
        &second[..4]
    );
    old.pause();
    let paused = draw(&mut old, &output, 1., true)?;
    ensure!(paused == second, "Paused forced redraw advanced history");
    let enlarged = RenderTarget::new(4, 4)?;
    let resized = draw(&mut old, &enlarged, 1., true)?;
    ensure!(
        resized.as_chunks::<4>().0.iter().all(|p| p == &second[..4]),
        "Paused resize advanced history"
    );
    let rejected_budget = lwe_scene::AtlasMemoryBudget::new(15)?;
    ensure!(
        old.freeze_with_budget(&rejected_budget).is_err(),
        "History snapshot exceeded a real CPU gate"
    );
    ensure!(
        !old.frozen && old.frozen_histories.is_empty(),
        "Failed snapshot changed live ownership"
    );
    ensure!(
        draw(&mut old, &enlarged, 2., true)? == resized,
        "Failed snapshot lost original presented pixels"
    );
    let mut pack = 0;
    unsafe {
        gl::GenBuffers(1, &mut pack);
        gl::BindBuffer(gl::PIXEL_PACK_BUFFER, pack);
        gl::BufferData(gl::PIXEL_PACK_BUFFER, 64, std::ptr::null(), gl::STATIC_DRAW);
        gl::PixelStorei(gl::PACK_ALIGNMENT, 8);
        gl::PixelStorei(gl::PACK_ROW_LENGTH, 6);
        gl::PixelStorei(gl::PACK_SKIP_PIXELS, 1);
        gl::PixelStorei(gl::PACK_SKIP_ROWS, 1);
    }
    old.freeze_for_rollback()?;
    let mut restored_pack = [0; 5];
    unsafe {
        for (i, key) in [
            gl::PIXEL_PACK_BUFFER_BINDING,
            gl::PACK_ALIGNMENT,
            gl::PACK_ROW_LENGTH,
            gl::PACK_SKIP_PIXELS,
            gl::PACK_SKIP_ROWS,
        ]
        .into_iter()
        .enumerate()
        {
            gl::GetIntegerv(key, &mut restored_pack[i]);
        }
        gl::BindBuffer(gl::PIXEL_PACK_BUFFER, 0);
        gl::DeleteBuffers(1, &pack);
        gl::PixelStorei(gl::PACK_ALIGNMENT, 4);
        gl::PixelStorei(gl::PACK_ROW_LENGTH, 0);
        gl::PixelStorei(gl::PACK_SKIP_PIXELS, 0);
        gl::PixelStorei(gl::PACK_SKIP_ROWS, 0);
    }
    ensure!(
        restored_pack == [pack as i32, 8, 6, 1, 1],
        "Frozen history leaked host pack state: {restored_pack:?}"
    );
    ensure!(
        old.frozen_histories
            .values()
            .map(|h| h.capacity_bytes())
            .sum::<usize>()
            == 16,
        "Snapshot actual capacity accounting differs"
    );
    ensure!(
        old.layers
            .iter()
            .flat_map(|l| &l.stages)
            .all(|s| s.targets.is_empty()),
        "Frozen history remained allocated on GPU"
    );
    let frozen = old.allocation_plan()?;
    ensure!(
        frozen.work.is_empty() && old.back.is_none() && old.scratch.is_empty(),
        "Frozen rollback retained reconstructible work"
    );
    let mut candidate = SceneRenderer::initialize_shared(&green, Rc::clone(&pool))?;
    let candidate_pixels = draw(&mut candidate, &enlarged, 0., false)?;
    ensure!(
        candidate_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| (127..=128).contains(&p[1]) && p[0] == 0 && p[2] == 0 && p[3] == p[1]),
        "Candidate history was contaminated"
    );
    let restored = draw(&mut old, &enlarged, 3., true)?;
    ensure!(
        restored == resized,
        "Frozen rollback pixels changed after shared work overwrite"
    );
    candidate.cleanup();
    old.thaw(false);
    let resumed = draw(&mut old, &enlarged, 3., false)?;
    ensure!(
        resumed
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| (223..=224).contains(&p[0]) && p[1..3] == [0, 0] && p[3] == p[0]),
        "Restored history did not continue from accepted state"
    );
    old.cleanup();
    pool.drain_retired()?;
    ensure!(
        pool.resident_bytes()? == 0,
        "History cleanup retained shared allocations"
    );
    Ok(())
}
#[test]
#[ignore = "requires opt-in surfaceless software EGL; no desktop backend"]
fn swap_ack_is_required_and_failed_candidate_keeps_old_cache() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let scene = history_fixture([255, 0, 0, 255]);
    let output = RenderTarget::new(2, 2)?;
    let mut old = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    let pixels = draw(&mut old, &output, 0., false)?;
    old.freeze_for_rollback()?;
    let mut invalid = history_fixture([0, 255, 0, 255]);
    invalid.layers[0].stages[0].passes[0]
        .shader
        .as_mut()
        .unwrap()
        .fragment = "not GLSL".into();
    ensure!(
        SceneRenderer::initialize_shared(&invalid, Rc::clone(&pool)).is_err(),
        "Invalid candidate shader succeeded"
    );
    ensure!(
        draw(&mut old, &output, 2., true)? == pixels,
        "Candidate failure lost actual old pixels"
    );
    old.cleanup();
    pool.drain_retired()?;
    let mut unpresented = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    unsafe {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, output.fbo);
    }
    ensure!(
        unpresented.render(2, 2, 0.)?,
        "Initial authored render failed"
    );
    ensure!(
        !unpresented.cache_valid && unpresented.awaiting_swap,
        "GL draw published pixels before swap"
    );
    ensure!(
        unpresented.render(2, 2, 0.2).is_err(),
        "Unacknowledged frame advanced"
    );
    unpresented.cleanup();
    pool.drain_retired()?;
    Ok(())
}

#[test]
#[ignore = "requires opt-in surfaceless software EGL; no desktop backend"]
fn failed_swap_quarantines_advanced_history_and_only_replays_presented_pixels() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let scene = history_fixture([255, 0, 0, 255]);
    let output = RenderTarget::new(2, 2)?;
    let mut renderer = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    let presented = draw(&mut renderer, &output, 0., false)?;
    let history = &renderer.layers[0].stages[1].targets["history"];
    let before = read(history)?;
    unsafe {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, output.fbo);
    }
    ensure!(renderer.render(2, 2, 0.2)?, "Attempt did not draw");
    let attempted = read(&renderer.layers[0].stages[1].targets["history"])?;
    ensure!(
        attempted != before && renderer.awaiting_swap,
        "Fixture did not advance private history"
    );
    renderer.report_present_failed();
    ensure!(
        renderer.quarantined && !renderer.awaiting_swap,
        "Failed swap did not quarantine generation"
    );
    renderer.freeze_for_rollback()?;
    ensure!(
        draw(&mut renderer, &output, 10., true)? == presented,
        "Failed swap published attempted pixels"
    );
    renderer.thaw(false);
    renderer.resume();
    ensure!(
        renderer.render(2, 2, 10.).is_err(),
        "Advanced VM/history resumed after failed swap"
    );
    let resized = RenderTarget::new(4, 4)?;
    unsafe {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, resized.fbo);
    }
    ensure!(
        renderer.render_forced(4, 4, 10.).is_err(),
        "Failed generation was reconstructed at another size"
    );
    ensure!(
        renderer.frozen_histories[&(0, 1, "history".into())].pixels() == attempted,
        "Quarantine consumed another history tick"
    );
    renderer.cleanup();
    pool.drain_retired()?;
    ensure!(
        pool.resident_bytes()? == 0 && pool.lease_count() == 0,
        "Rejected generation retained shared leases"
    );
    Ok(())
}
#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn pending_resize_freeze_keeps_only_the_budgeted_presented_cache() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let mut scene = super::sharing_tests::fixture(false, Vec::new());
    let source = authored_timed_source()?;
    let texture = &mut scene.textures[0];
    texture.fingerprint = source.encoded_sha256();
    texture.width = 8;
    texture.height = 4;
    texture.image_width = 8;
    texture.image_height = 4;
    texture.source_format = 4;
    texture.source_flags = 7;
    texture.timed = Some(source);
    scene.summary.animated = true;
    let output = RenderTarget::new(2, 2)?;
    let mut renderer = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    let presented = draw(&mut renderer, &output, 0., false)?;
    let presented_id = renderer.front.as_ref().unwrap().texture.id;
    renderer.update(0.)?;
    let gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
    renderer.texture_worker = Some(super::super::timed_runtime::TimedWorker::held(gate)?);
    let enlarged = RenderTarget::new(4, 4)?;
    unsafe {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, enlarged.fbo);
    }
    ensure!(
        !renderer.render(4, 4, 0.125)? && renderer.job_requested,
        "Resize did not reach the held region decode"
    );
    let invalid_id = renderer.front.as_ref().unwrap().texture.id;
    let attempt_id = renderer.back.as_ref().unwrap().texture.id;
    ensure!(
        !renderer.cache_valid
            && renderer.previous_front.as_ref().unwrap().texture.id == presented_id
            && unsafe { gl::IsTexture(invalid_id) } == gl::TRUE,
        "Fixture did not retain both an invalid resized front and the old cache"
    );
    let preflight = renderer.frozen_allocation_plan()?;
    renderer.freeze_for_rollback()?;
    ensure!(
        renderer.cache_valid
            && renderer.front.as_ref().unwrap().texture.id == presented_id
            && renderer.previous_front.is_none()
            && renderer.back.is_none()
            && unsafe { gl::IsTexture(invalid_id) } == gl::FALSE
            && unsafe { gl::IsTexture(attempt_id) } == gl::FALSE,
        "Freeze retained an unbudgeted resize cache or dropped the presented image"
    );
    ensure!(
        renderer.allocation_plan()?.private_bytes == preflight.private_bytes,
        "Frozen physical cache ownership differs from its preflight"
    );
    ensure!(
        draw(&mut renderer, &output, 1., true)? == presented,
        "Pending resize freeze lost last presented rollback pixels"
    );
    renderer.cleanup();
    pool.drain_retired()?;
    ensure!(
        pool.resident_bytes()? == 0,
        "Pending resize retained shared leases"
    );
    Ok(())
}

fn authored_timed_source() -> Result<Arc<lwe_scene::TimedAtlasSource>> {
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
    let budget = lwe_scene::AtlasMemoryBudget::new(8 * 1024 * 1024)?;
    let reservation = budget.reserve_bytes(bytes.capacity())?;
    Ok(Arc::new(lwe_scene::TimedAtlasSource::from_budgeted_bytes(
        bytes,
        reservation,
        lwe_scene::ResourceOrigin::Project,
        &mut lwe_scene::TextureInspectionBudget::default(),
        &budget,
    )?))
}
#[test]
#[ignore = "requires opt-in surfaceless software EGL; no desktop backend"]
fn pending_vm_and_shader_use_one_audio_snapshot_and_direct_base_preserves_pixels() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let output = RenderTarget::new(2, 2)?;
    let rgba = vec![
        255, 0, 0, 128, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
    ];
    let mut direct = super::sharing_tests::fixture(false, rgba.clone());
    direct.textures[0].source_flags = 2;
    direct.layers[0].stages[0].passes[0].label = "genericimage4".into();
    direct.layers[0].stages[0].passes[0].shader = Some(super::super::shader::image_material());
    direct.layers[0].stages[0].passes[0]
        .textures
        .push(TextureBinding::Input);
    let mut reference = super::sharing_tests::fixture(false, rgba);
    reference.textures[0].source_flags = 2;
    reference.layers[0].stages[0].passes[0].shader = Some(super::super::shader::image_material());
    reference.layers[0].stages[0].passes[0]
        .textures
        .push(TextureBinding::Input);
    let mut a = SceneRenderer::initialize_shared(&direct, Rc::clone(&pool))?;
    let mut b = SceneRenderer::initialize_shared(&reference, Rc::clone(&pool))?;
    ensure!(
        a.layers[0].direct && !b.layers[0].direct,
        "Direct eligibility fixture is ineffective"
    );
    ensure!(
        draw(&mut a, &output, 0., false)? == draw(&mut b, &output, 0., false)?,
        "Direct unlit sampling changed authored alpha/pixels"
    );
    a.cleanup();
    b.cleanup();
    let mut scene = super::sharing_tests::fixture(false, [255, 255, 255, 255].repeat(4));
    scene.layers[0].blend = Blend::Replace;
    scene.summary.animated = true;
    let source = authored_timed_source()?;
    let texture = &mut scene.textures[0];
    texture.fingerprint = source.encoded_sha256();
    texture.rgba.clear();
    texture.width = 8;
    texture.height = 4;
    texture.image_width = 8;
    texture.image_height = 4;
    texture.source_flags = 7;
    texture.timed = Some(source);
    scene.layers[0].stages[0].passes[0].shader.as_mut().unwrap().fragment="#version 130\nin vec2 v_UV;out vec4 lwe_FragColor;uniform float g_AudioSpectrum16Left[16];uniform vec4 g_Color4;void main(){lwe_FragColor=vec4(g_AudioSpectrum16Left[0]*g_Color4.r,0,0,1);}".into();
    scene.layers[0].stages[0].passes[0]
        .shader
        .as_mut()
        .unwrap()
        .uniforms
        .push(super::super::shader::Uniform {
            name: "g_AudioSpectrum16Left".into(),
            kind: "float".into(),
            material: None,
            default: None,
            texture_default: None,
            texture_combo: None,
            array: true,
            array_length: Some(16),
            material_range: None,
        });
    let state = super::super::dynamic_script::LayerState {
        handle: 1,
        origin: [1., 1., 0.],
        scale: [1.; 3],
        angles: [0.; 3],
        color: [1.; 3],
        alpha: 1.,
        visible: true,
        text: None,
    };
    scene.scripts=Some(super::super::dynamic_script::PreparedScripts::prepare(vec![super::super::dynamic_script::Binding {layer:1,property:"color".into(),source:"const audio=engine.registerAudioBuffers(64);export function update(){return new Vec3(audio.average[0],1,1);}".into(),input:serde_json::json!([1.,1.,1.]),script_properties:serde_json::json!({}),timeline:None}],vec![state],serde_json::json!({}))?.into());
    scene.script_layers.insert(1, 0);
    let mut renderer = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    draw(&mut renderer, &output, 0., false)?;
    let mut first = super::super::spectrum::SpectrumSnapshot::default();
    first.left[0] = 0.8;
    first.script_average[0] = 0.25;
    let mut second = super::super::spectrum::SpectrumSnapshot::default();
    second.left[0] = 0.2;
    second.script_average[0] = 0.75;
    let gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
    renderer.texture_worker = Some(super::super::timed_runtime::TimedWorker::held(Arc::clone(
        &gate,
    ))?);
    renderer.last_spectrum = first;
    renderer.time = 0.125;
    ensure!(
        !renderer.prepare_packet(false)? && renderer.vm_requested,
        "Fixture did not queue VM input before replacement snapshot"
    );
    let started = Instant::now();
    while !renderer.job_requested {
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, output.fbo);
        }
        ensure!(
            !renderer.render_internal(2, 2, 0.125, false, Some(second))?,
            "A pending GPU packet produced a partial frame"
        );
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "Fixture did not reach GPU wait"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    ensure!(
        !renderer.render_internal(2, 2, 0.125, false, Some(second))?,
        "Held GPU completion published a partial frame"
    );
    gate.store(true, std::sync::atomic::Ordering::Release);
    loop {
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, output.fbo);
        }
        if renderer.render_internal(2, 2, 0.125, false, Some(second))? {
            renderer.report_presented();
            break;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "Audio frame did not become ready"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let accepted = read(&output)?;

    ensure!(
        accepted
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [51, 0, 0, 255]),
        "Shader/VM used different audio snapshots: {:?}; seq={} requested={:?} packet={:?} accepted={:?} tint={:?}",
        &accepted[..4],
        renderer.accepted_seq,
        [
            renderer.requested_spectrum.left[0],
            renderer.requested_spectrum.script_average[0]
        ],
        [
            renderer.packet_spectrum.left[0],
            renderer.packet_spectrum.script_average[0]
        ],
        [
            renderer.accepted_spectrum.left[0],
            renderer.accepted_spectrum.script_average[0]
        ],
        renderer.layers[0].tint
    );
    ensure!(
        renderer.layers[0].tint[0] == 0.25 && renderer.accepted_spectrum.left[0] == 0.8,
        "Accepted packet did not keep the submitted PCM snapshot"
    );
    renderer.pause();
    ensure!(
        draw(&mut renderer, &output, 9., true)? == accepted,
        "Paused audio packet changed"
    );
    let resized = RenderTarget::new(4, 4)?;
    ensure!(
        draw(&mut renderer, &resized, 9., true)?
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [51, 0, 0, 255]),
        "Paused resize refreshed audio/VM"
    );
    renderer.cleanup();
    pool.drain_retired()?;
    Ok(())
}

#[test]
#[ignore = "requires opt-in surfaceless EGL; no desktop backend"]
fn future_copy_and_named_reads_keep_original_stage_pixels() -> Result<()> {
    let _context = super::sharing_tests::Offscreen::new()?;
    let pool = TexturePool::new()?;
    let output = RenderTarget::new(2, 2)?;
    let pass = |shader, textures, target, kind| PreparedPass {
        label: "authored future read".into(),
        shader,
        kind,
        textures,
        target,
        blend: Blend::Replace,
        constants: BTreeMap::new(),
    };
    let mut blue = super::super::shader::image();
    blue.fragment =
        "#version 130\nout vec4 lwe_FragColor;void main(){lwe_FragColor=vec4(0,0,1,1);}".into();
    let mut copy = super::sharing_tests::fixture(false, [255, 0, 0, 255].repeat(4));
    copy.layers[0].blend = Blend::Replace;
    copy.layers[0].stages.push(PreparedStage {
        targets: BTreeMap::from([("saved".into(), 1.)]),
        persistent_targets: std::collections::BTreeSet::from(["saved".into()]),
        passes: vec![
            pass(
                Some(blue),
                vec![TextureBinding::Input],
                None,
                super::super::PreparedPassKind::Render,
            ),
            pass(
                None,
                vec![],
                Some("saved".into()),
                super::super::PreparedPassKind::Copy {
                    source: TextureBinding::Input,
                },
            ),
            pass(
                Some(super::super::shader::image()),
                vec![TextureBinding::History("saved".into())],
                None,
                super::super::PreparedPassKind::Render,
            ),
        ],
    });
    let mut renderer = SceneRenderer::initialize_shared(&copy, Rc::clone(&pool))?;
    ensure!(
        draw(&mut renderer, &output, 0., false)?
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [255, 0, 0, 255]),
        "Future Copy.source read overwritten stage input"
    );
    renderer.cleanup();
    let mut named = super::sharing_tests::fixture(false, [255; 4].repeat(4));
    named.layers[0].blend = Blend::Replace;
    let color = |rgb: &str| {
        let mut s = super::super::shader::image();
        s.fragment = format!(
            "#version 130\nout vec4 lwe_FragColor;void main(){{lwe_FragColor=vec4({rgb},1);}}"
        );
        s
    };
    let mut intermediate = super::super::shader::image();
    intermediate.fragment="#version 130\nin vec2 v_UV;uniform sampler2D g_Texture0;out vec4 lwe_FragColor;void main(){lwe_FragColor=vec4(texture(g_Texture0,v_UV).rgb*0.0001+vec3(0,0,1),1);}".into();
    let mut combine = super::super::shader::image();
    combine.fragment="#version 130\nin vec2 v_UV;uniform sampler2D g_Texture0;uniform sampler2D g_Texture1;out vec4 lwe_FragColor;void main(){lwe_FragColor=mix(texture(g_Texture0,v_UV),texture(g_Texture1,v_UV),0.5);}".into();
    named.layers[0].stages.push(PreparedStage {
        targets: BTreeMap::from([("a".into(), 1.), ("b".into(), 1.)]),
        persistent_targets: Default::default(),
        passes: vec![
            pass(
                Some(color("1,0,0")),
                vec![TextureBinding::Input],
                Some("a".into()),
                super::super::PreparedPassKind::Render,
            ),
            pass(
                Some(color("0,1,0")),
                vec![TextureBinding::Input],
                Some("b".into()),
                super::super::PreparedPassKind::Render,
            ),
            pass(
                Some(intermediate),
                vec![TextureBinding::Input],
                None,
                super::super::PreparedPassKind::Render,
            ),
            pass(
                Some(combine),
                vec![
                    TextureBinding::Named("a".into()),
                    TextureBinding::Named("b".into()),
                ],
                None,
                super::super::PreparedPassKind::Render,
            ),
        ],
    });
    ensure!(
        resource_plan(&named)?.work.len() == 4,
        "Future named values lack bounded scratch storage"
    );
    let mut renderer = SceneRenderer::initialize_shared(&named, Rc::clone(&pool))?;
    ensure!(
        draw(&mut renderer, &output, 0., false)?
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| (127..=128).contains(&p[0])
                && (127..=128).contains(&p[1])
                && p[2] == 0
                && p[3] == 255),
        "Anonymous pass overwrote a future Named read"
    );
    renderer.cleanup();
    pool.drain_retired()?;
    ensure!(
        pool.resident_bytes()? == 0,
        "Future-read cleanup retained leases"
    );
    Ok(())
}
