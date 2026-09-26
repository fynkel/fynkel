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
    ("fynkel.fbx", 4),
    ("fynkel.gltf", 8),
    ("fynkel.image", 1),
    ("fynkel.audio", 2),
    ("fynkel.file", 1),
];

/// What `fynkel_import::expected_lock()` produces; mirrored here because
/// this crate cannot depend on the importers (cache version 8, container 1).
fn expected_lock() -> Lock {
    Lock::expected(IMPORTERS, 8, 1)
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

/// ADR-0029 §4, one lock shape: `package:fynkel` ships a lock like a
/// project's, whose runtime also names the release store and a prebuilt
/// library per target (unpublished until a release fills the checksums).
#[test]
fn package_fynkel_ships_its_lock_in_the_project_shape() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/fynkel")
        .join(LOCK_FILE);
    let text = std::fs::read_to_string(&path).unwrap();
    let lock = Lock::parse(Some("packages/fynkel"), &text, &schemas()).unwrap();
    let expected = expected_lock().keeping_store(&lock);
    if std::env::var_os("FYNKEL_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, expected.to_text()).unwrap();
    }
    assert_eq!(
        text,
        expected.to_text(),
        "packages/fynkel/{LOCK_FILE} is stale; run with FYNKEL_UPDATE_FIXTURES=1"
    );
    assert!(lock.check(&expected_lock(), None).is_empty());
    assert!(lock.runtime.store.as_deref().unwrap().contains("{version}"));
    assert_eq!(
        lock.runtime
            .artifacts
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "ios-arm64",
            "ios-simulator-arm64",
            "ios-simulator-x64",
            "macos-arm64",
            "macos-x64"
        ]
    );

    // `fynkel project lock` keeps the store; the libraries only for the
    // same runtime version.
    let mut older = lock.clone();
    older.runtime.version = "0.0.0".into();
    let rewritten = expected_lock().keeping_store(&older);
    assert_eq!(rewritten.runtime.store, lock.runtime.store);
    assert!(rewritten.runtime.artifacts.is_empty());
    // A project's lock without a store stays without one.
    assert_eq!(
        expected_lock().keeping_store(&expected_lock()),
        expected_lock()
    );
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
    assert_eq!(project.components.as_ref().unwrap().len(), 24);
    assert_eq!(project.physics.gravity, [0.0, -9.81, 0.0]);
    let lt = &project.layers_and_tags;
    assert_eq!(lt.layers, ["default", "props"]);
    assert_eq!(lt.mask(&["props"]), Some(2));
    assert_eq!(lt.mask(&["default", "props"]), Some(3));
    assert_eq!(lt.mask(&["enemies"]), None);
    assert_eq!(lt.layer("props"), Some(1));
    assert!(lt.tags.is_empty());
    assert!(project.allows_component("fynkel.Layer"));
    assert!(project.allows_component("fynkel.Tags"));
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
        (11, 6, 31)
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
    assert_eq!(v["assetCount"], 17);
    assert_eq!(v["inputActions"].as_array().unwrap().len(), 4);
    assert_eq!(v["quality"]["default"], "medium");
    assert_eq!(v["components"].as_array().unwrap().len(), 24);
    assert_eq!(v["layers"], serde_json::json!(["default", "props"]));
    assert_eq!(v["tags"], serde_json::json!([]));
    assert_eq!(v["targets"].as_array().unwrap().len(), 3);
    assert_eq!(v["lock"]["schemaFingerprint"], lock.schema_fingerprint);
    assert_eq!(
        v["modules"],
        serde_json::json!({"physics": true, "audio": true, "animation": true, "strip": false})
    );
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
    stale.importers.insert("fynkel.gltf".to_string(), 99);
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
    assert_eq!(report.documents, 11);
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
fn a_module_turned_off_fails_every_component_of_it() {
    let t = Temp::new("modules");
    t.edit_project(|v| v["modules"] = serde_json::json!({ "physics": false }));
    let project = Project::read(&t.dir).unwrap();
    assert!(!project.modules.modules.physics);
    assert!(project.modules.modules.audio && project.modules.modules.animation);
    assert!(!project.allows_component("fynkel.Collider"));
    assert!(!project.allows_component("fynkel.CharacterController"));
    assert!(project.allows_component("fynkel.AudioSource"));
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    assert!(report.has_errors());
    let off: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "FYN-P0018")
        .collect();
    // Every physics component in the fixture's scenes and prefabs: one
    // error each, anchored at its type.
    assert_eq!(off.len(), 10, "{off:#?}");
    assert!(off.iter().all(|d| d.message.contains("physics module")));
    assert!(off
        .iter()
        .all(|d| d.pointer.as_deref().unwrap().ends_with("/type")));
    assert!(off
        .iter()
        .any(|d| d.document.as_deref().unwrap().ends_with("coin.prefab.json")));
    assert!(off[0]
        .suggestion
        .as_deref()
        .unwrap()
        .contains("modules.physics"));
    assert_eq!(
        report.diagnostics.len(),
        off.len(),
        "nothing else: {:#?}",
        report.diagnostics
    );

    // Animation off: the animators and the skin.
    t.edit_project(|v| v["modules"] = serde_json::json!({ "animation": false, "strip": true }));
    let project = Project::read(&t.dir).unwrap();
    assert!(project.modules.strip);
    let codes = t.codes();
    assert!(codes.len() >= 3, "{codes:?}");
    assert!(codes.iter().all(|c| c.0 == "FYN-P0018"), "{codes:?}");

    // Everything on again: clean.
    t.edit_project(|v| {
        v["modules"] = serde_json::json!({ "physics": true, "audio": true, "animation": true })
    });
    assert!(t.codes().is_empty());

    // Not a boolean.
    t.edit_project(|v| v["modules"] = serde_json::json!({ "audio": "no" }));
    let codes = t.codes();
    assert!(
        codes
            .iter()
            .any(|c| c.1.as_deref() == Some("/modules/audio")),
        "{codes:?}"
    );
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
fn layers_tags_and_gravity_are_checked() {
    let t = Temp::new("physics");
    t.edit_project(|v| {
        let mut layers: Vec<String> = (0..33).map(|i| format!("layer{i}")).collect();
        layers[5] = "layer2".into();
        layers[7] = String::new();
        v["layers"] = serde_json::json!(layers);
        let mut tags: Vec<String> = (0..33).map(|i| format!("tag{i}")).collect();
        tags[3] = "tag1".into();
        v["tags"] = serde_json::json!(tags);
        v["physics"]["gravity"] = serde_json::json!([0, -9.81]);
    });
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    let bad: Vec<(String, String)> = report
        .diagnostics
        .iter()
        .map(|d| (d.pointer.clone().unwrap_or_default(), d.message.clone()))
        .collect();
    // The schema reports the short gravity, the first layer and both
    // lengths; the list rules are `FYN-P0014`.
    assert!(bad.iter().any(|(p, _)| p == "/physics/gravity"), "{bad:?}");
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.code.as_str() == "FYN-P0014")
            .count(),
        6,
        "{bad:?}"
    );
    for (pointer, words) in [
        ("/layers", "33 names"),
        ("/layers/5", "twice"),
        ("/layers/7", "empty"),
        ("/layers", "start with \"default\""),
        ("/tags", "33 names"),
        ("/tags/3", "twice"),
    ] {
        assert!(
            bad.iter().any(|(p, m)| p == pointer && m.contains(words)),
            "{pointer} {words}: {bad:?}"
        );
    }
    assert!(report.has_errors());
    // A project without the physics block validates and gets the defaults.
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
    // Without `layers` only `default` is declared: the fixture's objects on
    // layer 1 (`props`) are `FYN-H0012`, at the component's field.
    let t = Temp::new("layers-default");
    t.edit_project(|v| {
        v.as_object_mut().unwrap().remove("layers");
    });
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    let p = report.project.as_ref().unwrap();
    assert_eq!(
        p.layers_and_tags,
        fynkel_document::project::LayersAndTags::default()
    );
    let undeclared: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "FYN-H0012")
        .collect();
    assert!(!undeclared.is_empty(), "{:#?}", report.diagnostics);
    for d in &undeclared {
        assert!(
            d.message
                .contains("layer 1 is not declared: the project's layers are [default]"),
            "{d}"
        );
        assert!(
            d.pointer.as_deref().unwrap().ends_with("/data/layer"),
            "{d}"
        );
        assert!(d.id.is_some() && d.document.is_some(), "{d}");
    }
    assert_eq!(report.diagnostics.len(), undeclared.len());
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

