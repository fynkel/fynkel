//! The session over a copy of the fixture project: open, every kind of
//! read, one full edit cycle per document kind, undo to the original
//! bytes, save, the stale-revision conflict, dependents and rescan, the
//! read-only path, and the `FYN-T` errors for bad input.

use std::path::{Path, PathBuf};

use fynkel_authoring::{AuthoringSession, Handle, COMMANDS};
use fynkel_document::authoring::Request;
use fynkel_document::Code;
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
        let dir =
            std::env::temp_dir().join(format!("fynkel-authoring-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture(), &dir);
        Copy {
            dir: dir.canonicalize().unwrap(),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap()
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

const RIG_SCENE: &str = "assets/scenes/rig.scene.json";
const RUSTED: &str = "assets/materials/rusted.material.json";
const RUSTED_ID: &str = "c0000000-0000-4000-8000-000000000020";
const NORMAL_TEXTURE: &str = "c0000000-0000-4000-8000-000000000003";

fn code_of(diags: &[fynkel_document::Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn open(copy: &Copy) -> AuthoringSession {
    let mut s = AuthoringSession::new();
    let p = s
        .open_project(&copy.dir)
        .map_err(|d| format!("{d:?}"))
        .unwrap();
    assert_eq!(p.project.name(), "Fixture Game");
    s
}

#[test]
fn a_project_opens_with_its_lock_assets_schemas_and_references() {
    let copy = Copy::new("open");
    let s = open(&copy);
    let p = s.project().unwrap();
    assert!(p.read_only.is_none(), "{:?}", p.read_only);
    assert!(p.lock.is_some());
    // 11 documents (5 scenes, 3 prefabs, 1 material, 1 rig, 1 blend space) + 6 sources.
    assert_eq!(p.assets.len(), 17);
    assert!(p.metadata.is_none(), "the fixture has no lib/generated");
    // Declared behaviour, no metadata → a warning saying to run codegen.
    let codes = code_of(&p.diagnostics);
    assert!(codes.contains(&"FYN-T0006"), "{codes:?}");
    assert!(
        !p.diagnostics.iter().any(|d| d.is_error()),
        "{:?}",
        p.diagnostics
    );
    assert!(p.schema("fynkel.Transform").is_some());
    assert_eq!(
        p.schema("game.Spinner").unwrap()["properties"]["speed"]["x-fynkel"]["tooltip"],
        "Angular speed in radians per second"
    );
    assert!(p.schema("nope").is_none());
    assert!(p.references.len() >= 28, "{}", p.references.len());

    let v = s.query("project", &json!({})).unwrap();
    assert_eq!(v["name"], "Fixture Game");
    assert_eq!(v["dir"], copy.dir.display().to_string());
    assert_eq!(v["readOnly"], Value::Null);
    assert_eq!(v["assetCount"], 17);
    assert!(v["componentTypes"]
        .as_array()
        .unwrap()
        .contains(&json!("fynkel.MeshRenderer")));
    assert!(v["behaviourTypes"]
        .as_array()
        .unwrap()
        .contains(&json!("game.Spinner")));

    let schemas = s.query("schemas", &json!({})).unwrap();
    let transform = schemas["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "fynkel.Transform")
        .unwrap();
    assert_eq!(transform["id"], 1);
    assert_eq!(transform["allowed"], true);
    assert_eq!(transform["schema"]["title"], "fynkel.Transform");
    let spinner = schemas["behaviours"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "game.Spinner")
        .unwrap();
    assert_eq!(spinner["hasSchema"], true);
    assert_eq!(spinner["dart"], "package:spinner_game/spinner.dart#Spinner");

    let one = s
        .query("schema", &json!({"type": "fynkel.Camera"}))
        .unwrap();
    assert_eq!(one["x-fynkel"]["kind"], "component");
    let err = s.query("schema", &json!({"type": "nope"})).unwrap_err();
    assert_eq!(code_of(&err), ["FYN-T0003"]);
    assert_eq!(s.query("metadata", &json!({})).unwrap(), Value::Null);
    assert_eq!(s.query("commands", &json!({})).unwrap(), json!(COMMANDS));

    let all = s.query("assets", &json!({})).unwrap();
    assert_eq!(all.as_array().unwrap().len(), 17);
    let scenes = s.query("assets", &json!({"document": "scene"})).unwrap();
    assert_eq!(scenes.as_array().unwrap().len(), 5);
    let sources = s.query("assets", &json!({"kind": "source"})).unwrap();
    assert_eq!(sources.as_array().unwrap().len(), 6);
    let bad = s.query("assets", &json!({"kind": "thing"})).unwrap_err();
    assert_eq!(code_of(&bad), ["FYN-T0003"]);

    let rusted = s.query("asset", &json!({"asset": RUSTED_ID})).unwrap();
    assert_eq!(rusted["path"], "materials/rusted.material.json");
    let sub = s
        .query(
            "asset",
            &json!({"asset": "c0000000-0000-4000-8000-000000000050", "sub": "anim/wave"}),
        )
        .unwrap();
    assert_eq!(sub["sub"]["id"], "anim/wave");
    let missing = s
        .query(
            "asset",
            &json!({"asset": "c0000000-0000-4000-8000-0000000000ff"}),
        )
        .unwrap_err();
    assert_eq!(code_of(&missing), ["FYN-A0004"]);
    let malformed = s
        .query("asset", &json!({"asset": "not-a-uuid"}))
        .unwrap_err();
    assert_eq!(code_of(&malformed), ["FYN-T0003"]);

    // The normal map is used by the rusted material only.
    let deps = s
        .query("dependents", &json!({"asset": NORMAL_TEXTURE}))
        .unwrap();
    assert_eq!(
        deps,
        json!([{
            "document": RUSTED_ID,
            "path": "materials/rusted.material.json",
            "pointer": "/normal_texture",
            "asset": NORMAL_TEXTURE,
        }])
    );
    // The rig source is referenced by the Rig scene's instance, by the
    // rig document's skeleton and (through its `anim/*` subs) its clips.
    let rig_deps = s
        .query(
            "dependents",
            &json!({"asset": "c0000000-0000-4000-8000-000000000005"}),
        )
        .unwrap();
    let paths: Vec<&str> = rig_deps
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"rigs/column.rig.json"), "{paths:?}");
    assert!(paths.contains(&"scenes/rig.scene.json"), "{paths:?}");

    let unknown = s.query("nope", &json!({})).unwrap_err();
    assert_eq!(code_of(&unknown), ["FYN-T0004"]);
    assert!(unknown[0]
        .suggestion
        .as_deref()
        .unwrap()
        .contains("validateProject"));
}

#[test]
fn project_validation_is_the_cli_report() {
    let copy = Copy::new("validate");
    let s = open(&copy);
    let report = s.validate_project().unwrap();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.documents, 11);
    assert_eq!(report.sources, 6);
    let v = s.query("validateProject", &json!({})).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["documents"], 11);
    assert_eq!(v["project"]["name"], "Fixture Game");
}

#[test]
fn a_scene_edits_undoes_to_its_bytes_saves_and_conflicts() {
    let copy = Copy::new("scene");
    let mut s = open(&copy);
    let original = copy.read(RIG_SCENE);

    // By relative path, by absolute path and by id: one handle.
    let h = s.open_document(RIG_SCENE).unwrap();
    assert_eq!(h, 1);
    assert_eq!(
        s.open_document(copy.path(RIG_SCENE).to_str().unwrap())
            .unwrap(),
        h
    );
    assert_eq!(
        s.open_document("c0000000-0000-4000-8000-000000000034")
            .unwrap(),
        h
    );
    assert_eq!(s.handles(), vec![h]);
    assert_eq!(s.handle_of(&copy.path(RIG_SCENE)), Some(h));

    let state = s.state_json(h).unwrap();
    assert_eq!(state["kind"], "scene");
    assert_eq!(state["name"], "Rig");
    assert_eq!(state["relativePath"], "scenes/rig.scene.json");
    assert_eq!(state["dirty"], false);
    assert_eq!(state["canUndo"], false);
    let revision = state["revision"].as_str().unwrap().to_string();
    assert_eq!(revision.len(), 64);

    let doc = s.document_json(h).unwrap();
    assert_eq!(doc["root"]["name"], "Rig");
    // 9 authored entities; the Imported instance expands to 15 at runtime.
    assert_eq!(doc["root"]["entities"].as_object().unwrap().len(), 9);
    let inspect = s.inspect_json(h).unwrap();
    assert_eq!(inspect["entityCount"], 9);
    assert_eq!(inspect["diagnostics"], json!([]));

    // Rename the camera, then duplicate the rig: two transactions.
    let camera = "c0000000-0000-4000-8000-0000000004e0";
    let applied = s
        .apply_json(
            h,
            &json!({
                "baseRevision": revision,
                "commands": [{ "command": "renameObject", "id": camera, "name": "Eye" }],
            }),
        )
        .unwrap();
    assert_eq!(applied["ok"], true, "{applied}");
    assert_eq!(applied["state"]["dirty"], true);
    assert_eq!(applied["state"]["canUndo"], true);
    let rev2 = applied["revision"].as_str().unwrap().to_string();
    assert_ne!(rev2, revision);
    assert_eq!(
        s.document_json(h).unwrap()["root"]["entities"][camera]["name"],
        "Eye"
    );
    // The `ids` form of the `document` query: the named entities only (null
    // for one the scene lacks) and the ordered root ids — what the Hierarchy
    // re-reads from a transaction's `affectedIds`.
    let part = s
        .query(
            "document",
            &json!({"handle": h, "ids": [camera, "not-here"]}),
        )
        .unwrap();
    assert_eq!(part["entities"][camera]["name"], "Eye");
    assert_eq!(part["entities"]["not-here"], Value::Null);
    assert_eq!(part["entities"].as_object().unwrap().len(), 2);
    assert!(part.get("root").is_none(), "the whole document is not sent");
    assert_eq!(part["revision"], rev2);
    let roots = part["roots"].as_array().unwrap();
    assert_eq!(roots[0], camera, "the camera is the first root in the file");
    assert_eq!(
        roots,
        s.document_json(h).unwrap()["roots"].as_array().unwrap()
    );
    assert!(roots.iter().all(|r| {
        s.document_json(h).unwrap()["root"]["entities"][r.as_str().unwrap()]["parent"].is_null()
    }));
    let bad = s
        .query("document", &json!({"handle": h, "ids": "camera"}))
        .unwrap_err();
    assert_eq!(bad[0].code, Code::SESSION_INVALID_REQUEST);

    let column = "c0000000-0000-4000-8000-0000000004e2";
    let dup = s
        .apply_json(
            h,
            &json!([{ "command": "duplicateObject", "id": column, "name": "Column 2" }]),
        )
        .unwrap();
    assert_eq!(dup["ok"], true, "{dup}");
    let copy_root = dup["mutations"][0]["affectedIds"][0].as_str().unwrap();
    assert_eq!(
        s.document_json(h).unwrap()["root"]["entities"][copy_root]["name"],
        "Column 2"
    );
    assert_eq!(s.state_json(h).unwrap()["history"], 2);

    // A stale base revision is a conflict and changes nothing.
    let stale = s
        .apply_json(
            h,
            &json!({
                "baseRevision": revision,
                "commands": [{ "command": "renameObject", "id": camera, "name": "Nope" }],
            }),
        )
        .unwrap();
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["diagnostics"][0]["code"], "FYN-E0001");
    assert_eq!(s.state_json(h).unwrap()["history"], 2);

    // A malformed request is the parser's E0004, before any change.
    let bad = s
        .apply_json(h, &json!({"commands": [{ "command": "fly" }]}))
        .unwrap_err();
    assert_eq!(code_of(&bad), ["FYN-E0004"]);

    // Undo both → the original bytes; redo → the copy is back.
    let t = s.undo(h).unwrap();
    let tj = s.transaction_json(h, &t).unwrap();
    assert_eq!(tj["commands"], json!(["duplicateObject"]));
    assert_eq!(tj["state"]["canRedo"], true);
    s.undo(h).unwrap();
    assert_eq!(
        s.document(h).unwrap().editor.document().write().unwrap(),
        original
    );
    assert_eq!(s.state_json(h).unwrap()["dirty"], false);
    assert_eq!(s.state_json(h).unwrap()["revision"], revision);
    let nothing = s.undo(h).unwrap_err();
    assert_eq!(nothing.code, Code::EDIT_NO_HISTORY);
    s.redo(h).unwrap();
    s.redo(h).unwrap();
    assert_eq!(s.state_json(h).unwrap()["revision"], dup["revision"]);
    assert_eq!(
        s.document_json(h).unwrap()["root"]["entities"][copy_root]["name"],
        "Column 2"
    );

    // Save in place: the file has the copy, the CLI's inspect agrees, the
    // database still knows the scene.
    let written = s.save(h, None).unwrap();
    assert_eq!(written, copy.path(RIG_SCENE));
    assert_eq!(s.state_json(h).unwrap()["dirty"], false);
    let on_disk = fynkel_document::Document::read(&written, None).unwrap();
    assert_eq!(
        on_disk
            .root
            .pointer(&format!("/entities/{copy_root}/name"))
            .unwrap()
            .as_str(),
        Some("Column 2")
    );
    let scene = fynkel_document::Scene::from_document(on_disk);
    let in_memory = s.document_json(h).unwrap()["root"]["entities"]
        .as_object()
        .unwrap()
        .len();
    assert!(in_memory > 9, "{in_memory}");
    assert_eq!(scene.entities.len(), in_memory);
    assert_eq!(s.project().unwrap().assets.len(), 17);

    // Save-as: a new file the database picks up (17 assets) with the same id
    // is a duplicate — the scan says so and keeps the first path.
    let other = copy.path("assets/scenes/rig_copy.scene.json");
    let written = s.save(h, Some(&other)).unwrap();
    assert_eq!(written, other);
    assert_eq!(
        s.state_json(h).unwrap()["path"],
        other.display().to_string()
    );
    let p = s.project().unwrap();
    assert_eq!(p.assets.len(), 17, "a duplicate id is not a second asset");
    assert!(
        code_of(&p.diagnostics).contains(&"FYN-A0003"),
        "{:?}",
        code_of(&p.diagnostics)
    );

    // Validate the open document: clean; the reference check runs too.
    let v = s.query("validate", &json!({"handle": h})).unwrap();
    assert_eq!(v["ok"], true, "{v}");

    // Reload drops the history and reads the file again.
    s.apply_json(
        h,
        &json!([{ "command": "renameObject", "id": camera, "name": "Gone" }]),
    )
    .unwrap();
    assert_eq!(s.state_json(h).unwrap()["dirty"], true);
    s.reload(h).unwrap();
    let st = s.state_json(h).unwrap();
    assert_eq!(st["dirty"], false);
    assert_eq!(st["canUndo"], false);
    assert_eq!(
        s.document_json(h).unwrap()["root"]["entities"][camera]["name"],
        "Eye"
    );

    // Close; the handle is gone and never reused.
    s.close_document(h).unwrap();
    assert_eq!(code_of(&[s.close_document(h).unwrap_err()]), ["FYN-T0002"]);
    assert_eq!(code_of(&[s.state_json(h).unwrap_err()]), ["FYN-T0002"]);
    let again = s.open_document(RIG_SCENE).unwrap();
    assert_eq!(again, 2);
    assert_eq!(s.documents_json().len(), 1);
    let docs = s.query("documents", &json!({})).unwrap();
    assert_eq!(docs[0]["handle"], 2);
    let no_handle = s.query("document", &json!({})).unwrap_err();
    assert_eq!(code_of(&no_handle), ["FYN-T0003"]);
    let state_only = s
        .query("document", &json!({"handle": 2, "root": false}))
        .unwrap();
    assert!(state_only.get("root").is_none());
}

#[test]
fn a_material_edits_through_document_fields_and_dependents_follow_a_save() {
    let copy = Copy::new("material");
    let mut s = open(&copy);
    let original = copy.read(RUSTED);
    let h = s.open_document(RUSTED_ID).unwrap();
    assert_eq!(s.state_json(h).unwrap()["kind"], "material");
    let inspect = s.inspect_json(h).unwrap();
    assert_eq!(inspect["shader"], "fynkel.pbr@1");
    assert_eq!(inspect["fields"]["metallic"], 0.25);

    let applied = s
        .apply_json(
            h,
            &json!([
                { "command": "setDocumentField", "pointer": "/base_color", "value": [0.2, 0.3, 0.4, 1.0] },
                { "command": "unsetDocumentField", "pointer": "/normal_texture" },
            ]),
        )
        .unwrap();
    assert_eq!(applied["ok"], true, "{applied}");
    assert_eq!(
        s.document_json(h).unwrap()["root"]["base_color"],
        json!([0.2, 0.3, 0.4, 1.0])
    );
    // An entity command on a material is refused, nothing applied.
    let refused = s
        .apply_json(
            h,
            &json!([{ "command": "renameObject", "id": RUSTED_ID, "name": "x" }]),
        )
        .unwrap();
    assert_eq!(refused["ok"], false);
    assert_eq!(
        refused["mutations"][0]["diagnostics"][0]["code"],
        "FYN-E0007"
    );
    // A value the schema rejects rolls back.
    let rejected = s
        .apply_json(
            h,
            &json!([{ "command": "setDocumentField", "pointer": "/metallic", "value": "shiny" }]),
        )
        .unwrap();
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["diagnostics"][0]["code"], "FYN-E0005");

    // Before the save the database still sees the normal map used.
    let normal = uuid::Uuid::parse_str(NORMAL_TEXTURE).unwrap();
    assert_eq!(s.project().unwrap().dependents(normal).len(), 1);
    s.save(h, None).unwrap();
    assert_eq!(s.project().unwrap().dependents(normal).len(), 0);
    assert!(copy.read(RUSTED).contains("0.2"));

    // Undo back to the original bytes and save again: dependents return.
    s.undo(h).unwrap();
    assert_eq!(
        s.document(h).unwrap().editor.document().write().unwrap(),
        original
    );
    s.save(h, None).unwrap();
    assert_eq!(copy.read(RUSTED), original);
    assert_eq!(s.project().unwrap().dependents(normal).len(), 1);
}

