//! The world's promises (stage 1 plan, step 1): ids are never reused, a
//! stale id is a clear error that never reaches another object, every
//! write is validated, and resources can't be removed while in use.

use std::collections::HashSet;

use fynkel_world::*;

fn world_with_cube() -> (World, MeshId, MaterialId) {
    let mut world = World::new();
    let mesh = world.add_mesh(MeshData::cube());
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
        "object#1 does not exist (destroyed, or never added)"
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
        world.insert(id, MeshRenderer { mesh: Some(ghost_mesh), material: Some(material), ..Default::default() }),
        Err(WorldError::NoMesh(ghost_mesh))
    );
    assert_eq!(
        world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(ghost_material), ..Default::default() }),
        Err(WorldError::NoMaterial(ghost_material))
    );
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() }).unwrap();
    world.insert(id, MeshRenderer::default()).unwrap(); // empty is allowed: it draws nothing
}

#[test]
fn for_each2_visits_exactly_the_objects_with_both() {
    let (mut world, mesh, material) = world_with_cube();
    let renderer = MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() };
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
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() }).unwrap();

    assert_eq!(world.remove_mesh(mesh), Err(WorldError::MeshInUse { mesh, object: id }));
    assert_eq!(world.mesh(mesh).map(|m| m.indices().len()), Ok(36));

    world.despawn(id).unwrap();
    world.remove_mesh(mesh).unwrap();
    assert_eq!(world.mesh(mesh).err(), Some(WorldError::NoMesh(mesh)));
    assert_eq!(world.remove_mesh(mesh), Err(WorldError::NoMesh(mesh)));
}

#[test]
fn materials_cannot_be_removed_while_in_use() {
    let (mut world, mesh, material) = world_with_cube();
    let id = world.spawn();
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() }).unwrap();

    assert_eq!(world.remove_material(material), Err(WorldError::MaterialInUse { material, object: id }));
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: None, ..Default::default() }).unwrap();
    world.remove_material(material).unwrap();
    assert_eq!(world.material(material), Err(WorldError::NoMaterial(material)));
}

#[test]
fn resource_ids_are_never_reused() {
    let mut world = World::new();
    let a = world.add_mesh(MeshData::plane());
    world.remove_mesh(a).unwrap();
    let b = world.add_mesh(MeshData::plane());
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
    let cube = world.add_mesh(MeshData::cube());
    let sphere = world.add_mesh(MeshData::sphere(8, 4));
    let plane = world.add_mesh(MeshData::plane());
    world.remove_mesh(sphere).unwrap();
    let mut listed: Vec<_> = world.meshes().map(|(id, m, _)| (id, m.indices().len())).collect();
    listed.sort();
    assert_eq!(listed, [(cube, 36), (plane, 6)]);
}

#[test]
fn setting_a_mesh_shares_the_new_data_and_bumps_its_version() {
    let (mut world, mesh, _) = world_with_cube();
    let version = |world: &World| world.meshes().find(|&(id, ..)| id == mesh).unwrap().2;
    assert_eq!(version(&world), 0);
    let plane = std::sync::Arc::new(MeshData::plane());
    world.set_mesh(mesh, std::sync::Arc::clone(&plane)).unwrap();
    assert!(std::sync::Arc::ptr_eq(world.mesh(mesh).unwrap(), &plane));
    assert_eq!(version(&world), 1);
    world.remove_mesh(mesh).unwrap();
    assert_eq!(world.set_mesh(mesh, MeshData::cube()), Err(WorldError::NoMesh(mesh)));
}

#[test]
fn submesh_materials_must_exist_hold_their_materials_and_go_with_the_object() {
    let (mut world, _, red) = world_with_cube();
    let blue = world.add_material(Material::default()).unwrap();
    let object = world.spawn();
    assert!(world.submesh_materials(object).is_empty() && !world.has_submesh_materials());
    world.set_submesh_materials(object, &[blue, red]).unwrap();
    assert_eq!(world.submesh_materials(object), [blue, red]);
    assert!(matches!(world.remove_material(blue), Err(WorldError::MaterialInUse { .. })));
    let ghost = MaterialId::from_raw(77).unwrap();
    assert_eq!(world.set_submesh_materials(object, &[ghost]), Err(WorldError::NoMaterial(ghost)));
    assert_eq!(world.submesh_materials(object), [blue, red], "a refused change changes nothing");
    world.despawn(object).unwrap();
    assert!(!world.has_submesh_materials());
    world.remove_material(blue).unwrap();
}

