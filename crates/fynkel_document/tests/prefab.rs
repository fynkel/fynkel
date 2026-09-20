//! Prefab corpus (`fixtures/prefab/*`): scenes with instances are validated
//! and expanded against every `*.prefab.json` under `fixtures/`; each
//! `*.expected.json` lists the diagnostics (code, pointer, line, column) in
//! order. `instances_ok.scene.expanded.json` is the exact expansion (the
//! "prefab references remap correctly" gate): derived ids are deterministic,
//! so the file is stable.

use std::path::{Path, PathBuf};

use fynkel_document::prefab::derive_id;
use fynkel_document::{
    describe, expand, Diagnostic, Document, DocumentKind, Prefab, PrefabLibrary, Scene, SchemaSet,
};
use uuid::Uuid;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

fn library() -> PrefabLibrary {
    let (lib, diags) = PrefabLibrary::scan(&fixtures());
    assert!(diags.is_empty(), "{diags:#?}");
    assert!(lib.len() >= 4);
    lib
}

fn check(path: &Path, diags: &[Diagnostic]) {
    let expected: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(path.to_string_lossy().replace(".json", ".expected.json"))
            .unwrap(),
    )
    .unwrap();
    let shown = format!(
        "{}:\n{}",
        path.display(),
        diags.iter().map(|d| format!("  {d}\n")).collect::<String>()
    );
    assert_eq!(diags.len(), expected.len(), "{shown}");
    for (d, e) in diags.iter().zip(&expected) {
        assert_eq!(d.code.as_str(), e["code"].as_str().unwrap(), "{shown}");
        assert_eq!(d.pointer.as_deref(), e["pointer"].as_str(), "{shown}");
        assert_eq!(
            d.severity.as_str(),
            e.get("severity")
                .and_then(|s| s.as_str())
                .unwrap_or("error"),
            "{shown}"
        );
        assert_eq!(d.line, e["line"].as_u64().map(|v| v as u32), "{shown}");
        assert_eq!(d.column, e["column"].as_u64().map(|v| v as u32), "{shown}");
        assert!(d.document.is_some());
    }
}

