//! `fynkel editor [dir] [--plain] [--flutter <path>]` (ADR-0021): the
//! project editor — the editor with the project's game compiled in, so Play
//! runs the game inside the editor.
//!
//! 1. Generates `<project>/.fynkel/editor_host/`: a Flutter package that
//!    depends on `apps/editor` (`package:fynkel_editor`) and on the
//!    project's package, whose `main` calls `runFynkelEditor` with the
//!    game's `lib/fynkel_game.dart` `buildGame`, and the editor's macOS
//!    runner. Files are rewritten only when they change, so Flutter's
//!    build cache holds between opens. The `main` also imports every
//!    package's editor part (`lib/editor/editor.dart`, ADR-0028) that the
//!    game's dependency graph holds ([`editor_parts`]).
//! 2. Runs it in debug (JIT) under `flutter run --machine`.
//! 3. Watches the project's `lib/` and hot reloads on save. Every reload's
//!    outcome goes to `<host>/status.json` (the editor shows it: a compile
//!    error lands in the editor, not only in this terminal).
//!
//! Without a directory, with `--plain`, or for a project that is not a
//! Dart package with `lib/fynkel_game.dart`, it runs the plain editor
//! (`apps/editor`, no game compiled in).
//!
//! `--handover` (P3c): an editor that opens a game project starts this and
//! hands over to it. The editor follows the project's `status.json`
//! (`building` with Flutter's progress, then `running`, when it quits, or
//! `failed` with why), so every failure is written there, and a project
//! that is not a game fails instead of opening in the plain editor.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

use crate::editor_launch;

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel editor [<project dir>] [--plain] [--handover] [--window-frame x,y,w,h] [--flutter <path>]\n                                               the editor with the project's game compiled in (Play runs inside it; hot reload on save);\n                                               --plain (or no project) runs the editor without a game;\n                                               --handover: started by an editor that hands the project over (it follows status.json)"
    );
    ExitCode::from(2)
}

/// The generated host's directory under a project.
pub const HOST_DIR: &str = ".fynkel/editor_host";

/// What the host's status file says (`<host>/status.json`, version 1).
pub const STATUS_FILE: &str = "status.json";

/// The editor asks for a hot restart by creating this file next to the
/// status file (P3b: *Restart to Apply*, after it saved); it is removed as
/// the restart is sent.
pub const RESTART_REQUEST: &str = "restart.request";