// --- Textures ----------------------------------------------------------------

fn image() -> ImageData {
    ImageData::from_rgba(1, 1, vec![255; 4]).unwrap()
}

#[test]
fn textures_are_shared_and_must_exist_for_a_material_to_use_them() {
    let mut world = World::new();
    let image = std::sync::Arc::new(image());
    let texture = world.add_texture(image.clone());
    assert!(std::sync::Arc::ptr_eq(world.texture(texture).unwrap(), &image), "shared, not copied");

    let ghost = TextureId::from_raw(99).unwrap();
    let uses_ghost = Material { normal_texture: Some(ghost), ..Material::default() };
    assert_eq!(world.add_material(uses_ghost), Err(WorldError::NoTexture(ghost)));
    let material = world.add_material(Material { base_color_texture: Some(texture), ..Material::default() }).unwrap();
    assert_eq!(world.set_material(material, uses_ghost), Err(WorldError::NoTexture(ghost)));
    assert_eq!(world.material(material).unwrap().base_color_texture, Some(texture), "a refused write changes nothing");
}

#[test]
fn textures_cannot_be_removed_while_a_material_uses_them() {
    let mut world = World::new();
    let texture = world.add_texture(image());
    // Any slot counts.
    let material = world.add_material(Material { emissive_texture: Some(texture), ..Material::default() }).unwrap();
    assert_eq!(world.remove_texture(texture), Err(WorldError::TextureInUse { texture, material }));
    assert_eq!(world.remove_texture(texture).unwrap_err().to_string(), "texture#1 is still used by material#1");

    world.set_material(material, Material::default()).unwrap();
    world.remove_texture(texture).unwrap();
    assert_eq!(world.texture(texture), Err(WorldError::NoTexture(texture)));
    assert_eq!(world.remove_texture(texture), Err(WorldError::NoTexture(texture)));
    assert_ne!(world.add_texture(image()), texture, "ids are never reused");
    assert_eq!(world.textures().count(), 1);
}

// --- Shaders (stage 10) ------------------------------------------------------

fn shader(values_size: usize) -> Shader {
    Shader { name: "test.wgsl".into(), values_size, textures: 1, effect: false, compiled: std::sync::Arc::new(()) }
}

#[test]
fn a_material_with_a_shader_has_its_values_zeroed_and_kept_in_range() {
    let mut world = World::new();
    let ghost = ShaderId::from_raw(99).unwrap();
    assert_eq!(
        world.add_material(Material { shader: Some(ghost), ..Material::default() }),
        Err(WorldError::NoShader(ghost))
    );

    let lava = world.add_shader(shader(8));
    let material = world.add_material(Material { shader: Some(lava), ..Material::default() }).unwrap();
    assert_eq!(world.material_values(material).unwrap(), [0; 8]);
    world.set_material_values(material, 4, &2.5f32.to_le_bytes()).unwrap();
    assert_eq!(&world.material_values(material).unwrap()[4..], 2.5f32.to_le_bytes());
    assert_eq!(world.set_material_values(material, 6, &[0; 4]), Err(WorldError::Values { material, end: 10, size: 8 }));

    // Set with the same shader, its values stay; with another, they start again.
    world.set_material(material, Material { roughness: 0.2, shader: Some(lava), ..Material::default() }).unwrap();
    assert_eq!(&world.material_values(material).unwrap()[4..], 2.5f32.to_le_bytes());
    let other = world.add_shader(shader(4));
    world.set_material(material, Material { shader: Some(other), ..Material::default() }).unwrap();
    assert_eq!(world.material_values(material).unwrap(), [0; 4]);
    world.set_material(material, Material::default()).unwrap();
    assert!(world.material_values(material).unwrap().is_empty());
}

#[test]
fn a_shader_set_again_keeps_its_materials_values_and_gets_a_new_version() {
    let mut world = World::new();
    let id = world.add_shader(shader(4));
    let material = world.add_material(Material { shader: Some(id), ..Material::default() }).unwrap();
    world.set_material_values(material, 0, &[1, 2, 3, 4]).unwrap();
    let (_, _, before) = world.shaders().next().unwrap();
    world.set_shader(id, shader(8)).unwrap();
    let (_, _, after) = world.shaders().next().unwrap();
    assert_ne!(before, after);
    assert_eq!(world.material_values(material).unwrap(), [1, 2, 3, 4, 0, 0, 0, 0]);
}

