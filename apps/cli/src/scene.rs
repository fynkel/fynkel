//! `fynkel scene …` and `fynkel prefab …` (§14.1): `inspect`, `validate`
//! and `apply` for scene and prefab documents. Scenes with prefab instances
//! are checked (and, with `--expand`, shown) against the prefabs found under
//! `--prefabs <dir>` (default: the scene's own directory). `apply` runs one
//! authoring transaction (§11.1) from a patch file and writes the document
//! back unless `--dry-run`. Same exit codes and `--json` shape as
//! `fynkel document`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::prefab;
use fynkel_document::scene::{inspect_json, inspect_text};
use fynkel_document::{
    authoring, diagnostics_json, expand, has_errors, Diagnostic, Document, DocumentKind, Editor,
    Prefab, PrefabLibrary, Request, Scene, SchemaSet,
};

pub fn usage(cmd: &str) -> ExitCode {
    if cmd == "prefab" {
        eprintln!(
            "usage:\n  fynkel prefab inspect <path> [--json] [--schemas <dir>]      roots, entity tree and component summary\n  fynkel prefab validate <path>... [--json] [--schemas <dir>]  schema + semantic validation\n  fynkel prefab apply <path> --patch <ops.json> [--dry-run] [--json] [--schemas <dir>]\n                                               apply one authoring transaction (see `fynkel scene apply`)"
        );
    } else {
        eprintln!(
            "usage:\n  fynkel scene inspect <path> [--json] [--schemas <dir>] [--prefabs <dir>] [--expand]\n                                               entity tree and component summary (expanded: instances replaced by their entities)\n  fynkel scene validate <path>... [--json] [--schemas <dir>] [--prefabs <dir>]\n                                               schema + semantic validation, prefab instances resolved and their overrides checked\n  fynkel scene apply <path> --patch <ops.json> [--dry-run] [--json] [--schemas <dir>]\n                                               apply one authoring transaction: the patch is an array of commands or\n                                               {{ baseRevision?, transactionId?, commands: [...] }}; a stale baseRevision is a\n                                               conflict; a transaction that would leave errors is rejected whole; --dry-run\n                                               prints the diff instead of writing"
        );
    }
    ExitCode::from(2)
}

struct Opts {
    paths: Vec<PathBuf>,
    json: bool,
    schemas_dir: Option<PathBuf>,
    prefabs_dir: Option<PathBuf>,
    expand: bool,
    patch: Option<PathBuf>,
    dry_run: bool,
}

pub fn run(root: Option<&Path>, cmd: &str, args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        return usage(cmd);
    };
    let mut o = Opts {
        paths: Vec::new(),
        json: false,
        schemas_dir: None,
        prefabs_dir: None,
        expand: false,
        patch: None,
        dry_run: false,
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--expand" => o.expand = true,
            "--dry-run" => o.dry_run = true,
            "--patch" => {
                i += 1;
                o.patch = args.get(i).map(PathBuf::from);
            }
            "--schemas" => {
                i += 1;
                o.schemas_dir = args.get(i).map(PathBuf::from);
            }
            "--prefabs" => {
                i += 1;
                o.prefabs_dir = args.get(i).map(PathBuf::from);
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel {cmd}: unknown argument {other}");
                return usage(cmd);
            }
            p => o.paths.push(PathBuf::from(p)),
        }
        i += 1;
    }
    if o.paths.is_empty() {
        return usage(cmd);
    }
    let schemas = match crate::schemas::load_for(root, o.schemas_dir.as_deref(), &o.paths[0]) {
        Ok(s) => s,
        Err(d) => return report(&[d], o.json),
    };
    match (cmd, sub.as_str()) {
        ("scene", "inspect") => scene_inspect(&o, &schemas),
        ("scene", "validate") => scene_validate(&o, &schemas),
        ("prefab", "inspect") => prefab_inspect(&o, &schemas),
        ("prefab", "validate") => prefab_validate(&o, &schemas),
        ("scene", "apply") => apply(&o, DocumentKind::Scene, &schemas),
        ("prefab", "apply") => apply(&o, DocumentKind::Prefab, &schemas),
        _ => usage(cmd),
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
    ExitCode::from(1)
}

