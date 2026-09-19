//! `fynkel cook` and `fynkel cooked inspect` (§14.1, blueprint §19.2c
//! item 12). `cook` scans the asset database and cooks every asset into
//! the content-addressed cache (`.fynkel/cache`, version control ignores
//! it) for one profile; unchanged assets hit their keys and are skipped.
//! `cooked inspect` prints a cooked container's META and chunk table.
//! Exit 0 when every asset cooked or hit (warnings allowed), 1 on any
//! error, 2 on usage.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_cooked::ChunkFile;
use fynkel_document::asset::AssetDatabase;
use fynkel_document::{diagnostics_json, has_errors, Code, Diagnostic, SchemaSet};
use fynkel_import::{cook, Cache, CookOptions, Status};

/// Most a META chunk may be when inspecting.
const MAX_META_BYTES: u64 = 16 << 20;

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel cook [--assets <dir>] [--cache <dir>] [--profile <name>] [--force] [--prune] [--json] [--schemas <dir>]\n                                               cook every asset into the content-addressed cache (default .fynkel/cache, profile dev)\n  fynkel cooked inspect <file.fyna>... [--json]  print a cooked container's META and chunk table"
    );
    ExitCode::from(2)
}

struct Opts {
    assets: PathBuf,
    cache: PathBuf,
    profile: String,
    force: bool,
    prune: bool,
    json: bool,
    schemas_dir: Option<PathBuf>,
    files: Vec<PathBuf>,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        assets: PathBuf::from("assets"),
        cache: PathBuf::from(".fynkel/cache"),
        profile: fynkel_import::cook::DEFAULT_PROFILE.to_string(),
        force: false,
        prune: false,
        json: false,
        schemas_dir: None,
        files: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--force" => o.force = true,
            "--prune" => o.prune = true,
            "--assets" => {
                i += 1;
                o.assets = PathBuf::from(args.get(i)?);
            }
            "--cache" => {
                i += 1;
                o.cache = PathBuf::from(args.get(i)?);
            }
            "--profile" => {
                i += 1;
                o.profile = args.get(i)?.clone();
            }
            "--schemas" => {
                i += 1;
                o.schemas_dir = Some(PathBuf::from(args.get(i)?));
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel cook: unknown argument {other}");
                return None;
            }
            p => o.files.push(PathBuf::from(p)),
        }
        i += 1;
    }
    Some(o)
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let Some(opts) = parse(args) else {
        return usage();
    };
    if !opts.files.is_empty() {
        return usage();
    }
    if opts.profile.is_empty()
        || !opts
            .profile
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        eprintln!("fynkel cook: a profile name is ASCII letters, digits, '-' and '_'");
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
                Code::IO,
                format!("{} is not a directory", opts.assets.display()),
            )
            .with_suggestion("run from the project root, or pass --assets <dir>")],
            opts.json,
        );
    }
    let (db, mut diags) = AssetDatabase::scan(&opts.assets, &schemas);
    if has_errors(&diags) {
        diags.push(
            Diagnostic::error(
                Code::IO,
                "the asset database has errors; nothing cooked".to_string(),
            )
            .with_suggestion("`fynkel asset list` shows them"),
        );
        return finish(&diags, opts.json);
    }
    let cache = Cache::at(&opts.cache);
    let report = cook(
        &db,
        &schemas,
        &cache,
        &CookOptions {
            profile: opts.profile.clone(),
            force: opts.force,
            ..CookOptions::default()
        },
    );
    diags.extend(report.diagnostics.iter().cloned());
    let mut pruned = None;
    if opts.prune {
        // Only this profile's keys are known here; pruning drops every
        // other profile's objects too, and says so.
        let keep: Vec<_> = report.assets.iter().filter_map(|a| a.key).collect();
        match cache.prune(&keep) {
            Ok(n) => pruned = Some(n),
            Err(d) => diags.push(d),
        }
    }
    if opts.json {
        let mut v = report.json();
        v["diagnostics"] = diagnostics_json(&diags)["diagnostics"].clone();
        v["cache"] = cache.root().display().to_string().into();
        v["pruned"] = pruned.into();
        println!("{v}");
        return exit(&diags);
    }
    print!("{}", report.text());
    for d in &diags {
        eprintln!("{d}");
    }
    if let Some(n) = pruned {
        eprintln!(
            "fynkel cook: pruned {n} object(s) not produced by profile {} (other profiles' objects included)",
            opts.profile
        );
    }
    eprintln!(
        "fynkel cook: {} asset(s) → {} ({} cooked, {} cached, {} failed)",
        report.assets.len(),
        cache.root().display(),
        report.count(Status::Cooked),
        report.count(Status::Cached),
        report.count(Status::Failed)
    );
    exit(&diags)
}

