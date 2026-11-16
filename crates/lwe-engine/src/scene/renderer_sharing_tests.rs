//! Authored, tiny offscreen fixtures; no Workshop content or desktop backend.

use super::*;
use khronos_egl as egl;

pub(super) struct Offscreen {
    egl: egl::DynamicInstance<egl::EGL1_4>,
    display: egl::Display,
    config: egl::Config,
    contexts: Vec<egl::Context>,
    surface: egl::Surface,
}

impl Offscreen {
    pub(super) fn new() -> Result<Self> {
        let egl = unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required()? };
        let display =
            unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.context("No offscreen EGL display")?;
        egl.initialize(display)?;
        egl.bind_api(egl::OPENGL_API)?;
        let config = egl
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
            .context("No offscreen EGL configuration")?;
        let surface = egl.create_pbuffer_surface(
            display,
            config,
            &[egl::WIDTH, 2, egl::HEIGHT, 2, egl::NONE],
        )?;
        let mut result = Self {
            egl,
            display,
            config,
            contexts: Vec::new(),
            surface,
        };
        result.add_context()?;
        result.current(0)?;
        gl::load_with(|name| {
            result
                .egl
                .get_proc_address(name)
                .map(|function| function as *const std::ffi::c_void)
                .unwrap_or(std::ptr::null())
        });
        Ok(result)
    }

    fn add_context(&mut self) -> Result<()> {
        self.contexts.push(self.egl.create_context(
            self.display,
            self.config,
            None,
            &[
                egl::CONTEXT_MAJOR_VERSION,
                3,
                egl::CONTEXT_MINOR_VERSION,
                0,
                egl::NONE,
            ],
        )?);
        Ok(())
    }

    fn current(&self, index: usize) -> Result<()> {
        self.egl.make_current(
            self.display,
            Some(self.surface),
            Some(self.surface),
            Some(self.contexts[index]),
        )?;
        Ok(())
    }
}

impl Drop for Offscreen {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_surface(self.display, self.surface);
        for context in &self.contexts {
            let _ = self.egl.destroy_context(self.display, *context);
        }
        let _ = self.egl.terminate(self.display);
    }
}

pub(super) fn fixture(repeat: bool, rgba: Vec<u8>) -> PreparedScene {
    let shader = if repeat {
        let mut shader = super::super::shader::image();
        let shifted = shader.fragment.replace(
            "texture(g_Texture0,uv)",
            "texture(g_Texture0,uv + vec2(1.0,0.0))",
        );
        assert_ne!(
            shifted, shader.fragment,
            "repeat fixture failed to offset the real image sampling expression"
        );
        shader.fragment = shifted;
        shader
    } else {
        super::super::shader::image()
    };
    // Both passes describe the same source bytes/metadata. Their authored
    // labels select distinct fixed sampling policies from the real rules.
    PreparedScene {
        canvas: [2.0, 2.0],
        clear: [0.0, 0.0, 0.0, 1.0],
        textures: vec![PreparedTexture {
            static_source: None,

            timed: None,
            image: None,
            text_frame: None,
            _rgba_reservation: None,

            fingerprint: [0; 32],
            width: 2,
            height: 2,
            image_width: 2,
            image_height: 2,
            source_format: 0,
            source_flags: 0,
            rgba,
            animation: None,
        }],
        layers: vec![PreparedLayer {
            world_2d: None,
            clamp_uvs: false,
            crop_offset: [0.; 2],
            no_padding: false,
            text: None,
            origin_z: 0.,
            angles: [0.; 3],

            size: [2.0, 2.0],
            origin: [1.0, 1.0],
            scale: [1.0; 2],
            angle: 0.0,
            tint: [1.0; 4],
            blend: Blend::Alpha,
            composite: None,
            stages: vec![PreparedStage {
                persistent_targets: std::collections::BTreeSet::new(),

                targets: BTreeMap::new(),
                passes: vec![PreparedPass {
                    kind: crate::scene::PreparedPassKind::Render,

                    label: if repeat {
                        "genericimage"
                    } else {
                        "authored clamp fixture"
                    }
                    .into(),
                    shader: Some(shader),
                    textures: vec![TextureBinding::Static(0)],
                    constants: BTreeMap::new(),
                    target: None,
                    blend: Blend::Replace,
                }],
            }],
        }],
        audio: Vec::new(),
        bloom: None,
        particles: Vec::new(),
        draw_order: vec![super::super::DrawObject::Image(0)],
        summary: super::super::SceneSummary {
            layer_count: 1,
            effect_count: 0,
            pass_count: 1,
            animated: false,
        },
        scripts: None,
        script_layers: BTreeMap::new(),
    }
}

