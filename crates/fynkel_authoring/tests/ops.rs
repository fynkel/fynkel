//! The asset operations over a copy of the fixture project (blueprint
//! §19.2e item 5): new documents, rename and move with the sidecar (the
//! id and every reference survive, `fynkel validate` stays clean), delete
//! refused while referenced and a trash move otherwise, import settings,
//! reimport, the asset history's undo/redo, the `assetDetail` and
//! `assetHistory` queries, and the incremental rescan's speed on a
//! project of 2 000 assets.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fynkel_authoring::{AuthoringSession, OPERATIONS, TRASH_DIR};
use fynkel_document::{Diagnostic, Document, DocumentKind};
use serde_json::{json, Value};
use uuid::Uuid;

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
        let dir = std::env::temp_dir().join(format!("fynkel-ops-{name}-{}", std::process::id()));
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

fn open(copy: &Copy) -> AuthoringSession {
    let mut s = AuthoringSession::new();
    s.open_project(&copy.dir)
        .map_err(|d| format!("{d:?}"))
        .unwrap();
    s
}

const ROBOT: &str = "c0000000-0000-4000-8000-000000000001";
const SWATCH: &str = "c0000000-0000-4000-8000-000000000002";
const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";
const COIN: &str = "c0000000-0000-4000-8000-000000000042";
const MAIN: &str = "c0000000-0000-4000-8000-000000000030";

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
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

/// The whole-project validation is clean — what the gate calls `fynkel validate`.
fn assert_clean(s: &AuthoringSession) {
    let report = s.validate_project().unwrap();
    let errors: Vec<_> = report.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let project = s.project().unwrap();
    let errors: Vec<_> = project
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .collect();
    assert!(errors.is_empty(), "session diagnostics: {errors:#?}");
}

#[test]
fn operations_are_listed() {
    let copy = Copy::new("list");
    let s = open(&copy);
    assert_eq!(
        s.query("operations", &json!({})).unwrap(),
        json!(OPERATIONS)
    );
    let history = s.query("assetHistory", &json!({})).unwrap();
    assert_eq!(history["canUndo"], false);
    assert_eq!(history["undoLabel"], Value::Null);
    let mut s = s;
    let d = s.operate("nonsense", &json!({})).unwrap_err();
    assert_eq!(d[0].code.as_str(), "FYN-T0004");
    let d = s.operate("undoAsset", &json!({})).unwrap_err();
    assert_eq!(d[0].code.as_str(), "FYN-E0009");
    s.close_project();
    let d = s.operate("newDocument", &json!({})).unwrap_err();
    assert_eq!(d[0].code.as_str(), "FYN-T0001");
}

