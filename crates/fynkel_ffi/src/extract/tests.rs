use fynkel_world::{Material, Quat};

use super::*;

/// The stage 1 scene, built the way a game builds it.
fn stage1_world() -> (World, MeshId) {
    let mut world = World::new();
    let camera = world.spawn();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, Transform::from_position(Vec3::new(0.0, 1.5, 4.0)).looking_at(Vec3::ZERO)).unwrap();
    let sun = world.spawn();
    world.insert(sun, Light::DEFAULT).unwrap();
    world.insert(sun, Transform::default().looking_at(Vec3::new(-0.4, -1.0, -0.3))).unwrap();
    let ambient = world.spawn();
    world.insert(ambient, Light::ambient(Color::WHITE, 0.15)).unwrap();
    let mesh = world.add_mesh(MeshSource::Cube);
    let material = world.add_material(Material::color(Color::from_argb32(0xFFE0613A))).unwrap();
    let cube = world.spawn();
    world
        .insert(cube, Transform { rotation: Quat::from_rotation_y(30f32.to_radians()), ..Transform::default() })
        .unwrap();
    world.insert(cube, MeshRenderer { mesh: Some(mesh), material: Some(material) }).unwrap();
    (world, mesh)
}

fn extract(world: &World) -> (RenderScene, Vec<MeshOp>) {
    let (mut scene, mut ops) = (RenderScene::default(), Vec::new());
    Extract::default().run(world, &mut scene, &mut ops);
    (scene, ops)
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

    let sun = scene.sun.unwrap();
    assert!(close(sun.direction, Vec3::new(-0.4, -1.0, -0.3).normalize()), "{:?}", sun.direction);
    assert_eq!(sun.color, Vec3::ONE);
    assert!(close(scene.ambient, Vec3::splat(0.15)));

    let [draw] = scene.draws[..] else { panic!("{:?}", scene.draws) };
    assert_eq!(draw.mesh, mesh_key(mesh));
    assert!(draw.model.abs_diff_eq(Mat4::from_rotation_y(30f32.to_radians()), 1e-6));
    assert_eq!(draw.material.base_color, Color::from_argb32(0xFFE0613A).to_linear());
    assert_eq!((draw.material.metallic, draw.material.roughness), (0.0, 0.5));

    assert_eq!(ops, [MeshOp::Upload(mesh_key(mesh), MeshSource::Cube)]);
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
    assert_eq!(ops, [MeshOp::Upload(mesh_key(cube), MeshSource::Cube)]);

    ops.clear();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [], "nothing changed");

    let sphere = world.add_mesh(MeshSource::Sphere);
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [MeshOp::Upload(mesh_key(sphere), MeshSource::Sphere)]);

    ops.clear();
    world.remove_mesh(sphere).unwrap();
    extract.run(&world, &mut scene, &mut ops);
    assert_eq!(ops, [MeshOp::Remove(mesh_key(sphere))]);
}

#[test]
fn a_new_extract_uploads_every_mesh_the_world_has() {
    let (mut world, _) = stage1_world();
    world.add_mesh(MeshSource::Plane);
    assert_eq!(extract(&world).1.len(), 2);
}