#[test]
fn other_kinds_and_bad_paths_are_refused_and_nothing_works_without_a_project() {
    let copy = Copy::new("refused");
    let mut s = AuthoringSession::new();
    let no_project = s.open_document(RIG_SCENE).unwrap_err();
    assert_eq!(code_of(&no_project), ["FYN-T0001"]);
    assert_eq!(
        code_of(&s.query("project", &json!({})).unwrap_err()),
        ["FYN-T0001"]
    );
    assert_eq!(s.query("documents", &json!({})).unwrap(), json!([]));
    assert_eq!(code_of(&[s.validate_project().unwrap_err()]), ["FYN-T0001"]);
    assert_eq!(code_of(&[s.rescan(&[]).unwrap_err()]), ["FYN-T0001"]);

    let not_a_project = s
        .open_project(&copy.path("assets"))
        .map(|_| ())
        .unwrap_err();
    assert_eq!(code_of(&not_a_project), ["FYN-P0001"]);
    let no_dir = s
        .open_project(&copy.path("nowhere"))
        .map(|_| ())
        .unwrap_err();
    assert_eq!(code_of(&no_dir), ["FYN-T0006"]);

    s.open_project(&copy.dir).unwrap();
    let rig = s.open_document("assets/rigs/column.rig.json").unwrap_err();
    assert_eq!(code_of(&rig), ["FYN-E0007"]);
    let project_file = s.open_document("fynkel.project.json").unwrap_err();
    assert_eq!(code_of(&project_file), ["FYN-E0007"]);
    let missing = s
        .open_document("assets/scenes/none.scene.json")
        .unwrap_err();
    assert_eq!(code_of(&missing), ["FYN-T0006"]);
    let unknown_id = s
        .open_document("c0000000-0000-4000-8000-0000000000ff")
        .unwrap_err();
    assert_eq!(code_of(&unknown_id), ["FYN-T0006"]);
    let sidecar = s
        .open_document("assets/models/rig.glb.meta.json")
        .unwrap_err();
    assert_eq!(code_of(&sidecar), ["FYN-E0007"]);
    let source = s.open_document("assets/models/rig.glb").unwrap_err();
    assert_eq!(
        code_of(&source),
        ["FYN-D0015"],
        "not a document: unknown kind from the file name"
    );

    // `revision`: the file on disk, whether or not it is open — the same
    // hash the open editor reports until it is edited.
    let h = s.open_document(RIG_SCENE).unwrap();
    let on_disk = s.query("revision", &json!({"path": RIG_SCENE})).unwrap();
    assert_eq!(on_disk["revision"], s.state_json(h).unwrap()["revision"]);
    assert_eq!(on_disk["kind"], "scene");
    assert_eq!(on_disk["name"], "Rig");
    assert_eq!(on_disk["formatVersion"], 4);
    assert_eq!(
        on_disk["path"],
        copy.path(RIG_SCENE)
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );
    s.apply_json(
        h,
        &json!([{ "command": "renameObject", "id": "c0000000-0000-4000-8000-0000000004e0", "name": "Eye" }]),
    )
    .unwrap();
    assert_eq!(
        s.query("revision", &json!({"path": RIG_SCENE})).unwrap()["revision"],
        on_disk["revision"],
        "an unsaved edit does not move the file"
    );
    assert_ne!(s.state_json(h).unwrap()["revision"], on_disk["revision"]);
    let gone = s
        .query(
            "revision",
            &json!({"path": "assets/scenes/none.scene.json"}),
        )
        .unwrap_err();
    assert_eq!(code_of(&gone), ["FYN-D0900"]);
    assert_eq!(
        code_of(&s.query("revision", &json!({})).unwrap_err()),
        ["FYN-T0003"]
    );

    // Closing the project closes its documents.
    s.close_project();
    assert_eq!(code_of(&[s.state_json(h).unwrap_err()]), ["FYN-T0001"]);
    assert!(s.project().is_none());
}

