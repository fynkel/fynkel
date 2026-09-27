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
    assert!(turret
        .entity(turret.root)
        .is_some_and(|r| r.parent.is_none()));
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
    // The base is the prefab's root: it maps to the instance entity itself,
    // every other entity to a derived id.
    assert_eq!(a.entity(base_local), Some(a.anchor));
    assert_eq!(
        a.entity(barrel_local),
        Some(derive_id(a.instance, barrel_local))
    );
    let base = out.entity(a.anchor).unwrap();
    assert_eq!(base.name, "Turret A");
    assert_eq!(base.children, vec![a.entity(barrel_local).unwrap()]);
    let barrel = out.entity(a.entity(barrel_local).unwrap()).unwrap();
    assert_eq!(barrel.parent, Some(a.anchor));
    assert_eq!(barrel.children, vec![a.entity(muzzle_local).unwrap()]);
    assert!(barrel.component_of("fynkel.PointLight").is_some());
    let renderer = base.component_of("fynkel.MeshRenderer").unwrap();
    assert_eq!(
        renderer.data.get("cast_shadows").and_then(|n| n.as_bool()),
        Some(false)
    );
    // Instance B removed the barrel subtree and the renderer; its authored
    // child (Label) comes after the root's children, of which none are left.
    let b = &expanded.instances[1];
    assert!(b.entity(barrel_local).is_none());
    let base_b = out.entity(b.anchor).unwrap();
    assert!(base_b.component_of("fynkel.MeshRenderer").is_none());
    assert_eq!(base_b.children.len(), 1);
    assert_eq!(out.entity(base_b.children[0]).unwrap().name, "Label");

    // And it loads.
    let described = describe(&out, &schemas);
    assert!(!described.has_errors());
    assert_eq!(described.description.entities.len(), 3 + 2);
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

    // The flattened `Nested`: the holder is the inner instance, so it is
    // the turret's root (its components) and the barrel hangs off it.
    let nested = flat
        .get("10000000-0000-4000-8000-000000000003".parse().unwrap())
        .unwrap();
    let holder: Uuid = "10000000-0000-4000-8000-0000000000b1".parse().unwrap();
    let inner_instance: Uuid = "10000000-0000-4000-8000-0000000000e1".parse().unwrap();
    let barrel = derive_id(
        inner_instance,
        "10000000-0000-4000-8000-0000000000b2".parse().unwrap(),
    );
    assert_eq!(nested.root, holder);
    assert_eq!(nested.entities.len(), 2);
    assert_eq!(nested.entity(holder).unwrap().children, vec![barrel]);
    assert!(nested
        .entity(holder)
        .unwrap()
        .component_of("fynkel.MeshRenderer")
        .is_some());
    assert!(nested.validate(&schemas).is_empty());

    // A scene instantiating `Nested` gets the same two entities, remapped
    // once more by the scene instance (the holder becomes the instance
    // itself); one that instantiates a cyclic prefab is refused for that
    // instance (and only sees its own prefab's problems).
    let scene_json = r#"{
      "$schema": "https://fynkel.dev/schemas/scene.schema.json",
      "formatVersion": 6,
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
    let stand: Uuid = "20000000-0000-4000-8000-0000000000a1".parse().unwrap();
    assert_eq!(inst.entities.len(), 2);
    assert_eq!(inst.entity(holder), Some(stand));
    let out = expanded.scene();
    assert_eq!(out.entities.len(), 2);
    assert_eq!(out.entities[0].name, "Turret Stand");
    assert!(out.entities[0]
        .component_of("fynkel.MeshRenderer")
        .is_some());
    assert_eq!(
        out.entity(stand).unwrap().children,
        vec![inst.entity(barrel).unwrap()]
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

/// A script field inside a prefab referencing the prefab's own object and
/// the script on it (`{"entity", "component"}`, the Inspector's object
/// field) points at the instance's copies after expansion: the entity and
/// the component id are both derived; an id outside the prefab is kept.
#[test]
fn script_references_inside_a_prefab_reach_the_instance_copies() {
    let schemas = schemas();
    let prefab_json = r#"{
      "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
      "formatVersion": 6,
      "id": "30000000-0000-4000-8000-000000000001",
      "name": "Turret",
      "root": "30000000-0000-4000-8000-0000000000b1",
      "entities": {
        "30000000-0000-4000-8000-0000000000b1": {
          "name": "Turret", "parent": null, "children": ["30000000-0000-4000-8000-0000000000b2"],
          "components": {
            "30000000-0000-4000-8000-0000000000c1": { "type": "game.Turret", "version": 1, "data": {
              "target": { "entity": "30000000-0000-4000-8000-0000000000b2" },
              "spinner": { "entity": "30000000-0000-4000-8000-0000000000b2", "component": "30000000-0000-4000-8000-0000000000c2" },
              "outside": { "entity": "30000000-0000-4000-8000-0000000000ff" }
            } }
          }
        },
        "30000000-0000-4000-8000-0000000000b2": {
          "name": "Barrel", "parent": "30000000-0000-4000-8000-0000000000b1", "children": [],
          "components": {
            "30000000-0000-4000-8000-0000000000c2": { "type": "game.Spinner", "version": 1, "data": {} }
          }
        }
      }
    }"#;
    let scene_json = r#"{
      "$schema": "https://fynkel.dev/schemas/scene.schema.json",
      "formatVersion": 6,
      "id": "30000000-0000-4000-8000-000000000002",
      "name": "Turrets",
      "entities": {
        "30000000-0000-4000-8000-0000000000a1": {
          "name": "Turret A", "parent": null, "children": [], "components": {},
          "prefab": { "asset": { "asset": "30000000-0000-4000-8000-000000000001" }, "instance": "30000000-0000-4000-8000-0000000000e1" }
        }
      }
    }"#;
    let mut lib = PrefabLibrary::empty();
    lib.add(Prefab::from_document(
        Document::parse(
            DocumentKind::Prefab,
            Some("turret.prefab.json"),
            prefab_json,
        )
        .unwrap(),
    ));
    let scene = Scene::from_document(
        Document::parse(DocumentKind::Scene, Some("turrets.scene.json"), scene_json).unwrap(),
    );
    let expanded = expand(&scene, &lib, &schemas);
    let inst = &expanded.instances[0];
    let barrel_local: Uuid = "30000000-0000-4000-8000-0000000000b2".parse().unwrap();
    let spinner_local: Uuid = "30000000-0000-4000-8000-0000000000c2".parse().unwrap();
    let barrel = inst.entity(barrel_local).unwrap().to_string();
    let spinner = derive_id(inst.instance, spinner_local).to_string();
    let out = expanded.scene();
    let root = out.entity(inst.anchor).unwrap();
    let data = &root.component_of("game.Turret").unwrap().data;
    let field = |f: &str, k: &str| {
        data.get(f)
            .and_then(|n| n.get(k))
            .and_then(|n| n.as_str())
            .map(str::to_string)
    };
    assert_eq!(field("target", "entity"), Some(barrel.clone()));
    assert_eq!(field("spinner", "entity"), Some(barrel.clone()));
    assert_eq!(field("spinner", "component"), Some(spinner.clone()));
    assert_eq!(
        field("outside", "entity").as_deref(),
        Some("30000000-0000-4000-8000-0000000000ff")
    );
    // The barrel's Spinner is that component.
    let barrel_entity = out.entity(barrel.parse().unwrap()).unwrap();
    assert_eq!(
        barrel_entity
            .component_of("game.Spinner")
            .unwrap()
            .id
            .to_string(),
        spinner
    );
}

