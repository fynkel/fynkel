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
use fynkel_document::{validate_project, AssetDatabase, Lock, Project, SchemaSet};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/project")
}

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

const IMPORTERS: &[(&str, u32)] = &[
    ("fynkel.gltf", 1),
    ("fynkel.image", 1),
    ("fynkel.audio", 1),
    ("fynkel.file", 1),
];

/// What `fynkel_import::expected_lock()` produces; mirrored here because
/// this crate cannot depend on the importers.
fn expected_lock() -> Lock {
    Lock::expected(IMPORTERS, 1, 1)
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
    assert_eq!(project.components.as_ref().unwrap().len(), 6);
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
        (3, 2, 6)
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
    assert_eq!(v["assetCount"], 5);
    assert_eq!(v["inputActions"].as_array().unwrap().len(), 3);
    assert_eq!(v["quality"]["default"], "medium");
    assert_eq!(v["components"].as_array().unwrap().len(), 6);
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
    assert_eq!(report.documents, 3);
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
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].0, "FYN-P0004");
    assert!(warnings[0].1.ends_with("main.scene.json"));
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