pub fn run(root: Option<&Path>, args: &[String]) -> ExitCode {
    let mut project: Option<PathBuf> = None;
    let mut plain = false;
    let mut handover = false;
    let mut flutter: Option<PathBuf> = None;
    let mut window_frame: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--plain" => plain = true,
            "--handover" => handover = true,
            "--flutter" => {
                i += 1;
                match args.get(i) {
                    Some(f) => flutter = Some(PathBuf::from(f)),
                    None => return usage(),
                }
            }
            // The handing-over editor's window, "x,y,w,h": this one opens
            // in its place (MainFlutterWindow.swift).
            "--window-frame" => {
                i += 1;
                match args.get(i) {
                    Some(f) => window_frame = Some(f.clone()),
                    None => return usage(),
                }
            }
            "--root" => i += 1,
            "--help" | "-h" => return usage(),
            other if other.starts_with('-') => return usage(),
            other => project = Some(PathBuf::from(other)),
        }
        i += 1;
    }
    let Some(root) = root else {
        eprintln!("fynkel editor: not inside the Fynkel repository; pass --root");
        return ExitCode::from(2);
    };
    let root = std::fs::canonicalize(root).unwrap_or(root.to_path_buf());
    let Some(flutter) = flutter.or_else(find_flutter) else {
        eprintln!("fynkel editor: `flutter` is not on PATH (pass --flutter <path>)");
        return ExitCode::from(2);
    };
    let project = match project {
        Some(p) => match std::fs::canonicalize(&p) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("fynkel editor: {}: {e}", p.display());
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let editor = root.join("apps").join("editor");

    // An editor handing over reads what happens in the status file, from
    // the first moment: every early failure lands there too.
    let early = project
        .as_ref()
        .filter(|_| handover && !plain)
        .map(|d| d.join(HOST_DIR).join(STATUS_FILE));
    let mut seq = 0;
    if let Some(path) = &early {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        seq += 1;
        write_status_file(path, seq, "building", "", None, &[]);
    }
    let fail = |seq: u64, message: &str| {
        eprintln!("fynkel editor: {message}");
        if let Some(path) = &early {
            write_status_file(path, seq + 1, "failed", message, None, &[]);
        }
        ExitCode::from(1)
    };

    // The project editor, or why not.
    let host = match (&project, plain) {
        (Some(dir), false) => match game_package(dir) {
            Ok(name) => match generate_host(dir, &name, &editor) {
                Ok(host) => Some(host),
                Err(e) => return fail(seq, &e),
            },
            Err(why) if handover => return fail(seq, &why),
            Err(why) => {
                say(&format!(
                    "{why}; opening it in the plain editor (Play runs it as its own app)"
                ));
                None
            }
        },
        _ => None,
    };
    let started = Instant::now();
    let cwd = host.clone().unwrap_or_else(|| editor.clone());
    let exe = std::env::current_exe().ok();
    let mut env: Vec<(&str, PathBuf)> = vec![("FYNKEL_ROOT", root.clone())];
    if let Some(exe) = &exe {
        env.push(("FYNKEL_CLI", exe.clone()));
    }
    if let Some(h) = &host {
        env.push(("FYNKEL_EDITOR_STATUS", h.join(STATUS_FILE)));
    }
    // Always set: an editor started from another must not read that one's.
    env.push(("FYNKEL_EDITOR_UPDATING", editor_launch::updating_file(&cwd)));
    env.push((
        "FYNKEL_WINDOW_FRAME",
        PathBuf::from(window_frame.unwrap_or_default()),
    ));
    editor_launch::set_updating(&cwd, None);

    // Nothing native changed since the last build: launch the built app
    // and attach, no build (editor_launch.rs).
    let inputs = editor_launch::Inputs {
        root: root.clone(),
        app: cwd.clone(),
        project: host.as_ref().and(project.clone()),
        flutter: flutter.clone(),
    };
    let stamp = inputs.stamp();
    let built = editor_launch::read_stamp(&cwd).filter(|s| s.native == stamp.native);
    if let (Some(built), Some(app_exe)) = (built, editor_launch::built_app(&cwd)) {
        let args: Vec<&Path> = project.as_deref().into_iter().collect();
        let env: Vec<(&str, &Path)> = env.iter().map(|(k, v)| (*k, v.as_path())).collect();
        let stale = built.dart != stamp.dart;
        if stale {
            editor_launch::set_updating(&cwd, Some("stale"));
        }
        match editor_launch::launch(&app_exe, &cwd, &args, &env, Duration::from_secs(30)) {
            Ok((app, uri, lines)) => {
                say(&format!(
                    "opened the built editor in {:.1} s (nothing native changed)",
                    started.elapsed().as_secs_f64()
                ));
                // The plain editor needs Flutter only to bring its code up
                // to date; a project editor, for hot reload on save.
                let tool = if host.is_some() || stale {
                    let mut cmd = Command::new(&flutter);
                    cmd.args(["attach", "--machine", "-d", "macos", "--debug-url", &uri])
                        .current_dir(&cwd)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped());
                    match cmd.spawn() {
                        Ok(c) => Some(c),
                        Err(e) => {
                            say(&format!(
                                "hot reload is off: cannot run flutter attach: {e}"
                            ));
                            editor_launch::set_updating(&cwd, None);
                            None
                        }
                    }
                } else {
                    None
                };
                return supervise(
                    tool,
                    Session {
                        project: project.as_deref(),
                        host: host.as_deref(),
                        started,
                        seq,
                        app: Some((app, lines)),
                        restart: stale,
                        stamp: None,
                        app_dir: &cwd,
                        refresh: stale.then(|| editor_launch::Recording {
                            inputs: inputs.clone(),
                            dart: stamp.dart.clone(),
                        }),
                        flutter: &flutter,
                    },
                );
            }
            Err(why) => {
                editor_launch::set_updating(&cwd, None);
                say(&format!("{why}; building it instead"))
            }
        }
    }

    if host.is_some() {
        let pub_get = Command::new(&flutter)
            .args(["pub", "get"])
            .current_dir(&cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();
        match pub_get {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return fail(
                    seq,
                    &format!(
                        "`flutter pub get` failed in {}:\n{}",
                        cwd.display(),
                        String::from_utf8_lossy(&o.stderr)
                    ),
                );
            }
            Err(e) => return fail(seq, &format!("cannot run {}: {e}", flutter.display())),
        }
    }
    say(&match (&project, &host) {
        (Some(p), Some(_)) => format!("building the project editor for {} …", p.display()),
        (Some(p), None) => format!("starting the editor on {} …", p.display()),
        _ => "starting the editor …".to_string(),
    });

    let mut cmd = Command::new(&flutter);
    cmd.args(["run", "-d", "macos", "--machine"]);
    if let Some(p) = &project {
        cmd.arg("--dart-entrypoint-args").arg(p);
    }
    cmd.current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &env {
        cmd.env(k, v);
    }
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(seq, &format!("cannot run {}: {e}", flutter.display())),
    };
    supervise(
        Some(child),
        Session {
            project: project.as_deref(),
            host: host.as_deref(),
            started,
            seq,
            app: None,
            restart: false,
            stamp: Some(editor_launch::Recording {
                inputs,
                dart: stamp.dart,
            }),
            app_dir: &cwd,
            refresh: None,
            flutter: &flutter,
        },
    )
}

/// One editor run, as [supervise] follows it.
struct Session<'a> {
    project: Option<&'a Path>,
    host: Option<&'a Path>,
    started: Instant,
    seq: u64,
    /// The built app, launched directly; the tool is then `flutter attach`
    /// (or nothing), and the run ends when the app does.
    app: Option<(Child, mpsc::Receiver<editor_launch::AppLine>)>,
    /// The Dart code changed since the app was built: one hot restart once
    /// attached.
    restart: bool,
    /// After a build: what it was built from, recorded once it is up.
    stamp: Option<editor_launch::Recording>,
    /// The Flutter app's directory ([editor_launch::updating_file]).
    app_dir: &'a Path,
    /// A launched app older than the code: once it has quit, rebuild it so
    /// the next open starts on the current code (no catch-up restart).
    refresh: Option<editor_launch::Recording>,
    flutter: &'a Path,
}

