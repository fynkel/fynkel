//! Renders a core world's extract offscreen and checks pixels: the renderer
//! consumes `Extract` only, so this is the same path every host uses.

use fynkel_core::{Extract, World, WorldConfig};
use fynkel_render::{clear_rgba8, headless_device, render_offscreen, ExtractRenderer};
use fynkel_scene::Transform;
use glam::Vec3;

fn world_with_cube_at(position: Vec3) -> World {
    let mut world = World::new(WorldConfig::default());
    world
        .edit_gameplay(|edit| edit.spawn("", Transform::from_position(position), None))
        .unwrap();
    world.begin_frame(1.0 / 60.0);
    world.step();
    world
}

#[test]
fn extract_renders_cubes_where_the_world_puts_them() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let (w, h) = (256, 256);
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    let at = |px: &[[u8; 4]], x: u32, y: u32| px[(y * w + x) as usize];

    // Empty world: everything is the clear colour.
    let px = render_offscreen(&device, &queue, &mut renderer, &Extract::default());
    assert_eq!(renderer.instance_count(), 0);
    assert_eq!(at(&px, 128, 128), clear_rgba8());

    // One cube at the origin: centre lit, corner clear.
    let world = world_with_cube_at(Vec3::ZERO);
    let extract = world.extract();
    assert_eq!(extract.records.len(), 1);
    let px = render_offscreen(&device, &queue, &mut renderer, &extract);
    assert_eq!(renderer.instance_count(), 1);
    assert_ne!(at(&px, 128, 128), clear_rgba8(), "cube covers the centre");
    assert_eq!(at(&px, 2, 2), clear_rgba8(), "corner is background");

    // Move it far to +X: the centre is background again (the camera is fixed).
    let world = world_with_cube_at(Vec3::new(50.0, 0.0, 0.0));
    let px = render_offscreen(&device, &queue, &mut renderer, &world.extract());
    assert_eq!(at(&px, 128, 128), clear_rgba8());

    // Instance buffer growth: many cubes in one frame, no panic, count matches.
    let mut world = World::new(WorldConfig::default());
    world
        .edit_gameplay(|edit| {
            for i in 0..1000 {
                edit.spawn(
                    "",
                    Transform::from_position(Vec3::new((i % 40) as f32, 0.0, (i / 40) as f32)),
                    None,
                )?;
            }
            Ok(())
        })
        .unwrap();
    world.begin_frame(1.0 / 60.0);
    world.step();
    render_offscreen(&device, &queue, &mut renderer, &world.extract());
    // The grid runs past the fallback camera's view: what is off-screen is
    // culled, and every cube is accounted for either way.
    let stats = renderer.stats();
    assert_eq!(stats.instances + stats.culled, 1000);
    assert!(stats.culled > 0, "{stats:?}");
    assert_eq!(stats.draws, 1, "one (cube, placeholder) batch");
    renderer.frustum_culling = false;
    render_offscreen(&device, &queue, &mut renderer, &world.extract());
    assert_eq!(renderer.instance_count(), 1000);
    assert_eq!(renderer.stats().culled, 0);
}
