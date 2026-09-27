//! The project migration to one root per prefab (ADR-0022) over a small
//! v4 project: a prefab with two roots gets a wrapper, an anchor's
//! Transform composed with its prefab's root becomes overrides of the
//! root, its other components `addComponent` overrides, its children and
//! top-level added objects keep their place, a nested anchor inside a
//! prefab follows, and an anchor that cannot be expressed is refused with
//! its document left alone.

use std::path::{Path, PathBuf};

use fynkel_document::one_root::{wrapper_root, wrapper_transform};
use fynkel_document::{Migrations, SchemaSet};
use fynkel_import::keys_migration::{document_files, migrate_documents, Planned};
use serde_json::{json, Value};
use uuid::Uuid;

const PAIR: &str = "d0000000-0000-4000-8000-000000000001";
const LAMP: &str = "d0000000-0000-4000-8000-000000000002";
const STAND: &str = "d0000000-0000-4000-8000-000000000003";
const TILTED: &str = "d0000000-0000-4000-8000-000000000004";

const PAIR_A: &str = "d0000000-0000-4000-8000-0000000000a1";
const PAIR_B: &str = "d0000000-0000-4000-8000-0000000000a2";
const LAMP_ROOT: &str = "d0000000-0000-4000-8000-0000000000b1";
const LAMP_ROOT_TRANSFORM: &str = "d0000000-0000-4000-8000-0000000000b2";
const STAND_ANCHOR: &str = "d0000000-0000-4000-8000-0000000000c1";
const TILTED_ROOT: &str = "d0000000-0000-4000-8000-0000000000d1";

const LAMP_1: &str = "d0000000-0000-4000-8000-0000000000e1";
const LAMP_1_INSTANCE: &str = "d0000000-0000-4000-8000-0000000000e2";
const LAMP_1_COLLIDER: &str = "d0000000-0000-4000-8000-0000000000e3";
const TAG: &str = "d0000000-0000-4000-8000-0000000000e4";
const TAG_TRANSFORM: &str = "d0000000-0000-4000-8000-0000000000e5";
const EXTRA: &str = "d0000000-0000-4000-8000-0000000000e6";
const EXTRA_TRANSFORM: &str = "d0000000-0000-4000-8000-0000000000e7";
const PAIR_1: &str = "d0000000-0000-4000-8000-0000000000f1";
const STAND_1: &str = "d0000000-0000-4000-8000-0000000000f3";

fn uid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn transform(p: [f64; 3], q: [f64; 4], s: [f64; 3]) -> Value {
    json!({ "type": "fynkel.Transform", "version": 1,
            "data": { "position": p, "rotation": q, "scale": s } })
}

fn at(p: [f64; 3]) -> Value {
    transform(p, [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0])
}

fn entity(name: &str, parent: Option<&str>, children: &[&str], components: Value) -> Value {
    json!({ "name": name, "parent": parent, "children": children, "components": components })
}

fn collider() -> Value {
    json!({ "type": "fynkel.Collider", "version": 1,
            "data": { "shape": "box", "size": [1, 1, 1], "mesh": null, "offset": [0, 0, 0], "friction": 0.5,
                      "restitution": 0.0, "is_sensor": false, "mask": 4294967295u32 } })
}

fn prefab(id: &str, name: &str, roots: &[&str], entities: Value) -> Value {
    json!({
        "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
        "formatVersion": 4,
        "id": id,
        "name": name,
        "entities": entities,
        "roots": roots,
    })
}

fn scene(id: &str, entities: Value) -> Value {
    json!({
        "$schema": "https://fynkel.dev/schemas/scene.schema.json",
        "formatVersion": 4,
        "id": id,
        "name": "Main",
        "entities": entities,
    })
}

fn write(dir: &Path, rel: &str, v: &Value) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

