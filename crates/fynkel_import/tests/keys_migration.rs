//! The project migration to name keys (`docs/design/stable-sub-asset-ids.md`,
//! S2): a model's sidecar keeps its uuids under the new keys, references
//! follow, and override targets on the model's hierarchy — direct, through
//! a rig, and through a prefab nesting it — move to the ids the name keys
//! derive.

use std::path::{Path, PathBuf};

use fynkel_contracts::Code;
use fynkel_document::prefab::derive_id;
use fynkel_document::{Migrations, SchemaSet};
use fynkel_import::derive_sub_id;
use fynkel_import::keys_migration::{document_files, migrate_documents, Planned};
use serde_json::{json, Value};
use uuid::Uuid;

const RIG: &str = "c0000000-0000-4000-8000-000000000005";
const COLUMN: &str = "c0000000-0000-4000-8000-000000000050";
const HOLDER: &str = "c0000000-0000-4000-8000-000000000060";
const SCENE: &str = "c0000000-0000-4000-8000-000000000070";
/// The rig model's instance inside the Holder prefab.
const INNER: &str = "c0000000-0000-4000-8000-000000000061";

fn uid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

/// The rig's node 5 (`Prop`) and its renderer, as position keys derive
/// them and as name keys do.
fn prop(name_keys: bool) -> (Uuid, Uuid) {
    let node = if name_keys { "node/Rig/Prop" } else { "node/5" };
    (
        derive_sub_id(uid(RIG), node),
        derive_sub_id(uid(RIG), &format!("{node}/renderer")),
    )
}

fn set_material(entity: Uuid, component: Uuid) -> Value {
    json!({
        "op": "setField",
        "entity": entity.to_string(),
        "component": component.to_string(),
        "field": "material",
        "value": { "asset": "c0000000-0000-4000-8000-000000000020" }
    })
}

fn transform(id: &str) -> (String, Value) {
    (
        id.to_string(),
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

fn write(dir: &Path, rel: &str, v: &Value) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

/// A project at position keys: `rig.glb` with its v1 sidecar, a rig
/// document, a prefab holding the model's hierarchy with an override, and
/// a scene with a direct reference, an instance of the Holder (overriding
/// the nested entity) and an instance of the rig document.
fn project(with_model: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fynkel-keys-migration-{}", Uuid::new_v4()));
    let assets = dir.join("assets");
    std::fs::create_dir_all(assets.join("models")).unwrap();
    if with_model {
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rig.glb"),
            assets.join("models/rig.glb"),
        )
        .unwrap();
    }
    let sub = |k: &str| derive_sub_id(uid(RIG), k).to_string();
    write(
        &assets,
        "models/rig.glb.meta.json",
        &json!({
            "$schema": "https://fynkel.dev/schemas/meta.schema.json",
            "formatVersion": 1,
            "id": RIG,
            "name": "rig.glb",
            "importer": "fynkel.gltf",
            "importerVersion": 5,
            "settings": {},
            "subAssets": {
                "mesh/0": sub("mesh/0"),
                "mesh/1": sub("mesh/1"),
                "skin/0": sub("skin/0"),
                "anim/0": sub("anim/0"),
                "anim/1": sub("anim/1"),
                "prefab": sub("prefab"),
            }
        }),
    );
    write(
        &assets,
        "rigs/column.rig.json",
        &json!({
            "$schema": "https://fynkel.dev/schemas/rig.schema.json",
            "formatVersion": 1,
            "id": COLUMN,
            "name": "Column",
            "skeleton": { "asset": RIG },
            "clips": { "wave": { "asset": RIG, "sub": "anim/0" } }
        }),
    );
    let (old_prop, old_renderer) = prop(false);
    let holder_root = "c0000000-0000-4000-8000-000000000062";
    write(
        &assets,
        "prefabs/holder.prefab.json",
        &json!({
            "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
            "formatVersion": 4,
            "id": HOLDER,
            "name": "Holder",
            "roots": [holder_root],
            "entities": {
                holder_root: entity(
                    "Rig",
                    vec![transform("c0000000-0000-4000-8000-000000000063")],
                    Some(json!({
                        "asset": { "asset": RIG, "sub": "prefab" },
                        "instance": INNER,
                        "overrides": [set_material(old_prop, old_renderer)]
                    })),
                ),
            }
        }),
    );
    let outer = "c0000000-0000-4000-8000-000000000071";
    let column = "c0000000-0000-4000-8000-000000000072";
    write(
        &assets,
        "scenes/main.scene.json",
        &json!({
            "$schema": "https://fynkel.dev/schemas/scene.schema.json",
            "formatVersion": 4,
            "id": SCENE,
            "name": "Main",
            "entities": {
                "c0000000-0000-4000-8000-000000000073": entity(
                    "Body",
                    vec![
                        transform("c0000000-0000-4000-8000-000000000074"),
                        (
                            "c0000000-0000-4000-8000-000000000075".to_string(),
                            json!({
                                "type": "fynkel.MeshRenderer",
                                "version": 1,
                                "data": {
                                    "mesh": { "asset": RIG, "sub": "mesh/0" },
                                    "material": { "asset": "c0000000-0000-4000-8000-000000000020" },
                                    "cast_shadows": true,
                                    "receive_shadows": true,
                                    "visible": true
                                }
                            }),
                        ),
                    ],
                    None,
                ),
                "c0000000-0000-4000-8000-000000000076": entity(
                    "Holder",
                    vec![transform("c0000000-0000-4000-8000-000000000077")],
                    Some(json!({
                        "asset": { "asset": HOLDER },
                        "instance": outer,
                        "overrides": [set_material(
                            derive_id(uid(INNER), old_prop),
                            derive_id(uid(INNER), old_renderer),
                        )]
                    })),
                ),
                "c0000000-0000-4000-8000-000000000078": entity(
                    "Column",
                    vec![transform("c0000000-0000-4000-8000-000000000079")],
                    Some(json!({
                        "asset": { "asset": COLUMN, "sub": "prefab" },
                        "instance": column,
                        "overrides": [set_material(old_prop, old_renderer)]
                    })),
                ),
            }
        }),
    );
    dir
}

fn run(dir: &Path) -> (Vec<Planned>, Vec<fynkel_contracts::Diagnostic>) {
    let files = document_files(&dir.join("assets")).unwrap();
    migrate_documents(&files, &Migrations::builtin(), &SchemaSet::embedded())
}

fn planned<'a>(plans: &'a [Planned], rel: &str) -> &'a Planned {
    plans
        .iter()
        .find(|p| p.path.ends_with(rel))
        .unwrap_or_else(|| panic!("{rel} not planned"))
}