#[test]
fn a_lock_from_a_newer_fynkel_opens_the_project_read_only() {
    let copy = Copy::new("readonly");
    let lock = copy
        .read("fynkel.lock.json")
        .replace("\"version\": \"0.0.1\"", "\"version\": \"9.1.0\"");
    assert_ne!(lock, copy.read("fynkel.lock.json"));
    std::fs::write(copy.path("fynkel.lock.json"), lock).unwrap();
    let mut s = open(&copy);
    let reason = s.project().unwrap().read_only.clone().expect("read-only");
    assert!(reason.contains("9.1.0"), "{reason}");
    // The mismatch is reported too, as the CLI would.
    assert!(code_of(&s.project().unwrap().diagnostics).contains(&"FYN-P0011"));

    // Documents open and read; every mutation is refused with the reason.
    let h = s.open_document(RIG_SCENE).unwrap();
    assert_eq!(s.state_json(h).unwrap()["readOnly"], reason);
    let refused = s
        .apply(
            h,
            Request::new(vec![fynkel_document::authoring::Command::RenameObject {
                id: uuid::Uuid::parse_str("c0000000-0000-4000-8000-0000000004e0").unwrap(),
                name: "Eye".into(),
            }]),
        )
        .unwrap_err();
    assert_eq!(refused.code, Code::EDIT_NOT_EDITABLE);
    assert!(refused.message.contains("read-only"));
    assert_eq!(s.save(h, None).unwrap_err().code, Code::EDIT_NOT_EDITABLE);
    assert_eq!(s.undo(h).unwrap_err().code, Code::EDIT_NOT_EDITABLE);
    assert_eq!(copy.read(RIG_SCENE), fixture_text(RIG_SCENE));

    // An older lock is a mismatch, not read-only.
    let older = copy.read("fynkel.lock.json").replace("9.1.0", "0.0.0");
    std::fs::write(copy.path("fynkel.lock.json"), older).unwrap();
    s.open_project(&copy.dir).unwrap();
    assert!(s.project().unwrap().read_only.is_none());
    assert!(code_of(&s.project().unwrap().diagnostics).contains(&"FYN-P0011"));
}

