//! Project model corpus (`fixtures/project/`): a complete project file
//! with a startup scene, input actions, quality profiles, a component
//! registry and build targets, the lock this build writes, and an asset
//! tree that references a prefab, a source mesh and a material. The
//! fixture validates clean; each broken variant is a temporary copy with
//! one thing wrong and asserts the exact code it produces.
//!
//! `FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_document --test project`
//! rewrites the fixture lock for the current build.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fynkel_document::project::{describe_json, LOCK_FILE, PROJECT_FILE};
use fynkel_document::{
    validate_project_with, AssetDatabase, Lock, NoSourcePrefabs, Project, ProjectReport, SchemaSet,
};

/// The fixture's Rig scene instances `rig.glb#prefab` (M3-19a), so the
/// project validates with the importer's provider — as `fynkel validate`
/// does; the bare `validate_project` has no importer and reports the
/// instance as `D0401` (a test below).
fn validate_project(dir: &Path, schemas: &SchemaSet, lock: Option<&Lock>) -> ProjectReport {
    validate_project_with(
        dir,
        schemas,
        lock,
        &fynkel_import::ImportedPrefabs::default(),
    )
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/project")
}

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

const IMPORTERS: &[(&str, u32)] = &[
    ("fynkel.gltf", 4),
    ("fynkel.image", 1),
    ("fynkel.audio", 1),
    ("fynkel.file", 1),
];

/// What `fynkel_import::expected_lock()` produces; mirrored here because
/// this crate cannot depend on the importers (cache version 4, container 1).
fn expected_lock() -> Lock {
    Lock::expected(IMPORTERS, 4, 1)
}

/// A temporary copy of the fixture project, removed on drop.
struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(name: &str) -> Temp {
        let dir =
            std::env::temp_dir().join(format!("fynkel-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixtures(), &dir);
        Temp { dir }
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.join(rel)).unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.dir.join(rel), text).unwrap();
    }

    fn edit_project(&self, f: impl FnOnce(&mut serde_json::Value)) {
        let mut v: serde_json::Value = serde_json::from_str(&self.read(PROJECT_FILE)).unwrap();
        f(&mut v);
        self.write(PROJECT_FILE, &serde_json::to_string_pretty(&v).unwrap());
    }

    fn codes(&self) -> Vec<(String, Option<String>)> {
        let report = validate_project(&self.dir, &schemas(), Some(&expected_lock()));
        report
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str().to_string(), d.pointer.clone()))
            .collect()
    }
}

impl Drop for Temp {
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

#[test]
fn fixture_lock_matches_this_build() {
    let path = fixtures().join(LOCK_FILE);
    let expected = expected_lock();
    if std::env::var_os("FYNKEL_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, expected.to_text()).unwrap();
    }
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        expected.to_text(),
        "fixtures/project/{LOCK_FILE} is stale; run with FYNKEL_UPDATE_FIXTURES=1"
    );
    let lock = Lock::parse(Some("lock"), &text, &schemas()).unwrap();
    assert_eq!(lock, expected);
    assert!(lock.check(&expected, None).is_empty());
}

