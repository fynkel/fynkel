//! Reading `.glb` models (stage 2 step 4): what each sample turns into,
//! what's skipped with a warning, and what's refused with a readable error.

use std::path::PathBuf;

use fynkel_load::{Interpolation, ModelData, ModelError, Part, Property};
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
    // BLEND is see-through, as it's meant to be; the rest aren't.
    assert!(material("MatBlend").transparent);
    assert_eq!(material("MatBlend").alpha_cutoff, 0.0);
    assert!(!material("MatOpaque").transparent && !material("MatCutoff75").transparent);
    assert!(model.warnings().is_empty(), "{:?}", model.warnings());
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
    assert!(model.warnings().is_empty(), "{:?}", model.warnings());
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
    assert!(load("Fox.glb").warnings().is_empty(), "skins are read");
    assert_eq!(
        load("BoxTextured.glb").warnings(),
        ["nearest-neighbour texture filtering isn't supported; textures are filtered smoothly"]
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
    assert_eq!(warnings(|j| j["materials"][0]["alphaMode"] = json!("BLEND")), [""; 0]);
    assert_eq!(
        // Box's root is placed by a matrix; one that squashes everything
        // flat has no rotation to decompose into.
        warnings(|j| j["nodes"][0]["matrix"] = json!([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])),
        ["a node's transform can't be used (not finite, or a zero scale in a matrix); it's left out"]
    );
}

#[test]
fn a_skin_names_its_joints_and_a_skinned_mesh_has_four_joints_a_vertex() {
    let knight = load("Knight.glb");
    assert!(knight.warnings().is_empty(), "{:?}", knight.warnings());
    let [skin] = knight.skins() else { panic!("one skin, not {}", knight.skins().len()) };
    assert_eq!((skin.joints.len(), skin.inverse_binds.len()), (41, 41));
    let name = |n: usize| knight.nodes()[n].name.as_str();
    assert!(skin.joints.iter().any(|&j| name(j) == "hand.r"));
    // Its six body parts are bent by it; what it holds isn't, but hangs
    // from a joint.
    let skinned: Vec<&str> =
        knight.nodes().iter().filter(|n| n.skin == Some(0)).map(|n| n.name.as_str()).collect();
    assert_eq!(
        skinned,
        ["Knight_ArmLeft", "Knight_ArmRight", "Knight_Body", "Knight_Head", "Knight_LegLeft", "Knight_LegRight"]
    );
    for node in knight.nodes().iter().filter(|n| n.skin.is_some()) {
        for part in &node.parts {
            let mesh = &knight.meshes()[part.mesh];
            let joints = mesh.skin().expect("skinned");
            assert_eq!(joints.len(), mesh.vertices().len());
            assert!(joints.iter().flat_map(|v| v.joints).all(|j| usize::from(j) < 41));
        }
    }
    let sword = knight.nodes().iter().position(|n| n.name == "1H_Sword").unwrap();
    let slot = knight.nodes().iter().position(|n| n.children.contains(&sword)).unwrap();
    assert_eq!((name(slot), knight.nodes()[sword].skin), ("handslot.r", None));
    assert!(knight.meshes()[knight.nodes()[sword].parts[0].mesh].skin().is_none());
    assert_eq!(knight.clips().len(), 76);
}