#[test]
fn new_documents_are_valid_named_after_their_file_and_undo_removes_them() {
    let copy = Copy::new("new");
    let mut s = open(&copy);
    let before = s.project().unwrap().assets.len();

    // The suffix is added, the name comes from the file, the folder is made.
    let v = ok(s.operate(
        "newDocument",
        &json!({ "kind": "scene", "path": "scenes/levels/lobby" }),
    ));
    let lobby = copy.path("assets/scenes/levels/lobby.scene.json");
    assert!(lobby.is_file());
    assert_eq!(v["entry"]["name"], "lobby");
    assert_eq!(v["entry"]["document"], "scene");
    assert_eq!(v["label"], "New scene lobby.scene.json");
    let lobby_id = id(v["id"].as_str().unwrap());
    let doc = Document::read(&lobby, Some(DocumentKind::Scene)).unwrap();
    assert_eq!(doc.envelope.id, lobby_id);
    assert_eq!(doc.envelope.name, "lobby");
    assert_clean(&s);

    // A prefab with a given name; the file may be spelled with the suffix.
    let v = ok(s.operate(
        "newDocument",
        &json!({ "kind": "prefab", "path": "prefabs/gem.prefab.json", "name": "Gem" }),
    ));
    assert_eq!(v["entry"]["name"], "Gem");
    assert!(copy.path("assets/prefabs/gem.prefab.json").is_file());

    // A material: the built-in shader by default, another by name.
    let v = ok(s.operate(
        "newDocument",
        &json!({ "kind": "material", "path": "materials/gold" }),
    ));
    let gold = copy.path("assets/materials/gold.material.json");
    let gold_text = copy.read("assets/materials/gold.material.json");
    assert!(gold_text.contains("fynkel.pbr@1"), "{gold_text}");
    let gold_id = id(v["id"].as_str().unwrap());
    let detail = s
        .query("assetDetail", &json!({ "asset": gold_id.to_string() }))
        .unwrap();
    assert_eq!(detail["material"]["shader"], "fynkel.pbr@1");
    assert_eq!(detail["material"]["alphaMode"], "opaque");
    assert_eq!(s.project().unwrap().assets.len(), before + 3);
    assert_clean(&s);

    // Refusals leave nothing behind.
    refused(
        s.operate(
            "newDocument",
            &json!({ "kind": "material", "path": "materials/glass", "shader": "nobody.made@9" }),
        ),
        "FYN-T0003",
    );
    assert!(!copy.path("assets/materials/glass.material.json").exists());
    refused(
        s.operate(
            "newDocument",
            &json!({ "kind": "scene", "path": "scenes/levels/lobby" }),
        ),
        "FYN-T0003",
    );
    let d = refused(
        s.operate(
            "newDocument",
            &json!({ "kind": "scene", "path": "scenes/thing.prefab.json" }),
        ),
        "FYN-T0003",
    );
    assert!(d.message.contains("prefab document"), "{}", d.message);
    refused(
        s.operate("newDocument", &json!({ "kind": "rig", "path": "rigs/x" })),
        "FYN-T0003",
    );
    refused(
        s.operate(
            "newDocument",
            &json!({ "kind": "scene", "path": "../outside" }),
        ),
        "FYN-T0003",
    );
    refused(
        s.operate("newDocument", &json!({ "kind": "scene" })),
        "FYN-T0003",
    );
    assert_eq!(s.project().unwrap().assets.len(), before + 3);

    // Undo removes the material (unchanged since it was written); redo
    // writes the same bytes; an edited file is not removed.
    let history = s.query("assetHistory", &json!({})).unwrap();
    assert_eq!(history["undoLabel"], "New material gold.material.json");
    assert_eq!(history["undo"].as_array().unwrap().len(), 3);
    ok(s.operate("undoAsset", &json!({})));
    assert!(!gold.exists());
    assert!(s.project().unwrap().assets.get(gold_id).is_none());
    assert_eq!(
        s.query("assetHistory", &json!({})).unwrap()["redoLabel"],
        "New material gold.material.json"
    );
    ok(s.operate("redoAsset", &json!({})));
    assert_eq!(copy.read("assets/materials/gold.material.json"), gold_text);
    assert!(s.project().unwrap().assets.get(gold_id).is_some());
    std::fs::write(&gold, gold_text.replace("\"gold\"", "\"Gold Leaf\"")).unwrap();
    let d = refused(s.operate("undoAsset", &json!({})), "FYN-T0003");
    assert!(d.message.contains("edited since"), "{}", d.message);
    assert!(gold.is_file(), "an edited file stays");
    assert_eq!(
        s.query("assetHistory", &json!({})).unwrap()["undoLabel"],
        "New material gold.material.json"
    );
    assert_clean(&s);
}