#[test]
fn the_fixture_project_reads_and_validates_clean() {
    let schemas = schemas();
    let project = Project::read(&fixtures()).unwrap();
    assert_eq!(project.name(), "Fixture Game");
    assert_eq!(
        project.id().to_string(),
        "d0000000-0000-4000-8000-000000000001"
    );
    assert_eq!(project.assets, "assets");
    assert_eq!(
        project.startup_scene.as_ref().unwrap().asset.to_string(),
        "c0000000-0000-4000-8000-000000000030"
    );
    assert_eq!(
        project
            .input_actions
            .iter()
            .map(|a| (a.name.as_str(), a.kind.as_str(), a.bindings.len()))
            .collect::<Vec<_>>(),
        [
            ("move", "axis2d", 3),
            ("jump", "button", 3),
            ("look", "axis2d", 2),
            ("pause", "button", 2)
        ]
    );
    let quality = project.quality.as_ref().unwrap();
    assert_eq!(quality.default, "medium");
    let medium = quality
        .profiles
        .iter()
        .find(|p| p.name == "medium")
        .unwrap();
    assert_eq!((medium.shadow_resolution, medium.msaa), (1024, 4));
    assert!(medium.shadows, "shadows default to on");
    assert_eq!(project.components.as_ref().unwrap().len(), 21);
    assert_eq!(project.physics.gravity, [0.0, -9.81, 0.0]);
    assert_eq!(project.physics.layers, ["default", "props"]);
    assert_eq!(project.physics.mask(&["props"]), Some(2));
    assert_eq!(project.physics.mask(&["default", "props"]), Some(3));
    assert_eq!(project.physics.mask(&["enemies"]), None);
    assert!(project.allows_component("fynkel.Transform"));
    assert!(project.allows_component("game.Spinner"));
    assert!(!project.allows_component("fynkel.Name"));
    assert_eq!(
        project
            .enabled_targets()
            .map(|t| (t.name.as_str(), t.profile.as_str()))
            .collect::<Vec<_>>(),
        [("macos", "dev"), ("ios", "ios")]
    );
    assert!(project.validate(&schemas).is_empty());

    let report = validate_project(&fixtures(), &schemas, Some(&expected_lock()));
    assert!(
        report.diagnostics.is_empty(),
        "{:?}",
        report
            .diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        (report.documents, report.sources, report.references),
        (9, 5, 25)
    );
    assert!(report.lock.is_some());
    let json = report.json();
    assert_eq!(json["ok"], true);
    assert_eq!(json["project"]["name"], "Fixture Game");
    assert_eq!(json["errors"], 0);
}

#[test]
fn describe_json_lists_everything() {
    let schemas = schemas();
    let project = Project::read(&fixtures()).unwrap();
    let (db, diags) = AssetDatabase::scan(&project.assets_dir(&fixtures()), &schemas);
    assert!(diags.is_empty());
    let lock = Lock::read(&fixtures(), &schemas).unwrap();
    let v = describe_json(&project, Some(&db), Some(&lock));
    assert_eq!(v["name"], "Fixture Game");
    assert_eq!(v["startupScene"]["path"], "scenes/main.scene.json");
    assert_eq!(v["assets"], "assets");
    assert_eq!(v["assetCount"], 14);
    assert_eq!(v["inputActions"].as_array().unwrap().len(), 4);
    assert_eq!(v["quality"]["default"], "medium");
    assert_eq!(v["components"].as_array().unwrap().len(), 21);
    assert_eq!(
        v["physics"]["layers"],
        serde_json::json!(["default", "props"])
    );
    assert_eq!(v["targets"].as_array().unwrap().len(), 3);
    assert_eq!(v["lock"]["schemaFingerprint"], lock.schema_fingerprint);
}

