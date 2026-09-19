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
