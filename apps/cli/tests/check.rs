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
        stdout.contains("up to date (6 behaviour types, 1 data type)"),
        "{stdout}"
    );
    // A check never leaves the tree modified.
    assert!(example.join("lib/generated/behaviours.g.dart").is_file());
    assert!(example
        .join("schemas/data/game.Weapon.schema.json")
        .is_file());
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
  "formatVersion": 7,
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
          "data": { "projection": "perspective", "fov_y": 1.0, "ortho_height": 10, "near": 0.1, "far": 1000, "priority": 0, "exposure_mode": "auto", "exposure": 12.0, "culling_mask": 4294967295 }
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
    assert_eq!(v["scene"]["entities"], 6);
    assert_eq!(v["scene"]["retained"], 8);
    assert_eq!(v["scene"]["document_diagnostics"], serde_json::json!([]));
    assert_eq!(v["live_entities"], 6);
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
    assert_eq!(v["entityCount"], 6);
    // Crate A is the crate prefab's root, with its components and the
    // instance override applied.
    let crate_a = v["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "c0000000-0000-4000-8000-0000000000e2")
        .unwrap();
    assert!(!v
        .to_string()
        .contains("c0000000-0000-4000-8000-0000000000b1"));
    let renderer = crate_a["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "fynkel.MeshRenderer")
        .unwrap();
    assert_eq!(renderer["data"]["cast_shadows"], false);

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

    // A .shader via --shader (its id from the sidecar beside it); without
    // it the material's shader is unknown.
    let toon = fixtures.join("material/toon_missing_ramp.material.json");
    let manifest = fixtures.join("material/toon.shader");
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

    // A .shader: its header and interface (ADR-0031).
    let out = run(&["shader", "inspect", "--json", manifest.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["shaderName"], "Studio/Toon");
    assert_eq!(v["shaderId"], "be5b7a9c-6f8b-4dae-9ac1-5b6d7e8f9a01");
    assert_eq!(v["lighting"], "lit");
    assert_eq!(v["passes"], serde_json::json!(["opaque", "shadow"]));
    assert_eq!(v["parameters"][0]["name"], "bands");
    assert_eq!(v["textures"][0]["name"], "ramp");
    let out = run(&["shader", "validate", manifest.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // A WGSL mistake is at its line in the file.
    let dir = std::env::temp_dir().join(format!("fynkel-shader-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("bad.shader");
    std::fs::write(
        &bad,
        "shader \"Bad\" {\n}\n\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    s.base_colour = vec3<f32>(1.0);\n    return s;\n}\n",
    )
    .unwrap();
    let out = run(&["shader", "validate", "--json", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0313");
    assert_eq!(v["diagnostics"][0]["line"], 6);
    let _ = std::fs::remove_dir_all(&dir);

    // A .shader.json manifest is refused, with the header that says the same.
    let old = fixtures.join("material/bad_manifest.shader.json");
    let out = run(&["shader", "validate", "--json", old.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0015");
    assert!(v["diagnostics"][0]["suggestion"]
        .as_str()
        .unwrap()
        .contains("shader \"Bad manifest\" {"));
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
    assert_eq!(v["root"], "10000000-0000-4000-8000-0000000000b1");
    assert_eq!(v["entities"][0]["children"].as_array().unwrap().len(), 1);

    let mismatch = fixtures.join("prefab/root_mismatch.prefab.json");
    let out = run(&["prefab", "validate", "--json", mismatch.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0400");

    // A scene with instances: prefabs come from --prefabs (default: the
    // scene's directory, which here holds no prefab → D0401, a warning
    // since foundation 5.1: the instance stays as an empty placeholder).
    let scene = fixtures.join("prefab/instances_ok.scene.json");
    let out = run(&["scene", "validate", "--json", scene.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0401");
    assert_eq!(v["diagnostics"][0]["severity"], "warning");
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
    assert_eq!(v["roots"][0]["prefab"]["overrides"], 4);

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
    assert_eq!(v["entityCount"], 5);
    assert_eq!(v["instanceCount"], 0);
    // Turret A is the turret's root (Base); the Barrel hangs off it.
    assert_eq!(v["roots"][0]["name"], "Turret A");
    assert_eq!(v["roots"][0]["children"][0]["name"], "Barrel");
    assert_eq!(
        v["roots"][0]["children"][0]["id"],
        "cd1ea7dd-840a-51fc-ab33-dc6bc8e18d47"
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

/// `fynkel asset remap` / `extract-materials`
/// (`docs/design/material-remapping.md`) on a copy of the import corpus.
#[test]
fn asset_remap_and_extract_materials_edit_the_models_sidecar() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("fynkel-cli-remap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&root.join("crates/fynkel_import/fixtures/project"), &dir);
    let assets = dir.join("assets");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--assets")
            .arg(&assets)
            .arg("--root")
            .arg(&root)
            .current_dir(&dir)
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
    let sidecar = assets.join("models/robot.glb.meta.json");
    let original = std::fs::read_to_string(&sidecar).unwrap();

    // By file, to a material by file; then back to the embedded one.
    let out = run(&[
        "asset",
        "remap",
        "--json",
        "assets/models/robot.glb",
        "material/Painted",
        "assets/materials/rusted.material.json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        json(&out)["settings"]["materials"]["material/Painted"]["asset"],
        "c0000000-0000-4000-8000-000000000020"
    );
    let out = run(&[
        "asset",
        "remap",
        "c0000000-0000-4000-8000-000000000001",
        "material/Painted",
        "none",
    ]);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&sidecar).unwrap(), original);

    // Refusals: a key the model lacks (the keys it has are named), a
    // target that is not a material.
    let out = run(&[
        "asset",
        "remap",
        "--json",
        "assets/models/robot.glb",
        "material/Chrome",
        "none",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["diagnostics"][0]["code"], "FYN-A0005");
    assert!(v["diagnostics"][0]["suggestion"]
        .as_str()
        .unwrap()
        .contains("material/Glass"));
    let out = run(&[
        "asset",
        "remap",
        "--json",
        "assets/models/robot.glb",
        "material/Painted",
        "assets/textures/swatch.png",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["diagnostics"][0]["code"], "FYN-A0007");

    // Extract: documents in models/Materials, each remapped.
    let out = run(&[
        "asset",
        "extract-materials",
        "--json",
        "assets/models/robot.glb",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    let materials = v["materials"].as_array().unwrap();
    assert_eq!(materials.len(), 2);
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sidecar).unwrap()).unwrap();
    for m in materials {
        let path = PathBuf::from(m["path"].as_str().unwrap());
        assert!(
            path.starts_with(assets.join("models/Materials")),
            "{path:?}"
        );
        assert!(path.is_file());
        assert_eq!(
            meta["settings"]["materials"][m["key"].as_str().unwrap()]["asset"],
            m["id"]
        );
    }
    // The project still validates and cooks with the copies.
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(["cook", "--json", "--assets"])
        .arg(&assets)
        .arg("--cache")
        .arg(dir.join("cache"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // Nothing left to extract.
    let out = run(&["asset", "extract-materials", "assets/models/robot.glb"]);
    assert_eq!(out.status.code(), Some(1));
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
    assert_eq!(v["current"]["scene"], 7);
    assert_eq!(v["current"]["prefab"], 7);
    assert_eq!(v["current"]["project"], 2);
    assert_eq!(v["current"]["material"], 2);
    assert_eq!(v["current"]["meta"], 2);
    let names: Vec<&str> = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "material/1-2/pbr-fields",
            "meta/1-2/name-keys",
            "prefab/1-2/camera-exposure",
            "prefab/2-3/animator-layers",
            "prefab/3-4/object-layers",
            "prefab/4-5/one-root",
            "prefab/5-6/rigidbody-constraints",
            "prefab/6-7/object-references",
            "project/1-2/layers",
            "scene/0-1/entities-keyed-by-id",
            "scene/1-2/camera-exposure",
            "scene/2-3/animator-layers",
            "scene/3-4/object-layers",
            "scene/4-5/one-root",
            "scene/5-6/rigidbody-constraints",
            "scene/6-7/object-references"
        ]
    );

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
    assert_eq!(docs[2]["to"], 7);
    assert_eq!(
        docs[2]["applied"][0]["name"],
        "scene/0-1/entities-keyed-by-id"
    );
    assert_eq!(docs[2]["applied"][1]["name"], "scene/1-2/camera-exposure");
    assert_eq!(docs[2]["applied"][2]["name"], "scene/2-3/animator-layers");
    assert_eq!(docs[2]["applied"][3]["name"], "scene/3-4/object-layers");
    assert_eq!(docs[2]["applied"][4]["name"], "scene/4-5/one-root");
    assert_eq!(
        docs[2]["applied"][5]["name"],
        "scene/5-6/rigidbody-constraints"
    );
    assert_eq!(docs[2]["applied"][6]["name"], "scene/6-7/object-references");
    assert!(docs[2]["diff"]
        .as_str()
        .unwrap()
        .contains("+  \"formatVersion\": 7,"));
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
    assert!(subs.contains_key("mesh/Cube") && subs.contains_key("prefab"));
    // The robot's metallic-roughness image is also its base colour: it is
    // imported twice, once per colour space.
    assert!(subs.contains_key("image/paint") && subs.contains_key("image/paint/linear"));

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
        &format!("{id}#mesh/Cube"),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["resolved"][0]["sub"]["id"], "mesh/Cube");

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

    // `--prefab` (M3-19a) writes the hierarchy as an editable prefab
    // document next to the source: a fresh id, the importer's entity ids,
    // valid as a document; a second run refuses to overwrite it; a source
    // without a hierarchy is an error.
    let out = run(&[
        "import",
        "--json",
        "--prefab",
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
    let prefab = assets.join("models/robot.prefab.json");
    assert_eq!(v["files"][0]["prefab"], prefab.display().to_string());
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&prefab).unwrap()).unwrap();
    assert_ne!(doc["id"], subs["prefab"], "a fresh id, not the sub-asset's");
    assert_eq!(
        doc["entities"].as_object().unwrap().len(),
        10,
        "the robot's root, nodes and primitive entities"
    );
    let out = run(&["prefab", "validate", prefab.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run(&[
        "import",
        "--json",
        "--prefab",
        "--out",
        out_dir.to_str().unwrap(),
        glb.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["severity"] == "error"
            && d["message"].as_str().unwrap().contains("already exists")));
    let out = run(&[
        "import",
        "--json",
        "--prefab",
        "--out",
        out_dir.to_str().unwrap(),
        assets.join("swatch.png").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-A0105");
    assert_eq!(v["diagnostics"][0]["severity"], "error");

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
    assert_eq!(first["cooked"], 7, "six and the built-in primitives");
    assert_eq!(first["cached"], 0);
    assert_eq!(first["failed"], 0);
    assert!(cache.join("index.json").is_file());
    let second = cook(&[]);
    assert_eq!(second["cached"], 7);
    let ios = cook(&["--profile", "ios"]);
    assert_eq!(ios["cooked"], 7);

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
    assert_eq!(pruned["pruned"], 7);
    assert_eq!(cook(&[])["cooked"], 7);
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
    assert_eq!(v["documents"], 11);
    assert_eq!(v["sources"], 6);
    assert_eq!(v["references"], 31);
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
    assert_eq!(v["assetCount"], 17);
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

/// M4-07: `unpackPrefab` through `scene apply` reads the prefabs the way
/// `inspect --expand` does (`--prefabs`, else the scene's directory); the
/// written scene inspects exactly like the expansion of the original, and
/// without the prefabs the transaction is refused with the D0401 attached.
#[test]
fn scene_apply_unpacks_prefabs_with_the_inspect_library() {
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
    let dir = std::env::temp_dir().join(format!("fynkel-cli-unpack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scene = dir.join("instances.scene.json");
    std::fs::copy(fixtures.join("prefab/instances_ok.scene.json"), &scene).unwrap();
    let original = std::fs::read_to_string(&scene).unwrap();
    let scene_s = scene.to_str().unwrap();
    let prefabs = fixtures.to_str().unwrap();
    let patch = dir.join("ops.json");
    std::fs::write(
        &patch,
        serde_json::json!([
            {"command": "unpackPrefab", "entity": "20000000-0000-4000-8000-0000000000a1"},
            {"command": "unpackPrefab", "entity": "20000000-0000-4000-8000-0000000000a2"}
        ])
        .to_string(),
    )
    .unwrap();

    // The reference: the original scene expanded.
    let out = run(&[
        "scene",
        "inspect",
        "--json",
        "--expand",
        scene_s,
        "--prefabs",
        prefabs,
    ]);
    let expanded: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(expanded["entityCount"], 5);

    // Without the prefabs (the temp directory holds none): refused, untouched.
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
    let refused = &v["diagnostics"][0];
    assert_eq!(refused["code"], "FYN-E0004", "{v}");
    assert_eq!(refused["related"][0]["code"], "FYN-D0401", "{v}");
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), original);

    // With them: written, and the plain inspection of the result is the
    // expansion (ids, names, order, override values).
    let out = run(&[
        "scene",
        "apply",
        scene_s,
        "--patch",
        patch.to_str().unwrap(),
        "--prefabs",
        prefabs,
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["written"], true);
    assert_eq!(v["mutations"].as_array().unwrap().len(), 2);
    assert_eq!(v["mutations"][0]["command"], "unpackPrefab");
    let out = run(&["scene", "inspect", "--json", scene_s]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let unpacked: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(unpacked["expanded"], false);
    assert_eq!(unpacked["entityCount"], 5);
    assert_eq!(unpacked["instanceCount"], 0);
    assert_eq!(unpacked["roots"], expanded["roots"]);
    let _ = std::fs::remove_dir_all(&dir);
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
        r#"[{"command": "createObject", "name": "Antenna", "parent": "10000000-0000-4000-8000-0000000000b1",
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
    assert_eq!(v["root"], "10000000-0000-4000-8000-0000000000b1");
    assert_eq!(v["entities"][0]["children"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A scene that uses the built-in shapes (`Fynkel/Primitives.glb`, what
/// `createPrimitive` and `package:fynkel/authoring.dart` write) validates
/// in a project that never installed them, as it cooks (ADR-0026 §4); the
/// shapes are not counted among the project's sources.
#[test]
fn validate_resolves_the_built_in_shapes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("fynkel-cli-builtins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&root.join("crates/fynkel_document/fixtures/project"), &dir);
    let _ = std::fs::remove_dir_all(dir.join(".fynkel"));
    let scene = dir.join("assets/scenes/results.scene.json");
    let text = std::fs::read_to_string(&scene).unwrap().replacen(
        "\"asset\": \"c0000000-0000-4000-8000-000000000001\",\n              \"sub\": \"mesh/Cube\"",
        "\"asset\": \"f7a1e5d0-0000-4000-8000-000000000023\",\n              \"sub\": \"mesh/Cube\"",
        1,
    );
    assert!(text.contains("f7a1e5d0"));
    std::fs::write(&scene, text).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args([
            "validate",
            "--project",
            dir.to_str().unwrap(),
            "--json",
            "--root",
        ])
        .arg(&root)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["sources"], 6);
    assert!(!dir.join("assets/Fynkel").exists(), "nothing installed");
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

/// M3-19a on content nobody here made: the Khronos sample rigs (`Fox`,
/// `CesiumMan`, `RiggedFigure` under `crates/fynkel_import/fixtures/khronos/`)
/// dropped into a copy of the fixture project, each instanced by a scene
/// that names only the source — `fynkel import` (sidecars), `validate`
/// (the importer's provider), `cook` (the `prefab` chunk), `test
/// --headless` (the skinned mesh over every joint, the clip playing) and
/// `state dump` (a named joint of the import has moved after a second).
#[test]
fn khronos_rigs_instance_from_the_source_alone() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .output()
            .unwrap()
    };
    let dir = std::env::temp_dir().join(format!("fynkel-cli-khronos-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&root.join("crates/fynkel_document/fixtures/project"), &dir);
    let _ = std::fs::remove_dir_all(dir.join(".fynkel"));
    let project = dir.to_str().unwrap();

    // (model, clip to play, a joint the clip moves, joints, prefab entities)
    let rigs = [
        // The fox's skeleton and mesh are two top-level nodes: + a root.
        ("Fox", "anim/Run", "b_Neck_04", 24, 27),
        ("CesiumMan", "anim/#0", "Skeleton_arm_joint_L__3_", 19, 22),
        ("RiggedFigure", "anim/#0", "arm_joint_L_3", 19, 22),
    ];
    let mut files = Vec::new();
    for (model, ..) in &rigs {
        let glb = dir.join(format!("assets/models/{model}.glb"));
        std::fs::copy(
            root.join(format!("crates/fynkel_import/fixtures/khronos/{model}.glb")),
            &glb,
        )
        .unwrap();
        files.push(glb);
    }
    let mut args = vec!["import", "--json", "--out", ".fynkel/cache/import"];
    let shown: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
    args.extend(shown.iter().map(String::as_str));
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(&args)
        .arg("--root")
        .arg(&root)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let ids: Vec<String> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap().to_string())
        .collect();
    for (i, (_, _, _, _, entities)) in rigs.iter().enumerate() {
        assert_eq!(v["files"][i]["summary"]["prefabEntities"], *entities);
    }

    // One scene per rig: camera, sun, a `Character` instancing
    // `<source>#prefab` with an animator on the clip.
    let rig_scene: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("assets/scenes/rig.scene.json")).unwrap(),
    )
    .unwrap();
    let camera = &rig_scene["entities"]["c0000000-0000-4000-8000-0000000004e0"];
    let sun = &rig_scene["entities"]["c0000000-0000-4000-8000-0000000004e1"];
    let mut scene_ids = Vec::new();
    for (i, (model, clip, ..)) in rigs.iter().enumerate() {
        let scene_id = format!("c0000000-0000-4000-8000-0000000000{}", 0x35 + i);
        let scale = if *model == "Fox" { 0.01 } else { 1.0 };
        let scene = serde_json::json!({
            "$schema": "https://fynkel.dev/schemas/scene.schema.json",
            "formatVersion": 4,
            "id": scene_id,
            "name": model,
            "entities": {
                "c0000000-0000-4000-8000-0000000005e0": camera,
                "c0000000-0000-4000-8000-0000000005e1": sun,
                "c0000000-0000-4000-8000-0000000005e2": {
                    "name": "Character", "parent": null, "children": [],
                    "components": {
                        "c0000000-0000-4000-8000-000000000504": {
                            "type": "fynkel.Transform", "version": 1,
                            "data": { "position": [0, 0, 0], "rotation": [0, 0, 0, 1], "scale": [scale, scale, scale] }
                        },
                        "c0000000-0000-4000-8000-000000000505": {
                            "type": "fynkel.Animator", "version": 1,
                            "data": { "clip": { "asset": ids[i], "sub": clip }, "speed": 1.0, "looping": true, "playing": true, "weight": 1.0, "additive": false, "root_motion": false }
                        }
                    },
                    "prefab": { "asset": { "asset": ids[i], "sub": "prefab" }, "instance": "c0000000-0000-4000-8000-0000000005f0", "overrides": [] }
                }
            }
        });
        // Written the way a formatVersion 4 tool did (the anchor holds the
        // Transform and the Animator); `fynkel migrate` makes them
        // overrides of the imported root (ADR-0022).
        let path = dir.join(format!("assets/scenes/{}.scene.json", model.to_lowercase()));
        std::fs::write(&path, serde_json::to_string_pretty(&scene).unwrap()).unwrap();
        let out = run(&["migrate", path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{model}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::remove_file(path.with_extension("json.bak")).unwrap();
        scene_ids.push(scene_id);
    }

    let out = run(&["validate", "--project", project, "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (
            out.status.code(),
            v["errors"].as_u64(),
            v["warnings"].as_u64()
        ),
        (Some(0), Some(0), Some(0)),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = run(&["cook", "--project", project]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    for (i, (model, _, joint, joints, entities)) in rigs.iter().enumerate() {
        let out = run(&[
            "test",
            "--headless",
            "--project",
            project,
            "--no-cook",
            "--scene",
            &scene_ids[i],
            "--steps",
            "60",
            "--json",
        ]);
        assert!(
            out.status.success(),
            "{model}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        // The camera, the sun and the prefab (its root is the Character).
        assert_eq!(v["scene"]["entities"], entities + 2, "{model}");
        assert_eq!(v["diagnostics"], 0, "{model}");
        let anim = &v["animation"];
        assert_eq!(
            (
                anim["entities"].as_u64(),
                anim["clips"].as_u64(),
                anim["skins"].as_u64(),
                anim["skinned_meshes"].as_u64(),
                anim["joints"].as_u64(),
                anim["diagnostics"].as_u64()
            ),
            (Some(2), Some(1), Some(1), Some(1), Some(*joints), Some(0)),
            "{model}: {anim}"
        );

        // The named joint moves between the first tick and one second in.
        let pose = |steps: &str| -> serde_json::Value {
            let out = run(&[
                "state",
                "dump",
                "--project",
                project,
                "--no-cook",
                "--scene",
                &scene_ids[i],
                "--steps",
                steps,
                "--json",
            ]);
            assert!(
                out.status.success(),
                "{model}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            v["entities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["name"] == *joint)
                .unwrap_or_else(|| panic!("{model}: no entity named {joint}"))["world"]
                .clone()
        };
        let (start, later) = (pose("1"), pose("60"));
        assert_ne!(start, later, "{model}: {joint} did not move");
    }

    std::fs::remove_dir_all(&dir).unwrap();
}

/// M3-19b (b): the Khronos rigs assembled through the rig document —
/// `fynkel import --rig` writes it (the first file the skeleton, every
/// file's clips named by bone), `rig inspect`/`validate` read it back,
/// a scene names `{rig, prefab}` and `anim/<name>` and plays through
/// `cook`, `test --headless` and `state dump`; two rigs with different
/// bone names refuse to share a clip (`FYN-A0106`).
#[test]
fn khronos_rigs_assemble_into_a_rig_document() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("fynkel-cli-rigdoc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&root.join("crates/fynkel_document/fixtures/project"), &dir);
    let _ = std::fs::remove_dir_all(dir.join(".fynkel"));
    let project = dir.to_str().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .args(args)
            .arg("--root")
            .arg(&root)
            .current_dir(&dir)
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
    for model in ["Fox", "CesiumMan", "RiggedFigure"] {
        std::fs::copy(
            root.join(format!("crates/fynkel_import/fixtures/khronos/{model}.glb")),
            dir.join(format!("assets/models/{model}.glb")),
        )
        .unwrap();
    }

    // Sidecars for the two the Fox does not use (the database refuses a
    // source without one), then the Fox alone: skeleton and three clips
    // from one file.
    let out = run(&[
        "import",
        "--json",
        "--out",
        ".fynkel/cache/import",
        "assets/models/CesiumMan.glb",
        "assets/models/RiggedFigure.glb",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = run(&[
        "import",
        "--json",
        "--out",
        ".fynkel/cache/import",
        "--rig",
        "fox",
        "assets/models/Fox.glb",
    ]);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    let rig = &v["rig"];
    assert_eq!(
        (
            rig["ok"].as_bool(),
            rig["written"].as_bool(),
            rig["created"].as_bool()
        ),
        (Some(true), Some(true), Some(true)),
        "{rig}"
    );
    let rig_id = rig["id"].as_str().unwrap().to_string();
    let clips: Vec<(&str, u64, u64)> = rig["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap(),
                c["channels"].as_u64().unwrap(),
                c["matched"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(clips.len(), 3, "{rig}");
    assert_eq!(
        clips.iter().map(|c| c.0).collect::<Vec<_>>(),
        ["survey", "walk", "run"]
    );
    for (name, channels, matched) in &clips {
        assert_eq!(channels, matched, "{name}: every channel found a bone");
    }
    let rig_path = dir.join("assets/models/fox.rig.json");
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rig_path).unwrap()).unwrap();
    assert_eq!(doc["id"], rig_id.as_str());
    assert_eq!(doc["name"], "fox");
    assert_eq!(doc["skeleton"]["asset"], v["files"][0]["id"]);
    assert_eq!(doc["clips"]["run"]["sub"], "anim/Run");
    let written = std::fs::read_to_string(&rig_path).unwrap();

    // A dry run over an existing rig keeps its id and touches nothing.
    let out = run(&[
        "import",
        "--json",
        "--dry-run",
        "--out",
        ".fynkel/cache/import",
        "--rig",
        "fox",
        "assets/models/Fox.glb",
    ]);
    assert!(out.status.success());
    let rig = &json(&out)["rig"];
    assert_eq!(
        (
            rig["ok"].as_bool(),
            rig["written"].as_bool(),
            rig["created"].as_bool()
        ),
        (Some(true), Some(false), Some(false)),
        "{rig}"
    );
    assert_eq!(rig["id"], rig_id.as_str());
    assert_eq!(std::fs::read_to_string(&rig_path).unwrap(), written);

    // `rig inspect` reads the document back, text and JSON.
    let out = run(&["rig", "inspect", "assets/models/fox.rig.json", "--json"]);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(v["ok"], true);
    assert_eq!(v["id"], rig_id.as_str());
    assert_eq!(v["skeleton"]["joints"].as_array().unwrap().len(), 24);
    assert_eq!(v["skeleton"]["entities"], 27);
    assert_eq!(v["skeleton"]["roots"], serde_json::json!(["_rootJoint"]));
    assert_eq!(v["clips"][1]["name"], "walk");
    assert_eq!(v["clips"][1]["sub"], "anim/walk");
    assert_eq!(v["clips"][1]["matched"], v["clips"][1]["channels"]);
    assert_eq!(v["diagnostics"].as_array().unwrap().len(), 0);
    let out = run(&["rig", "inspect", "assets/models/fox.rig.json"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("fox (assets/models/fox.rig.json)"), "{text}");
    assert!(
        text.contains("24 joint(s) (root _rootJoint), 27 entities"),
        "{text}"
    );
    assert!(text.contains("clips (3)"), "{text}");
    assert!(text.contains("run "), "{text}");
    let out = run(&["rig", "validate", "assets/models/fox.rig.json"]);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("1/1 rig(s) ok"));

    // Markers (M3-19c): two footsteps authored on `run` in the rig
    // document, one past the clip's end that the cook drops with a
    // warning; `rig inspect` counts them per clip.
    let mut doc: serde_json::Value = serde_json::from_str(&written).unwrap();
    doc["markers"] = serde_json::json!({
        "run": [
            { "at": 0.1, "name": "step" },
            { "at": 0.4, "name": "step" },
            { "at": 9.0, "name": "late" }
        ]
    });
    std::fs::write(&rig_path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    let out = run(&["rig", "inspect", "assets/models/fox.rig.json", "--json"]);
    let v = json(&out);
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["clips"][2]["name"], "run");
    assert_eq!(v["clips"][2]["markers"], 3);
    assert_eq!(v["clips"][1]["markers"], 0);

    // A scene instances `{rig, prefab}` and plays `anim/run`.
    let rig_scene: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("assets/scenes/rig.scene.json")).unwrap(),
    )
    .unwrap();
    let scene_id = "c0000000-0000-4000-8000-000000000038";
    let scene = serde_json::json!({
        "$schema": "https://fynkel.dev/schemas/scene.schema.json",
        "formatVersion": 4,
        "id": scene_id,
        "name": "Fox",
        "entities": {
            "c0000000-0000-4000-8000-0000000005e0": rig_scene["entities"]["c0000000-0000-4000-8000-0000000004e0"],
            "c0000000-0000-4000-8000-0000000005e1": rig_scene["entities"]["c0000000-0000-4000-8000-0000000004e1"],
            "c0000000-0000-4000-8000-0000000005e2": {
                "name": "Character", "parent": null, "children": [],
                "components": {
                    "c0000000-0000-4000-8000-000000000504": {
                        "type": "fynkel.Transform", "version": 1,
                        "data": { "position": [0, 0, 0], "rotation": [0, 0, 0, 1], "scale": [0.01, 0.01, 0.01] }
                    },
                    "c0000000-0000-4000-8000-000000000505": {
                        "type": "fynkel.Animator", "version": 1,
                        "data": { "clip": { "asset": rig_id, "sub": "anim/run" }, "speed": 1.0, "looping": true, "playing": true, "weight": 1.0, "additive": false, "root_motion": false }
                    }
                },
                "prefab": { "asset": { "asset": rig_id, "sub": "prefab" }, "instance": "c0000000-0000-4000-8000-0000000005f0", "overrides": [] }
            }
        }
    });
    // Written as formatVersion 4 and migrated, as above.
    let path = dir.join("assets/scenes/fox.scene.json");
    std::fs::write(&path, serde_json::to_string_pretty(&scene).unwrap()).unwrap();
    let out = run(&["migrate", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_file(path.with_extension("json.bak")).unwrap();
    let out = run(&["validate", "--project", project, "--json"]);
    let v = json(&out);
    assert_eq!(
        (
            out.status.code(),
            v["errors"].as_u64(),
            v["warnings"].as_u64()
        ),
        (Some(0), Some(0), Some(0)),
        "{v}"
    );
    let out = run(&["cook", "--project", project, "--json"]);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let cook_text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        cook_text.contains("FYN-D0502") && cook_text.contains("\\\"late\\\""),
        "the marker past the clip's end is a warning: {cook_text}"
    );
    let out = run(&[
        "test",
        "--headless",
        "--project",
        project,
        "--no-cook",
        "--scene",
        scene_id,
        "--steps",
        "60",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(v["scene"]["entities"], 27 + 2);
    assert_eq!(v["diagnostics"], 0);
    let anim = &v["animation"];
    assert_eq!(
        (
            anim["clips"].as_u64(),
            anim["skins"].as_u64(),
            anim["joints"].as_u64(),
            anim["diagnostics"].as_u64()
        ),
        (Some(1), Some(1), Some(24), Some(0)),
        "{anim}"
    );
    // One second of the 1.16 s run crosses both steps (`ANIMATION_MARKER`
    // counted headlessly).
    assert_eq!(anim["markers"], 2, "{anim}");
    let pose = |steps: &str| -> serde_json::Value {
        let out = run(&[
            "state",
            "dump",
            "--project",
            project,
            "--no-cook",
            "--scene",
            scene_id,
            "--steps",
            steps,
            "--json",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        json(&out)["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == "b_Neck_04")
            .expect("the fox's neck")["world"]
            .clone()
    };
    assert_ne!(pose("1"), pose("60"), "the run did not move the neck");

    // A blend space (M3-19c): survey → walk → run along `speed`. The
    // scene's animator plays the document; the headless run registers
    // the space with its three clips; a parameter set from the world
    // moves along it phase-synced — at 1.5 every bone is halfway between
    // the walk and the run sampled at the same fraction of their own
    // length, and the clock keeps its phase across the change.
    let blend_id = "c0000000-0000-4000-8000-000000000059";
    std::fs::write(
        dir.join("assets/models/fox.blend.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "$schema": "https://fynkel.dev/schemas/blend.schema.json",
            "formatVersion": 1,
            "id": blend_id,
            "name": "Fox locomotion",
            "parameter": "speed",
            "points": [
                { "at": 0, "clip": { "asset": rig_id, "sub": "anim/survey" } },
                { "at": 1, "clip": { "asset": rig_id, "sub": "anim/walk" } },
                { "at": 2, "clip": { "asset": rig_id, "sub": "anim/run" } }
            ],
            "sync": "phase"
        }))
        .unwrap(),
    )
    .unwrap();
    let blend_scene_id = "c0000000-0000-4000-8000-000000000039";
    // The migrated fox scene with its Animator (an override of the
    // imported root now) playing the blend space.
    let mut blend_scene: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    blend_scene["id"] = serde_json::json!(blend_scene_id);
    blend_scene["name"] = serde_json::json!("Fox blend");
    let animator = blend_scene["entities"]["c0000000-0000-4000-8000-0000000005e2"]["prefab"]
        ["overrides"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|o| o["type"] == "fynkel.Animator")
        .unwrap();
    animator["data"]["clip"] = serde_json::json!({ "asset": blend_id });
    std::fs::write(
        dir.join("assets/scenes/fox_blend.scene.json"),
        serde_json::to_string_pretty(&blend_scene).unwrap(),
    )
    .unwrap();
    let out = run(&["validate", "--project", project, "--json"]);
    let v = json(&out);
    assert_eq!(
        (out.status.code(), v["errors"].as_u64()),
        (Some(0), Some(0)),
        "{v}"
    );
    let out = run(&["cook", "--project", project, "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run(&[
        "test",
        "--headless",
        "--project",
        project,
        "--no-cook",
        "--scene",
        blend_scene_id,
        "--steps",
        "60",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(v["diagnostics"], 0, "{v}");
    assert_eq!(v["animation"]["clips"], 1, "the space, with its clips");
    assert_eq!(
        v["animation"]["markers"], 0,
        "the survey (the first point) carries no markers"
    );
    {
        use fynkel_core::{AnimateOp, Command, Lane};
        let cache = dir.join(".fynkel/cache");
        let config = fynkel_host_headless::HeadlessConfig {
            steps: 1,
            cache: Some(cache),
            scene: Some(blend_scene_id.to_string()),
            ..fynkel_host_headless::HeadlessConfig::default()
        };
        let (report, mut world, loaded) = fynkel_host_headless::run_with(&config);
        let loaded = loaded.expect("the scene loaded");
        assert_eq!(report.diagnostics, 0, "{}", report.to_json());
        let character = loaded
            .entities()
            .iter()
            .find(|(id, _)| id.to_string() == "c0000000-0000-4000-8000-0000000005e2")
            .map(|(_, e)| *e)
            .unwrap();
        let rig: fynkel_contracts::Uuid = rig_id.parse().unwrap();
        let clip_of = |w: &fynkel_core::World, name: &str| {
            w.clip(&fynkel_contracts::AssetRef::with_sub(
                rig,
                format!("anim/{name}"),
            ))
            .unwrap_or_else(|| panic!("{name} registered"))
            .clone()
        };
        let (survey, walk, run_clip) = (
            clip_of(&world, "survey"),
            clip_of(&world, "walk"),
            clip_of(&world, "run"),
        );
        assert!(world.has_blend(&fynkel_contracts::AssetRef::new(blend_id.parse().unwrap())));
        let state = world.animator_state(character).unwrap();
        assert!((state.duration - survey.duration).abs() < 1e-5, "{state:?}");
        let animate = |w: &mut fynkel_core::World, ops: Vec<AnimateOp>| {
            w.submit(
                Lane::Simulation,
                ops.into_iter()
                    .map(|op| Command::Animate {
                        entity: character,
                        op,
                    })
                    .collect(),
            );
            let r = w.step();
            assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
        };
        let set_speed = |v: f32| AnimateOp::SetParameter {
            name: "speed".into(),
            value: v,
        };
        // Onto the walk, then a change of parameter keeps the phase.
        animate(&mut world, vec![set_speed(1.0)]);
        let s = world.animator_state(character).unwrap();
        assert!((s.duration - walk.duration).abs() < 1e-5, "{s:?}");
        let phase = s.time / s.duration;
        animate(&mut world, vec![set_speed(1.5)]);
        let s = world.animator_state(character).unwrap();
        let mid = (walk.duration + run_clip.duration) / 2.0;
        assert!((s.duration - mid).abs() < 1e-5, "{s:?}");
        assert!(
            ((s.time / s.duration) - (phase + 1.0 / 60.0 / mid)).abs() < 1e-4,
            "phase kept: {s:?} from {phase}"
        );
        // The pose at one phase: walk alone, run alone, and the middle.
        let phase = 0.3;
        let bones = |w: &fynkel_core::World| -> Vec<(String, [f32; 3], [f32; 4])> {
            let dump = fynkel_host_headless::dump_json(w, &loaded);
            let mut out: Vec<_> = dump["entities"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["name"].as_str().unwrap_or("").starts_with("b_"))
                .map(|e| {
                    let f = |v: &serde_json::Value| v.as_f64().unwrap() as f32;
                    let p = &e["local"]["position"];
                    let q = &e["local"]["rotation"];
                    (
                        e["name"].as_str().unwrap().to_string(),
                        [f(&p[0]), f(&p[1]), f(&p[2])],
                        [f(&q[0]), f(&q[1]), f(&q[2]), f(&q[3])],
                    )
                })
                .collect();
            out.sort_by(|a, b| a.0.cmp(&b.0));
            out
        };
        animate(
            &mut world,
            vec![set_speed(1.0), AnimateOp::SetTime(phase * walk.duration)],
        );
        let a = bones(&world);
        animate(
            &mut world,
            vec![
                set_speed(2.0),
                AnimateOp::SetTime(phase * run_clip.duration),
            ],
        );
        let b = bones(&world);
        animate(
            &mut world,
            vec![set_speed(1.5), AnimateOp::SetTime(phase * mid)],
        );
        let c = bones(&world);
        assert_eq!(a.len(), 23, "the Fox's b_ joints");
        assert_ne!(a, b, "the walk and the run differ");
        let mut checked = 0;
        for ((na, pa, qa), ((nb, pb, qb), (nc, pc, qc))) in a.iter().zip(b.iter().zip(c.iter())) {
            assert_eq!(na, nb);
            assert_eq!(na, nc);
            let qa = glam::Quat::from_array(*qa);
            let qb = glam::Quat::from_array(*qb);
            let qc = glam::Quat::from_array(*qc);
            let expect_p = glam::Vec3::from(*pa).lerp(glam::Vec3::from(*pb), 0.5);
            let expect_q = qa.slerp(qb, 0.5);
            assert!(
                (glam::Vec3::from(*pc) - expect_p).length() < 1e-3,
                "{na}: {pc:?} vs {expect_p}"
            );
            assert!(
                qc.abs_diff_eq(expect_q, 2e-3) || qc.abs_diff_eq(-expect_q, 2e-3),
                "{na}: {qc:?} vs {expect_q:?}"
            );
            checked += 1;
        }
        assert_eq!(checked, 23, "every bone sits between the two clips");
    }

    // Two rigs with different bone names: the CesiumMan clip animates
    // bones the RiggedFigure does not have, so the rig is written with an
    // error and refused by `rig validate` and `validate`.
    let out = run(&[
        "import",
        "--json",
        "--out",
        ".fynkel/cache/import",
        "--rig",
        "mismatch",
        "assets/models/RiggedFigure.glb",
        "assets/models/CesiumMan.glb",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["rig"]["ok"], false);
    assert_eq!(v["rig"]["written"], true);
    let clips = v["rig"]["clips"].as_array().unwrap();
    assert_eq!(clips[0]["name"], "riggedfigure");
    assert_eq!(clips[0]["matched"], clips[0]["channels"]);
    assert_eq!(clips[1]["name"], "cesiumman");
    assert_eq!(clips[1]["matched"], 27);
    assert_eq!(clips[1]["missing"].as_array().unwrap().len(), 10, "{v}");
    let codes: Vec<(&str, &str)> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["code"].as_str().unwrap(),
                d["pointer"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(codes, [("FYN-A0106", "/clips/cesiumman")], "{v}");
    let out = run(&[
        "rig",
        "validate",
        "assets/models/mismatch.rig.json",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["diagnostics"][0]["code"], "FYN-A0106");
    assert_eq!(v["diagnostics"][0]["pointer"], "/clips/cesiumman");
    // The document itself is valid (`validate` reads sources for their
    // ids, not their bones); the cook matches bones and refuses the rig.
    let out = run(&["validate", "--project", project, "--json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = run(&["cook", "--project", project, "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    let codes: Vec<(&str, &str)> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["code"].as_str().unwrap(),
                d["pointer"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert!(
        codes.contains(&("FYN-A0106", "/clips/cesiumman")),
        "{codes:?}"
    );

    std::fs::remove_dir_all(&dir).unwrap();
}
