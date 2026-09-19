//! `fynkel run [--project <dir>] [-- <flutter run args>]` (blueprint §13.3):
//! `flutter run` in the project with `FYNKEL_PROJECT` defined, so the app's
//! `Game.load(projectDirectory())` finds `fynkel.project.json` whatever the
//! process's working directory is. The project is validated first; a
//! project with errors does not run.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fynkel_document::{has_errors, validate_project};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel run [--project <dir>] [-d <device>] [--release] [-- <flutter run args>]\n                                               validate the project, then `flutter run` with FYNKEL_PROJECT set to it"
    );
    ExitCode::from(2)
}

pub fn run(root: Option<&Path>, args: &[String]) -> ExitCode {
    let mut project = PathBuf::from(".");
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
    let report = validate_project(&project, &schemas, Some(&expected));
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
