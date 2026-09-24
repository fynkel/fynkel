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
        "lib/fynkel_game.dart",
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
        "assets/audio/jump.ogg",
        "assets/audio/jump.ogg.meta.json",
        "assets/audio/land.ogg",
        "assets/audio/land.ogg.meta.json",
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
    assert!(meta["subAssets"]["mesh/Cube"].is_string());
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
    assert_eq!(t["scene"]["entities"], 6);
    // The crate is a dynamic body on the floor: physics ran headlessly.
    assert_eq!(t["physics"]["bodies"], 2, "{stdout}");
    assert_eq!(t["physics"]["colliders"], 3, "{stdout}");
    assert!(
        t["physics"]["collision_enter"].as_u64().unwrap() >= 1,
        "{stdout}"
    );
    // The cube's AudioSource names jump.ogg (M3-11); the Dart Mover plays
    // it, so headlessly no voice starts.
    assert_eq!(t["audio"]["sources"], 1, "{stdout}");
    assert_eq!(t["audio"]["sounds"], 1, "{stdout}");
    assert_eq!(t["audio"]["diagnostics"], 0, "{stdout}");
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
        // The editor's form (Create → Behaviour): the game's files only.
        let (ok, stdout, stderr) = fynkel(
            &dir,
            &[
                "codegen",
                "--check",
                "--project",
                ".",
                "--project-only",
                "--json",
            ],
        );
        assert!(ok, "{stdout}{stderr}");
        let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(report["native"]["status"], "skipped");
        assert_eq!(report["project"]["status"], "upToDate");
    } else {
        eprintln!("no Dart SDK on PATH; skipping the codegen --check step");
    }
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn codegen_project_only_needs_a_project() {
    let dir = fresh("project-only");
    std::fs::create_dir_all(&dir).unwrap();
    let (ok, _, stderr) = fynkel(&dir, &["codegen", "--project-only"]);
    assert!(!ok);
    assert!(
        stderr.contains("--project-only needs --project"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
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

/// build_runner deletes its outputs before it builds; a run that fails
/// (its error, a lock another build holds) must leave the committed
/// generated files in place — a failed editor check once left the game
/// without them.
#[test]
fn codegen_that_fails_puts_the_generated_files_back() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fresh("codegen-fails");
    std::fs::create_dir_all(dir.join("lib/generated")).unwrap();
    std::fs::create_dir_all(dir.join(".dart_tool")).unwrap();
    std::fs::write(dir.join("pubspec.yaml"), "name: g\n").unwrap();
    std::fs::write(dir.join(".dart_tool/package_config.json"), "{}").unwrap();
    std::fs::write(dir.join("lib/generated/behaviours.g.dart"), "// dart\n").unwrap();
    std::fs::write(dir.join("lib/generated/behaviours.g.json"), "{}\n").unwrap();
    let dart = dir.join("fake-dart");
    std::fs::write(
        &dart,
        "#!/bin/sh\nrm -f lib/generated/behaviours.g.dart lib/generated/behaviours.g.json\necho 'build failed' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&dart, std::fs::Permissions::from_mode(0o755)).unwrap();

    for check in [true, false] {
        let mut args = vec!["codegen", "--project", ".", "--project-only", "--json"];
        if check {
            args.push("--check");
        }
        let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(&args)
            .env("FYNKEL_DART", &dart)
            .current_dir(&dir)
            .output()
            .expect("run fynkel");
        assert!(!out.status.success(), "check={check}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("build failed"),
            "check={check}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("lib/generated/behaviours.g.dart")).unwrap(),
            "// dart\n",
            "check={check}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("lib/generated/behaviours.g.json")).unwrap(),
            "{}\n",
            "check={check}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn codegen_watch_reports_each_build_and_puts_the_files_back_when_it_ends() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;
    let dir = fresh("codegen-watch");
    std::fs::create_dir_all(dir.join("lib/generated")).unwrap();
    std::fs::create_dir_all(dir.join(".dart_tool")).unwrap();
    std::fs::write(dir.join("pubspec.yaml"), "name: g\n").unwrap();
    std::fs::write(dir.join(".dart_tool/package_config.json"), "{}").unwrap();
    // build_runner watch, faked: one good build, then — once `break` exists
    // — a failed one that deletes the outputs, as build_runner does.
    let dart = dir.join("fake-dart");
    std::fs::write(
        &dart,
        r#"#!/bin/sh
echo '// dart' > lib/generated/behaviours.g.dart
echo '{"behaviours":[{"id":"game.A","schema":{"type":"object"}}]}' > lib/generated/behaviours.g.json
echo '  Built with build_runner/aot in 1s; wrote 2 outputs.'
while [ ! -f break ]; do sleep 0.05; done
echo 'Starting build #2.'
rm -f lib/generated/behaviours.g.dart lib/generated/behaviours.g.json
echo 'E fynkel_generator:behaviours on lib/$lib$:'
echo '  12:3: Expected an identifier.'
echo '  0s fynkel_generator:behaviours on 1 input: 1 no-op'
echo '  Failed to build with build_runner/aot in 0s; wrote 0 outputs.'
while true; do sleep 0.05; done
"#,
    )
    .unwrap();
    std::fs::set_permissions(&dart, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(["codegen", "--watch", "--project", "."])
        .env("FYNKEL_DART", &dart)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run fynkel");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut next = || -> serde_json::Value {
        serde_json::from_str(&lines.next().expect("a line").unwrap()).unwrap()
    };
    assert_eq!(next()["event"], "building");
    let built = next();
    assert_eq!(built["event"], "built");
    assert_eq!(built["status"], "written");
    assert_eq!(built["types"], 1);
    assert!(dir.join("schemas/behaviours/game.A.schema.json").is_file());

    std::fs::write(dir.join("break"), "").unwrap();
    assert_eq!(next()["event"], "building");
    let failed = next();
    assert_eq!(failed["event"], "failed");
    assert_eq!(
        failed["error"],
        "E fynkel_generator:behaviours on lib/$lib$:\n12:3: Expected an identifier."
    );
    assert!(!dir.join("lib/generated/behaviours.g.dart").exists());

    // The editor quits: stdin closes, the watch ends, the last good files return.
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    assert_eq!(
        std::fs::read_to_string(dir.join("lib/generated/behaviours.g.dart")).unwrap(),
        "// dart\n"
    );
    assert!(dir.join("lib/generated/behaviours.g.json").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