/// Runs until the editor exits: relays what matters, hot reloads on save.
fn supervise(tool: Option<Child>, session: Session) -> ExitCode {
    let Session {
        project,
        host,
        started,
        seq,
        app,
        restart,
        stamp,
        app_dir,
        refresh,
        flutter,
    } = session;
    let (tx, rx) = mpsc::channel::<Line>();
    let (mut app, app_lines) = match app {
        Some((child, lines)) => (Some(child), Some(lines)),
        None => (None, None),
    };
    if let Some(lines) = app_lines {
        let app_tx = tx.clone();
        std::thread::spawn(move || {
            for line in lines {
                let line = match line {
                    editor_launch::AppLine::Out(l) => Line::App(l),
                    editor_launch::AppLine::Closed => Line::AppClosed,
                };
                if app_tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    let direct = app.is_some();
    let mut tool = tool;
    let stdin = match tool.as_mut() {
        Some(child) => {
            spawn_readers(child, tx);
            child.stdin.take()
        }
        None => None,
    };
    let stdin = Arc::new(Mutex::new(stdin));
    let mut state = Supervisor {
        app: None,
        next_id: 1,
        pending: BTreeMap::new(),
        seq,
        status: host.map(|h| h.join(STATUS_FILE)),
        tail: Vec::new(),
        ready: false,
        changed: Vec::new(),
        compiler: Vec::new(),
        restart_due: false,
        parts_changed: false,
        restart,
        stamp,
        app_dir: app_dir.to_path_buf(),
    };
    if direct {
        state.ready = true;
        state.write_status("running", "", None, &[]);
    } else {
        state.write_status("starting", "", None, &[]);
    }
    let lib = project.map(|p| p.join("lib"));
    let watching = host.is_some();
    let package = project
        .filter(|_| watching)
        .and_then(|p| game_package(p).ok());
    let mut seen = lib
        .as_deref()
        .map(|l| dart_files(l, &BTreeMap::new()))
        .unwrap_or_default();
    let mut changed_at: Option<Instant> = None;
    let mut closed = false;
    while !closed {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Line::Out(line)) => state.on_line(&line, started),
            Ok(Line::Err(line)) => {
                if std::env::var_os("FYNKEL_EDITOR_TRACE").is_some() {
                    say(&format!("[trace:stderr] {line}"));
                }
                if !state.pending.is_empty() {
                    state.compiler.push(line.clone());
                }
                state.keep(format!("[flutter] {line}"))
            }
            Ok(Line::App(line)) => {
                if std::env::var_os("FYNKEL_EDITOR_TRACE").is_some() {
                    say(&format!("[trace:app] {line}"));
                }
            }
            Ok(Line::Closed) if direct => {
                if watching {
                    say("hot reload is off: flutter attach ended (the editor keeps running)");
                }
                state.app = None;
                editor_launch::set_updating(app_dir, None);
            }
            Ok(Line::Closed) | Ok(Line::AppClosed) => closed = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => closed = true,
        }
        if std::mem::take(&mut state.restart_due) {
            say("the code changed since this editor was built → hot restart");
            editor_launch::set_updating(app_dir, Some("restarting"));
            state.request(
                &stdin,
                "app.restart",
                "stale",
                json!({"fullRestart": true, "pause": false, "reason": "stale"}),
                Vec::new(),
            );
        }
        // Hot reload: a change settles for 150 ms (an editor's save writes
        // more than once) before one reload covers it.
        // The editor asked for a restart (it saved first).
        if let Some(request) = host.map(|h| h.join(RESTART_REQUEST)) {
            if state.app.is_some() && std::fs::remove_file(&request).is_ok() {
                say("the editor asked for a restart → hot restart");
                state.write_status("restarting", "", None, &[]);
                state.request(
                    &stdin,
                    "app.restart",
                    "restart",
                    json!({"fullRestart": true, "pause": false, "reason": "restart"}),
                    Vec::new(),
                );
            }
        }
        if !watching || state.app.is_none() {
            continue;
        }
        let Some(lib) = lib.as_deref() else { continue };
        let now = dart_files(lib, &seen);
        if now != seen {
            let files = changed_files(&seen, &now, project.unwrap_or(lib));
            seen = now;
            if !files.is_empty() {
                changed_at = Some(Instant::now());
                state.changed.extend(files);
            }
        } else if changed_at.is_some_and(|t| t.elapsed() >= Duration::from_millis(150)) {
            changed_at = None;
            let files = std::mem::take(&mut state.changed);
            // An editor part appeared or went (ADR-0028): the host's main
            // imports it, and only a restart runs main again.
            if let (Some(p), Some(name)) = (project, package.as_deref()) {
                match write_host_main(p, name) {
                    Ok(changed) => state.parts_changed |= changed,
                    Err(e) => say(&format!("could not update the host's editor parts: {e}")),
                }
            }
            say(&format!("changed: {} → hot reload", files.join(", ")));
            state.write_status("reloading", "", None, &files);
            state.request(
                &stdin,
                "app.restart",
                "app.restart",
                json!({"fullRestart": false, "pause": false, "reason": "save"}),
                files,
            );
        }
    }
    let status = match app.as_mut() {
        Some(app) => {
            let status = app.wait();
            if let Some(t) = tool.as_mut() {
                let _ = t.kill();
                let _ = t.wait();
            }
            status
        }
        None => tool.as_mut().expect("a build has its tool").wait(),
    };
    editor_launch::set_updating(app_dir, None);
    let code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(1);
    if !state.ready {
        eprintln!(
            "fynkel editor: the editor exited before it started (code {code}); the last lines:"
        );
        for l in &state.tail {
            eprintln!("  {l}");
        }
        if host.is_some() {
            if let Some(p) = project {
                eprintln!(
                    "a project whose code does not compile cannot open in its project editor: fix it, or open it without the game: fynkel editor --plain {}",
                    p.display()
                );
            }
        }
        state.write_status("failed", &state.tail.join("\n"), None, &[]);
        return ExitCode::from(1);
    }
    state.write_status("exited", "", None, &[]);
    if let Some(recording) = refresh {
        refresh_build(flutter, app_dir, recording);
    }
    ExitCode::from(code.clamp(0, 255) as u8)
}

/// Rebuilds the editor's app after a run that had to catch its code up,
/// with the window already gone, and records what it was built from. The
/// next open then launches current code directly. A failed build leaves
/// the old record: that open catches up again, as this one did.
fn refresh_build(flutter: &Path, app_dir: &Path, recording: editor_launch::Recording) {
    say("updating the built editor for the next open …");
    let at = Instant::now();
    let out = Command::new(flutter)
        .args(["build", "macos", "--debug"])
        .current_dir(app_dir)
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => {
            recording.finish();
            say(&format!("updated in {:.1} s", at.elapsed().as_secs_f64()));
        }
        Ok(o) => say(&format!(
            "could not update the built editor (the next open catches up again):\n{}",
            String::from_utf8_lossy(&o.stderr).trim_end()
        )),
        Err(e) => say(&format!("could not run {}: {e}", flutter.display())),
    }
}

/// Relays a Flutter tool's stdout and stderr, in order per stream.
fn spawn_readers(child: &mut Child, tx: mpsc::Sender<Line>) {
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let out_tx = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if out_tx.send(Line::Out(line)).is_err() {
                break;
            }
        }
        let _ = out_tx.send(Line::Closed);
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(Line::Err(line)).is_err() {
                break;
            }
        }
    });
}

