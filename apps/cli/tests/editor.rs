//! `fynkel editor --handover` (ADR-0021 P3c): an editor handing a project
//! over follows the project's status file, so a project that cannot open in
//! its project editor fails there, with why, instead of opening the plain
//! editor.

use std::path::Path;
use std::process::Command;

#[test]
fn a_handover_to_a_project_without_a_game_fails_in_the_status_file() {
    let dir = std::env::temp_dir().join(format!("fynkel-handover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pubspec.yaml"), "name: no_game\n").unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args([
            "editor",
            "--handover",
            "--flutter",
            "/usr/bin/false",
            "--root",
        ])
        .arg(&root)
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let status = std::fs::read_to_string(dir.join(".fynkel/editor_host/status.json")).unwrap();
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["state"], "failed");
    assert_eq!(status["seq"], 2, "`building` came first");
    assert!(
        status["message"]
            .as_str()
            .unwrap()
            .contains("no lib/fynkel_game.dart"),
        "{status}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
