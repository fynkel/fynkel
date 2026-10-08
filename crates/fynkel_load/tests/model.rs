//! Reading `.glb` models (stage 2 step 4): what each sample turns into,
//! what's skipped with a warning, and what's refused with a readable error.

use std::path::PathBuf;

use fynkel_load::{ModelData, ModelError, Part};
use serde_json::{Value, json};

fn sample(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/gltf").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn load(name: &str) -> ModelData {
    ModelData::from_glb(&sample(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A `.glb`'s JSON and binary chunks.
fn split(glb: &[u8]) -> (Value, Vec<u8>) {
    let word = |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()) as usize;
    let json_len = word(12);
    let json = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    let bin = if glb.len() > 20 + json_len {
        glb[28 + json_len..28 + json_len + word(20 + json_len)].to_vec()
    } else {
        Vec::new()
    };
    (json, bin)
}

/// A `.glb` from JSON and a binary chunk, padded as the format says.
fn join(json: &Value, bin: &[u8]) -> Vec<u8> {
    let mut text = serde_json::to_vec(json).unwrap();
    text.resize(text.len().next_multiple_of(4), b' ');
    let mut bin = bin.to_vec();
    bin.resize(bin.len().next_multiple_of(4), 0);
    let mut out = Vec::new();
    let total = 12 + 8 + text.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    for word in [0x4654_6C67, 2, total as u32, text.len() as u32, 0x4E4F_534A] {
        out.extend(u32::to_le_bytes(word));
    }
    out.extend(&text);
    if !bin.is_empty() {
        out.extend(u32::to_le_bytes(bin.len() as u32));
        out.extend(u32::to_le_bytes(0x004E_4942));
        out.extend(&bin);
    }
    out
}

/// `name` with its JSON changed by `edit`.
fn edited(name: &str, edit: impl FnOnce(&mut Value)) -> Result<ModelData, ModelError> {
    let (mut json, bin) = split(&sample(name));
    edit(&mut json);
    ModelData::from_glb(&join(&json, &bin))
}

#[test]
fn split_and_join_give_back_a_file_that_reads_the_same() {
    // The helper the broken-file tests rest on.
    let (json, bin) = split(&sample("BoxTextured.glb"));
    let again = ModelData::from_glb(&join(&json, &bin)).unwrap();
    let original = load("BoxTextured.glb");
    assert_eq!(again.nodes(), original.nodes());
    assert_eq!(again.materials(), original.materials());
    assert_eq!(again.images(), original.images());
}

#[test]
fn a_box_is_a_node_tree_with_one_mesh() {
    let model = load("Box.glb");
    assert_eq!(model.name(), "");
    assert_eq!(model.roots(), [0]);
    let [root, child] = model.nodes() else { panic!("{:?}", model.nodes()) };
    assert_eq!((root.parts.as_slice(), root.children.as_slice()), (&[][..], &[1][..]));
    // glTF's Z-up to Y-up turn sits on the root, as a unit quaternion.
    let [x, y, z, w] = root.rotation;
    assert!(
        (x + 0.70710677).abs() < 1e-6 && y == 0.0 && z == 0.0 && (w - 0.70710677).abs() < 1e-6,
        "{:?}",
        root.rotation
    );
    assert_eq!(child.parts, [Part { mesh: 0, material: 0 }]);
    let mesh = &model.meshes()[0];
    // 6 faces × 4 corners, 2 triangles each.
    assert_eq!((mesh.vertices().len(), mesh.indices().len()), (24, 36));
    let red = &model.materials()[0];
    assert_eq!(red.name, "Red");
    // Linear, as the file has it.
    assert_eq!(red.base_color, [0.8, 0.0, 0.0, 1.0]);
    assert!(model.warnings().is_empty(), "{:?}", model.warnings());
}

#[test]
fn materials_keep_their_factors_textures_and_modes() {
    let model = load("AlphaBlendModeTest.glb");
    assert_eq!((model.nodes().len(), model.meshes().len(), model.images().len()), (9, 9, 4));
    let material = |name: &str| model.materials().iter().find(|m| m.name == name).unwrap();
    assert_eq!(material("MatOpaque").alpha_cutoff, 0.0);
    assert_eq!(material("MatCutoff75").alpha_cutoff, 0.75);
    // MASK without a cutoff: glTF's 0.5.
    assert_eq!(material("MatCutoffDefault").alpha_cutoff, 0.5);
    assert_eq!(material("MatCutoff25").alpha_cutoff, 0.25);
    assert!(material("MatCutoff25").double_sided);
    // BLEND is drawn opaque, said once though two nodes use it.
    assert_eq!(material("MatBlend").alpha_cutoff, 0.0);
    assert_eq!(
        model.warnings(),
        ["see-through (BLEND) materials are drawn opaque; only cut-outs (MASK) are supported"]
    );
    // One image for both metallic-roughness and occlusion, as glTF packs
    // them; decoded once.
    let bed = material("MatBed");
    assert!(!bed.double_sided);
    assert_eq!(
        [
            bed.base_color_texture,
            bed.normal_texture,
            bed.metallic_roughness_texture,
            bed.occlusion_texture,
            bed.emissive_texture
        ],
        [Some(2), Some(0), Some(1), Some(1), None]
    );
    for image in model.images() {
        assert_eq!(image.pixels().len() as u32, image.width() * image.height() * 4);
    }
}

#[test]
fn a_node_with_several_primitives_has_several_parts_and_shared_meshes_are_read_once() {
    let model = load("CesiumMilkTruck.glb");
    let node = |name: &str| model.nodes().iter().find(|n| n.name == name).unwrap();
    let truck = node("Cesium_Milk_Truck");
    let materials: Vec<&str> = truck.parts.iter().map(|p| model.materials()[p.material].name.as_str()).collect();
    assert_eq!(materials, ["truck", "glass", "window_trim"]);
    // Both wheel nodes use the one "Wheels" mesh.
    assert_eq!(node("Wheels").parts, node("Wheels.001").parts);
    assert_eq!(model.meshes().len(), 4);
    assert_eq!(model.warnings(), ["animations are skipped; the model shows its rest pose"]);
}

#[test]
fn a_hundred_node_model_keeps_its_tree_and_reads_each_mesh_once() {
    let model = load("tower.glb");
    assert_eq!(model.name(), "Tower");
    assert_eq!(model.nodes().len(), 138);
    let tower = &model.nodes()[model.roots()[0]];
    let names: Vec<&str> = tower.children.iter().map(|&c| model.nodes()[c].name.as_str()).collect();
    assert_eq!(&names[..2], ["Floor 0", "Floor 1"]);
    assert_eq!(&names[10..], ["Matrix", "Unmirrored", "Mirrored"]);
    for &floor in &tower.children[..10] {
        assert_eq!(model.nodes()[floor].children.len(), 12);
    }
    // Three glTF meshes, the same cube in three materials: one mesh.
    assert_eq!((model.meshes().len(), model.materials().len()), (1, 3));
    let block = &model.nodes()[model.nodes()[tower.children[0]].children[0]];
    assert_eq!(block.scale, [0.5, 1.0, 0.9]);
    // The matrix becomes translation, rotation and scale.
    let matrix = &model.nodes()[tower.children[10]];
    assert_eq!((matrix.translation, matrix.rotation, matrix.scale), ([0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [2.0; 3]));
    assert_eq!(model.nodes()[tower.children[12]].scale, [-1.0, 1.0, 1.0]);
    assert!(model.warnings().is_empty(), "{:?}", model.warnings());
}

#[test]
fn what_the_engine_cant_draw_is_skipped_with_one_warning_per_kind() {
    let fox = load("Fox.glb");
    assert_eq!(
        fox.warnings(),
        [
            "animations are skipped; the model shows its rest pose",
            "skins are skipped; skinned meshes show their bind pose"
        ]
    );
    // The skinned mesh is still there, in its bind pose.
    assert_eq!(fox.nodes().iter().filter(|n| !n.parts.is_empty()).count(), 1);
    assert_eq!(
        load("BoxTextured.glb").warnings(),
        ["nearest-neighbour texture filtering isn't supported; textures are filtered smoothly"]
    );

    // Said once however often it's met: three see-through materials.
    let tower = edited("tower.glb", |j| {
        for material in j["materials"].as_array_mut().unwrap() {
            material["alphaMode"] = json!("BLEND");
        }
    });
    assert_eq!(
        tower.unwrap().warnings(),
        ["see-through (BLEND) materials are drawn opaque; only cut-outs (MASK) are supported"]
    );

    // Edits of a plain box, one kind each.
    let warnings = |edit: fn(&mut Value)| edited("Box.glb", edit).unwrap().warnings().to_vec();
    assert_eq!(
        warnings(|j| j["extensionsUsed"] = json!(["KHR_lights_punctual", "EXT_something"])),
        [
            "the model's lights are skipped; light the scene from code",
            "the glTF extension EXT_something isn't supported; the model is read without it"
        ]
    );
    assert_eq!(
        warnings(|j| j["meshes"][0]["primitives"][0]["mode"] = json!(1)),
        ["points and lines are skipped; only triangles are drawn"]
    );
    assert_eq!(
        warnings(|j| j["materials"][0]["alphaMode"] = json!("BLEND")),
        ["see-through (BLEND) materials are drawn opaque; only cut-outs (MASK) are supported"]
    );
    assert_eq!(
        // Box's root is placed by a matrix; one that squashes everything
        // flat has no rotation to decompose into.
        warnings(|j| j["nodes"][0]["matrix"] = json!([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])),
        ["a node's transform can't be used (not finite, or a zero scale in a matrix); it's left out"]
    );
}

#[test]
fn texture_transforms_and_emissive_strength_are_read() {
    let model = edited("BoxTextured.glb", |j| {
        j["extensionsUsed"] = json!(["KHR_texture_transform", "KHR_materials_emissive_strength"]);
        j["extensionsRequired"] = json!(["KHR_texture_transform"]);
        let material = &mut j["materials"][0];
        material["pbrMetallicRoughness"]["baseColorTexture"]["extensions"] =
            json!({"KHR_texture_transform": {"scale": [2.0, 0.5], "offset": [0.25, 0.0]}});
        material["emissiveFactor"] = json!([1.0, 0.5, 0.0]);
        material["extensions"] = json!({"KHR_materials_emissive_strength": {"emissiveStrength": 4.0}});
    })
    .unwrap();
    let material = &model.materials()[0];
    assert_eq!((material.uv_scale, material.uv_offset), ([2.0, 0.5], [0.25, 0.0]));
    assert_eq!((material.emissive, material.emissive_strength), ([1.0, 0.5, 0.0], 4.0));
}

#[test]
fn strips_and_fans_become_triangle_lists_facing_the_same_way() {
    // Box's 36 indices are 12 triangles; as a strip or a fan the same
    // index buffer gives 34. What matters is the order: every triangle of
    // a strip keeps the first one's winding.
    let as_list = |mode: u32| {
        let model = edited("Box.glb", |j| j["meshes"][0]["primitives"][0]["mode"] = json!(mode)).unwrap();
        model.meshes()[0].indices().to_vec()
    };
    let list = as_list(4);
    let strip = as_list(5);
    let fan = as_list(6);
    assert_eq!((strip.len(), fan.len()), (34 * 3, 34 * 3));
    // Strip: (0 1 2), (2 1 3), (2 3 4), ...
    assert_eq!(&strip[..9], [list[0], list[1], list[2], list[2], list[1], list[3], list[2], list[3], list[4]]);
    // Fan: (0 1 2), (0 2 3), ...
    assert_eq!(&fan[..6], [list[0], list[1], list[2], list[0], list[2], list[3]]);
}

#[test]
fn broken_files_are_refused_with_a_readable_reason() {
    let error = |bytes: &[u8]| ModelData::from_glb(bytes).unwrap_err().to_string();
    assert_eq!(error(b""), "the model is empty (0 bytes)");
    assert_eq!(
        error(b"  {\"asset\": {\"version\": \"2.0\"}}"),
        "this is a .gltf (JSON) file; only .glb (binary glTF) can be loaded, since it holds everything in one file"
    );
    assert_eq!(error(b"\x89PNG\r\n\x1a\n"), "the model isn't a .glb (binary glTF) file");
    let box_glb = sample("Box.glb");
    assert!(error(&box_glb[..box_glb.len() / 2]).starts_with("the .glb is broken: "));

    fn refused(name: &str, edit: impl FnOnce(&mut Value)) -> String {
        edited(name, edit).unwrap_err().to_string()
    }
    assert_eq!(
        refused("Box.glb", |j: &mut Value| {
            j["extensionsUsed"] = json!(["KHR_draco_mesh_compression"]);
            j["extensionsRequired"] = json!(["KHR_draco_mesh_compression"]);
        }),
        "the model needs the glTF extension KHR_draco_mesh_compression, which isn't supported"
    );
    assert_eq!(
        refused("BoxTextured.glb", |j: &mut Value| j["images"][0] = json!({"uri": "CesiumLogoFlat.png"})),
        "the model refers to a file outside the .glb (CesiumLogoFlat.png); only self-contained .glb files load"
    );
    let bad_index = refused("Box.glb", |j: &mut Value| j["nodes"][1]["mesh"] = json!(7));
    assert!(bad_index.starts_with("the .glb is broken: "), "{bad_index}");
}

#[test]
fn a_broken_image_names_the_image() {
    // The image's bytes overwritten with zeros.
    let (json, mut bin) = split(&sample("BoxTextured.glb"));
    let view = &json["bufferViews"][json["images"][0]["bufferView"].as_u64().unwrap() as usize];
    let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
    let end = start + view["byteLength"].as_u64().unwrap() as usize;
    bin[start..end].fill(0);
    let error = ModelData::from_glb(&join(&json, &bin)).unwrap_err();
    assert_eq!(error.to_string(), "image 0 in the model: the image isn't a PNG or JPEG");
}

#[test]
fn material_values_outside_gltfs_ranges_are_clamped() {
    // glTF gives the ranges, but nothing in the format enforces them.
    let model = edited("BoxTextured.glb", |j| {
        j["extensionsUsed"] = json!(["KHR_materials_emissive_strength", "KHR_texture_transform"]);
        let material = &mut j["materials"][0];
        let pbr = &mut material["pbrMetallicRoughness"];
        pbr["baseColorFactor"] = json!([2.0, -1.0, 0.5, 1.0]);
        pbr["metallicFactor"] = json!(2.0);
        pbr["roughnessFactor"] = json!(-1.0);
        pbr["baseColorTexture"]["extensions"] =
            json!({"KHR_texture_transform": {"scale": [1e6, 1.0], "offset": [-1e9, 0.5]}});
        material["emissiveFactor"] = json!([3.0, 0.5, 0.0]);
        material["extensions"] = json!({"KHR_materials_emissive_strength": {"emissiveStrength": -4.0}});
    })
    .unwrap();
    let material = &model.materials()[0];
    assert_eq!(material.base_color, [1.0, 0.0, 0.5, 1.0]);
    assert_eq!((material.metallic, material.roughness), (1.0, 0.0));
    assert_eq!((material.emissive, material.emissive_strength), ([1.0, 0.5, 0.0], 0.0));
    assert_eq!((material.uv_scale, material.uv_offset), ([1e4, 1.0], [-1e4, 0.5]));
    assert!(
        model.warnings().contains(&"some material values are outside glTF's ranges; they're clamped".to_owned()),
        "{:?}",
        model.warnings()
    );
    // In range, nothing is said.
    assert!(!load("AlphaBlendModeTest.glb").warnings().iter().any(|w| w.contains("clamped")));
}