fn fixture_text(rel: &str) -> String {
    std::fs::read_to_string(fixture().join(rel)).unwrap()
}

#[test]
fn version_order() {
    use fynkel_authoring::project::version_newer;
    assert!(version_newer("0.0.2", "0.0.1"));
    assert!(version_newer("1.0.0", "0.9.9"));
    assert!(version_newer("0.1", "0.0.9"));
    assert!(!version_newer("0.0.1", "0.0.1"));
    assert!(!version_newer("0.0.1", "0.0.1.0"));
    assert!(!version_newer("0.0.0", "0.0.1"));
    assert!(version_newer("0.0.1-beta", "0.0.1"));
}

#[test]
fn rescan_reads_new_documents_and_drops_removed_ones() {
    let copy = Copy::new("rescan");
    let mut s = open(&copy);
    // A new material referencing the swatch appears after a rescan of
    // its path only.
    let swatch = "c0000000-0000-4000-8000-000000000001";
    let text = copy
        .read(RUSTED)
        .replace(RUSTED_ID, "c0000000-0000-4000-8000-0000000000aa")
        .replace(NORMAL_TEXTURE, swatch);
    let new_path = copy.path("assets/materials/second.material.json");
    std::fs::write(&new_path, text).unwrap();
    let before = s
        .project()
        .unwrap()
        .dependents(uuid::Uuid::parse_str(swatch).unwrap())
        .len();
    let scan = s.rescan(std::slice::from_ref(&new_path)).unwrap();
    assert!(!scan.iter().any(|d| d.is_error()), "{scan:?}");
    let p = s.project().unwrap();
    assert_eq!(p.assets.len(), 18);
    assert_eq!(
        p.dependents(uuid::Uuid::parse_str(swatch).unwrap()).len(),
        before + 1
    );
    // Removing the file and rescanning its path drops its references.
    std::fs::remove_file(&new_path).unwrap();
    s.rescan(&[PathBuf::from("assets/materials/second.material.json")])
        .unwrap();
    let p = s.project().unwrap();
    assert_eq!(p.assets.len(), 17);
    assert_eq!(
        p.dependents(uuid::Uuid::parse_str(swatch).unwrap()).len(),
        before
    );
    let _: Handle = 0;
}

