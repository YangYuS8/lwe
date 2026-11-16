//! Authored FullFrame liveness fixtures; no Workshop assets or desktop backend.

use super::super::shader::{self, Shader};
use super::*;

fn pixels(target: &RenderTarget) -> Result<Vec<u8>> {
    Ok(frozen_history::FrozenHistory::capture(
        target,
        super::super::timed_runtime::scene_texture_memory_budget(),
    )?
    .pixels()
    .to_vec())
}

fn draw(renderer: &mut SceneRenderer, size: [i32; 2], time: f64, force: bool) -> Result<Vec<u8>> {
    let drawn = if force {
        renderer.render_forced(size[0], size[1], time)?
    } else {
        renderer.render(size[0], size[1], time)?
    };
    ensure!(drawn, "FullFrame fixture did not render");
    renderer.report_presented();
    pixels(
        renderer
            .front
            .as_ref()
            .context("FullFrame fixture has no presented cache")?,
    )
}

fn fragment(source: &str) -> Shader {
    let mut shader = shader::image();
    shader.fragment = source.into();
    shader
}

fn render(shader: Shader, textures: Vec<TextureBinding>, target: Option<&str>) -> PreparedPass {
    PreparedPass {
        kind: super::super::PreparedPassKind::Render,
        label: "authored FullFrame stage".into(),
        shader: Some(shader),
        textures,
        constants: BTreeMap::new(),
        target: target.map(str::to_owned),
        blend: Blend::Replace,
    }
}

fn copy(source: TextureBinding, target: &str) -> PreparedPass {
    PreparedPass {
        kind: super::super::PreparedPassKind::Copy { source },
        label: "authored FullFrame history copy".into(),
        shader: None,
        textures: Vec::new(),
        constants: BTreeMap::new(),
        target: Some(target.into()),
        blend: Blend::Replace,
    }
}

fn fixture(force_snapshot: bool, history: bool) -> PreparedScene {
    let mut scene = sharing_tests::fixture(false, [80, 120, 200, 255].repeat(4));
    scene.layers[0].blend = Blend::Replace;
    let mut second = sharing_tests::fixture(false, [255; 16].to_vec());
    let mut layer = second.layers.remove(0);
    layer.blend = Blend::Alpha;
    // The forced-snapshot baseline uses the identical LWE copy shader that
    // composite=None selects. It changes no pass math or work allocation.
    layer.composite = force_snapshot.then(shader::image);
    layer.stages[0].passes = vec![render(
        shader::image(),
        vec![TextureBinding::FullFrame],
        None,
    )];
    if history {
        scene.textures[0].rgba = vec![
            80, 120, 200, 255, 200, 40, 100, 128, 10, 240, 80, 64, 150, 25, 220, 255,
        ];
        layer.origin = [0.5, 0.5];
        layer.scale = [0.6, 0.6];
        layer.angle = 0.17;
        layer.tint = [0.75, 0.5, 1., 0.625];
        layer.stages[0].passes[0].shader = Some(fragment(
            "#version 130
in vec2 v_UV; out vec4 lwe_FragColor; uniform sampler2D g_Texture0;
void main(){if(v_UV.x < 0.4) discard;
vec4 color=texture(g_Texture0,v_UV*1.4-0.2); lwe_FragColor=vec4(color.bgr,color.a*0.5);}",
        ));
        layer.stages.push(PreparedStage {
            targets: BTreeMap::from([
                ("captured".into(), 1.),
                ("previous".into(), 1.),
                ("work".into(), 1.),
            ]),
            persistent_targets: std::collections::BTreeSet::from([
                "captured".into(),
                "previous".into(),
            ]),
            passes: vec![
                copy(TextureBinding::FullFrame, "captured"),
                render(
                    fragment(
                        "#version 130
in vec2 v_UV; out vec4 lwe_FragColor; uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1; uniform sampler2D g_Texture2; uniform float g_Time;
void main(){vec4 inputColor=texture(g_Texture0,v_UV);
vec4 previous=texture(g_Texture1,v_UV); vec4 screen=texture(g_Texture2,v_UV*0.8+0.1);
lwe_FragColor=mix(inputColor,previous,0.25)+screen*(0.125+g_Time*0.01);}",
                    ),
                    vec![
                        TextureBinding::Input,
                        TextureBinding::History("previous".into()),
                        TextureBinding::FullFrame,
                    ],
                    Some("work"),
                ),
                copy(TextureBinding::Named("work".into()), "previous"),
                render(
                    shader::image(),
                    vec![TextureBinding::Named("work".into())],
                    None,
                ),
            ],
        });
    }
    scene.layers.push(layer);
    scene.draw_order.push(super::super::DrawObject::Image(1));
    scene.summary.layer_count = 2;
    scene.summary.pass_count = if history { 6 } else { 2 };
    scene.summary.animated = true;
    scene
}

