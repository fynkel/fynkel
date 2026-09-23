//! The glTF importer against the fixtures corpus (blueprint §19.2c item 10):
//! determinism (bytes committed), documents that validate, the security
//! boundary (external files), skipped/unsupported features, and stable ids.

use std::path::{Path, PathBuf};

use fynkel_contracts::{has_errors, Code};
use fynkel_document::asset::{sidecar_path, Meta};
use fynkel_document::{Document, DocumentKind, Material, Prefab, SchemaSet, ShaderLibrary};
use fynkel_import::{derive_sub_id, import, ColorSpace, Imported, Intermediate, Limits, Vertex};
use uuid::Uuid;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// The fixture's committed sidecar when it has one, a fresh one otherwise.
fn meta_for(source: &Path) -> Meta {
    let sidecar = sidecar_path(source);
    if sidecar.exists() {
        Meta::from_document(Document::read(&sidecar, None).unwrap())
    } else {
        Meta::new(source)
    }
}

fn import_fixture(name: &str) -> (Imported, Meta) {
    let path = fixture(name);
    let meta = meta_for(&path);
    let r = import(&path, &meta, &Limits::default());
    (r, meta)
}

fn codes(r: &Imported) -> Vec<Code> {
    r.diagnostics.iter().map(|d| d.code).collect()
}

const ROBOT_ID: &str = "b0000000-0000-4000-8000-000000000010";
const GLASS_ID: &str = "b0000000-0000-4000-8000-000000000011";

#[test]
fn robot_imports_meshes_materials_images_and_a_prefab() {
    let (r, meta) = import_fixture("robot.glb");
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    let i = r.intermediate.as_ref().unwrap();
    assert_eq!(i.importer, "fynkel.gltf");
    assert_eq!(i.source, "robot.glb");

    // Meshes: one per triangle primitive; a multi-primitive mesh gets `/p`.
    let subs: Vec<&str> = i.meshes.iter().map(|m| m.sub_id.as_str()).collect();
    assert_eq!(
        subs,
        [
            "mesh/Cube",
            "mesh/PyramidAndQuad/0",
            "mesh/PyramidAndQuad/1"
        ]
    );
    let cube = &i.meshes[0];
    assert_eq!(cube.name, "Cube");
    assert_eq!(cube.vertices.len(), 24);
    assert_eq!(cube.indices.len(), 36);
    assert_eq!(cube.triangle_count(), 12);
    assert_eq!(cube.material.as_deref(), Some("material/Painted"));
    assert_eq!(cube.bounds_min, [-0.5, -0.5, -0.5]);
    assert_eq!(cube.bounds_max, [0.5, 0.5, 0.5]);
    assert_eq!(cube.vertices[0].normal, [0.0, 0.0, 1.0]);
    assert_eq!(cube.vertices[0].tangent, [0.0; 4]);
    assert_eq!(cube.vertices[0].color, [1.0; 4]);

    // No normals and no indices: unwelded with flat face normals.
    let pyramid = &i.meshes[1];
    assert_eq!(pyramid.name, "PyramidAndQuad[0]");
    assert_eq!(pyramid.vertices.len(), 12);
    assert_eq!(pyramid.indices, (0..12).collect::<Vec<u32>>());
    assert_eq!(pyramid.material.as_deref(), Some("material/Glass"));
    for tri in pyramid.vertices.chunks(3) {
        let n = tri[0].normal;
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-5, "unit normal, got {n:?}");
        assert_eq!(tri[1].normal, n);
        assert_eq!(tri[2].normal, n);
    }

    // u32 indices, vertex colours, no material.
    let quad = &i.meshes[2];
    assert_eq!(quad.material, None);
    assert_eq!(quad.vertices.len(), 6);
    assert_eq!(quad.triangle_count(), 2);
    assert!(quad.vertices.iter().any(|v| v.color != [1.0; 4]));

    // Materials and images: the one image is bound as a colour (base
    // colour, emissive) and as data (normal, metallic-roughness,
    // occlusion), so it is imported twice with its two colour spaces.
    let names: Vec<&str> = i.materials.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Painted", "Glass"]);
    assert_eq!(i.images.len(), 2);
    assert_eq!(i.images[0].sub_id, "image/paint");
    assert_eq!(i.images[0].mime_type, "image/png");
    assert!(i.images[0].bytes.starts_with(b"\x89PNG"));
    assert_eq!(i.images[0].color_space, ColorSpace::Srgb);
    assert_eq!(i.images[1].sub_id, "image/paint/linear");
    assert_eq!(i.images[1].color_space, ColorSpace::Linear);
    assert_eq!(i.images[1].bytes, i.images[0].bytes);

    // Sub-asset ids: the sidecar's win, the rest are derived from the asset id.
    let asset = Uuid::parse_str(ROBOT_ID).unwrap();
    assert_eq!(meta.id(), asset);
    let expected: Vec<(String, Uuid)> = [
        "mesh/Cube",
        "mesh/PyramidAndQuad/0",
        "mesh/PyramidAndQuad/1",
        "material/Painted",
        "material/Glass",
        "image/paint",
        "image/paint/linear",
        "prefab",
    ]
    .iter()
    .map(|s| {
        let id = if *s == "material/Glass" {
            Uuid::parse_str(GLASS_ID).unwrap()
        } else {
            derive_sub_id(asset, s)
        };
        (s.to_string(), id)
    })
    .collect();
    assert_eq!(r.sub_assets, expected);
    assert_eq!(i.sub_assets, expected);
    assert!(r.sidecar_changed(&meta));

    // Warnings only: the fixture's sampler (nearest, clamp/repeat) is not
    // kept by the PBR model, and one slot samples TEXCOORD_1.
    assert_eq!(codes(&r), [Code::IMPORT_SKIPPED, Code::IMPORT_SKIPPED]);
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.message.contains("TEXCOORD_1")));
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.message.contains("sampler nearest")));
}