#[test]
fn a_missing_project_file_is_p0001_with_a_suggestion() {
    let dir = std::env::temp_dir().join(format!("fynkel-project-none-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let err = Project::read(&dir).unwrap_err();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code.as_str(), "FYN-P0001");
    assert!(err[0].suggestion.as_deref().unwrap().contains("--project"));
    let report = validate_project(&dir, &schemas(), None);
    assert!(report.project.is_none());
    assert_eq!(report.diagnostics[0].code.as_str(), "FYN-P0001");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_lock_is_p0010_and_a_stale_lock_is_p0011_per_field() {
    let t = Temp::new("lock");
    std::fs::remove_file(t.dir.join(LOCK_FILE)).unwrap();
    let codes = t.codes();
    assert_eq!(codes, [("FYN-P0010".to_string(), None)]);
    assert!(Lock::read(&t.dir, &schemas()).unwrap_err()[0]
        .suggestion
        .as_deref()
        .unwrap()
        .contains("fynkel project lock"));

    let mut stale = expected_lock();
    stale.runtime.version = "0.0.0".to_string();
    stale.schema_fingerprint = "0x0000000000000000".to_string();
    stale.importers.insert("fynkel.gltf".to_string(), 7);
    stale.importers.remove("fynkel.file");
    stale.cache.container = 2;
    t.write(LOCK_FILE, &stale.to_text());
    let codes = t.codes();
    let pointers: Vec<&str> = codes.iter().map(|(_, p)| p.as_deref().unwrap()).collect();
    assert!(codes.iter().all(|(c, _)| c == "FYN-P0011"), "{codes:?}");
    assert_eq!(
        pointers,
        [
            "/runtime/version",
            "/schemaFingerprint",
            "/importers/fynkel.file",
            "/importers/fynkel.gltf",
            "/cache/container",
        ]
    );
    // A stale lock never stops the rest of the project being checked.
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    assert_eq!(report.documents, 9);
}

#[test]
fn a_malformed_lock_is_p0012() {
    let t = Temp::new("lock-bad");
    t.write(LOCK_FILE, "{\"formatVersion\": 1}");
    let codes = t.codes();
    assert!(
        !codes.is_empty() && codes.iter().all(|(c, _)| c == "FYN-P0012"),
        "{codes:?}"
    );
    // Missing required members anchor at the root.
    assert!(
        codes.iter().all(|(_, p)| p.as_deref() == Some("")),
        "{codes:?}"
    );
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("runtime")));
    // Not even JSON: the strict-JSON code, anchored at the lock file.
    t.write(LOCK_FILE, "not json");
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    assert_eq!(report.diagnostics[0].code.as_str(), "FYN-D0001");
    assert!(report.diagnostics[0]
        .document
        .as_deref()
        .unwrap()
        .ends_with(LOCK_FILE));
}

#[test]
fn the_startup_scene_must_resolve_to_a_scene() {
    let t = Temp::new("startup");
    t.edit_project(|v| v["startupScene"]["asset"] = "c0000000-0000-4000-8000-0000000000ff".into());
    assert_eq!(
        t.codes(),
        [(
            "FYN-P0002".to_string(),
            Some("/startupScene/asset".to_string())
        )]
    );
    // Pointing at the prefab is the same error: not a scene.
    t.edit_project(|v| v["startupScene"]["asset"] = "c0000000-0000-4000-8000-000000000040".into());
    assert_eq!(
        t.codes(),
        [(
            "FYN-P0002".to_string(),
            Some("/startupScene/asset".to_string())
        )]
    );
    // The nil id is not an id at all: malformed, like any other bad entry.
    t.edit_project(|v| v["startupScene"]["asset"] = "00000000-0000-0000-0000-000000000000".into());
    assert!(t
        .codes()
        .iter()
        .any(|(c, p)| c == "FYN-P0008" && p.as_deref() == Some("/startupScene")));
}

#[test]
fn the_component_registry_is_checked_both_ways() {
    let t = Temp::new("registry");
    // A native entry this build does not have.
    t.edit_project(|v| {
        v["components"]["fynkel.Rigidbody"] = serde_json::json!({ "kind": "native" });
    });
    assert_eq!(
        t.codes(),
        [(
            "FYN-P0003".to_string(),
            Some("/components/fynkel.Rigidbody".to_string())
        )]
    );
    // A scene using a type the registry leaves out: one warning per
    // document and type, never an error.
    t.edit_project(|v| {
        v["components"]
            .as_object_mut()
            .unwrap()
            .remove("fynkel.Rigidbody");
        v["components"]
            .as_object_mut()
            .unwrap()
            .remove("fynkel.Camera");
    });
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    assert!(!report.has_errors());
    let warnings: Vec<_> = report
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.document.as_deref().unwrap_or("")))
        .collect();
    // All five scenes place a camera.
    assert_eq!(warnings.len(), 5, "{warnings:?}");
    assert!(warnings.iter().all(|w| w.0 == "FYN-P0004"));
    assert!(warnings[0].1.ends_with("catalogue.scene.json"));
    assert!(warnings[1].1.ends_with("main.scene.json"));
    assert!(warnings[2].1.ends_with("playground.scene.json"));
    assert!(warnings[3].1.ends_with("results.scene.json"));
    assert!(warnings[4].1.ends_with("rig.scene.json"));
    assert!(report.diagnostics[0].message.contains("fynkel.Camera"));
    // No registry at all means every built-in is allowed.
    t.edit_project(|v| {
        v.as_object_mut().unwrap().remove("components");
    });
    assert!(t.codes().is_empty());
}