#[test]
fn a_skin_that_cant_be_used_leaves_its_meshes_as_they_were_bound() {
    let warnings = |edit: fn(&mut Value)| {
        let fox = edited("Fox.glb", edit).unwrap();
        assert!(fox.nodes().iter().all(|n| n.skin.is_none()));
        fox.warnings().to_vec()
    };
    assert_eq!(
        warnings(|j| {
            let joints = j["skins"][0]["joints"].as_array().unwrap().clone();
            j["skins"][0]["joints"] = json!(joints.iter().cycle().take(300).collect::<Vec<_>>());
        }),
        ["a skin has 300 joints, more than 256; its meshes aren't bent"]
    );
    // Fewer joints than its mesh's vertices name.
    assert_eq!(
        warnings(|j| {
            let joints = j["skins"][0]["joints"].as_array().unwrap()[..3].to_vec();
            j["skins"][0]["joints"] = json!(joints);
        }),
        ["a skinned mesh names joints its skin doesn't have; it isn't bent"]
    );
    assert_eq!(
        warnings(|j| {
            let at = j["skins"][0]["inverseBindMatrices"].as_u64().unwrap() as usize;
            j["accessors"][at]["count"] = json!(2);
        }),
        ["a skin's inverse bind matrices can't be read; its meshes aren't bent"]
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

    // A sparse accessor needs no bytes for the values it leaves at 0: a
    // billion positions from a file of a few hundred bytes.
    let huge = refused("Box.glb", |j: &mut Value| {
        let positions = j["meshes"][0]["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
        let indices = j["meshes"][0]["primitives"][0]["indices"].clone();
        let (index_view, value_view) = (
            j["accessors"][indices.as_u64().unwrap() as usize]["bufferView"].clone(),
            j["accessors"][positions]["bufferView"].clone(),
        );
        let accessor = j["accessors"][positions].as_object_mut().unwrap();
        accessor.remove("bufferView");
        accessor.remove("byteOffset");
        accessor.insert("count".into(), json!(1_000_000_000u64));
        accessor.insert(
            "sparse".into(),
            json!({"count": 1, "indices": {"bufferView": index_view, "componentType": 5123}, "values": {"bufferView": value_view}}),
        );
    });
    assert!(huge.contains("claims 1000000000 values, more than the file has bytes"), "{huge}");
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

/// Hardening R13: KHR_mesh_quantization (gltfpack's default) stores
/// positions, normals and UVs as small integers, normalised or not. A
/// square, 2 m across, facing up.
#[test]
fn a_quantized_mesh_reads_as_floats() {
    let mut bin = Vec::new();
    // Positions: normalised i16, padded to 8 bytes a vertex.
    for (x, z) in [(-1i16, -1i16), (1, -1), (1, 1), (-1, 1)] {
        for c in [x * 32767, 0, z * 32767, 0] {
            bin.extend(c.to_le_bytes());
        }
    }
    // Normals: normalised i8, padded to 4 bytes.
    for _ in 0..4 {
        bin.extend([0u8, 127, 0, 0]);
    }
    // UVs: plain u8 (not normalised: whole texels of a 1 × 1 tile), padded to 4.
    for (u, v) in [(0u8, 0u8), (1, 0), (1, 1), (0, 1)] {
        bin.extend([u, v, 0, 0]);
    }
    for i in [0u16, 2, 1, 0, 3, 2] {
        bin.extend(i.to_le_bytes());
    }
    let json = json!({
        "asset": {"version": "2.0"},
        "extensionsUsed": ["KHR_mesh_quantization"],
        "extensionsRequired": ["KHR_mesh_quantization"],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 32, "byteStride": 8, "target": 34962},
            {"buffer": 0, "byteOffset": 32, "byteLength": 16, "byteStride": 4, "target": 34962},
            {"buffer": 0, "byteOffset": 48, "byteLength": 16, "byteStride": 4, "target": 34962},
            {"buffer": 0, "byteOffset": 64, "byteLength": 12, "target": 34963}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5122, "normalized": true, "count": 4, "type": "VEC3",
             "min": [-1, 0, -1], "max": [1, 0, 1]},
            {"bufferView": 1, "componentType": 5120, "normalized": true, "count": 4, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5121, "count": 4, "type": "VEC2"},
            {"bufferView": 3, "componentType": 5123, "count": 6, "type": "SCALAR"}
        ],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}, "indices": 3}]}],
        "nodes": [{"mesh": 0}],
        "scenes": [{"nodes": [0]}],
        "scene": 0
    });
    let model = ModelData::from_glb(&join(&json, &bin)).unwrap_or_else(|e| panic!("{e}"));
    let [mesh] = model.meshes() else { panic!("{:?}", model.meshes().len()) };
    let corners: Vec<([f32; 3], [f32; 3], [f32; 2])> =
        mesh.vertices().iter().map(|v| (v.position, v.normal, v.uv)).collect();
    for (position, normal, uv) in &corners {
        assert!(position.iter().all(|c| (c.abs() - 1.0).abs() < 1e-6 || *c == 0.0), "{position:?}");
        assert!((normal[1] - 1.0).abs() < 1e-6, "{normal:?}");
        assert!(uv.iter().all(|c| *c == 0.0 || *c == 1.0), "{uv:?}");
    }
    let first = corners.iter().find(|(p, ..)| p[0] < 0.0 && p[2] < 0.0).expect("the (-1, -1) corner");
    assert_eq!(first.2, [0.0, 0.0]);
    assert!(model.warnings().is_empty(), "{:?}", model.warnings());
}

