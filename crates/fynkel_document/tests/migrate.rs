//! Migration corpus (`fixtures/migrate/`): a synthetic v0 scene (arrays
//! carrying ids) migrated to v1 with an exact expected result and diff; a
//! v0 document a step cannot handle; a kind with no path; a document from
//! a newer tool.

use std::path::{Path, PathBuf};

use fynkel_document::material::{AlphaMode, ShaderLibrary};
use fynkel_document::migrate::backup_path;
use fynkel_document::{
    describe, migrate, Document, DocumentKind, Material, Migration, Migrations, Scene, SchemaSet,
};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/migrate")
}

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

#[test]
fn builtin_steps_chain_to_the_current_version() {
    let m = Migrations::builtin();
    for step in m.steps() {
        assert!(
            step.to() <= step.kind.supported_format_version(),
            "{}",
            step.id()
        );
        assert!(
            m.path(step.kind, step.from, step.kind.supported_format_version())
                .is_ok(),
            "{} has no chain to the current version",
            step.id()
        );
    }
    let ids: Vec<String> = m.steps().iter().map(Migration::id).collect();
    assert_eq!(
        ids,
        [
            "material/1-2/pbr-fields",
            "meta/1-2/name-keys",
            "prefab/1-2/camera-exposure",
            "prefab/2-3/animator-layers",
            "prefab/3-4/object-layers",
            "prefab/4-5/one-root",
            "prefab/5-6/rigidbody-constraints",
            "project/1-2/layers",
            "scene/0-1/entities-keyed-by-id",
            "scene/1-2/camera-exposure",
            "scene/2-3/animator-layers",
            "scene/3-4/object-layers",
            "scene/4-5/one-root",
            "scene/5-6/rigidbody-constraints"
        ]
    );
}

#[test]
fn v0_scene_migrates_to_v1_exactly() {
    let schemas = schemas();
    let path = fixtures().join("v0_training.scene.json");
    let doc = Document::read(&path, None).unwrap();
    assert!(doc.is_outdated());
    // Outdated documents validate to one diagnostic: migrate first.
    let diags = doc.validate(&schemas);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code.as_str(), "FYN-D0016");
    assert_eq!(diags[0].line, Some(3));

    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(m.diagnostics.is_empty(), "{:#?}", m.diagnostics);
    assert!(m.changed());
    // Four steps chain: 0 → 1 (keyed entities), 1 → 2 (camera exposure),
    // 2 → 3 (animator layers) and 3 → 4 (object layers) — the middle two
    // have nothing to do here, the scene has no camera and no animator; the
    // last gives the light its culling mask.
    assert_eq!((m.from, m.to), (0, 6));
    assert_eq!(m.applied.len(), 6);
    assert_eq!(m.applied[0].name, "scene/0-1/entities-keyed-by-id");
    assert_eq!(m.applied[1].name, "scene/1-2/camera-exposure");
    assert_eq!(m.applied[2].name, "scene/2-3/animator-layers");
    assert_eq!(m.applied[3].name, "scene/3-4/object-layers");
    assert_eq!(m.applied[4].name, "scene/4-5/one-root");
    assert_eq!(m.applied[5].name, "scene/5-6/rigidbody-constraints");
    assert_eq!(m.document.envelope.format_version, 6);
    assert!(!m.document.is_outdated());

    let text = m.document.write().unwrap();
    let expected_path = fixtures().join("v0_training.scene.migrated.json");
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    if text != expected {
        std::fs::write(expected_path.with_extension("actual.json"), &text).unwrap();
    }
    assert_eq!(text, expected, "see the .actual.json next to the fixture");

    // The diff is a unified diff of the canonical texts, headed by the path.
    assert!(m.diff.starts_with("--- "));
    assert!(m.diff.contains("(formatVersion 0)\n+++ "));
    assert!(m
        .diff
        .contains("-  \"formatVersion\": 0,\n+  \"formatVersion\": 6,"));
    assert!(m.diff.contains("-  \"entities\": [\n-    {\n-      \"id\": \"b0000000-0000-4000-8000-0000000000e1\",\n+  \"entities\": {\n+    \"b0000000-0000-4000-8000-0000000000e1\": {"));

    // Order kept, ids became keys, everything else untouched; the result
    // validates and loads.
    assert!(m.document.validate(&schemas).is_empty());
    let scene = Scene::from_document(m.document.clone());
    assert!(scene.validate(&schemas).is_empty());
    let ids: Vec<String> = scene.entities.iter().map(|e| e.id.to_string()).collect();
    assert_eq!(
        ids,
        [
            "b0000000-0000-4000-8000-0000000000e1",
            "b0000000-0000-4000-8000-0000000000e2"
        ]
    );
    assert_eq!(scene.entities[1].components.len(), 2);
    assert_eq!(scene.entities[1].components[1].type_id, "fynkel.PointLight");
    assert_eq!(
        m.document.root.get("editorNotes").and_then(|n| n.as_str()),
        Some("written by the v0 tool; arrays instead of keyed objects")
    );
    assert!(!describe(&scene, &SchemaSet::embedded()).has_errors());

    // Idempotent: the migrated document has nothing left to do.
    let again = migrate(&m.document, &Migrations::builtin(), &schemas);
    assert!(!again.changed());
    assert!(again.diagnostics.is_empty());
    assert!(again.diff.is_empty());
}

