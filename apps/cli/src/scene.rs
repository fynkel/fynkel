//! `fynkel scene …` (§14.1): `inspect` and `validate` for scene documents.
//! Same exit codes and `--json` shape as `fynkel document`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::scene::{inspect_json, inspect_text};
use fynkel_document::{diagnostics_json, has_errors, Document, DocumentKind, Scene, SchemaSet};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel scene inspect <path> [--json] [--schemas <dir>]     entity tree and component summary\n  fynkel scene validate <path>... [--json] [--schemas <dir>] schema + semantic validation"
    );
    ExitCode::from(2)
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        return usage();
    };
    let mut paths = Vec::new();
    let mut json = false;
    let mut schemas_dir = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--schemas" => {
                i += 1;
                schemas_dir = args.get(i).map(PathBuf::from);
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel scene: unknown argument {other}");
                return usage();
            }
            p => paths.push(PathBuf::from(p)),
        }
        i += 1;
    }
    if paths.is_empty() {
        return usage();
    }
    let schemas = match SchemaSet::load(&schemas_dir.unwrap_or_else(|| root.join("schemas"))) {
        Ok(s) => s,
        Err(d) => {
            if json {
                println!("{}", diagnostics_json(&[d]));
            } else {
                eprintln!("{d}");
            }
            return ExitCode::from(1);
        }
    };
    match sub.as_str() {
        "inspect" => inspect(&paths[0], json, &schemas),
        "validate" => validate(&paths, json, &schemas),
        _ => usage(),
    }
}

fn inspect(path: &Path, json: bool, schemas: &SchemaSet) -> ExitCode {
    let scene = match Document::read(path, Some(DocumentKind::Scene)) {
        Ok(doc) => Scene::from_document(doc),
        Err(diags) => {
            if json {
                println!("{}", diagnostics_json(&diags));
            } else {
                for d in &diags {
                    eprintln!("{d}");
                }
            }
            return ExitCode::from(1);
        }
    };
    let diags = scene.validate(schemas);
    if json {
        let mut v = inspect_json(&scene);
        v["ok"] = serde_json::Value::Bool(!has_errors(&diags));
        v["diagnostics"] = diagnostics_json(&diags)["diagnostics"].clone();
        println!("{v}");
    } else {
        print!("{}", inspect_text(&scene));
        for d in &diags {
            eprintln!("{d}");
        }
    }
    if has_errors(&diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn validate(paths: &[PathBuf], json: bool, schemas: &SchemaSet) -> ExitCode {
    let mut diags = Vec::new();
    for path in paths {
        match Document::read(path, Some(DocumentKind::Scene)) {
            Ok(doc) => diags.extend(Scene::from_document(doc).validate(schemas)),
            Err(d) => diags.extend(d),
        }
    }
    if json {
        println!("{}", diagnostics_json(&diags));
    } else {
        for d in &diags {
            eprintln!("{d}");
        }
        if !has_errors(&diags) {
            eprintln!("fynkel scene validate: {} scene(s) ok", paths.len());
        }
    }
    if has_errors(&diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
