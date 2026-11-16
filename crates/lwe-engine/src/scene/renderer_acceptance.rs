//! Opt-in GPU acceptance against user-owned original scene assets.

use super::super::spectrum::SpectrumSnapshot;
use super::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const WIDTH: i32 = 1920;
const HEIGHT: i32 = 1080;

fn bind_target(target: &RenderTarget) {
    unsafe {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target.fbo);
        gl::BindFramebuffer(gl::READ_FRAMEBUFFER, target.fbo);
        gl::ReadBuffer(gl::COLOR_ATTACHMENT0);
    }
}

fn pixels(target: &RenderTarget) -> Result<Vec<u8>> {
    bind_target(target);
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
    check_gl("reading original Scene acceptance pixels")?;
    Ok(pixels)
}

fn render_snapshot(
    renderer: &mut SceneRenderer,
    target: &RenderTarget,
    snapshot: SpectrumSnapshot,
) -> Result<Vec<u8>> {
    bind_target(target);
    // This probe intentionally changes uniforms/order outside a normal tick.
    renderer.drawn = false;
    ensure!(
        renderer.render_internal(
            target.texture.width,
            target.texture.height,
            0.0,
            false,
            Some(snapshot),
        )?,
        "Forced acceptance frame did not render"
    );
    renderer.report_presented();
    pixels(target)
}

fn difference(left: &[u8], right: &[u8]) -> Value {
    assert_eq!(left.len(), right.len());
    let mut changed_pixels = 0;
    let mut absolute_rgb_difference = 0_u64;
    let mut maximum_channel_difference = 0;
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
        changed_pixels += usize::from(changed);
    }
    json!({
        "changed_pixels": changed_pixels,
        "absolute_rgb_difference": absolute_rgb_difference,
        "maximum_channel_difference": maximum_channel_difference,
    })
}

struct Report {
    path: Option<PathBuf>,
    frames: Option<PathBuf>,
    value: Value,
}