#[test]
fn v1_material_migrates_to_v2_exactly() {
    let schemas = schemas();
    let path = fixtures().join("v1_rusted_metal.material.json");
    let doc = Document::read(&path, None).unwrap();
    assert!(doc.is_outdated());
    assert_eq!(doc.validate(&schemas)[0].code.as_str(), "FYN-D0016");

    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(m.diagnostics.is_empty(), "{:#?}", m.diagnostics);
    assert_eq!((m.from, m.to), (1, 2));
    assert_eq!(m.applied[0].name, "material/1-2/pbr-fields");

    let text = m.document.write().unwrap();
    let expected_path = fixtures().join("v1_rusted_metal.material.migrated.json");
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    if text != expected {
        std::fs::write(expected_path.with_extension("actual.json"), &text).unwrap();
    }
    assert_eq!(text, expected, "see the .actual.json next to the fixture");

    // The maps became the record's fields (sampler and colour-space
    // overrides dropped), cull: none became double_sided, the shadow
    // flags went; the result is the typed model.
    let material = Material::from_document(m.document.clone());
    assert!(material
        .validate(&schemas, &ShaderLibrary::builtin())
        .is_empty());
    let pbr = material.pbr.unwrap();
    assert_eq!(pbr.base_color.to_array(), [0.8, 0.5, 0.3, 1.0]);
    assert_eq!((pbr.metallic, pbr.roughness), (1.0, 0.65));
    assert_eq!(pbr.alpha_mode, AlphaMode::Mask);
    assert_eq!(pbr.alpha_cutoff, 0.4);
    assert!(pbr.double_sided);
    assert_eq!(pbr.metallic_roughness_texture.unwrap().sub, "packed");
    assert!(pbr.normal_texture.is_some() && pbr.occlusion_texture.is_none());
    for key in [
        "parameters",
        "textures",
        "cull",
        "castShadows",
        "receiveShadows",
        "alphaMode",
    ] {
        assert!(m.document.root.get(key).is_none(), "{key} survived");
    }

    let again = migrate(&m.document, &Migrations::builtin(), &schemas);
    assert!(!again.changed());

    // A pbr material naming a parameter the model has no field for cannot
    // be converted; the document is left as it was.
    let text = r#"{
  "$schema": "https://fynkel.dev/schemas/material.schema.json",
  "formatVersion": 1,
  "id": "b0000000-0000-4000-8000-000000000004",
  "name": "Odd",
  "shader": "fynkel.pbr@1",
  "parameters": { "sheen": 0.5 }
}"#;
    let doc = Document::parse(DocumentKind::Material, Some("odd.material.json"), text).unwrap();
    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(!m.changed());
    assert_eq!(m.diagnostics.len(), 1);
    assert_eq!(m.diagnostics[0].code.as_str(), "FYN-D0018");
    assert!(m.diagnostics[0].message.contains("/parameters/sheen"));
    assert_eq!(m.document.envelope.format_version, 1);
}