fn scene_fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixtures().join("prefab"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".scene.json"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn scene_fixtures_validate_and_expand_with_exact_diagnostics() {
    let schemas = schemas();
    let lib = library();
    let paths = scene_fixtures();
    assert!(paths.len() >= 4);
    for path in paths {
        let scene = Scene::from_document(Document::read(&path, Some(DocumentKind::Scene)).unwrap());
        let mut diags = scene.validate(&schemas);
        diags.extend(expand(&scene, &lib, &schemas).diagnostics);
        check(&path, &diags);
    }
}

#[test]
fn prefab_fixtures_report_exact_diagnostics() {
    let schemas = schemas();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixtures().join("prefab"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".prefab.json"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 2);
    for path in paths {
        let prefab =
            Prefab::from_document(Document::read(&path, Some(DocumentKind::Prefab)).unwrap());
        check(&path, &prefab.validate(&schemas));
    }
    let turret = Prefab::from_document(
        Document::read(&fixtures().join("valid/turret.prefab.json"), None).unwrap(),
    );
    assert!(turret.validate(&schemas).is_empty());
    assert_eq!(turret.roots.len(), 1);
    assert_eq!(turret.entities.len(), 2);
}

#[test]
fn expansion_is_exact_and_deterministic() {
    let schemas = schemas();
    let lib = library();
    let path = fixtures().join("prefab/instances_ok.scene.json");
    let scene = Scene::from_document(Document::read(&path, None).unwrap());
    let expanded = expand(&scene, &lib, &schemas);
    assert!(
        expanded.diagnostics.is_empty(),
        "{:#?}",
        expanded.diagnostics
    );
    assert_eq!(expanded.instances.len(), 2);

    let text = expanded.document.write().unwrap();
    let expected_path = fixtures().join("prefab/instances_ok.scene.expanded.json");
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    if text != expected {
        std::fs::write(expected_path.with_extension("actual.json"), &text).unwrap();
    }
    assert_eq!(
        text, expected,
        "expansion differs; see the .actual.json next to it"
    );

    // Same input, same output.
    let again = expand(&scene, &lib, &schemas);
    assert_eq!(again.document.write().unwrap(), text);
    assert_eq!(again.instances, expanded.instances);

    // The expanded scene is a valid scene with no instances left ...
    let out = expanded.scene();
    assert!(out.instances().next().is_none());
    let diags = out.validate(&schemas);
    assert!(diags.is_empty(), "{diags:#?}");

    // ... whose ids follow the derivation rule and whose references point
    // at derived ids.
    let a = &expanded.instances[0];
    let base_local: Uuid = "10000000-0000-4000-8000-0000000000b1".parse().unwrap();
    let barrel_local: Uuid = "10000000-0000-4000-8000-0000000000b2".parse().unwrap();
    let muzzle_local: Uuid = "10000000-0000-4000-8000-0000000000b3".parse().unwrap();
    assert_eq!(
        a.entity(base_local),
        Some(derive_id(a.instance, base_local))
    );
    let base = out.entity(a.entity(base_local).unwrap()).unwrap();
    assert_eq!(base.parent, Some(a.anchor));
    assert_eq!(base.children, vec![a.entity(barrel_local).unwrap()]);
    let anchor = out.entity(a.anchor).unwrap();
    assert_eq!(anchor.children, vec![a.entity(base_local).unwrap()]);
    let barrel = out.entity(a.entity(barrel_local).unwrap()).unwrap();
    assert_eq!(barrel.children, vec![a.entity(muzzle_local).unwrap()]);
    assert!(barrel.component_of("fynkel.PointLight").is_some());
    let renderer = base.component_of("fynkel.MeshRenderer").unwrap();
    assert_eq!(
        renderer.data.get("cast_shadows").and_then(|n| n.as_bool()),
        Some(false)
    );
    // Instance B removed the barrel subtree and the renderer.
    let b = &expanded.instances[1];
    assert!(b.entity(barrel_local).is_none());
    let base_b = out.entity(b.entity(base_local).unwrap()).unwrap();
    assert!(base_b.children.is_empty());
    assert!(base_b.component_of("fynkel.MeshRenderer").is_none());
    let anchor_b = out.entity(b.anchor).unwrap();
    // Authored children first, then the instance roots.
    assert_eq!(anchor_b.children.len(), 2);
    assert_eq!(anchor_b.children[1], b.entity(base_local).unwrap());

    // And it loads.
    let described = describe(&out, &schemas);
    assert!(!described.has_errors());
    assert_eq!(described.description.entities.len(), 3 + 3 + 1);
}

#[test]
fn derived_ids_are_stable_and_distinct_per_instance() {
    let local: Uuid = "10000000-0000-4000-8000-0000000000b1".parse().unwrap();
    let i1: Uuid = "20000000-0000-4000-8000-0000000000e1".parse().unwrap();
    let i2: Uuid = "20000000-0000-4000-8000-0000000000e2".parse().unwrap();
    assert_eq!(derive_id(i1, local), derive_id(i1, local));
    assert_ne!(derive_id(i1, local), derive_id(i2, local));
    assert_ne!(derive_id(i1, local), local);
    assert!(fynkel_document::parse_uuid(&derive_id(i1, local).to_string()).is_ok());
}

/// Nested prefabs (M3-08b): `nested.prefab.json` instantiates the turret;
/// flattening the library expands it into plain entities (innermost first,
/// ids derived from the inner instance id), a scene instantiating `Nested`
/// gets the turret's entities under the holder, and the `cycle_a`/`cycle_b`
/// pair is refused with `FYN-D0402` in the prefab documents, once each.
#[test]
fn nested_prefabs_flatten_and_cycles_are_refused() {
    let schemas = schemas();
    let lib = library();
    assert!(lib.has_instances());
    let (flat, diags) = lib.flattened(&schemas);
    assert_eq!(flat.len(), lib.len());
    assert!(!flat
        .get("10000000-0000-4000-8000-000000000003".parse().unwrap())
        .unwrap()
        .has_instances());
    assert!(
        diags
            .iter()
            .all(|d| d.code == fynkel_contracts::Code::PREFAB_NESTED),
        "{diags:#?}"
    );
    // Flattening A reaches B, which reaches A again: the cycle is reported
    // in B (the instance that closes it) and A stays unresolved for it.
    let mut where_: Vec<(String, String)> = diags
        .iter()
        .map(|d| (d.document.clone().unwrap(), d.pointer.clone().unwrap()))
        .collect();
    where_.sort();
    assert_eq!(where_.len(), 2, "{diags:#?}");
    assert!(where_[0].0.ends_with("cycle_a.prefab.json"), "{where_:?}");
    assert_eq!(
        where_[0].1,
        "/entities/10000000-0000-4000-8000-0000000000c1/prefab/asset"
    );
    assert!(where_[1].0.ends_with("cycle_b.prefab.json"), "{where_:?}");
    assert_eq!(
        where_[1].1,
        "/entities/10000000-0000-4000-8000-0000000000d1/prefab/asset"
    );
    // Idempotent: a flat library flattens to itself, silently.
    let (again, none) = flat.flattened(&schemas);
    assert!(none.is_empty());
    assert_eq!(again.len(), flat.len());

    // The flattened `Nested`: holder → base (derived from the inner instance) → barrel.
    let nested = flat
        .get("10000000-0000-4000-8000-000000000003".parse().unwrap())
        .unwrap();
    let holder: Uuid = "10000000-0000-4000-8000-0000000000b1".parse().unwrap();
    let inner_instance: Uuid = "10000000-0000-4000-8000-0000000000e1".parse().unwrap();
    let base = derive_id(
        inner_instance,
        "10000000-0000-4000-8000-0000000000b1".parse().unwrap(),
    );
    let barrel = derive_id(
        inner_instance,
        "10000000-0000-4000-8000-0000000000b2".parse().unwrap(),
    );
    assert_eq!(nested.entities.len(), 3);
    assert_eq!(nested.entity(holder).unwrap().children, vec![base]);
    assert_eq!(nested.entity(base).unwrap().children, vec![barrel]);
    assert!(nested
        .entity(base)
        .unwrap()
        .component_of("fynkel.MeshRenderer")
        .is_some());
    assert!(nested.validate(&schemas).is_empty());

    // A scene instantiating `Nested` gets the same three entities, remapped
    // once more by the scene instance; one that instantiates a cyclic
    // prefab is refused for that instance (and only sees its own prefab's
    // problems).
    let scene_json = r#"{
      "$schema": "https://fynkel.dev/schemas/scene.schema.json",
      "formatVersion": 2,
      "id": "20000000-0000-4000-8000-000000000077",
      "name": "Nesting",
      "entities": {
        "20000000-0000-4000-8000-0000000000a1": {
          "name": "Turret Stand", "parent": null, "children": [], "components": {},
          "prefab": { "asset": { "asset": "10000000-0000-4000-8000-000000000003" }, "instance": "20000000-0000-4000-8000-0000000000e1" }
        }
      }
    }"#;
    let scene = Scene::from_document(
        Document::parse(DocumentKind::Scene, Some("nesting.scene.json"), scene_json).unwrap(),
    );
    assert!(scene.validate(&schemas).is_empty());
    let expanded = expand(&scene, &lib, &schemas);
    assert!(
        expanded.diagnostics.is_empty(),
        "{:#?}",
        expanded.diagnostics
    );
    assert_eq!(expanded.instances.len(), 1);
    let inst = &expanded.instances[0];
    assert_eq!(inst.entities.len(), 3);
    let out = expanded.scene();
    assert_eq!(out.entities.len(), 4);
    assert_eq!(
        out.entity(inst.entity(holder).unwrap()).unwrap().children,
        vec![inst.entity(base).unwrap()]
    );
    assert!(out.validate(&schemas).is_empty());
    assert!(!describe(&out, &schemas).has_errors());

    let cyclic = scene_json.replace(
        "10000000-0000-4000-8000-000000000003",
        "10000000-0000-4000-8000-000000000004",
    );
    let scene = Scene::from_document(
        Document::parse(DocumentKind::Scene, Some("cyclic.scene.json"), &cyclic).unwrap(),
    );
    let expanded = expand(&scene, &lib, &schemas);
    let codes: Vec<&str> = expanded
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(
        codes,
        ["FYN-D0402", "FYN-D0402", "FYN-D0402"],
        "{:#?}",
        expanded.diagnostics
    );
    assert!(expanded.diagnostics[2].document.as_deref() == Some("cyclic.scene.json"));
    assert_eq!(
        expanded.diagnostics[2].pointer.as_deref(),
        Some("/entities/20000000-0000-4000-8000-0000000000a1/prefab/asset")
    );
}
