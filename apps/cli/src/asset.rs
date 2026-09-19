//! `fynkel asset …` (§14.1, blueprint §19.2c item 8): the asset database
//! as a command. `list` scans `assets/` (or `--assets <dir>`) and prints
//! every asset with its id; `resolve` answers "which file is this id" for
//! `<uuid>` or `<uuid>#<sub>`; `init` writes a fresh `.meta.json` sidecar
//! next to a source file. Same exit codes and `--json` shape as
//! `fynkel document`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_contracts::AssetRef;
use fynkel_document::asset::{entry_json, resolved_json, sidecar_path, Meta};
use fynkel_document::{
    diagnostics_json, has_errors, parse_uuid, AssetDatabase, Diagnostic, SchemaSet,
};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel asset list [--assets <dir>] [--json] [--schemas <dir>]\n                                               every asset under assets/ with its id, kind and sub-assets\n  fynkel asset resolve <uuid>[#<sub>]... [--assets <dir>] [--json] [--schemas <dir>]\n                                               the file (and sub-asset) an id stands for\n  fynkel asset init <file>... [--json]         write a fresh .meta.json sidecar next to each source file"
    );
    ExitCode::from(2)
}

struct Opts {
    args: Vec<String>,
    json: bool,
    schemas_dir: Option<PathBuf>,
    assets: PathBuf,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        args: Vec::new(),
        json: false,
        schemas_dir: None,
        assets: PathBuf::from("assets"),
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--schemas" => {
                i += 1;
                o.schemas_dir = Some(PathBuf::from(args.get(i)?));
            }
            "--assets" => {
                i += 1;
                o.assets = PathBuf::from(args.get(i)?);
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel asset: unknown argument {other}");
                return None;
            }
            p => o.args.push(p.to_string()),
        }
        i += 1;
    }
    Some(o)
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        return usage();
    };
    let Some(opts) = parse(&args[1..]) else {
        return usage();
    };
    match sub.as_str() {
        "init" => return init(&opts),
        "list" | "resolve" => {}
        _ => return usage(),
    }
    if sub == "resolve" && opts.args.is_empty() {
        return usage();
    }
    let schemas = match SchemaSet::load(
        &opts
            .schemas_dir
            .clone()
            .unwrap_or_else(|| root.join("schemas")),
    ) {
        Ok(s) => s,
        Err(d) => return finish(&[d], opts.json),
    };
    if !opts.assets.is_dir() {
        return finish(
            &[Diagnostic::error(
                fynkel_document::Code::IO,
                format!("{} is not a directory", opts.assets.display()),
            )
            .with_suggestion("run from the project root, or pass --assets <dir>")],
            opts.json,
        );
    }
    let (db, diags) = AssetDatabase::scan(&opts.assets, &schemas);
    match sub.as_str() {
        "list" => list(&opts, &db, diags),
        _ => resolve(&opts, &db, diags),
    }
}

fn list(opts: &Opts, db: &AssetDatabase, diags: Vec<Diagnostic>) -> ExitCode {
    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["root"] = db.root().display().to_string().into();
        v["assets"] = db
            .entries()
            .iter()
            .map(entry_json)
            .collect::<Vec<_>>()
            .into();
        println!("{v}");
        return exit(&diags);
    }
    println!("{} asset(s) under {}", db.len(), db.root().display());
    for e in db.entries() {
        println!("{}  {}  {}", e.id, e.kind.name(), e.path);
        for (sub, id) in &e.sub_assets {
            println!("  {id}  #{sub}");
        }
    }
    finish(&diags, false)
}

fn resolve(opts: &Opts, db: &AssetDatabase, mut diags: Vec<Diagnostic>) -> ExitCode {
    let mut results = Vec::new();
    for arg in &opts.args {
        let (id, sub) = match arg.split_once('#') {
            Some((id, sub)) => (id, sub),
            None => (arg.as_str(), ""),
        };
        let id = match parse_uuid(id) {
            Ok(id) => fynkel_contracts::Uuid::from_bytes(*id.as_bytes()),
            Err(e) => {
                diags.push(
                    Diagnostic::error(fynkel_document::Code::MALFORMED_ID, format!("{arg}: {e}"))
                        .with_id(arg.clone()),
                );
                continue;
            }
        };
        let r = AssetRef::with_sub(id, sub);
        match db.resolve(&r) {
            Ok(resolved) => {
                if !opts.json {
                    let file = resolved.entry.file(db.root());
                    match &resolved.sub {
                        Some((sub, uuid)) => {
                            println!("{r}  ->  {}  #{sub} ({uuid})", file.display())
                        }
                        None => println!(
                            "{r}  ->  {}  ({})",
                            file.display(),
                            resolved.entry.kind.name()
                        ),
                    }
                }
                let mut v = resolved_json(&resolved);
                v["query"] = r.to_string().into();
                v["file"] = resolved.entry.file(db.root()).display().to_string().into();
                results.push(v);
            }
            Err(d) => diags.push(d),
        }
    }
    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["resolved"] = results.into();
        println!("{v}");
        return exit(&diags);
    }
    finish(&diags, false)
}

fn init(opts: &Opts) -> ExitCode {
    if opts.args.is_empty() {
        return usage();
    }
    let mut diags = Vec::new();
    let mut written = Vec::new();
    for arg in &opts.args {
        let source = PathBuf::from(arg);
        let sidecar = sidecar_path(&source);
        if !source.is_file() {
            diags.push(
                Diagnostic::error(
                    fynkel_document::Code::IO,
                    "not a file (sidecars belong to existing source files)",
                )
                .with_document(arg.clone()),
            );
            continue;
        }
        if source
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".meta.json"))
            || fynkel_document::DocumentKind::from_path(&source).is_some()
        {
            diags.push(
                Diagnostic::error(
                    fynkel_document::Code::UNKNOWN_DOCUMENT_KIND,
                    "documents and sidecars carry their own id; only source files get a sidecar",
                )
                .with_document(arg.clone()),
            );
            continue;
        }
        if sidecar.exists() {
            diags.push(
                Diagnostic::warning(
                    fynkel_document::Code::ASSET_DUPLICATE_ID,
                    format!("{} already exists; kept", sidecar.display()),
                )
                .with_document(arg.clone())
                .with_suggestion(
                    "delete it first to assign a new id (references to the old id break)",
                ),
            );
            continue;
        }
        let meta = Meta::new(&source);
        let doc = meta.to_document();
        match doc.write().and_then(|text| {
            std::fs::write(&sidecar, text).map_err(|e| {
                Diagnostic::error(fynkel_document::Code::IO, format!("cannot write: {e}"))
                    .with_document(sidecar.display().to_string())
            })
        }) {
            Ok(()) => {
                if !opts.json {
                    println!("{}  {}  {}", meta.id(), meta.importer, sidecar.display());
                }
                written.push(serde_json::json!({
                    "id": meta.id().to_string(),
                    "importer": meta.importer,
                    "file": arg,
                    "sidecar": sidecar.display().to_string(),
                }));
            }
            Err(d) => diags.push(d),
        }
    }
    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["written"] = written.into();
        println!("{v}");
        return exit(&diags);
    }
    finish(&diags, false)
}

fn finish(diags: &[Diagnostic], json: bool) -> ExitCode {
    if json {
        println!("{}", diagnostics_json(diags));
    } else {
        for d in diags {
            eprintln!("{d}");
        }
    }
    exit(diags)
}

fn exit(diags: &[Diagnostic]) -> ExitCode {
    if has_errors(diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