#[test]
fn v1_scene_with_a_camera_migrates_to_v2_exactly() {
    let schemas = schemas();
    let path = fixtures().join("v1_lookout.scene.json");
    let doc = Document::read(&path, None).unwrap();
    assert!(doc.is_outdated());
    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(m.diagnostics.is_empty(), "{:#?}", m.diagnostics);
    assert_eq!((m.from, m.to), (1, 6));
    assert_eq!(m.applied.len(), 5);
    assert_eq!(m.applied[0].name, "scene/1-2/camera-exposure");
    assert_eq!(m.applied[1].name, "scene/2-3/animator-layers");
    assert_eq!(m.applied[2].name, "scene/3-4/object-layers");
    assert_eq!(m.applied[3].name, "scene/4-5/one-root");
    assert_eq!(m.applied[4].name, "scene/5-6/rigidbody-constraints");

    let text = m.document.write().unwrap();
    let expected_path = fixtures().join("v1_lookout.scene.migrated.json");
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    if text != expected {
        std::fs::write(expected_path.with_extension("actual.json"), &text).unwrap();
    }
    assert_eq!(text, expected, "see the .actual.json next to the fixture");
    // Every camera gained the two defaults after `priority`; the light and
    // the camera that already had a manual exposure are untouched.
    assert!(
        m.diff.contains("-            \"priority\": 0\n+            \"priority\": 0,\n+            \"exposure_mode\": \"auto\",\n+            \"exposure\": 12.0,\n+            \"culling_mask\": 4294967295\n"),
        "{}",
        m.diff
    );
    assert_eq!(m.diff.matches("+            \"exposure_mode\"").count(), 1);
    let scene = Scene::from_document(m.document.clone());
    assert!(scene.validate(&schemas).is_empty());
    assert!(!describe(&scene, &SchemaSet::embedded()).has_errors());

    let again = migrate(&m.document, &Migrations::builtin(), &schemas);
    assert!(!again.changed());
}

#[test]
fn v2_scene_with_animators_migrates_to_v3_exactly() {
    let schemas = schemas();
    let path = fixtures().join("v2_dancer.scene.json");
    let doc = Document::read(&path, None).unwrap();
    assert!(doc.is_outdated());
    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(m.diagnostics.is_empty(), "{:#?}", m.diagnostics);
    assert_eq!((m.from, m.to), (2, 6));
    assert_eq!(m.applied.len(), 4);
    assert_eq!(m.applied[0].name, "scene/2-3/animator-layers");
    assert_eq!(m.applied[1].name, "scene/3-4/object-layers");
    assert_eq!(m.applied[2].name, "scene/4-5/one-root");
    assert_eq!(m.applied[3].name, "scene/5-6/rigidbody-constraints");

    let text = m.document.write().unwrap();
    let expected_path = fixtures().join("v2_dancer.scene.migrated.json");
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    if text != expected {
        std::fs::write(expected_path.with_extension("actual.json"), &text).unwrap();
    }
    assert_eq!(text, expected, "see the .actual.json next to the fixture");
    // The plain animator gained the three defaults after `playing`; the
    // layer that already carried them is untouched.
    assert!(
        m.diff.contains("-            \"playing\": true\n+            \"playing\": true,\n+            \"weight\": 1.0,\n+            \"additive\": false,\n+            \"root_motion\": false\n"),
        "{}",
        m.diff
    );
    assert_eq!(m.diff.matches("+            \"weight\"").count(), 1);
    let scene = Scene::from_document(m.document.clone());
    assert!(scene.validate(&schemas).is_empty());
    assert!(!describe(&scene, &SchemaSet::embedded()).has_errors());

    let again = migrate(&m.document, &Migrations::builtin(), &schemas);
    assert!(!again.changed());
}