fn render_pixels(renderer: &mut SceneRenderer) -> Result<[u8; 16]> {
    unsafe {
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
    }
    ensure!(renderer.render_forced(2, 2, 0.0)?, "fixture did not render");
    renderer.report_presented();
    let mut pixels = [0; 16];
    unsafe {
        gl::ReadPixels(
            0,
            0,
            2,
            2,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            pixels.as_mut_ptr().cast(),
        );
    }
    check_gl("reading shared fixture pixels")?;
    Ok(pixels)
}

fn sampling(id: u32) -> i32 {
    let mut wrap = 0;
    unsafe {
        gl::BindTexture(gl::TEXTURE_2D, id);
        gl::GetTexParameteriv(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, &mut wrap);
    }
    wrap
}

#[test]
fn resource_plan_rejects_rgba_overflow_before_gpu_allocation() {
    let mut scene = fixture(false, [255, 0, 0, 255].repeat(4));
    scene.textures[0].width = u32::MAX;
    scene.textures[0].height = u32::MAX;
    assert!(
        resource_plan(&scene)
            .unwrap_err()
            .to_string()
            .contains("overflow")
    );
}

#[test]
#[ignore = "requires an explicit tiny EGL/OpenGL offscreen run"]
fn offscreen_shared_textures_preserve_pixels_sampling_release_and_context_identity() -> Result<()> {
    let mut contexts = Offscreen::new()?;
    let pool = TexturePool::new()?;
    let rgba = vec![
        255, 0, 0, 255, 255, 255, 0, 255, 0, 0, 255, 255, 0, 255, 0, 255,
    ];
    let expected = [
        0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 0, 255,
    ];
    let scene = fixture(false, rgba.clone());
    let mut current = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    let mut candidate = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    let id = current.textures[0].get(false)?.id;
    assert_eq!(candidate.textures[0].get(false)?.id, id);
    assert_eq!(pool.resident_bytes()?, 32);
    assert_eq!(pool.allocation_count(), 2);
    assert_eq!(pool.lease_count(), 4);
    let mut union = current.allocation_plan()?;
    union.merge(&candidate.allocation_plan()?)?;
    assert_eq!(
        union.total(0)?,
        32 + current.allocation_plan()?.private_bytes * 2
    );
    assert_eq!(render_pixels(&mut current)?, expected);
    assert_eq!(render_pixels(&mut candidate)?, expected);

    let repeating_scene = fixture(true, rgba);
    let mut repeating = SceneRenderer::initialize_shared(&repeating_scene, Rc::clone(&pool))?;
    let repeating_id = repeating.textures[0].get(true)?.id;
    assert_ne!(repeating_id, id);
    assert_eq!(pool.resident_bytes()?, 48);
    assert_eq!(sampling(id), gl::CLAMP_TO_EDGE as i32);
    assert_eq!(sampling(repeating_id), gl::REPEAT as i32);
    assert_eq!(render_pixels(&mut repeating)?, expected);
    assert_eq!(render_pixels(&mut current)?, expected);
    assert_eq!(sampling(id), gl::CLAMP_TO_EDGE as i32);
    assert_eq!(sampling(repeating_id), gl::REPEAT as i32);

    let changed_scene = fixture(false, [0, 255, 255, 255].repeat(4));
    let mut changed = SceneRenderer::initialize_shared(&changed_scene, Rc::clone(&pool))?;
    assert_ne!(changed.textures[0].get(false)?.id, id);
    assert_eq!(
        render_pixels(&mut changed)?,
        [0, 255, 255, 255].repeat(4).as_slice()
    );
    changed.cleanup();
    assert_eq!(pool.resident_bytes()?, 48);

    let mut invalid = fixture(false, [255, 0, 0, 255].repeat(4));
    // Failure occurs after a fresh readonly upload. Only that lease may retire;
    // the two existing generations and sampler variant retain their pixels.
    invalid.layers[0].stages[0].passes[0]
        .shader
        .as_mut()
        .unwrap()
        .fragment = "invalid authored GLSL".into();
    assert!(SceneRenderer::initialize_shared(&invalid, Rc::clone(&pool)).is_err());
    assert_eq!(pool.resident_bytes()?, 48);
    assert_eq!(render_pixels(&mut current)?, expected);

    invalid.textures[0].width = 16384;
    invalid.textures[0].height = 16384;
    let before = pool.allocation_count();
    assert!(SceneRenderer::initialize_shared(&invalid, Rc::clone(&pool)).is_err());
    assert_eq!(
        pool.allocation_count(),
        before,
        "over-budget candidate uploaded before preflight"
    );

    candidate.cleanup();
    assert_eq!(pool.lease_count(), 4);
    assert_eq!(unsafe { gl::IsTexture(id) }, gl::TRUE);
    assert_eq!(render_pixels(&mut current)?, expected);
    current.cleanup();
    assert_eq!(unsafe { gl::IsTexture(id) }, gl::FALSE);
    assert_eq!(pool.resident_bytes()?, 32);
    repeating.cleanup();
    assert_eq!(pool.resident_bytes()?, 0);
    assert_eq!(pool.allocation_count(), 0);
    assert_eq!(pool.lease_count(), 0);

    // A last lease may expire with another context current. Its bytes stay
    // charged until the original context actually drains its retirement.
    let mut retained = SceneRenderer::initialize_shared(&scene, Rc::clone(&pool))?;
    assert_eq!(render_pixels(&mut retained)?, expected);
    let guard = GlState::capture();
    guard.begin();
    let key = TextureKey {
        fingerprint: changed_scene.textures[0].fixture_fingerprint(),
        repeat: false,
    };
    let lease = pool.acquire(key, 16, || GpuTexture::upload(&changed_scene.textures[0]))?;
    drop(guard);
    contexts.add_context()?;
    contexts.current(1)?;
    drop(lease);
    assert_eq!(pool.retired_bytes()?, 16);
    assert!(pool.drain_retired().is_err());
    assert!(SceneRenderer::initialize_shared(&scene, Rc::clone(&pool)).is_err());
    assert!(retained.render_forced(2, 2, 0.0).is_err());
    assert_eq!(pool.resident_bytes()?, 48);
    let fresh_pool = TexturePool::new()?;
    assert_ne!(fresh_pool.generation(), pool.generation());
    let mut fresh = SceneRenderer::initialize_shared(&scene, Rc::clone(&fresh_pool))?;
    assert_eq!(render_pixels(&mut fresh)?, expected);
    fresh.cleanup();
    fresh_pool.close()?;
    contexts.current(0)?;
    pool.drain_retired()?;
    assert_eq!(pool.retired_bytes()?, 0);
    assert_eq!(render_pixels(&mut retained)?, expected);
    assert!(retained.render(2, 2, 0.1).is_err());
    retained.cleanup();
    assert_eq!(pool.resident_bytes()?, 0);
    pool.close()?;
    assert!(pool.ensure_current().is_err());
    Ok(())
}

#[test]
#[ignore = "requires an explicit tiny EGL/OpenGL offscreen run"]
fn offscreen_close_with_live_lease_quarantines_and_keeps_charge() -> Result<()> {
    let _context = Offscreen::new()?;
    let pool = TexturePool::new()?;
    let mut renderer = SceneRenderer::initialize_shared(
        &fixture(false, [255, 0, 0, 255].repeat(4)),
        Rc::clone(&pool),
    )?;
    assert!(pool.close().is_err());
    assert_eq!(pool.resident_bytes()?, 32);
    assert_eq!(pool.lease_count(), 2);
    assert!(pool.ensure_current().is_err());
    renderer.cleanup();
    assert_eq!(pool.retired_bytes()?, 32);
    assert!(pool.drain_retired().is_err());
    Ok(())
}