fn exit(diags: &[Diagnostic]) -> ExitCode {
    if has_errors(diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Prefabs for a scene: `--prefabs`, else the scene's directory. Scanned
/// only when the scene has instances.
fn library_for(
    scene: &Scene,
    opts: &Opts,
    path: &Path,
    diags: &mut Vec<Diagnostic>,
) -> PrefabLibrary {
    if scene.instances().next().is_none() {
        return PrefabLibrary::empty();
    }
    let dir = opts
        .prefabs_dir
        .clone()
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let (lib, scan) = PrefabLibrary::scan(&dir);
    diags.extend(scan);
    lib
}

/// Validation plus prefab resolution/expansion diagnostics; the expanded
/// scene when requested.
fn check_scene(
    scene: &Scene,
    opts: &Opts,
    path: &Path,
    schemas: &SchemaSet,
) -> (Vec<Diagnostic>, Option<Scene>) {
    let mut diags = scene.validate(schemas);
    if scene.instances().next().is_none() {
        return (diags, None);
    }
    let lib = library_for(scene, opts, path, &mut diags);
    let expanded = expand(scene, &lib, schemas);
    diags.extend(expanded.diagnostics.iter().cloned());
    (diags, Some(expanded.scene()))
}

fn scene_inspect(opts: &Opts, schemas: &SchemaSet) -> ExitCode {
    let path = &opts.paths[0];
    let scene = match Document::read(path, Some(DocumentKind::Scene)) {
        Ok(doc) => Scene::from_document(doc),
        Err(diags) => return report(&diags, opts.json),
    };
    let (diags, expanded) = check_scene(&scene, opts, path, schemas);
    let shown = match (&expanded, opts.expand) {
        (Some(e), true) => e,
        _ => &scene,
    };
    if opts.json {
        let mut v = inspect_json(shown);
        v["expanded"] = serde_json::Value::Bool(opts.expand && expanded.is_some());
        v["ok"] = serde_json::Value::Bool(!has_errors(&diags));
        v["diagnostics"] = diagnostics_json(&diags)["diagnostics"].clone();
        println!("{v}");
    } else {
        print!("{}", inspect_text(shown));
        for d in &diags {
            eprintln!("{d}");
        }
    }
    exit(&diags)
}

fn scene_validate(opts: &Opts, schemas: &SchemaSet) -> ExitCode {
    let mut diags = Vec::new();
    for path in &opts.paths {
        match Document::read(path, Some(DocumentKind::Scene)) {
            Ok(doc) => {
                let scene = Scene::from_document(doc);
                diags.extend(check_scene(&scene, opts, path, schemas).0);
            }
            Err(d) => diags.extend(d),
        }
    }
    if opts.json {
        println!("{}", diagnostics_json(&diags));
    } else {
        for d in &diags {
            eprintln!("{d}");
        }
        if !has_errors(&diags) {
            eprintln!("fynkel scene validate: {} scene(s) ok", opts.paths.len());
        }
    }
    exit(&diags)
}

fn prefab_inspect(opts: &Opts, schemas: &SchemaSet) -> ExitCode {
    let prefab = match Document::read(&opts.paths[0], Some(DocumentKind::Prefab)) {
        Ok(doc) => Prefab::from_document(doc),
        Err(diags) => return report(&diags, opts.json),
    };
    let diags = prefab.validate(schemas);
    if opts.json {
        let mut v = prefab::inspect_json(&prefab);
        v["ok"] = serde_json::Value::Bool(!has_errors(&diags));
        v["diagnostics"] = diagnostics_json(&diags)["diagnostics"].clone();
        println!("{v}");
    } else {
        print!("{}", prefab::inspect_text(&prefab));
        for d in &diags {
            eprintln!("{d}");
        }
    }
    exit(&diags)
}

fn prefab_validate(opts: &Opts, schemas: &SchemaSet) -> ExitCode {
    let mut diags = Vec::new();
    for path in &opts.paths {
        match Document::read(path, Some(DocumentKind::Prefab)) {
            Ok(doc) => diags.extend(Prefab::from_document(doc).validate(schemas)),
            Err(d) => diags.extend(d),
        }
    }
    if opts.json {
        println!("{}", diagnostics_json(&diags));
    } else {
        for d in &diags {
            eprintln!("{d}");
        }
        if !has_errors(&diags) {
            eprintln!("fynkel prefab validate: {} prefab(s) ok", opts.paths.len());
        }
    }
    exit(&diags)
}

/// `fynkel scene|prefab apply <path> --patch <ops.json> [--dry-run] [--json]`.
/// Exit 0 when the transaction committed (and, unless `--dry-run`, the file
/// was written), 1 when it was refused, 2 when the document, the patch or
/// the arguments cannot be read.
fn apply(opts: &Opts, kind: DocumentKind, schemas: &SchemaSet) -> ExitCode {
    let cmd = kind.name();
    let (Some(path), Some(patch_path)) = (opts.paths.first(), opts.patch.as_ref()) else {
        eprintln!("fynkel {cmd} apply: <path> and --patch <ops.json> are required");
        return usage(cmd);
    };
    if opts.paths.len() != 1 {
        eprintln!("fynkel {cmd} apply: exactly one document");
        return usage(cmd);
    }
    let patch: serde_json::Value = match std::fs::read_to_string(patch_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(v) => v,
        Err(why) => {
            let d = Diagnostic::error(
                fynkel_document::Code::EDIT_INVALID_COMMAND,
                format!("cannot read patch: {why}"),
            )
            .with_document(patch_path.display().to_string());
            report(&[d], opts.json);
            return ExitCode::from(2);
        }
    };
    let request = match Request::from_json(&patch) {
        Ok(r) => r,
        Err(diags) => {
            let diags: Vec<Diagnostic> = diags
                .into_iter()
                .map(|d| d.with_document(patch_path.display().to_string()))
                .collect();
            report(&diags, opts.json);
            return ExitCode::from(2);
        }
    };
    let document = match Document::read(path, Some(kind)) {
        Ok(doc) => doc,
        Err(diags) => {
            report(&diags, opts.json);
            return ExitCode::from(2);
        }
    };
    let before = match document.write() {
        Ok(t) => t,
        Err(d) => return report(&[d], opts.json),
    };
    let mut editor = match Editor::open(document, schemas) {
        Ok(e) => e,
        Err(diags) => return report(&diags, opts.json),
    };
    let applied = editor.apply(request, schemas);
    let mut diags = applied.all_diagnostics();
    let mut diff = None;
    let mut written = false;
    if applied.ok {
        let after = match editor.document().write() {
            Ok(t) => t,
            Err(d) => return report(&[d], opts.json),
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        diff = Some(authoring::diff(&before, &after, &name));
        if !opts.dry_run {
            match editor.save(None) {
                Ok(_) => written = true,
                Err(d) => diags.push(d),
            }
        }
    }
    let ok = applied.ok && !has_errors(&diags);
    if opts.json {
        let mut v = applied.json();
        v["ok"] = serde_json::Value::Bool(ok);
        v["dryRun"] = serde_json::Value::Bool(opts.dry_run);
        v["written"] = serde_json::Value::Bool(written);
        v["diagnostics"] = diagnostics_json(&diags)["diagnostics"].clone();
        if let Some(d) = diff {
            v["diff"] = serde_json::Value::String(d);
        }
        println!("{v}");
    } else {
        for d in &diags {
            eprintln!("{d}");
        }
        if let Some(d) = &diff {
            if opts.dry_run || !d.is_empty() {
                print!("{d}");
            }
        }
        if applied.ok {
            eprintln!(
                "fynkel {cmd} apply: transaction {} — {} command(s), revision {} → {}{}",
                applied.transaction_id,
                applied.mutations.len(),
                &applied.base_revision.as_str()[..12],
                &applied.revision.as_str()[..12],
                if opts.dry_run {
                    " (dry run, nothing written)"
                } else if written {
                    ", written"
                } else {
                    ", NOT written"
                }
            );
        } else {
            eprintln!("fynkel {cmd} apply: transaction refused, nothing applied");
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
