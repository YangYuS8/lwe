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
