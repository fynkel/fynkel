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
    assert_eq!(r["project"]["assetCount"], 15);
    assert_eq!(r["project"]["readOnly"], Value::Null);

    // Queries: with and without arguments; bad ones.
    assert_eq!(
        h.query("assets", Some(&json!({"kind": "source"}))),
        Status::Ok
    );
    assert_eq!(h.result()["result"].as_array().unwrap().len(), 5);
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
    assert_eq!(p["assetCount"], 15);
    assert!(codes(&p).contains(&"FYN-A0003"), "{p}");
    // Rescan: everything, a list, a bad list.
    assert_eq!(
        unsafe { fynkel_authoring_rescan(h.0, ptr::null(), 0) },
        Status::Ok
    );
    assert_eq!(h.result()["assetCount"], 15);
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
