//! Spawning a loaded model (stage 2 plan §3.3): the node tree becomes
//! objects, materials become the world's (sRGB), and every copy shares the
//! same meshes, textures and materials.

use std::path::PathBuf;

use fynkel_world::*;

fn load(name: &str) -> ModelData {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/gltf").join(name);
    ModelData::from_glb(&std::fs::read(&path).unwrap()).unwrap()
}

fn spawn(world: &mut World, model: &ModelData, ids: &ModelIds, at: Vec3) -> Result<ObjectId> {
    world.spawn_model(model, &ids.meshes, &ids.materials, Transform::from_position(at))
}

/// Every object from `root` down, depth first.
fn tree(world: &World, root: ObjectId) -> Vec<ObjectId> {
    let mut all = vec![root];
    for &child in world.children(root).unwrap() {
        all.extend(tree(world, child));
    }
    all
}

#[test]
fn linear_colours_become_srgb_and_back() {
    for c in [0.0, 0.002, 0.0031308, 0.04, 0.2, 0.5, 1.0] {
        let color = Color::from_linear([c, c, c, 0.25]);
        assert_eq!(color.a, 0.25, "alpha is unchanged");
        let back = color.to_linear();
        assert!((back[0] - c).abs() < 1e-6, "{c} → {} → {}", color.r, back[0]);
    }
    // glTF's mid grey: linear 0.214 is sRGB 0.5.
    assert!((Color::from_linear([0.214_041, 0.0, 0.0, 1.0]).r - 0.5).abs() < 1e-5);
}

#[test]
fn a_models_materials_become_the_worlds() {
    let model = load("AlphaBlendModeTest.glb");
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    assert_eq!((ids.meshes.len(), ids.textures.len(), ids.materials.len()), (9, 4, 6));
    // The same data, not copies.
    assert!(std::sync::Arc::ptr_eq(world.mesh(ids.meshes[0]).unwrap(), &model.meshes()[0]));
    assert!(std::sync::Arc::ptr_eq(world.texture(ids.textures[3]).unwrap(), &model.images()[3]));
    let (index, data) = model.materials().iter().enumerate().find(|(_, m)| m.name == "MatBed").unwrap();
    let bed = world.material(ids.materials[index]).unwrap();
    assert_eq!(bed.base_color_texture, Some(ids.textures[data.base_color_texture.unwrap()]));
    assert_eq!(bed.normal_texture, Some(ids.textures[0]));
    assert_eq!(bed.metallic_roughness_texture, Some(ids.textures[1]));
    assert_eq!(bed.occlusion_texture, Some(ids.textures[1]));
    assert_eq!(bed.emissive_texture, None);
    let cutoff = model.materials().iter().position(|m| m.name == "MatCutoff25").unwrap();
    let cutoff = world.material(ids.materials[cutoff]).unwrap();
    assert_eq!((cutoff.alpha_cutoff, cutoff.double_sided, cutoff.blend), (0.25, true, Blend::Opaque));
    let glass = model.materials().iter().position(|m| m.name == "MatBlend").unwrap();
    assert_eq!(world.material(ids.materials[glass]).unwrap().blend, Blend::Transparent);
}

#[test]
fn material_factors_are_converted_from_linear() {
    let data = MaterialData {
        base_color: [0.214_041, 1.0, 0.0, 0.5],
        emissive: [1.0, 0.214_041, 0.0],
        emissive_strength: 6.0,
        metallic: 0.3,
        roughness: 0.7,
        uv_scale: [2.0, 3.0],
        uv_offset: [0.5, -0.5],
        ..MaterialData::DEFAULT
    };
    let material = Material::from_model(&data, &[]);
    let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
    let Color { r, g, b, a } = material.base_color;
    assert!(close(r, 0.5) && close(g, 1.0) && close(b, 0.0) && a == 0.5, "{:?}", material.base_color);
    assert!(close(material.emissive.g, 0.5) && material.emissive_intensity == 6.0);
    assert_eq!((material.metallic, material.roughness), (0.3, 0.7));
    assert_eq!((material.uv_scale, material.uv_offset), (Vec2::new(2.0, 3.0), Vec2::new(0.5, -0.5)));
    // glTF's default material: white, fully metallic and rough.
    let default = Material::from_model(&MaterialData::DEFAULT, &[]);
    assert_eq!((default.base_color, default.metallic, default.roughness), (Color::WHITE, 1.0, 1.0));
}

