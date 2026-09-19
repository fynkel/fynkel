//! `fynkel scene …` and `fynkel prefab …` (§14.1): `inspect` and `validate`
//! for scene and prefab documents. Scenes with prefab instances are checked
//! (and, with `--expand`, shown) against the prefabs found under
//! `--prefabs <dir>` (default: the scene's own directory). Same exit codes
//! and `--json` shape as `fynkel document`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::prefab;
use fynkel_document::scene::{inspect_json, inspect_text};
use fynkel_document::{
    diagnostics_json, expand, has_errors, Diagnostic, Document, DocumentKind, Prefab,
    PrefabLibrary, Scene, SchemaSet,
};

pub fn usage(cmd: &str) -> ExitCode {
    if cmd == "prefab" {
        eprintln!(
            "usage:\n  fynkel prefab inspect <path> [--json] [--schemas <dir>]      roots, entity tree and component summary\n  fynkel prefab validate <path>... [--json] [--schemas <dir>]  schema + semantic validation"
        );
    } else {
        eprintln!(
            "usage:\n  fynkel scene inspect <path> [--json] [--schemas <dir>] [--prefabs <dir>] [--expand]\n                                               entity tree and component summary (expanded: instances replaced by their entities)\n  fynkel scene validate <path>... [--json] [--schemas <dir>] [--prefabs <dir>]\n                                               schema + semantic validation, prefab instances resolved and their overrides checked"
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
}

pub fn run(root: &Path, cmd: &str, args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        return usage(cmd);
    };
    let mut o = Opts {
        paths: Vec::new(),
        json: false,
        schemas_dir: None,
        prefabs_dir: None,
        expand: false,
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--expand" => o.expand = true,
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
    let schemas = match SchemaSet::load(
        &o.schemas_dir
            .clone()
            .unwrap_or_else(|| root.join("schemas")),
    ) {
        Ok(s) => s,
        Err(d) => return report(&[d], o.json),
    };
    match (cmd, sub.as_str()) {
        ("scene", "inspect") => scene_inspect(&o, &schemas),
        ("scene", "validate") => scene_validate(&o, &schemas),
        ("prefab", "inspect") => prefab_inspect(&o, &schemas),
        ("prefab", "validate") => prefab_validate(&o, &schemas),
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
