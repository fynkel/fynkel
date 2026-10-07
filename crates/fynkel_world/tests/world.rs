//! The world's promises (stage 1 plan, step 1): ids are never reused, a
//! stale id is a clear error that never reaches another object, every
//! write is validated, and resources can't be removed while in use.

use std::collections::HashSet;

use fynkel_world::*;

fn world_with_cube() -> (World, MeshId, MaterialId) {
    let mut world = World::new();
    let mesh = world.add_mesh(MeshSource::Cube);
    let material = world.add_material(Material::default()).unwrap();
    (world, mesh, material)
}

// --- Ids -------------------------------------------------------------------

#[test]
fn ids_start_at_one_and_count_up() {
    let mut world = World::new();
    let ids: Vec<u64> = (0..3).map(|_| world.spawn().raw()).collect();
    assert_eq!(ids, [1, 2, 3]);
}

#[test]
fn ids_are_never_reused_after_despawn() {
    let mut world = World::new();
    let mut seen = HashSet::new();
    let mut live = Vec::new();
    // Spawn and despawn in a churning pattern, so hecs reuses its slots
    // many times over.
    for round in 0..1000 {
        let id = world.spawn();
        assert!(seen.insert(id), "id {id} handed out twice");
        live.push(id);
        if round % 3 != 0 {
            world.despawn(live.remove(0)).unwrap();
        }
    }
    assert_eq!(world.len(), live.len());
}

#[test]
fn a_despawned_id_is_a_clear_error() {
    let mut world = World::new();
    let id = world.spawn();
    world.insert(id, Transform::default()).unwrap();
    world.despawn(id).unwrap();

    assert!(!world.contains(id));
    assert_eq!(world.despawn(id), Err(WorldError::NoObject(id)));
    assert_eq!(world.get::<Transform>(id), Err(WorldError::NoObject(id)));
    assert_eq!(world.insert(id, Transform::default()), Err(WorldError::NoObject(id)));
    assert_eq!(world.remove::<Transform>(id), Err(WorldError::NoObject(id)));
    assert_eq!(world.has::<Transform>(id), Err(WorldError::NoObject(id)));
    assert_eq!(
        world.get::<Transform>(id).unwrap_err().to_string(),
        "object#1 does not exist (despawned, or never spawned)"
    );
}

#[test]
fn a_stale_id_never_reaches_a_new_object() {
    let mut world = World::new();
    let old = world.spawn();
    world.despawn(old).unwrap();
    // hecs hands this object the old one's slot.
    let new = world.spawn();
    world.insert(new, Transform::from_position(Vec3::X)).unwrap();

    assert_ne!(old, new);
    assert!(world.insert(old, Transform::from_position(Vec3::Y)).is_err());
    assert_eq!(world.get::<Transform>(new).unwrap().position, Vec3::X);
}

#[test]
fn an_id_that_was_never_spawned_is_an_error() {
    let world = World::new();
    let id = ObjectId::from_raw(42).unwrap();
    assert_eq!(world.get::<Camera>(id), Err(WorldError::NoObject(id)));
    assert_eq!(ObjectId::from_raw(0), None);
}

// --- Components ------------------------------------------------------------

#[test]
fn insert_get_replace_remove() {
    let mut world = World::new();
    let id = world.spawn();
    assert_eq!(world.has::<Camera>(id), Ok(false));
    assert_eq!(world.get::<Camera>(id), Err(WorldError::NoComponent { object: id, component: "Camera" }));

    world.insert(id, Camera::default()).unwrap();
    assert_eq!(world.has::<Camera>(id), Ok(true));
    assert_eq!(world.get::<Camera>(id), Ok(Camera::default()));

    let wide = Camera { fov_y: 90.0, ..Camera::default() };
    world.insert(id, wide).unwrap();
    assert_eq!(world.get::<Camera>(id).unwrap().fov_y, 90.0);

    assert_eq!(world.remove::<Camera>(id), Ok(wide));
    assert_eq!(world.has::<Camera>(id), Ok(false));
    assert_eq!(world.remove::<Camera>(id), Err(WorldError::NoComponent { object: id, component: "Camera" }));
    assert!(world.contains(id), "removing a component keeps the object");
}

#[test]
fn insert_validates_fields_written_directly() {
    let mut world = World::new();
    let id = world.spawn();
    world.insert(id, Transform::default()).unwrap();

    let nan = Transform { position: Vec3::new(f32::NAN, 0.0, 0.0), ..Transform::default() };
    assert_eq!(
        world.insert(id, nan),
        Err(WorldError::Field(FieldError::NotFinite { ty: "Transform", field: "position" }))
    );
    assert_eq!(world.get::<Transform>(id), Ok(Transform::default()), "the old value is kept");

    let skewed = Transform { rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 2.0), ..Transform::default() };
    assert!(matches!(world.insert(id, skewed), Err(WorldError::Field(FieldError::NotUnit { .. }))));

    let flat = Camera { fov_y: 0.0, ..Camera::default() };
    assert!(matches!(world.insert(id, flat), Err(WorldError::Field(FieldError::OutOfRange { .. }))));
    assert_eq!(world.has::<Camera>(id), Ok(false));
}

