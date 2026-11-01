use fynkel_world::{Blend, ImageData, Material, Quat, TextSettings, Vec2};

use super::*;

/// The stage 1 scene, built the way a game builds it.
fn stage1_world() -> (World, MeshId) {
    let mut world = World::new();
    let (_, mesh) = stage1_into(&mut world);
    (world, mesh)
}

/// Adds the stage 1 scene to `world`. Returns the cube and its mesh.
fn stage1_into(world: &mut World) -> (ObjectId, MeshId) {
    let camera = world.spawn();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, Transform::from_position(Vec3::new(0.0, 1.5, 4.0)).looking_at(Vec3::ZERO)).unwrap();
    let sun = world.spawn();
    world.insert(sun, Light::DEFAULT).unwrap();
    world.insert(sun, Transform::default().looking_at(Vec3::new(-0.4, -1.0, -0.3))).unwrap();
    let ambient = world.spawn();
    world.insert(ambient, Light::ambient(Color::WHITE, 0.15)).unwrap();
    let mesh = world.add_mesh(MeshData::cube());
    let material = world.add_material(Material::color(Color::from_argb32(0xFFE0613A))).unwrap();
    let cube = world.spawn();
    world
        .insert(cube, Transform { rotation: Quat::from_rotation_y(30f32.to_radians()), ..Transform::default() })
        .unwrap();
    world.insert(cube, MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() }).unwrap();
    (cube, mesh)
}

fn extract(world: &World) -> (RenderScene, Vec<ResourceOp>) {
    let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
    Extract::default().run(world, &mut scene, &mut ops);
    (scene, ops)
}

/// The keys of the uploads in `ops`, which must hold only uploads.
fn uploads(ops: &[ResourceOp]) -> Vec<MeshKey> {
    ops.iter()
        .map(|op| match op {
            ResourceOp::UploadMesh(key, _) => *key,
            other => panic!("expected only mesh uploads, got {other:?}"),
        })
        .collect()
}

/// The texture uploads in `ops`, which must hold only those.
fn texture_uploads(ops: &[ResourceOp]) -> Vec<(TextureKey, ColorSpace)> {
    ops.iter()
        .map(|op| match op {
            ResourceOp::UploadTexture(key, space, _) => (*key, *space),
            other => panic!("expected only texture uploads, got {other:?}"),
        })
        .collect()
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.abs_diff_eq(b, 1e-5)
}

#[test]
fn the_stage_1_scene_is_extracted_resolved_and_linear() {
    let (world, mesh) = stage1_world();
    let (scene, ops) = extract(&world);

    let camera = scene.camera.unwrap();
    let expected = CameraView::looking_at(Vec3::new(0.0, 1.5, 4.0), Vec3::ZERO);
    assert!(camera.view.abs_diff_eq(expected.view, 1e-5), "{:?}", camera.view);
    assert_eq!(camera.position, expected.position);
    assert!((camera.fov_y - 60f32.to_radians()).abs() < 1e-6);
    assert_eq!((camera.near, camera.far), (0.1, 1000.0));
    assert_eq!(camera.clear_color, rgb(Camera::default().clear_color));
    assert_eq!(camera.orthographic, None, "perspective unless asked");
    assert_eq!(camera.bloom, Some(fynkel_render::Bloom { strength: 1.0, threshold: 1.0 }), "bloom unless asked");
    assert_eq!(
        camera.ambient_occlusion,
        Some(fynkel_render::AmbientOcclusion { intensity: 1.0, radius: 0.5 }),
        "and ambient occlusion"
    );

    let (direction, color) = scene.sun().unwrap();
    assert!(close(direction, Vec3::new(-0.4, -1.0, -0.3).normalize()), "{direction:?}");
    assert_eq!(color, Vec3::ONE);
    assert!(close(scene.ambient, Vec3::splat(0.15)));

    let [draw] = scene.draws[..] else { panic!("{:?}", scene.draws) };
    assert_eq!(draw.mesh, mesh_key(mesh));
    assert!(draw.model.abs_diff_eq(Mat4::from_rotation_y(30f32.to_radians()), 1e-6));
    assert_eq!(draw.material.base_color, Color::from_argb32(0xFFE0613A).to_linear());
    assert_eq!((draw.material.metallic, draw.material.roughness), (0.0, 0.5));

    assert_eq!(uploads(&ops), [mesh_key(mesh)]);
}