/// Pair (two roots), Lamp (one root at y 1 with a Bulb), Stand (a Lamp
/// anchor as its root, moved), Tilted (a root turned a quarter about y);
/// the main scene places a Lamp (with a Collider, a child and an added
/// object), a Pair and a Stand; the bad scene squashes a Tilted.
fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fynkel-one-root-{}", Uuid::new_v4()));
    let assets = dir.join("assets");
    write(
        &assets,
        "prefabs/pair.prefab.json",
        &prefab(
            PAIR,
            "Pair",
            &[PAIR_A, PAIR_B],
            json!({
                PAIR_A: entity("A", None, &[], json!({ "d0000000-0000-4000-8000-0000000000a3": at([1.0, 0.0, 0.0]) })),
                PAIR_B: entity("B", None, &[], json!({ "d0000000-0000-4000-8000-0000000000a4": at([-1.0, 0.0, 0.0]) })),
            }),
        ),
    );
    write(
        &assets,
        "prefabs/lamp.prefab.json",
        &prefab(
            LAMP,
            "Lamp",
            &[LAMP_ROOT],
            json!({
                LAMP_ROOT: entity("Lamp", None, &["d0000000-0000-4000-8000-0000000000b3"],
                    json!({ LAMP_ROOT_TRANSFORM: at([0.0, 1.0, 0.0]) })),
                "d0000000-0000-4000-8000-0000000000b3": entity("Bulb", Some(LAMP_ROOT), &[],
                    json!({ "d0000000-0000-4000-8000-0000000000b4": at([0.0, 0.5, 0.0]) })),
            }),
        ),
    );
    write(
        &assets,
        "prefabs/stand.prefab.json",
        &prefab(
            STAND,
            "Stand",
            &[STAND_ANCHOR],
            json!({
                STAND_ANCHOR: {
                    "name": "Holder", "parent": null, "children": [],
                    "components": { "d0000000-0000-4000-8000-0000000000c2": at([0.0, 0.0, 2.0]) },
                    "prefab": { "asset": { "asset": LAMP }, "instance": "d0000000-0000-4000-8000-0000000000c3" }
                },
            }),
        ),
    );
    let half = std::f64::consts::FRAC_1_SQRT_2;
    write(
        &assets,
        "prefabs/tilted.prefab.json",
        &prefab(
            TILTED,
            "Tilted",
            &[TILTED_ROOT],
            json!({
                TILTED_ROOT: entity("Tilted", None, &[], json!({
                    "d0000000-0000-4000-8000-0000000000d2": transform([0.0; 3], [0.0, half, 0.0, half], [1.0; 3]) })),
            }),
        ),
    );
    write(
        &assets,
        "scenes/main.scene.json",
        &scene(
            "d0000000-0000-4000-8000-000000000010",
            json!({
                LAMP_1: {
                    "name": "Lamp 1", "parent": null, "children": [TAG],
                    "components": {
                        "d0000000-0000-4000-8000-0000000000e8": at([5.0, 0.0, 0.0]),
                        LAMP_1_COLLIDER: collider(),
                    },
                    "prefab": {
                        "asset": { "asset": LAMP }, "instance": LAMP_1_INSTANCE,
                        "overrides": [
                            { "op": "addObject", "entity": EXTRA, "name": "Extra", "parent": null,
                              "components": { EXTRA_TRANSFORM: at([0.0, 3.0, 0.0]) } }
                        ]
                    }
                },
                TAG: entity("Tag", Some(LAMP_1), &[], json!({ TAG_TRANSFORM: at([0.0, 2.0, 0.0]) })),
                PAIR_1: {
                    "name": "Pair 1", "parent": null, "children": [],
                    "components": { "d0000000-0000-4000-8000-0000000000f2": at([0.0, 0.0, 3.0]) },
                    "prefab": { "asset": { "asset": PAIR }, "instance": "d0000000-0000-4000-8000-0000000000f5" }
                },
                STAND_1: {
                    "name": "Stand 1", "parent": null, "children": [],
                    "components": { "d0000000-0000-4000-8000-0000000000f4": at([0.0, 0.0, 0.0]) },
                    "prefab": { "asset": { "asset": STAND }, "instance": "d0000000-0000-4000-8000-0000000000f6" }
                },
            }),
        ),
    );
    write(
        &assets,
        "scenes/bad.scene.json",
        &scene(
            "d0000000-0000-4000-8000-000000000011",
            json!({
                "d0000000-0000-4000-8000-000000000012": {
                    "name": "Squashed", "parent": null, "children": [],
                    "components": { "d0000000-0000-4000-8000-000000000013":
                        transform([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 1.0]) },
                    "prefab": { "asset": { "asset": TILTED }, "instance": "d0000000-0000-4000-8000-000000000014" }
                },
            }),
        ),
    );
    dir
}

fn run(dir: &Path) -> Vec<Planned> {
    let files = document_files(&dir.join("assets")).unwrap();
    let (plans, diags) = migrate_documents(&files, &Migrations::builtin(), &SchemaSet::embedded());
    assert!(diags.is_empty(), "{diags:#?}");
    plans
}

fn planned<'a>(plans: &'a [Planned], rel: &str) -> &'a Planned {
    plans
        .iter()
        .find(|p| p.path.ends_with(rel))
        .unwrap_or_else(|| panic!("{rel} not planned"))
}

fn json_of(p: &Planned) -> Value {
    assert!(
        !p.migrated.has_errors(),
        "{}: {:#?}",
        p.path.display(),
        p.migrated.diagnostics
    );
    serde_json::from_str(&p.migrated.document.write().unwrap()).unwrap()
}

fn close(v: &Value, want: &[f64]) -> bool {
    let got: Vec<f64> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap())
        .collect();
    got.len() == want.len() && got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6)
}

