//! `fynkel asset …` (§14.1, blueprint §19.2c item 8): the asset database
//! as a command. `list` scans `assets/` (or `--assets <dir>`) and prints
//! every asset with its id; `resolve` answers "which file is this id" for
//! `<uuid>` or `<uuid>#<sub>`; `init` writes a fresh `.meta.json` sidecar
//! next to a source file; `remap` and `extract-materials` edit a model's
//! material remaps (`docs/design/material-remapping.md`). Same exit codes
//! and `--json` shape as `fynkel document`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_contracts::AssetRef;
use fynkel_document::asset::{
    entry_json, resolved_json, sidecar_path, AssetEntry, AssetKind, Meta,
};
use fynkel_document::{
    diagnostics_json, has_errors, parse_uuid, AssetDatabase, Code, Diagnostic, Document,
    DocumentKind,
};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel asset list [--assets <dir>] [--json] [--schemas <dir>]\n                                               every asset under assets/ with its id, kind and sub-assets\n  fynkel asset resolve <uuid>[#<sub>]... [--assets <dir>] [--json] [--schemas <dir>]\n                                               the file (and sub-asset) an id stands for\n  fynkel asset init <file>... [--json]         write a fresh .meta.json sidecar next to each source file\n  fynkel asset remap <model> <key> <material|none> [--assets <dir>] [--json]\n                                               draw the model's embedded material <key> (material/<name>) with a project material, or the embedded one again\n  fynkel asset extract-materials <model> [--to <dir>] [--assets <dir>] [--json]\n                                               write the model's embedded materials as .material.json documents (Materials/ next to it) and remap them"
    );
    ExitCode::from(2)
}

struct Opts {
    args: Vec<String>,
    json: bool,
    schemas_dir: Option<PathBuf>,
    assets: PathBuf,
    to: Option<PathBuf>,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        args: Vec::new(),
        json: false,
        schemas_dir: None,
        assets: PathBuf::from("assets"),
        to: None,
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
            "--to" => {
                i += 1;
                o.to = Some(PathBuf::from(args.get(i)?));
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

pub fn run(root: Option<&Path>, args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        return usage();
    };
    let Some(opts) = parse(&args[1..]) else {
        return usage();
    };
    match sub.as_str() {
        "init" => return init(&opts),
        "list" | "resolve" | "remap" | "extract-materials" => {}
        _ => return usage(),
    }
    let arity_ok = match sub.as_str() {
        "resolve" => !opts.args.is_empty(),
        "remap" => opts.args.len() == 3,
        "extract-materials" => opts.args.len() == 1,
        _ => true,
    };
    if !arity_ok {
        return usage();
    }
    let schemas = match crate::schemas::load(root, opts.schemas_dir.as_deref()) {
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
        "remap" => remap(&opts, &db, diags),
        "extract-materials" => extract_materials(&opts, &db, diags),
        _ => resolve(&opts, &db, diags),
    }
}

/// The asset an argument names: its id, or its file (relative to the
/// working directory, or absolute).
#[allow(clippy::result_large_err)]
fn find<'a>(db: &'a AssetDatabase, arg: &str) -> Result<&'a AssetEntry, Diagnostic> {
    if let Ok(id) = parse_uuid(arg) {
        return db.get(id).ok_or_else(|| {
            Diagnostic::error(Code::ASSET_UNRESOLVED, format!("no asset with id {arg}"))
                .with_id(arg.to_string())
        });
    }
    let root = db.root().canonicalize().unwrap_or(db.root().to_path_buf());
    Path::new(arg)
        .canonicalize()
        .ok()
        .and_then(|p| {
            p.strip_prefix(&root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
        })
        .and_then(|rel| db.by_path(&rel))
        .ok_or_else(|| {
            Diagnostic::error(Code::ASSET_UNRESOLVED, format!("{arg}: not an asset"))
                .with_suggestion(format!(
                    "name an asset by id or by its file under {}",
                    db.root().display()
                ))
        })
}

/// The model an argument names and its sidecar.
fn model<'a>(
    db: &'a AssetDatabase,
    arg: &str,
) -> Result<(&'a AssetEntry, PathBuf, Meta), Vec<Diagnostic>> {
    let entry = find(db, arg).map_err(|d| vec![d])?;
    if !fynkel_document::rig::is_model(&entry.kind) {
        return Err(vec![Diagnostic::error(
            Code::ASSET_WRONG_KIND,
            format!("{} is not an imported model (glTF, FBX)", entry.path),
        )]);
    }
    let sidecar = entry.sidecar(db.root()).expect("a source has a sidecar");
    let doc = Document::read(&sidecar, Some(DocumentKind::Meta))?;
    Ok((entry, sidecar, Meta::from_document(doc)))
}

