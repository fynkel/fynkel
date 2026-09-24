//! `setLayersAndTags` (ADR-0020, L4) over a copy of the fixture project:
//! a rename or an addition writes the project file only; a reorder or a
//! removal rewrites every scene and prefab so each object keeps its
//! meaning (layer indices, tag bits, layer masks, prefab overrides), the
//! project stays valid, and undo/redo put the bytes back. Refused with
//! nothing written for bad lists and for a dirty document that uses them.

use std::path::{Path, PathBuf};

use fynkel_authoring::AuthoringSession;
use fynkel_document::Diagnostic;
use serde_json::{json, Value};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
}

/// A private copy of the fixture project (removed on drop).
struct Copy {
    dir: PathBuf,
}

impl Copy {
    fn new(name: &str) -> Copy {
        let dir = std::env::temp_dir().join(format!("fynkel-layers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture(), &dir);
        Copy {
            dir: dir.canonicalize().unwrap(),
        }
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.join(rel)).unwrap()
    }

    fn json(&self, rel: &str) -> Value {
        serde_json::from_str(&self.read(rel)).unwrap()
    }

    fn edit(&self, rel: &str, f: impl FnOnce(&mut Value)) {
        let mut v = self.json(rel);
        f(&mut v);
        std::fs::write(
            self.dir.join(rel),
            serde_json::to_string_pretty(&v).unwrap() + "\n",
        )
        .unwrap();
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

fn ok(r: Result<Value, Vec<Diagnostic>>) -> Value {
    let v = r.map_err(|d| format!("{d:#?}")).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    v
}

fn refused(r: Result<Value, Vec<Diagnostic>>, code: &str) -> Diagnostic {
    let d = r.map(|v| format!("{v}")).unwrap_err();
    assert_eq!(d.len(), 1, "{d:#?}");
    assert_eq!(d[0].code.as_str(), code, "{d:#?}");
    d.into_iter().next().unwrap()
}

fn assert_clean(s: &AuthoringSession) {
    let report = s.validate_project().unwrap();
    let errors: Vec<_> = report.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let errors: Vec<_> = s
        .project()
        .unwrap()
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .collect();
    assert!(errors.is_empty(), "session diagnostics: {errors:#?}");
}

const PROJECT: &str = "fynkel.project.json";
const MAIN: &str = "assets/scenes/main.scene.json";
const SPARK: &str = "assets/prefabs/spark.prefab.json";
const COIN: &str = "assets/prefabs/coin.prefab.json";
const CRATE_A: &str = "c0000000-0000-4000-8000-0000000000e2";
const CRATE_B: &str = "c0000000-0000-4000-8000-0000000000e5";
const FLOOR: &str = "c0000000-0000-4000-8000-0000000000e4";
const ADDED_COLLIDER: &str = "c0000000-0000-4000-8000-0000000000f7";

/// The fixture with tags and partial masks to move: tags `[pickup,
/// enemy]`; Crate B (layer `props`) tagged `enemy`; the floor collides
/// with `props` only; Crate A's instance adds a collider (default + props)
/// and a `setField` narrows it to `props`.
fn seeded(name: &str) -> Copy {
    let copy = Copy::new(name);
    copy.edit(PROJECT, |p| p["tags"] = json!(["pickup", "enemy"]));
    copy.edit(MAIN, |m| {
        let e = &mut m["entities"];
        e[FLOOR]["components"]["c0000000-0000-4000-8000-0000000000d1"]["data"]["mask"] = json!(2);
        e[CRATE_B]["components"]["c0000000-0000-4000-8000-0000000000f6"] = json!({
            "type": "fynkel.Tags", "version": 1, "data": { "bits": 2 }
        });
        let overrides = e[CRATE_A]["prefab"]["overrides"].as_array_mut().unwrap();
        overrides.push(json!({
            "op": "addComponent",
            "entity": "c0000000-0000-4000-8000-0000000000b1",
            "component": ADDED_COLLIDER,
            "type": "fynkel.Collider",
            "version": 1,
            "data": {
                "shape": "box", "size": [1, 1, 1], "mesh": null, "offset": [0, 0, 0],
                "friction": 0.5, "restitution": 0.0, "is_sensor": false, "mask": 3
            }
        }));
        overrides.push(json!({
            "op": "setField",
            "entity": "c0000000-0000-4000-8000-0000000000b1",
            "component": ADDED_COLLIDER,
            "field": "mask",
            "value": 2
        }));
    });
    copy
}

fn component<'a>(scene: &'a Value, entity: &str, component: &str) -> &'a Value {
    &scene["entities"][entity]["components"][component]["data"]
}

#[test]
fn renames_and_additions_write_the_project_file_only() {
    let copy = seeded("rename");
    let mut s = open(&copy);
    assert_clean(&s);
    let main = copy.read(MAIN);
    let spark = copy.read(SPARK);
    let project = copy.read(PROJECT);

    let v = ok(s.operate(
        "setLayersAndTags",
        &json!({ "layers": ["default", "crates", "ghosts"], "tags": ["coin", "enemy", "boss"] }),
    ));
    assert_eq!(v["rewritten"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["reload"], json!([]));
    let lt = &s.project().unwrap().project.layers_and_tags;
    assert_eq!(lt.layers, ["default", "crates", "ghosts"]);
    assert_eq!(lt.tags, ["coin", "enemy", "boss"]);
    assert_eq!(copy.read(MAIN), main);
    assert_eq!(copy.read(SPARK), spark);
    assert_eq!(
        copy.json(PROJECT)["layers"],
        json!(["default", "crates", "ghosts"])
    );
    assert_clean(&s);

    let history = s.query("assetHistory", &json!({})).unwrap();
    assert_eq!(history["undoLabel"], "Edit Tags and Layers");
    ok(s.operate("undoAsset", &json!({})));
    assert_eq!(copy.read(PROJECT), project);
    assert_eq!(
        s.project().unwrap().project.layers_and_tags.layers,
        ["default", "props"]
    );
    ok(s.operate("redoAsset", &json!({})));
    assert_eq!(
        s.project().unwrap().project.layers_and_tags.tags,
        ["coin", "enemy", "boss"]
    );

    // The same lists again: nothing to do.
    let d = refused(
        s.operate(
            "setLayersAndTags",
            &json!({ "layers": ["default", "crates", "ghosts"], "tags": ["coin", "enemy", "boss"] }),
        ),
        "FYN-T0003",
    );
    assert!(d.message.contains("already has"), "{}", d.message);
}

#[test]
fn a_reorder_moves_indices_bits_and_masks_everywhere() {
    let copy = seeded("reorder");
    let mut s = open(&copy);
    let before: Vec<String> = [PROJECT, MAIN, SPARK, COIN]
        .iter()
        .map(|f| copy.read(f))
        .collect();

    // A layer before props; the tags swap.
    let v = ok(s.operate(
        "setLayersAndTags",
        &json!({
            "layers": ["default", "ghosts", "props"], "layersFrom": [0, null, 1],
            "tags": ["enemy", "pickup"], "tagsFrom": [1, 0],
        }),
    ));
    let rewritten: Vec<&str> = v["rewritten"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert!(
        rewritten.iter().any(|f| f.ends_with("main.scene.json")),
        "{v}"
    );
    assert!(
        rewritten.iter().any(|f| f.ends_with("spark.prefab.json")),
        "{v}"
    );
    // The coin's masks are Everything: untouched.
    assert!(
        !rewritten.iter().any(|f| f.ends_with("coin.prefab.json")),
        "{v}"
    );
    assert_eq!(copy.read(COIN), before[3]);

    let main = copy.json(MAIN);
    let b = CRATE_B;
    assert_eq!(
        component(&main, b, "f11ff3e3-dccd-5aac-9202-dfbdaccd166e")["layer"],
        2
    );
    assert_eq!(
        component(&main, b, "c0000000-0000-4000-8000-0000000000f6")["bits"],
        1
    );
    assert_eq!(
        component(&main, b, "c0000000-0000-4000-8000-0000000000d4")["mask"],
        json!(u32::MAX),
        "Everything stays Everything"
    );
    assert_eq!(
        component(&main, FLOOR, "c0000000-0000-4000-8000-0000000000d1")["mask"],
        4,
        "props only follows props"
    );
    let overrides = &main["entities"][CRATE_A]["prefab"]["overrides"];
    assert_eq!(overrides[0]["value"], false, "cast_shadows untouched");
    assert_eq!(overrides[1]["field"], "position", "the placement untouched");
    assert_eq!(overrides[2]["data"]["mask"], 5);
    assert_eq!(overrides[3]["value"], 4);
    let spark = copy.json(SPARK);
    let spark_layer = spark["entities"]["c0000000-0000-4000-8000-0000000000b2"]["components"]
        ["3a7e307e-e092-5956-8abe-d1bad540c72e"]["data"]["layer"]
        .clone();
    assert_eq!(spark_layer, 2);
    assert_clean(&s);

    ok(s.operate("undoAsset", &json!({})));
    for (f, text) in [PROJECT, MAIN, SPARK, COIN].iter().zip(&before) {
        assert_eq!(copy.read(f), *text, "{f}");
    }
    assert_clean(&s);
    ok(s.operate("redoAsset", &json!({})));
    assert_eq!(copy.json(SPARK), spark);

    // A file changed behind the operation's back: undo refuses.
    let main_text = copy.read(MAIN);
    std::fs::write(copy.dir.join(SPARK), &before[2]).unwrap();
    let d = refused(s.operate("undoAsset", &json!({})), "FYN-T0003");
    assert!(d.message.contains("changed since"), "{}", d.message);
    assert_eq!(copy.read(MAIN), main_text, "nothing written");
}

#[test]
fn a_removal_sends_objects_to_default_and_drops_the_bits() {
    let copy = seeded("remove");
    let mut s = open(&copy);
    ok(s.operate(
        "setLayersAndTags",
        &json!({ "layers": ["default"], "tags": ["enemy"], "tagsFrom": [1] }),
    ));
    let main = copy.json(MAIN);
    assert_eq!(
        component(&main, CRATE_B, "f11ff3e3-dccd-5aac-9202-dfbdaccd166e")["layer"],
        0
    );
    assert_eq!(
        component(&main, CRATE_B, "c0000000-0000-4000-8000-0000000000f6")["bits"],
        1
    );
    assert_eq!(
        component(&main, FLOOR, "c0000000-0000-4000-8000-0000000000d1")["mask"],
        0
    );
    assert_eq!(
        component(&main, CRATE_B, "c0000000-0000-4000-8000-0000000000d4")["mask"],
        json!(u32::MAX)
    );
    assert_eq!(
        main["entities"][CRATE_A]["prefab"]["overrides"][2]["data"]["mask"],
        1
    );
    assert_clean(&s);
}

#[test]
fn open_documents_are_reloaded_or_protect_their_edits() {
    let copy = seeded("open");
    let mut s = open(&copy);
    let spark = s.open_document(SPARK).unwrap();
    let main = s.open_document(MAIN).unwrap();
    // An unsaved edit in the scene: the reorder that needs it is refused.
    s.apply_json(
        main,
        &json!([{ "command": "renameObject", "id": FLOOR, "name": "Ground" }]),
    )
    .map_err(|d| format!("{d:#?}"))
    .unwrap();
    let project = copy.read(PROJECT);
    let spark_text = copy.read(SPARK);
    let args = json!({ "layers": ["default", "ghosts", "props"], "layersFrom": [0, null, 1], "tags": ["pickup", "enemy"] });
    let d = refused(s.operate("setLayersAndTags", &args), "FYN-T0003");
    assert!(d.message.contains("unsaved"), "{}", d.message);
    assert_eq!(copy.read(PROJECT), project);
    assert_eq!(copy.read(SPARK), spark_text);

    // A rename touches no document: allowed with the edit still there.
    ok(s.operate(
        "setLayersAndTags",
        &json!({ "layers": ["default", "crates"], "tags": ["pickup", "enemy"] }),
    ));

    // Saved: the rewrite goes through and both open documents reload.
    s.save(main, None).unwrap();
    let v = ok(s.operate(
        "setLayersAndTags",
        &json!({ "layers": ["default", "ghosts", "crates"], "layersFrom": [0, null, 1], "tags": ["pickup", "enemy"] }),
    ));
    let mut reload: Vec<u64> = v["reload"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h.as_u64().unwrap())
        .collect();
    reload.sort();
    let mut expected = vec![spark as u64, main as u64];
    expected.sort();
    assert_eq!(reload, expected, "{v}");
}

#[test]
fn bad_lists_are_refused() {
    let copy = Copy::new("bad");
    let mut s = open(&copy);
    let project = copy.read(PROJECT);
    for (args, says) in [
        (
            json!({ "layers": ["props", "default"], "tags": [] }),
            "default",
        ),
        (
            json!({ "layers": ["default", "props"], "layersFrom": [1, 0], "tags": [] }),
            "default",
        ),
        (
            json!({ "layers": ["default", "a", "a"], "tags": [] }),
            "twice",
        ),
        (json!({ "layers": ["default", " "], "tags": [] }), "empty"),
        (json!({ "layers": ["default"], "tags": [1] }), "names"),
        (json!({ "tags": [] }), "required"),
        (
            json!({ "layers": ["default", "x"], "layersFrom": [0, 5], "tags": [] }),
            "old index",
        ),
        (
            json!({ "layers": ["default", "x", "y"], "layersFrom": [0, 1, 1], "tags": [] }),
            "used twice",
        ),
        (
            json!({ "layers": ["default", "x"], "layersFrom": [0], "tags": [] }),
            "entries for",
        ),
        (
            json!({ "layers": ["default"], "tags": (0..33).map(|i| format!("t{i}")).collect::<Vec<_>>() }),
            "at most 32",
        ),
    ] {
        let d = refused(s.operate("setLayersAndTags", &args), "FYN-T0003");
        assert!(d.message.contains(says), "{args}: {}", d.message);
    }
    assert_eq!(copy.read(PROJECT), project);
    let history = s.query("assetHistory", &json!({})).unwrap();
    assert_eq!(history["canUndo"], false);
}