#[test]
fn the_lowest_id_camera_wins_suns_come_in_id_order_and_ambients_add_up() {
    let mut world = World::new();
    let ids: Vec<ObjectId> = (0..4).map(|_| world.spawn()).collect();
    // Inserted in reverse, so the ECS's own order isn't the id order.
    for (i, &id) in ids.iter().enumerate().rev() {
        let camera = Camera { fov_y: 10.0 * (i + 1) as f32, ..Camera::default() };
        world.insert(id, camera).unwrap();
        world.insert(id, Transform::from_position(Vec3::X * i as f32)).unwrap();
        world.insert(id, Light::directional(Color::WHITE, (i + 1) as f32)).unwrap();
    }
    for intensity in [0.25, 0.5] {
        let id = world.spawn();
        world.insert(id, Light::ambient(Color::WHITE, intensity)).unwrap();
    }
    let (scene, _) = extract(&world);
    assert!((scene.camera.unwrap().fov_y - 10f32.to_radians()).abs() < 1e-6);
    assert_eq!(scene.sun().unwrap().1, Vec3::ONE, "the first sun is the lowest id's");
    let colors: Vec<f32> = scene.lights.iter().map(|l| l.color.x).collect();
    assert_eq!(colors, [1.0, 2.0, 3.0, 4.0], "every sun is in the list, by id");
    assert!(close(scene.ambient, Vec3::splat(0.75)));
}

/// Point and spot lights go with the suns, in id order: placed by their
/// world transform, but a parent's scale doesn't stretch their range. A
/// spot's angle goes from degrees to radians.
#[test]
fn point_and_spot_lights_are_placed_by_their_world_transform() {
    use fynkel_render::LightKind as Kind;
    let mut world = World::new();
    let rig = world.spawn();
    let rig_transform = Transform {
        position: Vec3::new(10.0, 0.0, 0.0),
        rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        scale: Vec3::splat(3.0),
    };
    world.insert(rig, rig_transform).unwrap();
    let (lamp, spot, sun) = (world.spawn(), world.spawn(), world.spawn());
    world.insert(lamp, Light::point(Color::WHITE, 2.0, 5.0)).unwrap();
    world.insert(lamp, Transform::from_position(Vec3::new(0.0, 1.0, 0.0))).unwrap();
    world.insert(spot, Light::spot(Color::WHITE, 3.0, 8.0, 40.0, 0.5)).unwrap();
    world.insert(spot, Transform::from_position(Vec3::new(0.0, 0.0, 1.0))).unwrap();
    world.set_parent(spot, Some(rig)).unwrap();
    world.insert(sun, Light::DEFAULT).unwrap();
    world.insert(sun, Transform::default()).unwrap();

    let (scene, _) = extract(&world);
    let [lamp, spot, sun] = scene.lights[..] else { panic!("three lights: {:?}", scene.lights) };
    assert_eq!(lamp.kind, Kind::Point { position: Vec3::new(0.0, 1.0, 0.0), range: 5.0 });
    assert_eq!(lamp.color, Vec3::splat(2.0));
    let Kind::Spot { position, direction, range, angle, softness } = spot.kind else { panic!("{spot:?}") };
    // One unit behind the rig (+Z), times 3, turned a quarter left: +X.
    assert!(close(position, Vec3::new(13.0, 0.0, 0.0)), "{position}");
    assert!(close(direction, Vec3::NEG_X), "{direction}");
    assert_eq!((range, softness), (8.0, 0.5), "the rig's scale leaves the range alone");
    assert!((angle - 40f32.to_radians()).abs() < 1e-6);
    assert!(matches!(sun.kind, Kind::Sun { .. }));
}

#[test]
fn the_worlds_quality_level_goes_with_its_scene() {
    let (world, _) = stage1_world();
    let mut extract = Extract::default();
    let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(scene.quality, None, "the device's, unless the world picks");
    extract.quality = Some(Quality::LOW);
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(scene.quality, Some(Quality::LOW));
}

#[test]
fn a_sun_without_a_transform_has_no_direction_and_is_ignored() {
    let mut world = World::new();
    let id = world.spawn();
    world.insert(id, Light::DEFAULT).unwrap();
    assert_eq!(extract(&world).0.lights, []);
}

#[test]
fn an_object_missing_a_mesh_material_or_transform_is_not_drawn() {
    let (mut world, mesh) = stage1_world();
    let material = world.add_material(Material::DEFAULT).unwrap();
    for renderer in [
        MeshRenderer { mesh: Some(mesh), material: None, ..Default::default() },
        MeshRenderer { mesh: None, material: Some(material), ..Default::default() },
    ] {
        let id = world.spawn();
        world.insert(id, Transform::default()).unwrap();
        world.insert(id, renderer).unwrap();
    }
    let no_transform = world.spawn();
    world
        .insert(no_transform, MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() })
        .unwrap();
    assert_eq!(extract(&world).0.draws.len(), 1, "only the cube");
}

#[test]
fn each_mesh_is_uploaded_once_and_removed_when_the_world_removes_it() {
    let (mut world, cube) = stage1_world();
    let (mut extract, mut scene, mut ops) = (Extract::default(), RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(uploads(&ops), [mesh_key(cube)]);
    let ResourceOp::UploadMesh(_, shared) = &ops[0] else { unreachable!() };
    assert!(Arc::ptr_eq(shared, world.mesh(cube).unwrap()), "the world's data, not a copy");

    ops.clear();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [], "nothing changed");

    let sphere = world.add_mesh(MeshData::sphere(8, 4));
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(uploads(&ops), [mesh_key(sphere)]);

    ops.clear();
    world.remove_mesh(sphere).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [ResourceOp::RemoveMesh(mesh_key(sphere))]);
}