fn assert_histories_equal(candidate: &SceneRenderer, baseline: &SceneRenderer) -> Result<()> {
    for name in ["captured", "previous"] {
        assert_eq!(
            pixels(&candidate.layers[1].stages[1].targets[name])?,
            pixels(&baseline.layers[1].stages[1].targets[name])?,
            "History {name}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires an opt-in current EGL/OpenGL context"]
fn current_fullframe_matches_snapshot_with_history_discard_alpha_and_thaw() -> Result<()> {
    let _context = sharing_tests::Offscreen::new()?;
    let scene = fixture(false, true);
    let mut candidate = SceneRenderer::initialize(&scene)?;
    let mut baseline = SceneRenderer::initialize(&fixture(true, true))?;
    assert!(candidate.layers[1].can_sample_current_frame());
    assert!(!baseline.layers[1].can_sample_current_frame());
    // A stale snapshot must remain untouched on the borrowed-source path.
    // Pixel equality alone would also pass if both renderers still copied.
    {
        let guard = GlState::capture();
        guard.begin();
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, candidate.full_frame[1].fbo);
            gl::ClearColor(1., 0., 1., 1.);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        check_gl("seeding the unused FullFrame snapshot")?;
    }
    let current_id = candidate.full_frame[0].texture.id;
    let layer = &candidate.layers[1];
    assert!(
        candidate.scratch[&(2, 2)]
            .iter()
            .skip(layer.work_offset)
            .all(|target| target.texture.id != current_id)
    );
    assert!(
        layer
            .stages
            .iter()
            .flat_map(|stage| stage.targets.values())
            .all(|target| target.texture.id != current_id)
    );
    let mut last_history = None;
    let mut last_captured = None;
    for frame in 0..3 {
        for renderer in [&mut candidate, &mut baseline] {
            renderer.layers[0].tint = [0.5 + frame as f32 * 0.125, 0.75, 1., 1.];
        }
        let output = draw(&mut candidate, [2, 2], frame as f64 * 0.25, false)?;
        assert_eq!(
            output,
            draw(&mut baseline, [2, 2], frame as f64 * 0.25, false)?
        );
        assert_eq!(
            pixels(&candidate.full_frame[1])?,
            [255, 0, 255, 255].repeat(4)
        );
        assert_histories_equal(&candidate, &baseline)?;
        let stage = &candidate.layers[1].stages[1];
        let captured = pixels(&stage.targets["captured"])?;
        assert!(
            captured
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel != &captured[..4])
        );
        let pairs = output
            .as_chunks::<4>()
            .0
            .iter()
            .zip(captured.as_chunks::<4>().0);
        assert!(
            pairs.clone().any(|(output, source)| output == source),
            "Partial quad lost its background"
        );
        assert!(
            pairs.clone().any(|(output, source)| output != source),
            "Partial quad made no visible change"
        );
        if let Some(previous) = last_captured {
            assert_ne!(captured, previous);
        }
        last_captured = Some(captured);
        let history = pixels(&stage.targets["previous"])?;
        if let Some(previous) = last_history {
            assert_ne!(history, previous);
        }
        last_history = Some(history);
    }
    let seq = candidate.accepted_seq;
    let time = candidate.accepted_time;
    for renderer in [&mut candidate, &mut baseline] {
        renderer.pause();
    }
    assert_eq!(
        draw(&mut candidate, [4, 4], 1., true)?,
        draw(&mut baseline, [4, 4], 1., true)?
    );
    assert_histories_equal(&candidate, &baseline)?;
    for renderer in [&mut candidate, &mut baseline] {
        renderer.freeze_for_rollback()?;
        renderer.thaw(true);
    }
    assert_eq!(
        draw(&mut candidate, [6, 4], 2., true)?,
        draw(&mut baseline, [6, 4], 2., true)?
    );
    assert_histories_equal(&candidate, &baseline)?;
    assert_eq!(candidate.accepted_seq, seq);
    assert_eq!(candidate.accepted_time, time);
    assert_eq!(
        pixels(&candidate.layers[1].stages[1].targets["previous"])?,
        last_history.unwrap()
    );
    candidate.resume();
    baseline.resume();
    assert_eq!(
        draw(&mut candidate, [2, 2], 3., false)?,
        draw(&mut baseline, [2, 2], 3., false)?
    );
    assert_histories_equal(&candidate, &baseline)?;
    assert_eq!(candidate.gpu_payload_bytes(), baseline.gpu_payload_bytes());
    Ok(())
}

#[test]
#[ignore = "requires an opt-in current EGL/OpenGL context"]
fn direct_partial_work_and_color_composite_keep_snapshot_fallback() -> Result<()> {
    let _context = sharing_tests::Offscreen::new()?;
    for mode in ["direct", "small", "composite"] {
        let make = |force_snapshot| {
            let mut scene = fixture(force_snapshot, false);
            let layer = &mut scene.layers[1];
            match mode {
                "direct" => layer.stages[0].passes[0].label = "LWE image material".into(),
                "small" => layer.size = [1.; 2],
                "composite" => {
                    layer.composite = Some(fragment(
                        "#version 130
in vec2 v_UV; out vec4 lwe_FragColor; uniform sampler2D g_Texture0; uniform sampler2D g_Texture4;
void main(){lwe_FragColor=mix(texture(g_Texture0,v_UV),texture(g_Texture4,v_UV),0.5);}",
                    ))
                }
                _ => unreachable!(),
            }
            scene
        };
        let mut candidate = SceneRenderer::initialize(&make(false))?;
        let mut baseline = SceneRenderer::initialize(&make(true))?;
        assert!(
            !candidate.layers[1].can_sample_current_frame(),
            "Fallback {mode}"
        );
        for (frame, factor) in [1., 0.5].into_iter().enumerate() {
            for renderer in [&mut candidate, &mut baseline] {
                renderer.layers[0].tint = [factor, factor, factor, 1.];
            }
            assert_eq!(
                draw(&mut candidate, [2, 2], frame as f64 * 0.25, false)?,
                draw(&mut baseline, [2, 2], frame as f64 * 0.25, false)?,
                "Fallback {mode}"
            );
            let rgba = if frame == 0 {
                [80, 120, 200, 255]
            } else {
                [40, 60, 100, 255]
            };
            assert_eq!(
                pixels(&candidate.full_frame[1])?,
                rgba.repeat(4),
                "Snapshot {mode}"
            );
        }
    }
    Ok(())
}
