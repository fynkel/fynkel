//! The asset operations over a copy of the fixture project (blueprint
//! §19.2e item 5): new documents, rename and move with the sidecar (the
//! id and every reference survive, `fynkel validate` stays clean), delete
//! refused while referenced and a trash move otherwise, import settings,
//! reimport, the asset history's undo/redo, the `assetDetail` and
//! `assetHistory` queries, and the incremental rescan's speed on a
//! project of 2 000 assets.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fynkel_authoring::{AuthoringSession, OPERATIONS, PRIMITIVES_ID, TRASH_DIR};
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

    // A volume profile (Unity's Create → Volume Profile), its starting look.
    let v = ok(s.operate("newDocument", &json!({ "kind": "volume", "path": "look" })));
    assert_eq!(v["entry"]["document"], "volume");
    let look = Document::read(
        &copy.path("assets/look.volume.json"),
        Some(DocumentKind::Volume),
    )
    .unwrap();
    assert_eq!(look.envelope.name, "look");
    // A physics material (Unity's Create → Physics Material, ADR-0025),
    // Unity's default material spelled out.
    let v = ok(s.operate(
        "newDocument",
        &json!({ "kind": "physics_material", "path": "physics/ice" }),
    ));
    assert_eq!(v["entry"]["document"], "physics_material");
    assert_eq!(v["label"], "New physics_material ice.physicsmaterial.json");
    let ice = Document::read(
        &copy.path("assets/physics/ice.physicsmaterial.json"),
        Some(DocumentKind::PhysicsMaterial),
    )
    .unwrap();
    assert_eq!(ice.envelope.name, "ice");
    assert_eq!(
        fynkel_document::PhysicsMaterialDocument::from_document(ice).material,
        fynkel_core::physics::PhysicsMaterial::default()
    );
    let d = refused(
        s.operate(
            "newDocument",
            &json!({ "kind": "physics_material", "path": "physics/ice.material.json" }),
        ),
        "FYN-T0003",
    );
    assert!(d.message.contains("material document"), "{}", d.message);
    // A data asset (ADR-0027) names its type; its data is optional.
    let v = ok(s.operate(
        "newDocument",
        &json!({ "kind": "data", "path": "data/sword", "type": "game.Weapon", "data": { "damage": 3 } }),
    ));
    assert_eq!(v["entry"]["document"], "data");
    let sword = fynkel_document::DataDocument::from_document(
        Document::read(
            &copy.path("assets/data/sword.data.json"),
            Some(DocumentKind::Data),
        )
        .unwrap(),
    );
    assert_eq!(
        (sword.type_id.as_str(), sword.version, sword.name()),
        ("game.Weapon", 1, "sword")
    );
    assert_eq!(
        sword.data().unwrap().get("damage").and_then(|n| n.as_i64()),
        Some(3)
    );
    refused(
        s.operate(
            "newDocument",
            &json!({ "kind": "data", "path": "data/shield" }),
        ),
        "FYN-T0003",
    );
    refused(
        s.operate(
            "newDocument",
            &json!({ "kind": "data", "path": "data/shield", "type": "Shield" }),
        ),
        "FYN-T0003",
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

/// Material remapping (`docs/design/material-remapping.md`): a remap is
/// an import setting (a wrong target refused), and Extract Materials
/// writes the embedded materials as documents next to the model and
/// remaps them in one step of the asset history.
#[test]
fn remaps_are_settings_and_extract_materials_is_one_step() {
    let copy = Copy::new("remap");
    let mut s = open(&copy);
    let sidecar = "assets/models/robot.glb.meta.json";
    let original = copy.read(sidecar);

    // A remap to the project material is a setting like any other…
    ok(s.operate(
        "setImportSettings",
        &json!({ "asset": ROBOT, "settings": { "materials": { "material/Painted": { "asset": RUSTED } } } }),
    ));
    assert!(copy.read(sidecar).contains(RUSTED));
    assert_clean(&s);
    // …and one to something that is not a material document is refused.
    let d = refused(
        s.operate(
            "setImportSettings",
            &json!({ "asset": ROBOT, "settings": { "materials": { "material/Painted": { "asset": SWATCH } } } }),
        ),
        "FYN-T0003",
    );
    assert!(
        d.related.iter().any(|r| r.code.as_str() == "FYN-A0007"),
        "{d:#?}"
    );
    ok(s.operate("undoAsset", &json!({})));
    assert_eq!(copy.read(sidecar), original);

    // Extract: every embedded material becomes a document under
    // models/Materials, remapped.
    let v = ok(s.operate("extractMaterials", &json!({ "asset": ROBOT })));
    assert_eq!(v["label"], "Extract Materials of robot.glb");
    let materials = v["materials"].as_array().unwrap().clone();
    let keys: Vec<&str> = materials
        .iter()
        .map(|m| m["key"].as_str().unwrap())
        .collect();
    let embedded: Vec<String> = s
        .project()
        .unwrap()
        .assets
        .get(id(ROBOT))
        .unwrap()
        .sub_assets
        .keys()
        .filter(|k| k.starts_with("material/"))
        .cloned()
        .collect();
    assert!(!keys.is_empty());
    assert_eq!(keys.len(), embedded.len(), "{keys:?} vs {embedded:?}");
    for m in &materials {
        let path = PathBuf::from(m["path"].as_str().unwrap());
        assert_eq!(path.parent().unwrap(), copy.path("assets/models/Materials"));
        let doc = Document::read(&path, Some(DocumentKind::Material)).unwrap();
        assert_eq!(doc.envelope.id.to_string(), m["id"].as_str().unwrap());
        assert!(doc
            .root
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap()
            .starts_with("Extracted from robot.glb"));
        assert_eq!(
            v["settings"]["materials"][m["key"].as_str().unwrap()]["asset"],
            m["id"]
        );
    }
    assert_clean(&s);

    // Everything is remapped now: a second extract has nothing to do.
    refused(
        s.operate("extractMaterials", &json!({ "asset": ROBOT })),
        "FYN-T0003",
    );
    // Not a model.
    refused(
        s.operate("extractMaterials", &json!({ "asset": SWATCH })),
        "FYN-T0003",
    );

    // One undo removes the documents and restores the sidecar; redo
    // brings both back.
    ok(s.operate("undoAsset", &json!({})));
    assert_eq!(copy.read(sidecar), original);
    for m in &materials {
        assert!(!Path::new(m["path"].as_str().unwrap()).exists());
    }
    assert_clean(&s);
    ok(s.operate("redoAsset", &json!({})));
    for m in &materials {
        assert!(Path::new(m["path"].as_str().unwrap()).exists());
    }
    assert!(copy
        .read(sidecar)
        .contains(materials[0]["id"].as_str().unwrap()));
    assert_clean(&s);

    // A second model extracting into the same folder never overwrites:
    // taken names get " 2". (The robot again, its remaps cleared.)
    ok(s.operate(
        "setImportSettings",
        &json!({ "asset": ROBOT, "settings": {} }),
    ));
    let again = ok(s.operate("extractMaterials", &json!({ "asset": ROBOT })));
    for (m, first) in again["materials"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&materials)
    {
        let first = first["path"]
            .as_str()
            .unwrap()
            .trim_end_matches(".material.json");
        assert_eq!(
            m["path"].as_str().unwrap(),
            format!("{first} 2.material.json")
        );
    }
    assert_clean(&s);

    // An edited extracted material stops the undo before anything moves.
    let edited = PathBuf::from(again["materials"][0]["path"].as_str().unwrap());
    let text = std::fs::read_to_string(&edited).unwrap();
    std::fs::write(
        &edited,
        text.replace("Extracted from", "Edited, extracted from"),
    )
    .unwrap();
    let before = copy.read(sidecar);
    refused(s.operate("undoAsset", &json!({})), "FYN-T0003");
    assert_eq!(copy.read(sidecar), before);
    assert!(edited.exists());
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

/// `setStartupScene` (Unity's first scene in Build Settings): the project
/// file's `startupScene`, one undo step; a material, an unknown id and the
/// scene that is already the startup scene are refused.
#[test]
fn the_startup_scene_is_set_from_the_editor_and_undone() {
    const RIG: &str = "c0000000-0000-4000-8000-000000000034";
    let copy = Copy::new("startup");
    let mut s = open(&copy);
    let before = copy.read("fynkel.project.json");

    refused(
        s.operate("setStartupScene", &json!({ "id": RUSTED })),
        "FYN-T0003",
    );
    refused(
        s.operate(
            "setStartupScene",
            &json!({ "id": Uuid::new_v4().to_string() }),
        ),
        "FYN-T0003",
    );
    refused(
        s.operate("setStartupScene", &json!({ "id": MAIN })),
        "FYN-T0003",
    );
    assert_eq!(copy.read("fynkel.project.json"), before);

    let v = ok(s.operate("setStartupScene", &json!({ "id": RIG })));
    assert_eq!(v["label"], "Set Startup Scene Rig");
    let after = copy.read("fynkel.project.json");
    assert!(after.contains(RIG) && !after.contains(MAIN), "{after}");
    let startup = s.project().unwrap().project.startup_scene.clone().unwrap();
    assert_eq!(startup.asset.to_string(), RIG);
    assert_clean(&s);

    ok(s.operate("undoAsset", &json!({})));
    assert_eq!(copy.read("fynkel.project.json"), before);
    let startup = s.project().unwrap().project.startup_scene.clone().unwrap();
    assert_eq!(startup.asset.to_string(), MAIN);
}

/// `setModules` (Project Settings › Modules, ADR-0029 §3): the project
/// file's `modules` with only what differs from the defaults, one undo
/// step; the components of a module turned off stay and validate reports
/// them; nothing changing and a non-boolean are refused.
#[test]
fn the_modules_are_set_from_the_editor_and_undone() {
    let copy = Copy::new("modules");
    let mut s = open(&copy);
    let before = copy.read("fynkel.project.json");

    refused(s.operate("setModules", &json!({ "physics": true })), "FYN-T0003");
    refused(s.operate("setModules", &json!({ "audio": "off" })), "FYN-T0003");
    assert_eq!(copy.read("fynkel.project.json"), before);

    let v = ok(s.operate("setModules", &json!({ "animation": false, "strip": true })));
    assert_eq!(v["label"], "Set Modules (Animation Off, Strip On)");
    assert_eq!(v["modules"]["animation"], false);
    assert_eq!(v["modules"]["physics"], true);
    let after: serde_json::Value = serde_json::from_str(&copy.read("fynkel.project.json")).unwrap();
    assert_eq!(after["modules"], json!({ "animation": false, "strip": true }));
    let modules = s.project().unwrap().project.modules;
    assert!(!modules.modules.animation && modules.strip);
    assert!(!s.project().unwrap().project.allows_component("fynkel.Animator"));

    // Back on: the block goes.
    ok(s.operate("setModules", &json!({ "animation": true, "strip": false })));
    let after: serde_json::Value = serde_json::from_str(&copy.read("fynkel.project.json")).unwrap();
    assert!(after.get("modules").is_none(), "{after}");

    ok(s.operate("undoAsset", &json!({})));
    ok(s.operate("undoAsset", &json!({})));
    assert_eq!(copy.read("fynkel.project.json"), before);
    assert!(s.project().unwrap().project.modules.modules.animation);
}

#[test]
fn a_duplicate_is_a_fresh_asset_beside_its_original_and_undo_removes_it() {
    let copy = Copy::new("duplicate");
    let mut s = open(&copy);
    let before = s.project().unwrap().assets.len();

    // A prefab: `coin 1`, its own id, the name follows the file, the body kept.
    let v = ok(s.operate("duplicateAsset", &json!({ "asset": COIN })));
    let coin1 = copy.path("assets/prefabs/coin 1.prefab.json");
    assert!(coin1.is_file());
    assert_eq!(v["label"], "Duplicate coin.prefab.json");
    let coin1_id = id(v["id"].as_str().unwrap());
    assert_ne!(coin1_id, id(COIN));
    let doc = Document::read(&coin1, Some(DocumentKind::Prefab)).unwrap();
    assert_eq!(doc.envelope.id, coin1_id);
    assert_eq!(doc.envelope.name, "coin 1");
    let original = Document::read(
        &copy.path("assets/prefabs/coin.prefab.json"),
        Some(DocumentKind::Prefab),
    )
    .unwrap();
    assert_eq!(
        doc.root.get("entities").map(|n| n.to_serde()),
        original.root.get("entities").map(|n| n.to_serde())
    );
    assert_clean(&s);

    // Again from the copy: `coin 2`.
    ok(s.operate("duplicateAsset", &json!({ "asset": coin1_id.to_string() })));
    assert!(copy.path("assets/prefabs/coin 2.prefab.json").is_file());

    // A source: the bytes, a sidecar of its own with fresh sub-asset ids.
    let v = ok(s.operate("duplicateAsset", &json!({ "asset": ROBOT })));
    let robot1 = copy.path("assets/models/robot 1.glb");
    assert_eq!(
        std::fs::read(&robot1).unwrap(),
        std::fs::read(copy.path("assets/models/robot.glb")).unwrap()
    );
    let robot1_id = id(v["id"].as_str().unwrap());
    let project = s.project().unwrap();
    let a = project.assets.get(id(ROBOT)).unwrap();
    let b = project.assets.get(robot1_id).unwrap();
    assert_eq!(
        a.sub_assets.keys().collect::<Vec<_>>(),
        b.sub_assets.keys().collect::<Vec<_>>()
    );
    assert!(!a.sub_assets.is_empty());
    for (k, sub) in &b.sub_assets {
        assert_ne!(
            Some(sub),
            a.sub_assets.get(k),
            "{k} keeps the original's id"
        );
    }
    assert_eq!(project.assets.len(), before + 3);
    assert_clean(&s);

    // A path that exists is refused and leaves nothing.
    refused(
        s.operate(
            "duplicateAsset",
            &json!({ "asset": MAIN, "path": "scenes/results.scene.json" }),
        ),
        "FYN-T0003",
    );

    // Undo, undo, undo; redo brings the model back byte for byte.
    ok(s.operate("undoAsset", &json!({})));
    assert!(!robot1.exists());
    assert!(!copy.path("assets/models/robot 1.glb.meta.json").exists());
    ok(s.operate("redoAsset", &json!({})));
    assert!(robot1.is_file());
    assert!(s.project().unwrap().assets.get(robot1_id).is_some());
    for _ in 0..3 {
        ok(s.operate("undoAsset", &json!({})));
    }
    assert!(!coin1.exists());
    assert_eq!(s.project().unwrap().assets.len(), before);
    assert_clean(&s);
}

#[test]
fn install_primitives_writes_the_built_in_shapes_once_under_their_fixed_id() {
    let copy = Copy::new("primitives");
    let mut s = open(&copy);
    let before = s.project().unwrap().assets.len();
    let glb = copy.path("assets/Fynkel/Primitives.glb");

    let v = ok(s.operate("installPrimitives", &json!({})));
    assert_eq!(v["installed"], true, "{v}");
    assert_eq!(v["label"], "Install Primitives");
    assert!(glb.is_file());
    assert!(copy
        .path("assets/Fynkel/Primitives.glb.meta.json")
        .is_file());
    let entry = s
        .project()
        .unwrap()
        .assets
        .get(PRIMITIVES_ID)
        .unwrap()
        .clone();
    assert_eq!(entry.path, "Fynkel/Primitives.glb");
    // Every sub-asset id derives from the fixed id, so the editor can
    // name `mesh/Cube` without asking.
    for key in [
        "mesh/Cube",
        "mesh/Sphere",
        "mesh/Capsule",
        "mesh/Cylinder",
        "mesh/Plane",
        "mesh/Quad",
        "material/Default",
    ] {
        assert_eq!(
            entry.sub_assets.get(key),
            Some(&Uuid::new_v5(&PRIMITIVES_ID, key.as_bytes())),
            "{key}"
        );
    }
    assert_eq!(s.project().unwrap().assets.len(), before + 1);
    // It imports: six meshes on the one material.
    let v = ok(s.operate("reimport", &json!({ "asset": PRIMITIVES_ID.to_string() })));
    assert_eq!(
        v["summary"]["meshes"].as_array().map(Vec::len),
        Some(6),
        "{v}"
    );
    assert_clean(&s);

    // Again: found, nothing written, nothing on the history.
    let undo_label = v["undoLabel"].clone();
    let v = ok(s.operate("installPrimitives", &json!({})));
    assert_eq!(v["installed"], false);
    assert_eq!(v["undoLabel"], undo_label);

    // Moved elsewhere, it is still found by id.
    ok(s.operate(
        "renameAsset",
        &json!({ "asset": PRIMITIVES_ID.to_string(), "path": "models/Shapes.glb" }),
    ));
    let v = ok(s.operate("installPrimitives", &json!({})));
    assert_eq!(v["installed"], false);
    assert!(
        v["path"].as_str().unwrap().ends_with("models/Shapes.glb"),
        "{v}"
    );
    assert!(!glb.exists());
    ok(s.operate("undoAsset", &json!({})));
    assert!(glb.is_file());

    // Undo removes what it wrote; installing again puts it back.
    ok(s.operate("undoAsset", &json!({})));
    assert!(!glb.exists());
    assert!(!copy.path("assets/Fynkel/Primitives.glb.meta.json").exists());
    assert!(s.project().unwrap().assets.get(PRIMITIVES_ID).is_none());
    assert_eq!(s.project().unwrap().assets.len(), before);
    let v = ok(s.operate("installPrimitives", &json!({})));
    assert_eq!(v["installed"], true);
    assert_clean(&s);
}

/// A code-built scene names the built-in shapes without installing them
/// (ADR-0026 §4): the editor's check resolves them as the cook and `fynkel
/// validate` do, and the asset query names them.
#[test]
fn documents_resolve_the_built_in_shapes_without_installing_them() {
    let copy = Copy::new("builtin_refs");
    let scene = copy.path("assets/scenes/main.scene.json");
    let text = copy.read("assets/scenes/main.scene.json").replacen(
        "\"asset\": \"c0000000-0000-4000-8000-000000000001\",\n              \"sub\": \"mesh/Cube\"",
        &format!("\"asset\": \"{PRIMITIVES_ID}\",\n              \"sub\": \"mesh/Cube\""),
        1,
    );
    assert!(text.contains(&PRIMITIVES_ID.to_string()));
    std::fs::write(&scene, text).unwrap();
    let mut s = open(&copy);
    assert!(s.project().unwrap().assets.get(PRIMITIVES_ID).is_none());

    let doc = s.open_document(MAIN).map_err(|d| format!("{d:?}")).unwrap();
    let errors: Vec<_> = s
        .validate(doc)
        .unwrap()
        .into_iter()
        .filter(|d| d.is_error())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let v = s
        .query(
            "asset",
            &json!({ "asset": PRIMITIVES_ID.to_string(), "sub": "mesh/Cube" }),
        )
        .map_err(|d| format!("{d:?}"))
        .unwrap();
    assert!(v.to_string().contains("Primitives"), "{v}");
    assert_clean(&s);
}

/// Unity's drag in from Finder: files from outside copied into a folder,
/// a source's own sidecar kept when its id is free (else a fresh one from
/// the scan), a document's taken id replaced, a taken name counting on;
/// one asset-history step takes them all out again.
#[test]
fn import_files_copies_files_in_with_their_sidecars_and_undo_takes_them_out() {
    let copy = Copy::new("import");
    let outside = copy.path("outside");
    std::fs::create_dir_all(&outside).unwrap();
    // A texture with no sidecar; one with its own (a free id); a scene
    // whose id the project already has; a sidecar alone.
    std::fs::copy(
        copy.path("assets/textures/swatch.png"),
        outside.join("tree.png"),
    )
    .unwrap();
    std::fs::copy(
        copy.path("assets/textures/swatch.png"),
        outside.join("bark.png"),
    )
    .unwrap();
    let own = Uuid::new_v4();
    let mut meta: Value =
        serde_json::from_str(&copy.read("assets/textures/swatch.png.meta.json")).unwrap();
    meta["id"] = json!(own.to_string());
    let meta = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(outside.join("bark.png.meta.json"), meta).unwrap();
    std::fs::copy(
        copy.path("assets/scenes/main.scene.json"),
        outside.join("level.scene.json"),
    )
    .unwrap();
    std::fs::write(outside.join("lonely.png.meta.json"), "{}").unwrap();
    let mut s = open(&copy);
    let before = s.project().unwrap().assets.len();
    let paths: Vec<String> = [
        "tree.png",
        "bark.png",
        "bark.png.meta.json",
        "level.scene.json",
        "lonely.png.meta.json",
    ]
    .iter()
    .map(|n| outside.join(n).display().to_string())
    .collect();

    let v = ok(s.operate(
        "importFiles",
        &json!({ "paths": paths, "folder": "imported" }),
    ));
    assert_eq!(v["label"], "Import 3 files", "{v}");
    let imported = v["imported"].as_array().unwrap();
    assert_eq!(imported.len(), 3);
    assert!(
        copy.path("assets/imported/tree.png.meta.json").is_file(),
        "the scan wrote its sidecar"
    );
    let bark = s
        .project()
        .unwrap()
        .assets
        .get(own)
        .expect("the sidecar's id kept");
    assert_eq!(bark.path, "imported/bark.png");
    let level = Document::read(
        &copy.path("assets/imported/level.scene.json"),
        Some(DocumentKind::Scene),
    )
    .unwrap();
    let main_id = Document::read(
        &copy.path("assets/scenes/main.scene.json"),
        Some(DocumentKind::Scene),
    )
    .unwrap()
    .envelope
    .id;
    assert_ne!(level.envelope.id, main_id, "a taken id is replaced");
    assert!(!copy.path("assets/imported/lonely.png.meta.json").exists());
    assert_eq!(s.project().unwrap().assets.len(), before + 3);
    assert_clean(&s);

    // Again: the names count on.
    let v = ok(s.operate(
        "importFiles",
        &json!({ "paths": [outside.join("tree.png").display().to_string()], "folder": "imported" }),
    ));
    assert_eq!(v["label"], "Import tree 1.png");
    assert!(copy.path("assets/imported/tree 1.png").is_file());

    // Refusals write nothing.
    refused(
        s.operate(
            "importFiles",
            &json!({ "paths": [outside.display().to_string()] }),
        ),
        "FYN-T0003",
    );
    refused(
        s.operate(
            "importFiles",
            &json!({ "paths": [copy.path("assets/textures/swatch.png").display().to_string()] }),
        ),
        "FYN-T0003",
    );
    refused(
        s.operate("importFiles", &json!({ "paths": [] })),
        "FYN-T0003",
    );

    // One undo per import, sidecars and all.
    ok(s.operate("undoAsset", &json!({})));
    assert!(!copy.path("assets/imported/tree 1.png").exists());
    assert!(!copy.path("assets/imported/tree 1.png.meta.json").exists());
    ok(s.operate("undoAsset", &json!({})));
    for f in [
        "tree.png",
        "tree.png.meta.json",
        "bark.png",
        "bark.png.meta.json",
        "level.scene.json",
    ] {
        assert!(!copy.path(&format!("assets/imported/{f}")).exists(), "{f}");
    }
    assert_eq!(s.project().unwrap().assets.len(), before);
    ok(s.operate("redoAsset", &json!({})));
    assert!(s.project().unwrap().assets.get(own).is_some());
    assert_clean(&s);
}
