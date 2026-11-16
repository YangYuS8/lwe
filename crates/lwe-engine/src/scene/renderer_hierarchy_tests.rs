//! Pixel regression for final placement on both renderer paths.

use super::super::hierarchy::Transform;
use super::*;

fn render_placement(hierarchy: bool, staged: bool, output: [i32; 2]) -> Result<Vec<u8>> {
    let rgba = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ];
    let mut scene = sharing_tests::fixture(false, rgba.to_vec());
    scene.canvas = [32., 32.];
    let layer = &mut scene.layers[0];
    layer.size = [3., 5.];
    layer.stages[0].passes[0].label = "LWE image material".into();
    if hierarchy {
        // A parent's translation and 2× scale applied to a child at (2,3)
        // in its local space. The stored local position is intentionally wrong
        // for drawing, so dropping the parent transform fails the oracle.
        layer.origin = [2., 3.];
        layer.world_2d = Some(Transform {
            x: [2., 0.],
            y: [0., 2.],
            origin: [18., 20.],
        });
    } else {
        layer.origin = [18., 20.];
        layer.scale = [2., 2.];
    }
    if staged {
        layer.stages.push(PreparedStage {
            targets: BTreeMap::new(),
            persistent_targets: Default::default(),
            passes: vec![PreparedPass {
                kind: crate::scene::PreparedPassKind::Render,
                label: "authored child-local stage".into(),
                shader: Some(super::super::shader::image()),
                textures: vec![TextureBinding::Input],
                constants: BTreeMap::new(),
                target: None,
                blend: Blend::Replace,
            }],
        });
        scene.summary.pass_count += 1;
    }
    let [width, height] = output;
    let target = RenderTarget::new(width, height)?;
    unsafe { gl::BindFramebuffer(gl::FRAMEBUFFER, target.fbo) };
    let mut renderer = scene.initialize()?;
    assert_eq!(renderer.layers[0].direct, !staged);
    if staged {
        assert!(renderer.scratch.contains_key(&(3, 5)));
    }
    ensure!(
        renderer.render(width, height, 0.)?,
        "No hierarchy fixture frame"
    );
    renderer.report_presented();
    let snapshot = frozen_history::FrozenHistory::capture(
        &target,
        super::super::scene_texture_memory_budget(),
    )?;
    Ok(snapshot.pixels().to_vec())
}

#[test]
#[ignore = "requires an explicit EGL/OpenGL offscreen acceptance run"]
fn offscreen_parent_placement_matches_flattened_direct_and_staged_frames() -> Result<()> {
    let _context = sharing_tests::Offscreen::new()?;
    for output in [[32, 32], [48, 32]] {
        for staged in [false, true] {
            let flattened = render_placement(false, staged, output)?;
            assert!(
                flattened
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[0..3] != [0; 3])
            );
            assert_eq!(render_placement(true, staged, output)?, flattened);
        }
    }
    Ok(())
}

fn draw_accepted(
    renderer: &mut SceneRenderer,
    target: &RenderTarget,
    time: f64,
    force: bool,
) -> Result<Vec<u8>> {
    let width = target.texture.width;
    let height = target.texture.height;
    unsafe { gl::BindFramebuffer(gl::FRAMEBUFFER, target.fbo) };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let drawn = if force {
            renderer.render_forced(width, height, time)?
        } else {
            renderer.render(width, height, time)?
        };
        if drawn {
            break;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "Hierarchy worker did not produce a frame"
        );
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
    renderer.report_presented();
    let snapshot = frozen_history::FrozenHistory::capture(
        target,
        super::super::scene_texture_memory_budget(),
    )?;
    Ok(snapshot.pixels().to_vec())
}

fn assert_child_frame(renderer: &SceneRenderer, rgba: &[u8], sequence: u64, x: f32, visible: bool) {
    assert_eq!(renderer.accepted_seq, sequence);
    assert_eq!(renderer.layers[0].world_2d.unwrap().origin, [x, 8.]);
    assert_eq!(!renderer.hidden_layers.contains(&0), visible);
    assert_eq!(
        rgba.as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > 200 && p[1] == 0),
        visible,
    );
}

#[test]
#[ignore = "requires explicit EGL/OpenGL and a bounded SceneScript worker"]
fn offscreen_scripted_hierarchy_accepts_whole_frames_and_replays_accepted_pose() -> Result<()> {
    use super::super::tests::Fixture;
    use serde_json::json;

    let fixture = Fixture::new();
    let scripted = "let frame=0; export function update() { frame+=1; \
        thisLayer.visible=frame!==3; let z=frame>=5 ? 1 : 0; \
        return new Vec3(2+frame*2,8,z); }";
    fixture.scene(json!([
        {"id":3,"image":"models/image.json","parent":2,"origin":"1 0 0","size":"3 3"},
        {"id":2,"parent":1,"origin":"0 0 0"},
        {"id":1,"origin":{"value":"4 8 0","script":scripted}}
    ]));
    let scene = PreparedScene::prepare(&fixture.0, None)?;
    let _context = sharing_tests::Offscreen::new()?;
    let output = RenderTarget::new(16, 16)?;
    let wider = RenderTarget::new(24, 16)?;
    let mut renderer = scene.initialize()?;

    let initial = draw_accepted(&mut renderer, &output, 0., false)?;
    assert_child_frame(&renderer, &initial, 0, 5., true);
    let moved = draw_accepted(&mut renderer, &output, 0.25, false)?;
    assert_child_frame(&renderer, &moved, 1, 7., true);
    assert_ne!(initial, moved);
    assert_eq!(moved[(8 * 16 + 7) * 4], 255);

    // Submit the next script epoch, whose ancestor is hidden, without
    // accepting it. A paused resize must use the visible accepted pose.
    renderer.prepare_ahead(0.5, true)?;
    assert!(renderer.packet.is_some() || renderer.vm_requested);
    renderer.pause();
    renderer.freeze_for_rollback()?;
    renderer.thaw(true);
    let replay = draw_accepted(&mut renderer, &wider, 0.5, true)?;
    assert_child_frame(&renderer, &replay, 1, 7., true);
    assert_eq!(replay[(8 * 24 + 10) * 4], 255);

    renderer.resume();
    let hidden = draw_accepted(&mut renderer, &output, 0.75, false)?;
    assert_child_frame(&renderer, &hidden, 2, 9., false);
    let revealed = draw_accepted(&mut renderer, &output, 1., false)?;
    assert_child_frame(&renderer, &revealed, 3, 11., true);
    assert_eq!(revealed[(8 * 16 + 11) * 4], 255);

    let rejected = draw_accepted(&mut renderer, &output, 1.25, false)
        .expect_err("out-of-plane parent must fail the complete packet");
    assert!(format!("{rejected:#}").contains("planar"));
    assert_child_frame(&renderer, &revealed, 3, 11., true);
    assert_eq!(draw_accepted(&mut renderer, &output, 1.25, true)?, revealed);
    Ok(())
}
