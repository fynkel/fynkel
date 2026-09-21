//! `fynkel project describe|lock`, `fynkel validate` and `fynkel schema
//! list|describe` (§14.1, blueprint §19.2c item 13). `validate` is the
//! one command that checks a whole project: the project file, the lock
//! against this build, every document, sidecar and reference under the
//! asset root. Exit codes are fixed here and everywhere: 0 ok (warnings
//! allowed), 1 diagnostics with errors, 2 usage or a project that cannot
//! be opened at all (no `fynkel.project.json`, unreadable schemas).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::asset::AssetDatabase;
use fynkel_document::project::{describe_json, describe_text, LOCK_FILE, PROJECT_FILE};
use fynkel_document::{
    diagnostics_json, has_errors, validate_project_with, Code, Diagnostic, DocumentKind, Lock,
    Project, SchemaSet,
};

pub fn usage_project() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel project describe [--project <dir>] [--json] [--schemas <dir>]\n                                               the project file: startup scene, input actions, quality, components, targets, lock\n  fynkel project lock [--project <dir>] [--check] [--json]\n                                               write fynkel.lock.json for this build (--check: compare only)"
    );
    ExitCode::from(2)
}

pub fn usage_validate() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel validate [--project <dir>] [--json] [--schemas <dir>]\n                                               validate the whole project: project file, lock, every document, sidecar and reference"
    );
    ExitCode::from(2)
}

pub fn usage_schema() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel schema list [--json]                  every document kind and component type this build knows\n  fynkel schema describe <type> [--json]       one component type (fields, sizes) or document kind (schema, envelope)"
    );
    ExitCode::from(2)
}

struct Opts {
    project: PathBuf,
    json: bool,
    check: bool,
    schemas_dir: Option<PathBuf>,
    positional: Vec<String>,
}

fn parse(args: &[String], name: &str) -> Option<Opts> {
    let mut o = Opts {
        project: PathBuf::from("."),
        json: false,
        check: false,
        schemas_dir: None,
        positional: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--check" => o.check = true,
            "--project" => {
                i += 1;
                o.project = PathBuf::from(args.get(i)?);
            }
            "--schemas" => {
                i += 1;
                o.schemas_dir = Some(PathBuf::from(args.get(i)?));
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel {name}: unknown argument {other}");
                return None;
            }
            p => o.positional.push(p.to_string()),
        }
        i += 1;
    }
    Some(o)
}

// As elsewhere: a `Diagnostic` is the error type on purpose.
#[allow(clippy::result_large_err)]
fn load_schemas(root: Option<&Path>, opts: &Opts) -> Result<SchemaSet, Diagnostic> {
    crate::schemas::load(root, opts.schemas_dir.as_deref())
}

/// `fynkel project …`.
pub fn run_project(root: Option<&Path>, args: &[String]) -> ExitCode {
    let Some((sub, rest)) = args.split_first() else {
        return usage_project();
    };
    let Some(opts) = parse(rest, "project") else {
        return usage_project();
    };
    if !opts.positional.is_empty() {
        return usage_project();
    }
    match sub.as_str() {
        "describe" => describe(root, &opts),
        "lock" => lock(root, &opts),
        _ => usage_project(),
    }
}

fn describe(root: Option<&Path>, opts: &Opts) -> ExitCode {
    let schemas = match load_schemas(root, opts) {
        Ok(s) => s,
        Err(d) => return cannot_open(&[d], opts.json),
    };
    let project = match Project::read(&opts.project) {
        Ok(p) => p,
        Err(diags) => return cannot_open(&diags, opts.json),
    };
    let mut diags = project.validate(&schemas);
    let lock = match Lock::read(&opts.project, &schemas) {
        Ok(l) => Some(l),
        Err(d) => {
            diags.extend(d);
            None
        }
    };
    let assets = project.assets_dir(&opts.project);
    let db = if assets.is_dir() {
        let (db, scan) = AssetDatabase::scan(&assets, &schemas);
        diags.extend(scan);
        diags.extend(project.validate_against(&db, &schemas));
        Some(db)
    } else {
        None
    };
    if opts.json {
        let mut v = describe_json(&project, db.as_ref(), lock.as_ref());
        v["ok"] = (!has_errors(&diags)).into();
        v["diagnostics"] = diagnostics_json(&diags)["diagnostics"].clone();
        println!("{v}");
    } else {
        print!("{}", describe_text(&project, db.as_ref(), lock.as_ref()));
        for d in &diags {
            eprintln!("{d}");
        }
    }
    exit(&diags)
}