#[test]
fn rename_and_move_carry_the_sidecar_so_the_id_and_every_reference_survive() {
    let copy = Copy::new("rename");
    let mut s = open(&copy);
    let dependents_before = s.project().unwrap().dependents(id(ROBOT)).len();
    assert!(dependents_before >= 5);

    // Rename a source: the sidecar follows, the id stays, references resolve.
    let v = ok(s.operate(
        "renameAsset",
        &json!({ "asset": ROBOT, "path": "models/bot.glb" }),
    ));
    assert_eq!(v["label"], "Rename robot.glb → bot.glb");
    assert!(copy.path("assets/models/bot.glb").is_file());
    assert!(copy.path("assets/models/bot.glb.meta.json").is_file());
    assert!(!copy.path("assets/models/robot.glb").exists());
    assert!(!copy.path("assets/models/robot.glb.meta.json").exists());
    let entry = s.project().unwrap().assets.get(id(ROBOT)).unwrap().clone();
    assert_eq!(entry.path, "models/bot.glb");
    assert_eq!(entry.sub_assets.len(), 8, "sub-assets kept");
    assert_eq!(
        s.project().unwrap().dependents(id(ROBOT)).len(),
        dependents_before
    );
    assert_clean(&s);

    // Move into a folder (trailing slash keeps the name); the folder is made.
    let v = ok(s.operate(
        "renameAsset",
        &json!({ "asset": ROBOT, "path": "models/props/" }),
    ));
    assert_eq!(v["label"], "Move bot.glb → models/props/");
    assert_eq!(
        s.project().unwrap().assets.get(id(ROBOT)).unwrap().path,
        "models/props/bot.glb"
    );
    assert!(copy.path("assets/models/props/bot.glb.meta.json").is_file());
    // An existing folder without the slash means the same.
    ok(s.operate("renameAsset", &json!({ "asset": ROBOT, "path": "models" })));
    assert_eq!(
        s.project().unwrap().assets.get(id(ROBOT)).unwrap().path,
        "models/bot.glb"
    );
    assert_clean(&s);

    // Refusals: another extension, an existing target, outside, the same place.
    let d = refused(
        s.operate(
            "renameAsset",
            &json!({ "asset": ROBOT, "path": "models/bot.fbx" }),
        ),
        "FYN-T0003",
    );
    assert!(d.message.contains(".glb"), "{}", d.message);
    refused(
        s.operate(
            "renameAsset",
            &json!({ "asset": ROBOT, "path": "models/rig.glb" }),
        ),
        "FYN-T0003",
    );
    refused(
        s.operate(
            "renameAsset",
            &json!({ "asset": ROBOT, "path": "../bot.glb" }),
        ),
        "FYN-T0003",
    );
    refused(
        s.operate(
            "renameAsset",
            &json!({ "asset": ROBOT, "path": "models/bot.glb" }),
        ),
        "FYN-T0003",
    );
    refused(
        s.operate(
            "renameAsset",
            &json!({ "asset": Uuid::nil().to_string(), "path": "x.glb" }),
        ),
        "FYN-A0004",
    );
    assert_eq!(
        s.project().unwrap().assets.get(id(ROBOT)).unwrap().path,
        "models/bot.glb"
    );

    // A document keeps its kind's suffix; an open document follows to the
    // new path with its history, and saves there.
    let rusted_dependents = s.project().unwrap().dependents(id(RUSTED)).len();
    assert_eq!(
        rusted_dependents, 8,
        "six documents, one of them three times"
    );
    let h = s
        .open_document("assets/materials/rusted.material.json")
        .unwrap();
    let applied = s
        .apply_json(
            h,
            &json!([{ "command": "setDocumentField", "pointer": "/double_sided", "value": true }]),
        )
        .unwrap();
    assert_eq!(applied["ok"], true, "{applied}");
    refused(
        s.operate(
            "renameAsset",
            &json!({ "asset": RUSTED, "path": "materials/rust.json" }),
        ),
        "FYN-T0003",
    );
    let v = ok(s.operate(
        "renameAsset",
        &json!({ "asset": RUSTED, "path": "materials/rust.material.json" }),
    ));
    assert_eq!(v["entry"]["path"], "materials/rust.material.json");
    let state = s.state_json(h).unwrap();
    assert_eq!(state["relativePath"], "materials/rust.material.json");
    assert_eq!(
        state["path"],
        copy.path("assets/materials/rust.material.json")
            .display()
            .to_string()
    );
    assert_eq!(state["dirty"], true, "the edit is still there");
    assert_eq!(state["canUndo"], true);
    let written = s.save(h, None).unwrap();
    assert_eq!(written, copy.path("assets/materials/rust.material.json"));
    assert!(copy
        .read("assets/materials/rust.material.json")
        .contains("\"double_sided\": true"));
    assert!(!copy.path("assets/materials/rusted.material.json").exists());
    assert_eq!(
        s.project().unwrap().dependents(id(RUSTED)).len(),
        rusted_dependents,
        "every field still points at it"
    );
    assert_clean(&s);

    // Undo walks the moves back, sidecar and open document included.
    ok(s.operate("undoAsset", &json!({}))); // rust → rusted
    assert!(copy.path("assets/materials/rusted.material.json").is_file());
    assert_eq!(
        s.state_json(h).unwrap()["relativePath"],
        "materials/rusted.material.json"
    );
    ok(s.operate("undoAsset", &json!({}))); // models/bot ← models/props/bot
    assert_eq!(
        s.project().unwrap().assets.get(id(ROBOT)).unwrap().path,
        "models/props/bot.glb"
    );
    ok(s.operate("undoAsset", &json!({})));
    ok(s.operate("undoAsset", &json!({})));
    assert_eq!(
        s.project().unwrap().assets.get(id(ROBOT)).unwrap().path,
        "models/robot.glb"
    );
    assert!(copy.path("assets/models/robot.glb.meta.json").is_file());
    assert!(!copy.path("assets/models/bot.glb").exists());
    assert_eq!(
        s.query("assetHistory", &json!({})).unwrap()["canUndo"],
        false
    );
    ok(s.operate("redoAsset", &json!({})));
    assert_eq!(
        s.project().unwrap().assets.get(id(ROBOT)).unwrap().path,
        "models/bot.glb"
    );
    assert_clean(&s);
}

