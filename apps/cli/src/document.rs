//! `fynkel document …` (M2-02): the generic document commands every kind
//! shares. Kind-specific commands (`fynkel scene …`) build on these.
//!
//! Exit codes (§14.1): 0 ok, 1 diagnostics with errors (or I/O failure),
//! 2 usage. `--json` prints `{ "ok": bool, "diagnostics": [...] }` with
//! the §14.2 records; the human form prints one diagnostic per line.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::{
    diagnostics_json, has_errors, Diagnostic, Document, DocumentKind, SchemaSet,
};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel document validate <path>... [--json] [--schemas <dir>] [--kind <kind>]\n  fynkel document format <path>... [--check] [--json]     rewrite in canonical form (or report drift)\n  fynkel document new <kind> <name> [--out <path>]        print (or write) a fresh document\n\nkinds: scene, prefab, material, shader, project, meta"
    );
    ExitCode::from(2)
}

struct Opts {
    paths: Vec<PathBuf>,
    json: bool,
    check: bool,
    schemas: Option<PathBuf>,
    kind: Option<DocumentKind>,
    out: Option<PathBuf>,
}

fn parse_opts(args: &[String]) -> Result<Opts, ExitCode> {
    let mut o = Opts {
        paths: Vec::new(),
        json: false,
        check: false,
        schemas: None,
        kind: None,
        out: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--check" => o.check = true,
            "--schemas" => {
                i += 1;
                o.schemas = args.get(i).map(PathBuf::from);
            }
            "--out" => {
                i += 1;
                o.out = args.get(i).map(PathBuf::from);
            }
            "--kind" => {
                i += 1;
                o.kind = args.get(i).and_then(|k| DocumentKind::parse_name(k));
                if o.kind.is_none() {
                    eprintln!("fynkel document: unknown kind");
                    return Err(usage());
                }
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel document: unknown argument {other}");
                return Err(usage());
            }
            p => o.paths.push(PathBuf::from(p)),
        }
        i += 1;
    }
    Ok(o)
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        return usage();
    };
    let opts = match parse_opts(&args[1..]) {
        Ok(o) => o,
        Err(code) => return code,
    };
    match sub.as_str() {
        "validate" => validate(root, opts),
        "format" => format(opts),
        "new" => new(opts),
        _ => usage(),
    }
}

fn report(diags: &[Diagnostic], json: bool) -> ExitCode {
    if json {
        println!("{}", diagnostics_json(diags));
    } else {
        for d in diags {
            eprintln!("{d}");
        }
    }
    if has_errors(diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn validate(root: &Path, opts: Opts) -> ExitCode {
    if opts.paths.is_empty() {
        return usage();
    }
    let schemas_dir = opts.schemas.clone().unwrap_or_else(|| root.join("schemas"));
    let schemas = match SchemaSet::load(&schemas_dir) {
        Ok(s) => s,
        Err(d) => return report(&[d], opts.json),
    };
    let mut diags = Vec::new();
    for path in &opts.paths {
        match Document::read(path, opts.kind) {
            Ok(doc) => diags.extend(doc.validate(&schemas)),
            Err(d) => diags.extend(d),
        }
    }
    if !opts.json && !has_errors(&diags) {
        eprintln!(
            "fynkel document validate: {} document(s) ok{}",
            opts.paths.len(),
            if diags.is_empty() {
                String::new()
            } else {
                format!(" with {} warning(s)", diags.len())
            }
        );
    }
    report(&diags, opts.json)
}

fn format(opts: Opts) -> ExitCode {
    if opts.paths.is_empty() {
        return usage();
    }
    let mut diags = Vec::new();
    let mut drift = Vec::new();
    for path in &opts.paths {
        let doc = match Document::read(path, opts.kind) {
            Ok(doc) => doc,
            Err(d) => {
                diags.extend(d);
                continue;
            }
        };
        let text = match doc.write() {
            Ok(t) => t,
            Err(d) => {
                diags.push(d.with_document(path.display().to_string()));
                continue;
            }
        };
        let current = std::fs::read_to_string(path).unwrap_or_default();
        if current == text {
            continue;
        }
        if opts.check {
            drift.push(path.clone());
        } else if let Err(e) = std::fs::write(path, text) {
            diags.push(
                Diagnostic::error(fynkel_document::Code::IO, format!("cannot write: {e}"))
                    .with_document(path.display().to_string()),
            );
        }
    }
    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["drift"] = serde_json::Value::Array(
            drift
                .iter()
                .map(|p| serde_json::Value::String(p.display().to_string()))
                .collect(),
        );
        v["ok"] = serde_json::Value::Bool(!has_errors(&diags) && drift.is_empty());
        println!("{v}");
    } else {
        for d in &diags {
            eprintln!("{d}");
        }
        for p in &drift {
            eprintln!(
                "fynkel document format --check: not canonical: {}",
                p.display()
            );
        }
    }
    if has_errors(&diags) || !drift.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn new(opts: Opts) -> ExitCode {
    let (Some(kind), Some(name)) = (
        opts.paths
            .first()
            .and_then(|p| DocumentKind::parse_name(&p.to_string_lossy())),
        opts.paths.get(1),
    ) else {
        return usage();
    };
    let doc = Document::new(kind, &name.to_string_lossy());
    let text = doc.write().expect("fresh documents are writable");
    match opts.out {
        Some(path) => match std::fs::write(&path, text) {
            Ok(()) => {
                if opts.json {
                    println!(
                        "{}",
                        serde_json::json!({ "ok": true, "path": path.display().to_string(), "id": doc.envelope.id.to_string() })
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => report(
                &[
                    Diagnostic::error(fynkel_document::Code::IO, format!("cannot write: {e}"))
                        .with_document(path.display().to_string()),
                ],
                opts.json,
            ),
        },
        None => {
            print!("{text}");
            ExitCode::SUCCESS
        }
    }
}