impl Report {
    fn new() -> Result<Self> {
        let path = std::env::var_os("LWE_SPACE_GL_REPORT").map(PathBuf::from);
        let frames = if let Some(path) = &path {
            ensure!(
                path.is_absolute(),
                "Acceptance report path must be absolute"
            );
            let parent = path.parent().context("Acceptance report has no parent")?;
            std::fs::create_dir_all(parent)?;
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .context("No workspace root")?
                .canonicalize()?;
            ensure!(
                !parent.canonicalize()?.starts_with(workspace),
                "Original Scene pixel diagnostics must be outside the Git workspace"
            );
            let frames = parent.join(format!(
                "{}-frames",
                path.file_stem()
                    .context("No report stem")?
                    .to_string_lossy()
            ));
            std::fs::create_dir_all(&frames)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&frames, std::fs::Permissions::from_mode(0o700))?;
            }
            Some(frames)
        } else {
            None
        };
        Ok(Self {
            path,
            frames,
            value: json!({
                "scope": "Original user-owned 2.5D Space GPU/FBO pixels; deterministic injected spectrum; no desktop application, audio capture, or original-engine visual parity claim",
                "comparisons": {},
            }),
        })
    }

    fn frame(&self, name: &str, width: i32, height: i32, pixels: &[u8]) -> Result<()> {
        if let Some(directory) = &self.frames {
            let path = directory.join(format!("{name}.png"));
            let image = image::RgbaImage::from_raw(width as u32, height as u32, pixels.to_vec())
                .context("Invalid acceptance pixel buffer")?;
            image::imageops::flip_vertical(&image).save(&path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(())
    }

    fn compare(
        &mut self,
        name: &str,
        baseline: &[u8],
        candidate: &[u8],
        expected_different: bool,
    ) -> Result<()> {
        let result = difference(baseline, candidate);
        let changed = result["changed_pixels"]
            .as_u64()
            .context("Invalid difference count")?;
        self.value["comparisons"][name] = result;
        ensure!(
            if expected_different {
                changed > 0
            } else {
                changed == 0
            },
            "Original Scene pixel acceptance {name}: {changed} changed pixels"
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

fn native_composite_pixels(
    assets: &lwe_scene::SceneAssets,
    vao: u32,
    mode: i32,
) -> Result<Vec<u8>> {
    // Independently chosen constant pixels make native modes 0/30/31 measurable
    // without copying any installed shader source into a fixture.
    let source = GpuTexture::upload(&PreparedTexture {
        static_source: None,

        timed: None,
        image: None,
        text_frame: None,
        _rgba_reservation: None,

        fingerprint: [0; 32],
        width: 1,
        height: 1,
        image_width: 1,
        image_height: 1,
        source_format: 0,
        source_flags: 2,
        animation: None,
        rgba: vec![20, 50, 120, 128],
    })?;
    let background = GpuTexture::upload(&PreparedTexture {
        static_source: None,

        timed: None,
        image: None,
        text_frame: None,
        _rgba_reservation: None,

        fingerprint: [0; 32],
        width: 1,
        height: 1,
        image_width: 1,
        image_height: 1,
        source_format: 0,
        source_flags: 2,
        animation: None,
        rgba: vec![40, 80, 160, 255],
    })?;
    let target = RenderTarget::new(2, 2)?;
    let shader = super::super::shader::load_image_composite(assets, mode)?;
    let program = Program::compile(&shader, "Original image composition acceptance")?;
    bind_target(&target);
    unsafe {
        gl::BindVertexArray(vao);
        gl::Viewport(0, 0, 2, 2);
        gl::Disable(gl::BLEND);
        gl::UseProgram(program.id);
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, source.id);
        gl::Uniform1i(program.location("g_Texture0"), 0);
        gl::ActiveTexture(gl::TEXTURE4);
        gl::BindTexture(gl::TEXTURE_2D, background.id);
        gl::Uniform1i(program.location("g_Texture4"), 4);
        gl::Uniform4f(program.location("g_Color4"), 1.0, 1.0, 1.0, 1.0);
    }
    program.matrix(&IDENTITY);
    unsafe {
        gl::DrawArrays(gl::TRIANGLES, 0, 6);
    }
    let pixels = pixels(&target)?;
    // Unbind before deleting: a current GL program remains delete-pending and
    // cannot be restored by the next renderer's state guard after unbinding it.
    unsafe {
        gl::UseProgram(0);
    }
    check_gl("reading original native composition pixels")?;
    Ok(pixels)
}

fn acceptance(project: &Path, assets_root: &Path, report: &mut Report) -> Result<()> {
    let properties = PreparedScene::property_definitions(project, Some(assets_root))?;
    ensure!(
        properties.len() == 20,
        "Original Space property declaration count changed"
    );
    let choices = BTreeMap::from([("globalspeed".to_owned(), json!(0))]);
    let prepared = PreparedScene::prepare_with_properties(project, Some(assets_root), &choices)?;
    let summary = prepared.summary();
    ensure!(
        summary.layer_count == 10 && summary.effect_count == 11 && summary.pass_count == 24,
        "Original Space layers/effects/passes changed: {summary:?}"
    );
    ensure!(
        estimated_gpu_bytes(&prepared)? == 202_180_100,
        "Original GPU logical payload changed"
    );
    let layout = scratch_layout(
        &prepared.layers,
        prepared.canvas,
        needs_full_frame(&prepared),
    );
    ensure!(
        layout == BTreeMap::from([((1920, 1080), 4), ((3840, 1080), 2)]),
        "Scratch FBO pooling changed: {layout:?}"
    );
    for layer in &prepared.layers {
        for stage in &layer.stages {
            for pass in &stage.passes {
                if pass.label == "effects/scroll" {
                    ensure!(
                        pass.constants["g_ScrollX"] == [0.0],
                        "Global Speed did not stop an original scrolling layer"
                    );
                }
            }
        }
    }
    report.value["original_structure"] = json!({
        "properties":20,"layers":10,"effects":11,"passes":24,
        "logical_gpu_payload_bytes":202_180_100,"shared_work_groups":2,"shared_work_targets":6,
    });
    let assets = lwe_scene::SceneAssets::open(project, Some(assets_root))?;
    let mut renderer = SceneRenderer::initialize(&prepared)?;
    let target = RenderTarget::new(WIDTH, HEIGHT)?;
    let silence = SpectrumSnapshot::default();
    let positive = SpectrumSnapshot {
        left: [0.75; 16],
        right: [0.60; 16],
        ..Default::default()
    };
    let quiet_pixels = render_snapshot(&mut renderer, &target, silence)?;
    let baseline = render_snapshot(&mut renderer, &target, positive)?;
    report.frame("default-silence", WIDTH, HEIGHT, &quiet_pixels)?;
    report.frame("default-stereo-spectrum", WIDTH, HEIGHT, &baseline)?;
    report.compare(
        "spectrum_changes_original_effects",
        &quiet_pixels,
        &baseline,
        true,
    )?;
    ensure!(
        baseline
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[..3] != [0, 0, 0])
            .count()
            > 10_000,
        "Original Scene is unexpectedly blank"
    );
    let left = render_snapshot(
        &mut renderer,
        &target,
        SpectrumSnapshot {
            left: [0.8; 16],
            right: [0.0; 16],
            ..Default::default()
        },
    )?;
    let right = render_snapshot(
        &mut renderer,
        &target,
        SpectrumSnapshot {
            left: [0.0; 16],
            right: [0.8; 16],
            ..Default::default()
        },
    )?;
    report.compare(
        "both_original_stereo_channels_reach_the_shader",
        &left,
        &right,
        false,
    )?;
    report.compare(
        "single_channel_spectrum_changes_pixels",
        &quiet_pixels,
        &left,
        true,
    )?;
    render_snapshot(&mut renderer, &target, positive)?;

    for (mode, expected) in [
        (0, [20, 50, 120, 128]),
        (30, [26, 56, 118, 255]),
        (31, [50, 105, 220, 255]),
    ] {
        let actual = native_composite_pixels(&assets, renderer.vao, mode)?;
        for pixel in actual.as_chunks::<4>().0.iter() {
            ensure!(
                pixel
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
                "Original composition mode {mode} returned {pixel:?}, expected {expected:?}"
            );
        }
        report.value["native_composite_pixels"][mode.to_string()] = json!(&actual[..4]);
    }

    let image_indices = [0, 1, 2, 3, 5, 6, 7, 8];
    for index in image_indices {
        let alpha = renderer.layers[index].tint[3];
        renderer.layers[index].tint[3] = 0.0;
        let candidate = render_snapshot(&mut renderer, &target, positive)?;
        renderer.layers[index].tint[3] = alpha;
        let name = format!("original-image-{index}-alpha-zero");
        report.frame(&name, WIDTH, HEIGHT, &candidate)?;
        report.compare(&name, &baseline, &candidate, true)?;
    }
    renderer.draw_order.reverse();
    let reversed = render_snapshot(&mut renderer, &target, positive)?;
    renderer.draw_order.reverse();
    report.frame("reversed-authored-order", WIDTH, HEIGHT, &reversed)?;
    report.compare(
        "authored_layer_and_effect_order_is_observable",
        &baseline,
        &reversed,
        true,
    )?;
    report.compare(
        "original_order_restored_exactly",
        &baseline,
        &render_snapshot(&mut renderer, &target, positive)?,
        false,
    )?;

    renderer.pause();
    bind_target(&target);
    ensure!(
        renderer.render_forced(WIDTH, HEIGHT, 90.0)?,
        "Paused forced frame did not render"
    );
    report.compare(
        "paused_forced_frame_retains_spectrum",
        &baseline,
        &pixels(&target)?,
        false,
    )?;
    let small = RenderTarget::new(WIDTH / 2, HEIGHT / 2)?;
    bind_target(&small);
    renderer.render_forced(WIDTH / 2, HEIGHT / 2, 120.0)?;
    renderer.report_presented();
    let resized = pixels(&small)?;
    renderer.render_forced(WIDTH / 2, HEIGHT / 2, 150.0)?;
    renderer.report_presented();
    report.compare(
        "paused_resized_frames_are_identical",
        &resized,
        &pixels(&small)?,
        false,
    )?;
    bind_target(&target);
    renderer.render_forced(WIDTH, HEIGHT, 180.0)?;
    renderer.report_presented();
    report.compare(
        "paused_resize_back_retains_original_pixels",
        &baseline,
        &pixels(&target)?,
        false,
    )?;
    ensure!(
        renderer.last_spectrum == positive && renderer.time == 0.0,
        "Pause lost retained spectrum/time"
    );
    ensure!(
        !renderer.render(WIDTH, HEIGHT, 200.0)?,
        "Ordinary paused rendering should not draw"
    );
    renderer.resume();
    report.compare(
        "resume_accepts_new_spectrum",
        &quiet_pixels,
        &render_snapshot(&mut renderer, &target, silence)?,
        false,
    )?;
    ensure!(
        renderer.time == 0.0,
        "Resume advanced a stopped original Scene clock"
    );
    renderer.cleanup();
    ensure!(
        renderer.gpu_payload_bytes() == 0,
        "Cleanup retained logical GPU allocations"
    );
    drop(renderer);
    drop(prepared);

    // These variants use only the project's own declared user properties. Every
    // image and all enabled effects still pass through production preparation.
    for property in ["chromaticabberation", "pulse", "bloom"] {
        let mut choices = choices.clone();
        choices.insert(property.to_owned(), json!(false));
        let prepared =
            PreparedScene::prepare_with_properties(project, Some(assets_root), &choices)?;
        let mut renderer = SceneRenderer::initialize(&prepared)?;
        let candidate = render_snapshot(&mut renderer, &target, positive)?;
        let name = format!("property-{property}-disabled");
        report.frame(&name, WIDTH, HEIGHT, &candidate)?;
        report.compare(&name, &baseline, &candidate, true)?;
    }
    for group in 1..=6 {
        let mut choices = choices.clone();
        choices.insert(format!("_{group}opacity"), json!(0));
        let prepared =
            PreparedScene::prepare_with_properties(project, Some(assets_root), &choices)?;
        let mut renderer = SceneRenderer::initialize(&prepared)?;
        let candidate = render_snapshot(&mut renderer, &target, positive)?;
        let name = format!("property-opacity-group-{group}-zero");
        report.frame(&name, WIDTH, HEIGHT, &candidate)?;
        report.compare(&name, &baseline, &candidate, true)?;
    }
    let mut isolated = choices.clone();
    for group in 1..=6 {
        isolated.insert(format!("_{group}opacity"), json!(0));
    }
    for property in ["chromaticabberation", "pulse", "bloom"] {
        isolated.insert(property.to_owned(), json!(false));
    }
    isolated.insert("schemecolor".to_owned(), json!([0.125, 0.375, 0.625]));
    let prepared = PreparedScene::prepare_with_properties(project, Some(assets_root), &isolated)?;
    let mut renderer = SceneRenderer::initialize(&prepared)?;
    let background = render_snapshot(&mut renderer, &target, silence)?;
    for pixel in background.as_chunks::<4>().0.iter() {
        ensure!(
            pixel
                .iter()
                .zip([32, 96, 159, 255])
                .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
            "Original scheme color property did not reach framebuffer: {pixel:?}"
        );
    }
    report.frame("isolated-scheme-color", WIDTH, HEIGHT, &background)?;
    report.value["isolated_scheme_color_pixel"] = json!(&background[..4]);
    check_gl("completed original Space pixel acceptance")?;
    Ok(())
}

#[test]
#[ignore = "requires explicit original 2.5D Space assets and EGL/OpenGL; offscreen GPU pixels only"]
fn original_space_layers_effects_properties_and_paused_frames() -> Result<()> {
    use khronos_egl as egl;
    let project = PathBuf::from(
        std::env::var_os("LWE_SPACE_TEST_PROJECT").context("Set LWE_SPACE_TEST_PROJECT")?,
    );
    let assets = PathBuf::from(
        std::env::var_os("LWE_SCENE_TEST_ASSETS").context("Set LWE_SCENE_TEST_ASSETS")?,
    );
    let mut report = Report::new()?;
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
        .context("No EGL configuration")?;
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
        "vendor":driver_string(gl::VENDOR)?, "renderer":driver_string(gl::RENDERER)?,
        "version":driver_string(gl::VERSION)?,
    });
    let result = acceptance(&project, &assets, &mut report);
    report.save(&result)?;
    instance.make_current(display, None, None, None)?;
    instance.destroy_surface(display, surface)?;
    instance.destroy_context(display, context)?;
    instance.terminate(display)?;
    result
}