#[test]
fn a_source_without_a_sidecar_gets_one_on_the_next_scan() {
    let copy = Copy::new("sidecars");
    // One file there before the project opens, as a checkout or an
    // unzip leaves it.
    let early = copy.path("assets/textures/early.png");
    std::fs::copy(copy.path("assets/textures/swatch.png"), &early).unwrap();
    let mut s = open(&copy);
    let p = s.project().unwrap();
    let early_sidecar = copy.path("assets/textures/early.png.meta.json");
    assert_eq!(p.sidecars_written, vec![early_sidecar.clone()]);
    assert!(p.assets.by_path("textures/early.png").is_some());
    assert!(!code_of(&p.diagnostics).contains(&"FYN-A0001"));
    let before = p.assets.len();

    // Dropped in from Finder: a texture, and a folder with a sound and a
    // file no importer knows. The watcher names the file and the folder.
    let png = copy.path("assets/textures/dropped.png");
    std::fs::copy(copy.path("assets/textures/swatch.png"), &png).unwrap();
    let folder = copy.path("assets/drop");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("notes.txt"), "hello").unwrap();
    let scan = s.rescan(&[png.clone(), folder.clone()]).unwrap();
    assert!(!scan.iter().any(|d| d.is_error()), "{scan:?}");
    let p = s.project().unwrap();
    let mut written = p.sidecars_written.clone();
    written.sort();
    assert_eq!(
        written,
        vec![
            folder.join("notes.txt.meta.json"),
            copy.path("assets/textures/dropped.png.meta.json"),
        ]
    );
    assert_eq!(p.assets.len(), before + 2);
    let meta = |rel: &str| {
        fynkel_document::asset::Meta::from_document(
            fynkel_document::Document::read(&copy.path(rel), None).unwrap(),
        )
    };
    assert_eq!(
        meta("assets/textures/dropped.png.meta.json").importer,
        "fynkel.image"
    );
    assert_eq!(
        meta("assets/drop/notes.txt.meta.json").importer,
        "fynkel.file"
    );
    let dropped = p.assets.by_path("textures/dropped.png").unwrap();
    assert_eq!(
        dropped.id,
        meta("assets/textures/dropped.png.meta.json").id()
    );
    assert_ne!(
        dropped.id,
        p.assets.by_path("textures/early.png").unwrap().id
    );

    // A second scan writes nothing; an existing sidecar is never replaced.
    s.rescan(std::slice::from_ref(&png)).unwrap();
    assert!(s.project().unwrap().sidecars_written.is_empty());

    // A read-only project (a newer build's lock) is not written to.
    let lock = copy
        .read("fynkel.lock.json")
        .replace("\"version\": \"0.0.1\"", "\"version\": \"9.1.0\"");
    std::fs::write(copy.path("fynkel.lock.json"), lock).unwrap();
    let late = copy.path("assets/textures/late.png");
    std::fs::copy(copy.path("assets/textures/swatch.png"), &late).unwrap();
    s.open_project(&copy.dir).unwrap();
    let p = s.project().unwrap();
    assert!(p.read_only.is_some());
    assert!(p.sidecars_written.is_empty());
    assert!(!copy.path("assets/textures/late.png.meta.json").exists());
    assert!(code_of(&p.diagnostics).contains(&"FYN-A0001"));
}