#[test]
fn animations_become_clips_on_the_models_nodes() {
    let truck = load("CesiumMilkTruck.glb");
    let [wheels] = truck.clips() else { panic!("{:?}", truck.clips().len()) };
    assert_eq!((wheels.name.as_str(), wheels.length), ("Wheels", 1.25));
    let moved: Vec<&str> = wheels.channels.iter().map(|c| truck.nodes()[c.node].name.as_str()).collect();
    assert_eq!(moved, ["Wheels", "Wheels.001"]);
    let turn = &wheels.channels[0];
    assert_eq!((turn.property, turn.interpolation, turn.times.len()), (Property::Rotation, Interpolation::Linear, 31));
    // The file's second key, at 1/24 s, read as it is.
    assert!((turn.times[1] - 1.0 / 24.0).abs() < 1e-6);
    let key = turn.sample(turn.times[1]);
    assert!((key[1] + 0.189_215_38).abs() < 1e-5 && (key[3] - 0.981_935_6).abs() < 1e-5, "{key:?}");

    let fox = load("Fox.glb");
    let names: Vec<&str> = fox.clips().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Survey", "Walk", "Run"]);
}

#[test]
fn the_garden_characters_clips_move_its_parts() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/garden/assets/character.glb");
    let model = ModelData::from_glb(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(model.clips().len(), 27);
    let clip = |name: &str| model.clips().iter().find(|c| c.name == name).unwrap();
    let parts = |name: &str| {
        let mut parts: Vec<(&str, Property)> =
            clip(name).channels.iter().map(|c| (model.nodes()[c.node].name.as_str(), c.property)).collect();
        parts.sort_by_key(|p| p.0);
        parts
    };
    assert_eq!(
        parts("idle"),
        [
            ("arm-left", Property::Rotation),
            ("arm-right", Property::Rotation),
            ("head", Property::Rotation),
            ("torso", Property::Rotation)
        ]
    );
    assert!((clip("idle").length - 4.0 / 3.0).abs() < 1e-3, "{}", clip("idle").length);
    assert!(parts("walk").contains(&("root", Property::Translation)));
    assert_eq!(model.warnings(), ["unlit materials are drawn lit"]);
}

#[test]
fn a_clips_unusable_channels_are_skipped_with_a_warning() {
    // Morph weights, and keys that don't match their times (cubic needs
    // three values a key): the clip keeps the other channel.
    let model = edited("CesiumMilkTruck.glb", |json| {
        json["animations"][0]["channels"][0]["target"]["path"] = json!("weights");
        json["animations"][0]["samplers"][1]["interpolation"] = json!("CUBICSPLINE");
        json["animations"][0]["channels"]
            .as_array_mut()
            .unwrap()
            .push(json!({"sampler": 0, "target": {"node": 0, "path": "scale"}}));
    })
    .unwrap();
    let clip = &model.clips()[0];
    assert_eq!(clip.channels.len(), 0);
    assert_eq!(
        model.warnings(),
        [
            "animated morph target weights are skipped",
            "animation 'Wheels' has a channel with broken keys; it's skipped"
        ]
    );
    // A STEP clip holds each key.
    let step = edited("CesiumMilkTruck.glb", |json| {
        json["animations"][0]["samplers"][0]["interpolation"] = json!("STEP");
        json["animations"][0]["name"] = json!("");
    })
    .unwrap();
    let clip = &step.clips()[0];
    assert_eq!(clip.name, "animation 0");
    let turn = &clip.channels[0];
    assert_eq!(turn.interpolation, Interpolation::Step);
    assert_eq!(turn.sample(turn.times[1] * 0.99), turn.sample(0.0));
}