enum Line {
    /// The Flutter tool's stdout (`flutter run` or `flutter attach`).
    Out(String),
    Err(String),
    /// The tool ended.
    Closed,
    /// The directly launched app said something.
    App(String),
    AppClosed,
}

struct Supervisor {
    app: Option<String>,
    next_id: u64,
    /// Requests in flight: id → (what for, sent at, files).
    pending: BTreeMap<u64, (String, Instant, Vec<String>)>,
    seq: u64,
    status: Option<PathBuf>,
    /// The last lines before the app started (why a build failed).
    tail: Vec<String>,
    ready: bool,
    /// Files changed since the last reload was sent (collected while a
    /// change settles).
    changed: Vec<String>,
    /// What the compiler printed while a reload was in flight (its errors
    /// arrive on stderr; the response only says "DevFS synchronization
    /// failed").
    compiler: Vec<String>,
    /// A launched app whose code is older than the files: restart it once
    /// attached ([Self::restart_due] asks the loop to).
    restart: bool,
    restart_due: bool,
    /// The last save added or removed an editor part: its reload is
    /// reported as rejected, so the editor restarts (saving first).
    parts_changed: bool,
    /// After a build: what it was built from, written once it is up.
    stamp: Option<editor_launch::Recording>,
    app_dir: PathBuf,
}

impl Supervisor {
    fn keep(&mut self, line: String) {
        if !self.ready {
            self.tail.push(line);
            if self.tail.len() > 40 {
                self.tail.remove(0);
            }
        }
    }

    fn request(
        &mut self,
        stdin: &Arc<Mutex<Option<ChildStdin>>>,
        method: &str,
        what: &str,
        mut params: Value,
        files: Vec<String>,
    ) {
        let Some(app) = self.app.clone() else { return };
        params["appId"] = json!(app);
        let id = self.next_id;
        self.next_id += 1;
        self.compiler.clear();
        self.pending
            .insert(id, (what.to_string(), Instant::now(), files));
        let line = json!([{"id": id, "method": method, "params": params}]).to_string();
        if let Ok(mut guard) = stdin.lock() {
            let Some(s) = guard.as_mut() else { return };
            let _ = writeln!(s, "{line}");
            let _ = s.flush();
        }
    }

