//! Prefab mode (M4-07) over a copy of the fixture project: the `instance`
//! query (the expansion of one anchor with its overrides marked, live
//! against an open prefab document), the two-step "apply to prefab",
//! `unpackPrefab` through the session with its library, imported
//! hierarchies opened read-only (`<id>#prefab`), and the broken cases —
//! a missing prefab, an override that does not apply — shown and left
//! alone.

use std::path::{Path, PathBuf};

use fynkel_authoring::AuthoringSession;
use fynkel_document::{expand, Document, DocumentKind, PrefabLibrary, Scene};
use serde_json::{json, Value};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
}

struct Copy {
    dir: PathBuf,
}

impl Copy {
    fn new(name: &str) -> Copy {
        let dir =
            std::env::temp_dir().join(format!("fynkel-prefab-mode-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture(), &dir);
        Copy {
            dir: dir.canonicalize().unwrap(),
        }
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.join(rel)).unwrap()
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

fn open(copy: &Copy) -> AuthoringSession {
    let mut s = AuthoringSession::new();
    s.open_project(&copy.dir)
        .map_err(|d| format!("{d:?}"))
        .unwrap();
    s
}

const MAIN_SCENE: &str = "assets/scenes/main.scene.json";
const RIG_SCENE: &str = "assets/scenes/rig.scene.json";
const CRATE_PREFAB: &str = "c0000000-0000-4000-8000-000000000040";
const CRATE_A: &str = "c0000000-0000-4000-8000-0000000000e2";
const CRATE_A_INSTANCE: &str = "c0000000-0000-4000-8000-0000000000f1";
const CRATE_LOCAL: &str = "c0000000-0000-4000-8000-0000000000b1";
const CRATE_LOCAL_RENDERER: &str = "c0000000-0000-4000-8000-0000000000c3";
const CRATE_LOCAL_TRANSFORM: &str = "c0000000-0000-4000-8000-0000000000c2";
const CAMERA: &str = "c0000000-0000-4000-8000-0000000000e0";
const COIN_PREFAB: &str = "c0000000-0000-4000-8000-000000000042";
const COIN_LOCAL: &str = "c0000000-0000-4000-8000-0000000000b3";
const COIN_LOCAL_TRIGGER: &str = "c0000000-0000-4000-8000-0000000000cf";
const IMPORTED: &str = "c0000000-0000-4000-8000-0000000004e8";
const COLUMN_RIG: &str = "c0000000-0000-4000-8000-000000000050";
const RIG_GLB: &str = "c0000000-0000-4000-8000-000000000005";
const TEXTURE: &str = "c0000000-0000-4000-8000-000000000003";
const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";

fn instance(s: &AuthoringSession, h: u32, entity: &str) -> Value {
    s.query("instance", &json!({ "handle": h, "entity": entity }))
        .unwrap_or_else(|d| panic!("{d:#?}"))
}

fn apply(s: &mut AuthoringSession, h: u32, commands: Value) -> Value {
    let r = s
        .apply_json(h, &commands)
        .unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(r["ok"], true, "{r:#}");
    r
}

fn names(v: &Value) -> Vec<&str> {
    v["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect()
}

fn derived(instance: &str, local: &str) -> String {
    fynkel_document::prefab::derive_id(instance.parse().unwrap(), local.parse().unwrap())
        .to_string()
}

#[test]
fn the_instance_query_expands_one_anchor_with_its_overrides_marked() {
    let copy = Copy::new("instance");
    let mut s = open(&copy);
    let h = s.open_document(MAIN_SCENE).unwrap();

    let v = instance(&s, h, CRATE_A);
    assert_eq!(v["anchor"], CRATE_A);
    assert_eq!(v["instance"], CRATE_A_INSTANCE);
    assert_eq!(v["asset"], json!({ "asset": CRATE_PREFAB, "sub": "" }));
    assert_eq!(v["expanded"], true);
    assert_eq!(v["diagnostics"], json!([]));
    assert_eq!(v["prefab"]["id"], CRATE_PREFAB);
    assert_eq!(v["prefab"]["name"], "Crate");
    assert_eq!(v["prefab"]["path"], "prefabs/crate.prefab.json");
    assert_eq!(v["prefab"]["kind"], "prefab");
    assert_eq!(v["prefab"]["open"], CRATE_PREFAB);
    assert_eq!(v["prefab"]["handle"], Value::Null, "not open yet");
    assert_eq!(v["prefab"]["readOnly"], Value::Null);

    // One entity: the prefab's root is the instance itself (its id and
    // name, ADR-0022), its components under the ids the runtime derives.
    assert_eq!(names(&v), ["Crate A"]);
    let e = &v["entities"][0];
    let crate_id = CRATE_A.to_string();
    let renderer_id = derived(CRATE_A_INSTANCE, CRATE_LOCAL_RENDERER);
    assert_eq!(e["id"], crate_id);
    assert_eq!(e["local"], CRATE_LOCAL);
    assert_eq!(e["parent"], Value::Null);
    assert_eq!(e["children"], json!([]));
    assert_eq!(e["added"], false);
    assert_eq!(e["active"], true);
    assert_eq!(e["activeOverridden"], false);
    let renderer = &e["components"][&renderer_id];
    assert_eq!(renderer["type"], "fynkel.MeshRenderer");
    assert_eq!(renderer["local"], CRATE_LOCAL_RENDERER);
    assert_eq!(
        renderer["data"]["cast_shadows"], false,
        "the override applied"
    );
    assert_eq!(renderer["overridden"], json!(["cast_shadows"]));
    assert_eq!(renderer["added"], false);
    let transform = &e["components"][derived(CRATE_A_INSTANCE, CRATE_LOCAL_TRANSFORM)];
    assert_eq!(
        transform["overridden"],
        json!(["position"]),
        "where it stands"
    );
    assert_eq!(transform["data"]["position"], json!([2.0, 0.5, 0.0]));

    // The override, annotated with where it landed.
    let o = &v["overrides"][0];
    assert_eq!(o["index"], 0);
    assert_eq!(o["op"], "setField");
    assert_eq!(o["entity"], CRATE_LOCAL);
    assert_eq!(o["entityId"], crate_id);
    assert_eq!(o["entityName"], "Crate A");
    assert_eq!(o["component"], CRATE_LOCAL_RENDERER);
    assert_eq!(o["componentId"], renderer_id);
    assert_eq!(o["componentType"], "fynkel.MeshRenderer");
    assert_eq!(o["field"], "cast_shadows");
    assert_eq!(o["value"], false);
    assert_eq!(
        o["pointer"],
        format!("/entities/{CRATE_A}/prefab/overrides/0")
    );

    // Live: the prefab opened and changed in its own editor shows through
    // the instance before anything is saved; its handle is reported. The
    // instance's own name wins over the root's.
    let p = s.open_document(CRATE_PREFAB).unwrap();
    assert_eq!(instance(&s, h, CRATE_A)["prefab"]["handle"], p);
    apply(
        &mut s,
        p,
        json!([
            { "command": "renameObject", "id": CRATE_LOCAL, "name": "Box" },
            { "command": "setField", "entity": CRATE_LOCAL, "component": CRATE_LOCAL_TRANSFORM, "field": "scale", "value": [2, 2, 2] }
        ]),
    );
    let v = instance(&s, h, CRATE_A);
    assert_eq!(names(&v), ["Crate A"]);
    let transform =
        &v["entities"][0]["components"][derived(CRATE_A_INSTANCE, CRATE_LOCAL_TRANSFORM)];
    assert_eq!(transform["data"]["scale"], json!([2, 2, 2]));
    s.undo(p).unwrap();
    let v = instance(&s, h, CRATE_A);
    let transform =
        &v["entities"][0]["components"][derived(CRATE_A_INSTANCE, CRATE_LOCAL_TRANSFORM)];
    assert_eq!(transform["data"]["scale"], json!([1, 1, 1]));

    // Every override kind marks its target.
    let sensor = derived(CRATE_A_INSTANCE, "c0000000-0000-4000-8000-0000000000aa");
    apply(
        &mut s,
        h,
        json!([
            { "command": "setPrefabOverride", "entity": CRATE_A, "override": {
                "op": "setField", "entity": CRATE_LOCAL, "component": CRATE_LOCAL_RENDERER, "field": "cast_shadows", "value": true } },
            { "command": "setPrefabOverride", "entity": CRATE_A, "override": {
                "op": "setField", "entity": CRATE_LOCAL, "component": CRATE_LOCAL_TRANSFORM, "field": "position", "value": [0, 2, 0] } },
            { "command": "setPrefabOverride", "entity": CRATE_A, "override": {
                "op": "addComponent", "entity": CRATE_LOCAL, "component": "c0000000-0000-4000-8000-0000000000aa",
                "type": "fynkel.Collider", "version": 1,
                "data": { "shape": "box", "size": [1, 1, 1], "mesh": null, "offset": [0, 0, 0], "friction": 0.5,
                          "restitution": 0.0, "is_sensor": false, "mask": 4294967295u32 } } },
            { "command": "setPrefabOverride", "entity": CRATE_A, "override": {
                "op": "addObject", "entity": "c0000000-0000-4000-8000-0000000000ab", "name": "Lid",
                "parent": CRATE_LOCAL, "components": {} } },
            { "command": "setPrefabOverride", "entity": CRATE_A, "override": {
                "op": "setActive", "entity": "c0000000-0000-4000-8000-0000000000ab", "value": false } }
        ]),
    );
    let v = instance(&s, h, CRATE_A);
    assert_eq!(v["expanded"], true, "{:#}", v["diagnostics"]);
    assert_eq!(names(&v), ["Crate A", "Lid"]);
    let e = &v["entities"][0];
    // The second setField replaced the first on the same field (an
    // override's `field` is a whole key of `data`, never a pointer into it).
    assert_eq!(
        e["components"][&renderer_id]["overridden"],
        json!(["cast_shadows"])
    );
    assert_eq!(e["components"][&renderer_id]["data"]["cast_shadows"], true);
    let transform = &e["components"][derived(CRATE_A_INSTANCE, CRATE_LOCAL_TRANSFORM)];
    assert_eq!(transform["overridden"], json!(["position"]));
    assert_eq!(transform["data"]["position"][1], 2);
    assert_eq!(e["components"][&sensor]["added"], true);
    assert_eq!(e["components"][&sensor]["type"], "fynkel.Collider");
    assert_eq!(
        e["children"],
        json!([derived(
            CRATE_A_INSTANCE,
            "c0000000-0000-4000-8000-0000000000ab"
        )])
    );
    let lid = &v["entities"][1];
    assert_eq!(lid["added"], true);
    assert_eq!(lid["active"], false);
    assert_eq!(lid["activeOverridden"], true);
    assert_eq!(e["activeOverridden"], false);
    assert_eq!(v["overrides"][4]["op"], "setActive");
    assert_eq!(v["overrides"][4]["entityName"], "Lid");
    assert_eq!(lid["parent"], crate_id);
    assert_eq!(v["overrides"].as_array().unwrap().len(), 5);
    assert_eq!(v["overrides"][2]["componentId"], sensor);
    assert_eq!(v["overrides"][2]["componentType"], "fynkel.Collider");
    assert_eq!(v["overrides"][3]["entityName"], "Lid");

    // Not an instance, unknown, a material.
    let err = s
        .query("instance", &json!({ "handle": h, "entity": CAMERA }))
        .unwrap_err();
    assert_eq!(err[0].code.as_str(), "FYN-E0008");
    let err = s
        .query(
            "instance",
            &json!({ "handle": h, "entity": "c0000000-0000-4000-8000-0000000000ff" }),
        )
        .unwrap_err();
    assert_eq!(err[0].code.as_str(), "FYN-E0002");
    let m = s
        .open_document("assets/materials/rusted.material.json")
        .unwrap();
    let err = s
        .query("instance", &json!({ "handle": m, "entity": CAMERA }))
        .unwrap_err();
    assert_eq!(err[0].code.as_str(), "FYN-E0008");
    let err = s.query("instance", &json!({ "handle": h })).unwrap_err();
    assert_eq!(err[0].code.as_str(), "FYN-T0003");
}

#[test]
fn apply_to_prefab_is_two_transactions_and_unpack_is_one() {
    let copy = Copy::new("apply");
    let mut s = open(&copy);
    let h = s.open_document(MAIN_SCENE).unwrap();
    let scene_text = copy.read(MAIN_SCENE);

    // A Coin instance with an override on its TriggerZone (the gate's coin).
    let coin_instance = "c0000000-0000-4000-8000-0000000000f9";
    let r = apply(
        &mut s,
        h,
        json!([{ "command": "instantiatePrefab", "name": "Coin", "prefab": { "asset": COIN_PREFAB },
                 "instance": coin_instance, "components": [] }]),
    );
    let coin = r["mutations"][0]["affectedIds"][0]
        .as_str()
        .unwrap()
        .to_string();
    apply(
        &mut s,
        h,
        json!([{ "command": "setPrefabOverride", "entity": coin, "override": {
            "op": "setField", "entity": COIN_LOCAL, "component": COIN_LOCAL_TRIGGER, "field": "event", "value": "pickup" } }]),
    );
    let trigger = derived(coin_instance, COIN_LOCAL_TRIGGER);
    let v = instance(&s, h, &coin);
    assert_eq!(
        v["entities"][0]["components"][&trigger]["data"]["event"],
        "pickup"
    );
    assert_eq!(
        v["entities"][0]["components"][&trigger]["overridden"],
        json!(["event"])
    );

    // Apply to prefab: the prefab document takes the value (one
    // transaction there), the instance drops the override (one here).
    let p = s.open_document(COIN_PREFAB).unwrap();
    let prefab_text = copy.read("assets/prefabs/coin.prefab.json");
    apply(
        &mut s,
        p,
        json!([{ "command": "setField", "entity": COIN_LOCAL, "component": COIN_LOCAL_TRIGGER, "field": "event", "value": "pickup" }]),
    );
    apply(
        &mut s,
        h,
        json!([{ "command": "revertPrefabOverride", "entity": coin, "index": 0 }]),
    );
    let v = instance(&s, h, &coin);
    assert_eq!(v["overrides"], json!([]));
    assert_eq!(
        v["entities"][0]["components"][&trigger]["data"]["event"], "pickup",
        "inherited now"
    );
    assert_eq!(
        v["entities"][0]["components"][&trigger]["overridden"],
        json!([])
    );
    assert_eq!(s.state_json(p).unwrap()["dirty"], true);
    // Two undo steps: the scene's override is back, then the prefab's value.
    s.undo(h).unwrap();
    assert_eq!(
        instance(&s, h, &coin)["overrides"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    s.undo(p).unwrap();
    assert_eq!(s.state_json(p).unwrap()["dirty"], false);
    assert_eq!(
        copy.read("assets/prefabs/coin.prefab.json"),
        prefab_text,
        "never written"
    );
    assert_eq!(
        instance(&s, h, &coin)["entities"][0]["components"][&trigger]["data"]["event"],
        "pickup",
        "the override still applies"
    );

    // Unpack: one transaction, the document then holds what the
    // expansion held, under the same ids; nothing is an instance any more.
    let expected = {
        let document =
            Document::read(&copy.dir.join(MAIN_SCENE), Some(DocumentKind::Scene)).unwrap();
        // The live document: the coin is unsaved, so build it from the editor.
        let _ = document;
        let root = s.document_json(h).unwrap()["root"].clone();
        let mut document =
            Document::read(&copy.dir.join(MAIN_SCENE), Some(DocumentKind::Scene)).unwrap();
        document.root = fynkel_document::Node::from_serde(&root);
        let scene = Scene::from_document(document);
        let (lib, diags) = PrefabLibrary::scan(&copy.dir.join("assets/prefabs"));
        assert!(diags.is_empty(), "{diags:#?}");
        let x = expand(&scene, &lib, &fynkel_document::SchemaSet::embedded());
        assert!(!x.has_errors(), "{:#?}", x.diagnostics);
        x.document.write().unwrap()
    };
    let r = apply(
        &mut s,
        h,
        json!([
            { "command": "unpackPrefab", "entity": CRATE_A },
            { "command": "unpackPrefab", "entity": coin }
        ]),
    );
    assert_eq!(r["mutations"].as_array().unwrap().len(), 2);
    // The coin is its prefab's root: it stays itself, with its components.
    assert_eq!(r["mutations"][1]["affectedIds"][0], coin);
    assert!(r["mutations"][1]["affectedIds"]
        .as_array()
        .unwrap()
        .contains(&json!(trigger)));
    let live = s.document(h).unwrap().editor.document().write().unwrap();
    assert_eq!(live, expected);
    let err = s
        .query("instance", &json!({ "handle": h, "entity": CRATE_A }))
        .unwrap_err();
    assert_eq!(err[0].code.as_str(), "FYN-E0008", "a plain entity now");
    let part = s
        .query("document", &json!({ "handle": h, "ids": [coin] }))
        .unwrap();
    assert_eq!(part["entities"][&coin]["name"], "Coin");
    assert_eq!(
        part["entities"][&coin]["components"][&trigger]["data"]["event"],
        "pickup"
    );
    assert_eq!(s.inspect_json(h).unwrap()["instanceCount"], 0);
    // One undo step brings both instances back.
    s.undo(h).unwrap();
    assert_eq!(s.inspect_json(h).unwrap()["instanceCount"], 2);
    assert_eq!(instance(&s, h, CRATE_A)["expanded"], true);
    // Nothing was ever written.
    assert_eq!(copy.read(MAIN_SCENE), scene_text);
}

#[test]
fn broken_and_missing_prefabs_are_shown_and_left_alone() {
    let copy = Copy::new("broken");
    let mut s = open(&copy);
    let h = s.open_document(MAIN_SCENE).unwrap();
    let text = copy.read(MAIN_SCENE);

    // An override on an entity the prefab does not have: the instance
    // keeps its data, the query says why it does not expand.
    apply(
        &mut s,
        h,
        json!([{ "command": "setPrefabOverride", "entity": CRATE_A, "override": {
            "op": "setField", "entity": "c0000000-0000-4000-8000-0000000000bb", "component": CRATE_LOCAL_RENDERER,
            "field": "cast_shadows", "value": true } }]),
    );
    let v = instance(&s, h, CRATE_A);
    assert_eq!(v["expanded"], false);
    assert_eq!(v["entities"], json!([]));
    assert_eq!(v["prefab"]["name"], "Crate", "the prefab itself is known");
    assert_eq!(v["overrides"].as_array().unwrap().len(), 3);
    assert_eq!(v["overrides"][2]["entityId"], Value::Null);
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0403");
    assert_eq!(
        v["diagnostics"][0]["pointer"],
        format!("/entities/{CRATE_A}/prefab/overrides/2")
    );
    // Unpacking it is refused with the same diagnostic; nothing changes.
    let before = s.state_json(h).unwrap()["revision"].clone();
    let r = s
        .apply_json(
            h,
            &json!([{ "command": "unpackPrefab", "entity": CRATE_A }]),
        )
        .unwrap();
    assert_eq!(r["ok"], false);
    let d = &r["mutations"][0]["diagnostics"][0];
    assert_eq!(d["code"], "FYN-E0004");
    assert_eq!(d["related"][0]["code"], "FYN-D0403");
    assert_eq!(s.state_json(h).unwrap()["revision"], before);
    s.undo(h).unwrap();

    // A prefab that does not exist: `prefab` is null, D0401 explains.
    let ghost = "c0000000-0000-4000-8000-0000000000dd";
    let r = apply(
        &mut s,
        h,
        json!([{ "command": "instantiatePrefab", "name": "Ghost", "prefab": { "asset": ghost }, "components": [] }]),
    );
    let anchor = r["mutations"][0]["affectedIds"][0]
        .as_str()
        .unwrap()
        .to_string();
    let v = instance(&s, h, &anchor);
    assert_eq!(v["expanded"], false);
    assert_eq!(v["prefab"], Value::Null);
    assert_eq!(v["entities"], json!([]));
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0401");
    assert_eq!(v["asset"]["asset"], ghost);
    // The anchor's own data is untouched and still edits.
    apply(
        &mut s,
        h,
        json!([{ "command": "renameObject", "id": anchor, "name": "Ghost (missing)" }]),
    );
    assert_eq!(instance(&s, h, &anchor)["expanded"], false);
    // Save it: the reference is written as it is, never "fixed".
    s.save(h, None).unwrap();
    let saved: Value = serde_json::from_str(&copy.read(MAIN_SCENE)).unwrap();
    assert_eq!(
        saved["entities"][&anchor]["prefab"]["asset"]["asset"],
        ghost
    );
    assert_eq!(saved["entities"][&anchor]["name"], "Ghost (missing)");
    assert_ne!(copy.read(MAIN_SCENE), text);
}

#[test]
fn imported_hierarchies_open_read_only_and_instances_of_them_expand() {
    let copy = Copy::new("imported");
    let mut s = open(&copy);
    let h = s.open_document(RIG_SCENE).unwrap();
    let rig_text = copy.read(RIG_SCENE);

    // The Imported instance of `column.rig.json#prefab`.
    let v = instance(&s, h, IMPORTED);
    assert_eq!(v["expanded"], true, "{:#}", v["diagnostics"]);
    assert_eq!(v["asset"], json!({ "asset": COLUMN_RIG, "sub": "prefab" }));
    assert_eq!(v["prefab"]["kind"], "rig");
    assert_eq!(v["prefab"]["path"], "rigs/column.rig.json");
    assert_eq!(v["prefab"]["open"], format!("{COLUMN_RIG}#prefab"));
    assert_eq!(v["prefab"]["handle"], Value::Null);
    let reason = v["prefab"]["readOnly"].as_str().unwrap();
    assert!(
        reason.contains("column.rig.json#prefab") && reason.contains("import --prefab"),
        "{reason}"
    );
    assert_eq!(
        names(&v),
        ["Imported", "Body", "hips", "spine", "head", "Prop"]
    );
    let prop = &v["entities"][5];
    let renderer = prop["components"]
        .as_object()
        .unwrap()
        .values()
        .find(|c| c["type"] == "fynkel.MeshRenderer")
        .unwrap();
    assert_eq!(renderer["data"]["material"]["asset"], RUSTED);
    assert_eq!(renderer["overridden"], json!(["material"]));
    assert_eq!(v["overrides"][0]["entityName"], "Prop");
    assert_eq!(v["overrides"][0]["componentType"], "fynkel.MeshRenderer");

    // Open the hierarchy itself: a read-only prefab document.
    let p = s.open_document(&format!("{COLUMN_RIG}#prefab")).unwrap();
    assert_eq!(
        s.open_document(&format!("{COLUMN_RIG}#prefab")).unwrap(),
        p,
        "idempotent"
    );
    assert_eq!(
        s.open_document("assets/rigs/column.rig.json#prefab")
            .unwrap(),
        p,
        "by path too"
    );
    assert_eq!(instance(&s, h, IMPORTED)["prefab"]["handle"], p);
    let state = s.state_json(p).unwrap();
    assert_eq!(state["kind"], "prefab");
    assert_eq!(state["sub"], "prefab");
    assert_eq!(state["relativePath"], "rigs/column.rig.json#prefab");
    assert!(state["path"].as_str().unwrap().ends_with("column.rig.json"));
    assert_eq!(state["readOnly"].as_str().unwrap(), reason);
    assert_eq!(state["dirty"], false);
    let doc = s.document_json(p).unwrap();
    let entities = doc["root"]["entities"].as_object().unwrap();
    assert_eq!(entities.len(), 6);
    let local_names: Vec<&str> = entities
        .values()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        local_names,
        ["Rig", "Body", "hips", "spine", "head", "Prop"]
    );
    assert_eq!(doc["roots"].as_array().unwrap().len(), 1);
    // Its entity ids are the ones the instance's `local` ids name.
    assert!(entities.contains_key(v["entities"][5]["local"].as_str().unwrap()));
    assert_eq!(s.inspect_json(p).unwrap()["entityCount"], 6);
    // Nothing edits, saves or undoes.
    let err = s
        .apply_json(
            p,
            &json!([{ "command": "renameObject", "id": v["entities"][0]["local"], "name": "X" }]),
        )
        .unwrap_err();
    assert_eq!(err[0].code.as_str(), "FYN-E0007");
    assert!(err[0].message.contains("read-only"));
    assert_eq!(s.save(p, None).unwrap_err().code.as_str(), "FYN-E0007");
    assert_eq!(s.undo(p).unwrap_err().code.as_str(), "FYN-E0007");
    s.reload(p).unwrap();
    assert_eq!(
        s.state_json(p).unwrap()["readOnly"].as_str().unwrap(),
        reason
    );
    // Not the same handle as the rig document's file would be, and the
    // scene's own handle lookup is untouched.
    assert_eq!(
        s.handle_of(&copy.dir.join("assets/rigs/column.rig.json")),
        None
    );
    assert_eq!(s.handle_of(&copy.dir.join(RIG_SCENE)), Some(h));
    assert_eq!(s.handles(), vec![h, p]);

    // A model source's hierarchy opens the same way; sources without one
    // and other subs are D0401.
    let g = s.open_document(&format!("{RIG_GLB}#prefab")).unwrap();
    let state = s.state_json(g).unwrap();
    assert_eq!(state["relativePath"], "models/rig.glb#prefab");
    assert!(state["readOnly"].as_str().unwrap().contains("model source"));
    assert_eq!(s.inspect_json(g).unwrap()["entityCount"], 6);
    for (what, code) in [
        (format!("{TEXTURE}#prefab"), "FYN-D0401"),
        (format!("{RIG_GLB}#mesh/0"), "FYN-D0401"),
        (format!("{CRATE_PREFAB}#prefab"), "FYN-D0401"),
        (
            "c0000000-0000-4000-8000-0000000000dd#prefab".to_string(),
            "FYN-T0006",
        ),
    ] {
        let err = s.open_document(&what).unwrap_err();
        assert_eq!(err[0].code.as_str(), code, "{what}: {err:#?}");
    }

    // Unpack Imported: it becomes the rig's root, the scene holds the five
    // derived entities under it, the way
    // `fynkel scene inspect --expand` shows them; one undo step.
    let r = apply(
        &mut s,
        h,
        json!([{ "command": "unpackPrefab", "entity": IMPORTED }]),
    );
    assert!(r["mutations"][0]["affectedIds"].as_array().unwrap().len() > 7);
    let inspect = s.inspect_json(h).unwrap();
    assert_eq!(inspect["entityCount"], 14);
    assert_eq!(inspect["instanceCount"], 0);
    assert_eq!(inspect["diagnostics"], json!([]));
    let expected = {
        let scene = Scene::from_document(
            Document::read(&copy.dir.join(RIG_SCENE), Some(DocumentKind::Scene)).unwrap(),
        );
        let (db, _) = fynkel_document::AssetDatabase::scan(
            &copy.dir.join("assets"),
            &fynkel_document::SchemaSet::embedded(),
        );
        let mut lib = PrefabLibrary::empty();
        lib.add_sources(
            &db,
            &PrefabLibrary::wanted_sources(&scene),
            &fynkel_import::ImportedPrefabs::default(),
        );
        expand(&scene, &lib, &fynkel_document::SchemaSet::embedded())
            .document
            .write()
            .unwrap()
    };
    assert_eq!(
        s.document(h).unwrap().editor.document().write().unwrap(),
        expected
    );
    s.undo(h).unwrap();
    assert_eq!(
        s.document(h).unwrap().editor.document().write().unwrap(),
        rig_text
    );
    assert_eq!(copy.read(RIG_SCENE), rig_text);
}