#[test]
fn a_changed_mesh_goes_up_again_and_a_change_still_waiting_is_replaced() {
    let (mut world, cube) = stage1_world();
    let (mut extract, mut scene, mut ops) = (Extract::default(), RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);
    ops.clear();

    let plane = Arc::new(MeshData::plane());
    world.set_mesh(cube, Arc::clone(&plane)).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(uploads(&ops), [mesh_key(cube)]);

    // Not taken by the worker yet: the next change takes its place.
    let sphere = Arc::new(MeshData::sphere(8, 4));
    world.set_mesh(cube, Arc::clone(&sphere)).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(uploads(&ops), [mesh_key(cube)], "one upload, not two");
    let ResourceOp::UploadMesh(_, waiting) = &ops[0] else { unreachable!() };
    assert!(Arc::ptr_eq(waiting, &sphere), "the latest");

    ops.clear();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [], "nothing changed since");
}

#[test]
fn each_submesh_after_the_first_is_drawn_with_its_own_material() {
    let mut world = World::new();
    let (cube, mesh) = stage1_into(&mut world);
    let blue = world.add_material(Material::color(Color::from_argb32(0xFF2040E0))).unwrap();
    let green = world.add_material(Material::color(Color::from_argb32(0xFF20E040))).unwrap();
    world.set_submesh_materials(cube, &[blue, green]).unwrap();
    let draws = extract(&world).0.draws;
    assert_eq!(
        draws.iter().map(|d| (d.mesh, d.submesh)).collect::<Vec<_>>(),
        [(mesh_key(mesh), 0), (mesh_key(mesh), 1), (mesh_key(mesh), 2)]
    );
    assert_eq!(draws[1].material.base_color, Color::from_argb32(0xFF2040E0).to_linear());
    assert_eq!(draws[1].model, draws[0].model);
    assert_eq!(draws[2].material.base_color, Color::from_argb32(0xFF20E040).to_linear());
}

#[test]
fn a_new_extract_uploads_every_mesh_the_world_has() {
    let (mut world, _) = stage1_world();
    world.add_mesh(MeshData::plane());
    assert_eq!(extract(&world).1.len(), 2);
}

// --- Textures (stage 2 plan §3.2) ---------------------------------------------

fn image() -> ImageData {
    ImageData::from_rgba(1, 1, vec![255; 4]).unwrap()
}

#[test]
fn a_material_is_extracted_with_its_textures_and_linear_emission() {
    let (mut world, mesh) = stage1_world();
    let (a, b) = (world.add_texture(image()), world.add_texture(image()));
    let orange = Color::from_argb32(0xFFE0613A);
    let material = world
        .add_material(Material {
            emissive: orange,
            emissive_intensity: 2.0,
            uv_scale: Vec2::splat(10.0),
            alpha_cutoff: 0.5,
            blend: Blend::Additive,
            base_color_texture: Some(a),
            occlusion_texture: Some(b),
            ..Material::DEFAULT
        })
        .unwrap();
    let id = world.spawn();
    world.insert(id, Transform::default()).unwrap();
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() }).unwrap();

    let (scene, _) = extract(&world);
    let m = scene.draws.iter().find(|d| d.material.uv_scale == [10.0; 2]).unwrap().material;
    assert_eq!(m.emissive, (Vec3::from(rgb(orange)) * 2.0).to_array());
    assert_eq!(m.alpha_cutoff, 0.5);
    assert_eq!(m.blend, fynkel_render::Blend::Additive);
    assert_eq!(
        m.textures,
        MaterialTextures {
            base_color: Some(texture_key(a)),
            occlusion: Some(texture_key(b)),
            ..MaterialTextures::NONE
        }
    );
}

#[test]
fn a_texture_is_uploaded_once_per_colour_space_a_material_reads_it_in() {
    let mut world = World::new();
    let texture = world.add_texture(image());
    world.add_texture(image());
    let (mut extract, mut scene, mut ops) = (Extract::default(), RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [], "a texture no material uses isn't uploaded");

    let base = world.add_material(Material { base_color_texture: Some(texture), ..Material::DEFAULT }).unwrap();
    // A second material reading it as colour again costs nothing more.
    world.add_material(Material { emissive_texture: Some(texture), ..Material::DEFAULT }).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    let key = texture_key(texture);
    assert_eq!(texture_uploads(&ops), [(key, ColorSpace::Srgb)]);
    let ResourceOp::UploadTexture(_, _, shared) = &ops[0] else { unreachable!() };
    assert!(Arc::ptr_eq(shared, world.texture(texture).unwrap()), "the world's pixels, not a copy");

    // Read as data too: a second upload, in the other space.
    ops.clear();
    world.set_material(base, Material { normal_texture: Some(texture), ..Material::DEFAULT }).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(texture_uploads(&ops), [(key, ColorSpace::Linear)]);

    ops.clear();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [], "nothing changed");
}