/// `fynkel cooked inspect <file>...`.
pub fn run_cooked(args: &[String]) -> ExitCode {
    let Some((sub, rest)) = args.split_first() else {
        return usage();
    };
    if sub != "inspect" {
        return usage();
    }
    let Some(opts) = parse(rest) else {
        return usage();
    };
    if opts.files.is_empty() {
        return usage();
    }
    let mut diags = Vec::new();
    let mut reports = Vec::new();
    for path in &opts.files {
        match inspect(path) {
            Ok(v) => {
                if !opts.json {
                    print_inspect(&v);
                }
                reports.push(v);
            }
            Err(d) => {
                diags.push(d);
                reports
                    .push(serde_json::json!({ "file": path.display().to_string(), "ok": false }));
            }
        }
    }
    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["files"] = reports.into();
        println!("{v}");
        return exit(&diags);
    }
    for d in &diags {
        eprintln!("{d}");
    }
    exit(&diags)
}

// As elsewhere: a `Diagnostic` is the error type on purpose.
#[allow(clippy::result_large_err)]
fn inspect(path: &Path) -> Result<serde_json::Value, Diagnostic> {
    let shown = path.display().to_string();
    let bad = |msg: String| {
        Diagnostic::error(Code::IMPORT_INVALID_DATA, msg).with_document(shown.clone())
    };
    let mut file = ChunkFile::open_path(path).map_err(|e| bad(format!("{e}")))?;
    let meta_bytes = file
        .meta(MAX_META_BYTES)
        .map_err(|e| bad(format!("META: {e}")))?;
    let meta: serde_json::Value =
        serde_json::from_slice(&meta_bytes).map_err(|e| bad(format!("META is not JSON: {e}")))?;
    let verified = file.verify().map_err(|e| format!("{e}"));
    let table = file.table();
    Ok(serde_json::json!({
        "file": shown,
        "ok": verified.is_ok(),
        "version": table.version,
        "bytes": table.file_len,
        "verified": verified.is_ok(),
        "error": verified.err(),
        "chunks": table.chunks.iter().enumerate().map(|(i, c)| serde_json::json!({
            "index": i,
            "kind": c.kind_str(),
            "offset": c.offset,
            "length": c.length,
            "crc32": format!("{:08x}", c.crc32),
        })).collect::<Vec<_>>(),
        "meta": meta,
    }))
}

fn print_inspect(v: &serde_json::Value) {
    let meta = &v["meta"];
    println!(
        "{}: cooked container v{} ({} bytes){}",
        v["file"].as_str().unwrap_or(""),
        v["version"],
        v["bytes"],
        if v["verified"] == true {
            ", checksums ok".to_string()
        } else {
            format!(", DAMAGED: {}", v["error"].as_str().unwrap_or("?"))
        }
    );
    println!(
        "  asset {} {} ({}) profile {} key {}",
        meta["id"].as_str().unwrap_or("?"),
        meta["path"].as_str().unwrap_or("?"),
        meta["kind"].as_str().unwrap_or("?"),
        meta["profile"].as_str().unwrap_or("?"),
        meta["key"]
            .as_str()
            .map(|k| &k[..k.len().min(12)])
            .unwrap_or("?")
    );
    if let Some(deps) = meta["dependencies"].as_array() {
        if !deps.is_empty() {
            println!(
                "  depends on {}",
                deps.iter()
                    .filter_map(|d| d.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    let chunk_meta = meta["chunks"].as_array();
    for (i, c) in v["chunks"].as_array().into_iter().flatten().enumerate() {
        let m = chunk_meta.and_then(|list| list.get(i));
        let name = m
            .and_then(|m| m["sub"].as_str().or_else(|| m["name"].as_str()))
            .unwrap_or("");
        let detail = m
            .map(|m| {
                let mut parts = Vec::new();
                for key in [
                    "documentKind",
                    "vertices",
                    "indices",
                    "width",
                    "height",
                    "colorSpace",
                    "sampleRate",
                    "channels",
                    "frames",
                    "entities",
                ] {
                    if let Some(val) = m.get(key) {
                        parts.push(format!("{key}={val}"));
                    }
                }
                parts.join(" ")
            })
            .unwrap_or_default();
        println!(
            "  [{i}] {} {:>10} B  crc {}  {name} {detail}",
            c["kind"].as_str().unwrap_or("????"),
            c["length"].as_u64().unwrap_or(0),
            c["crc32"].as_str().unwrap_or(""),
        );
    }
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