/// A model's material remap (`settings.materials`) is checked like any
/// reference: a missing target is `A0004`, a target that is not a
/// material document `A0007`; a remap to a project material is clean.
#[test]
fn material_remaps_validate_against_the_database() {
    let t = Temp::new("remaps");
    let sidecar = t.read("assets/models/robot.glb.meta.json");
    let remap = |target: &str| {
        sidecar.replace(
            "\"settings\": {}",
            &format!("\"settings\": {{ \"materials\": {{ \"material/Painted\": {{ \"asset\": \"{target}\" }} }} }}"),
        )
    };
    assert_ne!(
        remap("x"),
        sidecar,
        "the fixture sidecar has empty settings"
    );
    t.write(
        "assets/models/robot.glb.meta.json",
        &remap("c0000000-0000-4000-8000-000000000020"),
    );
    assert_eq!(t.codes(), Vec::<(String, Option<String>)>::new());
    t.write(
        "assets/models/robot.glb.meta.json",
        &remap("c0000000-0000-4000-8000-000000000002"),
    );
    let codes = t.codes();
    assert!(
        codes.iter().any(|(c, p)| c == "FYN-A0007"
            && p.as_deref() == Some("/settings/materials/material~1Painted")),
        "{codes:?}"
    );
    t.write(
        "assets/models/robot.glb.meta.json",
        &remap("c0000000-0000-4000-8000-0000000000ee"),
    );
    let codes = t.codes();
    assert!(codes.iter().any(|(c, _)| c == "FYN-A0004"), "{codes:?}");
}