#[test]
fn a_texture_the_world_removes_is_freed_and_comes_back_if_added_again() {
    let mut world = World::new();
    let texture = world.add_texture(image());
    let material = world.add_material(Material { base_color_texture: Some(texture), ..Material::DEFAULT }).unwrap();
    let (mut extract, mut scene, mut ops) = (Extract::default(), RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);

    ops.clear();
    world.set_material(material, Material::DEFAULT).unwrap();
    world.remove_texture(texture).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [ResourceOp::RemoveTexture(texture_key(texture))]);

    // A new texture has a new id, so it can't be mistaken for the old one.
    ops.clear();
    let again = world.add_texture(image());
    world.set_material(material, Material { base_color_texture: Some(again), ..Material::DEFAULT }).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(texture_uploads(&ops), [(texture_key(again), ColorSpace::Srgb)]);
}

// --- Hierarchy (stage 2 plan §3.1) -------------------------------------------

#[test]
fn a_child_is_drawn_where_its_parent_puts_it_and_follows_it_next_frame() {
    let (mut world, _) = stage1_world();
    let mut cube = None;
    world.for_each::<MeshRenderer>(|id, _| cube = Some(id));
    let cube = cube.unwrap();
    let root = world.spawn();
    world.insert(root, Transform::from_position(Vec3::new(0.0, 2.0, 0.0))).unwrap();
    world.set_parent(cube, Some(root)).unwrap();

    let mut extract = Extract::default();
    let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);
    let expected = Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0)) * Mat4::from_rotation_y(30f32.to_radians());
    assert!(scene.draws[0].model.abs_diff_eq(expected, 1e-5), "{}", scene.draws[0].model);

    // Only the parent's transform changes: no change of parent.
    world.insert(root, Transform::from_position(Vec3::new(5.0, 0.0, 0.0))).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    let expected = Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0)) * Mat4::from_rotation_y(30f32.to_radians());
    assert!(scene.draws[0].model.abs_diff_eq(expected, 1e-5), "{}", scene.draws[0].model);
}

#[test]
fn the_camera_and_the_sun_follow_their_parents() {
    let mut world = World::new();
    let rig = world.spawn();
    world
        .insert(
            rig,
            Transform {
                position: Vec3::new(10.0, 0.0, 0.0),
                rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                // Scale on a parent mustn't stretch the view or the light.
                scale: Vec3::splat(3.0),
            },
        )
        .unwrap();
    let camera = world.spawn();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, Transform::from_position(Vec3::new(0.0, 0.0, 1.0))).unwrap();
    world.set_parent(camera, Some(rig)).unwrap();
    let sun = world.spawn();
    world.insert(sun, Light::DEFAULT).unwrap();
    world.insert(sun, Transform::default()).unwrap();
    world.set_parent(sun, Some(rig)).unwrap();

    let (scene, _) = extract(&world);
    let camera = scene.camera.unwrap();
    // One unit behind the rig (+Z), times 3, turned a quarter left: +X.
    assert!(close(camera.position, Vec3::new(13.0, 0.0, 0.0)), "{}", camera.position);
    let expected = CameraView::looking_at(Vec3::new(13.0, 0.0, 0.0), Vec3::new(12.0, 0.0, 0.0));
    assert!(camera.view.abs_diff_eq(expected.view, 1e-5), "{}", camera.view);
    // The rig faces -X, and so does the sun, with unit length.
    assert!(close(scene.sun().unwrap().0, Vec3::NEG_X), "{}", scene.sun().unwrap().0);
}