#[test]
fn a_model_spawns_as_a_tree_of_named_objects_under_one_root() {
    let model = load("tower.glb");
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let root = spawn(&mut world, &model, &ids, Vec3::new(10.0, 0.0, 0.0)).unwrap();
    // A root for the scene, holding the file's own root node.
    assert_eq!(world.name(root).unwrap(), Some("Tower"));
    assert_eq!(world.get::<Transform>(root).unwrap().position, Vec3::new(10.0, 0.0, 0.0));
    assert_eq!(world.len(), 1 + 138);
    assert_eq!(tree(&world, root).len(), 1 + 138);
    let [tower] = world.children(root).unwrap() else { panic!() };
    assert_eq!(world.name(*tower).unwrap(), Some("Tower"));
    // Children in the file's order.
    let names: Vec<_> = world.children(*tower).unwrap().iter().map(|&c| world.name(c).unwrap().unwrap()).collect();
    assert_eq!(&names[..3], ["Floor 0", "Floor 1", "Floor 2"]);

    let block = world.find(root, "Block 3.4").unwrap().unwrap();
    let floor = world.parent(block).unwrap().unwrap();
    assert_eq!(world.name(floor).unwrap(), Some("Floor 3"));
    // Brick: floors take the three materials in turn.
    let renderer = world.get::<MeshRenderer>(block).unwrap();
    assert_eq!(
        renderer,
        MeshRenderer { mesh: Some(ids.meshes[0]), material: Some(ids.materials[0]), ..Default::default() }
    );
    assert_eq!(
        world.get::<MeshRenderer>(world.find(root, "Block 4.0").unwrap().unwrap()).unwrap().material,
        Some(ids.materials[1])
    );
    assert_eq!(world.get::<Transform>(block).unwrap().scale, Vec3::new(0.5, 1.0, 0.9));
    // A node that draws nothing has no renderer.
    assert!(!world.has::<MeshRenderer>(floor).unwrap());

    // Placed by the file's transforms, then the spawn's: the matrix node
    // is 1 up, and the spawn 10 across.
    let matrix = world.find(root, "Matrix").unwrap().unwrap();
    let placed = world.world_matrix(matrix).unwrap();
    assert!(placed.abs_diff_eq(
        Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, Vec3::new(10.0, 1.0, 0.0)),
        1e-5
    ));
    // The mirrored shape is mirrored in the world, so it's drawn with its
    // winding flipped.
    let mirrored = world.find(root, "Mirrored foot").unwrap().unwrap();
    assert!(world.world_matrix(mirrored).unwrap().determinant() < 0.0);
    let unmirrored = world.find(root, "Unmirrored foot").unwrap().unwrap();
    assert!(world.world_matrix(unmirrored).unwrap().determinant() > 0.0);
}

#[test]
fn a_node_drawing_several_primitives_gets_a_child_for_each() {
    let model = load("CesiumMilkTruck.glb");
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let root = spawn(&mut world, &model, &ids, Vec3::ZERO).unwrap();
    let truck = world.find(root, "Cesium_Milk_Truck").unwrap().unwrap();
    assert!(!world.has::<MeshRenderer>(truck).unwrap());
    let parts: Vec<ObjectId> =
        world.children(truck).unwrap().iter().copied().filter(|&c| world.has::<MeshRenderer>(c).unwrap()).collect();
    assert_eq!(parts.len(), 3);
    for &part in &parts {
        assert_eq!(world.name(part).unwrap(), None);
        assert_eq!(world.get::<Transform>(part).unwrap(), Transform::default());
        assert!(world.children(part).unwrap().is_empty());
    }
    let materials: Vec<&str> = parts
        .iter()
        .map(|&p| {
            let id = world.get::<MeshRenderer>(p).unwrap().material.unwrap();
            model.materials()[ids.materials.iter().position(|&m| m == id).unwrap()].name.as_str()
        })
        .collect();
    assert_eq!(materials, ["truck", "glass", "window_trim"]);
    // Both wheel nodes draw the one wheel mesh.
    let wheels = |name| world.get::<MeshRenderer>(world.find(root, name).unwrap().unwrap()).unwrap();
    assert_eq!(wheels("Wheels"), wheels("Wheels.001"));
    // The scene's name, and the file's node names, as they are.
    assert_eq!(world.name(root).unwrap(), Some("Scene"));
}

#[test]
fn copies_of_a_model_share_its_meshes_textures_and_materials() {
    let model = load("CesiumMilkTruck.glb");
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let first = spawn(&mut world, &model, &ids, Vec3::ZERO).unwrap();
    let resources = |world: &World| (world.meshes().count(), world.textures().count(), world.materials().count());
    let before = resources(&world);
    let second = spawn(&mut world, &model, &ids, Vec3::X * 5.0).unwrap();
    assert_eq!(resources(&world), before);
    assert_eq!(tree(&world, first).len(), tree(&world, second).len());
    let wheels = |root| world.get::<MeshRenderer>(world.find(root, "Wheels").unwrap().unwrap()).unwrap();
    assert_eq!(wheels(first), wheels(second));
    // Despawning one leaves the other whole.
    let objects = tree(&world, second).len();
    world.despawn(first).unwrap();
    assert_eq!(world.len(), objects);
}

#[test]
fn a_spawn_that_fails_leaves_nothing_behind() {
    let model = load("CesiumMilkTruck.glb");
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let error = world.spawn_model(&model, &ids.meshes[1..], &ids.materials, Transform::default()).unwrap_err();
    assert_eq!(error, WorldError::ModelIds { what: "meshes", given: 3, needed: 4 });
    assert_eq!(error.to_string(), "the model has 4 meshes, but 3 ids were given for them");
    assert!(world.is_empty());

    // A material removed after adding: found only partway through.
    let glass = model.materials().iter().position(|m| m.name == "glass").unwrap();
    world.remove_material(ids.materials[glass]).unwrap();
    let error = spawn(&mut world, &model, &ids, Vec3::ZERO).unwrap_err();
    assert_eq!(error, WorldError::NoMaterial(ids.materials[glass]));
    assert!(world.is_empty(), "{} objects left", world.len());
}