#[test]
fn delete_is_refused_while_referenced_lists_the_dependents_and_trashes_otherwise() {
    let copy = Copy::new("delete");
    let mut s = open(&copy);
    let before = s.project().unwrap().assets.len();

    // Referenced: refused, every referencing field listed.
    let d = refused(
        s.operate("deleteAsset", &json!({ "asset": RUSTED })),
        "FYN-T0003",
    );
    assert!(d.message.contains("referenced by"), "{}", d.message);
    assert!(d.related.len() >= 6, "{:#?}", d.related);
    assert!(d
        .related
        .iter()
        .any(|r| r.message.contains("scenes/main.scene.json")));
    assert!(d.related.iter().all(|r| r.pointer.is_some()));
    assert!(copy.path("assets/materials/rusted.material.json").is_file());

    // Unreferenced: moved to the trash, the entry gone, validation clean.
    let v = ok(s.operate("deleteAsset", &json!({ "asset": COIN })));
    assert_eq!(v["label"], "Delete coin.prefab.json");
    let trash = copy.dir.join(TRASH_DIR).join(COIN).join("coin.prefab.json");
    assert_eq!(v["trash"], trash.display().to_string());
    assert!(trash.is_file());
    assert!(!copy.path("assets/prefabs/coin.prefab.json").exists());
    assert!(s.project().unwrap().assets.get(id(COIN)).is_none());
    assert_eq!(s.project().unwrap().assets.len(), before - 1);
    assert_clean(&s);
    ok(s.operate("undoAsset", &json!({})));
    assert!(copy.path("assets/prefabs/coin.prefab.json").is_file());
    assert_eq!(
        s.project().unwrap().assets.get(id(COIN)).unwrap().name,
        "Coin"
    );
    ok(s.operate("redoAsset", &json!({})));
    assert!(s.project().unwrap().assets.get(id(COIN)).is_none());
    ok(s.operate("undoAsset", &json!({})));

    // Open as a document: refused.
    let h = s.open_document("assets/prefabs/coin.prefab.json").unwrap();
    let d = refused(
        s.operate("deleteAsset", &json!({ "asset": COIN })),
        "FYN-T0003",
    );
    assert!(d.message.contains("open as a document"), "{}", d.message);
    s.close_document(h).unwrap();

    // Forced: the source and its sidecar go, the dependents are warnings,
    // the project now has dangling references — until the undo.
    let v = ok(s.operate("deleteAsset", &json!({ "asset": SWATCH, "force": true })));
    assert!(!copy.path("assets/textures/swatch.png").exists());
    assert!(!copy.path("assets/textures/swatch.png.meta.json").exists());
    let trash_dir = copy.dir.join(TRASH_DIR).join(SWATCH);
    assert!(trash_dir.join("swatch.png").is_file());
    assert!(trash_dir.join("swatch.png.meta.json").is_file());
    let warnings = v["diagnostics"].as_array().unwrap();
    assert!(!warnings.is_empty());
    assert!(
        warnings.iter().all(|w| w["severity"] == "warning"),
        "{warnings:#?}"
    );
    let report = s.validate_project().unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "FYN-A0004"),
        "{:#?}",
        report.diagnostics
    );
    ok(s.operate("undoAsset", &json!({})));
    assert!(copy.path("assets/textures/swatch.png.meta.json").is_file());
    assert_eq!(
        s.project().unwrap().assets.get(id(SWATCH)).unwrap().path,
        "textures/swatch.png"
    );
    assert_clean(&s);
}