#[test]
fn an_inactive_object_and_everything_below_it_are_left_out() {
    let (world, _) = stage1_world();
    let (full, _) = extract(&world);

    // A rig with a camera, a sun, an ambient light and a sphere below it,
    // spawned first, so its camera and sun have the lowest ids.
    let mut world = World::new();
    let rig = world.spawn();
    let parts: [ObjectId; 4] = std::array::from_fn(|_| world.spawn());
    let (cube, cube_mesh) = stage1_into(&mut world);
    for part in parts {
        world.set_parent(part, Some(rig)).unwrap();
    }
    let sphere = world.add_mesh(MeshData::sphere(8, 4));
    let material = world.add_material(Material::default()).unwrap();
    world.insert(parts[0], Camera { fov_y: 20.0, ..Camera::default() }).unwrap();
    world.insert(parts[0], Transform::from_position(Vec3::new(9.0, 9.0, 9.0))).unwrap();
    world.insert(parts[1], Light { intensity: 9.0, ..Light::DEFAULT }).unwrap();
    world.insert(parts[1], Transform::default().looking_at(Vec3::X)).unwrap();
    world.insert(parts[2], Light::ambient(Color::WHITE, 9.0)).unwrap();
    world.insert(parts[3], Transform::default()).unwrap();
    world
        .insert(parts[3], MeshRenderer { mesh: Some(sphere), material: Some(material), ..Default::default() })
        .unwrap();
    world.set_active(rig, false).unwrap();

    let mut extract = Extract::default();
    let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(scene.camera, full.camera, "the rig's camera is left out");
    assert_ne!(world.active_camera(), Some(parts[0]), "and the world agrees");
    assert_eq!(scene.lights, full.lights, "and its sun");
    assert!(close(scene.ambient, full.ambient), "and its ambient light");
    assert_eq!(scene.draws.iter().map(|d| d.mesh).collect::<Vec<_>>(), [mesh_key(cube_mesh)], "and its sphere");

    world.set_active(rig, true).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert!((scene.camera.unwrap().fov_y - 20f32.to_radians()).abs() < 1e-6, "shown, the rig's camera wins");
    assert_eq!(world.active_camera(), Some(parts[0]));
    assert_eq!(scene.sun().unwrap().1, Vec3::splat(9.0), "and its sun");
    assert!(close(scene.ambient, Vec3::splat(9.15)));
    assert_eq!(scene.draws.len(), 2);

    // One part on its own, and a loose object.
    world.set_active(parts[3], false).unwrap();
    world.set_active(cube, false).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert!(scene.draws.is_empty(), "{:?}", scene.draws);
    assert!(scene.camera.is_some(), "the rest of the rig is still shown");
}

/// Probe for the stage 2 budget (§3.1): 10,000 drawn objects extract in
/// under 1 ms on the iPhone. Run it in release, on the Mac or a phone:
/// `cargo test --release -p fynkel_ffi --lib -- --ignored --nocapture bench_extract`.
/// Prints the median and the slowest of 500 steady frames for each shape,
/// and holds the median to the Mac's bar (stage 14's GT1).
/// Each frame one object is moved, so every world matrix is worked out
/// again, as in a game: a frame where nothing moved skips that (WO4).
/// The fourth shape hides one tree in ten (stage 3 plan §3.3), so the flags
/// are looked up for every object. The last draws nothing (a culling mask
/// of 0): the gap to the first is all culling in the extract could save,
/// the most (stage 7 phase 6).
#[test]
#[ignore = "a benchmark: run in release"]
fn bench_extract() {
    const N: usize = 10_000;
    let wide = (|i: usize| (!i.is_multiple_of(100)).then(|| i - i % 100)) as fn(usize) -> Option<usize>;
    for (name, parent_of, hidden, mask, bar_ms) in [
        ("100 × 100 wide", wide, 0, u32::MAX, 0.6),
        ("100 chains 100 deep", |i| (!i.is_multiple_of(100)).then(|| i - 1), 0, u32::MAX, 0.6),
        ("no tree (stage 1)", |_| None, 0, u32::MAX, 0.3),
        ("wide, 1 in 10 hidden", wide, 1000, u32::MAX, 0.6),
        ("wide, nothing drawn", wide, 0, 0, 0.25),
    ] {
        let (mut world, _) = stage1_world();
        let mut lens = None;
        world.for_each::<Camera>(|id, c| lens = Some((id, *c)));
        let (camera, lens) = lens.unwrap();
        world.insert(camera, Camera { culling_mask: mask, ..lens }).unwrap();
        let mut renderer = None;
        world.for_each::<MeshRenderer>(|_, r| renderer = Some(*r));
        let ids: Vec<ObjectId> = (0..N)
            .map(|i| {
                let id = world.spawn();
                let t = Transform {
                    position: Vec3::new(0.0, 0.01, 0.1),
                    rotation: Quat::from_rotation_y(i as f32 * 1e-3),
                    scale: Vec3::ONE,
                };
                world.insert(id, t).unwrap();
                world.insert(id, renderer.unwrap()).unwrap();
                id
            })
            .collect();
        for (i, &id) in ids.iter().enumerate() {
            if let Some(p) = parent_of(i) {
                world.set_parent(id, Some(ids[p])).unwrap();
            }
        }
        for &root in ids.iter().step_by(1000) {
            world.set_active(root, hidden == 0).unwrap();
        }

        let mut extract = Extract::default();
        let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
        let moved = world.get::<Transform>(ids[1]).unwrap();
        let first = std::time::Instant::now();
        extract.run(&world, &mut scene, &mut ops);
        let first = first.elapsed();
        assert_eq!(scene.draws.len(), if mask == 0 { 0 } else { N + 1 - hidden });
        let mut times: Vec<std::time::Duration> = (0..500)
            .map(|_| {
                ops.clear();
                world.insert(ids[1], moved).unwrap();
                let t = std::time::Instant::now();
                extract.run(&world, &mut scene, &mut ops);
                t.elapsed()
            })
            .collect();
        times.sort();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
        println!(
            "{name:<20} first {:>6.3} ms   median {:>6.3} ms   slowest {:>6.3} ms",
            ms(first),
            ms(times[times.len() / 2]),
            ms(times[times.len() - 1])
        );
        fynkel_bench::bench(&format!("extracting 10,000 objects, {name}"), 500, bar_ms, || {
            ops.clear();
            world.insert(ids[1], moved).unwrap();
            extract.run(&world, &mut scene, &mut ops);
        });
    }
}

