//! `fynkel run [--project <dir>] [-- <flutter run args>]` (blueprint §13.3):
//! `flutter run` in the project with `FYNKEL_PROJECT` defined, so the app's
//! `Game.load(projectDirectory())` finds `fynkel.project.json` whatever the
//! process's working directory is. The project is validated first (a
//! project with errors does not run), then cooked into `.fynkel/cache`
//! and staged into `fynkel_cache/` (`crate::bundle`): on this Mac the app
//! loads from the cache by path; on a device (`-d <iphone>`) that path
//! does not exist and the app unpacks the staged copy it carries as
//! Flutter assets (`--no-cook` skips both; a warm cache costs
//! milliseconds).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fynkel_document::{has_errors, validate_project_with};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel run [--project <dir>] [--no-cook] [-d <device>] [--release] [-- <flutter run args>]\n                                               validate and cook the project, then `flutter run` with FYNKEL_PROJECT set to it"
    );
    ExitCode::from(2)
}

pub fn run(root: Option<&Path>, args: &[String]) -> ExitCode {
    let mut project = PathBuf::from(".");
    let mut cook = true;
    let mut passthrough: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--project" => {
                i += 1;
                match args.get(i) {
                    Some(p) => project = PathBuf::from(p),
                    None => return usage(),
                }
            }
            "--no-cook" => cook = false,
            "--root" => i += 1,
            "--" => {
                passthrough.extend(args[i + 1..].iter().cloned());
                break;
            }
            other => passthrough.push(other.to_string()),
        }
        i += 1;
    }
    let schemas = match crate::schemas::load(root, None) {
        Ok(s) => s,
        Err(d) => {
            eprintln!("fynkel run: {d}");
            return ExitCode::from(2);
        }
    };
    let expected = fynkel_import::expected_lock();
    let sources = fynkel_import::ImportedPrefabs::default();
    let report = validate_project_with(&project, &schemas, Some(&expected), &sources);
    for d in &report.diagnostics {
        eprintln!("{d}");
    }
    if has_errors(&report.diagnostics) {
        eprintln!("fynkel run: the project has errors; fix them first (`fynkel validate`)");
        return ExitCode::from(1);
    }
    let Some(flutter) = crate::create::find_flutter() else {
        eprintln!("fynkel run: `flutter` is not on PATH");
        return ExitCode::from(2);
    };
    let abs = std::fs::canonicalize(&project).unwrap_or(project.clone());
    // The app reads the cache only (`World.loadProject` opens
    // `<project>/.fynkel/cache`): cook it, same as `fynkel cook` would.
    let target = crate::cook::Target {
        project: Some(abs.clone()),
        no_cook: !cook,
        ..crate::cook::Target::default()
    };
    // A project without a Flutter app has nothing to run (the stress
    // scene is generated bare unless `generate.py --app`).
    if !crate::bundle::is_app(&abs) {
        eprintln!(
            "fynkel run: {} is not a Flutter app (no pubspec.yaml) — `fynkel create` makes one; benchmarks/stress: `generate.py --app`",
            abs.display()
        );
        return ExitCode::from(1);
    }
    if let Err(code) = crate::cook::prepare("fynkel run", &target) {
        return code;
    }
    // On a device the app finds no project directory and opens the
    // cache it bundles; it must exist before `flutter run` builds.
    if !abs
        .join(crate::bundle::STAGE_DIR)
        .join("index.json")
        .is_file()
    {
        eprintln!(
            "fynkel run: {} is not staged (run without --no-cook once)",
            abs.join(crate::bundle::STAGE_DIR).display()
        );
        return ExitCode::from(1);
    }
    let status = Command::new(flutter)
        .arg("run")
        .arg(format!("--dart-define=FYNKEL_PROJECT={}", abs.display()))
        .args(&passthrough)
        .current_dir(&abs)
        .status();
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("fynkel run: cannot run flutter: {e}");
            ExitCode::from(2)
        }
    }
}