const MAIN_SCENE: &str = "assets/scenes/main.scene.json";

#[test]
fn a_transaction_that_writes_resolves_paths_against_the_project_and_rescans() {
    let copy = Copy::new("writes");
    let mut s = open(&copy);
    let before = s.project().unwrap().assets.len();
    let h = s.open_document(MAIN_SCENE).unwrap();
    // The editor's process may run from anywhere: a relative path is the
    // project's, not the working directory's.
    let applied = s
        .apply_json(
            h,
            &json!([
                { "command": "renameObject", "id": "c0000000-0000-4000-8000-0000000000e1", "name": "Bot" },
                { "command": "createMaterial", "name": "Gold", "path": "assets/materials/gold.material.json" },
                { "command": "saveDocument" },
            ]),
        )
        .unwrap();
    assert_eq!(applied["ok"], true, "{applied}");
    let gold = copy.dir.join("assets/materials/gold.material.json");
    assert!(gold.is_file(), "written into the project, not the cwd");
    assert!(copy.read(MAIN_SCENE).contains("\"Bot\""));
    // Both writes reached the asset database without an explicit rescan.
    let project = s.project().unwrap();
    assert_eq!(project.assets.len(), before + 1);
    let gold_id = fynkel_document::Document::read(&gold, None)
        .unwrap()
        .envelope
        .id;
    assert!(project.assets.get(gold_id).is_some());
    assert_eq!(s.state_json(h).unwrap()["dirty"], false);
    // Writing twice is refused (createMaterial never overwrites) and the
    // rename in the same transaction rolls back with it.
    let refused = s
        .apply_json(
            h,
            &json!([
                { "command": "renameObject", "id": "c0000000-0000-4000-8000-0000000000e1", "name": "Bot 2" },
                { "command": "createMaterial", "name": "Gold", "path": "assets/materials/gold.material.json" },
            ]),
        )
        .unwrap();
    assert_eq!(refused["ok"], false, "{refused}");
    let root = s.document_json(h).unwrap();
    assert_eq!(
        root["root"]["entities"]["c0000000-0000-4000-8000-0000000000e1"]["name"],
        "Bot"
    );
}