#[test]
fn shaders_and_their_textures_cannot_be_removed_while_a_material_uses_them() {
    let mut world = World::new();
    let id = world.add_shader(shader(0));
    let texture = world.add_texture(image());
    let ghost = TextureId::from_raw(99).unwrap();
    let uses_ghost =
        Material { shader: Some(id), shader_textures: [Some(ghost), None, None, None], ..Material::default() };
    assert_eq!(world.add_material(uses_ghost), Err(WorldError::NoTexture(ghost)));
    let material = world
        .add_material(Material {
            shader: Some(id),
            shader_textures: [Some(texture), None, None, None],
            ..Material::default()
        })
        .unwrap();
    assert_eq!(world.remove_texture(texture), Err(WorldError::TextureInUse { texture, material }));
    assert_eq!(world.remove_shader(id), Err(WorldError::ShaderInUse { shader: id, material }));
    assert_eq!(world.remove_shader(id).unwrap_err().to_string(), "shader#1 is still used by material#1");
    world.remove_material(material).unwrap();
    world.remove_shader(id).unwrap();
    world.remove_texture(texture).unwrap();
    assert!(world.shader(id).is_err());
}

fn effect(values_size: usize) -> Shader {
    Shader { effect: true, textures: 0, ..shader(values_size) }
}

#[test]
fn a_camera_takes_screen_effects_and_a_material_refuses_them() {
    let mut world = World::new();
    let tv = world.add_shader(effect(8));
    let lava = world.add_shader(shader(4));
    let camera = world.spawn();
    let plain = world.spawn();
    let on = |shader, values: &[u8]| ScreenEffect { shader, values: values.to_vec(), textures: [None; 4] };

    assert_eq!(
        world.add_material(Material { shader: Some(tv), ..Material::default() }),
        Err(WorldError::ShaderKind { shader: tv, effect: true })
    );
    assert_eq!(
        world.set_camera_effects(plain, vec![on(tv, &[0; 8])]),
        Err(WorldError::NoComponent { object: plain, component: "Camera" })
    );
    world.insert(camera, Camera::default()).unwrap();
    assert_eq!(
        world.set_camera_effects(camera, vec![on(lava, &[0; 4])]),
        Err(WorldError::ShaderKind { shader: lava, effect: false })
    );
    assert_eq!(
        world.set_camera_effects(camera, vec![on(tv, &[0; 4])]),
        Err(WorldError::EffectValues { shader: tv, given: 4, size: 8 })
    );
    assert!(world.camera_effects(camera).is_empty());

    world.set_camera_effects(camera, vec![on(tv, &[1; 8]), on(tv, &[2; 8])]).unwrap();
    assert_eq!(world.camera_effects(camera), [on(tv, &[1; 8]), on(tv, &[2; 8])]);
    // Set again: its values keep what fits; it can't stop being an effect's.
    world.set_shader(tv, effect(4)).unwrap();
    assert_eq!(world.camera_effects(camera)[0].values, [1; 4]);
    assert_eq!(world.set_shader(tv, shader(4)), Err(WorldError::ShaderKind { shader: tv, effect: true }));
    assert_eq!(world.remove_shader(tv), Err(WorldError::EffectInUse { shader: tv, object: camera }));

    // Its textures must be there, and stay while it uses them.
    let lut = world.add_texture(image());
    let graded = ScreenEffect { textures: [None, Some(lut), None, None], ..on(tv, &[0; 4]) };
    world.set_camera_effects(camera, vec![graded.clone()]).unwrap();
    assert_eq!(world.remove_texture(lut), Err(WorldError::EffectTextureInUse { texture: lut, object: camera }));
    world.set_camera_effects(camera, Vec::new()).unwrap();
    world.remove_texture(lut).unwrap();
    assert_eq!(world.set_camera_effects(camera, vec![graded]), Err(WorldError::NoTexture(lut)));

    assert!(world.camera_effects(camera).is_empty());
    world.set_camera_effects(camera, vec![on(tv, &[0; 4])]).unwrap();
    world.despawn(camera).unwrap();
    world.remove_shader(tv).unwrap();
}