#[allow(clippy::result_large_err)]
fn write_sidecar(sidecar: &Path, meta: &Meta) -> Result<(), Diagnostic> {
    let text = meta.to_document().write()?;
    std::fs::write(sidecar, text).map_err(|e| {
        Diagnostic::error(Code::IO, format!("cannot write: {e}"))
            .with_document(sidecar.display().to_string())
    })
}

/// `remap <model> <key> <material|none>`.
fn remap(opts: &Opts, db: &AssetDatabase, mut diags: Vec<Diagnostic>) -> ExitCode {
    let (entry, sidecar, mut meta) = match model(db, &opts.args[0]) {
        Ok(m) => m,
        Err(d) => {
            diags.extend(d);
            return finish(&diags, opts.json);
        }
    };
    let key = &opts.args[1];
    let keys: Vec<&str> = entry
        .sub_assets
        .keys()
        .map(String::as_str)
        .filter(|k| k.starts_with("material/"))
        .collect();
    if !keys.contains(&key.as_str()) {
        diags.push(
            Diagnostic::error(
                Code::ASSET_UNKNOWN_SUB,
                format!("{} has no embedded material {key}", entry.path),
            )
            .with_suggestion(if keys.is_empty() {
                "the model has no materials".to_string()
            } else {
                format!("one of: {}", keys.join(", "))
            }),
        );
        return finish(&diags, opts.json);
    }
    let target = match opts.args[2].as_str() {
        "none" => None,
        arg => match find(db, arg) {
            Ok(t) if t.kind == AssetKind::Document(DocumentKind::Material) => Some(t),
            Ok(t) => {
                diags.push(Diagnostic::error(
                    Code::ASSET_WRONG_KIND,
                    format!("{} ({}) is not a material document", t.path, t.kind.name()),
                ));
                return finish(&diags, opts.json);
            }
            Err(d) => {
                diags.push(d);
                return finish(&diags, opts.json);
            }
        },
    };
    meta.set_material_remap(key, target.map(|t| t.id));
    if let Err(d) = write_sidecar(&sidecar, &meta) {
        diags.push(d);
        return finish(&diags, opts.json);
    }
    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["sidecar"] = sidecar.display().to_string().into();
        v["settings"] = meta.settings.to_serde();
        println!("{v}");
        return exit(&diags);
    }
    match target {
        Some(t) => println!("{}: {key} -> {}", entry.path, t.path),
        None => println!("{}: {key} -> the embedded material", entry.path),
    }
    finish(&diags, false)
}

/// `extract-materials <model> [--to <dir>]`.
fn extract_materials(opts: &Opts, db: &AssetDatabase, mut diags: Vec<Diagnostic>) -> ExitCode {
    let (entry, sidecar, mut meta) = match model(db, &opts.args[0]) {
        Ok(m) => m,
        Err(d) => {
            diags.extend(d);
            return finish(&diags, opts.json);
        }
    };
    let source = entry.file(db.root());
    let folder = opts
        .to
        .clone()
        .unwrap_or_else(|| fynkel_import::extract::default_folder(&source));
    let extracted = match fynkel_import::extract::extract_materials(
        &source,
        &meta,
        &folder,
        &fynkel_import::Limits::default(),
    ) {
        Ok(e) => e,
        Err(d) => {
            diags.extend(d);
            return finish(&diags, opts.json);
        }
    };
    if extracted.is_empty() {
        diags.push(
            Diagnostic::error(
                Code::ASSET_MALFORMED_ENTRY,
                format!(
                    "every embedded material of {} is remapped already",
                    entry.path
                ),
            )
            .with_suggestion("`fynkel asset remap <model> <key> none` to extract one again"),
        );
        return finish(&diags, opts.json);
    }
    if let Err(e) = std::fs::create_dir_all(&folder) {
        diags.push(
            Diagnostic::error(Code::IO, format!("cannot create the folder: {e}"))
                .with_document(folder.display().to_string()),
        );
        return finish(&diags, opts.json);
    }
    let mut written = Vec::new();
    for e in &extracted {
        if let Err(err) = std::fs::write(&e.path, &e.text) {
            diags.push(
                Diagnostic::error(Code::IO, format!("cannot write: {err}"))
                    .with_document(e.path.display().to_string()),
            );
            return finish(&diags, opts.json);
        }
        meta.set_material_remap(&e.key, Some(e.id));
        if !opts.json {
            println!("{}  {}  {}", e.id, e.key, e.path.display());
        }
        written.push(serde_json::json!({
            "key": e.key,
            "id": e.id.to_string(),
            "path": e.path.display().to_string(),
        }));
    }
    if let Err(d) = write_sidecar(&sidecar, &meta) {
        diags.push(d);
        return finish(&diags, opts.json);
    }
    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["sidecar"] = sidecar.display().to_string().into();
        v["materials"] = written.into();
        println!("{v}");
        return exit(&diags);
    }
    finish(&diags, false)
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