    fn on_line(&mut self, line: &str, started: Instant) {
        if std::env::var_os("FYNKEL_EDITOR_TRACE").is_some() {
            say(&format!("[trace] {line}"));
        }
        let Some(messages) = parse_machine_line(line) else {
            self.keep(format!("[flutter] {line}"));
            return;
        };
        for msg in messages {
            if let Some(event) = msg.get("event").and_then(Value::as_str) {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                match event {
                    "app.start" => self.app = params["appId"].as_str().map(str::to_string),
                    "app.started" => {
                        self.app = params["appId"]
                            .as_str()
                            .map(str::to_string)
                            .or(self.app.take());
                        if std::mem::take(&mut self.restart) {
                            self.restart_due = true;
                        }
                        if self.ready {
                            // Attached to the launched app.
                            say(&format!(
                                "hot reload on ({:.1} s)",
                                started.elapsed().as_secs_f64()
                            ));
                            continue;
                        }
                        self.ready = true;
                        self.tail.clear();
                        say(&format!("ready ({:.1} s)", started.elapsed().as_secs_f64()));
                        self.write_status("running", "", None, &[]);
                        if let Some(recording) = self.stamp.take() {
                            recording.finish();
                        }
                    }
                    "app.log" => {
                        let log = params["log"].as_str().unwrap_or("").trim_end();
                        self.keep(format!("[app] {log}"));
                        if params["error"].as_bool() == Some(true) {
                            say(&format!("[app] {log}"));
                        }
                    }
                    "app.progress" => {
                        if let Some(m) = params["message"].as_str() {
                            if params["finished"].as_bool() != Some(true) && !m.is_empty() {
                                self.keep(format!("[flutter] {m}"));
                                if !self.ready {
                                    say(m);
                                    self.write_status("building", m, None, &[]);
                                }
                            }
                        }
                    }
                    "daemon.logMessage" => {
                        if let Some(m) = params["message"].as_str() {
                            self.keep(format!("[flutter] {m}"));
                        }
                    }
                    "app.stop" => say("the editor stopped"),
                    _ => {}
                }
            } else if let Some(id) = msg.get("id").and_then(Value::as_u64) {
                let Some((method, at, files)) = self.pending.remove(&id) else {
                    continue;
                };
                let ms = at.elapsed().as_millis() as u64;
                let (ok, mut message) = response_outcome(&msg);
                let errors = compiler_errors(&std::mem::take(&mut self.compiler));
                if !ok && !errors.is_empty() {
                    message = errors;
                }
                if method == "stale" {
                    editor_launch::set_updating(&self.app_dir, None);
                    if ok {
                        say(&format!("up to date in {ms} ms"));
                    } else {
                        say(&format!("the hot restart FAILED in {ms} ms: {message}"));
                        self.write_status("reloadFailed", &message, Some(ms), &files);
                    }
                } else if method == "restart" {
                    if ok {
                        say(&format!("restarted in {ms} ms"));
                        self.write_status("restarted", "", Some(ms), &files);
                    } else {
                        say(&format!("the hot restart FAILED in {ms} ms: {message}"));
                        self.write_status("reloadFailed", &message, Some(ms), &files);
                    }
                } else if method == "app.restart" {
                    if ok && std::mem::take(&mut self.parts_changed) {
                        let why = "an editor part (lib/editor/editor.dart) was added or removed: a restart applies it";
                        say(&format!("reloaded in {ms} ms; {why}"));
                        self.write_status(
                            "reloadRejected",
                            &format!("Reload rejected: {why}"),
                            Some(ms),
                            &files,
                        );
                    } else if ok {
                        say(&format!("reloaded in {ms} ms"));
                        self.write_status("reloaded", "", Some(ms), &files);
                    } else if message.starts_with("Reload rejected") {
                        // The code compiles, but a hot reload cannot apply
                        // it: the editor offers a restart.
                        say(&format!(
                            "reload rejected in {ms} ms, a restart applies it: {message}"
                        ));
                        self.write_status("reloadRejected", &message, Some(ms), &files);
                    } else {
                        say(&format!("reload FAILED in {ms} ms: {message}"));
                        self.write_status("reloadFailed", &message, Some(ms), &files);
                    }
                }
            }
        }
    }

    /// The next event in the status file.
    fn write_status(&mut self, state: &str, message: &str, ms: Option<u64>, files: &[String]) {
        let Some(path) = &self.status else { return };
        self.seq += 1;
        write_status_file(path, self.seq, state, message, ms, files);
    }
}

/// `<host>/status.json`, written whole and renamed into place.
fn write_status_file(
    path: &Path,
    seq: u64,
    state: &str,
    message: &str,
    ms: Option<u64>,
    files: &[String],
) {
    let at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut body = json!({
        "version": 1,
        "seq": seq,
        "state": state,
        "pid": std::process::id(),
        "at": at,
    });
    if !message.is_empty() {
        body["message"] = json!(message);
    }
    if let Some(ms) = ms {
        body["ms"] = json!(ms);
    }
    if !files.is_empty() {
        body["files"] = json!(files);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, format!("{body:#}\n")).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// `flutter run --machine` prints JSON-RPC messages as one-line arrays;
/// anything else is ordinary output.
pub fn parse_machine_line(line: &str) -> Option<Vec<Value>> {
    let t = line.trim();
    if !(t.starts_with("[{") && t.ends_with("}]")) {
        return None;
    }
    match serde_json::from_str::<Value>(t) {
        Ok(Value::Array(v)) => Some(v),
        _ => None,
    }
}

/// A response's outcome: an `error`, or a `result` with a non-zero `code`
/// (a rejected reload, a compile error), is a failure with its message.
pub fn response_outcome(msg: &Value) -> (bool, String) {
    if let Some(e) = msg.get("error") {
        let m = e
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| e.to_string());
        return (false, m);
    }
    let r = &msg["result"];
    let code = r["code"].as_i64().unwrap_or(0);
    let message = r["message"].as_str().unwrap_or("").trim().to_string();
    (code == 0, message)
}

/// The compiler's report from the lines it printed, with its paths (relative
/// to the host, `../../lib/…`) made relative to the project; at most 30 lines.
pub fn compiler_errors(lines: &[String]) -> String {
    let mut out: Vec<String> = lines
        .iter()
        .map(|l| l.replace("../../lib/", "lib/"))
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !out.iter().any(|l| l.contains("Error:")) {
        return String::new();
    }
    out.truncate(30);
    out.join("\n")
}

