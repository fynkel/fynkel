//! Scene semantics corpus (`fixtures/scene/*`): each scene's
//! `*.expected.json` lists the diagnostics `Scene::validate` must report
//! (code, pointer, optional id/severity), in order; an empty list means clean.

use std::path::{Path, PathBuf};

use fynkel_document::{Document, DocumentKind, Scene, SchemaSet};

#[test]
fn scene_fixtures_report_exact_semantics() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/scene");
    let schemas =
        SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".scene.json"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 10);
    for path in paths {
        let expected: Vec<serde_json::Value> = serde_json::from_str(
            &std::fs::read_to_string(path.to_string_lossy().replace(".json", ".expected.json"))
                .unwrap(),
        )
        .unwrap();
        let doc = Document::read(&path, Some(DocumentKind::Scene)).unwrap();
        let scene = Scene::from_document(doc);
        let diags = scene.validate(&schemas);
        let shown = format!(
            "{}:\n{}",
            path.display(),
            diags.iter().map(|d| format!("  {d}\n")).collect::<String>()
        );
        assert_eq!(diags.len(), expected.len(), "{shown}");
        for (d, e) in diags.iter().zip(&expected) {
            assert_eq!(d.code.as_str(), e["code"].as_str().unwrap(), "{shown}");
            assert_eq!(d.pointer.as_deref(), e["pointer"].as_str(), "{shown}");
            if let Some(id) = e.get("id") {
                assert_eq!(d.id.as_deref(), id.as_str(), "{shown}");
            }
            assert_eq!(
                d.severity.as_str(),
                e.get("severity")
                    .and_then(|s| s.as_str())
                    .unwrap_or("error"),
                "{shown}"
            );
            assert!(d.line.is_some() && d.column.is_some(), "{shown}");
            assert!(d.document.is_some());
        }
    }
}

#[test]
fn scene_view_is_lenient_and_ordered() {
    let text = r#"{
  "formatVersion": 4,
  "id": "70d7ba8b-f7fb-4eba-920d-ec9e6b968330",
  "name": "Lenient",
  "entities": {
    "aaaaaaaa-0000-4000-8000-000000000001": { "name": "Second root", "parent": null, "children": [], "components": {} },
    "not-a-uuid": { "name": "Skipped", "parent": null, "children": [], "components": {} },
    "bbbbbbbb-0000-4000-8000-000000000002": { "name": "Kid", "parent": "aaaaaaaa-0000-4000-8000-000000000001", "children": [], "components": {} }
  }
}"#;
    let doc = Document::parse(DocumentKind::Scene, None, text).unwrap();
    let scene = Scene::from_document(doc);
    assert_eq!(scene.entities.len(), 2);
    assert_eq!(scene.roots().count(), 1);
    assert_eq!(scene.read_diagnostics().len(), 1);
    assert_eq!(scene.read_diagnostics()[0].code.as_str(), "FYN-D0012");
    let json = fynkel_document::scene::inspect_json(&scene);
    assert_eq!(json["entityCount"], 2);
    assert_eq!(json["roots"][0]["name"], "Second root");
    // Kid is not in the root's children list: the tree omits it, validation reports it.
    assert_eq!(json["roots"][0]["children"].as_array().unwrap().len(), 0);
    let diags = scene.validate(&SchemaSet::embedded());
    assert!(diags.iter().any(|d| d.code.as_str() == "FYN-D0202"));
}