// ------------------------------------------------------------- variants

const TURRET: &str = "10000000-0000-4000-8000-000000000001";
const T_BASE: &str = "10000000-0000-4000-8000-0000000000b1";
const T_BARREL: &str = "10000000-0000-4000-8000-0000000000b2";
const T_RENDERER: &str = "10000000-0000-4000-8000-0000000000c2";
const T_BARREL_TRANSFORM: &str = "10000000-0000-4000-8000-0000000000c3";
const VARIANT: &str = "50000000-0000-4000-8000-000000000001";
const V_ROOT: &str = "50000000-0000-4000-8000-0000000000a1";
const V_INSTANCE: &str = "50000000-0000-4000-8000-0000000000e1";
const V_SIGHT: &str = "50000000-0000-4000-8000-0000000000b9";
const V_FLAG: &str = "50000000-0000-4000-8000-0000000000f1";
const V_FLAG_TRANSFORM: &str = "50000000-0000-4000-8000-0000000000cf";

fn uu(s: &str) -> Uuid {
    s.parse().unwrap()
}

fn transform_at(y: f64) -> serde_json::Value {
    serde_json::json!({ "type": "fynkel.Transform", "version": 1,
        "data": { "position": [0, y, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] } })
}

fn prefab_doc(path: &str, v: serde_json::Value) -> Prefab {
    Prefab::from_document(
        Document::parse(DocumentKind::Prefab, Some(path), &v.to_string()).unwrap(),
    )
}