/// A reference to an asset of the wrong kind is `A0007` (ADR-0027 §4):
/// a built-in field (the mesh and material of a `MeshRenderer`, swapped)
/// and a script's `asset(mesh)` field given a material document.
#[test]
fn references_of_the_wrong_kind_fail_validate() {
    let t = Temp::new("wrong-kind");
    let scene = t.read("assets/scenes/main.scene.json");
    let mesh = r#""asset": "c0000000-0000-4000-8000-000000000001",
              "sub": "mesh/Cube""#;
    let material = r#""asset": "c0000000-0000-4000-8000-000000000020""#;
    assert!(scene.contains(mesh) && scene.contains(material));
    let swapped = scene
        .replace(mesh, "@MESH@")
        .replace(material, mesh)
        .replace("@MESH@", material);
    t.write("assets/scenes/main.scene.json", &swapped);
    let codes = t.codes();
    let robot = "/entities/c0000000-0000-4000-8000-0000000000e1/components";
    for field in ["mesh", "material"] {
        let at = format!("{robot}/c0000000-0000-4000-8000-0000000000c1/data/{field}");
        assert!(
            codes
                .iter()
                .any(|(c, p)| c == "FYN-A0007" && p.as_deref() == Some(at.as_str())),
            "{field}: {codes:?}"
        );
    }

    let showcase = scene.replacen(
        "\"speed\": 2.5,",
        "\"speed\": 2.5, \"showcase\": { \"asset\": \"c0000000-0000-4000-8000-000000000020\" },",
        1,
    );
    assert_ne!(showcase, scene);
    t.write("assets/scenes/main.scene.json", &showcase);
    let at = format!("{robot}/c0000000-0000-4000-8000-0000000000c2/data/showcase");
    assert_eq!(
        t.codes(),
        vec![("FYN-A0007".to_string(), Some(at))],
        "a script's asset(mesh) field holding a material"
    );
    t.write(
        "assets/scenes/main.scene.json",
        &showcase.replace(
            r#""showcase": { "asset": "c0000000-0000-4000-8000-000000000020" }"#,
            r#""showcase": { "asset": "c0000000-0000-4000-8000-000000000001", "sub": "mesh/Cube" }"#,
        ),
    );
    assert_eq!(t.codes(), Vec::<(String, Option<String>)>::new());
}

