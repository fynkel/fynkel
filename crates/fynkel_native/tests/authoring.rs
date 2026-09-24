//! The authoring ABI driven from Rust as a host would (feature
//! `authoring`): a session over a copy of the fixture project, the JSON
//! results and the status mapping of `include/fynkel_authoring.h`, NULL
//! safety, and the poison rule. The complete edit session (every §11.1
//! command, undo to the original bytes) is the Dart test in
//! `packages/fynkel_authoring`; this file proves the C edge.

#![cfg(feature = "authoring")]

use std::path::{Path, PathBuf};
use std::ptr;

use fynkel_contracts::Status;
use fynkel_native::authoring::*;
use fynkel_native::FynkelBufferView;
use serde_json::{json, Value};

const RIG_SCENE: &str = "assets/scenes/rig.scene.json";
const CAMERA: &str = "c0000000-0000-4000-8000-0000000004e0";

/// A private copy of the fixture project (removed on drop).
struct Copy {
    dir: PathBuf,
}

impl Copy {
    fn new(name: &str) -> Copy {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fynkel_document/fixtures/project")
            .canonicalize()
            .unwrap();
        let dir = std::env::temp_dir().join(format!(
            "fynkel-native-authoring-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&from, &dir);
        Copy {
            dir: dir.canonicalize().unwrap(),
        }
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

struct Host(*mut FynkelAuthoringSession);

impl Host {
    fn new() -> Host {
        let s = fynkel_authoring_session_create();
        assert!(!s.is_null());
        Host(s)
    }

    fn result(&self) -> Value {
        let v = unsafe { fynkel_authoring_result(self.0) };
        let bytes = view(v);
        serde_json::from_slice(bytes)
            .unwrap_or_else(|e| panic!("{e}: {:?}", String::from_utf8_lossy(bytes)))
    }

    fn open_project(&self, dir: &Path) -> Status {
        let d = dir.to_string_lossy();
        unsafe { fynkel_authoring_open_project(self.0, d.as_ptr(), d.len() as u32) }
    }

    fn open_document(&self, path: &str) -> (Status, u32) {
        let mut h = 0u32;
        let s = unsafe {
            fynkel_authoring_open_document(self.0, path.as_ptr(), path.len() as u32, &mut h)
        };
        (s, h)
    }

    fn apply(&self, handle: u32, request: &Value) -> Status {
        let text = request.to_string();
        unsafe { fynkel_authoring_apply(self.0, handle, text.as_ptr(), text.len() as u32) }
    }

    fn query(&self, kind: &str, args: Option<&Value>) -> Status {
        let text = args.map(|a| a.to_string());
        let (ptr, len) = match &text {
            Some(t) => (t.as_ptr(), t.len() as u32),
            None => (ptr::null(), 0),
        };
        unsafe { fynkel_authoring_query(self.0, kind.as_ptr(), kind.len() as u32, ptr, len) }
    }

    fn operate(&self, kind: &str, args: Option<&Value>) -> Status {
        let text = args.map(|a| a.to_string());
        let (ptr, len) = match &text {
            Some(t) => (t.as_ptr(), t.len() as u32),
            None => (ptr::null(), 0),
        };
        unsafe { fynkel_authoring_operate(self.0, kind.as_ptr(), kind.len() as u32, ptr, len) }
    }

    fn save(&self, handle: u32, path: Option<&Path>) -> Status {
        let text = path.map(|p| p.to_string_lossy().into_owned());
        let (ptr, len) = match &text {
            Some(t) => (t.as_ptr(), t.len() as u32),
            None => (ptr::null(), 0),
        };
        unsafe { fynkel_authoring_save(self.0, handle, ptr, len) }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        unsafe { fynkel_authoring_session_destroy(self.0) };
    }
}

fn view(v: FynkelBufferView) -> &'static [u8] {
    if v.length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(v.data, v.length as usize) }
    }
}

fn codes(v: &Value) -> Vec<&str> {
    v["diagnostics"]
        .as_array()
        .map(|a| a.iter().map(|d| d["code"].as_str().unwrap()).collect())
        .unwrap_or_default()
}

#[test]
fn version_and_null_safety() {
    assert_eq!(fynkel_authoring_abi_version(), AUTHORING_ABI_VERSION);
    unsafe {
        assert_eq!(fynkel_authoring_result(ptr::null()).length, 0);
        assert_eq!(fynkel_authoring_is_poisoned(ptr::null()), 0);
        assert_eq!(
            fynkel_authoring_open_project(ptr::null_mut(), ptr::null(), 0),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_close_project(ptr::null_mut()),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_open_document(ptr::null_mut(), ptr::null(), 0, ptr::null_mut()),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_close_document(ptr::null_mut(), 1),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_reload_document(ptr::null_mut(), 1),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_apply(ptr::null_mut(), 1, ptr::null(), 0),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_undo(ptr::null_mut(), 1),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_redo(ptr::null_mut(), 1),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_save(ptr::null_mut(), 1, ptr::null(), 0),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_rescan(ptr::null_mut(), ptr::null(), 0),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_authoring_query(ptr::null_mut(), ptr::null(), 0, ptr::null(), 0),
            Status::InvalidArgument
        );
        fynkel_authoring_session_destroy(ptr::null_mut());
    }

    // A live session with NULL text: INVALID_ARGUMENT and a T0003 result.
    let h = Host::new();
    assert_eq!(
        unsafe { fynkel_authoring_result(h.0) }.length,
        0,
        "empty before any call"
    );
    assert_eq!(
        unsafe { fynkel_authoring_open_project(h.0, ptr::null(), 3) },
        Status::InvalidArgument
    );
    let r = h.result();
    assert_eq!(r["ok"], false);
    assert_eq!(codes(&r), ["FYN-T0003"]);
    // Non-UTF-8 text is refused the same way.
    let bad = [0xffu8, 0xfe];
    assert_eq!(
        unsafe { fynkel_authoring_open_project(h.0, bad.as_ptr(), 2) },
        Status::InvalidArgument
    );
    assert_eq!(codes(&h.result()), ["FYN-T0003"]);
    // Without a project everything is T0001 → INVALID_ARGUMENT.
    assert_eq!(h.open_document(RIG_SCENE).0, Status::InvalidArgument);
    assert_eq!(codes(&h.result()), ["FYN-T0001"]);
    assert_eq!(h.query("project", None), Status::InvalidArgument);
    assert_eq!(codes(&h.result()), ["FYN-T0001"]);
    assert_eq!(h.query("commands", None), Status::Ok);
    assert!(h.result()["result"]
        .as_array()
        .unwrap()
        .contains(&json!("duplicateObject")));
}

#[test]
fn a_session_over_the_fixture_project() {
    let copy = Copy::new("session");
    let h = Host::new();

    // Not a project → LOAD_FAILED with the project diagnostic.
    assert_eq!(h.open_project(&std::env::temp_dir()), Status::LoadFailed);
    let r = h.result();
    assert_eq!(r["ok"], false);
    assert!(codes(&r)[0].starts_with("FYN-P"), "{r}");

    assert_eq!(h.open_project(&copy.dir), Status::Ok);
    let r = h.result();
    assert_eq!(r["ok"], true);
    assert_eq!(r["project"]["name"], "Fixture Game");
    assert_eq!(r["project"]["assetCount"], 17);
    assert_eq!(r["project"]["readOnly"], Value::Null);

    // Queries: with and without arguments; bad ones.
    assert_eq!(
        h.query("assets", Some(&json!({"kind": "source"}))),
        Status::Ok
    );
    assert_eq!(h.result()["result"].as_array().unwrap().len(), 6);
    assert_eq!(
        h.query("assets", Some(&json!([1]))),
        Status::InvalidArgument
    );
    assert_eq!(codes(&h.result()), ["FYN-T0003"]);
    assert_eq!(h.query("nope", None), Status::InvalidArgument);
    assert_eq!(codes(&h.result()), ["FYN-T0004"]);
    assert_eq!(
        h.query("schema", Some(&json!({"type": "fynkel.Transform"}))),
        Status::Ok
    );
    assert_eq!(h.result()["result"]["title"], "fynkel.Transform");

    // Open by path, then by id (idempotent), then a missing file.
    let (s, handle) = h.open_document(RIG_SCENE);
    assert_eq!(s, Status::Ok);
    assert_eq!(handle, 1);
    let r = h.result();
    assert_eq!(r["handle"], 1);
    assert_eq!(r["state"]["kind"], "scene");
    assert_eq!(r["state"]["dirty"], false);
    let original_revision = r["state"]["revision"].as_str().unwrap().to_string();
    let (s, again) = h.open_document("c0000000-0000-4000-8000-000000000034");
    assert_eq!((s, again), (Status::Ok, 1));
    assert_eq!(
        h.open_document("assets/scenes/nope.scene.json").0,
        Status::LoadFailed
    );
    assert_eq!(codes(&h.result()), ["FYN-T0006"]);
    assert_eq!(
        h.open_document("assets/models/rig.glb").0,
        Status::LoadFailed
    );
    assert_eq!(codes(&h.result()), ["FYN-D0015"]);

    // An edit; the Applied record and the state travel together.
    let rename = json!([{"command": "renameObject", "id": CAMERA, "name": "Eye"}]);
    assert_eq!(h.apply(handle, &rename), Status::Ok);
    let r = h.result();
    assert_eq!(r["ok"], true);
    assert_eq!(r["mutations"][0]["command"], "renameObject");
    assert_eq!(r["mutations"][0]["affectedIds"], json!([CAMERA]));
    assert_eq!(r["state"]["dirty"], true);
    assert_eq!(r["state"]["canUndo"], true);
    let revision = r["revision"].as_str().unwrap().to_string();
    assert_ne!(revision, original_revision);

    // A stale baseRevision: refused, nothing touched.
    let stale = json!({
        "baseRevision": original_revision,
        "commands": [{"command": "renameObject", "id": CAMERA, "name": "Nope"}],
    });
    assert_eq!(h.apply(handle, &stale), Status::CommandsRejected);
    let r = h.result();
    assert_eq!(r["ok"], false);
    assert_eq!(codes(&r), ["FYN-E0001"]);
    assert_eq!(r["state"]["revision"], revision);
    // Not JSON at all / a malformed command.
    let junk = "{";
    assert_eq!(
        unsafe { fynkel_authoring_apply(h.0, handle, junk.as_ptr(), junk.len() as u32) },
        Status::InvalidArgument
    );
    assert_eq!(codes(&h.result()), ["FYN-T0003"]);
    assert_eq!(
        h.apply(handle, &json!([{"command": "warp"}])),
        Status::CommandsRejected
    );
    assert_eq!(codes(&h.result()), ["FYN-E0004"]);
    // Unknown handle.
    assert_eq!(h.apply(7, &rename), Status::InvalidArgument);
    assert_eq!(codes(&h.result()), ["FYN-T0002"]);

    // The document as JSON shows the edit; inspect shows it typed.
    assert_eq!(
        h.query("document", Some(&json!({"handle": handle}))),
        Status::Ok
    );
    assert_eq!(
        h.result()["result"]["root"]["entities"][CAMERA]["name"],
        "Eye"
    );
    assert_eq!(
        h.query("inspect", Some(&json!({"handle": handle}))),
        Status::Ok
    );
    assert_eq!(h.result()["result"]["revision"], revision);

    // Undo → the original revision; redo → the edit; then nothing to redo.
    assert_eq!(unsafe { fynkel_authoring_undo(h.0, handle) }, Status::Ok);
    let r = h.result();
    assert_eq!(r["ok"], true);
    assert_eq!(r["commands"], json!(["renameObject"]));
    assert_eq!(r["state"]["revision"], original_revision);
    assert_eq!(r["state"]["dirty"], false);
    assert_eq!(unsafe { fynkel_authoring_redo(h.0, handle) }, Status::Ok);
    assert_eq!(h.result()["state"]["revision"], revision);
    assert_eq!(
        unsafe { fynkel_authoring_redo(h.0, handle) },
        Status::CommandsRejected
    );
    assert_eq!(codes(&h.result()), ["FYN-E0009"]);

    // Save in place, then save-as; the database follows.
    assert_eq!(h.save(handle, None), Status::Ok);
    let r = h.result();
    assert_eq!(r["state"]["dirty"], false);
    assert_eq!(r["path"], copy.dir.join(RIG_SCENE).display().to_string());
    assert!(std::fs::read_to_string(copy.dir.join(RIG_SCENE))
        .unwrap()
        .contains("\"Eye\""));
    let copy_path = copy.dir.join("assets/scenes/rig_copy.scene.json");
    assert_eq!(h.save(handle, Some(&copy_path)), Status::Ok);
    assert_eq!(
        h.result()["state"]["relativePath"],
        "scenes/rig_copy.scene.json"
    );
    // The copy carries the same id: the database keeps one entry and
    // reports the duplicate, as `fynkel validate` would.
    assert_eq!(h.query("project", None), Status::Ok);
    let p = h.result()["result"].clone();
    assert_eq!(p["assetCount"], 17);
    assert!(codes(&p).contains(&"FYN-A0003"), "{p}");
    // Rescan: everything, a list, a bad list.
    assert_eq!(
        unsafe { fynkel_authoring_rescan(h.0, ptr::null(), 0) },
        Status::Ok
    );
    assert_eq!(h.result()["assetCount"], 17);
    assert!(codes(&h.result()).contains(&"FYN-A0003"));
    let list = json!([RIG_SCENE]).to_string();
    assert_eq!(
        unsafe { fynkel_authoring_rescan(h.0, list.as_ptr(), list.len() as u32) },
        Status::Ok
    );
    assert_eq!(h.result()["scanned"], json!([RIG_SCENE]));
    let bad = json!({"a": 1}).to_string();
    assert_eq!(
        unsafe { fynkel_authoring_rescan(h.0, bad.as_ptr(), bad.len() as u32) },
        Status::InvalidArgument
    );

    // Reload drops the history; close makes the handle unknown; the
    // project closes its documents.
    assert_eq!(h.apply(handle, &rename), Status::Ok);
    assert_eq!(
        unsafe { fynkel_authoring_reload_document(h.0, handle) },
        Status::Ok
    );
    assert_eq!(h.result()["state"]["canUndo"], false);
    assert_eq!(
        unsafe { fynkel_authoring_close_document(h.0, handle) },
        Status::Ok
    );
    assert_eq!(
        unsafe { fynkel_authoring_close_document(h.0, handle) },
        Status::InvalidArgument
    );
    assert_eq!(codes(&h.result()), ["FYN-T0002"]);
    let (s, second) = h.open_document(RIG_SCENE);
    assert_eq!((s, second), (Status::Ok, 2), "handles are never reused");
    assert_eq!(unsafe { fynkel_authoring_close_project(h.0) }, Status::Ok);
    assert_eq!(h.result(), json!({"ok": true}));
    assert_eq!(h.query("documents", None), Status::Ok);
    assert_eq!(h.result()["result"], json!([]));
    assert_eq!(unsafe { fynkel_authoring_is_poisoned(h.0) }, 0);
}

#[test]
fn a_material_edits_through_document_fields() {
    let copy = Copy::new("material");
    let h = Host::new();
    assert_eq!(h.open_project(&copy.dir), Status::Ok);
    let (s, handle) = h.open_document("assets/materials/rusted.material.json");
    assert_eq!(s, Status::Ok);
    assert_eq!(h.result()["state"]["kind"], "material");
    let set = json!([{"command": "setDocumentField", "pointer": "/base_color", "value": [0.1, 0.2, 0.3, 1.0]}]);
    assert_eq!(h.apply(handle, &set), Status::Ok);
    assert_eq!(
        h.query("inspect", Some(&json!({"handle": handle}))),
        Status::Ok
    );
    let r = h.result();
    assert_eq!(r["result"]["kind"], "material");
    assert_eq!(
        r["result"]["fields"]["base_color"],
        json!([0.1, 0.2, 0.3, 1.0])
    );
    assert_eq!(r["result"]["shader"], "fynkel.pbr@1");
    // A value the shader refuses rolls back.
    let bad = json!([{"command": "setDocumentField", "pointer": "/base_color", "value": "red"}]);
    assert_eq!(h.apply(handle, &bad), Status::CommandsRejected);
    assert!(h.result()["ok"] == false);
    assert_eq!(unsafe { fynkel_authoring_undo(h.0, handle) }, Status::Ok);
    assert_eq!(h.result()["state"]["dirty"], false);
}

#[test]
fn a_newer_lock_opens_read_only_and_refuses_edits() {
    let copy = Copy::new("readonly");
    let lock_path = copy.dir.join("fynkel.lock.json");
    let lock = std::fs::read_to_string(&lock_path)
        .unwrap()
        .replace("\"version\": \"0.0.1\"", "\"version\": \"9.1.0\"");
    std::fs::write(&lock_path, lock).unwrap();
    let h = Host::new();
    assert_eq!(h.open_project(&copy.dir), Status::Ok);
    let r = h.result();
    assert!(r["project"]["readOnly"].is_string(), "{r}");
    let (s, handle) = h.open_document(RIG_SCENE);
    assert_eq!(s, Status::Ok);
    assert!(h.result()["state"]["readOnly"].is_string());
    let rename = json!([{"command": "renameObject", "id": CAMERA, "name": "Eye"}]);
    assert_eq!(h.apply(handle, &rename), Status::CommandsRejected);
    assert_eq!(codes(&h.result()), ["FYN-E0007"]);
    assert_eq!(h.save(handle, None), Status::CommandsRejected);
    assert_eq!(codes(&h.result()), ["FYN-E0007"]);
}

#[test]
fn asset_operations_cross_the_edge_with_their_statuses() {
    let copy = Copy::new("operate");
    let h = Host::new();
    // No project: invalid argument, like a query.
    assert_eq!(
        h.operate(
            "newDocument",
            Some(&json!({"kind": "scene", "path": "scenes/x"}))
        ),
        Status::InvalidArgument
    );
    assert_eq!(codes(&h.result()), ["FYN-T0001"]);
    assert_eq!(h.open_project(&copy.dir), Status::Ok);
    let before = h.result()["project"]["assetCount"].as_u64().unwrap();

    // The listing and the empty history.
    assert_eq!(h.query("operations", None), Status::Ok);
    let ops = h.result()["result"].clone();
    assert!(
        ops.as_array().unwrap().iter().any(|o| o == "renameAsset"),
        "{ops}"
    );
    assert_eq!(h.query("assetHistory", None), Status::Ok);
    assert_eq!(h.result()["result"]["canUndo"], false);

    // A new material, then a rename that keeps its id; the entry and the
    // history come back in the result.
    assert_eq!(
        h.operate(
            "newDocument",
            Some(&json!({"kind": "material", "path": "materials/gold"}))
        ),
        Status::Ok
    );
    let r = h.result();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["entry"]["name"], "gold");
    assert_eq!(r["assetCount"].as_u64().unwrap(), before + 1);
    assert_eq!(r["canUndo"], true);
    let id = r["id"].as_str().unwrap().to_string();
    assert!(copy
        .dir
        .join("assets/materials/gold.material.json")
        .is_file());
    assert_eq!(
        h.operate(
            "renameAsset",
            Some(&json!({"asset": id, "path": "materials/aurum.material.json"}))
        ),
        Status::Ok
    );
    let r = h.result();
    assert_eq!(r["entry"]["id"], id);
    assert_eq!(r["entry"]["path"], "materials/aurum.material.json");
    assert_eq!(
        h.query("assetDetail", Some(&json!({"asset": id}))),
        Status::Ok
    );
    assert_eq!(h.result()["result"]["material"]["shader"], "fynkel.pbr@1");