/// The Turret's variant: its root is an instance of the Turret (no
/// shadows, a Sight added under the barrel) with a Flag of its own.
fn variant() -> Prefab {
    prefab_doc(
        "turret_variant.prefab.json",
        serde_json::json!({
            "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
            "formatVersion": 6, "id": VARIANT, "name": "Turret Variant", "root": V_ROOT,
            "entities": {
                V_ROOT: { "name": "Base", "parent": null, "children": [V_FLAG], "components": {},
                    "prefab": { "asset": { "asset": TURRET }, "instance": V_INSTANCE, "overrides": [
                        { "op": "setField", "entity": T_BASE, "component": T_RENDERER, "field": "cast_shadows", "value": false },
                        { "op": "addObject", "entity": V_SIGHT, "name": "Sight", "parent": T_BARREL,
                          "components": { "50000000-0000-4000-8000-0000000000c9": transform_at(0.25) } }
                    ] } },
                V_FLAG: { "name": "Flag", "parent": V_ROOT, "children": [],
                    "components": { V_FLAG_TRANSFORM: transform_at(2.0) } }
            }
        }),
    )
}

/// Prefab variants (ADR-0022 amendment 2026-09-25): a prefab whose root
/// is an instance of another validates, flattens with the base's entities
/// under ids derived from the root's instance (its overrides applied, its
/// own children after the base's), a variant of the variant flattens once
/// more, and a scene instance of the chain expands to one root with every
/// level's overrides; a variant of itself is D0402.
#[test]
fn variants_flatten_through_chains_and_refuse_themselves() {
    let schemas = schemas();
    let v = variant();
    assert!(
        v.validate(&schemas).is_empty(),
        "{:#?}",
        v.validate(&schemas)
    );
    assert_eq!(v.variant_of().unwrap().prefab_id(), uu(TURRET));
    assert_eq!(
        fynkel_document::prefab::inspect_json(&v)["variantOf"],
        serde_json::json!({ "asset": TURRET })
    );
    let turret_barrel = derive_id(uu(V_INSTANCE), uu(T_BARREL));
    let turret_barrel_t = derive_id(uu(V_INSTANCE), uu(T_BARREL_TRANSFORM));
    let sight = derive_id(uu(V_INSTANCE), uu(V_SIGHT));

    // A variant of the variant moves the (variant's) barrel.
    const CHAIN: &str = "50000000-0000-4000-8000-000000000002";
    const C_ROOT: &str = "50000000-0000-4000-8000-0000000000a2";
    const C_INSTANCE: &str = "50000000-0000-4000-8000-0000000000e2";
    let chain = prefab_doc(
        "turret_variant_2.prefab.json",
        serde_json::json!({
            "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
            "formatVersion": 6, "id": CHAIN, "name": "Turret Variant 2", "root": C_ROOT,
            "entities": { C_ROOT: { "name": "Base", "parent": null, "children": [], "components": {},
                "prefab": { "asset": { "asset": VARIANT }, "instance": C_INSTANCE, "overrides": [
                    { "op": "setField", "entity": turret_barrel.to_string(), "component": turret_barrel_t.to_string(),
                      "field": "position", "value": [0, 3, 0] }
                ] } } }
        }),
    );
    assert!(chain.validate(&schemas).is_empty());
    let mut lib = library();
    lib.add(v.clone());
    lib.add(chain);
    let (flat, diags) = lib.flattened(&schemas);
    assert!(
        diags
            .iter()
            .all(|d| d.code == fynkel_contracts::Code::PREFAB_NESTED),
        "only the cycle fixtures: {diags:#?}"
    );

    // The variant, flat: Base (the Turret's root, no shadows) › Barrel ›
    // Sight, then its own Flag.
    let fv = flat.get(uu(VARIANT)).unwrap();
    assert!(!fv.has_instances());
    assert_eq!(fv.root, uu(V_ROOT));
    assert!(
        fv.validate(&schemas).is_empty(),
        "{:#?}",
        fv.validate(&schemas)
    );
    let root = fv.entity(uu(V_ROOT)).unwrap();
    assert_eq!(root.name, "Base");
    assert_eq!(root.children, vec![turret_barrel, uu(V_FLAG)]);
    let r = root.component_of("fynkel.MeshRenderer").unwrap();
    assert_eq!(r.id, derive_id(uu(V_INSTANCE), uu(T_RENDERER)));
    assert_eq!(r.data.get("cast_shadows").unwrap().as_bool(), Some(false));
    assert_eq!(fv.entity(turret_barrel).unwrap().children, vec![sight]);
    assert_eq!(fv.entities.len(), 4);

    // The chain, flat: every id derived once more, the barrel moved.
    let fc = flat.get(uu(CHAIN)).unwrap();
    assert_eq!(fc.root, uu(C_ROOT));
    assert_eq!(fc.entities.len(), 4);
    let c_barrel = derive_id(uu(C_INSTANCE), turret_barrel);
    let moved = fc
        .entity(c_barrel)
        .unwrap()
        .component_of("fynkel.Transform")
        .unwrap();
    assert_eq!(moved.id, derive_id(uu(C_INSTANCE), turret_barrel_t));
    assert_eq!(
        moved.data.get("position").unwrap().to_serde(),
        serde_json::json!([0, 3, 0])
    );

    // A scene instance of the chain overrides the variant's Flag, which
    // the chain shows under its derived id.
    let flag = derive_id(uu(C_INSTANCE), uu(V_FLAG));
    let flag_t = derive_id(uu(C_INSTANCE), uu(V_FLAG_TRANSFORM));
    let scene_json = serde_json::json!({
        "$schema": "https://fynkel.dev/schemas/scene.schema.json",
        "formatVersion": 6, "id": "50000000-0000-4000-8000-000000000003", "name": "Variants",
        "entities": { "50000000-0000-4000-8000-0000000000a3": {
            "name": "Turret V2", "parent": null, "children": [], "components": {},
            "prefab": { "asset": { "asset": CHAIN }, "instance": "50000000-0000-4000-8000-0000000000e3", "overrides": [
                { "op": "setField", "entity": flag.to_string(), "component": flag_t.to_string(),
                  "field": "position", "value": [0, 5, 0] }
            ] } } }
    });
    let scene = Scene::from_document(
        Document::parse(
            DocumentKind::Scene,
            Some("variants.scene.json"),
            &scene_json.to_string(),
        )
        .unwrap(),
    );
    let expanded = expand(&scene, &lib, &schemas);
    assert!(!expanded.has_errors(), "{:#?}", expanded.diagnostics);
    let out = expanded.scene();
    assert!(out.validate(&schemas).is_empty());
    assert!(!describe(&out, &schemas).has_errors());
    assert_eq!(
        out.entities.iter().filter(|e| e.parent.is_none()).count(),
        1,
        "one root"
    );
    let top = out
        .entity(uu("50000000-0000-4000-8000-0000000000a3"))
        .unwrap();
    assert_eq!(top.name, "Turret V2");
    assert_eq!(
        top.component_of("fynkel.MeshRenderer")
            .unwrap()
            .data
            .get("cast_shadows")
            .unwrap()
            .as_bool(),
        Some(false)
    );
    let names: Vec<&str> = out.entities.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Turret V2", "Barrel", "Sight", "Flag"]);
    let e3 = uu("50000000-0000-4000-8000-0000000000e3");
    let position = |id: Uuid| {
        out.entity(id)
            .unwrap()
            .component_of("fynkel.Transform")
            .unwrap()
            .data
            .get("position")
            .unwrap()
            .to_serde()
    };
    assert_eq!(position(derive_id(e3, flag)), serde_json::json!([0, 5, 0]));
    assert_eq!(
        position(derive_id(e3, c_barrel)),
        serde_json::json!([0, 3, 0])
    );

    // A variant of itself: D0402 in the prefab.
    let own = prefab_doc(
        "self.prefab.json",
        serde_json::json!({
            "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
            "formatVersion": 6, "id": "50000000-0000-4000-8000-000000000009", "name": "Self",
            "root": "50000000-0000-4000-8000-0000000000a9",
            "entities": { "50000000-0000-4000-8000-0000000000a9": {
                "name": "Self", "parent": null, "children": [], "components": {},
                "prefab": { "asset": { "asset": "50000000-0000-4000-8000-000000000009" },
                            "instance": "50000000-0000-4000-8000-0000000000e9" } } }
        }),
    );
    let mut lib = PrefabLibrary::empty();
    lib.add(own);
    let (_, diags) = lib.flattened(&schemas);
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(diags[0].code.as_str(), "FYN-D0402");
    assert_eq!(
        diags[0].pointer.as_deref(),
        Some("/entities/50000000-0000-4000-8000-0000000000a9/prefab/asset")
    );
}
