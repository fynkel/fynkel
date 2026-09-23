//! Opening an editor without building it (ADR-0021 P3c follow-up): the
//! editor's app bundle is the same from one open to the next unless
//! something native changed — the Flutter engine, a plugin, the Rust
//! runtime, the macOS runner. So after a build `fynkel editor` records what
//! the bundle was built from (`build/fynkel_launch.json` next to the Flutter
//! app), and the next open launches the built app directly (its window is
//! up in about half a second) and attaches `flutter attach` for hot reload.
//! Dart code changed since the build is brought in by one hot restart once
//! attached; anything native that changed means a full `flutter run` again.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Bumped when what the fingerprints cover changes: every stamp is stale.
const STAMP_VERSION: u64 = 1;

/// What an editor bundle was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// The engine, plugins, the Rust runtime, the macOS runner.
    pub native: String,
    /// The Dart code compiled into the bundle.
    pub dart: String,
}

/// The inputs of one editor build: `app` is the Flutter app's directory
/// (the project's `.fynkel/editor_host`, or `apps/editor`).
#[derive(Clone)]
pub struct Inputs {
    pub root: PathBuf,
    pub app: PathBuf,
    pub project: Option<PathBuf>,
    pub flutter: PathBuf,
}

/// A build in flight: the Dart code as it was when it started (a file
/// saved during the build may or may not be in it, so the older state is
/// the safe record) and, once it is up, the native inputs as the build
/// left them (it regenerates some runner files).
pub struct Recording {
    pub inputs: Inputs,
    pub dart: String,
}

impl Recording {
    pub fn finish(self) {
        let native = self.inputs.stamp().native;
        write_stamp(
            &self.inputs.app,
            &Stamp {
                native,
                dart: self.dart,
            },
        );
    }
}

impl Inputs {
    pub fn stamp(&self) -> Stamp {
        let flutter_root = self
            .flutter
            .canonicalize()
            .unwrap_or_else(|_| self.flutter.to_path_buf())
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut native = vec![
            flutter_root.join("bin/cache/engine.stamp"),
            flutter_root.join("bin/cache/flutter.version.json"),
            self.app.join("pubspec.yaml"),
            self.app.join("pubspec.lock"),
            self.app.join("macos"),
            self.root.join("Cargo.toml"),
            self.root.join("Cargo.lock"),
            self.root.join("rust-toolchain.toml"),
            self.root.join("crates"),
            self.root.join("schemas"),
            self.root.join("platform"),
            self.root.join("apps/editor/pubspec.yaml"),
            self.root.join("apps/editor/macos"),
        ];
        let mut dart = vec![self.app.join("lib"), self.root.join("apps/editor/lib")];
        for package in subdirs(&self.root.join("packages")) {
            native.push(package.join("pubspec.yaml"));
            native.push(package.join("hook"));
            dart.push(package.join("lib"));
        }
        if let Some(p) = &self.project {
            native.push(p.join("pubspec.yaml"));
            native.push(p.join("pubspec.lock"));
            dart.push(p.join("lib"));
        }
        Stamp {
            native: fingerprint(&native, |_| true),
            dart: fingerprint(&dart, |p| p.extension().is_some_and(|x| x == "dart")),
        }
    }
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Every file under `roots` that `keep` takes: its path and content.
fn fingerprint(roots: &[PathBuf], keep: impl Fn(&Path) -> bool) -> String {
    const SKIP: &[&str] = &[
        ".dart_tool",
        "build",
        "Pods",
        "ephemeral",
        "xcuserdata",
        "target",
        ".git",
    ];
    let mut files = Vec::new();
    for root in roots {
        let mut stack = vec![root.clone()];
        while let Some(p) = stack.pop() {
            let Ok(meta) = std::fs::metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                let skipped = p.file_name().is_some_and(|n| SKIP.iter().any(|s| n == *s));
                if !skipped || &p == root {
                    if let Ok(rd) = std::fs::read_dir(&p) {
                        stack.extend(rd.flatten().map(|e| e.path()));
                    }
                }
            } else if keep(&p) {
                // By content: Flutter touches engine.stamp on every
                // command, and `codegen --check` rewrites generated files
                // with the bytes they had.
                let Ok(bytes) = std::fs::read(&p) else {
                    continue;
                };
                let mut h = DefaultHasher::new();
                bytes.hash(&mut h);
                files.push((p, h.finish(), meta.len()));
            }
        }
    }
    files.sort();
    if let Some(dir) = std::env::var_os("FYNKEL_LAUNCH_TRACE") {
        let list: String = files
            .iter()
            .map(|(p, at, len)| format!("{at} {len} {}\n", p.display()))
            .collect();
        let name = format!("fingerprint-{}.txt", files.len());
        let _ = std::fs::write(Path::new(&dir).join(name), list);
    }
    let mut h = DefaultHasher::new();
    STAMP_VERSION.hash(&mut h);
    files.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn stamp_file(app: &Path) -> PathBuf {
    app.join("build").join("fynkel_launch.json")
}

pub fn read_stamp(app: &Path) -> Option<Stamp> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(stamp_file(app)).ok()?).ok()?;
    Some(Stamp {
        native: v["native"].as_str()?.to_string(),
        dart: v["dart"].as_str()?.to_string(),
    })
}