#[test]
fn an_orthographic_camera_is_extracted_with_its_size() {
    let (mut world, _) = stage1_world();
    let mut id = None;
    world.for_each::<Camera>(|i, _| id = Some(i));
    let camera = Camera { orthographic: true, size: 2.5, ..Camera::default() };
    world.insert(id.unwrap(), camera).unwrap();
    let (scene, _) = extract(&world);
    assert_eq!(scene.camera.unwrap().orthographic, Some(2.5));
}

#[test]
fn only_layers_in_the_cameras_culling_mask_are_drawn_and_lights_light_them_all() {
    let mut world = World::new();
    let (cube, _) = stage1_into(&mut world);
    let camera = world.active_camera().unwrap();
    world.set_layer(cube, 4).unwrap();
    // A light on layer 4 still lights everything.
    let ambient = world.spawn();
    world.insert(ambient, Light::ambient(Color::WHITE, 1.0)).unwrap();
    world.set_layer(ambient, 4).unwrap();

    let mut extract = Extract::default();
    let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
    for (mask, drawn) in [(u32::MAX, 1), (!(1 << 4), 0), (1 << 4, 1), (1, 0), (0, 0)] {
        let lens = world.get::<Camera>(camera).unwrap();
        world.insert(camera, Camera { culling_mask: mask, ..lens }).unwrap();
        extract.run(&world, &mut scene, &mut ops);
        assert_eq!(scene.draws.len(), drawn, "mask {mask:#x}");
        assert!(close(scene.ambient, Vec3::splat(1.15)), "mask {mask:#x}");
        assert!(scene.camera.is_some() && scene.sun().is_some());
    }
}

#[test]
fn the_worlds_debug_lines_are_copied_with_linear_colours() {
    let (mut world, _) = stage1_world();
    let (scene, _) = extract(&world);
    assert!(scene.lines.is_empty());
    let grey = Color::rgba(0.5, 0.5, 0.5, 0.25);
    world.set_debug_lines([fynkel_world::DebugLine { a: Vec3::ZERO, b: Vec3::X, color: grey }]);
    let (scene, _) = extract(&world);
    assert_eq!(scene.lines, [Line { a: Vec3::ZERO, b: Vec3::X, color: grey.to_linear() }]);
    assert!(scene.lines[0].color[0] < 0.25, "linear: sRGB 0.5 is about 0.21");
    assert_eq!(scene.lines[0].color[3], 0.25, "alpha as given");
}

/// Which lights cast shadows, and which renderers cast and receive them,
/// reach the scene as the world has them.
#[test]
fn shadow_settings_reach_the_scene() {
    let mut world = World::new();
    let (cube, _) = stage1_into(&mut world);
    let (scene, _) = extract(&world);
    assert!(!scene.lights[0].shadows, "a light casts none unless asked");
    assert!(scene.draws[0].cast_shadows && scene.draws[0].receive_shadows, "a renderer does both unless told");

    let mut sun = None;
    world.for_each::<Light>(|id, l| {
        if l.kind == LightKind::Directional {
            sun = Some(id);
        }
    });
    let sun = sun.unwrap();
    world.insert(sun, Light::DEFAULT.with_shadows(true)).unwrap();
    let mut r = world.get::<MeshRenderer>(cube).unwrap();
    (r.cast_shadows, r.receive_shadows) = (false, false);
    world.insert(cube, r).unwrap();
    let (scene, _) = extract(&world);
    assert!(scene.lights[0].shadows);
    assert!(!scene.draws[0].cast_shadows && !scene.draws[0].receive_shadows);
}

#[test]
fn a_renderer_s_own_color_replaces_its_material_s_for_it_alone() {
    let mut world = World::new();
    let (cube, mesh) = stage1_into(&mut world);
    let material = world.get::<MeshRenderer>(cube).unwrap().material;
    let other = world.spawn();
    world.insert(other, Transform::from_position(Vec3::new(1.0, 0.0, 0.0))).unwrap();
    let white = MeshRenderer { mesh: Some(mesh), material, own_color: true, color: Color::WHITE, ..Default::default() };
    world.insert(other, white).unwrap();

    let (scene, _) = extract(&world);
    let [plain, own] = [&scene.draws[0], &scene.draws[1]];
    assert_eq!(plain.material.base_color, Color::from_argb32(0xFFE0613A).to_linear(), "the shared material's");
    assert_eq!(own.material.base_color, [1.0; 4], "its own: white, for a flash");
    assert_eq!(own.material.roughness, plain.material.roughness, "the rest is the material's");
}