#[test]
fn v3_scene_with_collider_layers_migrates_to_v4_exactly() {
    let schemas = schemas();
    let path = fixtures().join("v3_arena.scene.json");
    let doc = Document::read(&path, None).unwrap();
    assert!(doc.is_outdated());
    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(m.diagnostics.is_empty(), "{:#?}", m.diagnostics);
    assert_eq!((m.from, m.to), (3, 6));
    assert_eq!(m.applied.len(), 3);
    assert_eq!(m.applied[0].name, "scene/3-4/object-layers");
    assert_eq!(m.applied[1].name, "scene/4-5/one-root");
    assert_eq!(m.applied[2].name, "scene/5-6/rigidbody-constraints");

    let text = m.document.write().unwrap();
    let expected_path = fixtures().join("v3_arena.scene.migrated.json");
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    if text != expected {
        std::fs::write(expected_path.with_extension("actual.json"), &text).unwrap();
    }
    assert_eq!(text, expected, "see the .actual.json next to the fixture");
    assert!(
        m.diff.contains("-            \"layer\": 2,\n"),
        "{}",
        m.diff
    );

    let scene = Scene::from_document(m.document.clone());
    let d = scene.validate(&schemas);
    assert!(d.is_empty(), "{d:#?}");
    let layer = |name: &str| {
        let e = scene.entities.iter().find(|e| e.name == name).unwrap();
        e.component_of("fynkel.Layer")
            .map(|c| c.data.get("layer").and_then(|n| n.as_i64()).unwrap())
    };
    // Bit 0 is the default layer: no component. The wall's bit 1 is layer
    // 1, the player's controller bit 2 layer 2; of the sensor's bits 3 and
    // 4 the lowest wins.
    assert_eq!(layer("Ground"), None);
    assert_eq!(layer("Wall"), Some(1));
    assert_eq!(layer("Player"), Some(2));
    assert_eq!(layer("Sensor"), Some(3));
    // The same id on a second run and for every copy of the entity.
    let wall = scene.entities.iter().find(|e| e.name == "Wall").unwrap();
    assert_eq!(
        wall.component_of("fynkel.Layer").unwrap().id.to_string(),
        uuid::Uuid::new_v5(&wall.id, b"fynkel.Layer").to_string()
    );
    // Every camera and light gained `culling_mask` (all layers), and the
    // spot light that had one kept it.
    for (ty, mask) in [
        ("fynkel.Camera", u32::MAX as i64),
        ("fynkel.PointLight", u32::MAX as i64),
        ("fynkel.SpotLight", 6),
    ] {
        let c = scene
            .entities
            .iter()
            .find_map(|e| e.component_of(ty))
            .unwrap();
        assert_eq!(
            c.data.get("culling_mask").and_then(|n| n.as_i64()),
            Some(mask),
            "{ty}"
        );
    }
    // The added collider lost its bits to an added fynkel.Layer right after it.
    let overrides = m
        .document
        .root
        .get("entities")
        .and_then(|e| e.get("b0000000-0000-4000-8000-0000000000e6"))
        .and_then(|e| e.get("prefab"))
        .and_then(|p| p.get("overrides"))
        .and_then(|o| o.as_array())
        .unwrap();
    assert_eq!(overrides.len(), 3);
    assert_eq!(
        overrides[2].get("type").and_then(|n| n.as_str()),
        Some("fynkel.Layer")
    );
    assert_eq!(
        overrides[2]
            .get("data")
            .and_then(|d| d.get("layer"))
            .and_then(|n| n.as_i64()),
        Some(3)
    );
    assert!(!describe(&scene, &SchemaSet::embedded()).has_errors());

    let again = migrate(&m.document, &Migrations::builtin(), &schemas);
    assert!(!again.changed());

    // A `setField` of a collider's `layer` has no equivalent: the
    // migration fails with the override's pointer and nothing changes.
    let text = std::fs::read_to_string(&path).unwrap().replace(
        "\"field\": \"position\",\n            \"value\": [\n              0,\n              1,\n              0\n            ]",
        "\"field\": \"layer\",\n            \"value\": 2",
    );
    assert!(text.contains("\"field\": \"layer\""));
    let doc = Document::parse(DocumentKind::Scene, Some("arena.scene.json"), &text).unwrap();
    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(!m.changed());
    assert_eq!(m.diagnostics[0].code.as_str(), "FYN-D0018");
    assert!(
        m.diagnostics[0]
            .message
            .contains("/entities/b0000000-0000-4000-8000-0000000000e6/prefab/overrides/0"),
        "{}",
        m.diagnostics[0].message
    );
}

#[test]
fn v1_project_moves_its_physics_layers_to_the_top_level() {
    let schemas = schemas();
    // The fixture project as a v1 tool wrote it: the layers under physics.
    let mut v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("../project/fynkel.project.json")).unwrap(),
    )
    .unwrap();
    let layers = v.as_object_mut().unwrap().remove("layers").unwrap();
    assert_eq!(layers, serde_json::json!(["default", "props"]));
    v["formatVersion"] = 1.into();
    v["physics"]["layers"] = layers;
    let text = serde_json::to_string_pretty(&v).unwrap();
    let doc = Document::parse(DocumentKind::Project, Some("fynkel.project.json"), &text).unwrap();
    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(m.diagnostics.is_empty(), "{:#?}", m.diagnostics);
    assert_eq!((m.from, m.to), (1, 2));
    assert_eq!(m.applied[0].name, "project/1-2/layers");
    let root = &m.document.root;
    assert!(root.get("physics").and_then(|p| p.get("layers")).is_none());
    let layers: Vec<&str> = root
        .get("layers")
        .and_then(|l| l.as_array())
        .unwrap()
        .iter()
        .filter_map(|n| n.as_str())
        .collect();
    assert_eq!(layers, ["default", "props"]);
    let again = migrate(&m.document, &Migrations::builtin(), &schemas);
    assert!(!again.changed());
}

