use fynkel_world::{ImageData, Material, Quat, Vec2};

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
    world.insert(cube, MeshRenderer { mesh: Some(mesh), material: Some(material) }).unwrap();
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

    let sun = scene.sun.unwrap();
    assert!(close(sun.direction, Vec3::new(-0.4, -1.0, -0.3).normalize()), "{:?}", sun.direction);
    assert_eq!(sun.color, Vec3::ONE);
    assert!(close(scene.ambient, Vec3::splat(0.15)));

    let [draw] = scene.draws[..] else { panic!("{:?}", scene.draws) };
    assert_eq!(draw.mesh, mesh_key(mesh));
    assert!(draw.model.abs_diff_eq(Mat4::from_rotation_y(30f32.to_radians()), 1e-6));
    assert_eq!(draw.material.base_color, Color::from_argb32(0xFFE0613A).to_linear());
    assert_eq!((draw.material.metallic, draw.material.roughness), (0.0, 0.5));

    assert_eq!(uploads(&ops), [mesh_key(mesh)]);
}

#[test]
fn the_lowest_id_camera_and_sun_win_and_ambients_add_up() {
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
    assert_eq!(scene.sun.unwrap().color, Vec3::ONE);
    assert!(close(scene.ambient, Vec3::splat(0.75)));
}

#[test]
fn a_sun_without_a_transform_has_no_direction_and_is_ignored() {
    let mut world = World::new();
    let id = world.spawn();
    world.insert(id, Light::DEFAULT).unwrap();
    assert_eq!(extract(&world).0.sun, None);
}

#[test]
fn an_object_missing_a_mesh_material_or_transform_is_not_drawn() {
    let (mut world, mesh) = stage1_world();
    let material = world.add_material(Material::DEFAULT).unwrap();
    for renderer in
        [MeshRenderer { mesh: Some(mesh), material: None }, MeshRenderer { mesh: None, material: Some(material) }]
    {
        let id = world.spawn();
        world.insert(id, Transform::default()).unwrap();
        world.insert(id, renderer).unwrap();
    }
    let no_transform = world.spawn();
    world.insert(no_transform, MeshRenderer { mesh: Some(mesh), material: Some(material) }).unwrap();
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
            base_color_texture: Some(a),
            occlusion_texture: Some(b),
            ..Material::DEFAULT
        })
        .unwrap();
    let id = world.spawn();
    world.insert(id, Transform::default()).unwrap();
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material) }).unwrap();

    let (scene, _) = extract(&world);
    let m = scene.draws.iter().find(|d| d.material.uv_scale == [10.0; 2]).unwrap().material;
    assert_eq!(m.emissive, (Vec3::from(rgb(orange)) * 2.0).to_array());
    assert_eq!(m.alpha_cutoff, 0.5);
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
    assert!(close(scene.sun.unwrap().direction, Vec3::NEG_X), "{}", scene.sun.unwrap().direction);
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
    world.insert(parts[3], MeshRenderer { mesh: Some(sphere), material: Some(material) }).unwrap();
    world.set_active(rig, false).unwrap();

    let mut extract = Extract::default();
    let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(scene.camera, full.camera, "the rig's camera is left out");
    assert_ne!(world.active_camera(), Some(parts[0]), "and the world agrees");
    assert_eq!(scene.sun, full.sun, "and its sun");
    assert!(close(scene.ambient, full.ambient), "and its ambient light");
    assert_eq!(scene.draws.iter().map(|d| d.mesh).collect::<Vec<_>>(), [mesh_key(cube_mesh)], "and its sphere");

    world.set_active(rig, true).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert!((scene.camera.unwrap().fov_y - 20f32.to_radians()).abs() < 1e-6, "shown, the rig's camera wins");
    assert_eq!(world.active_camera(), Some(parts[0]));
    assert_eq!(scene.sun.unwrap().color, Vec3::splat(9.0), "and its sun");
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
/// `cargo test --release -p fynkel_ffi --lib -- --ignored --nocapture extract_probe`.
/// Prints the median and the slowest of 500 steady frames for each shape.
/// The last shape hides one tree in ten (stage 3 plan §3.3), so the flags
/// are looked up for every object.
#[test]
#[ignore = "a measurement, run by hand"]
fn extract_probe() {
    const N: usize = 10_000;
    let wide = (|i: usize| (!i.is_multiple_of(100)).then(|| i - i % 100)) as fn(usize) -> Option<usize>;
    for (name, parent_of, hidden) in [
        ("100 × 100 wide", wide, 0),
        ("100 chains 100 deep", |i| (!i.is_multiple_of(100)).then(|| i - 1), 0),
        ("no tree (stage 1)", |_| None, 0),
        ("wide, 1 in 10 hidden", wide, 1000),
    ] {
        let (mut world, _) = stage1_world();
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
        let first = std::time::Instant::now();
        extract.run(&world, &mut scene, &mut ops);
        let first = first.elapsed();
        assert_eq!(scene.draws.len(), N + 1 - hidden);
        let mut times: Vec<std::time::Duration> = (0..500)
            .map(|_| {
                ops.clear();
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