    // Refusals: a referenced asset's delete lists its dependents (T0003),
    // an unknown asset is A0004, an unknown operation T0004, bad input
    // never reaches the session.
    assert_eq!(
        h.operate(
            "deleteAsset",
            Some(&json!({"asset": "c0000000-0000-4000-8000-000000000020"}))
        ),
        Status::InvalidArgument
    );
    let r = h.result();
    assert_eq!(codes(&r), ["FYN-T0003"]);
    assert!(
        !r["diagnostics"][0]["related"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{r}"
    );
    assert_eq!(
        h.operate(
            "deleteAsset",
            Some(&json!({"asset": "00000000-0000-0000-0000-000000000000"}))
        ),
        Status::LoadFailed
    );
    assert_eq!(codes(&h.result()), ["FYN-A0004"]);
    assert_eq!(h.operate("explode", None), Status::InvalidArgument);
    assert_eq!(codes(&h.result()), ["FYN-T0004"]);
    assert_eq!(
        h.operate("newDocument", Some(&json!([1]))),
        Status::InvalidArgument
    );
    assert_eq!(codes(&h.result()), ["FYN-T0003"]);
    assert_eq!(
        unsafe { fynkel_authoring_operate(h.0, ptr::null(), 0, ptr::null(), 0) },
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe { fynkel_authoring_operate(ptr::null_mut(), ptr::null(), 0, ptr::null(), 0) },
        Status::InvalidArgument
    );

    // Undo twice walks the material back out; a third is E0009.
    assert_eq!(h.operate("undoAsset", None), Status::Ok);
    assert_eq!(h.result()["entry"]["path"], "materials/gold.material.json");
    assert_eq!(h.operate("undoAsset", None), Status::Ok);
    assert!(!copy
        .dir
        .join("assets/materials/gold.material.json")
        .exists());
    assert_eq!(h.result()["assetCount"].as_u64().unwrap(), before);
    assert_eq!(h.operate("undoAsset", None), Status::CommandsRejected);
    assert_eq!(codes(&h.result()), ["FYN-E0009"]);
    assert_eq!(h.operate("redoAsset", None), Status::Ok);
    assert_eq!(h.query("assetHistory", None), Status::Ok);
    assert_eq!(
        h.result()["result"]["redoLabel"],
        "Rename gold.material.json → aurum.material.json"
    );
}

// ---------------------------------------------------------------- previews

const MAIN_SCENE: &str = "assets/scenes/main.scene.json";
const MAIN_ROBOT: &str = "c0000000-0000-4000-8000-0000000000e1";
const MAIN_SUN: &str = "c0000000-0000-4000-8000-0000000000e3";
const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";
const MAIN_FLOOR: &str = "c0000000-0000-4000-8000-0000000000e4";
const MAIN_FLOOR_TRANSFORM: &str = "c0000000-0000-4000-8000-0000000000d0";

impl Host {
    fn preview_call(&self, handle: u32, kind: &str, args: &Value) -> Status {
        let text = args.to_string();
        unsafe {
            fynkel_authoring_preview_call(
                self.0,
                handle,
                kind.as_ptr(),
                kind.len() as u32,
                text.as_ptr(),
                text.len() as u32,
            )
        }
    }

