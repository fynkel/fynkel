//! `fynkel cook` and `fynkel cooked inspect` (§14.1, blueprint §19.2c
//! item 12, §19.2d item 3). `cook` cooks a project — every asset in its
//! database, with the project's behaviour schemas, and the project's name
//! and startup scene into the index — into the content-addressed cache
//! (`.fynkel/cache`, version control ignores it) for one profile;
//! unchanged assets hit their keys and are skipped. `--assets <dir>`
//! cooks a bare asset root instead (no project record in the index).
//! `cooked inspect` prints a cooked container's META and chunk table.
//! Exit 0 when every asset cooked or hit (warnings allowed), 1 on any
//! error, 2 on usage.
//!
//! [`prepare`] is what `fynkel test --headless`, `fynkel state dump`,
//! `fynkel render` and `fynkel run` do before they read a cache: the
//! runtime reads the cache only, so the tools cook first.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_contracts::Uuid;
use fynkel_cooked::ChunkFile;
use fynkel_document::asset::AssetDatabase;
use fynkel_document::project::PROJECT_FILE;
use fynkel_document::{diagnostics_json, has_errors, Code, Diagnostic, Project};
use fynkel_import::{cook, cook_project, project_cache_dir, Cache, CookOptions, Status};

/// Most a META chunk may be when inspecting.
const MAX_META_BYTES: u64 = 16 << 20;

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel cook [--project <dir>] [--cache <dir>] [--profile <name>] [--force] [--prune] [--json]\n                                               cook the project's assets into its content-addressed cache (default .fynkel/cache, profile dev)\n  fynkel cook --assets <dir> [--cache <dir>] [--schemas <dir>] ...\n                                               cook a bare asset root (no project)\n  fynkel cooked inspect <file.fyna>... [--json]  print a cooked container's META and chunk table"
    );
    ExitCode::from(2)
}