pub fn write_stamp(app: &Path, stamp: &Stamp) {
    let path = stamp_file(app);
    let body = json!({"version": 1, "native": stamp.native, "dart": stamp.dart});
    let _ = std::fs::write(path, format!("{body:#}\n"));
}

/// While a launched app runs older Dart code than the files, its window
/// is covered ("Loading code changes…") until the hot restart that brings
/// it up to date: whatever was done in the old code would be lost. The
/// app finds this file through `FYNKEL_EDITOR_UPDATING`: `stale` from
/// launch, `restarting` once the restart is asked for (the restarted code
/// reads that and shows nothing), gone when it is over.
pub fn updating_file(app: &Path) -> PathBuf {
    app.join("build").join("fynkel_updating")
}

/// Writes [updating_file] (`Some`) or removes it (`None`).
pub fn set_updating(app: &Path, state: Option<&str>) {
    let path = updating_file(app);
    match state {
        Some(s) => {
            let _ = std::fs::write(path, s);
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The debug app `flutter run` built under `app`, if there is one.
pub fn built_app(app: &Path) -> Option<PathBuf> {
    let products = app.join("build/macos/Build/Products/Debug");
    let bundle = std::fs::read_dir(&products)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "app"))?;
    let name = bundle.file_stem()?.to_owned();
    let exe = bundle.join("Contents/MacOS").join(name);
    exe.is_file().then_some(exe)
}

/// A line from the directly launched app, or its end.
pub enum AppLine {
    Out(String),
    Closed,
}

/// Starts the built app as `flutter run` would (the same engine switches)
/// and waits for its VM service. The app's lines keep coming on the
/// returned channel.
pub fn launch(
    exe: &Path,
    dir: &Path,
    args: &[&Path],
    env: &[(&str, &Path)],
    timeout: Duration,
) -> Result<(Child, String, mpsc::Receiver<AppLine>), String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(dir)
        .env("FLUTTER_ENGINE_SWITCH_1", "enable-dart-profiling=true")
        .env("FLUTTER_ENGINE_SWITCH_2", "enable-checked-mode=true")
        .env("FLUTTER_ENGINE_SWITCH_3", "verify-entry-points=true")
        .env("FLUTTER_ENGINE_SWITCHES", "3")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let (tx, rx) = mpsc::channel();
    let out = child.stdout.take().expect("piped");
    let err = child.stderr.take().expect("piped");
    let err_tx = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(AppLine::Out(line)).is_err() {
                return;
            }
        }
        let _ = tx.send(AppLine::Closed);
    });
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            if err_tx.send(AppLine::Out(line)).is_err() {
                return;
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let mut said = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(AppLine::Out(line)) => {
                if let Some(uri) = vm_service_uri(&line) {
                    return Ok((child, uri, rx));
                }
                said.push(line);
            }
            Ok(AppLine::Closed) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the built editor did not start{}",
                    if said.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", said.join("\n"))
                    }
                ));
            }
        }
    }
}

/// "The Dart VM service is listening on http://127.0.0.1:55705/Gou…=/".
fn vm_service_uri(line: &str) -> Option<String> {
    if !line.contains("VM service is listening on") {
        return None;
    }
    let start = line.find("http://")?;
    Some(
        line[start..]
            .split_whitespace()
            .next()?
            .trim_end_matches(['.', ','])
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vm_service_line() {
        assert_eq!(
            vm_service_uri(
                "flutter: The Dart VM service is listening on http://127.0.0.1:55705/GouhnTtPbfQ=/"
            )
            .as_deref(),
            Some("http://127.0.0.1:55705/GouhnTtPbfQ=/")
        );
        assert!(vm_service_uri("flutter: hello http://example.com").is_none());
    }

    #[test]
    fn a_touched_file_changes_its_fingerprint_and_skipped_dirs_do_not() {
        let dir = std::env::temp_dir().join(format!("fynkel-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::write(dir.join("lib/a.dart"), "a").unwrap();
        let dart = |d: &Path| fingerprint(&[d.to_path_buf()], |_| true);
        let before = dart(&dir);
        std::fs::write(dir.join("build/x.o"), "x").unwrap();
        assert_eq!(dart(&dir), before, "build/ is not an input");
        std::fs::write(dir.join("lib/a.dart"), "ab").unwrap();
        assert_ne!(dart(&dir), before);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