fn json_of(p: &Planned) -> Value {
    serde_json::from_str(&p.migrated.document.write().unwrap()).unwrap()
}

#[test]
fn keys_rename_uuids_stay_and_override_targets_follow() {
    let dir = project(true);
    let (plans, diags) = run(&dir);
    assert!(diags.is_empty(), "{diags:#?}");
    for p in &plans {
        assert!(
            !p.migrated.has_errors(),
            "{}: {:#?}",
            p.path.display(),
            p.migrated.diagnostics
        );
    }

    // The sidecar: name keys, the same uuids, version 2.
    let meta = json_of(planned(&plans, "models/rig.glb.meta.json"));
    assert_eq!(meta["formatVersion"], 2);
    let subs = meta["subAssets"].as_object().unwrap();
    let sub = |k: &str| Value::from(derive_sub_id(uid(RIG), k).to_string());
    assert_eq!(subs["mesh/Body"], sub("mesh/0"));
    assert_eq!(subs["mesh/Prop"], sub("mesh/1"));
    assert_eq!(subs["skin/Skin"], sub("skin/0"));
    assert_eq!(subs["anim/wave"], sub("anim/0"));
    assert_eq!(subs["anim/bob"], sub("anim/1"));
    assert_eq!(subs["prefab"], sub("prefab"));
    assert_eq!(subs.len(), 6);

    // References into the model take the new keys.
    let rig = json_of(planned(&plans, "rigs/column.rig.json"));
    assert_eq!(rig["clips"]["wave"]["sub"], "anim/wave");
    let scene = json_of(planned(&plans, "scenes/main.scene.json"));
    let entities = &scene["entities"];
    assert_eq!(
        entities["c0000000-0000-4000-8000-000000000073"]["components"]
            ["c0000000-0000-4000-8000-000000000075"]["data"]["mesh"]["sub"],
        "mesh/Body"
    );

    // Override targets: on the model's hierarchy inside the Holder…
    let (new_prop, new_renderer) = prop(true);
    let holder = json_of(planned(&plans, "prefabs/holder.prefab.json"));
    let o = &holder["entities"]["c0000000-0000-4000-8000-000000000062"]["prefab"]["overrides"][0];
    assert_eq!(o["entity"], new_prop.to_string());
    assert_eq!(o["component"], new_renderer.to_string());
    // …on the same entity through an instance of the Holder…
    let o = &entities["c0000000-0000-4000-8000-000000000076"]["prefab"]["overrides"][0];
    assert_eq!(o["entity"], derive_id(uid(INNER), new_prop).to_string());
    assert_eq!(
        o["component"],
        derive_id(uid(INNER), new_renderer).to_string()
    );
    // …and through the rig document, which keeps its skeleton's ids.
    let o = &entities["c0000000-0000-4000-8000-000000000078"]["prefab"]["overrides"][0];
    assert_eq!(o["entity"], new_prop.to_string());
    assert_eq!(o["component"], new_renderer.to_string());

    // A second run finds nothing to do.
    for p in &plans {
        if p.migrated.changed() {
            p.migrated.write_with_backup(&p.path).unwrap();
        }
    }
    let (again, diags) = run(&dir);
    assert!(diags.is_empty(), "{diags:#?}");
    assert!(
        again.iter().all(|p| !p.migrated.changed()),
        "{:?}",
        again
            .iter()
            .filter(|p| p.migrated.changed())
            .map(|p| &p.path)
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_missing_model_is_reported_and_its_references_are_kept() {
    let dir = project(false);
    let (plans, diags) = run(&dir);
    assert!(
        diags
            .iter()
            .any(|d| d.code == Code::MIGRATION_FAILED && d.message.contains("missing")),
        "{diags:#?}"
    );
    // The sidecar's own step refuses; nothing points at new keys.
    assert!(planned(&plans, "models/rig.glb.meta.json")
        .migrated
        .has_errors());
    let scene = json_of(planned(&plans, "scenes/main.scene.json"));
    assert_eq!(
        scene["entities"]["c0000000-0000-4000-8000-000000000073"]["components"]
            ["c0000000-0000-4000-8000-000000000075"]["data"]["mesh"]["sub"],
        "mesh/0"
    );
    let rig = json_of(planned(&plans, "rigs/column.rig.json"));
    assert_eq!(rig["clips"]["wave"]["sub"], "anim/0");
}