#[test]
#[ignore = "requires an installed Wallpaper Engine font and explicit offscreen EGL"]
fn original_font_text_input_and_script_options_match_literal_frames() -> Result<()> {
    use super::super::tests::Fixture;

    let assets = PathBuf::from(
        std::env::var_os("LWE_SCENE_TEST_ASSETS").context("Set LWE_SCENE_TEST_ASSETS")?,
    );
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("project.json"),
        br#"{"type":"scene","file":"scene.json","general":{"properties":{
            "caption":{"type":"textinput","value":"A"},
            "help":{"type":"text","text":"Caption information","value":""}
        }}}"#,
    )?;
    let script = r#"
        export var scriptProperties = createScriptProperties()
            .addText({name:"separator",label:"Separator",value:"/"})
            .addCombo({name:"format",label:"Format",options:[
                {label:"First",value:"1"},{label:"Second",value:"2"}
            ]})
            .finish();
        export function update(value) {
            return value === "" ? "" : value + scriptProperties.separator + scriptProperties.format;
        }
    "#;
    let object = |text| {
        json!([{
            "text":text,"origin":"8 8 0","size":"16 16",
            "font":"fonts/Atami-Regular.otf","pointsize":8,
            "horizontalalign":"center","verticalalign":"center"
        }])
    };
    let binding = json!({
        "user":"caption","value":"Fallback","script":script,
        "scriptproperties":{"separator":""}
    });
    let _context = super::sharing_tests::Offscreen::new()?;
    let target = RenderTarget::new(16, 16)?;
    let draw = |scene: &PreparedScene| -> Result<Vec<u8>> {
        let mut renderer = scene.initialize()?;
        bind_target(&target);
        let timeout = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !renderer.render(16, 16, 0.)? {
            ensure!(
                std::time::Instant::now() < timeout,
                "Scene text frame not ready"
            );
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        renderer.report_presented();
        pixels(&target)
    };
    let mut frames = Vec::new();
    for (override_value, expected_text) in [(None, "A1"), (Some("B"), "B1"), (Some(""), "")] {
        fixture.scene(object(binding.clone()));
        let overrides = override_value
            .map(|value| BTreeMap::from([("caption".to_owned(), json!(value))]))
            .unwrap_or_default();
        let prepared =
            PreparedScene::prepare_with_properties(&fixture.0, Some(&assets), &overrides)?;
        let actual = draw(&prepared)?;

        fixture.scene(object(json!(expected_text)));
        let literal = PreparedScene::prepare(&fixture.0, Some(&assets))?;
        assert_eq!(actual, draw(&literal)?);
        frames.push(actual);
    }
    assert_ne!(frames[0], frames[1]);
    assert!(
        frames[0]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] != 0)
    );
    assert!(
        frames[2]
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [0; 3])
    );
    Ok(())
}