#[test]
fn a_camera_without_bloom_or_ambient_occlusion_has_neither() {
    let (mut world, _) = stage1_world();
    let mut camera = None;
    world.for_each::<Camera>(|id, _| camera = Some(id));
    let camera = camera.unwrap();
    let lens = Camera { bloom: false, ambient_occlusion: false, ..Camera::default() };
    world.insert(camera, lens).unwrap();
    let view = extract(&world).0.camera.unwrap();
    assert_eq!((view.bloom, view.ambient_occlusion), (None, None));

    let lens = Camera {
        bloom_strength: 0.3,
        bloom_threshold: 2.0,
        ambient_occlusion_intensity: 2.0,
        ambient_occlusion_radius: 1.5,
        ..Camera::default()
    };
    world.insert(camera, lens).unwrap();
    let view = extract(&world).0.camera.unwrap();
    assert_eq!(view.bloom, Some(fynkel_render::Bloom { strength: 0.3, threshold: 2.0 }));
    assert_eq!(view.ambient_occlusion, Some(fynkel_render::AmbientOcclusion { intensity: 2.0, radius: 1.5 }));
}

fn roboto() -> std::sync::Arc<[u8]> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/assets/fonts/Roboto-Regular.ttf")).unwrap().into()
}

/// The stage 1 world with a font and an object showing `text` with
/// `settings` at `at`, turned by `turn`.
fn with_text(
    text: &str,
    at: Vec3,
    turn: Quat,
    settings: impl Fn(FontId) -> fynkel_world::TextSettings,
) -> (World, ObjectId, FontId) {
    let (mut world, _) = stage1_world();
    let font = world.add_font(roboto()).unwrap();
    let id = world.spawn();
    world.insert(id, Transform { position: at, rotation: turn, ..Transform::default() }).unwrap();
    world.set_text_renderer(id, settings(font), text).unwrap();
    (world, id, font)
}

fn atlas_uploads(ops: &[ResourceOp]) -> Vec<TextureKey> {
    ops.iter().filter_map(|op| if let ResourceOp::UploadAtlas(key, _) = op { Some(*key) } else { None }).collect()
}

#[test]
fn a_text_is_its_letters_from_its_fonts_atlas_one_draw_a_font() {
    let (mut world, id, font) =
        with_text("AB", Vec3::new(1.0, 2.0, 0.0), Quat::IDENTITY, |f| TextSettings::new(f, 0.5));
    // A second text in the same font joins its draw.
    let other = world.spawn();
    world.insert(other, Transform::default()).unwrap();
    world.set_text_renderer(other, TextSettings::new(font, 0.5), "C").unwrap();
    let mut extract = Extract::default();
    let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(scene.glyphs.len(), 3);
    let draws: Vec<_> = scene.particle_draws.iter().filter(|d| matches!(d.mode, ParticleMode::Glyphs { .. })).collect();
    assert_eq!(draws.len(), 1, "one draw a font");
    let draw = draws[0];
    assert_eq!((draw.first, draw.count, draw.lit), (0, 3, false));
    assert_eq!(draw.material.textures.base_color, Some(atlas_key(font)));
    assert_eq!(draw.material.blend, fynkel_render::Blend::Transparent);
    assert_eq!(atlas_uploads(&ops), [atlas_key(font)], "its atlas goes up");

    // Placed where the object is, as the world laid them out.
    let quads = world.text_renderer(id).unwrap().quads();
    let mine: Vec<&Glyph> = scene.glyphs.iter().filter(|g| g.center.y > 1.0).collect();
    assert_eq!(mine.len(), 2);
    let [l, b, r, t] = quads[0].rect;
    let first = mine.iter().min_by(|a, b| a.center.x.total_cmp(&b.center.x)).unwrap();
    assert!(close(first.center, Vec3::new(1.0 + (l + r) * 0.5, 2.0 + (b + t) * 0.5, 0.0)));
    assert!(close(first.right, Vec3::new((r - l) * 0.5, 0.0, 0.0)));
    assert!(close(first.up, Vec3::new(0.0, (t - b) * 0.5, 0.0)));
    let atlas = world.font(font).unwrap().pixels();
    let [x, y, w, h] = quads[0].pixels.map(f32::from);
    let (aw, ah) = (atlas.width as f32, atlas.height as f32);
    assert_eq!(first.uv, [w / aw, h / ah, x / aw, y / ah]);
    assert_eq!(first.color, Color::WHITE.to_linear());

    // The same atlas isn't sent again; a new letter grows it, and it is.
    ops.clear();
    extract.run(&world, &mut scene, &mut ops);
    assert!(atlas_uploads(&ops).is_empty());
    world.set_text(other, "Q").unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(atlas_uploads(&ops), [atlas_key(font)]);
}

