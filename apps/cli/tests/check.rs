//! Committed generated files must match the schemas (ADR-0012): this is the
//! `fynkel codegen --check` gate as a test, so drift fails `cargo test`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generated_files_are_up_to_date() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(["codegen", "--check", "--root"])
        .arg(&root)
        .output()
        .expect("run fynkel");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Pipeline (b): the example package's committed generated files and
/// `schemas/behaviours/` match its `@FynkelBehaviour` classes. Needs a Dart
/// SDK (`FYNKEL_DART`, `flutter` or `dart` on PATH) and the pub cache;
/// skipped with a note otherwise so the Rust suite stays runnable alone.
#[test]
fn behaviour_generator_output_is_up_to_date() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example = root.join("packages/fynkel_generator/example/spinner_game");
    let has_dart = std::env::var_os("FYNKEL_DART").is_some()
        || std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path)
                .any(|d| d.join("flutter").is_file() || d.join("dart").is_file())
        });
    if !has_dart {
        eprintln!("no Dart SDK on PATH; skipping behaviour_generator_output_is_up_to_date");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(["codegen", "--check", "--root"])
        .arg(&root)
        .arg("--project")
        .arg(&example)
        .output()
        .expect("run fynkel");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("up to date (2 behaviour types)"),
        "{stdout}"
    );
    // A check never leaves the tree modified.
    assert!(example.join("lib/generated/behaviours.g.dart").is_file());
    assert!(example
        .join("schemas/behaviours/game.Spinner.schema.json")
        .is_file());
}

/// The fixture project validates its `game.Spinner` data against the very
/// schema the generator produced for the example package.
#[test]
fn fixture_project_carries_the_generated_spinner_schema() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let generated = std::fs::read_to_string(
        root.join("packages/fynkel_generator/example/spinner_game/schemas/behaviours/game.Spinner.schema.json"),
    )
    .unwrap();
    let fixture = std::fs::read_to_string(root.join(
        "crates/fynkel_document/fixtures/project/schemas/behaviours/game.Spinner.schema.json",
    ))
    .unwrap();
    assert_eq!(
        generated, fixture,
        "copy the example's schema into the fixture project"
    );
    let v: serde_json::Value = serde_json::from_str(&generated).unwrap();
    assert_eq!(v["x-fynkel"]["kind"], "behaviour");
    assert_eq!(v["properties"]["speed"]["minimum"], 0);
    assert_eq!(v["additionalProperties"], false);
}