#[test]
fn quality_targets_and_asset_root_are_checked() {
    let t = Temp::new("semantics");
    t.edit_project(|v| v["quality"]["default"] = "ultra".into());
    assert_eq!(
        t.codes(),
        [(
            "FYN-P0005".to_string(),
            Some("/quality/default".to_string())
        )]
    );
    t.edit_project(|v| {
        v["quality"]["default"] = "medium".into();
        v["targets"]["macos"]["enabled"] = false.into();
        v["targets"]["ios"]["enabled"] = false.into();
    });
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    assert!(!report.has_errors());
    assert_eq!(report.diagnostics[0].code.as_str(), "FYN-P0006");
    t.edit_project(|v| {
        v["targets"]["macos"]["enabled"] = true.into();
        v["assets"] = "assets/../../elsewhere".into();
    });
    let codes = t.codes();
    assert_eq!(
        codes[0],
        ("FYN-P0007".to_string(), Some("/assets".to_string()))
    );
    t.edit_project(|v| v["assets"] = "content".into());
    let codes = t.codes();
    assert_eq!(codes[0].0, "FYN-P0007", "a missing asset root: {codes:?}");
}

#[test]
fn input_bindings_are_checked_by_the_runtime_grammar() {
    let t = Temp::new("bindings");
    assert!(t.codes().is_empty(), "the fixture's bindings all parse");
    t.edit_project(|v| {
        v["inputActions"]["jump"]["bindings"] =
            serde_json::json!(["keyboard:Space", "gamepad:cross", "joystick:a"]);
        v["inputActions"]["move"]["bindings"] =
            serde_json::json!(["keyboard:WASD", "mouse:left", "keyboard:A/D"]);
    });
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    let bad: Vec<(String, String)> = report
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "FYN-P0009")
        .map(|d| (d.pointer.clone().unwrap_or_default(), d.message.clone()))
        .collect();
    // Document order: `move` is declared before `jump`.
    assert_eq!(bad.len(), 4, "{bad:?}");
    assert_eq!(bad[0].0, "/inputActions/move/bindings/1");
    assert!(
        bad[0].1.contains("mouse:left is not an axis2d control"),
        "{}",
        bad[0].1
    );
    assert_eq!(bad[1].0, "/inputActions/move/bindings/2");
    assert_eq!(bad[2].0, "/inputActions/jump/bindings/1");
    assert!(
        bad[2]
            .1
            .starts_with("gamepad:cross is not a button control"),
        "{}",
        bad[2].1
    );
    assert_eq!(bad[3].0, "/inputActions/jump/bindings/2");
    assert!(report.has_errors());
}

