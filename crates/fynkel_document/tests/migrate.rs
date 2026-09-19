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
            "prefab/1-2/camera-exposure",
            "scene/0-1/entities-keyed-by-id",
            "scene/1-2/camera-exposure"
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
    // Two steps chain: 0 → 1 (keyed entities), 1 → 2 (camera exposure —
    // nothing to do here, the scene has no camera).
    assert_eq!((m.from, m.to), (0, 2));
    assert_eq!(m.applied.len(), 2);
    assert_eq!(m.applied[0].name, "scene/0-1/entities-keyed-by-id");
    assert_eq!(m.applied[1].name, "scene/1-2/camera-exposure");
    assert_eq!(m.document.envelope.format_version, 2);
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
        .contains("-  \"formatVersion\": 0,\n+  \"formatVersion\": 2,"));
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
    assert_eq!((m.from, m.to), (1, 2));
    assert_eq!(m.applied.len(), 1);
    assert_eq!(m.applied[0].name, "scene/1-2/camera-exposure");

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
        m.diff.contains("-            \"priority\": 0\n+            \"priority\": 0,\n+            \"exposure_mode\": \"auto\",\n+            \"exposure\": 12.0\n"),
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
    reg.add(
        Migrations::builtin()
            .step(DocumentKind::Scene, 1)
            .unwrap()
            .clone(),
    );
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
