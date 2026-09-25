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
    written += write_assets(project, check, &mut drift)?;

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

/// `lib/generated/assets.g.dart` (ADR-0026 §3) for a package that is a
/// project's: compared, and written unless `check`. Returns the files
/// written (0 or 1); a difference goes into `drift`.
pub fn write_assets(
    project: &Path,
    check: bool,
    drift: &mut Vec<PathBuf>,
) -> Result<usize, String> {
    let Some(text) = crate::assets_gen::generate(project)? else {
        return Ok(0);
    };
    let path = project.join(crate::assets_gen::DART_OUT);
    if read_opt(&path).as_deref() == Some(text.as_str()) {
        return Ok(0);
    }
    drift.push(PathBuf::from(crate::assets_gen::DART_OUT));
    if check {
        return Ok(0);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(1)
}

/// `fynkel codegen --watch --project <dir>`: `dart run build_runner watch`
/// kept running, so a saved script regenerates in about a second instead
/// of paying build_runner's start-up on every save (ADR-0016 amendment of
/// 2026-09-24, third). After every build the schemas are split out as in
/// [`run`], and one JSON line goes to stdout:
/// `{"event":"building"}`, `{"event":"built","status":"written"|"upToDate","types":N}`
/// or `{"event":"failed","error":"…"}`.
///
/// build_runner deletes its outputs when a build fails, and putting them
/// back starts another build that fails again, so while a script has an
/// error the generated files are absent (Unity refuses Play then too).
/// When the watch ends — stdin closes (the editor quit) or build_runner
/// exits — the last good files are put back if they are missing.
pub fn watch(project: &Path) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    use std::sync::mpsc;

    if !project.join("pubspec.yaml").is_file() {
        return Err(format!(
            "{} has no pubspec.yaml; --project names the game's Dart package",
            project.display()
        ));
    }
    let dart = find_dart().ok_or_else(|| {
        "no Dart SDK found: set FYNKEL_DART to a `dart` executable, or put `flutter` or `dart` on PATH".to_string()
    })?;
    if !project.join(".dart_tool/package_config.json").is_file() {
        sh(&dart, &["pub", "get"], project)?;
    }
    let dart_out = project.join(DART_OUT);
    let json_out = project.join(JSON_OUT);
    let mut good = (read_opt(&dart_out), read_opt(&json_out));

    let mut command = Command::new(&dart);
    command
        .args(["run", "build_runner", "watch"])
        .current_dir(project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own process group: `dart` starts the VM, the VM the builders, and
    // ending the watch ends all of them.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", dart.display()))?;

    enum Msg {
        Line(String),
        Exited,
        StdinClosed,
    }
    let (tx, rx) = mpsc::channel();
    for pipe in [
        child
            .stdout
            .take()
            .map(|o| Box::new(o) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|e| Box::new(e) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if tx.send(Msg::Line(line)).is_err() {
                    return;
                }
            }
            let _ = tx.send(Msg::Exited);
        });
    }
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut sink = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut sink);
            let _ = tx.send(Msg::StdinClosed);
        });
    }
    drop(tx);

    let emit = |v: serde_json::Value| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{v}");
        let _ = out.flush();
    };
    emit(serde_json::json!({ "event": "building" }));
    let mut log: Vec<String> = Vec::new();
    let mut closed_pipes = 0;
    let mut result = Ok(());
    while let Ok(msg) = rx.recv() {
        let line = match msg {
            Msg::Line(l) => l,
            Msg::StdinClosed => break,
            Msg::Exited => {
                closed_pipes += 1;
                if closed_pipes == 2 {
                    result = Err(format!("build_runner watch exited:\n{}", log.join("\n")));
                    break;
                }
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.starts_with("Starting build") {
            log.clear();
            emit(serde_json::json!({ "event": "building" }));
        } else if trimmed.starts_with("Built with") {
            let after = (read_opt(&dart_out), read_opt(&json_out));
            match finish(project, &after) {
                Ok((written, types)) => {
                    let changed = written > 0 || after != good;
                    good = after;
                    emit(serde_json::json!({
                        "event": "built",
                        "status": if changed { "written" } else { "upToDate" },
                        "types": types,
                    }));
                }
                Err(e) => emit(serde_json::json!({ "event": "failed", "error": e })),
            }
            log.clear();
        } else if trimmed.starts_with("Failed to build") {
            emit(serde_json::json!({ "event": "failed", "error": failure(&log) }));
            log.clear();
        } else if !trimmed.is_empty() {
            log.push(line);
        }
    }
    kill_tree(&mut child);
    // A watch that ends on an error leaves the last good files, as `run` does.
    if !dart_out.exists() || !json_out.exists() {
        restore(&dart_out, good.0.as_deref())?;
        restore(&json_out, good.1.as_deref())?;
    }
    result
}

/// The schema split after a watch build: (files written, types).
fn finish(
    project: &Path,
    after: &(Option<String>, Option<String>),
) -> Result<(usize, usize), String> {
    let json_text = after
        .1
        .as_ref()
        .ok_or_else(|| format!("build_runner did not write {JSON_OUT}"))?;
    let metadata: Value =
        serde_json::from_str(json_text).map_err(|e| format!("{JSON_OUT}: not JSON: {e}"))?;
    let schemas = split_schemas(&metadata)?;
    let schemas_dir = project.join(SCHEMAS_DIR);
    let mut written = 0;
    if schemas_dir.is_dir() {
        for entry in std::fs::read_dir(&schemas_dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".schema.json") && !schemas.contains_key(&name) {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                written += 1;
            }
        }
    }
    for (name, text) in &schemas {
        let path = schemas_dir.join(name);
        if read_opt(&path).as_deref() != Some(text.as_str()) {
            std::fs::create_dir_all(&schemas_dir).map_err(|e| e.to_string())?;
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
            written += 1;
        }
    }
    Ok((written, schemas.len()))
}

/// The error lines of a failed build: build_runner's `E` blocks, else all.
fn failure(log: &[String]) -> String {
    let mut out = Vec::new();
    let mut in_error = false;
    for line in log {
        if line.starts_with("E ") {
            in_error = true;
        } else if !line.starts_with("  ") {
            in_error = false;
        }
        let progress = line
            .trim_start()
            .split_once("s ")
            .is_some_and(|(n, _)| n.parse::<u32>().is_ok());
        if in_error && !progress {
            out.push(line.trim().to_string());
        }
    }
    if out.is_empty() {
        log.iter().map(|l| l.trim()).collect::<Vec<_>>().join("\n")
    } else {
        out.join("\n")
    }
}

/// Ends `dart run build_runner watch` and every process it started (its
/// process group): TERM, then KILL for what is left a second later.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let group = format!("-{}", child.id());
        let _ = Command::new("kill").args(["-TERM", "--", &group]).status();
        for _ in 0..10 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = Command::new("kill")
            .args(["-KILL", "--", &group])
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
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
