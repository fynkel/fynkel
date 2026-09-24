//! Behaviour generator driver (ADR-0012 pipeline (b)): `fynkel codegen
//! --project <dir>` runs `dart run build_runner build` in the project's Dart
//! package (the `fynkel_generator` builder writes
//! `lib/generated/behaviours.g.dart` and `.g.json`), then splits the JSON
//! metadata into `schemas/behaviours/<id>.schema.json` — the files
//! `fynkel_document` loads for a project (`SchemaSet::embedded_with`) so
//! scene validation knows every behaviour type and its fields.
//!
//! The two pipelines have separate caches: (a) is a pure function of the
//! Rust registry (no cache), (b) is build_runner's `.dart_tool/build`.
//! `--check` runs the generator too and compares; on drift the committed
//! files are put back so a check never mutates the tree. A run that fails
//! (build_runner's error, a lock held by another build) puts them back as
//! well: build_runner deletes its outputs before it builds, and a failed
//! check used to leave the game without them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

pub const DART_OUT: &str = "lib/generated/behaviours.g.dart";
pub const JSON_OUT: &str = "lib/generated/behaviours.g.json";
pub const SCHEMAS_DIR: &str = "schemas/behaviours";

pub enum Outcome {
    /// Files written or refreshed (count) for `types` behaviour types.
    Written {
        files: usize,
        types: usize,
    },
    UpToDate {
        types: usize,
    },
    Drift(Vec<PathBuf>),
}

/// The `dart` executable: `$FYNKEL_DART`, else the one next to `flutter`
/// on `PATH` (Flutter ships its own SDK), else `dart` on `PATH`.
pub fn find_dart() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FYNKEL_DART") {
        let p = PathBuf::from(p);
        return if p.is_file() { Some(p) } else { None };
    }
    if let Some(flutter) = which("flutter") {
        let real = std::fs::canonicalize(&flutter).unwrap_or(flutter);
        if let Some(bin) = real.parent() {
            let dart = bin.join("dart");
            if dart.is_file() {
                return Some(dart);
            }
        }
    }
    which("dart")
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// Runs pipeline (b) for the Dart package at `project`.
pub fn run(project: &Path, check: bool) -> Result<Outcome, String> {
    if !project.join("pubspec.yaml").is_file() {
        return Err(format!(
            "{} has no pubspec.yaml; --project names the game's Dart package",
            project.display()
        ));
    }
    let dart = find_dart().ok_or_else(|| {
        "no Dart SDK found: set FYNKEL_DART to a `dart` executable, or put `flutter` or `dart` on PATH".to_string()
    })?;

    let dart_out = project.join(DART_OUT);
    let json_out = project.join(JSON_OUT);
    let before = (read_opt(&dart_out), read_opt(&json_out));
    let result = generate(project, &dart, check, &dart_out, &json_out, &before);
    if result.is_err() || check {
        restore(&dart_out, before.0.as_deref())?;
        restore(&json_out, before.1.as_deref())?;
    }
    result
}

fn generate(
    project: &Path,
    dart: &Path,
    check: bool,
    dart_out: &Path,
    json_out: &Path,
    before: &(Option<String>, Option<String>),
) -> Result<Outcome, String> {
    if !project.join(".dart_tool/package_config.json").is_file() {
        sh(dart, &["pub", "get"], project)?;
    }
    sh(dart, &["run", "build_runner", "build"], project)?;

    let after = (read_opt(dart_out), read_opt(json_out));
    let json_text = after
        .1
        .clone()
        .ok_or_else(|| format!("build_runner did not write {}", json_out.display()))?;
    let metadata: Value = serde_json::from_str(&json_text)
        .map_err(|e| format!("{}: not JSON: {e}", json_out.display()))?;
    let schemas = split_schemas(&metadata)?;
    let types = schemas.len();

    let schemas_dir = project.join(SCHEMAS_DIR);
    let mut drift = Vec::new();
    let mut written = 0usize;
    if before.0 != after.0 {
        drift.push(PathBuf::from(DART_OUT));
        written += 1;
    }
    if before.1 != after.1 {
        drift.push(PathBuf::from(JSON_OUT));
        written += 1;
    }
    // Stale schema files: anything in the directory the metadata no longer
    // names.
    let mut stale = Vec::new();
    if schemas_dir.is_dir() {
        for entry in std::fs::read_dir(&schemas_dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".schema.json") && !schemas.contains_key(&name) {
                stale.push(path);
            }
        }
    }
    for (name, text) in &schemas {
        let path = schemas_dir.join(name);
        if read_opt(&path).as_deref() != Some(text.as_str()) {
            drift.push(PathBuf::from(SCHEMAS_DIR).join(name));
            if !check {
                std::fs::create_dir_all(&schemas_dir).map_err(|e| e.to_string())?;
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                written += 1;
            }
        }
    }
    for path in &stale {
        drift.push(PathBuf::from(SCHEMAS_DIR).join(path.file_name().unwrap()));
        if !check {
            std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            written += 1;
        }
    }

    if check {
        // `run` puts the committed generated files back: a check never mutates.
        return Ok(if drift.is_empty() {
            Outcome::UpToDate { types }
        } else {
            Outcome::Drift(drift)
        });
    }
    Ok(if written == 0 {
        Outcome::UpToDate { types }
    } else {
        Outcome::Written {
            files: written,
            types,
        }
    })
}

/// `<id>.schema.json` → pretty text, from the metadata's `behaviours[].schema`.
pub fn split_schemas(metadata: &Value) -> Result<BTreeMap<String, String>, String> {
    let list = metadata
        .get("behaviours")
        .and_then(|b| b.as_array())
        .ok_or("behaviours.g.json has no `behaviours` array")?;
    let mut out = BTreeMap::new();
    for b in list {
        let id = b
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or("behaviour entry without `id`")?;
        let schema = b
            .get("schema")
            .ok_or_else(|| format!("behaviour {id} without `schema`"))?;
        let mut text = serde_json::to_string_pretty(schema).map_err(|e| e.to_string())?;
        text.push('\n');
        out.insert(format!("{id}.schema.json"), text);
    }
    Ok(out)
}

fn read_opt(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn restore(path: &Path, content: Option<&str>) -> Result<(), String> {
    match content {
        Some(text) => std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display())),
        None => {
            if path.exists() {
                std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Ok(())
        }
    }
}

fn sh(dart: &Path, args: &[&str], cwd: &Path) -> Result<(), String> {
    let output = Command::new(dart)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", dart.display()))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "`dart {}` in {} failed ({}):\n{}{}",
        args.join(" "),
        cwd.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}