fn lock(root: Option<&Path>, opts: &Opts) -> ExitCode {
    let schemas = match load_schemas(root, opts) {
        Ok(s) => s,
        Err(d) => return cannot_open(&[d], opts.json),
    };
    if let Err(diags) = Project::read(&opts.project) {
        return cannot_open(&diags, opts.json);
    }
    let expected = fynkel_import::expected_lock();
    let path = opts.project.join(LOCK_FILE);
    let shown = path.display().to_string();
    let current = Lock::read(&opts.project, &schemas);
    let mut diags = Vec::new();
    let mut written = false;
    if opts.check {
        match current {
            Ok(l) => diags.extend(l.check(&expected, Some(&shown))),
            Err(d) => diags.extend(d),
        }
    } else {
        let changed = match &current {
            Ok(l) => *l != expected,
            Err(_) => true,
        };
        if changed {
            match std::fs::write(&path, expected.to_text()) {
                Ok(()) => written = true,
                Err(e) => diags.push(
                    Diagnostic::error(Code::IO, format!("cannot write: {e}"))
                        .with_document(shown.clone()),
                ),
            }
        }
    }
    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["lock"] = serde_json::to_value(&expected).unwrap();
        v["path"] = shown.into();
        v["written"] = written.into();
        v["check"] = opts.check.into();
        println!("{v}");
        return exit(&diags);
    }
    for d in &diags {
        eprintln!("{d}");
    }
    if opts.check {
        eprintln!(
            "fynkel project lock --check: {}",
            if has_errors(&diags) {
                "the lock does not match this build"
            } else {
                "the lock matches this build"
            }
        );
    } else if written {
        eprintln!("fynkel project lock: wrote {shown}");
    } else if !has_errors(&diags) {
        eprintln!("fynkel project lock: {shown} already matches this build");
    }
    exit(&diags)
}

/// `fynkel validate`.
pub fn run_validate(root: Option<&Path>, args: &[String]) -> ExitCode {
    let Some(opts) = parse(args, "validate") else {
        return usage_validate();
    };
    if !opts.positional.is_empty() {
        return usage_validate();
    }
    let schemas = match load_schemas(root, &opts) {
        Ok(s) => s,
        Err(d) => return cannot_open(&[d], opts.json),
    };
    if !opts.project.join(PROJECT_FILE).is_file() {
        return cannot_open(
            &[Diagnostic::error(
                Code::PROJECT_MISSING,
                format!("no {PROJECT_FILE} in {}", opts.project.display()),
            )
            .with_document(opts.project.join(PROJECT_FILE).display().to_string())
            .with_suggestion("run from the project directory, or pass --project <dir>")],
            opts.json,
        );
    }
    let expected = fynkel_import::expected_lock();
    let sources = fynkel_import::ImportedPrefabs::default();
    let report = validate_project_with(&opts.project, &schemas, Some(&expected), &sources);
    if opts.json {
        println!("{}", report.json());
        return exit(&report.diagnostics);
    }
    for d in &report.diagnostics {
        eprintln!("{d}");
    }
    let errors = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == fynkel_document::Severity::Error)
        .count();
    let warnings = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == fynkel_document::Severity::Warning)
        .count();
    eprintln!(
        "fynkel validate: {} — {} document(s), {} source(s), {} reference(s); {errors} error(s), {warnings} warning(s)",
        report
            .project
            .as_ref()
            .map(|p| p.name().to_string())
            .unwrap_or_else(|| opts.project.display().to_string()),
        report.documents,
        report.sources,
        report.references
    );
    exit(&report.diagnostics)
}

