//! Play cooks what the editor holds (ADR-0021 P5): a scene with unsaved
//! edits cooks as the editor has it, its file is left alone, a recovery
//! copy of the unsaved text is written under `.fynkel/recovery/`, and the
//! next cook from disk cooks the file again.

use std::path::{Path, PathBuf};

use fynkel_authoring::AuthoringSession;
use fynkel_import::Status;
use serde_json::json;

const MAIN: &str = "assets/scenes/main.scene.json";
const MAIN_ID: &str = "c0000000-0000-4000-8000-000000000030";
const FLOOR: &str = "c0000000-0000-4000-8000-0000000000e4";
const FLOOR_TRANSFORM: &str = "c0000000-0000-4000-8000-0000000000d0";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
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

#[test]
fn play_cooks_unsaved_documents_and_leaves_their_files_alone() {
    let dir = std::env::temp_dir().join(format!("fynkel-play-cook-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&fixture(), &dir);
    let dir = dir.canonicalize().unwrap();

    let mut s = AuthoringSession::new();
    s.open_project(&dir).map_err(|d| format!("{d:?}")).unwrap();
    let first = s.project().unwrap().cook();
    assert!(!first.has_errors(), "{:?}", first.diagnostics);
    let key = |r: &fynkel_import::CookReport| {
        let a = r
            .assets
            .iter()
            .find(|a| a.id.to_string() == MAIN_ID)
            .unwrap();
        (a.key, a.status)
    };

    let h = s.open_document(MAIN).unwrap();
    assert!(s.unsaved_texts().is_empty(), "nothing unsaved yet");
    let applied = s
        .apply_json(
            h,
            &json!([{ "command": "setField", "entity": FLOOR, "component": FLOOR_TRANSFORM, "field": "position", "value": [0, -1.5, 0] }]),
        )
        .unwrap();
    assert_eq!(applied["ok"], true, "{applied}");

    let file = dir.join(MAIN);
    let on_disk = std::fs::read_to_string(&file).unwrap();
    let texts = s.unsaved_texts();
    assert_eq!(texts.keys().collect::<Vec<_>>(), [&file]);
    assert!(texts[&file].contains("-1.5"));

    let project = s.project().unwrap();
    assert_eq!(project.write_recovery(&texts), 1);
    assert_eq!(
        std::fs::read_to_string(dir.join(".fynkel/recovery").join(MAIN)).unwrap(),
        texts[&file]
    );
    let played = project.cook_live(texts);
    assert!(!played.has_errors(), "{:?}", played.diagnostics);
    assert_eq!(key(&played).1, Status::Cooked, "the unsaved scene cooks");
    assert_ne!(key(&played).0, key(&first).0);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        on_disk,
        "the file is not written"
    );

    // From disk again: the file's own object, already in the cache.
    let again = project.cook();
    assert_eq!(key(&again), (key(&first).0, Status::Cached));

    let _ = std::fs::remove_dir_all(&dir);
}
