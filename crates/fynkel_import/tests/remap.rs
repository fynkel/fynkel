//! Material remapping (`docs/design/material-remapping.md`, M1): a
//! model's `settings.materials` points the renderers of its hierarchy at
//! project materials. Instances pick it up, their overrides still win, an
//! explicit reference to the embedded material is left alone, and the
//! remap target is loaded with the model without being a key input.

use std::path::{Path, PathBuf};

use fynkel_contracts::Code;
use fynkel_cooked::Container;
use fynkel_document::asset::AssetDatabase;
use fynkel_document::SchemaSet;
use fynkel_import::{cook, Cache, CookOptions, CookReport, Status};
use serde_json::{json, Value};
use uuid::Uuid;

const ROBOT: &str = "c0000000-0000-4000-8000-000000000001";
const SWATCH: &str = "c0000000-0000-4000-8000-000000000002";
const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";
const ROBOTS: &str = "c0000000-0000-4000-8000-0000000000a0";

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn schemas() -> SchemaSet {
    SchemaSet::embedded()
}

/// A copy of the import corpus (`robot.glb` with `material/Painted` and
/// `material/Glass`, `rusted.material.json`, `swatch.png`), removed on drop.
struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str) -> Project {
        let dir = std::env::temp_dir().join(format!("fynkel-remap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/project/assets"),
            &dir.join("assets"),
        );
        Project { dir }
    }

    fn assets(&self) -> PathBuf {
        self.dir.join("assets")
    }

    fn cook(&self) -> CookReport {
        let (db, diags) = AssetDatabase::scan(&self.assets(), &schemas());
        assert!(!fynkel_contracts::has_errors(&diags), "{diags:?}");
        cook(
            &db,
            &schemas(),
            &Cache::at(&self.dir.join("cache")),
            &CookOptions::default(),
        )
    }

    fn write(&self, rel: &str, v: &Value) {
        std::fs::write(
            self.assets().join(rel),
            serde_json::to_string_pretty(v).unwrap(),
        )
        .unwrap();
    }

    /// The robot's sidecar with `settings.materials` set to `materials`.
    fn remap(&self, materials: Value) {
        let path = self.assets().join("models/robot.glb.meta.json");
        let mut meta: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        meta["settings"]["materials"] = materials;
        self.write("models/robot.glb.meta.json", &meta);
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The JSON chunk `sub` of an asset's cooked object (`""` for a
/// document's own).
fn json_chunk(r: &CookReport, asset: &str, sub: &str) -> Value {
    let a = r.get(id(asset)).unwrap();
    let bytes = std::fs::read(a.file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    let meta: Value = serde_json::from_slice(c.meta(1 << 20).unwrap()).unwrap();
    let i = meta["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c["kind"] == "JSON" && c["sub"].as_str().unwrap_or("") == sub)
        .unwrap_or_else(|| panic!("{asset} has no JSON chunk {sub:?}: {meta}"));
    serde_json::from_slice(c.read(i, 1 << 20).unwrap()).unwrap()
}

/// Every `fynkel.MeshRenderer` of a document body: (entity, component,
/// material reference).
fn renderers(doc: &Value) -> Vec<(String, String, Value)> {
    let mut out = Vec::new();
    for (e, entity) in doc["entities"].as_object().unwrap() {
        for (c, component) in entity["components"].as_object().unwrap() {
            if component["type"] == "fynkel.MeshRenderer" {
                out.push((e.clone(), c.clone(), component["data"]["material"].clone()));
            }
        }
    }
    out
}

fn embedded(sub: &str) -> Value {
    json!({ "asset": ROBOT, "sub": sub })
}

fn transform(component: &str) -> (String, Value) {
    (
        component.to_string(),
        json!({
            "type": "fynkel.Transform",
            "version": 1,
            "data": { "position": [0, 0, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] }
        }),
    )
}

fn entity(name: &str, components: Vec<(String, Value)>, prefab: Option<Value>) -> Value {
    let mut e = json!({
        "name": name,
        "parent": null,
        "children": [],
        "components": components.into_iter().collect::<serde_json::Map<_, _>>(),
    });
    if let Some(p) = prefab {
        e["prefab"] = p;
    }
    e
}

#[test]
fn a_remap_points_the_models_hierarchy_at_the_project_material() {
    let p = Project::new("hierarchy");
    let before = p.cook();
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    let original = renderers(&json_chunk(&before, ROBOT, "prefab"));
    let painted: Vec<_> = original
        .iter()
        .filter(|(_, _, m)| *m == embedded("material/Painted"))
        .collect();
    assert!(!painted.is_empty(), "{original:?}");
    let (painted_entity, painted_renderer, _) = painted[0].clone();

    p.remap(json!({ "material/Painted": { "asset": RUSTED } }));
    // A scene with a plain instance, an instance overriding the remapped
    // renderer, and an entity naming the embedded material itself.
    p.write(
        "scenes/robots.scene.json",
        &json!({
            "$schema": "https://fynkel.dev/schemas/scene.schema.json",
            "formatVersion": 6,
            "id": ROBOTS,
            "name": "Robots",
            "entities": {
                "c0000000-0000-4000-8000-0000000000a1": entity(
                    "Plain",
                    vec![],
                    Some(json!({
                        "asset": { "asset": ROBOT, "sub": "prefab" },
                        "instance": "c0000000-0000-4000-8000-0000000000a3",
                        "overrides": []
                    })),
                ),
                "c0000000-0000-4000-8000-0000000000a4": entity(
                    "Overridden",
                    vec![],
                    Some(json!({
                        "asset": { "asset": ROBOT, "sub": "prefab" },
                        "instance": "c0000000-0000-4000-8000-0000000000a6",
                        "overrides": [{
                            "op": "setField",
                            "entity": painted_entity,
                            "component": painted_renderer,
                            "field": "material",
                            "value": embedded("material/Glass")
                        }]
                    })),
                ),
                "c0000000-0000-4000-8000-0000000000a7": entity(
                    "Explicit",
                    vec![
                        transform("c0000000-0000-4000-8000-0000000000a8"),
                        (
                            "c0000000-0000-4000-8000-0000000000a9".to_string(),
                            json!({
                                "type": "fynkel.MeshRenderer",
                                "version": 1,
                                "data": {
                                    "mesh": { "asset": ROBOT, "sub": "mesh/Cube" },
                                    "material": embedded("material/Painted"),
                                    "cast_shadows": true,
                                    "receive_shadows": true,
                                    "visible": true
                                }
                            }),
                        ),
                    ],
                    None,
                ),
            }
        }),
    );
    let r = p.cook();
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(r.get(id(ROBOT)).unwrap().status, Status::Cooked);

    // The hierarchy: Painted → rusted, Glass untouched, the same ids.
    let remapped = renderers(&json_chunk(&r, ROBOT, "prefab"));
    assert_eq!(remapped.len(), original.len());
    for ((e, c, before), (e2, c2, after)) in original.iter().zip(&remapped) {
        assert_eq!((e, c), (e2, c2));
        if *before == embedded("material/Painted") {
            assert_eq!(*after, json!({ "asset": RUSTED }));
        } else {
            assert_eq!(after, before);
        }
    }
    // The embedded material is still a sub-asset, cooked.
    let robot = r.get(id(ROBOT)).unwrap();
    let bytes = std::fs::read(robot.file.as_ref().unwrap()).unwrap();
    let meta: Value =
        serde_json::from_slice(Container::parse(&bytes).unwrap().meta(1 << 20).unwrap()).unwrap();
    assert!(
        meta["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["sub"] == "material/Painted"),
        "{meta}"
    );
    // The project material is loaded with the model.
    assert!(robot.dependencies.contains(&id(RUSTED)));

    // The scene: the plain instance draws rusted, the override wins, the
    // explicit reference keeps the embedded material.
    let scene = json_chunk(&r, ROBOTS, "");
    let drawn: Vec<(String, Value)> = renderers(&scene)
        .into_iter()
        .map(|(e, _, m)| {
            (
                scene["entities"][&e]["name"].as_str().unwrap().to_string(),
                m,
            )
        })
        .collect();
    let host_of = |host: &str| -> Vec<Value> {
        let children = scene["entities"][host]["children"]
            .as_array()
            .unwrap()
            .clone();
        let mut out = Vec::new();
        let mut stack: Vec<String> = children
            .iter()
            .map(|c| c.as_str().unwrap().to_string())
            .collect();
        while let Some(e) = stack.pop() {
            let entity = &scene["entities"][&e];
            for c in entity["components"].as_object().unwrap().values() {
                if c["type"] == "fynkel.MeshRenderer" {
                    out.push(c["data"]["material"].clone());
                }
            }
            stack.extend(
                entity["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c.as_str().unwrap().to_string()),
            );
        }
        out
    };
    let plain = host_of("c0000000-0000-4000-8000-0000000000a1");
    assert!(plain.contains(&json!({ "asset": RUSTED })), "{plain:?}");
    assert!(!plain.contains(&embedded("material/Painted")), "{plain:?}");
    let overridden = host_of("c0000000-0000-4000-8000-0000000000a4");
    assert_eq!(
        overridden
            .iter()
            .filter(|m| **m == json!({ "asset": RUSTED }))
            .count(),
        plain
            .iter()
            .filter(|m| **m == json!({ "asset": RUSTED }))
            .count()
            - 1,
        "the overridden renderer draws Glass: {overridden:?}"
    );
    assert!(
        drawn.contains(&("Explicit".to_string(), embedded("material/Painted"))),
        "{drawn:?}"
    );

    // Editing the project material recooks it alone: the model holds its
    // id, not its values.
    let rusted = p.assets().join("materials/rusted.material.json");
    let mut doc: Value = serde_json::from_slice(&std::fs::read(&rusted).unwrap()).unwrap();
    doc["description"] = json!("Edited.");
    p.write("materials/rusted.material.json", &doc);
    let again = p.cook();
    assert!(!again.has_errors(), "{:?}", again.diagnostics);
    assert_eq!(again.get(id(RUSTED)).unwrap().status, Status::Cooked);
    assert_eq!(again.get(id(ROBOT)).unwrap().status, Status::Cached);
    assert_eq!(again.get(id(ROBOTS)).unwrap().status, Status::Cached);

    // Removing the remap recooks the model back to its embedded material.
    p.remap(json!({}));
    let back = p.cook();
    assert_eq!(back.get(id(ROBOT)).unwrap().status, Status::Cooked);
    assert_eq!(renderers(&json_chunk(&back, ROBOT, "prefab")), original);
    assert!(back.get(id(ROBOT)).unwrap().dependencies.is_empty());
}

#[test]
fn a_remap_for_a_key_the_file_lacks_warns_and_the_model_still_cooks() {
    let p = Project::new("unknown-key");
    p.remap(json!({
        "material/Painted": { "asset": RUSTED },
        "material/Chrome": { "asset": RUSTED }
    }));
    let r = p.cook();
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(r.get(id(ROBOT)).unwrap().status, Status::Cooked);
    let w = r
        .diagnostics
        .iter()
        .find(|d| d.code == Code::IMPORT_SKIPPED && d.message.contains("material/Chrome"))
        .unwrap_or_else(|| panic!("{:#?}", r.diagnostics));
    assert!(
        w.message.contains("material/Painted, material/Glass"),
        "names the current keys: {}",
        w.message
    );
}

#[test]
fn a_remap_to_something_that_is_not_a_material_fails_the_model() {
    let p = Project::new("wrong-kind");
    p.remap(json!({ "material/Painted": { "asset": SWATCH } }));
    let r = p.cook();
    assert_eq!(r.get(id(ROBOT)).unwrap().status, Status::Failed);
    let d = r
        .diagnostics
        .iter()
        .find(|d| d.code == Code::ASSET_WRONG_KIND)
        .unwrap_or_else(|| panic!("{:#?}", r.diagnostics));
    assert!(d.message.contains("textures/swatch.png"), "{}", d.message);
    assert_eq!(
        d.pointer.as_deref(),
        Some("/settings/materials/material~1Painted")
    );

    // An embedded material of the model itself is a sub-asset, not a
    // material document.
    p.remap(json!({ "material/Painted": embedded("material/Glass") }));
    let r = p.cook();
    assert_eq!(r.get(id(ROBOT)).unwrap().status, Status::Failed);
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == Code::ASSET_WRONG_KIND));

    // A target that does not exist is the usual unresolved reference.
    p.remap(json!({ "material/Painted": { "asset": "c0000000-0000-4000-8000-0000000000ff" } }));
    let r = p.cook();
    assert_eq!(r.get(id(ROBOT)).unwrap().status, Status::Failed);
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == Code::ASSET_UNRESOLVED));

    // Not a reference at all.
    p.remap(json!({ "material/Painted": "rusted" }));
    let r = p.cook();
    assert_eq!(r.get(id(ROBOT)).unwrap().status, Status::Failed);
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == Code::ASSET_MALFORMED_ENTRY));

    // A nil reference means "use the embedded one".
    p.remap(json!({ "material/Painted": { "asset": Uuid::nil().to_string() } }));
    let r = p.cook();
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
}
