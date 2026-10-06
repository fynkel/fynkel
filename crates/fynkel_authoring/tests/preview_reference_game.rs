//! The edit-preview sync over the reference game's playground (F4 of
//! docs/in-app-checks.md): the editor's stats showed every sync falling
//! back to a full reload on this scene, which the fixture-project tests
//! never saw.

use std::path::{Path, PathBuf};

use fynkel_assets::Assets;
use fynkel_authoring::{AuthoringSession, Preview};
use fynkel_core::{World, WorldConfig};
use serde_json::{json, Value};

fn reference_game() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/reference_game")
        .canonicalize()
        .unwrap()
}

/// The project files the editor reads, without build outputs.
fn copy_project(to: &Path) {
    let from = reference_game();
    std::fs::create_dir_all(to).unwrap();
    for file in ["fynkel.project.json", "fynkel.lock.json"] {
        std::fs::copy(from.join(file), to.join(file)).unwrap();
    }
    copy_dir(&from.join("assets"), &to.join("assets"));
    copy_dir(&from.join("schemas"), &to.join("schemas"));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn a_small_edit_syncs_incrementally_on_the_reference_games_playground() {
    let dir = std::env::temp_dir().join(format!("fynkel-preview-rg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_project(&dir);
    let dir = dir.canonicalize().unwrap();

    let mut s = AuthoringSession::new();
    s.open_project(&dir).map_err(|d| format!("{d:?}")).unwrap();
    let report = s.project().unwrap().cook();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    let assets = Assets::open(&s.project().unwrap().cache_dir()).unwrap();

    let h = s
        .open_document("assets/scenes/playground.scene.json")
        .unwrap();
    let document = s.document(h).unwrap().editor.document().envelope.id;
    let described = s.describe(h).unwrap();
    assert!(!described.has_errors(), "{:?}", described.diagnostics);
    let mut w = World::new(WorldConfig {
        edit_mode: true,
        ..WorldConfig::default()
    });
    let mut p = Preview::load(document, &mut w, Some(&assets), &described.description);
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);

    // No change at all: the world already equals the document. A Skin's
    // skeleton is an object reference by authoring id in the document and
    // a live handle in the world; comparing them raw never matched, the
    // write was refused and every sync reloaded the scene.
    let r = p.sync(&mut w, Some(&assets), &described.description);
    assert!(!r.reloaded, "no-op sync fell back to a reload: {r:?}");
    assert_eq!(
        (r.spawned, r.destroyed, r.updated, r.steps),
        (0, 0, 0, 0),
        "a no-op sync changes nothing"
    );

    // A small edit stays incremental, and the world equals a fresh load.
    let first = described.description.entities[0].id;
    let applied = s
        .apply_json(
            h,
            &json!([{ "command": "renameObject", "id": first.to_string(), "name": "Hero" }]),
        )
        .unwrap();
    assert_eq!(applied["ok"], true, "{applied}");
    let described = s.describe(h).unwrap();
    assert!(!described.has_errors(), "{:?}", described.diagnostics);
    let r = p.sync(&mut w, Some(&assets), &described.description);
    assert!(!r.reloaded, "a rename fell back to a reload: {r:?}");
    assert_eq!(r.updated, 1, "{r:?}");
    let mut fresh_world = World::new(WorldConfig {
        edit_mode: true,
        ..WorldConfig::default()
    });
    let fresh = Preview::load(
        document,
        &mut fresh_world,
        Some(&assets),
        &described.description,
    );
    assert_eq!(
        without_handles(p.dump(&w)),
        without_handles(fresh.dump(&fresh_world))
    );
}

/// The dump prints an object reference as the live handle, which differs
/// between two worlds loaded from the same document; blank the handles.
fn without_handles(value: Value) -> Value {
    match value {
        Value::String(s) if s.starts_with("runtime:") => Value::String("runtime".into()),
        Value::Array(items) => Value::Array(items.into_iter().map(without_handles).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, without_handles(v)))
                .collect(),
        ),
        other => other,
    }
}