    fn undo(&self, handle: u32) -> Status {
        unsafe { fynkel_authoring_undo(self.0, handle) }
    }
}

/// The edit-preview world across the edge (ABI v3): opened over a cook,
/// rendered from through its world pointer, picked, hidden, kept in step
/// with the document by apply and undo (the incremental dump equals a
/// reload's), recooked, and closed with its document.
#[test]
fn the_edit_preview_world_crosses_the_edge() {
    let copy = Copy::new("preview");
    let host = Host::new();
    assert_eq!(host.open_project(&copy.dir), Status::Ok);
    let (status, scene) = host.open_document(MAIN_SCENE);
    assert_eq!(status, Status::Ok);

    // Nothing before it is opened.
    assert!(unsafe { fynkel_authoring_preview_world(host.0, scene) }.is_null());
    assert_eq!(
        host.preview_call(scene, "stats", &json!({})),
        Status::InvalidArgument
    );
    assert_eq!(codes(&host.result()), ["FYN-T0002"]);

    // Open: the project cooks into its cache, the world loads.
    assert_eq!(
        unsafe { fynkel_authoring_preview_open(host.0, scene) },
        Status::Ok
    );
    let opened = host.result();
    assert_eq!(opened["opened"], true);
    assert_eq!(opened["cook"]["ok"], true, "{opened}");
    assert_eq!(opened["stats"]["entities"], 6);
    assert_eq!(opened["diagnostics"], json!([]));
    assert!(copy.dir.join(".fynkel/cache/index.json").is_file());
    // Again: idempotent, no second cook.
    assert_eq!(
        unsafe { fynkel_authoring_preview_open(host.0, scene) },
        Status::Ok
    );
    assert_eq!(host.result()["opened"], false);
    assert!(host.result().get("cook").is_none());

    // The world pointer is a live edit-mode world with the scene in it.
    let world = unsafe { fynkel_authoring_preview_world(host.0, scene) };
    assert!(!world.is_null());
    let core = unsafe { (*world).core() };
    assert!(core.is_edit_mode());
    let meshes = core.extract().meshes.len();
    assert!(meshes > 0);
    assert_eq!(json!(meshes), opened["stats"]["meshes"]);
    assert!(
        unsafe { (*world).assets() }.is_some(),
        "the viewport adopts the store"
    );

    // The editor camera: 20 m up and 2 m aside (the sun's icon sits right
    // above the robot), looking straight down.
    let looking_down = glam::Mat4::from_rotation_translation(
        glam::Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        glam::Vec3::new(2.0, 20.0, 0.0),
    );
    let view = json!({
        "world": looking_down.to_cols_array(),
        "aspect": 16.0 / 9.0,
        "near": 0.1,
        "far": 100.0,
    });
    assert_eq!(host.preview_call(scene, "camera", &view), Status::Ok);
    assert_eq!(
        core.edit_camera().map(|(m, _)| *m),
        Some(looking_down),
        "the extract draws through it"
    );

    // Pick through the pixel at the robot's centre.
    assert_eq!(
        host.preview_call(scene, "worldMatrix", &json!({ "ids": [MAIN_ROBOT] })),
        Status::Ok
    );
    let m = host.result()["matrices"][MAIN_ROBOT]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect::<Vec<_>>();
    let robot =
        glam::Mat4::from_cols_array(&m.try_into().unwrap()).transform_point3(glam::Vec3::Y * 0.5);
    let vp = fynkel_core::projection_matrix(
        &fynkel_core::components::Camera {
            far: 100.0,
            ..Default::default()
        },
        16.0 / 9.0,
    ) * fynkel_core::view_matrix(&looking_down);
    let clip = vp * robot.extend(1.0);
    let ndc = [clip.x / clip.w, clip.y / clip.w];
    let pick = json!({ "view": view, "ndc": ndc });
    assert_eq!(host.preview_call(scene, "pick", &pick), Status::Ok);
    let hits = host.result();
    assert_eq!(hits["hits"][0]["id"], MAIN_ROBOT, "{hits}");
    assert_eq!(hits["hits"][0]["mesh"], true);
    // The same pick with an explicit ray.
    let ray = json!({ "ray": { "origin": [robot.x, 5.0, robot.z], "dir": [0, -1, 0] } });
    assert_eq!(host.preview_call(scene, "pick", &ray), Status::Ok);
    assert_eq!(host.result()["hits"][0]["id"], MAIN_ROBOT);

    // Hidden: gone from the picks, counted in the stats; back again.
    assert_eq!(
        host.preview_call(scene, "hidden", &json!({ "ids": [MAIN_ROBOT] })),
        Status::Ok
    );
    assert_eq!(host.result()["hidden"], 1);
    assert_eq!(host.preview_call(scene, "pick", &pick), Status::Ok);
    assert!(
        host.result()["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["id"] != MAIN_ROBOT),
        "{}",
        host.result()
    );
    assert_eq!(host.preview_call(scene, "stats", &json!({})), Status::Ok);
    assert_eq!(host.result()["stats"]["hidden"], 1);
    assert_eq!(
        host.preview_call(scene, "hidden", &json!({ "ids": [] })),
        Status::Ok
    );

    // Lines: the robot's outline is a box (12 edges); the icons of a
    // camera and a sun are there.
    assert_eq!(
        host.preview_call(
            scene,
            "lines",
            &json!({ "outline": { "ids": [MAIN_ROBOT] }, "icons": { "aspect": 16.0 / 9.0 } })
        ),
        Status::Ok
    );
    let lines = host.result();
    assert!(lines["count"].as_u64().unwrap() > 12, "{}", lines["count"]);
    assert_eq!(lines["lines"][0].as_array().unwrap().len(), 10);
    // Without boxes the robot is left to the renderer's silhouette: only
    // the sun, which draws no mesh, keeps its cross (three segments).
    let crosses = |host: &Host| {
        assert_eq!(
            host.preview_call(
                scene,
                "lines",
                &json!({ "outline": { "ids": [MAIN_ROBOT, MAIN_SUN], "boxes": false } })
            ),
            Status::Ok
        );
        host.result()["count"].as_u64().unwrap()
    };
    assert_eq!(crosses(&host), 3);
    assert_eq!(
        host.preview_call(
            scene,
            "lines",
            &json!({ "outline": { "ids": [MAIN_ROBOT], "boxes": 1 } })
        ),
        Status::InvalidArgument
    );

    // The handles the renderer outlines: one per live id, the unknown
    // and the hidden left out.
    let entities = |host: &Host, ids: Value| {
        assert_eq!(
            host.preview_call(scene, "entities", &json!({ "ids": ids })),
            Status::Ok
        );
        host.result()["entities"].as_array().unwrap().clone()
    };
    let both = entities(
        &host,
        json!([MAIN_ROBOT, MAIN_SUN, "c0000000-0000-4000-8000-0000000000ff"]),
    );
    assert_eq!(both.len(), 2, "{both:?}");
    assert!(both.iter().all(|b| b.as_u64().is_some_and(|b| b != 0)));
    assert_ne!(both[0], both[1]);
    assert_eq!(
        host.preview_call(scene, "hidden", &json!({ "ids": [MAIN_ROBOT] })),
        Status::Ok
    );
    assert_eq!(
        entities(&host, json!([MAIN_ROBOT, MAIN_SUN])),
        vec![both[1].clone()]
    );
    assert_eq!(
        host.preview_call(scene, "hidden", &json!({ "ids": [] })),
        Status::Ok
    );

    // A transaction: the apply result carries the preview's report and
    // the incremental world dumps exactly like a reload of the document.
    assert_eq!(host.preview_call(scene, "dump", &json!({})), Status::Ok);
    let before = host.result()["dump"].clone();
    assert_eq!(
        host.apply(
            scene,
            &json!([{ "command": "setField", "entity": MAIN_FLOOR, "component": MAIN_FLOOR_TRANSFORM, "field": "position", "value": [0, -1.5, 0] }])
        ),
        Status::Ok
    );
    let applied = host.result();
    assert_eq!(applied["previews"][0]["handle"], scene, "{applied}");
    assert_eq!(applied["previews"][0]["updated"], 1);
    assert_eq!(applied["previews"][0]["reloaded"], false);
    assert_eq!(host.preview_call(scene, "dump", &json!({})), Status::Ok);
    let incremental = host.result()["dump"].clone();
    assert_ne!(incremental, before);
    assert_eq!(host.preview_call(scene, "reload", &json!({})), Status::Ok);
    assert_eq!(host.result()["stats"]["entities"], 6);
    assert_eq!(host.preview_call(scene, "dump", &json!({})), Status::Ok);
    assert_eq!(host.result()["dump"], incremental, "incremental == fresh");
    // Undo brings the world back with the document.
    assert_eq!(host.undo(scene), Status::Ok);
    assert_eq!(host.result()["previews"][0]["updated"], 1);
    assert_eq!(host.preview_call(scene, "dump", &json!({})), Status::Ok);
    assert_eq!(host.result()["dump"], before);

    // The gizmo's preview is not an edit: the document stays clean and a
    // resync finds nothing to do once the transforms are restored.
    assert_eq!(
        host.preview_call(
            scene,
            "setTransforms",
            &json!({ "transforms": [{ "id": MAIN_ROBOT, "position": [3, 0, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] }] })
        ),
        Status::Ok
    );
    assert_eq!(
        host.query("document", Some(&json!({ "handle": scene }))),
        Status::Ok
    );
    assert_eq!(host.result()["result"]["dirty"], false);
    assert_eq!(
        host.preview_call(scene, "restoreTransforms", &json!({ "ids": [MAIN_ROBOT] })),
        Status::Ok
    );
    assert_eq!(host.preview_call(scene, "resync", &json!({})), Status::Ok);
    let resync = host.result();
    assert_eq!(
        (&resync["updated"], &resync["spawned"], &resync["steps"]),
        (&json!(0), &json!(0), &json!(0))
    );

    // The Inspector's live colour: set in the world only (a resync finds
    // the one difference), and restored so a resync finds none.
    let red = json!({ "components": [{ "id": MAIN_SUN, "type": "fynkel.DirectionalLight", "data": { "color": [1, 0, 0], "intensity": 10000.0, "cast_shadows": true } }] });
    assert_eq!(host.preview_call(scene, "setComponents", &red), Status::Ok);
    assert_eq!(host.result()["count"], 1);
    assert_eq!(
        host.query("document", Some(&json!({ "handle": scene }))),
        Status::Ok
    );
    assert_eq!(host.result()["result"]["dirty"], false);
    assert_eq!(host.preview_call(scene, "resync", &json!({})), Status::Ok);
    assert_eq!(host.result()["updated"], 1, "the world held the red sun");
    assert_eq!(host.preview_call(scene, "setComponents", &red), Status::Ok);
    assert_eq!(
        host.preview_call(scene, "restoreComponents", &json!({ "ids": [MAIN_SUN] })),
        Status::Ok
    );
    assert_eq!(host.preview_call(scene, "resync", &json!({})), Status::Ok);
    assert_eq!(host.result()["updated"], 0);
    // A behaviour is skipped (nothing draws it); bad data is refused.
    assert_eq!(
        host.preview_call(
            scene,
            "setComponents",
            &json!({ "components": [{ "id": MAIN_SUN, "type": "game.Spin", "data": {} }] })
        ),
        Status::Ok
    );
    assert_eq!(host.result()["count"], 0);
    assert_ne!(
        host.preview_call(
            scene,
            "setComponents",
            &json!({ "components": [{ "id": MAIN_SUN, "type": "fynkel.DirectionalLight", "data": { "color": "red" } }] })
        ),
        Status::Ok
    );

    // The Inspector's live material: drawn in place of the cooked one by
    // the store, the file untouched; restored, the cooked one comes back.
    let rusted = fynkel_contracts::AssetRef::new(fynkel_contracts::Uuid(
        *uuid::Uuid::parse_str(RUSTED).unwrap().as_bytes(),
    ));
    let store = unsafe { (*world).assets() }.expect("the preview draws from the cache");
    assert_eq!(store.material(&rusted).unwrap().metallic, 0.25);
    let edits = store.live_edits();
    let file = std::fs::read(copy.dir.join("assets/materials/rusted.material.json")).unwrap();
    assert_eq!(
        host.preview_call(
            scene,
            "setMaterial",
            &json!({ "material": RUSTED, "data": { "shader": "fynkel.pbr@1", "metallic": 0.9, "base_color": [0, 1, 0, 1] } })
        ),
        Status::Ok
    );
    assert_eq!(host.result()["stores"], 1);
    let live = store.material(&rusted).unwrap();
    assert_eq!((live.metallic, live.base_color[1]), (0.9, 1.0));
    assert_eq!(
        host.preview_call(scene, "material", &json!({ "material": RUSTED })),
        Status::Ok
    );
    assert_eq!(host.result()["material"]["metallic"], 0.9);
    assert!(
        store.live_edits() > edits,
        "the renderer rebuilds its materials"
    );
    assert_eq!(
        std::fs::read(copy.dir.join("assets/materials/rusted.material.json")).unwrap(),
        file
    );
    assert_eq!(
        host.preview_call(scene, "restoreMaterial", &json!({ "material": RUSTED })),
        Status::Ok
    );
    assert_eq!(store.material(&rusted).unwrap().metallic, 0.25);
    assert_ne!(
        host.preview_call(
            scene,
            "setMaterial",
            &json!({ "material": RUSTED, "data": { "metallic": "shiny" } })
        ),
        Status::Ok
    );

    // The live volume profile the same way (this project cooks none: the
    // edit is what the store holds, restored it is gone again).
    let look = "c0000000-0000-4000-8000-0000000000f7";
    let edits = store.live_edits();
    assert_eq!(
        host.preview_call(
            scene,
            "setVolume",
            &json!({ "volume": look, "data": { "bloom": { "intensity": 2, "tint": [1, 0.5, 0] } } })
        ),
        Status::Ok
    );
    assert_eq!(host.result()["stores"], 1);
    assert!(
        store.live_edits() > edits,
        "the renderer re-reads its profiles"
    );
    assert_eq!(
        host.preview_call(scene, "volume", &json!({ "volume": look })),
        Status::Ok
    );
    assert_eq!(
        host.result()["volume"],
        json!({ "bloom": { "intensity": 2.0, "tint": [1.0, 0.5, 0.0] } })
    );
    assert_eq!(
        host.preview_call(scene, "restoreVolume", &json!({ "volume": look })),
        Status::Ok
    );
    assert_ne!(
        host.preview_call(scene, "volume", &json!({ "volume": look })),
        Status::Ok
    );
    assert_ne!(
        host.preview_call(
            scene,
            "setVolume",
            &json!({ "volume": look, "data": { "bloom": { "intensity": "bright" } } })
        ),
        Status::Ok
    );

    // Recook: the cache rebuilt, every preview reloaded over the new store.
    assert_eq!(host.preview_call(scene, "recook", &json!({})), Status::Ok);
    let recook = host.result();
    assert_eq!(recook["cook"]["ok"], true, "{recook}");
    assert_eq!(recook["previews"][0]["handle"], scene);
    assert_eq!(recook["previews"][0]["stats"]["entities"], 6);
    let world2 = unsafe { fynkel_authoring_preview_world(host.0, scene) };
    assert_eq!(world, world2, "the world pointer stays put");

    // A manual exposure (an aligned scene camera's) reaches the edit camera.
    let view = json!({
        "world": looking_down.to_cols_array(),
        "aspect": 16.0 / 9.0,
        "exposureMode": "manual",
        "exposure": 12.5,
    });
    assert_eq!(host.preview_call(scene, "camera", &view), Status::Ok);
    let (_, camera) = core.edit_camera().unwrap();
    assert_eq!(
        camera.exposure_mode,
        fynkel_core::components::ExposureMode::Manual
    );
    assert_eq!(camera.exposure, 12.5);

    // Refusals: bad arguments, an unknown call, a material.
    assert_eq!(
        host.preview_call(scene, "pick", &json!({ "ndc": [0, 0] })),
        Status::InvalidArgument
    );
    assert_eq!(codes(&host.result()), ["FYN-T0003"]);
    let view =
        json!({ "world": looking_down.to_cols_array(), "aspect": 1.0, "exposureMode": "bright" });
    assert_eq!(
        host.preview_call(scene, "camera", &view),
        Status::InvalidArgument
    );
    assert_eq!(codes(&host.result()), ["FYN-T0003"]);
    assert_eq!(
        host.preview_call(scene, "teleport", &json!({})),
        Status::InvalidArgument
    );
    assert_eq!(codes(&host.result()), ["FYN-T0004"]);
    let (status, material) = host.open_document("assets/materials/rusted.material.json");
    assert_eq!(status, Status::Ok);
    assert_eq!(
        unsafe { fynkel_authoring_preview_open(host.0, material) },
        Status::InvalidArgument
    );
    assert_eq!(codes(&host.result()), ["FYN-T0003"]);

    // Closing the document closes its preview; the pointer is gone.
    assert_eq!(
        unsafe { fynkel_authoring_close_document(host.0, scene) },
        Status::Ok
    );
    assert_eq!(host.result()["previewClosed"], true);
    assert!(unsafe { fynkel_authoring_preview_world(host.0, scene) }.is_null());

    // NULL safety.
    assert!(unsafe { fynkel_authoring_preview_world(ptr::null(), 0) }.is_null());
    assert_eq!(
        unsafe { fynkel_authoring_preview_open(ptr::null_mut(), 0) },
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe { fynkel_authoring_preview_close(ptr::null_mut(), 0) },
        Status::InvalidArgument
    );
    assert_eq!(
        unsafe {
            fynkel_authoring_preview_call(ptr::null_mut(), 0, ptr::null(), 0, ptr::null(), 0)
        },
        Status::InvalidArgument
    );
}