struct Opts {
    project: Option<PathBuf>,
    assets: Option<PathBuf>,
    cache: Option<PathBuf>,
    profile: String,
    force: bool,
    prune: bool,
    json: bool,
    schemas_dir: Option<PathBuf>,
    files: Vec<PathBuf>,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        project: None,
        assets: None,
        cache: None,
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
            "--project" => {
                i += 1;
                o.project = Some(PathBuf::from(args.get(i)?));
            }
            "--assets" => {
                i += 1;
                o.assets = Some(PathBuf::from(args.get(i)?));
            }
            "--cache" => {
                i += 1;
                o.cache = Some(PathBuf::from(args.get(i)?));
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

pub fn run(root: Option<&Path>, args: &[String]) -> ExitCode {
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
    let cook_opts = CookOptions {
        profile: opts.profile.clone(),
        force: opts.force,
        ..CookOptions::default()
    };
    let (report, cache, mut diags) = match &opts.assets {
        // A bare asset root: the old M2 shape, no project record.
        Some(assets) => {
            if opts.project.is_some() {
                eprintln!("fynkel cook: --assets and --project exclude each other");
                return usage();
            }
            let schemas = match crate::schemas::load(root, opts.schemas_dir.as_deref()) {
                Ok(s) => s,
                Err(d) => return finish(&[d], opts.json),
            };
            if !assets.is_dir() {
                return finish(
                    &[Diagnostic::error(
                        Code::IO,
                        format!("{} is not a directory", assets.display()),
                    )
                    .with_suggestion("run from the project root, or pass --assets <dir>")],
                    opts.json,
                );
            }
            let (db, diags) = AssetDatabase::scan(assets, &schemas);
            if has_errors(&diags) {
                let mut diags = diags;
                diags.push(
                    Diagnostic::error(
                        Code::IO,
                        "the asset database has errors; nothing cooked".to_string(),
                    )
                    .with_suggestion("`fynkel asset list` shows them"),
                );
                return finish(&diags, opts.json);
            }
            let cache = Cache::at(opts.cache.as_deref().unwrap_or(Path::new(".fynkel/cache")));
            let report = cook(&db, &schemas, &cache, &cook_opts);
            (report, cache, diags)
        }
        None => {
            let project = opts.project.clone().unwrap_or_else(|| PathBuf::from("."));
            if opts.schemas_dir.is_some() {
                eprintln!("fynkel cook: --schemas applies to --assets; a project brings its own");
                return usage();
            }
            let cache_dir = opts
                .cache
                .clone()
                .unwrap_or_else(|| project_cache_dir(&project));
            match cook_project(&project, Some(&cache_dir), &cook_opts) {
                Ok(report) => {
                    let mut diags = Vec::new();
                    if !report.has_errors() {
                        stage_for_app(&project, &cache_dir, &mut diags);
                    }
                    (report, Cache::at(&cache_dir), diags)
                }
                Err(diags) => {
                    let mut diags = diags;
                    diags.push(
                        Diagnostic::error(
                            Code::IO,
                            "the project has errors; nothing cooked".to_string(),
                        )
                        .with_suggestion("`fynkel validate` shows them"),
                    );
                    return finish(&diags, opts.json);
                }
            }
        }
    };
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

/// Stages the cache into the project's app bundle directory when the
/// project is a Flutter app (`crate::bundle`); a failure is a diagnostic,
/// the cook itself stands.
pub fn stage_for_app(project: &Path, cache: &Path, diags: &mut Vec<Diagnostic>) {
    if !crate::bundle::is_app(project) {
        return;
    }
    match crate::bundle::stage(project, cache) {
        Ok(s) => eprintln!(
            "fynkel cook: staged {} object(s) into {} for the app bundle ({} copied, {} removed)",
            s.objects,
            s.dir.display(),
            s.copied,
            s.removed
        ),
        Err(e) => diags.push(
            Diagnostic::error(
                Code::IO,
                format!(
                    "cannot stage the cache into {}: {e}",
                    project.join(crate::bundle::STAGE_DIR).display()
                ),
            )
            .with_suggestion("the app bundles it from there (pubspec.yaml `assets:`)"),
        ),
    }
}

/// A cache ready to load from, for the tools that run the runtime.
pub struct Prepared {
    pub cache: PathBuf,
    /// The scene to load; `None` is the project's startup scene.
    pub scene: Option<Uuid>,
}

/// What a runtime tool takes: a project (`--project`, default the working
/// directory), or a scene — a `.scene.json` inside a project or an asset
/// id — and where its cache is.
#[derive(Default)]
pub struct Target {
    pub project: Option<PathBuf>,
    pub scene: Option<String>,
    pub cache: Option<PathBuf>,
    /// `--no-cook`: read the cache as it is.
    pub no_cook: bool,
}

/// Resolves the target's project and scene and cooks the project into
/// its cache (unless `no_cook`), so the runtime finds every object. A
/// `--cache` with no project around is read as it is. `Err` is the exit
/// code, the reason already printed: 2 when there is no project (a loose
/// `.scene.json` outside any project cannot be cooked, so the runtime
/// cannot load it), 1 when the cook failed.
pub fn prepare(tool: &str, target: &Target) -> Result<Prepared, ExitCode> {
    let mut scene: Option<Uuid> = None;
    let mut project = target.project.clone();
    if let Some(text) = &target.scene {
        if let Ok(id) = text.parse::<Uuid>() {
            scene = Some(id);
        } else {
            let path = Path::new(text);
            if !path.is_file() {
                eprintln!("{tool}: {text} is neither a scene file nor an asset id");
                return Err(ExitCode::from(2));
            }
            let Some(dir) = Project::dir_of(path) else {
                eprintln!(
                    "{tool}: {text} is outside any project (no {PROJECT_FILE} above it); the runtime loads scenes from a project's cooked cache only"
                );
                eprintln!(
                    "  put the scene under a project's asset root (`fynkel create` makes one)"
                );
                return Err(ExitCode::from(2));
            };
            match project {
                Some(p) if !same_dir(&p, &dir) => {
                    eprintln!(
                        "{tool}: {text} belongs to {}, not --project {}",
                        dir.display(),
                        p.display()
                    );
                    return Err(ExitCode::from(2));
                }
                _ => project = Some(dir),
            }
            scene = Some(match scene_id(path) {
                Ok(id) => id,
                Err(d) => {
                    eprintln!("{d}");
                    return Err(ExitCode::from(1));
                }
            });
        }
    }
    let project = match project {
        Some(p) => p,
        None if Path::new(PROJECT_FILE).is_file() => PathBuf::from("."),
        None => match &target.cache {
            Some(cache) => {
                return Ok(Prepared {
                    cache: cache.clone(),
                    scene,
                })
            }
            None => {
                eprintln!(
                    "{tool}: no {PROJECT_FILE} here; pass --project <dir>, --scene <path> or --cache <dir>"
                );
                return Err(ExitCode::from(2));
            }
        },
    };
    let cache = target
        .cache
        .clone()
        .unwrap_or_else(|| project_cache_dir(&project));
    if !target.no_cook {
        match cook_project(&project, Some(&cache), &CookOptions::default()) {
            Ok(report) => {
                for d in &report.diagnostics {
                    eprintln!("{d}");
                }
                if report.has_errors() {
                    eprintln!("{tool}: cooking failed; fix the assets first (`fynkel cook`)");
                    return Err(ExitCode::from(1));
                }
                let mut diags = Vec::new();
                stage_for_app(&project, &cache, &mut diags);
                for d in &diags {
                    eprintln!("{d}");
                }
            }
            Err(diags) => {
                for d in &diags {
                    eprintln!("{d}");
                }
                eprintln!("{tool}: the project has errors; fix them first (`fynkel validate`)");
                return Err(ExitCode::from(1));
            }
        }
    }
    Ok(Prepared { cache, scene })
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The `id` of a scene document (the asset id its cooked object has).
#[allow(clippy::result_large_err)]
fn scene_id(path: &Path) -> Result<Uuid, Diagnostic> {
    let shown = path.display().to_string();
    let text = std::fs::read_to_string(path)
        .map_err(|e| Diagnostic::error(Code::IO, e.to_string()).with_document(shown.clone()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        Diagnostic::error(Code::JSON_SYNTAX, e.to_string()).with_document(shown.clone())
    })?;
    v.get("id")
        .and_then(|id| id.as_str())
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| {
            Diagnostic::error(
                Code::ENVELOPE_MISSING_FIELD,
                "the scene has no `id`".to_string(),
            )
            .with_document(shown)
        })
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