#[test]
fn a_mesh_renderer_must_point_at_existing_resources() {
    let (mut world, mesh, material) = world_with_cube();
    let id = world.spawn();

    let ghost_mesh = MeshId::from_raw(99).unwrap();
    let ghost_material = MaterialId::from_raw(99).unwrap();
    assert_eq!(
        world.insert(id, MeshRenderer { mesh: Some(ghost_mesh), material: Some(material) }),
        Err(WorldError::NoMesh(ghost_mesh))
    );
    assert_eq!(
        world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(ghost_material) }),
        Err(WorldError::NoMaterial(ghost_material))
    );
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material) }).unwrap();
    world.insert(id, MeshRenderer::default()).unwrap(); // empty is allowed: it draws nothing
}

#[test]
fn for_each2_visits_exactly_the_objects_with_both() {
    let (mut world, mesh, material) = world_with_cube();
    let renderer = MeshRenderer { mesh: Some(mesh), material: Some(material) };
    let both = world.spawn();
    world.insert(both, Transform::default()).unwrap();
    world.insert(both, renderer).unwrap();
    let only_transform = world.spawn();
    world.insert(only_transform, Transform::default()).unwrap();
    let only_renderer = world.spawn();
    world.insert(only_renderer, renderer).unwrap();

    let mut visited = Vec::new();
    world.for_each2::<Transform, MeshRenderer>(|id, _, r| {
        assert_eq!(*r, renderer);
        visited.push(id);
    });
    assert_eq!(visited, [both]);

    let mut transforms = 0;
    world.for_each::<Transform>(|_, _| transforms += 1);
    assert_eq!(transforms, 2);
}

// --- Resources -------------------------------------------------------------

#[test]
fn meshes_cannot_be_removed_while_in_use() {
    let (mut world, mesh, material) = world_with_cube();
    let id = world.spawn();
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material) }).unwrap();

    assert_eq!(world.remove_mesh(mesh), Err(WorldError::MeshInUse { mesh, object: id }));
    assert_eq!(world.mesh(mesh), Ok(MeshSource::Cube));

    world.despawn(id).unwrap();
    world.remove_mesh(mesh).unwrap();
    assert_eq!(world.mesh(mesh), Err(WorldError::NoMesh(mesh)));
    assert_eq!(world.remove_mesh(mesh), Err(WorldError::NoMesh(mesh)));
}

#[test]
fn materials_cannot_be_removed_while_in_use() {
    let (mut world, mesh, material) = world_with_cube();
    let id = world.spawn();
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material) }).unwrap();

    assert_eq!(world.remove_material(material), Err(WorldError::MaterialInUse { material, object: id }));
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: None }).unwrap();
    world.remove_material(material).unwrap();
    assert_eq!(world.material(material), Err(WorldError::NoMaterial(material)));
}

#[test]
fn resource_ids_are_never_reused() {
    let mut world = World::new();
    let a = world.add_mesh(MeshSource::Cube);
    world.remove_mesh(a).unwrap();
    let b = world.add_mesh(MeshSource::Cube);
    assert_ne!(a, b);

    let m = world.add_material(Material::default()).unwrap();
    world.remove_material(m).unwrap();
    assert_ne!(m, world.add_material(Material::default()).unwrap());
}

#[test]
fn materials_are_validated_and_can_change() {
    let mut world = World::new();
    let rough = Material { roughness: 2.0, ..Material::default() };
    assert!(matches!(world.add_material(rough), Err(WorldError::Field(FieldError::OutOfRange { .. }))));

    let id = world.add_material(Material::default()).unwrap();
    let red = Material::color(Color::from_argb32(0xFFFF0000));
    world.set_material(id, red).unwrap();
    assert_eq!(world.material(id), Ok(red));
    assert!(world.set_material(id, rough).is_err());
    assert_eq!(world.material(id), Ok(red), "a refused write changes nothing");

    let ghost = MaterialId::from_raw(77).unwrap();
    assert_eq!(world.set_material(ghost, red), Err(WorldError::NoMaterial(ghost)));
}

#[test]
fn meshes_lists_every_mesh_still_there() {
    let mut world = World::new();
    let cube = world.add_mesh(MeshSource::Cube);
    let sphere = world.add_mesh(MeshSource::Sphere);
    let plane = world.add_mesh(MeshSource::Plane);
    world.remove_mesh(sphere).unwrap();
    let mut listed: Vec<_> = world.meshes().collect();
    listed.sort_by_key(|(id, _)| *id);
    assert_eq!(listed, [(cube, MeshSource::Cube), (plane, MeshSource::Plane)]);
}
