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

#[test]
fn scene_documents_load_headlessly_and_dump_by_id() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scene = root.join("crates/fynkel_document/fixtures/valid/training_room.scene.json");
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(["test", "--headless", "--steps", "3", "--json", "--scene"])
        .arg(&scene)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["scene"]["loaded"], true);
    assert_eq!(v["scene"]["entities"], 4);
    assert_eq!(v["live_entities"], 4);

    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(["state", "dump", "--json", "--scene"])
        .arg(&scene)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(
        v["entities"][0]["id"],
        "f4d6d600-7931-44f5-a258-45b5a6850300"
    );
    assert!(!out.stdout.windows(7).any(|w| w == b"handle:"));

    let bad = root.join("crates/fynkel_document/fixtures/scene/dangling_parent.scene.json");
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .args(["state", "dump", "--json", "--scene"])
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0200");
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

    let bad = fixtures.join("material/bad_parameters.material.json");
    let out = run(&["material", "validate", "--json", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["code"], "FYN-D0301");

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