#[test]
fn robot_material_documents_validate_against_the_pbr_manifest() {
    let (r, _) = import_fixture("robot.glb");
    let i = r.intermediate.unwrap();
    let schemas = SchemaSet::embedded();
    let shaders = ShaderLibrary::builtin();

    let painted = &i.materials[0].document;
    assert_eq!(painted["shader"], "fynkel.pbr@1");
    assert_eq!(
        painted["id"],
        derive_sub_id(Uuid::parse_str(ROBOT_ID).unwrap(), "material/Painted").to_string()
    );
    assert_eq!(painted["formatVersion"], 2);
    assert_eq!(
        painted["base_color"],
        serde_json::json!([0.8, 0.2, 0.2, 1.0])
    );
    assert_eq!(painted["metallic"], 0.1);
    assert_eq!(painted["roughness"], 0.7);
    assert_eq!(painted["emissive_strength"], 3.0);
    assert_eq!(painted["normal_scale"], 0.5);
    assert_eq!(painted["occlusion_strength"], 0.8);
    assert_eq!(painted["base_color_texture"]["asset"], ROBOT_ID);
    assert_eq!(painted["base_color_texture"]["sub"], "image/paint");
    assert_eq!(painted["emissive_texture"]["sub"], "image/paint");
    // The data slots read the linear copy of the same image.
    for slot in [
        "normal_texture",
        "metallic_roughness_texture",
        "occlusion_texture",
    ] {
        assert_eq!(painted[slot]["asset"], ROBOT_ID, "{slot}");
        assert_eq!(painted[slot]["sub"], "image/paint/linear", "{slot}");
    }
    assert!(painted.get("alpha_mode").is_none());
    assert!(painted.get("parameters").is_none() && painted.get("textures").is_none());

    let glass = &i.materials[1].document;
    assert_eq!(glass["id"], GLASS_ID);
    assert_eq!(glass["alpha_mode"], "blend");
    assert_eq!(glass["double_sided"], true);
    assert!(glass.get("base_color_texture").is_none());

    for m in &i.materials {
        let text = serde_json::to_string_pretty(&m.document).unwrap();
        let doc = Document::parse(DocumentKind::Material, Some(&m.sub_id), &text)
            .unwrap_or_else(|d| panic!("{d:#?}"));
        let material = Material::from_document(doc);
        let diags = material.validate(&schemas, &shaders);
        assert!(!has_errors(&diags), "{}: {diags:#?}", m.sub_id);
        assert!(material.pbr.is_some());
    }
    let painted = Material::from_document(
        Document::parse(
            DocumentKind::Material,
            None,
            &serde_json::to_string(&i.materials[0].document).unwrap(),
        )
        .unwrap(),
    )
    .pbr
    .unwrap();
    assert_eq!(painted.emissive_strength, 3.0);
    assert_eq!(
        painted
            .textures()
            .iter()
            .filter(|(_, _, t)| t.is_some())
            .count(),
        5
    );
}