#[test]
fn every_prefab_gets_one_root_and_every_instance_becomes_it() {
    let dir = project();
    let plans = run(&dir);

    // Two roots: a wrapper named after the prefab over them.
    let pair = json_of(planned(&plans, "prefabs/pair.prefab.json"));
    let wrapper = wrapper_root(uid(PAIR)).to_string();
    assert_eq!(pair["formatVersion"], 6);
    assert_eq!(pair["root"], wrapper.as_str());
    assert!(pair.get("roots").is_none());
    let w = &pair["entities"][&wrapper];
    assert_eq!(w["name"], "Pair");
    assert_eq!(w["parent"], Value::Null);
    assert_eq!(w["children"], json!([PAIR_A, PAIR_B]));
    let wt = &w["components"][wrapper_transform(uid(PAIR)).to_string()];
    assert_eq!(wt["type"], "fynkel.Transform");
    assert!(close(&wt["data"]["position"], &[0.0, 0.0, 0.0]));
    assert_eq!(pair["entities"][PAIR_A]["parent"], wrapper.as_str());
    // One root already: only the version and the key move.
    let lamp = json_of(planned(&plans, "prefabs/lamp.prefab.json"));
    assert_eq!(lamp["root"], LAMP_ROOT);
    assert_eq!(lamp["entities"].as_object().unwrap().len(), 2);

    let main = json_of(planned(&plans, "scenes/main.scene.json"));
    assert_eq!(main["formatVersion"], 6);
    let e = &main["entities"];

    // The Lamp: no components of its own. Its Transform (x 5) composed
    // with the root's (y 1) is one position override; the Collider is
    // added to the root.
    let lamp_1 = &e[LAMP_1];
    assert_eq!(lamp_1["components"], json!({}));
    assert_eq!(lamp_1["name"], "Lamp 1");
    let overrides = lamp_1["prefab"]["overrides"].as_array().unwrap();
    let position = overrides
        .iter()
        .find(|o| o["op"] == "setField" && o["field"] == "position")
        .expect("a position override");
    assert_eq!(position["entity"], LAMP_ROOT);
    assert_eq!(position["component"], LAMP_ROOT_TRANSFORM);
    assert!(close(&position["value"], &[5.0, 1.0, 0.0]));
    assert!(
        !overrides
            .iter()
            .any(|o| o["field"] == "rotation" || o["field"] == "scale"),
        "only what differs: {overrides:#?}"
    );
    let added = overrides
        .iter()
        .find(|o| o["op"] == "addComponent")
        .expect("the Collider moves to the root");
    assert_eq!(added["entity"], LAMP_ROOT);
    assert_eq!(added["component"], LAMP_1_COLLIDER);
    assert_eq!(added["type"], "fynkel.Collider");
    // What was placed from the anchor is now placed from the root (y 1):
    // the child and the top-level added object keep their world place.
    assert_eq!(lamp_1["children"], json!([TAG]));
    assert!(close(
        &e[TAG]["components"][TAG_TRANSFORM]["data"]["position"],
        &[0.0, 1.0, 0.0]
    ));
    let extra = overrides
        .iter()
        .find(|o| o["op"] == "addObject")
        .expect("the added object stays");
    assert_eq!(extra["parent"], LAMP_ROOT);
    assert!(close(
        &extra["components"][EXTRA_TRANSFORM]["data"]["position"],
        &[0.0, 2.0, 0.0]
    ));

    // The Pair: its root is the wrapper, at the identity; the anchor's
    // place is the wrapper's.
    let pair_1 = &e[PAIR_1];
    assert_eq!(pair_1["components"], json!({}));
    let o = &pair_1["prefab"]["overrides"];
    assert_eq!(o.as_array().unwrap().len(), 1, "{o:#}");
    assert_eq!(o[0]["entity"], wrapper.as_str());
    assert_eq!(o[0]["component"], wrapper_transform(uid(PAIR)).to_string());
    assert!(close(&o[0]["value"], &[0.0, 0.0, 3.0]));

    // A nested anchor as a prefab's root: the Stand's Holder is a Lamp
    // moved to z 2, so its root stands at (0, 1, 2).
    let stand = json_of(planned(&plans, "prefabs/stand.prefab.json"));
    assert_eq!(stand["root"], STAND_ANCHOR);
    let holder = &stand["entities"][STAND_ANCHOR];
    assert_eq!(holder["components"], json!({}));
    let o = &holder["prefab"]["overrides"];
    assert_eq!(o[0]["entity"], LAMP_ROOT);
    assert!(close(&o[0]["value"], &[0.0, 1.0, 2.0]));
    // …and the Stand's instance, at the origin, needs no override.
    assert_eq!(e[STAND_1]["components"], json!({}));
    assert!(e[STAND_1]["prefab"].get("overrides").is_none());

    // A non-uniform scale over a turned root is not a position/rotation/
    // scale: refused, the document as it was.
    let bad = planned(&plans, "scenes/bad.scene.json");
    assert!(bad.migrated.has_errors());
    assert!(!bad.migrated.changed());
    let why = format!("{:?}", bad.migrated.diagnostics);
    assert!(
        why.contains("Squashed") && why.contains("non-uniform"),
        "{why}"
    );

    // Written, a second run has nothing to do.
    for p in &plans {
        if p.migrated.changed() && !p.migrated.has_errors() {
            p.migrated.write_with_backup(&p.path).unwrap();
        }
    }
    std::fs::remove_file(dir.join("assets/scenes/bad.scene.json")).unwrap();
    let again = run(&dir);
    assert!(
        again.iter().all(|p| !p.migrated.changed()),
        "{:?}",
        again
            .iter()
            .filter(|p| p.migrated.changed())
            .map(|p| &p.path)
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