#[test]
fn the_migrations_query_previews_outdated_documents_without_writing() {
    let copy = Copy::new("migrations");
    {
        let s = open(&copy);
        let none = s.query("migrations", &json!({})).unwrap();
        assert_eq!(none["outdated"], 0);
        assert!(none["documents"].as_array().unwrap().is_empty());
        assert!(
            none["steps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|st| st["id"] == "scene/0-1/entities-keyed-by-id"),
            "{}",
            none["steps"]
        );
    }
    // A v0 scene lands in the project: the preview names it, the steps and
    // the diff; the file itself is untouched.
    let v0 = fixture()
        .parent()
        .unwrap()
        .join("migrate/v0_training.scene.json");
    let target = copy.path("assets/scenes/training.scene.json");
    std::fs::copy(&v0, &target).unwrap();
    let before = std::fs::read_to_string(&target).unwrap();
    let s = open(&copy);
    let preview = s.query("migrations", &json!({})).unwrap();
    assert_eq!(preview["outdated"], 1, "{preview}");
    let doc = &preview["documents"][0];
    assert_eq!(doc["kind"], "scene");
    assert_eq!(doc["from"], 0);
    // Scenes are at formatVersion 4 (ADR-0020): four steps, the v0 one first.
    assert_eq!(doc["to"], 4);
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["path"], target.display().to_string());
    assert_eq!(doc["applied"].as_array().unwrap().len(), 4);
    assert_eq!(doc["applied"][0]["name"], "scene/0-1/entities-keyed-by-id");
    assert_eq!(doc["applied"][1]["from"], 1);
    assert!(doc["diff"].as_str().unwrap().contains("formatVersion"));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), before);
    // Validation reports the same document as outdated (FYN-D0016).
    let report = s.validate_project().unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "FYN-D0016"),
        "{:?}",
        code_of(&report.diagnostics)
    );
}