/// The project's package name when it can open in a project editor, or why not.
pub fn game_package(project: &Path) -> Result<String, String> {
    let pubspec = std::fs::read_to_string(project.join("pubspec.yaml")).map_err(|_| {
        format!(
            "{} is not a Dart package (no pubspec.yaml)",
            project.display()
        )
    })?;
    if !project.join("lib").join("fynkel_game.dart").is_file() {
        return Err(format!(
            "{} has no lib/fynkel_game.dart (the game's entry point, `Widget buildGame(FynkelLaunch launch)`)",
            project.display()
        ));
    }
    pubspec
        .lines()
        .find_map(|l| l.strip_prefix("name:"))
        .map(|n| n.trim().trim_matches(['"', '\'']).to_string())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| format!("{}/pubspec.yaml has no name", project.display()))
}

/// Writes the host package under [project] (only files whose bytes change).
pub fn generate_host(project: &Path, package: &str, editor: &Path) -> Result<PathBuf, String> {
    let host = project.join(HOST_DIR);
    let q = |p: &Path| p.display().to_string().replace('\'', "''");
    write_if_changed(
        &host.join("pubspec.yaml"),
        &format!(
            "# GENERATED by `fynkel editor` — the editor with {package} compiled in (ADR-0021). Do not edit.\n\
             name: fynkel_editor_host\n\
             publish_to: none\n\
             environment:\n  sdk: ^3.13.0\n\
             dependencies:\n  flutter:\n    sdk: flutter\n  fynkel_editor:\n    path: '{editor}'\n  {package}:\n    path: '{project}'\n\
             flutter:\n  uses-material-design: true\n\
             hooks:\n  user_defines:\n    fynkel:\n      features: [authoring]\n",
            editor = q(editor),
            project = q(project),
        ),
    )?;
    write_host_main(project, package)?;
    write_if_changed(
        &host.join(".gitignore"),
        "# GENERATED by `fynkel editor`.\n*\n",
    )?;
    copy_tree(&editor.join("macos"), &host.join("macos"))?;
    Ok(host)
}

/// Writes the host's `lib/main.dart`: the game, and every editor part
/// [editor_parts] finds (ADR-0028). True when its bytes changed — a new
/// or removed editor part, which only a hot restart applies.
pub fn write_host_main(project: &Path, package: &str) -> Result<bool, String> {
    let path = project.join(HOST_DIR).join("lib").join("main.dart");
    let dir = project
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\'', "\\'");
    let parts = editor_parts(project, package);
    let mut imports = String::new();
    let mut list = String::new();
    for (i, part) in parts.iter().enumerate() {
        imports.push_str(&format!(
            "import 'package:{part}/editor/editor.dart' as editor{i};\n"
        ));
        list.push_str(&format!(
            "    EditorPart('{part}', editor{i}.registerEditor),\n"
        ));
    }
    // The game's behaviours and data types, for what the extensions write
    // (ADR-0028 step 2).
    let (behaviours_import, behaviours_arg) = if project.join(crate::behaviours::DART_OUT).is_file()
    {
        let under_lib = crate::behaviours::DART_OUT.trim_start_matches("lib/");
        (
            format!("import 'package:{package}/{under_lib}' as behaviours;\n"),
            ", registerBehaviours: behaviours.registerBehaviours",
        )
    } else {
        (String::new(), "")
    };
    let extensions = if parts.is_empty() {
        String::new()
    } else {
        format!("  extensions: const [\n{list}  ],\n")
    };
    let text = format!(
        "// GENERATED by `fynkel editor` — the editor with {package} compiled in (ADR-0021). Do not edit.\n\
         import 'package:fynkel_editor/fynkel_editor.dart';\n\
         import 'package:{package}/fynkel_game.dart' as game;\n\
         {behaviours_import}\
         {imports}\
         \n\
         void main(List<String> args) => runFynkelEditor(\n  \
           args,\n  \
           game: const EditorGame(name: '{package}', projectDir: '{dir}', build: game.buildGame{behaviours_arg}),\n\
         {extensions}\
         );\n"
    );
    let changed = std::fs::read(&path).map_or(true, |b| b != text.as_bytes());
    write_if_changed(&path, &text)?;
    Ok(changed)
}

/// The packages with an editor part (`lib/editor/editor.dart`, ADR-0028)
/// among the game [package] and what it depends on, in the order their
/// `registerEditor` runs: dependencies before their dependents, otherwise
/// alphabetical, the game last — the package graph (ADR-0029 §1). Without
/// a resolved `.dart_tool/package_config.json`, only the game's own part
/// is found.
pub fn editor_parts(project: &Path, package: &str) -> Vec<String> {
    fynkel_document::packages::package_graph(project, package).editor_parts()
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read(path).is_ok_and(|b| b == text.as_bytes()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The editor's macOS runner, without Flutter's generated and per-user parts.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    const SKIP: &[&str] = &["ephemeral", "Pods", "xcuserdata", "build", ".DS_Store"];
    let entries = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if SKIP.iter().any(|s| name == *s) {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            let bytes = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
            if std::fs::read(&dst).is_ok_and(|b| b == bytes) {
                continue;
            }
            std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
            std::fs::write(&dst, bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
        }
    }
    Ok(())
}

/// Every `.dart` file under [lib] with its modification time.
pub fn dart_files(lib: &Path, before: &Snapshot) -> Snapshot {
    let mut out = BTreeMap::new();
    let mut stack = vec![lib.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "dart") {
                let Ok(m) = e.metadata().and_then(|m| m.modified()) else {
                    continue;
                };
                // Read only what was touched since the last look.
                let hash = match before.get(&p) {
                    Some(&(was, hash)) if was == m => hash,
                    _ => match std::fs::read(&p) {
                        Ok(bytes) => content_hash(&bytes),
                        Err(_) => continue,
                    },
                };
                out.insert(p, (m, hash));
            }
        }
    }
    out
}