#[test]
fn import_settings_are_written_to_the_sidecar_and_reimport_checks_them() {
    let copy = Copy::new("settings");
    let mut s = open(&copy);
    let sidecar = "assets/textures/swatch.png.meta.json";
    let original = copy.read(sidecar);

    let v = ok(s.operate(
        "setImportSettings",
        &json!({ "asset": SWATCH, "settings": { "colorSpace": "linear" } }),
    ));
    assert_eq!(v["label"], "Import settings of swatch.png");
    let text = copy.read(sidecar);
    assert!(text.contains("\"colorSpace\": \"linear\""), "{text}");
    assert!(text.contains(SWATCH), "the id stays");
    let detail = s.query("assetDetail", &json!({ "asset": SWATCH })).unwrap();
    assert_eq!(detail["sidecar"]["settings"]["colorSpace"], "linear");
    assert_eq!(detail["sidecar"]["importer"], "fynkel.image");
    assert_eq!(detail["dependents"].as_array().unwrap().len(), 1);
    assert!(detail["file"]["size"].as_u64().unwrap() > 0);
    assert_clean(&s);

    // Not an object, not a source: refused, the sidecar untouched.
    refused(
        s.operate(
            "setImportSettings",
            &json!({ "asset": SWATCH, "settings": 3 }),
        ),
        "FYN-T0003",
    );
    let d = refused(
        s.operate(
            "setImportSettings",
            &json!({ "asset": RUSTED, "settings": {} }),
        ),
        "FYN-T0003",
    );
    assert!(d.message.contains("material document"), "{}", d.message);
    assert_eq!(copy.read(sidecar), text);

    // Reimport runs the importer with the settings: a bad value is the
    // importer's refusal (ok: false), the sidecar is left alone.
    ok(s.operate(
        "setImportSettings",
        &json!({ "asset": SWATCH, "settings": { "colorSpace": "purple" } }),
    ));
    let v = s.operate("reimport", &json!({ "asset": SWATCH })).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert!(!v["diagnostics"].as_array().unwrap().is_empty());
    ok(s.operate("undoAsset", &json!({})));
    assert_eq!(
        s.query("assetDetail", &json!({ "asset": SWATCH })).unwrap()["sidecar"]["settings"]
            ["colorSpace"],
        "linear"
    );
    let v = ok(s.operate("reimport", &json!({ "asset": SWATCH })));
    assert_eq!(v["sidecarWritten"], false);
    let textures = v["summary"]["textures"].as_array().unwrap();
    assert_eq!(textures.len(), 1, "{v}");
    assert_eq!(
        textures[0]["colorSpace"], "linear",
        "the setting reached the importer"
    );

    // Undo restores the original settings byte for byte.
    ok(s.operate("undoAsset", &json!({})));
    assert_eq!(copy.read(sidecar), original);
    ok(s.operate("redoAsset", &json!({})));
    assert!(copy.read(sidecar).contains("linear"));

    // A model's reimport reports its sub-assets; a document has no importer.
    let v = ok(s.operate("reimport", &json!({ "asset": ROBOT })));
    assert_eq!(
        v["sidecarWritten"], false,
        "the fixture's sidecar is current"
    );
    assert!(v["subAssets"]["prefab"].is_string());
    assert!(
        !v["summary"]["meshes"].as_array().unwrap().is_empty(),
        "{v}"
    );
    refused(
        s.operate("reimport", &json!({ "asset": MAIN })),
        "FYN-T0003",
    );
    assert_clean(&s);
}

#[test]
fn asset_detail_describes_every_kind() {
    let copy = Copy::new("detail");
    let s = open(&copy);
    let robot = s.query("assetDetail", &json!({ "asset": ROBOT })).unwrap();
    assert_eq!(robot["path"], "models/robot.glb");
    assert_eq!(robot["sidecar"]["importer"], "fynkel.gltf");
    assert_eq!(robot["sidecar"]["importerVersion"], 1);
    assert!(robot["sidecar"]["subAssets"]["prefab"].is_string());
    assert_eq!(
        robot["dependents"].as_array().unwrap().len(),
        s.project().unwrap().dependents(id(ROBOT)).len()
    );
    assert!(robot["file"]["modified"].is_number());

    let rusted = s.query("assetDetail", &json!({ "asset": RUSTED })).unwrap();
    assert_eq!(rusted["material"]["shader"], "fynkel.pbr@1");
    assert!(
        rusted["material"]["fields"]["base_color"].is_array(),
        "{rusted}"
    );
    assert!(rusted["material"]["doubleSided"].is_boolean());

    let main = s.query("assetDetail", &json!({ "asset": MAIN })).unwrap();
    assert!(main["entities"].as_u64().unwrap() > 0);
    assert_eq!(main["dependents"].as_array().unwrap().len(), 0);

    let rig = s
        .query(
            "assetDetail",
            &json!({ "asset": "c0000000-0000-4000-8000-000000000050" }),
        )
        .unwrap();
    assert!(
        rig["fields"]["clips"].is_object() || rig["fields"]["clips"].is_array(),
        "{rig}"
    );
    assert!(
        rig["subAssets"].is_object(),
        "a rig's derived sub-assets are listed: {rig}"
    );

    let d = s
        .query("assetDetail", &json!({ "asset": Uuid::nil().to_string() }))
        .unwrap_err();
    assert_eq!(d[0].code.as_str(), "FYN-A0004");
    let d = s.query("assetDetail", &json!({})).unwrap_err();
    assert_eq!(d[0].code.as_str(), "FYN-T0003");
}