#[test]
fn texts_on_top_and_lit_ones_are_their_own_draws_and_hidden_ones_are_left_out() {
    let (mut world, id, font) = with_text("A", Vec3::ZERO, Quat::IDENTITY, |f| TextSettings::new(f, 0.5));
    for (on_top, lit) in [(true, false), (false, true)] {
        let o = world.spawn();
        world.insert(o, Transform::default()).unwrap();
        world.set_text_renderer(o, TextSettings { on_top, lit, ..TextSettings::new(font, 0.5) }, "B").unwrap();
    }
    let (scene, _) = extract(&world);
    let mut looks: Vec<_> = scene
        .particle_draws
        .iter()
        .filter_map(|d| match d.mode {
            ParticleMode::Glyphs { on_top } => Some((on_top, d.lit, d.count)),
            _ => None,
        })
        .collect();
    looks.sort_by_key(|l| (l.0, l.1));
    assert_eq!(looks, [(false, false, 1), (false, true, 1), (true, false, 1)]);
    world.set_active(id, false).unwrap();
    assert_eq!(extract(&world).0.glyphs.len(), 2, "a hidden text isn't drawn");
}

#[test]
fn a_text_facing_the_camera_turns_with_it_and_an_upright_one_only_round_up() {
    let camera_at = Vec3::new(0.0, 1.5, 4.0);
    let turned = Quat::from_rotation_y(1.0);
    let letter = |facing: fynkel_world::Facing| {
        let (world, ..) =
            with_text("I", Vec3::new(0.0, 0.0, 0.0), turned, |f| TextSettings { facing, ..TextSettings::new(f, 1.0) });
        extract(&world).0.glyphs[0]
    };
    let normal = |g: &Glyph| g.right.cross(g.up).normalize();

    let own = letter(fynkel_world::Facing::None);
    assert!(close(normal(&own), turned * Vec3::Z), "as the object turns");

    let facing = letter(fynkel_world::Facing::Camera);
    let camera = Transform::from_position(camera_at).looking_at(Vec3::ZERO).rotation;
    assert!(close(normal(&facing), camera * Vec3::Z), "square to the camera");
    assert!(close(facing.up.normalize(), camera * Vec3::Y), "its top the camera's up");

    let upright = letter(fynkel_world::Facing::Upright);
    assert!(upright.up.normalize().abs_diff_eq(Vec3::Y, 1e-5), "standing up");
    // Turned about its object's place, as a whole: level towards the
    // camera from there.
    let level = Vec3::new(camera_at.x, 0.0, camera_at.z).normalize();
    assert!(close(normal(&upright), level), "turned round up to the camera");

    let flat = letter(fynkel_world::Facing::Up);
    assert!(close(normal(&flat), Vec3::Y), "flat, facing up");
    assert!(close(flat.up.normalize(), turned * Vec3::NEG_Z), "its top where the object faces");
}

#[test]
fn a_text_with_a_shadow_draws_each_letter_twice_the_shadow_offset_behind_and_first() {
    let shadow = Color::rgba(0.0, 0.0, 0.0, 0.6);
    let (world, ..) = with_text("I", Vec3::ZERO, Quat::IDENTITY, |f| TextSettings {
        outline: 0.3,
        shadow_color: shadow,
        shadow_offset: Vec2::new(0.1, -0.2),
        shadow_softness: 0.5,
        ..TextSettings::new(f, 2.0)
    });
    let scene = extract(&world).0;
    let [a, b] = scene.glyphs[..] else { panic!("a letter and its shadow: {:?}", scene.glyphs) };
    let (letter, under) = if a.color == shadow.to_linear() { (b, a) } else { (a, b) };
    assert_eq!(under.color, shadow.to_linear());
    assert_eq!(under.outline_color, shadow.to_linear(), "the outline's shadow too");
    assert_eq!((under.outline, under.softness), (0.3, 0.5));
    assert_eq!((letter.right, letter.up), (under.right, under.up));
    // The offset is of the em: 0.1 and -0.2 of 2 m (and a few mm back).
    let moved = under.center - letter.center;
    assert!((moved.x - 0.2).abs() < 0.005 && (moved.y + 0.4).abs() < 0.005, "{moved}");
    // Behind as the camera sees it, so it's drawn first.
    let camera = Transform::from_position(Vec3::new(0.0, 1.5, 4.0)).looking_at(Vec3::ZERO).rotation;
    let forward = camera * Vec3::NEG_Z;
    assert!(under.center.dot(forward) > letter.center.dot(forward));
    let [draw] = &scene.particle_draws[..] else { panic!() };
    assert_eq!(draw.count, 2);

    let (plain, ..) = with_text("I", Vec3::ZERO, Quat::IDENTITY, |f| TextSettings::new(f, 2.0));
    assert_eq!(extract(&plain).0.glyphs.len(), 1, "no shadow unless its colour shows");
}