#[test]
fn physics_layers_and_gravity_are_checked() {
    let t = Temp::new("physics");
    t.edit_project(|v| {
        let mut layers: Vec<String> = (0..33).map(|i| format!("layer{i}")).collect();
        layers[5] = "layer2".into();
        layers[7] = String::new();
        v["physics"]["layers"] = serde_json::json!(layers);
        v["physics"]["gravity"] = serde_json::json!([0, -9.81]);
    });
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    let bad: Vec<(String, String)> = report
        .diagnostics
        .iter()
        .map(|d| (d.pointer.clone().unwrap_or_default(), d.message.clone()))
        .collect();
    // The schema reports the short gravity (once); the layer rules are
    // `FYN-P0013`.
    assert!(bad.iter().any(|(p, _)| p == "/physics/gravity"), "{bad:?}");
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.code.as_str() == "FYN-P0013")
            .count(),
        3,
        "{bad:?}"
    );
    assert!(bad
        .iter()
        .any(|(p, m)| p == "/physics/layers" && m.contains("33 names")));
    assert!(bad
        .iter()
        .any(|(p, m)| p == "/physics/layers/5" && m.contains("twice")));
    assert!(bad
        .iter()
        .any(|(p, m)| p == "/physics/layers/7" && m.contains("empty")));
    assert!(report.has_errors());
    // A project without the block validates and gets the defaults.
    let t = Temp::new("physics-default");
    t.edit_project(|v| {
        v.as_object_mut().unwrap().remove("physics");
    });
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    let p = report.project.unwrap();
    assert_eq!(
        p.physics,
        fynkel_document::project::PhysicsSettings::default()
    );
}

#[test]
fn schema_violations_in_the_project_file_are_reported_once_each() {
    let t = Temp::new("schema");
    t.edit_project(|v| {
        v["inputActions"]["Jump!"] = serde_json::json!({ "type": "button", "bindings": [] });
        v["targets"]["amiga"] = serde_json::json!({});
        v["quality"]["profiles"]["low"]["msaa"] = 3.into();
    });
    let codes = t.codes();
    let by_code: BTreeMap<&str, usize> = codes.iter().fold(BTreeMap::new(), |mut m, (c, _)| {
        *m.entry(c.as_str()).or_default() += 1;
        m
    });
    assert_eq!(by_code.get("FYN-D0100"), Some(&3), "{codes:?}");
    assert!(!by_code.contains_key("FYN-P0008"), "{codes:?}");
}

#[test]
fn broken_references_and_documents_under_the_asset_root_fail_validate() {
    let t = Temp::new("documents");
    let scene = t.read("assets/scenes/main.scene.json");
    t.write(
        "assets/scenes/main.scene.json",
        &scene.replace(
            "c0000000-0000-4000-8000-000000000020",
            "c0000000-0000-4000-8000-0000000000ee",
        ),
    );
    let codes = t.codes();
    assert!(
        codes.iter().any(|(c, _)| c == "FYN-A0004"),
        "an unresolved material reference: {codes:?}"
    );
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    assert!(report.has_errors());
    assert_eq!(report.json()["ok"], false);
}