#[test]
fn robot_prefab_mirrors_the_node_hierarchy_and_validates() {
    let (r, _) = import_fixture("robot.glb");
    let i = r.intermediate.unwrap();
    let asset = Uuid::parse_str(ROBOT_ID).unwrap();
    // Node ids derive from name paths: nodes 0–4 of the file.
    let paths = [
        "Robot",
        "Robot/Body",
        "Robot/Body/Head",
        "Robot/Antenna",
        "Ground",
    ];
    let node = |n: usize| derive_sub_id(asset, &format!("node/{}", paths[n])).to_string();
    let p = i.prefab.as_ref().unwrap();
    assert_eq!(p["name"], "Robot");
    assert_eq!(p["id"], derive_sub_id(asset, "prefab").to_string());
    assert_eq!(p["roots"], serde_json::json!([node(0), node(4)]));

    let entities = p["entities"].as_object().unwrap();
    assert_eq!(entities.len(), 9); // 5 nodes + 2 primitive children under Antenna and Ground
    let order: Vec<&str> = entities
        .values()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        order,
        [
            "Robot",
            "Body",
            "Head",
            "Antenna",
            "PyramidAndQuad[0]",
            "PyramidAndQuad[1]",
            "Ground",
            "PyramidAndQuad[0]",
            "PyramidAndQuad[1]",
        ]
    );

    let robot = &entities[&node(0)];
    assert_eq!(robot["parent"], serde_json::Value::Null);
    assert_eq!(robot["children"], serde_json::json!([node(1), node(3)]));
    let robot_transform = robot["components"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(robot_transform["type"], "fynkel.Transform");
    assert_eq!(
        robot_transform["data"]["position"],
        serde_json::json!([0.0, 1.0, 0.0])
    );

    let body = &entities[&node(1)];
    assert_eq!(body["parent"], node(0));
    assert_eq!(body["children"], serde_json::json!([node(2)]));
    let body_components: Vec<&serde_json::Value> =
        body["components"].as_object().unwrap().values().collect();
    assert_eq!(body_components.len(), 2);
    assert_eq!(
        body_components[0]["data"]["scale"],
        serde_json::json!([1.0, 2.0, 1.0])
    );
    assert_eq!(body_components[1]["type"], "fynkel.MeshRenderer");
    assert_eq!(body_components[1]["data"]["mesh"]["sub"], "mesh/Cube");
    assert_eq!(
        body_components[1]["data"]["material"]["sub"],
        "material/Painted"
    );

    let head = &entities[&node(2)];
    let head_transform = head["components"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    // The file's own digits (an f32 quarter turn), not the exact constant.
    let half = head_transform["data"]["rotation"][1].as_f64().unwrap();
    assert!((half - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert_eq!(
        head_transform["data"]["rotation"],
        serde_json::json!([0.0, half, 0.0, half])
    );

    // Antenna uses a matrix in the file and carries a two-primitive mesh:
    // its renderers live on two child entities, the second without a material.
    let antenna = &entities[&node(3)];
    assert_eq!(antenna["components"].as_object().unwrap().len(), 1);
    let antenna_transform = antenna["components"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(
        antenna_transform["data"]["position"],
        serde_json::json!([0.0, 3.0, 0.0])
    );
    let kids = antenna["children"].as_array().unwrap();
    assert_eq!(kids.len(), 2);
    let second = &entities[kids[1].as_str().unwrap()];
    assert_eq!(second["parent"], node(3));
    let renderer = second["components"]
        .as_object()
        .unwrap()
        .values()
        .nth(1)
        .unwrap();
    assert_eq!(renderer["data"]["mesh"]["sub"], "mesh/PyramidAndQuad/1");
    assert_eq!(
        renderer["data"]["material"]["asset"],
        Uuid::nil().to_string()
    );
    assert!(renderer["data"]["material"].get("sub").is_none());

    let schemas = SchemaSet::embedded();
    let text = serde_json::to_string_pretty(p).unwrap();
    let doc = Document::parse(DocumentKind::Prefab, Some("prefab"), &text)
        .unwrap_or_else(|d| panic!("{d:#?}"));
    let diags = Prefab::from_document(doc).validate(&schemas);
    assert!(!has_errors(&diags), "{diags:#?}");
}

#[test]
fn intermediate_bytes_are_deterministic_and_round_trip() {
    let (a, _) = import_fixture("robot.glb");
    let (b, _) = import_fixture("robot.glb");
    let a = a.intermediate.unwrap();
    let b = b.intermediate.unwrap();
    assert_eq!(a, b);
    let bytes = a.to_bytes();
    assert_eq!(bytes, b.to_bytes());
    assert!(bytes.starts_with(b"FYNI"));

    let back = Intermediate::from_bytes(&bytes).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(back, a);

    // The committed bytes pin the format and the derivation of every id.
    let expected_path = fixture("robot.expected.fyni");
    if std::env::var_os("FYNKEL_UPDATE_FIXTURES").is_some() {
        std::fs::write(&expected_path, &bytes).unwrap();
    }
    let expected = std::fs::read(&expected_path)
        .expect("run with FYNKEL_UPDATE_FIXTURES=1 to (re)create robot.expected.fyni");
    assert!(
        bytes == expected,
        "robot.glb no longer imports to the committed bytes ({} vs {} bytes); if the change is intended, rerun with FYNKEL_UPDATE_FIXTURES=1",
        bytes.len(),
        expected.len()
    );
}

#[test]
fn malformed_intermediate_bytes_are_rejected() {
    assert_eq!(
        Intermediate::from_bytes(b"").unwrap_err().code,
        Code::IMPORT_UNREADABLE
    );
    assert_eq!(
        Intermediate::from_bytes(b"FYNI\x01\x00").unwrap_err().code,
        Code::IMPORT_UNREADABLE
    );
    let (r, _) = import_fixture("robot.glb");
    let mut bytes = r.intermediate.unwrap().to_bytes();
    bytes.truncate(bytes.len() / 2);
    assert_eq!(
        Intermediate::from_bytes(&bytes).unwrap_err().code,
        Code::IMPORT_UNREADABLE
    );
}

#[test]
fn vertex_layout_is_fixed_at_72_bytes() {
    let v = Vertex {
        position: [1.0, 2.0, 3.0],
        normal: [0.0, 1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
        uv0: [0.25, 0.75],
        uv1: [0.5, 0.5],
        color: [1.0, 0.5, 0.25, 1.0],
    };
    let mut out = Vec::new();
    v.write(&mut out);
    assert_eq!(out.len(), Vertex::STRIDE);
    assert_eq!(Vertex::read(&out), Some(v));
}

#[test]
fn data_uri_gltf_gives_the_same_meshes_as_the_glb() {
    let (glb, _) = import_fixture("robot.glb");
    let (gltf, _) = import_fixture("robot_data_uri.gltf");
    assert!(!gltf.has_errors(), "{:#?}", gltf.diagnostics);
    let glb = glb.intermediate.unwrap();
    let gltf = gltf.intermediate.unwrap();
    assert_eq!(glb.meshes, gltf.meshes);
    assert_eq!(glb.images, gltf.images);
    assert_eq!(glb.sub_assets, gltf.sub_assets);
    // Only the provenance differs.
    assert_eq!(gltf.source, "robot_data_uri.gltf");
    assert_ne!(glb.to_bytes(), gltf.to_bytes());
}

#[test]
fn external_buffers_are_refused_unless_the_sidecar_allows_them() {
    let path = fixture("external.gltf");

    // A fresh sidecar: no settings, so no reading next to the file.
    let denied = import(&path, &Meta::new(&path), &Limits::default());
    assert!(denied.has_errors());
    assert!(denied.intermediate.is_none());
    assert_eq!(codes(&denied), [Code::IMPORT_EXTERNAL_URI]);
    assert!(denied.diagnostics[0].message.contains("robot.bin"));

    // The committed sidecar sets allowExternalFiles.
    let meta = meta_for(&path);
    let allowed = import(&path, &meta, &Limits::default());
    assert!(!allowed.has_errors(), "{:#?}", allowed.diagnostics);
    let i = allowed.intermediate.unwrap();
    let (glb, _) = import_fixture("robot.glb");
    assert_eq!(i.meshes, glb.intermediate.unwrap().meshes);
}

#[test]
fn external_uris_that_escape_the_directory_are_refused_even_when_allowed() {
    let dir = std::env::temp_dir().join(format!("fynkel-import-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    for uri in [
        "../robot.bin",
        "/etc/passwd",
        "file:///robot.bin",
        "http://example.com/robot.bin",
    ] {
        let text = std::fs::read_to_string(fixture("external.gltf"))
            .unwrap()
            .replace("robot.bin", uri);
        let path = dir.join("escape.gltf");
        std::fs::write(&path, text).unwrap();
        std::fs::write(
            sidecar_path(&path),
            std::fs::read(fixture("external.gltf.meta.json")).unwrap(),
        )
        .unwrap();
        let meta = meta_for(&path);
        let r = import(&path, &meta, &Limits::default());
        assert_eq!(
            codes(&r),
            [Code::IMPORT_EXTERNAL_URI],
            "{uri}: {:#?}",
            r.diagnostics
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn non_triangle_primitives_are_skipped_with_a_warning() {
    let (r, _) = import_fixture("strip.glb");
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    assert_eq!(codes(&r), [Code::IMPORT_SKIPPED]);
    assert!(r.diagnostics[0].message.contains("TriangleStrip"));
    let i = r.intermediate.unwrap();
    let subs: Vec<&str> = i.meshes.iter().map(|m| m.sub_id.as_str()).collect();
    assert_eq!(subs, ["mesh/Strip/1"]);
    assert_eq!(i.meshes[0].triangle_count(), 4);
    // One primitive survived, so the node carries the renderer itself (the
    // sub id keeps its `/1`, since that is what the file calls it).
    let entities = i.prefab.as_ref().unwrap()["entities"].as_object().unwrap();
    assert_eq!(entities.len(), 1);
    let renderer = entities.values().next().unwrap()["components"]
        .as_object()
        .unwrap()
        .values()
        .nth(1)
        .unwrap();
    assert_eq!(renderer["data"]["mesh"]["sub"], "mesh/Strip/1");
}

#[test]
fn required_unsupported_extensions_are_refused() {
    let (r, _) = import_fixture("draco.glb");
    assert!(r.intermediate.is_none());
    assert_eq!(codes(&r), [Code::IMPORT_UNSUPPORTED]);
    assert!(
        r.diagnostics[0]
            .message
            .contains("KHR_draco_mesh_compression"),
        "{}",
        r.diagnostics[0]
    );
}

#[test]
fn node_cycles_are_an_error_not_a_hang() {
    let (r, _) = import_fixture("cycle.glb");
    assert!(r.intermediate.is_none());
    let errors: Vec<Code> = r
        .diagnostics
        .iter()
        .filter(|d| d.severity == fynkel_contracts::Severity::Error)
        .map(|d| d.code)
        .collect();
    assert_eq!(errors, [Code::IMPORT_INVALID_DATA]);
}

#[test]
fn limits_are_enforced() {
    let path = fixture("robot.glb");
    let meta = meta_for(&path);
    let tiny = Limits {
        max_file_bytes: 16,
        ..Limits::default()
    };
    let r = import(&path, &meta, &tiny);
    assert_eq!(codes(&r), [Code::IMPORT_LIMIT]);

    let few_vertices = Limits {
        max_vertices: 10,
        ..Limits::default()
    };
    let r = import(&path, &meta, &few_vertices);
    assert!(
        codes(&r).contains(&Code::IMPORT_LIMIT),
        "{:#?}",
        r.diagnostics
    );
    assert!(r.intermediate.is_none());

    let shallow = Limits {
        max_node_depth: 2,
        ..Limits::default()
    };
    let r = import(&path, &meta, &shallow);
    assert!(
        codes(&r).contains(&Code::IMPORT_LIMIT),
        "{:#?}",
        r.diagnostics
    );
}

#[test]
fn unreadable_and_unknown_sources() {
    let missing = fixture("nope.glb");
    let r = import(&missing, &Meta::new(&missing), &Limits::default());
    assert_eq!(codes(&r), [Code::IO]);

    let not_gltf = fixture("robot.glb.meta.json");
    let mut meta = Meta::new(&not_gltf);
    meta.importer = "fynkel.gltf".to_string();
    let r = import(&not_gltf, &meta, &Limits::default());
    assert_eq!(codes(&r), [Code::IMPORT_UNREADABLE]);

    // The dispatcher: an importer that does not exist yet.
    let png = fixture("robot.bin");
    let mut meta = Meta::new(&png);
    meta.importer = "fynkel.image".to_string();
    let r = import(&png, &meta, &Limits::default());
    assert_eq!(codes(&r), [Code::IMPORT_UNSUPPORTED]);
}

#[test]
fn sidecar_ids_that_the_file_no_longer_produces_are_reported() {
    let path = fixture("robot.glb");
    let mut meta = meta_for(&path);
    meta.sub_assets.push(("mesh/9".to_string(), Uuid::new_v4()));
    let r = import(&path, &meta, &Limits::default());
    assert!(!r.has_errors());
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == Code::IMPORT_SKIPPED && d.message.contains("mesh/9")));
    assert!(!r.sub_assets.iter().any(|(k, _)| k == "mesh/9"));
}

const RIG_ID: &str = "b0000000-0000-4000-8000-000000000012";

/// `rig.glb` (`fixtures/rig.py`): a skinned column over a three-joint
/// chain, a static prop, two clips (M3-19).
#[test]
fn rig_imports_skin_weights_skins_and_animations() {
    use fynkel_cooked::payload::{
        ANIM_INTERPOLATION_CUBIC_SPLINE, ANIM_INTERPOLATION_LINEAR, ANIM_INTERPOLATION_STEP,
        ANIM_PROPERTY_ROTATION, ANIM_PROPERTY_TRANSLATION,
    };
    let (r, _) = import_fixture("rig.glb");
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    assert!(
        r.diagnostics.is_empty(),
        "a clean rig imports silently: {:#?}",
        r.diagnostics
    );
    let i = r.intermediate.as_ref().unwrap();

    // Skin weights: one per vertex, normalized, u8 joints widened.
    let body = &i.meshes[0];
    assert_eq!(body.sub_id, "mesh/Body");
    assert!(body.is_skinned());
    assert_eq!(body.skin.len(), body.vertices.len());
    assert_eq!(body.skin[0].joints, [0, 0, 0, 0]);
    assert_eq!(body.skin[0].weights, [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(body.skin[4].joints, [0, 1, 0, 0]);
    assert_eq!(body.skin[4].weights, [0.5, 0.5, 0.0, 0.0]);
    assert_eq!(body.skin[12].joints, [2, 2, 0, 0]);
    assert!(!i.meshes[1].is_skinned());

    // The skin: joints in the file's order, parents inside the skin,
    // inverse bind matrices, rest pose.
    assert_eq!(i.skins.len(), 1);
    let skin = &i.skins[0];
    assert_eq!(skin.sub_id, "skin/Skin");
    assert_eq!(skin.name, "Skin");
    assert_eq!(skin.payload.skeleton, "hips");
    let names: Vec<&str> = skin
        .payload
        .joints
        .iter()
        .map(|j| j.name.as_str())
        .collect();
    assert_eq!(names, ["hips", "spine", "head"]);
    let parents: Vec<Option<u32>> = skin.payload.joints.iter().map(|j| j.parent).collect();
    assert_eq!(parents, [None, Some(0), Some(1)]);
    assert_eq!(skin.payload.joints[2].inverse_bind[13], -1.0);
    assert_eq!(skin.payload.joints[1].rest_translation, [0.0, 0.5, 0.0]);
    assert_eq!(skin.payload.joints[1].rest_rotation, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(skin.payload.joints[1].rest_scale, [1.0; 3]);

    // The clips: channels by node name, interpolation kept, duration.
    assert_eq!(i.animations.len(), 2);
    let wave = &i.animations[0];
    assert_eq!(wave.sub_id, "anim/wave");
    assert_eq!(wave.name, "wave");
    assert_eq!(wave.payload.duration, 1.0);
    assert_eq!(wave.payload.channels.len(), 3);
    let spine = &wave.payload.channels[0];
    assert_eq!(spine.target, "spine");
    assert_eq!(spine.property, ANIM_PROPERTY_ROTATION);
    assert_eq!(spine.interpolation, ANIM_INTERPOLATION_LINEAR);
    assert_eq!(spine.times, [0.0, 0.5, 1.0]);
    assert_eq!(spine.values.len(), 12);
    assert!((spine.values[6] - (std::f32::consts::FRAC_PI_8).sin()).abs() < 1e-6);
    let head = &wave.payload.channels[1];
    assert_eq!(head.target, "head");
    assert_eq!(head.interpolation, ANIM_INTERPOLATION_STEP);
    let hips = &wave.payload.channels[2];
    assert_eq!(hips.target, "hips");
    assert_eq!(hips.property, ANIM_PROPERTY_TRANSLATION);
    assert_eq!(hips.interpolation, ANIM_INTERPOLATION_CUBIC_SPLINE);
    assert_eq!(hips.values.len(), 2 * 3 * 3);
    assert_eq!(hips.values[12..15], [0.0, 0.25, 0.0]);
    let bob = &i.animations[1];
    assert_eq!(bob.name, "bob");
    assert_eq!(bob.payload.duration, 2.0);
    assert_eq!(bob.payload.channels[0].target, "Prop");

    // Sub-assets and derived ids.
    let subs: Vec<&str> = i.sub_assets.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        subs,
        [
            "mesh/Body",
            "mesh/Prop",
            "skin/Skin",
            "anim/wave",
            "anim/bob",
            "prefab"
        ]
    );
    let asset = Uuid::parse_str(RIG_ID).unwrap();
    assert_eq!(i.sub_assets[2].1, derive_sub_id(asset, "skin/Skin"));

    // The prefab: `fynkel.Skin` next to the Body's renderer, none on the prop.
    let p = i.prefab.as_ref().unwrap();
    let entities = p["entities"].as_object().unwrap();
    assert_eq!(entities.len(), 6);
    let paths = [
        "Rig",
        "Rig/Body",
        "Rig/hips",
        "Rig/hips/spine",
        "Rig/hips/spine/head",
        "Rig/Prop",
    ];
    let node = |n: usize| derive_sub_id(asset, &format!("node/{}", paths[n])).to_string();
    let body = &entities[&node(1)];
    let components: Vec<&serde_json::Value> =
        body["components"].as_object().unwrap().values().collect();
    assert_eq!(components.len(), 3);
    assert_eq!(components[1]["type"], "fynkel.MeshRenderer");
    assert_eq!(components[2]["type"], "fynkel.Skin");
    assert_eq!(components[2]["data"]["skin"]["asset"], RIG_ID);
    assert_eq!(components[2]["data"]["skin"]["sub"], "skin/Skin");
    assert_eq!(components[2]["data"]["skeleton"], "hips");
    assert!(body["components"]
        .as_object()
        .unwrap()
        .contains_key(&derive_sub_id(asset, "node/Rig/Body/skin").to_string()));
    let prop = &entities[&node(5)];
    assert_eq!(prop["components"].as_object().unwrap().len(), 2);
    let names: Vec<&str> = entities
        .values()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Rig", "Body", "hips", "spine", "head", "Prop"]);

    let schemas = SchemaSet::embedded();
    let text = serde_json::to_string_pretty(p).unwrap();
    let doc = Document::parse(DocumentKind::Prefab, Some("prefab"), &text)
        .unwrap_or_else(|d| panic!("{d:#?}"));
    let diags = Prefab::from_document(doc).validate(&schemas);
    assert!(!has_errors(&diags), "{diags:#?}");

    // The intermediate's bytes carry all of it.
    let again = Intermediate::from_bytes(&i.to_bytes()).unwrap();
    assert_eq!(&again, i);
}

#[test]
fn duplicate_joint_names_in_a_skin_are_an_error() {
    let (r, _) = import_fixture("rig_dupe_joints.glb");
    assert_eq!(codes(&r), [Code::IMPORT_INVALID_DATA]);
    assert!(r.diagnostics[0].message.contains("\"spine\""));
    assert!(r.intermediate.is_none());
}

#[test]
fn skin_joint_limit_is_enforced() {
    let path = fixture("rig.glb");
    let meta = meta_for(&path);
    let two = Limits {
        max_joints: 2,
        ..Limits::default()
    };
    let r = import(&path, &meta, &two);
    assert_eq!(codes(&r), [Code::IMPORT_LIMIT]);
}

/// The Khronos sample rigs (`fixtures/khronos/ATTRIBUTION.md`) — content
/// from outside this repo, the blueprint's acceptance rule since item
/// 19a: each imports clean, its skin covers every joint the file
/// declares, its clips keep their names and lengths, its hierarchy is a
/// prefab whose skinned node carries the `Skin`, and the bytes are
/// deterministic.
#[test]
fn khronos_sample_rigs_import_clean() {
    // (file, joints, clips as (name, seconds), prefab entities, skinned node)
    type Rig = (
        &'static str,
        usize,
        &'static [(&'static str, f32)],
        usize,
        &'static str,
    );
    let rigs: [Rig; 3] = [
        (
            "khronos/Fox.glb",
            24,
            &[("Survey", 3.42), ("Walk", 0.71), ("Run", 1.16)],
            26,
            "fox",
        ),
        (
            "khronos/CesiumMan.glb",
            19,
            &[("anim0", 2.0)],
            22,
            "Cesium_Man",
        ),
        (
            "khronos/RiggedFigure.glb",
            19,
            &[("anim0", 1.25)],
            22,
            "Proxy",
        ),
    ];
    for (file, joints, clips, entities, skinned) in rigs {
        let (r, meta) = import_fixture(file);
        assert!(!r.has_errors(), "{file}: {:#?}", r.diagnostics);
        let i = r.intermediate.as_ref().unwrap();
        assert_eq!(i.skins.len(), 1, "{file}");
        assert_eq!(i.skins[0].payload.joints.len(), joints, "{file}");
        assert_eq!(i.meshes.len(), 1, "{file}");
        assert_eq!(
            i.meshes[0].skin.len(),
            i.meshes[0].vertices.len(),
            "{file}: every vertex is skinned"
        );
        assert!(
            i.meshes[0]
                .skin
                .iter()
                .all(|w| w.weights.iter().sum::<f32>() > 0.99),
            "{file}: weights normalized"
        );
        let got: Vec<(&str, f32)> = i
            .animations
            .iter()
            .map(|a| {
                (
                    a.name.as_str(),
                    (a.payload.duration * 100.0).round() / 100.0,
                )
            })
            .collect();
        assert_eq!(got, clips, "{file}");
        assert_eq!(i.prefab_entity_count(), entities, "{file}");
        let prefab = i.prefab.as_ref().unwrap();
        let node = prefab["entities"]
            .as_object()
            .unwrap()
            .values()
            .find(|e| e["name"] == skinned)
            .unwrap_or_else(|| panic!("{file}: no node {skinned}"));
        let types: Vec<&str> = node["components"]
            .as_object()
            .unwrap()
            .values()
            .map(|c| c["type"].as_str().unwrap())
            .collect();
        assert!(
            types.contains(&"fynkel.Skin") && types.contains(&"fynkel.MeshRenderer"),
            "{file}: {types:?}"
        );
        // No committed sidecar: the same (fresh) meta again gives the same bytes.
        let again = import(&fixture(file), &meta, &Limits::default());
        assert!(
            again.intermediate.unwrap().to_bytes() == i.to_bytes(),
            "{file}: deterministic"
        );
    }
}