#[test]
fn a_read_only_project_refuses_every_operation() {
    let copy = Copy::new("readonly");
    let lock = copy.path("fynkel.lock.json");
    let text = copy
        .read("fynkel.lock.json")
        .replace("\"version\": \"0.0.1\"", "\"version\": \"9.1.0\"");
    assert_ne!(text, copy.read("fynkel.lock.json"));
    std::fs::write(&lock, text).unwrap();
    let mut s = open(&copy);
    assert!(
        s.project().unwrap().read_only.is_some(),
        "{:?}",
        s.project().unwrap().read_only
    );
    let d = s
        .operate(
            "newDocument",
            &json!({ "kind": "scene", "path": "scenes/x" }),
        )
        .unwrap_err();
    assert_eq!(d[0].code.as_str(), "FYN-E0007");
    assert!(!copy.path("assets/scenes/x.scene.json").exists());
}

/// A project of 2 000 assets: one change is re-read in well under the
/// gate's 50 ms, where a full scan takes tens of milliseconds even in a
/// release build (~90 ms debug).
#[test]
fn an_incremental_rescan_of_two_thousand_assets_stays_under_fifty_milliseconds() {
    let copy = Copy::new("large");
    let materials = copy.path("assets/materials/many");
    let prefabs = copy.path("assets/prefabs/many");
    std::fs::create_dir_all(&materials).unwrap();
    std::fs::create_dir_all(&prefabs).unwrap();
    for i in 0..1000 {
        let mut m = Document::new(DocumentKind::Material, &format!("Material {i}"));
        let path = materials.join(format!("m{i:04}.material.json"));
        m.path = Some(path.display().to_string());
        std::fs::write(&path, m.write().unwrap()).unwrap();
        let mut p = Document::new(DocumentKind::Prefab, &format!("Prefab {i}"));
        let path = prefabs.join(format!("p{i:04}.prefab.json"));
        p.path = Some(path.display().to_string());
        std::fs::write(&path, p.write().unwrap()).unwrap();
    }
    let mut s = open(&copy);
    let count = s.project().unwrap().assets.len();
    assert!(count >= 2000, "{count}");

    let target = materials.join("m0500.material.json");
    let text = std::fs::read_to_string(&target).unwrap();
    let mut best = Duration::MAX;
    for round in 0..5 {
        std::fs::write(
            &target,
            text.replace("Material 500", &format!("Material {round}")),
        )
        .unwrap();
        let start = Instant::now();
        let diags = s.rescan(std::slice::from_ref(&target)).unwrap();
        best = best.min(start.elapsed());
        assert!(diags.is_empty(), "{diags:#?}");
    }
    assert_eq!(s.project().unwrap().assets.len(), count);
    assert!(
        best < Duration::from_millis(50),
        "incremental rescan took {best:?}"
    );

    // The same bound for a rename through the session (two paths re-read).
    let entry = s
        .project()
        .unwrap()
        .assets
        .by_path("materials/many/m0100.material.json")
        .unwrap()
        .clone();
    let start = Instant::now();
    ok(s.operate(
        "renameAsset",
        &json!({ "asset": entry.id.to_string(), "path": "materials/many/renamed.material.json" }),
    ));
    let took = start.elapsed();
    assert!(took < Duration::from_millis(50), "rename took {took:?}");
    assert_eq!(
        s.project().unwrap().assets.get(entry.id).unwrap().path,
        "materials/many/renamed.material.json"
    );
    assert_eq!(s.project().unwrap().assets.len(), count);
    eprintln!("incremental rescan best {best:?}, rename {took:?}, full scan of {count} assets:");
    let start = Instant::now();
    s.rescan(&[]).unwrap();
    eprintln!("  {:?}", start.elapsed());
}