#[test]
fn schema_command_prints_fingerprint() {
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(["schema", "--root"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(&format!(
        "fingerprint {:#018x}",
        fynkel_core::schema::schema_fingerprint()
    )));
    assert!(text.contains("fynkel.Transform"));
}

#[test]
fn test_headless_reports_json() {
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args([
            "test",
            "--headless",
            "--steps",
            "30",
            "--seed",
            "3",
            "--satellites",
            "4",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json report");
    assert_eq!(v["tick"], 30);
    assert_eq!(v["live_entities"], 5);
    assert_eq!(v["scene_ready"], true);
    assert!(v["checksum"].as_str().unwrap().len() == 16);
}

/// A second scene for the temp project: a Player with a child Camera, no
/// asset references.
const SECOND_SCENE: &str = r#"{
  "$schema": "https://fynkel.dev/schemas/scene.schema.json",
  "formatVersion": 1,
  "id": "70d7ba8b-f7fb-4eba-920d-ec9e6b968331",
  "name": "Second",
  "entities": {
    "f4d6d600-7931-44f5-a258-45b5a6850300": {
      "name": "Player", "parent": null,
      "children": ["a1b2c3d4-0000-4000-8000-000000000010"],
      "components": {
        "818dc3dc-a9ee-4355-908f-58e20a60ce20": {
          "type": "fynkel.Transform", "version": 1,
          "data": { "position": [0, 1.0, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] }
        }
      }
    },
    "a1b2c3d4-0000-4000-8000-000000000010": {
      "name": "Camera", "parent": "f4d6d600-7931-44f5-a258-45b5a6850300",
      "children": [],
      "components": {
        "818dc3dc-a9ee-4355-908f-58e20a60ce30": {
          "type": "fynkel.Transform", "version": 1,
          "data": { "position": [0, 1.5, 4], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] }
        },
        "818dc3dc-a9ee-4355-908f-58e20a60ce31": {
          "type": "fynkel.Camera", "version": 1,
          "data": { "projection": "perspective", "fov_y": 1.0, "ortho_height": 10, "near": 0.1, "far": 1000, "priority": 0 }
        }
      }
    }
  }
}
"#;

/// The runtime tools read the cooked cache only (M3-03): `fynkel test
/// --headless` and `fynkel state dump` cook the enclosing project first,
/// load by id from the cache, and refuse a scene outside any project.
#[test]
fn scene_documents_load_headlessly_and_dump_by_id() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("fynkel-cli-headless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&root.join("crates/fynkel_document/fixtures/project"), &dir);
    let second = dir.join("assets/scenes/second.scene.json");
    std::fs::write(&second, SECOND_SCENE).unwrap();
    let cache = dir.join("cooked");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let json = |out: &std::process::Output| -> serde_json::Value {
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "{e}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    };

    // The project's startup scene, cooked on the way: the fixture Robot's
    // game.Spinner is known through the project's schemas, prefabs are
    // expanded by the cook (4 entities), and the report names the cache.
    let out = run(&[
        "test",
        "--headless",
        "--steps",
        "3",
        "--json",
        "--project",
        dir.to_str().unwrap(),
        "--cache",
        cache.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(v["scene"]["loaded"], true);
    assert_eq!(v["scene"]["name"], "Main");
    assert_eq!(v["scene"]["path"], "scenes/main.scene.json");
    assert_eq!(v["scene"]["cache"], cache.to_str().unwrap());
    assert_eq!(v["scene"]["entities"], 4);
    assert_eq!(v["scene"]["retained"], 5);
    assert_eq!(v["scene"]["document_diagnostics"], serde_json::json!([]));
    assert_eq!(v["live_entities"], 4);
    assert!(cache.join("index.json").is_file());

    // A scene by path resolves the enclosing project; by id it reads the
    // cache directly (`--no-cook`: nothing changed).
    let out = run(&[
        "test",
        "--headless",
        "--steps",
        "3",
        "--json",
        "--scene",
        second.to_str().unwrap(),
        "--cache",
        cache.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(v["scene"]["name"], "Second");
    assert_eq!(v["scene"]["entities"], 2);
    let out = run(&[
        "state",
        "dump",
        "--json",
        "--no-cook",
        "--cache",
        cache.to_str().unwrap(),
        "--scene",
        "70d7ba8b-f7fb-4eba-920d-ec9e6b968331",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(v["ok"], true);
    assert_eq!(v["scene"]["name"], "Second");
    assert_eq!(
        v["entities"][0]["id"],
        "f4d6d600-7931-44f5-a258-45b5a6850300"
    );
    assert_eq!(v["entities"][1]["world"]["position"][1], 2.5);
    assert!(!out.stdout.windows(7).any(|w| w == b"handle:"));

    // The cook's expansion reaches the runtime: the crate under the
    // instance entity carries a derived id and the override.
    let out = run(&[
        "state",
        "dump",
        "--json",
        "--project",
        dir.to_str().unwrap(),
        "--cache",
        cache.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["entityCount"], 4);
    let crate_entity = &v["entities"][3];
    assert_eq!(
        crate_entity["parent"],
        "c0000000-0000-4000-8000-0000000000e2"
    );
    assert_ne!(crate_entity["id"], "c0000000-0000-4000-8000-0000000000b1");
    assert_eq!(crate_entity["components"][1]["data"]["cast_shadows"], false);

    // A scene that does not validate fails the cook: exit 1, the document
    // diagnostic on stderr, nothing loaded.
    let bad = dir.join("assets/scenes/bad.scene.json");
    std::fs::copy(
        root.join("crates/fynkel_document/fixtures/scene/dangling_parent.scene.json"),
        &bad,
    )
    .unwrap();
    let out = run(&[
        "state",
        "dump",
        "--json",
        "--scene",
        bad.to_str().unwrap(),
        "--cache",
        cache.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("FYN-D0200"), "{err}");
    assert!(err.contains("cooking failed"), "{err}");
    std::fs::remove_file(&bad).unwrap();

    // An asset id the cache does not hold, and a prefab (no scene chunk).
    let out = run(&[
        "test",
        "--headless",
        "--json",
        "--no-cook",
        "--cache",
        cache.to_str().unwrap(),
        "--scene",
        "c0000000-0000-4000-8000-0000000000ff",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        json(&out)["scene"]["document_diagnostics"][0]["code"],
        "FYN-R0003"
    );
    let out = run(&[
        "test",
        "--headless",
        "--json",
        "--no-cook",
        "--cache",
        cache.to_str().unwrap(),
        "--scene",
        "c0000000-0000-4000-8000-000000000040",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        json(&out)["scene"]["document_diagnostics"][0]["code"],
        "FYN-R0006"
    );

    // Loose scenes have no cache to come from: refused, exit 2.
    let loose = root.join("crates/fynkel_document/fixtures/valid/training_room.scene.json");
    let out = run(&[
        "test",
        "--headless",
        "--json",
        "--scene",
        loose.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("outside any project"));
    let out = run(&["state", "dump", "--scene", loose.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    // `--prefabs` is gone.
    let out = run(&[
        "state",
        "dump",
        "--scene",
        second.to_str().unwrap(),
        "--prefabs",
        "x",
    ]);
    assert_eq!(out.status.code(), Some(2));

    // A missing cache says to cook (no project around to cook here).
    let none = dir.join("nope");
    let out = run(&[
        "test",
        "--headless",
        "--json",
        "--cache",
        none.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["scene"]["document_diagnostics"][0]["code"], "FYN-R0001");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn material_and_shader_commands_validate_and_inspect() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("crates/fynkel_document/fixtures");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let rusted = fixtures.join("valid/rusted_metal.material.json");

    let out = run(&["material", "validate", "--json", rusted.to_str().unwrap()]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true);

    let out = run(&["material", "inspect", "--json", rusted.to_str().unwrap()]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["shader"], "fynkel.pbr@1");
    assert_eq!(v["parameters"].as_array().unwrap().len(), 7);
    assert_eq!(v["textures"][3]["slot"], "occlusion");
    assert!(v["textures"][3]["texture"].is_null());
    assert_eq!(v["parameters"][2]["name"], "roughness");
    assert_eq!(v["parameters"][2]["value"], 0.65);
    assert_eq!(v["parameters"][2]["authored"], true);
    assert_eq!(v["doubleSided"], true);
    assert!(v.get("cull").is_none());

    let bad = fixtures.join("material/bad_parameters.material.json");
    let out = run(&["material", "validate", "--json", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0100");
    assert_eq!(v["diagnostics"][0]["pointer"], "/base_colour");
    assert_eq!(v["diagnostics"][3]["code"], "FYN-D0303");

    // A custom manifest via --shader; without it the material's shader is unknown.
    let toon = fixtures.join("material/toon_missing_ramp.material.json");
    let manifest = fixtures.join("material/toon.shader.json");
    let out = run(&["material", "validate", "--json", toon.to_str().unwrap()]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0300");
    let out = run(&[
        "material",
        "validate",
        "--json",
        toon.to_str().unwrap(),
        "--shader",
        manifest.to_str().unwrap(),
    ]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0305");

    let out = run(&["shader", "inspect", "--json", "--builtin", "fynkel.pbr@1"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["shaderId"], "fynkel.pbr@1");
    assert_eq!(v["textures"][2]["name"], "normal");

    let broken = fixtures.join("material/bad_manifest.shader.json");
    let out = run(&["shader", "validate", "--json", broken.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][2]["code"], "FYN-D0311");
}

#[test]
fn prefab_commands_validate_inspect_and_expand() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("crates/fynkel_document/fixtures");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let turret = fixtures.join("valid/turret.prefab.json");
    let out = run(&["prefab", "validate", "--json", turret.to_str().unwrap()]);
    assert!(out.status.success());
    let out = run(&["prefab", "inspect", "--json", turret.to_str().unwrap()]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["kind"], "prefab");
    assert_eq!(v["roots"][0], "10000000-0000-4000-8000-0000000000b1");
    assert_eq!(v["entities"][0]["children"].as_array().unwrap().len(), 1);

    let mismatch = fixtures.join("prefab/roots_mismatch.prefab.json");
    let out = run(&["prefab", "validate", "--json", mismatch.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0400");

    // A scene with instances: prefabs come from --prefabs (default: the
    // scene's directory, which here holds no prefab → D0401).
    let scene = fixtures.join("prefab/instances_ok.scene.json");
    let out = run(&["scene", "validate", "--json", scene.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0401");
    let out = run(&[
        "scene",
        "validate",
        "--json",
        scene.to_str().unwrap(),
        "--prefabs",
        fixtures.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );

    let out = run(&[
        "scene",
        "inspect",
        "--json",
        scene.to_str().unwrap(),
        "--prefabs",
        fixtures.to_str().unwrap(),
    ]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["expanded"], false);
    assert_eq!(v["entityCount"], 3);
    assert_eq!(v["instanceCount"], 2);
    assert_eq!(
        v["roots"][0]["prefab"]["asset"],
        "10000000-0000-4000-8000-000000000001"
    );
    assert_eq!(v["roots"][0]["prefab"]["overrides"], 3);

    let out = run(&[
        "scene",
        "inspect",
        "--json",
        "--expand",
        scene.to_str().unwrap(),
        "--prefabs",
        fixtures.to_str().unwrap(),
    ]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["expanded"], true);
    assert_eq!(v["entityCount"], 7);
    assert_eq!(v["instanceCount"], 0);
    assert_eq!(v["roots"][0]["children"][0]["name"], "Base");
    assert_eq!(
        v["roots"][0]["children"][0]["id"],
        "650fd857-152f-582a-9c45-012b2be17ad8"
    );

    let bad = fixtures.join("prefab/bad_overrides.scene.json");
    let out = run(&[
        "scene",
        "validate",
        "--json",
        bad.to_str().unwrap(),
        "--prefabs",
        fixtures.join("valid").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"].as_array().unwrap().len(), 5);
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0403");
}

#[test]
fn asset_commands_list_resolve_and_init() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("crates/fynkel_document/fixtures/assets");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let assets = fixtures.to_str().unwrap();

    let out = run(&["asset", "list", "--json", "--assets", assets]);
    assert_eq!(out.status.code(), Some(1), "the corpus has errors");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["assets"].as_array().unwrap().len(), 6);
    assert_eq!(v["assets"][2]["path"], "models/robot.glb");
    assert_eq!(v["assets"][2]["importer"], "fynkel.gltf");
    assert_eq!(
        v["assets"][2]["subAssets"]["mesh/Body"],
        "a0000000-0000-4000-8000-000000000011"
    );
    assert_eq!(v["assets"][1]["document"], "material");
    let codes: Vec<&str> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["FYN-A0003", "FYN-A0001", "FYN-A0002", "FYN-A0003"]);

    let out = run(&[
        "asset",
        "resolve",
        "--json",
        "--assets",
        assets,
        "a0000000-0000-4000-8000-000000000001#mesh/Head",
        "a0000000-0000-4000-8000-000000000001#mesh/Nope",
    ]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["resolved"].as_array().unwrap().len(), 1);
    assert_eq!(v["resolved"][0]["path"], "models/robot.glb");
    assert_eq!(v["resolved"][0]["sub"]["id"], "mesh/Head");
    assert!(v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "FYN-A0005"));

    // init: a fresh tree with one source file → sidecar → clean list.
    let dir = std::env::temp_dir().join(format!("fynkel-cli-asset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("textures")).unwrap();
    let png = dir.join("textures/new.png");
    std::fs::write(&png, b"png").unwrap();
    let out = run(&["asset", "init", "--json", png.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["written"][0]["importer"], "fynkel.image");
    let id = v["written"][0]["id"].as_str().unwrap().to_string();
    assert!(dir.join("textures/new.png.meta.json").is_file());
    // Running it again keeps the sidecar (warning, exit 0).
    let out = run(&["asset", "init", "--json", png.to_str().unwrap()]);
    assert!(out.status.success());
    let out = run(&["asset", "list", "--json", "--assets", dir.to_str().unwrap()]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["assets"][0]["id"], id);
    let out = run(&["asset", "resolve", "--assets", dir.to_str().unwrap(), &id]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("textures/new.png"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn migrate_dry_runs_then_writes_with_backup() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("crates/fynkel_document/fixtures/migrate");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let out = run(&["migrate", "--list", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["current"]["scene"], 1);
    assert_eq!(v["current"]["material"], 2);
    assert_eq!(v["steps"][0]["name"], "material/1-2/pbr-fields");
    assert_eq!(v["steps"][1]["name"], "scene/0-1/entities-keyed-by-id");

    let dir = std::env::temp_dir().join(format!("fynkel-cli-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    let scene = dir.join("scenes/room.scene.json");
    std::fs::copy(fixtures.join("v0_training.scene.json"), &scene).unwrap();
    std::fs::copy(
        root.join("crates/fynkel_document/fixtures/valid/rusted_metal.material.json"),
        dir.join("current.material.json"),
    )
    .unwrap();
    std::fs::copy(
        fixtures.join("v1_rusted_metal.material.json"),
        dir.join("legacy.material.json"),
    )
    .unwrap();
    let original = std::fs::read(&scene).unwrap();

    // Dry run over the directory: the diff is reported, nothing written.
    let out = run(&["migrate", "--dry-run", "--json", dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["dryRun"], true);
    let docs = v["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 3);
    assert_eq!(docs[0]["kind"], "material");
    assert_eq!(docs[0]["applied"].as_array().unwrap().len(), 0);
    assert_eq!(docs[1]["kind"], "material");
    assert_eq!(docs[1]["from"], 1);
    assert_eq!(docs[1]["to"], 2);
    assert_eq!(docs[1]["applied"][0]["name"], "material/1-2/pbr-fields");
    assert!(docs[1]["diff"]
        .as_str()
        .unwrap()
        .contains("+  \"base_color_texture\": {"));
    assert_eq!(docs[2]["from"], 0);
    assert_eq!(docs[2]["to"], 1);
    assert_eq!(
        docs[2]["applied"][0]["name"],
        "scene/0-1/entities-keyed-by-id"
    );
    assert!(docs[2]["diff"]
        .as_str()
        .unwrap()
        .contains("+  \"formatVersion\": 1,"));
    assert_eq!(docs[2]["written"], false);
    assert_eq!(std::fs::read(&scene).unwrap(), original);
    assert!(!dir.join("scenes/room.scene.json.bak").exists());

    // Until then the scene is refused everywhere with FYN-D0016.
    let out = run(&["scene", "validate", "--json", scene.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0016");

    // The real run writes and keeps the original next to it.
    let out = run(&["migrate", "--json", dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["documents"][1]["written"], true);
    assert_eq!(
        std::fs::read_to_string(dir.join("legacy.material.json")).unwrap(),
        std::fs::read_to_string(fixtures.join("v1_rusted_metal.material.migrated.json")).unwrap()
    );
    assert_eq!(v["documents"][2]["written"], true);
    let backup = PathBuf::from(v["documents"][2]["backup"].as_str().unwrap());
    assert_eq!(backup, dir.join("scenes/room.scene.json.bak"));
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    assert_eq!(
        std::fs::read_to_string(&scene).unwrap(),
        std::fs::read_to_string(fixtures.join("v0_training.scene.migrated.json")).unwrap()
    );
    let out = run(&["scene", "validate", scene.to_str().unwrap()]);
    assert!(out.status.success());

    // Errors: a document from a newer tool is reported and untouched.
    let newer = dir.join("future.scene.json");
    std::fs::copy(fixtures.join("newer.scene.json"), &newer).unwrap();
    let before = std::fs::read(&newer).unwrap();
    let out = run(&["migrate", "--json", newer.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0013");
    assert_eq!(std::fs::read(&newer).unwrap(), before);
    assert!(!dir.join("future.scene.json.bak").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_writes_intermediate_and_maintains_the_sidecar() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("crates/fynkel_import/fixtures");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let dir = std::env::temp_dir().join(format!("fynkel-cli-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let assets = dir.join("assets");
    std::fs::create_dir_all(assets.join("models")).unwrap();
    let glb = assets.join("models/robot.glb");
    std::fs::copy(fixtures.join("robot.glb"), &glb).unwrap();
    let out_dir = dir.join("cache");

    // No sidecar yet: a dry run reports what would happen and writes nothing.
    let out = run(&[
        "import",
        "--dry-run",
        "--json",
        "--out",
        out_dir.to_str().unwrap(),
        glb.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["dryRun"], true);
    assert_eq!(v["files"][0]["ok"], true);
    assert_eq!(v["files"][0]["sidecarCreated"], true);
    assert_eq!(v["files"][0]["sidecarWritten"], false);
    assert_eq!(
        v["files"][0]["summary"]["meshes"].as_array().unwrap().len(),
        3
    );
    assert!(!assets.join("models/robot.glb.meta.json").exists());
    assert!(!out_dir.exists());

    // The real run creates the sidecar (with every sub-asset id) and the intermediate.
    let out = run(&[
        "import",
        "--json",
        "--out",
        out_dir.to_str().unwrap(),
        glb.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["files"][0]["ok"], true);
    assert_eq!(v["files"][0]["sidecarWritten"], true);
    let id = v["files"][0]["id"].as_str().unwrap().to_string();
    let fyni = out_dir.join(format!("{id}.fyni"));
    assert_eq!(v["files"][0]["intermediate"], fyni.display().to_string());
    let bytes = std::fs::read(&fyni).unwrap();
    assert!(bytes.starts_with(b"FYNI"));
    let sidecar: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(assets.join("models/robot.glb.meta.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(sidecar["id"], id);
    assert_eq!(sidecar["importer"], "fynkel.gltf");
    let subs = sidecar["subAssets"].as_object().unwrap();
    assert_eq!(subs.len(), 8);
    assert!(subs.contains_key("mesh/0") && subs.contains_key("prefab"));
    // The robot's metallic-roughness image is also its base colour: it is
    // imported twice, once per colour space.
    assert!(subs.contains_key("image/0") && subs.contains_key("image/0/linear"));

    // Re-importing is a no-op for the sidecar and reproduces the bytes.
    let out = run(&[
        "import",
        "--json",
        "--out",
        out_dir.to_str().unwrap(),
        glb.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["files"][0]["sidecarCreated"], false);
    assert_eq!(v["files"][0]["sidecarWritten"], false);
    assert_eq!(std::fs::read(&fyni).unwrap(), bytes);

    // The asset database now resolves the sub-assets by the sidecar's ids.
    let out = run(&[
        "asset",
        "resolve",
        "--json",
        "--assets",
        assets.to_str().unwrap(),
        &format!("{id}#mesh/0"),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["resolved"][0]["sub"]["id"], "mesh/0");

    // An external buffer is refused (exit 1, FYN-A0102) and nothing is written.
    std::fs::copy(
        fixtures.join("external.gltf"),
        assets.join("models/external.gltf"),
    )
    .unwrap();
    std::fs::copy(fixtures.join("robot.bin"), assets.join("models/robot.bin")).unwrap();
    let out = run(&[
        "import",
        "--json",
        "--out",
        out_dir.to_str().unwrap(),
        assets.join("models/external.gltf").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["files"][0]["ok"], false);
    assert_eq!(v["diagnostics"][0]["code"], "FYN-A0102");
    assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 1);

    // Textures and audio go through the same command; several files at once.
    std::fs::copy(
        fixtures.join("textures/swatch.png"),
        assets.join("swatch.png"),
    )
    .unwrap();
    std::fs::copy(fixtures.join("audio/tone.ogg"), assets.join("tone.ogg")).unwrap();
    let out = run(&[
        "import",
        "--json",
        "--out",
        out_dir.to_str().unwrap(),
        assets.join("swatch.png").to_str().unwrap(),
        assets.join("tone.ogg").to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["importer"], "fynkel.image");
    assert_eq!(files[0]["sidecarCreated"], true);
    assert_eq!(files[0]["summary"]["textures"][0]["width"], 4);
    assert_eq!(files[0]["summary"]["textures"][0]["colorSpace"], "srgb");
    assert_eq!(files[1]["importer"], "fynkel.audio");
    assert_eq!(files[1]["summary"]["clips"][0]["codec"], "vorbis");
    assert_eq!(files[1]["summary"]["clips"][0]["frames"], 400);
    assert!(assets.join("swatch.png.meta.json").exists());
    assert!(assets.join("tone.ogg.meta.json").exists());
    assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 3);

    // Usage.
    let out = run(&["import"]);
    assert_eq!(out.status.code(), Some(2));

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cook_fills_the_cache_and_cooked_inspect_reads_it() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let assets = root.join("crates/fynkel_import/fixtures/project/assets");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let cache = std::env::temp_dir().join(format!("fynkel-cli-cook-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let cook = |extra: &[&str]| {
        let mut args = vec![
            "cook",
            "--json",
            "--assets",
            assets.to_str().unwrap(),
            "--cache",
            cache.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let out = run(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v
    };
    let first = cook(&[]);
    assert_eq!(first["profile"], "dev");
    assert_eq!(first["cooked"], 6);
    assert_eq!(first["cached"], 0);
    assert_eq!(first["failed"], 0);
    assert!(cache.join("index.json").is_file());
    let second = cook(&[]);
    assert_eq!(second["cached"], 6);
    let ios = cook(&["--profile", "ios"]);
    assert_eq!(ios["cooked"], 6);

    // Inspect the scene's container.
    let scene = first["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["path"] == "scenes/main.scene.json")
        .unwrap();
    let file = scene["file"].as_str().unwrap();
    let out = run(&["cooked", "inspect", file]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("checksums ok"), "{text}");
    assert!(text.contains("[1] JSON"), "{text}");
    let out = run(&["cooked", "inspect", "--json", file]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["files"][0]["verified"], true);
    assert_eq!(v["files"][0]["meta"]["kind"], "document/scene");
    assert_eq!(v["files"][0]["chunks"].as_array().unwrap().len(), 2);

    // A damaged container is reported, exit 1.
    let mut bytes = std::fs::read(file).unwrap();
    bytes[60] ^= 0xff; // inside the META payload
    let damaged = cache.join("damaged.fyna");
    std::fs::write(&damaged, &bytes).unwrap();
    let out = run(&["cooked", "inspect", damaged.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));

    // Pruning after an ios cook drops the dev objects.
    let pruned = cook(&["--profile", "ios", "--prune"]);
    assert_eq!(pruned["pruned"], 6);
    assert_eq!(cook(&[])["cooked"], 6);
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn validate_project_describe_lock_and_schema_commands_fix_their_exit_codes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = root.join("crates/fynkel_document/fixtures/project");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let json = |out: &std::process::Output| -> serde_json::Value {
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "{e}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    };

    // 0: the fixture project is clean.
    let out = run(&["validate", "--project", fixture.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let v = json(&out);
    assert_eq!(v["ok"], true);
    assert_eq!(v["project"]["name"], "Fixture Game");
    assert_eq!(v["documents"], 3);
    assert_eq!(v["sources"], 2);
    assert_eq!(v["references"], 6);
    assert_eq!(v["lock"]["cache"]["container"], 1);
    let out = run(&["validate", "--project", fixture.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("0 error(s), 0 warning(s)"));

    // 2: no project file at all, or bad usage.
    let empty = std::env::temp_dir().join(format!("fynkel-cli-validate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    let out = run(&["validate", "--project", empty.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(json(&out)["diagnostics"][0]["code"], "FYN-P0001");
    let out = run(&["validate", "--bogus"]);
    assert_eq!(out.status.code(), Some(2));
    let out = run(&["project"]);
    assert_eq!(out.status.code(), Some(2));

    // 1: a copy with a stale lock and a dangling startup scene.
    let broken = empty.join("broken");
    copy_dir(&fixture, &broken);
    let lock_path = broken.join("fynkel.lock.json");
    let lock = std::fs::read_to_string(&lock_path).unwrap();
    std::fs::write(
        &lock_path,
        lock.replace("\"abiVersion\": 1", "\"abiVersion\": 99"),
    )
    .unwrap();
    let project_path = broken.join("fynkel.project.json");
    let project = std::fs::read_to_string(&project_path).unwrap();
    std::fs::write(
        &project_path,
        project.replace(
            "c0000000-0000-4000-8000-000000000030",
            "c0000000-0000-4000-8000-0000000000ff",
        ),
    )
    .unwrap();
    let out = run(&["validate", "--project", broken.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["ok"], false);
    let codes: Vec<&str> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["FYN-P0011", "FYN-P0002"], "{v}");

    // project lock --check sees the stale lock (1); project lock rewrites it (0).
    let out = run(&[
        "project",
        "lock",
        "--check",
        "--project",
        broken.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("/runtime/abiVersion"));
    let out = run(&[
        "project",
        "lock",
        "--project",
        broken.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out)["written"], true);
    assert_eq!(
        std::fs::read_to_string(&lock_path).unwrap(),
        lock,
        "the rewritten lock is byte-identical to the fixture's"
    );
    let out = run(&[
        "project",
        "lock",
        "--check",
        "--project",
        broken.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0));

    // project describe --json.
    let out = run(&[
        "project",
        "describe",
        "--project",
        fixture.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let v = json(&out);
    assert_eq!(v["ok"], true);
    assert_eq!(v["startupScene"]["path"], "scenes/main.scene.json");
    assert_eq!(v["assetCount"], 5);
    assert_eq!(v["quality"]["default"], "medium");
    assert_eq!(v["targets"].as_array().unwrap().len(), 3);
    let out = run(&[
        "project",
        "describe",
        "--project",
        fixture.to_str().unwrap(),
    ]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("project Fixture Game"));

    // schema list / describe.
    let out = run(&["schema", "list", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let v = json(&out);
    assert_eq!(
        v["fingerprint"],
        format!("{:#018x}", fynkel_core::schema::schema_fingerprint())
    );
    assert!(v["documents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["kind"] == "project"));
    assert!(v["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["name"] == "fynkel.Transform" && r["wireSize"] == 40));
    let out = run(&["schema", "describe", "fynkel.Transform", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let v = json(&out);
    assert_eq!(v["kind"], "component");
    assert_eq!(v["id"], 1);
    assert_eq!(v["fields"][1]["name"], "rotation");
    assert_eq!(v["fields"][1]["offset"], 12);
    assert_eq!(
        v["schema"],
        "https://fynkel.dev/schemas/components/fynkel.Transform.schema.json"
    );
    let out = run(&["schema", "describe", "scene", "--json"]);
    assert_eq!(
        json(&out)["schema"],
        "https://fynkel.dev/schemas/scene.schema.json"
    );
    let out = run(&["schema", "describe", "fynkel.Nope", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let out = run(&["schema", "describe"]);
    assert_eq!(out.status.code(), Some(2));
    // The bare command still prints the registry.
    let out = run(&["schema"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("fynkel.Transform"));

    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn scene_apply_dry_runs_conflicts_and_writes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("crates/fynkel_document/fixtures");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let dir = std::env::temp_dir().join(format!("fynkel-cli-apply-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scene = dir.join("edit.scene.json");
    std::fs::copy(fixtures.join("scene/hierarchy_ok.scene.json"), &scene).unwrap();
    let original = std::fs::read_to_string(&scene).unwrap();
    let scene_s = scene.to_str().unwrap();

    // The revision comes from `inspect --json`.
    let out = run(&["scene", "inspect", "--json", scene_s]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let revision = v["revision"].as_str().unwrap().to_string();
    assert_eq!(revision.len(), 64);

    // Usage errors and unreadable patches exit 2.
    assert_eq!(run(&["scene", "apply", scene_s]).status.code(), Some(2));
    let patch = dir.join("ops.json");
    std::fs::write(&patch, "not json").unwrap();
    let out = run(&[
        "scene",
        "apply",
        scene_s,
        "--patch",
        patch.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-E0004");
    std::fs::write(&patch, r#"[{"command": "warp"}]"#).unwrap();
    let out = run(&[
        "scene",
        "apply",
        scene_s,
        "--patch",
        patch.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(2));

    // A dry run prints the diff and the mutations, writes nothing.
    let ops = serde_json::json!({
        "baseRevision": revision,
        "commands": [
            {"command": "renameObject", "id": "aaaaaaaa-0000-4000-8000-000000000001", "name": "Renamed"},
            {"command": "setField", "entity": "aaaaaaaa-0000-4000-8000-000000000001",
             "component": "11111111-0000-4000-8000-000000000001", "field": "/position/2", "value": 4}
        ]
    });
    std::fs::write(&patch, ops.to_string()).unwrap();
    let out = run(&[
        "scene",
        "apply",
        scene_s,
        "--patch",
        patch.to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["dryRun"], true);
    assert_eq!(v["written"], false);
    assert_eq!(v["baseRevision"], revision);
    assert_ne!(v["revision"], revision);
    assert_eq!(v["mutations"].as_array().unwrap().len(), 2);
    assert_eq!(v["mutations"][0]["command"], "renameObject");
    assert_eq!(v["mutations"][0]["undo"][0]["value"], "Root");
    assert_eq!(
        v["mutations"][1]["affectedIds"].as_array().unwrap().len(),
        2
    );
    let diff = v["diff"].as_str().unwrap();
    assert!(diff.contains("+      \"name\": \"Renamed\""), "{diff}");
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), original);
    let out = run(&[
        "scene",
        "apply",
        scene_s,
        "--patch",
        patch.to_str().unwrap(),
        "--dry-run",
    ]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("-      \"name\": \"Root\""));
    assert!(String::from_utf8_lossy(&out.stderr).contains("dry run"));

    // For real: the file changes and the old revision is now stale.
    let out = run(&[
        "scene",
        "apply",
        scene_s,
        "--patch",
        patch.to_str().unwrap(),
        "--json",
    ]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["written"], true);
    let new_revision = v["revision"].as_str().unwrap().to_string();
    let written = std::fs::read_to_string(&scene).unwrap();
    assert!(written.contains("\"Renamed\""));
    let out = run(&["scene", "inspect", "--json", scene_s]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["revision"], new_revision);
    let out = run(&[
        "scene",
        "apply",
        scene_s,
        "--patch",
        patch.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["diagnostics"][0]["code"], "FYN-E0001");
    assert!(v["mutations"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), written);

    // A transaction that would leave the document invalid is refused whole.
    std::fs::write(
        &patch,
        r#"[{"command": "setField", "entity": "aaaaaaaa-0000-4000-8000-000000000001",
             "component": "11111111-0000-4000-8000-000000000001", "field": "position", "value": "x"}]"#,
    )
    .unwrap();
    let out = run(&[
        "scene",
        "apply",
        scene_s,
        "--patch",
        patch.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-E0005");
    assert_eq!(v["diagnostics"][0]["related"][0]["code"], "FYN-D0100");
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), written);

    // Prefab documents go through the same path.
    let prefab = dir.join("turret.prefab.json");
    std::fs::copy(fixtures.join("valid/turret.prefab.json"), &prefab).unwrap();
    std::fs::write(
        &patch,
        r#"[{"command": "createObject", "name": "Antenna", "parent": null,
             "components": [{"type": "fynkel.Transform", "data": {"position": [0, 1, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1]}}]}]"#,
    )
    .unwrap();
    let out = run(&[
        "prefab",
        "apply",
        prefab.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = run(&["prefab", "inspect", "--json", prefab.to_str().unwrap()]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["roots"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
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
