//! `fynkel create` (blueprint §19.2c item 17): fresh output is a complete
//! project that `fynkel validate` and `fynkel test --headless` accept, with
//! the behaviour's generated files matching what the generator emits.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fynkel(cwd: &Path, args: &[&str]) -> (bool, String, String) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(args)
        .arg("--root")
        .arg(&root)
        .current_dir(cwd)
        .output()
        .expect("run fynkel");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn fresh(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fynkel-create-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_fresh_project_validates_and_runs_headlessly() {
    let parent = fresh("gate");
    std::fs::create_dir_all(&parent).unwrap();
    let dir = parent.join("my_game");
    let (ok, stdout, stderr) = fynkel(&parent, &["create", "my_game", "--no-flutter", "--json"]);
    assert!(ok, "{stdout}{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["package"], "my_game");
    assert_eq!(report["title"], "My Game");
    assert_eq!(report["flutter"], false);
    let files: Vec<&str> = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    for expected in [
        "pubspec.yaml",
        "fynkel.project.json",
        "fynkel.lock.json",
        "AGENTS.md",
        "README.md",
        "lib/main.dart",
        "lib/game/game.dart",
        "lib/game/spinner.dart",
        "lib/generated/behaviours.g.dart",
        "lib/generated/behaviours.g.json",
        "schemas/behaviours/game.Spinner.schema.json",
        "assets/scenes/main.scene.json",
        "assets/materials/starter.material.json",
        "assets/models/cube.glb",
        "assets/models/cube.glb.meta.json",
        "assets/textures/swatch.png",
        "assets/textures/swatch.png.meta.json",
        "test/game_test.dart",
    ] {
        assert!(
            files.contains(&expected),
            "{expected} missing from {files:?}"
        );
        assert!(dir.join(expected).is_file(), "{expected} not written");
    }
    // No placeholder survived.
    for f in &files {
        if let Ok(text) = std::fs::read_to_string(dir.join(f)) {
            assert!(!text.contains("{{"), "{f} still has a placeholder");
        }
    }
    // The sidecar carries the imported sub-assets the scene references.
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("assets/models/cube.glb.meta.json")).unwrap(),
    )
    .unwrap();
    assert!(meta["subAssets"]["mesh/0"].is_string());
    let scene = std::fs::read_to_string(dir.join("assets/scenes/main.scene.json")).unwrap();
    assert!(scene.contains(meta["id"].as_str().unwrap()));
    assert!(scene.contains("\"game.Spinner\""));
    // The pubspec points at the checkout's SDK packages.
    let pubspec = std::fs::read_to_string(dir.join("pubspec.yaml")).unwrap();
    assert!(pubspec.contains("name: my_game"));
    assert!(pubspec.contains("packages/fynkel\n") || pubspec.contains("packages/fynkel\r\n"));
    assert!(pubspec.contains("packages/fynkel_flutter"));
    assert!(pubspec.contains("packages/fynkel_generator"));

    // The M2-17 gate: validate and the headless host, from inside the
    // project, no flags.
    let (ok, stdout, stderr) = fynkel(&dir, &["validate", "--json"]);
    assert!(ok, "{stdout}{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["errors"], 0, "{stdout}");
    assert_eq!(v["warnings"], 0, "{stdout}");

    let (ok, stdout, stderr) = fynkel(&dir, &["test", "--headless", "--steps", "120", "--json"]);
    assert!(ok, "{stdout}{stderr}");
    let t: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(t["scene"]["loaded"], true, "{stdout}");
    assert_eq!(t["scene"]["entities"], 3);
    assert_eq!(t["scene"]["name"], "Main");
    assert_eq!(t["tick"], 120);
    assert_eq!(t["diagnostics"], 0);
    assert_eq!(
        t["scene"]["document_diagnostics"].as_array().unwrap().len(),
        0,
        "{stdout}"
    );

    // The project's own schema makes `game.Spinner` a known type for the
    // standalone scene commands too.
    let (ok, stdout, stderr) = fynkel(
        &dir,
        &[
            "scene",
            "validate",
            "assets/scenes/main.scene.json",
            "--json",
        ],
    );
    assert!(ok, "{stdout}{stderr}");
    let s: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(s["diagnostics"].as_array().unwrap().is_empty(), "{stdout}");

    let (ok, stdout, stderr) = fynkel(&dir, &["state", "dump", "--steps", "2", "--json"]);
    assert!(ok, "{stdout}{stderr}");
    assert!(stdout.contains("\"Cube\""));

    // The starter's recorded input stream replays against its actions
    // (M3-07): move held while W/D/the stick are, two jump presses.
    let (ok, stdout, stderr) = fynkel(
        &dir,
        &[
            "test",
            "--headless",
            "--input",
            "test/walk.input.json",
            "--json",
        ],
    );
    assert!(ok, "{stdout}{stderr}");
    let t: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let input = &t["input"];
    assert_eq!(input["frames"], 70, "{stdout}");
    assert_eq!(input["events"], 11);
    assert!(input["binding_errors"].as_array().unwrap().is_empty());
    let actions = input["actions"].as_array().unwrap();
    assert_eq!(actions[0]["name"], "move");
    assert_eq!(actions[0]["held_ticks"], 11);
    assert_eq!(actions[1]["name"], "jump");
    assert_eq!(actions[1]["pressed"], 2);
    assert_eq!(actions[1]["released"], 2);
    assert_eq!(actions[2]["name"], "pause");
    assert_eq!(actions[2]["pressed"], 0);

    // The committed generated files are exactly what the generator emits
    // (needs a Dart SDK; skipped otherwise).
    let has_dart = std::env::var_os("FYNKEL_DART").is_some()
        || std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path)
                .any(|d| d.join("flutter").is_file() || d.join("dart").is_file())
        });
    if has_dart {
        let (ok, stdout, stderr) = fynkel(&dir, &["codegen", "--check", "--project", "."]);
        assert!(ok, "{stdout}{stderr}");
        assert!(
            stdout.contains("up to date (2 behaviour types)"),
            "{stdout}"
        );
    } else {
        eprintln!("no Dart SDK on PATH; skipping the codegen --check step");
    }
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn create_refuses_bad_names_and_non_empty_directories() {
    let parent = fresh("refuse");
    std::fs::create_dir_all(&parent).unwrap();
    let (ok, _, stderr) = fynkel(&parent, &["create", "My-Game", "--no-flutter"]);
    assert!(!ok);
    assert!(stderr.contains("not a valid package name"), "{stderr}");
    assert!(!parent.join("My-Game").exists());

    let taken = parent.join("taken");
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("note.txt"), "x").unwrap();
    let (ok, _, stderr) = fynkel(&parent, &["create", "taken", "--no-flutter"]);
    assert!(!ok);
    assert!(stderr.contains("not empty"), "{stderr}");
    assert!(!taken.join("pubspec.yaml").exists());

    // --name and --title override the directory-derived defaults.
    let (ok, stdout, stderr) = fynkel(
        &parent,
        &[
            "create",
            "somewhere",
            "--name",
            "space_game",
            "--title",
            "Space!",
            "--no-flutter",
            "--json",
        ],
    );
    assert!(ok, "{stdout}{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["package"], "space_game");
    assert_eq!(report["title"], "Space!");
    let project = std::fs::read_to_string(parent.join("somewhere/fynkel.project.json")).unwrap();
    assert!(project.contains("\"name\": \"Space!\""));
    let generated =
        std::fs::read_to_string(parent.join("somewhere/lib/generated/behaviours.g.dart")).unwrap();
    assert!(generated.contains("package:space_game/game/spinner.dart"));
    let _ = std::fs::remove_dir_all(&parent);
}