#[test]
fn defaults_cover_every_authored_component() {
    use fynkel_document::generated::{decode_component, AUTHORED_COMPONENTS};
    let copy = Copy::new("defaults");
    let s = open(&copy);
    for ty in AUTHORED_COMPONENTS {
        let node = fynkel_authoring::component_default(ty)
            .unwrap_or_else(|| panic!("{ty} has no default — add it to component_default"));
        // The default reads back through the codec as the same component.
        decode_component(ty, &node)
            .expect("a built-in component")
            .unwrap_or_else(|e| panic!("{ty}: the default does not decode: {e:?}"));
        let v = s.query("defaults", &json!({"type": ty})).unwrap();
        assert_eq!(v["type"], *ty);
        assert_eq!(v["data"], node.to_serde());
    }
    let camera = s
        .query("defaults", &json!({"type": "fynkel.Camera"}))
        .unwrap();
    assert_eq!(camera["data"]["projection"], "perspective");
    assert!(camera["data"]["near"].as_f64().unwrap() > 0.0);
    let transform = s
        .query("defaults", &json!({"type": "fynkel.Transform"}))
        .unwrap();
    assert_eq!(transform["data"]["scale"], json!([1.0, 1.0, 1.0]));
    let behaviour = s
        .query("defaults", &json!({"type": "game.Spinner"}))
        .unwrap_err();
    assert_eq!(code_of(&behaviour), ["FYN-T0003"]);
    assert_eq!(
        code_of(&s.query("defaults", &json!({})).unwrap_err()),
        ["FYN-T0003"]
    );
}