/// Each `.dart` file under lib/: when it was written and what it holds.
pub type Snapshot = BTreeMap<PathBuf, (SystemTime, u64)>;

fn content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Files added, removed or changed between two [dart_files] snapshots,
/// relative to [base]. A file written with the bytes it had is not a
/// change: `fynkel codegen --check` (the editor runs it when a project
/// opens) rewrites the generated files and puts them back.
pub fn changed_files(before: &Snapshot, after: &Snapshot, base: &Path) -> Vec<String> {
    let mut out: Vec<String> = before
        .keys()
        .chain(after.keys())
        .filter(|p| before.get(*p).map(|e| e.1) != after.get(*p).map(|e| e.1))
        .map(|p| p.strip_prefix(base).unwrap_or(p).display().to_string())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn find_flutter() -> Option<PathBuf> {
    std::env::var_os("FYNKEL_FLUTTER")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(crate::create::find_flutter)
}

/// One line for the person at the terminal; a closed terminal (the
/// launcher that started us has quit) is not an error.
fn say(line: &str) {
    let _ = writeln!(std::io::stdout(), "fynkel editor: {line}");
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_lines_and_outcomes() {
        assert!(parse_machine_line("Launching lib/main.dart on macOS").is_none());
        let v = parse_machine_line(r#"[{"event":"app.started","params":{"appId":"a1"}}]"#).unwrap();
        assert_eq!(v[0]["params"]["appId"], "a1");
        let ok = json!({"id": 3, "result": {"code": 0, "message": ""}});
        assert_eq!(response_outcome(&ok), (true, String::new()));
        let rejected = json!({"id": 4, "result": {"code": 1, "message": "Hot reload rejected: const class changed"}});
        assert_eq!(
            response_outcome(&rejected),
            (false, "Hot reload rejected: const class changed".into())
        );
        let err = json!({"id": 5, "error": "compile error"});
        assert_eq!(response_outcome(&err), (false, "compile error".into()));
    }

    #[test]
    fn a_host_is_generated_once_and_left_alone_when_nothing_changed() {
        let dir = std::env::temp_dir().join(format!("fynkel_editor_host_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("my_game");
        let editor = dir.join("editor");
        std::fs::create_dir_all(project.join("lib")).unwrap();
        std::fs::create_dir_all(editor.join("macos/Runner")).unwrap();
        std::fs::create_dir_all(editor.join("macos/Flutter/ephemeral")).unwrap();
        std::fs::write(editor.join("macos/Runner/Info.plist"), "plist").unwrap();
        std::fs::write(editor.join("macos/Flutter/ephemeral/x"), "generated").unwrap();

        assert!(game_package(&project)
            .unwrap_err()
            .contains("no pubspec.yaml"));
        std::fs::write(project.join("pubspec.yaml"), "name: my_game\n").unwrap();
        assert!(game_package(&project)
            .unwrap_err()
            .contains("lib/fynkel_game.dart"));
        std::fs::write(project.join("lib/fynkel_game.dart"), "").unwrap();
        assert_eq!(game_package(&project).unwrap(), "my_game");

        let host = generate_host(&project, "my_game", &editor).unwrap();
        assert_eq!(host, project.join(HOST_DIR));
        let pubspec = std::fs::read_to_string(host.join("pubspec.yaml")).unwrap();
        assert!(pubspec.contains(&format!("path: '{}'", editor.display())));
        assert!(pubspec.contains("  my_game:\n"));
        assert!(pubspec.contains("features: [authoring]"));
        let main = std::fs::read_to_string(host.join("lib/main.dart")).unwrap();
        assert!(main.contains("import 'package:my_game/fynkel_game.dart' as game;"));
        assert!(main.contains("build: game.buildGame"));
        assert!(host.join("macos/Runner/Info.plist").is_file());
        assert!(
            !host.join("macos/Flutter/ephemeral").exists(),
            "Flutter's generated files stay out"
        );

        // A second run rewrites nothing (Flutter's build cache holds).
        let stamp = std::fs::metadata(host.join("lib/main.dart"))
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(Duration::from_millis(20));
        generate_host(&project, "my_game", &editor).unwrap();
        assert_eq!(
            std::fs::metadata(host.join("lib/main.dart"))
                .unwrap()
                .modified()
                .unwrap(),
            stamp
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_parts_are_found_in_the_package_graph_dependencies_first() {
        let dir = std::env::temp_dir().join(format!("fynkel_editor_parts_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let package = |name: &str, pubspec: &str, part: bool| {
            let root = dir.join(name);
            std::fs::create_dir_all(root.join("lib/editor")).unwrap();
            std::fs::write(
                root.join("pubspec.yaml"),
                format!("name: {name}\n{pubspec}"),
            )
            .unwrap();
            if part {
                std::fs::write(root.join("lib/editor/editor.dart"), "").unwrap();
            }
            root
        };
        let game = package(
            "my game",
            "dependencies:\n  flutter:\n    sdk: flutter\n  zeta_tools: any # a tool\n  alpha_tools:\n    path: ../alpha_tools\n\
             dev_dependencies:\n  dev_tools: any\n",
            false,
        );
        package("zeta_tools", "", true);
        package(
            "alpha_tools",
            "dependencies:\n  zeta_tools: any\n  plain: any\n",
            true,
        );
        package("plain", "dependencies:\n  beta_tools: any\n", false);
        package("beta_tools", "", true);
        package("dev_tools", "", true);
        package("flutter", "", false);
        // Without a resolved graph only the game's own part counts.
        assert_eq!(editor_parts(&game, "my_game"), Vec::<String>::new());
        std::fs::write(game.join("lib/editor/editor.dart"), "").unwrap();
        assert_eq!(editor_parts(&game, "my_game"), vec!["my_game"]);

        std::fs::create_dir_all(game.join(".dart_tool")).unwrap();
        let config = json!({"configVersion": 2, "packages": [
            {"name": "my_game", "rootUri": "../", "packageUri": "lib/"},
            {"name": "zeta_tools", "rootUri": format!("file://{}", dir.join("zeta_tools").display()), "packageUri": "lib/"},
            {"name": "alpha_tools", "rootUri": "../../alpha_tools/", "packageUri": "lib/"},
            {"name": "plain", "rootUri": "../../plain", "packageUri": "lib/"},
            {"name": "beta_tools", "rootUri": "../../beta_tools", "packageUri": "lib/"},
            {"name": "dev_tools", "rootUri": "../../dev_tools", "packageUri": "lib/"},
            {"name": "flutter", "rootUri": "../../flutter", "packageUri": "lib/"},
        ]});
        std::fs::write(
            game.join(".dart_tool/package_config.json"),
            config.to_string(),
        )
        .unwrap();
        // beta before alpha (alpha reaches it through plain), zeta before
        // alpha; dev_dependencies are not the host's; the game last.
        assert_eq!(
            editor_parts(&game, "my_game"),
            vec!["beta_tools", "zeta_tools", "alpha_tools", "my_game"]
        );

        std::fs::create_dir_all(game.join("lib")).unwrap();
        std::fs::write(game.join("lib/fynkel_game.dart"), "").unwrap();
        let editor = dir.join("editor");
        std::fs::create_dir_all(editor.join("macos")).unwrap();
        let host = generate_host(&game, "my_game", &editor).unwrap();
        let main = std::fs::read_to_string(host.join("lib/main.dart")).unwrap();
        assert!(main.contains("import 'package:beta_tools/editor/editor.dart' as editor0;\n"));
        assert!(main.contains("import 'package:my_game/editor/editor.dart' as editor3;\n"));
        assert!(main.contains(
            "  extensions: const [\n    EditorPart('beta_tools', editor0.registerEditor),\n"
        ));
        assert!(main.contains("    EditorPart('my_game', editor3.registerEditor),\n  ],\n);\n"));
        // Nothing new: unchanged. The game's part removed: changed.
        assert!(!write_host_main(&game, "my_game").unwrap());
        std::fs::remove_file(game.join("lib/editor/editor.dart")).unwrap();
        assert!(write_host_main(&game, "my_game").unwrap());
        let main = std::fs::read_to_string(host.join("lib/main.dart")).unwrap();
        assert!(!main.contains("my_game/editor"));
        assert!(!main.contains("registerBehaviours"));
        // Its generated behaviours: passed for what the extensions write.
        std::fs::create_dir_all(game.join("lib/generated")).unwrap();
        std::fs::write(game.join("lib/generated/behaviours.g.dart"), "").unwrap();
        assert!(write_host_main(&game, "my_game").unwrap());
        let main = std::fs::read_to_string(host.join("lib/main.dart")).unwrap();
        assert!(
            main.contains("import 'package:my_game/generated/behaviours.g.dart' as behaviours;\n")
        );
        assert!(main.contains(
            "build: game.buildGame, registerBehaviours: behaviours.registerBehaviours),"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_compile_error_is_the_compilers_words_with_project_paths() {
        let lines: Vec<String> = [
            "../../lib/game/collectible.dart:66:13: Error: Can't find '}' to match '{'.",
            "int broken( {",
            "            ^",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            compiler_errors(&lines),
            "lib/game/collectible.dart:66:13: Error: Can't find '}' to match '{'.\nint broken( {\n            ^"
        );
        assert_eq!(compiler_errors(&["Syncing files".to_string()]), "");
    }

    #[test]
    fn changed_files_lists_added_removed_and_touched() {
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + Duration::from_secs(1);
        let base = Path::new("/p");
        let before = BTreeMap::from([
            (PathBuf::from("/p/lib/a.dart"), (t0, 1)),
            (PathBuf::from("/p/lib/b.dart"), (t0, 2)),
            (PathBuf::from("/p/lib/g.dart"), (t0, 3)),
        ]);
        let after = BTreeMap::from([
            (PathBuf::from("/p/lib/a.dart"), (t1, 4)),
            (PathBuf::from("/p/lib/c.dart"), (t0, 5)),
            // Rewritten with the same bytes: not a change.
            (PathBuf::from("/p/lib/g.dart"), (t1, 3)),
        ]);
        assert_eq!(
            changed_files(&before, &after, base),
            ["lib/a.dart", "lib/b.dart", "lib/c.dart"]
        );
    }
}