/// M3-19a/19b: the Rig scene's `Imported` entity instances the Column
/// rig's `prefab` — the skeleton file `rig.glb`'s hierarchy under the rig
/// document's id. Without a provider the instance is `D0401` (the
/// source's hierarchy is not a document); with one the import expands
/// like a prefab document, its override on the Prop node applies (the
/// entity ids are the skeleton file's), a wrong sub or a source without a
/// hierarchy is `D0401` with the hint, and a nested prefab may instance
/// the source itself too.
#[test]
fn imported_hierarchies_instantiate_like_prefabs() {
    use fynkel_document::prefab::expand;
    use fynkel_document::{Document, DocumentKind, PrefabLibrary, Scene};

    const RIG_SCENE: &str = "assets/scenes/rig.scene.json";
    const RIG: &str = "c0000000-0000-4000-8000-000000000005";
    const COLUMN: &str = "c0000000-0000-4000-8000-000000000050";
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
        .contains(&format!("prefab {COLUMN}#prefab is not available")));

    // With the importer: clean, and the expansion has the rig's root as the
    // instance and its five nodes under it, with the Prop's material overridden to `rusted`.
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
            "Camera", "Sun", "Rig", "Body", "hips", "spine", "head", "Prop", "Imported", "Body",
            "hips", "spine", "head", "Prop"
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
        "the setField override on node/Rig/Prop/renderer applied"
    );

    // The rig's prefab is the skeleton file's under the rig's own
    // sub-asset id; the library answers both references with the same
    // hierarchy.
    let column_id: uuid::Uuid = COLUMN.parse().unwrap();
    let column = lib
        .get_ref(&fynkel_contracts::AssetRef::with_sub(
            fynkel_contracts::Uuid::from_bytes(*column_id.as_bytes()),
            "prefab",
        ))
        .expect("the rig's prefab");
    assert_eq!(
        column.id(),
        fynkel_document::Rig::sub_id(column_id, "prefab")
    );
    assert_eq!(column.document.envelope.name, "Column");
    assert_eq!(column.entities.len(), 6);

    // A wrong sub (a clip of the rig, a mesh of the source): D0401 with
    // the hint. A source without a hierarchy (the swatch texture): its
    // sidecar has no `prefab`, so the reference is `A0004` and the
    // instance `D0401`; the importer is not consulted.
    let mut v: serde_json::Value = serde_json::from_str(&t.read(RIG_SCENE)).unwrap();
    v["entities"]["c0000000-0000-4000-8000-0000000004e8"]["prefab"]["asset"]["sub"] =
        "anim/wave".into();
    t.write(RIG_SCENE, &serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(t.codes(), [("FYN-D0401".to_string(), Some(at.to_string()))]);
    v["entities"]["c0000000-0000-4000-8000-0000000004e8"]["prefab"]["asset"] =
        serde_json::json!({ "asset": RIG, "sub": "mesh/Body" });
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
    let root = crate_doc["root"].as_str().unwrap().to_string();
    // The instance is the imported root: no components of its own.
    crate_doc["entities"]["c0000000-0000-4000-8000-0000000004ee"] = serde_json::json!({
        "name": "Statue",
        "parent": root,
        "children": [],
        "components": {},
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

/// M3-19b: the Column rig document — its sub-assets in the database
/// (`prefab`, `anim/<name>`, derived from its id), and the rules: the
/// skeleton must be an imported model without a sub (`D0500`), a clip an
/// imported model's `anim/<n>` (`D0501`), a clip name an identifier
/// (`D0502`), unknown ids the database's `A0004`/`A0005`.
#[test]
fn rig_documents_validate_against_the_database() {
    use fynkel_document::Rig;

    const COLUMN_DOC: &str = "assets/rigs/column.rig.json";
    const COLUMN: &str = "c0000000-0000-4000-8000-000000000050";
    const RIG: &str = "c0000000-0000-4000-8000-000000000005";
    const SWATCH: &str = "c0000000-0000-4000-8000-000000000003";
    const CRATE: &str = "c0000000-0000-4000-8000-000000000040";

    let schemas = schemas();
    let (db, diags) = AssetDatabase::scan(&fixtures().join("assets"), &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    let column_id: uuid::Uuid = COLUMN.parse().unwrap();
    let entry = db.get(column_id).unwrap();
    assert_eq!(
        entry.kind,
        fynkel_document::AssetKind::Document(fynkel_document::DocumentKind::Rig)
    );
    assert_eq!(
        entry.sub_assets.keys().collect::<Vec<_>>(),
        ["anim/bob", "anim/wave", "prefab"]
    );
    assert_eq!(
        entry.sub_assets["anim/wave"],
        Rig::sub_id(column_id, "anim/wave")
    );
    let resolved = db
        .resolve(&fynkel_contracts::AssetRef::with_sub(
            fynkel_contracts::Uuid::from_bytes(*column_id.as_bytes()),
            "anim/wave",
        ))
        .unwrap();
    assert_eq!(resolved.entry.path, "rigs/column.rig.json");
    let by_sub_id = db
        .resolve(&fynkel_contracts::AssetRef::new(
            fynkel_contracts::Uuid::from_bytes(*entry.sub_assets["prefab"].as_bytes()),
        ))
        .unwrap();
    assert_eq!(
        by_sub_id.sub.as_ref().map(|(s, _)| s.as_str()),
        Some("prefab")
    );

    let t = Temp::new("rig-doc");
    let original = t.read(COLUMN_DOC);
    let edit = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v: serde_json::Value = serde_json::from_str(&original).unwrap();
        f(&mut v);
        t.write(COLUMN_DOC, &serde_json::to_string_pretty(&v).unwrap());
        t.codes()
    };
    // A skeleton that is a texture, a prefab document, a sub-asset of the
    // model, or unset: D0500, and the scene's instance of the rig's
    // prefab cannot expand (D0401).
    let instance = (
        "FYN-D0401".to_string(),
        Some("/entities/c0000000-0000-4000-8000-0000000004e8/prefab/asset".to_string()),
    );
    let skeleton = |pointer: &str| ("FYN-D0500".to_string(), Some(pointer.to_string()));
    assert_eq!(
        edit(&|v| v["skeleton"] = serde_json::json!({ "asset": SWATCH })),
        [skeleton("/skeleton"), instance.clone()]
    );
    assert_eq!(
        edit(&|v| v["skeleton"] = serde_json::json!({ "asset": CRATE })),
        [skeleton("/skeleton"), instance.clone()]
    );
    assert_eq!(
        edit(&|v| v["skeleton"] = serde_json::json!({ "asset": RIG, "sub": "mesh/Body" })),
        [skeleton("/skeleton"), instance.clone()]
    );
    assert_eq!(
        edit(&|v| v["skeleton"] =
            serde_json::json!({ "asset": "00000000-0000-0000-0000-000000000000" })),
        [skeleton("/skeleton/asset"), instance.clone()]
    );
    // A clip that is the model itself, its mesh, a texture, or an unknown
    // sub; a clip name that is not an identifier.
    assert_eq!(
        edit(&|v| v["clips"]["wave"] = serde_json::json!({ "asset": RIG })),
        [("FYN-D0501".to_string(), Some("/clips/wave".to_string()))]
    );
    assert_eq!(
        edit(&|v| v["clips"]["wave"] = serde_json::json!({ "asset": RIG, "sub": "mesh/Body" })),
        [("FYN-D0501".to_string(), Some("/clips/wave".to_string()))]
    );
    assert_eq!(
        edit(&|v| v["clips"]["wave"] = serde_json::json!({ "asset": SWATCH })),
        [("FYN-D0501".to_string(), Some("/clips/wave".to_string()))]
    );
    let codes =
        edit(&|v| v["clips"]["wave"] = serde_json::json!({ "asset": RIG, "sub": "anim/9" }));
    assert_eq!(
        codes.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(),
        ["FYN-A0005"],
        "{codes:?}"
    );
    let codes = edit(&|v| {
        let wave = v["clips"].as_object_mut().unwrap().remove("wave").unwrap();
        v["clips"]["fast run"] = wave;
    });
    assert_eq!(
        codes
            .iter()
            .map(|(c, p)| (c.as_str(), p.as_deref()))
            .collect::<Vec<_>>(),
        [
            // The fixture's blend space names `anim/wave` (M3-19c).
            ("FYN-A0005", Some("/points/1/clip")),
            ("FYN-D0100", Some("/clips")),
            ("FYN-D0502", Some("/clips/fast run")),
            // The fixture's markers name `wave` (M3-19c): orphaned now.
            ("FYN-D0502", Some("/markers/wave")),
            (
                "FYN-A0005",
                Some("/entities/c0000000-0000-4000-8000-0000000004e8/prefab/overrides/2/data/clip")
            ),
        ],
        "{codes:?}"
    );
    // The scene's Animator naming a clip the rig does not declare: A0005
    // (and the markers orphaned with it).
    let codes = edit(&|v| {
        v["clips"].as_object_mut().unwrap().remove("wave");
    });
    assert_eq!(
        codes
            .iter()
            .map(|(c, p)| (c.as_str(), p.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("FYN-A0005", Some("/points/1/clip")),
            ("FYN-D0502", Some("/markers/wave")),
            (
                "FYN-A0005",
                Some("/entities/c0000000-0000-4000-8000-0000000004e8/prefab/overrides/2/data/clip")
            )
        ],
        "{codes:?}"
    );
    // Markers (M3-19c): a malformed entry, a non-array, a name that is not
    // a clip; a valid list on `bob` is clean.
    assert_eq!(
        edit(&|v| v["markers"]["wave"][1] = serde_json::json!({ "at": -1.0, "name": "x" })),
        [
            (
                "FYN-D0100".to_string(),
                Some("/markers/wave/1/at".to_string())
            ),
            ("FYN-D0502".to_string(), Some("/markers/wave/1".to_string()))
        ]
    );
    assert_eq!(
        edit(&|v| v["markers"]["wave"][0] = serde_json::json!({ "at": 0.5 })),
        [("FYN-D0100".to_string(), Some("/markers/wave/0".to_string()))],
        "the schema requires the name"
    );
    assert_eq!(
        edit(&|v| v["markers"]["bob"] = serde_json::json!({ "at": 0.5, "name": "x" })),
        [("FYN-D0100".to_string(), Some("/markers/bob".to_string()))],
        "the schema wants an array"
    );
    assert_eq!(
        edit(&|v| v["markers"]["nope"] = serde_json::json!([])),
        [("FYN-D0502".to_string(), Some("/markers/nope".to_string()))]
    );
    assert_eq!(
        edit(&|v| v["markers"]["bob"] = serde_json::json!([{ "at": 1.0, "name": "lift" }])),
        []
    );
}

/// Blend spaces (M3-19c, `docs/documents.md` § Blend spaces): the points
/// strictly increasing (`D0600`), every point an animation clip of a rig
/// or a model (`D0601`), a malformed entry `D0602`, unknown ids the
/// database's `A0004`/`A0005`.
#[test]
fn blend_documents_validate_against_the_database() {
    use fynkel_document::{Blend, Document};

    const BLEND_DOC: &str = "assets/rigs/column.blend.json";
    const BLEND: &str = "c0000000-0000-4000-8000-000000000051";
    const COLUMN: &str = "c0000000-0000-4000-8000-000000000050";
    const RIG: &str = "c0000000-0000-4000-8000-000000000005";
    const SWATCH: &str = "c0000000-0000-4000-8000-000000000003";

    let schemas = schemas();
    let (db, diags) = AssetDatabase::scan(&fixtures().join("assets"), &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    let id: uuid::Uuid = BLEND.parse().unwrap();
    let entry = db.get(id).unwrap();
    assert_eq!(
        entry.kind,
        fynkel_document::AssetKind::Document(fynkel_document::DocumentKind::Blend)
    );
    assert!(entry.sub_assets.is_empty());
    let doc = Document::read(&fixtures().join(BLEND_DOC), None).unwrap();
    assert_eq!(doc.kind, fynkel_document::DocumentKind::Blend);
    let blend = Blend::from_document(doc);
    assert_eq!(blend.parameter, "energy");
    assert_eq!(blend.sync, "phase");
    assert_eq!(
        blend
            .points
            .iter()
            .map(|p| (p.at, p.clip.sub.as_str(), p.pointer.as_str()))
            .collect::<Vec<_>>(),
        [
            (0.0, "anim/bob", "/points/0"),
            (1.0, "anim/wave", "/points/1")
        ]
    );
    assert!(blend.validate(&schemas).is_empty());
    assert!(blend.validate_references(&db).is_empty());

    let t = Temp::new("blend-doc");
    let original = t.read(BLEND_DOC);
    let edit = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v: serde_json::Value = serde_json::from_str(&original).unwrap();
        f(&mut v);
        t.write(BLEND_DOC, &serde_json::to_string_pretty(&v).unwrap());
        t.codes()
    };
    let codes_of = |codes: &[(String, Option<String>)]| {
        codes
            .iter()
            .map(|(c, p)| (c.clone(), p.clone()))
            .collect::<Vec<_>>()
    };
    let d = |code: &str, pointer: &str| (code.to_string(), Some(pointer.to_string()));

    // Out of order, equal, and a single point (fine).
    let codes = edit(&|v| v["points"][1]["at"] = serde_json::json!(-1));
    assert_eq!(codes_of(&codes), [d("FYN-D0600", "/points/1/at")]);
    let codes = edit(&|v| v["points"][1]["at"] = serde_json::json!(0));
    assert_eq!(codes_of(&codes), [d("FYN-D0600", "/points/1/at")]);
    let codes = edit(&|v| {
        v["points"].as_array_mut().unwrap().pop();
    });
    assert!(codes.is_empty(), "{codes:?}");
    let codes = edit(&|v| v["points"] = serde_json::json!([]));
    assert_eq!(codes_of(&codes), [d("FYN-D0100", "/points")]);

    // A point naming the model's mesh, the whole rig, a texture, the
    // model's own clip (fine: a model's `anim/<n>` is a clip too), or a
    // nil id.
    let codes =
        edit(&|v| v["points"][1]["clip"] = serde_json::json!({ "asset": RIG, "sub": "mesh/Body" }));
    assert_eq!(codes_of(&codes), [d("FYN-D0601", "/points/1/clip")]);
    let codes = edit(&|v| v["points"][1]["clip"] = serde_json::json!({ "asset": COLUMN }));
    assert_eq!(codes_of(&codes), [d("FYN-D0601", "/points/1/clip")]);
    let codes = edit(&|v| v["points"][1]["clip"] = serde_json::json!({ "asset": SWATCH }));
    assert_eq!(codes_of(&codes), [d("FYN-D0601", "/points/1/clip")]);
    let codes =
        edit(&|v| v["points"][1]["clip"] = serde_json::json!({ "asset": RIG, "sub": "anim/bob" }));
    assert!(codes.is_empty(), "{codes:?}");
    let codes = edit(&|v| {
        v["points"][1]["clip"] =
            serde_json::json!({ "asset": "00000000-0000-0000-0000-000000000000" })
    });
    assert_eq!(codes_of(&codes), [d("FYN-D0601", "/points/1/clip/asset")]);
    // An unknown sub is the database's.
    let codes = edit(&|v| v["points"][1]["clip"]["sub"] = serde_json::json!("anim/nope"));
    assert_eq!(codes_of(&codes), [d("FYN-A0005", "/points/1/clip")]);

    // Malformed entries: a non-finite `at` the schema refuses, a clip
    // that is not an object, a missing parameter, another sync.
    let codes = edit(&|v| v["points"][0]["clip"] = serde_json::json!("walk"));
    assert_eq!(codes_of(&codes), [d("FYN-D0100", "/points/0/clip")]);
    let codes = edit(&|v| {
        v.as_object_mut().unwrap().remove("parameter");
    });
    assert_eq!(codes_of(&codes), [d("FYN-D0100", "")]);
    let codes = edit(&|v| v["parameter"] = serde_json::json!(""));
    assert_eq!(codes_of(&codes), [d("FYN-D0100", "/parameter")]);
    let codes = edit(&|v| v["sync"] = serde_json::json!("time"));
    assert_eq!(codes_of(&codes), [d("FYN-D0100", "/sync")]);
    // The read view reports what the schema cannot see: a point that is
    // not an object (schema) and, on a lenient parse, its own codes.
    let text = original.replace("\"at\": 1,", "\"at\": 1e999,");
    let doc = Document::parse(fynkel_document::DocumentKind::Blend, None, &text);
    if let Ok(doc) = doc {
        let blend = Blend::from_document(doc);
        assert!(blend
            .read_diagnostics()
            .iter()
            .any(|d| d.code == fynkel_document::Code::BLEND_MALFORMED_ENTRY));
    }
}

/// A `.shader` file (ADR-0031) is an asset with a sidecar: its header and
/// WGSL are checked, and a material names it by asset reference and is
/// validated against its properties. A `.shader.json` manifest is no
/// longer a project document.
#[test]
fn shader_files_are_checked_and_materials_use_them() {
    let t = Temp::new("shaders");
    std::fs::create_dir_all(t.dir.join("assets/shaders")).unwrap();
    let shader = "shader \"Examples/Glow\" {\n    properties {\n        tint color = (1, 0.5, 0.2, 1)\n        power float = 2 range(0, 10)\n    }\n}\n\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    s.emission = material.tint.rgb * material.power;\n    return s;\n}\n";
    t.write("assets/shaders/glow.shader", shader);
    t.write(
        "assets/shaders/glow.shader.meta.json",
        r#"{
  "$schema": "https://fynkel.dev/schemas/meta.schema.json",
  "formatVersion": 2,
  "id": "c0000000-0000-4000-8000-0000000000a1",
  "name": "glow.shader",
  "importer": "fynkel.shader",
  "importerVersion": 1,
  "settings": {},
  "subAssets": {}
}
"#,
    );
    let material = |power: &str| {
        format!(
            r#"{{
  "$schema": "https://fynkel.dev/schemas/material.schema.json",
  "formatVersion": 2,
  "id": "c0000000-0000-4000-8000-0000000000a2",
  "name": "Glow",
  "shader": {{ "asset": "c0000000-0000-4000-8000-0000000000a1" }},
  "parameters": {{ "power": {power} }}
}}
"#
        )
    };
    t.write("assets/materials/glow.material.json", &material("4"));
    assert_eq!(t.codes(), Vec::<(String, Option<String>)>::new());

    // The material is checked against the header's properties.
    t.write("assets/materials/glow.material.json", &material("20"));
    assert_eq!(
        t.codes(),
        vec![("FYN-D0303".to_string(), Some("/parameters/power".to_string()))]
    );
    t.write("assets/materials/glow.material.json", &material("4"));

    // A WGSL mistake is at its line in the file; the material then names
    // a shader that is not there.
    t.write(
        "assets/shaders/glow.shader",
        &shader.replace("material.power", "material.powr"),
    );
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    let d = &report.diagnostics;
    assert_eq!(d[0].code.as_str(), "FYN-D0313", "{d:#?}");
    assert_eq!(d[0].line, Some(10));
    assert!(d[0].document.as_deref().unwrap().ends_with("glow.shader"));
    assert!(d.iter().any(|d| d.code.as_str() == "FYN-D0300"), "{d:#?}");
    t.write("assets/shaders/glow.shader", shader);

    // A manifest is refused, with the header that says the same.
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/material/bad_manifest.shader.json"),
        t.dir.join("assets/shaders/old.shader.json"),
    )
    .unwrap();
    let report = validate_project(&t.dir, &schemas(), Some(&expected_lock()));
    let d: Vec<_> = report.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert_eq!(d.len(), 1, "{d:#?}");
    assert_eq!(d[0].code.as_str(), "FYN-D0015");
    assert!(d[0].suggestion.as_deref().unwrap().contains("a float = 2 range(0, 1)"));
}