/// M3-19a: the Rig scene's `Imported` entity instances `rig.glb#prefab`.
/// Without a provider the instance is `D0401` (the source's hierarchy is
/// not a document); with one the import expands like a prefab document,
/// its override on the Prop node applies, a wrong sub or a source without
/// a hierarchy is `D0401` with the hint, and a nested prefab may instance
/// an import too.
#[test]
fn imported_hierarchies_instantiate_like_prefabs() {
    use fynkel_document::prefab::expand;
    use fynkel_document::{Document, DocumentKind, PrefabLibrary, Scene};

    const RIG_SCENE: &str = "assets/scenes/rig.scene.json";
    const RIG: &str = "c0000000-0000-4000-8000-000000000005";
    const SWATCH: &str = "c0000000-0000-4000-8000-000000000010";
    let at = "/entities/c0000000-0000-4000-8000-0000000004e8/prefab/asset";

    // Bare: no importer, D0401 naming the instance.
    let bare = validate_project_with(
        &fixtures(),
        &schemas(),
        Some(&expected_lock()),
        &NoSourcePrefabs,
    );
    let codes: Vec<_> = bare
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.pointer.as_deref()))
        .collect();
    assert_eq!(codes, [("FYN-D0401", Some(at))], "{:#?}", bare.diagnostics);
    assert!(bare.diagnostics[0]
        .message
        .contains(&format!("prefab {RIG}#prefab is not available")));

    // With the importer: clean, and the expansion has the six nodes under
    // the anchor with the Prop's material overridden to `rusted`.
    let t = Temp::new("imported");
    assert_eq!(t.codes(), []);
    let scene = Scene::from_document(
        Document::read(&t.dir.join(RIG_SCENE), Some(DocumentKind::Scene)).unwrap(),
    );
    let (db, _) = AssetDatabase::scan(&t.dir.join("assets"), &schemas());
    let mut lib = PrefabLibrary::empty();
    let diags = lib.add_sources(
        &db,
        &PrefabLibrary::wanted_sources(&scene),
        &fynkel_import::ImportedPrefabs::default(),
    );
    assert!(diags.is_empty(), "{diags:#?}");
    let expanded = expand(&scene, &lib, &schemas());
    assert!(!expanded.has_errors(), "{:#?}", expanded.diagnostics);
    let out = Scene::from_document(expanded.document);
    let names: Vec<&str> = out.entities.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Camera", "Sun", "Rig", "Body", "hips", "spine", "head", "Prop", "Imported", "Rig",
            "Body", "hips", "spine", "head", "Prop"
        ]
    );
    let prop = out.entities.last().unwrap();
    let renderer = prop
        .components
        .iter()
        .find(|c| c.type_id == "fynkel.MeshRenderer")
        .unwrap();
    assert_eq!(
        renderer
            .data
            .get("material")
            .unwrap()
            .get("asset")
            .unwrap()
            .as_str(),
        Some("c0000000-0000-4000-8000-000000000020"),
        "the setField override on node/5/renderer applied"
    );

    // A wrong sub: D0401 with the hint. A source without a hierarchy (the
    // swatch texture): its sidecar has no `prefab`, so the reference is
    // `A0004` and the instance `D0401`; the importer is not consulted.
    let mut v: serde_json::Value = serde_json::from_str(&t.read(RIG_SCENE)).unwrap();
    v["entities"]["c0000000-0000-4000-8000-0000000004e8"]["prefab"]["asset"]["sub"] =
        "mesh/0".into();
    t.write(RIG_SCENE, &serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(t.codes(), [("FYN-D0401".to_string(), Some(at.to_string()))]);
    v["entities"]["c0000000-0000-4000-8000-0000000004e8"]["prefab"]["asset"] =
        serde_json::json!({ "asset": SWATCH, "sub": "prefab" });
    v["entities"]["c0000000-0000-4000-8000-0000000004e8"]["prefab"]["overrides"] =
        serde_json::json!([]);
    t.write(RIG_SCENE, &serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(
        t.codes(),
        [
            ("FYN-A0004".to_string(), Some(at.to_string())),
            ("FYN-D0401".to_string(), Some(at.to_string()))
        ]
    );

    // A prefab document instancing the import: nested, flattened, clean.
    let t = Temp::new("imported-nested");
    let mut crate_doc: serde_json::Value =
        serde_json::from_str(&t.read("assets/prefabs/crate.prefab.json")).unwrap();
    let root = crate_doc["roots"][0].as_str().unwrap().to_string();
    crate_doc["entities"]["c0000000-0000-4000-8000-0000000004ee"] = serde_json::json!({
        "name": "Statue",
        "parent": root,
        "children": [],
        "components": {
            "c0000000-0000-4000-8000-0000000004ef": {
                "type": "fynkel.Transform", "version": 1,
                "data": { "position": [0, 1, 0], "rotation": [0, 0, 0, 1], "scale": [0.25, 0.25, 0.25] }
            }
        },
        "prefab": { "asset": { "asset": RIG, "sub": "prefab" }, "instance": "c0000000-0000-4000-8000-0000000004f1" }
    });
    crate_doc["entities"][&root]["children"]
        .as_array_mut()
        .unwrap()
        .push("c0000000-0000-4000-8000-0000000004ee".into());
    t.write(
        "assets/prefabs/crate.prefab.json",
        &serde_json::to_string_pretty(&crate_doc).unwrap(),
    );
    assert_eq!(t.codes(), []);
}