/// `fynkel schema list|describe`.
pub fn run_schema(args: &[String]) -> ExitCode {
    let Some((sub, rest)) = args.split_first() else {
        return usage_schema();
    };
    let Some(opts) = parse(rest, "schema") else {
        return usage_schema();
    };
    let schemas = SchemaSet::embedded();
    match sub.as_str() {
        "list" if opts.positional.is_empty() => {
            let records: Vec<serde_json::Value> = fynkel_core::schema::ALL
                .iter()
                .map(|s| record_json(s, &schemas))
                .collect();
            if opts.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": true,
                        "fingerprint": format!("{:#018x}", fynkel_core::schema::schema_fingerprint()),
                        "abiVersion": fynkel_contracts::ABI_VERSION,
                        "documents": DocumentKind::ALL.iter().map(|k| serde_json::json!({
                            "kind": k.name(),
                            "suffix": k.suffix(),
                            "schema": k.schema_id(),
                            "formatVersion": k.supported_format_version(),
                        })).collect::<Vec<_>>(),
                        "records": records,
                    })
                );
            } else {
                for k in DocumentKind::ALL {
                    println!(
                        "{:<10} {:>4}  {:<32} v{}  {}",
                        "document",
                        "",
                        k.name(),
                        k.supported_format_version(),
                        k.suffix()
                    );
                }
                for s in fynkel_core::schema::ALL {
                    println!(
                        "{:<10} {:>4}  {:<32} {:>3} B  {}",
                        s.kind.name(),
                        s.id,
                        s.name,
                        s.wire_size(),
                        s.canonical()
                    );
                }
                println!(
                    "fingerprint {:#018x}",
                    fynkel_core::schema::schema_fingerprint()
                );
            }
            ExitCode::SUCCESS
        }
        "describe" if opts.positional.len() == 1 => {
            let name = &opts.positional[0];
            if let Some(kind) = DocumentKind::parse_name(name) {
                let v = serde_json::json!({
                    "ok": true,
                    "kind": "document",
                    "name": kind.name(),
                    "suffix": kind.suffix(),
                    "schema": kind.schema_id(),
                    "formatVersion": kind.supported_format_version(),
                    "envelope": ["$schema", "formatVersion", "id", "name"],
                });
                if opts.json {
                    println!("{v}");
                } else {
                    println!(
                        "document {}  files *{}  formatVersion {}\n  schema {}\n  envelope: $schema, formatVersion, id, name",
                        kind.name(),
                        kind.suffix(),
                        kind.supported_format_version(),
                        kind.schema_id()
                    );
                }
                return ExitCode::SUCCESS;
            }
            let Some(record) = fynkel_core::schema::ALL.iter().find(|s| s.name == name) else {
                let d = Diagnostic::error(
                    Code::SCENE_UNKNOWN_COMPONENT_TYPE,
                    format!("no schema named {name:?}"),
                )
                .with_suggestion("`fynkel schema list` shows every name");
                if opts.json {
                    println!("{}", diagnostics_json(&[d]));
                } else {
                    eprintln!("{d}");
                }
                return ExitCode::from(1);
            };
            let v = record_json(record, &schemas);
            if opts.json {
                let mut v = v;
                v["ok"] = true.into();
                println!("{v}");
            } else {
                println!(
                    "{} {} (id {}, {} B){}\n  {}",
                    record.kind.name(),
                    record.name,
                    record.id,
                    record.wire_size(),
                    if record.authored {
                        ""
                    } else {
                        ", engine-managed"
                    },
                    record.doc
                );
                for (f, offset) in record.fields.iter().zip(record.offsets()) {
                    println!(
                        "  {:<20} {:<10} @{:<3} {}",
                        f.name,
                        f.ty.canonical(),
                        offset,
                        f.doc
                    );
                }
            }
            ExitCode::SUCCESS
        }
        _ => usage_schema(),
    }
}

fn record_json(s: &fynkel_contracts::RecordSchema, schemas: &SchemaSet) -> serde_json::Value {
    serde_json::json!({
        "kind": s.kind.name(),
        "id": s.id,
        "name": s.name,
        "rust": s.rust_path,
        "doc": s.doc,
        "wireSize": s.wire_size(),
        "authored": s.authored,
        "schema": schemas.component(s.name).map(|_| format!("https://fynkel.dev/schemas/components/{}.schema.json", s.name)),
        "fields": s.fields.iter().zip(s.offsets()).map(|(f, offset)| serde_json::json!({
            "name": f.name,
            "type": f.ty.canonical(),
            "offset": offset,
            "size": f.ty.wire_size(),
            "doc": f.doc,
        })).collect::<Vec<_>>(),
    })
}

/// Exit 2: the project (or the schemas) could not be opened at all.
fn cannot_open(diags: &[Diagnostic], json: bool) -> ExitCode {
    if json {
        println!("{}", diagnostics_json(diags));
    } else {
        for d in diags {
            eprintln!("{d}");
        }
    }
    ExitCode::from(2)
}

fn exit(diags: &[Diagnostic]) -> ExitCode {
    if has_errors(diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