#[test]
fn failures_leave_the_document_untouched() {
    let schemas = schemas();
    let reg = Migrations::builtin();

    let doc = Document::read(&fixtures().join("v0_bad_entity.scene.json"), None).unwrap();
    let m = migrate(&doc, &reg, &schemas);
    assert!(!m.changed());
    assert_eq!(m.diagnostics.len(), 1);
    assert_eq!(m.diagnostics[0].code.as_str(), "FYN-D0018");
    assert!(m.diagnostics[0]
        .message
        .contains("/entities/0: id is missing"));
    assert_eq!(m.document.envelope.format_version, 0);

    let doc = Document::read(&fixtures().join("v0_no_path.material.json"), None).unwrap();
    let m = migrate(&doc, &reg, &schemas);
    assert_eq!(m.diagnostics[0].code.as_str(), "FYN-D0017");
    assert_eq!(m.diagnostics[0].line, Some(3));

    // Newer documents do not even parse (read-only, FYN-D0013).
    let err = Document::read(&fixtures().join("newer.scene.json"), None).unwrap_err();
    assert_eq!(err[0].code.as_str(), "FYN-D0013");

    // A step whose output the schema rejects is reported and discarded.
    let mut reg = Migrations::empty();
    for from in [1, 2, 3, 4, 5] {
        reg.add(
            Migrations::builtin()
                .step(DocumentKind::Scene, from)
                .unwrap()
                .clone(),
        );
    }
    reg.add(Migration {
        kind: DocumentKind::Scene,
        from: 0,
        name: "drop-entities",
        description: "test: removes entities",
        apply: |root| {
            if let Some(members) = root.as_object_mut() {
                members.retain(|m| m.key != "entities");
            }
            Ok(())
        },
    });
    let doc = Document::read(&fixtures().join("v0_training.scene.json"), None).unwrap();
    let m = migrate(&doc, &reg, &schemas);
    assert!(!m.changed());
    assert_eq!(m.diagnostics[0].code.as_str(), "FYN-D0018");
    assert!(m.diagnostics.iter().any(|d| d.code.as_str() == "FYN-D0100"));
    assert!(m.document.root.get("entities").is_some());
}

#[test]
fn writing_keeps_a_backup() {
    let schemas = schemas();
    let dir = std::env::temp_dir().join(format!("fynkel-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("room.scene.json");
    let original = std::fs::read(fixtures().join("v0_training.scene.json")).unwrap();
    std::fs::write(&path, &original).unwrap();

    let doc = Document::read(&path, None).unwrap();
    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    let backup = m.write_with_backup(&path).unwrap();
    assert_eq!(backup, backup_path(&path));
    assert_eq!(backup, dir.join("room.scene.json.bak"));
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        std::fs::read_to_string(fixtures().join("v0_training.scene.migrated.json")).unwrap()
    );
    let migrated = Document::read(&path, None).unwrap();
    assert!(migrated.validate(&schemas).is_empty());

    // Nothing to do → nothing written, no new backup.
    let again = migrate(&migrated, &Migrations::builtin(), &schemas);
    assert!(again.write_with_backup(&path).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Foundation 2.5: every `fynkel.RigidBody` — an entity's, and one an
/// instance override adds — gains `constraints: 0` (nothing frozen).
#[test]
fn v5_scene_with_bodies_migrates_to_v6_exactly() {
    let schemas = schemas();
    let path = fixtures().join("v5_crates.scene.json");
    let doc = Document::read(&path, None).unwrap();
    assert!(doc.is_outdated());
    let m = migrate(&doc, &Migrations::builtin(), &schemas);
    assert!(m.diagnostics.is_empty(), "{:#?}", m.diagnostics);
    assert_eq!((m.from, m.to), (5, 6));
    assert_eq!(m.applied.len(), 1);
    assert_eq!(m.applied[0].name, "scene/5-6/rigidbody-constraints");

    let text = m.document.write().unwrap();
    let expected_path = fixtures().join("v5_crates.scene.migrated.json");
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    if text != expected {
        std::fs::write(expected_path.with_extension("actual.json"), &text).unwrap();
    }
    assert_eq!(text, expected, "see the .actual.json next to the fixture");
    assert_eq!(m.diff.matches("+            \"constraints\": 0").count(), 1);
    assert_eq!(
        m.diff.matches("+              \"constraints\": 0").count(),
        1
    );
    let scene = Scene::from_document(m.document.clone());
    // Only the hint that the crate has no collider.
    let diags = scene.validate(&schemas);
    assert!(
        diags.iter().all(|d| d.code.as_str() == "FYN-H0004"),
        "{diags:#?}"
    );

    let again = migrate(&m.document, &Migrations::builtin(), &schemas);
    assert!(!again.changed());
}
