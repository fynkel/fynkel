//! ADR-0003 contract: `hecs` is not visible in public APIs. Mechanically:
//! no `pub` line outside the `internal` module of `fynkel_ecs` mentions
//! `hecs`, and no other runtime crate depends on `hecs` directly.

use std::fs;
use std::path::Path;

#[test]
fn hecs_hidden_behind_internal_module() {
    let src = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let mut in_internal = false;
    let mut depth = 0i32;
    for (n, line) in src.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("pub mod internal") {
            in_internal = true;
            depth = 0;
        }
        if in_internal {
            depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
            if depth <= 0 && n > 0 && t.ends_with('}') {
                in_internal = false;
            }
            continue;
        }
        if t.starts_with("pub ") && t.contains("hecs::") {
            panic!("line {}: public item names hecs: {t}", n + 1);
        }
    }
}

#[test]
fn only_fynkel_ecs_depends_on_hecs() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for entry in fs::read_dir(crates).unwrap() {
        let dir = entry.unwrap().path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.exists() || dir.file_name().unwrap() == "fynkel_ecs" {
            continue;
        }
        let text = fs::read_to_string(&manifest).unwrap();
        assert!(
            !text.lines().any(|l| l.trim_start().starts_with("hecs")),
            "{} depends on hecs directly",
            manifest.display()
        );
    }
}
