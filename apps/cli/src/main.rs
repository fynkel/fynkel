//! `fynkel` — the Fynkel command-line tool.
//!
//! M1 scope: `fynkel codegen` (ADR-0012, native schema generator),
//! `fynkel schema`, and the seed of `fynkel test` (§13.3/§14.1): the Rust
//! headless host. M2 adds `fynkel document` (validate/format/new), `fynkel
//! scene`, `fynkel prefab`, `fynkel material`, `fynkel shader`, `fynkel asset`,
//! `fynkel migrate`, `fynkel import`, `fynkel cook`, `fynkel cooked inspect`,
//! `--scene`/`--project` for the headless host, `fynkel state dump`,
//! `fynkel create` and `fynkel run`. M3 makes the cooked cache the only
//! thing the runtime reads: `test --headless`, `state dump`, `render` and
//! `run` cook the project first (`cook::prepare`). Outside the repository
//! the commands validate against the embedded schema set (`schemas.rs`).
//! Exit codes: 0 ok, 1 error, 2 usage or `--check` found drift.

mod asset;
mod behaviours;
mod codegen;
mod cook;
mod create;
mod document;
mod import;
mod material;
mod migrate;
mod project;
mod render;
mod run;
mod scene;
mod schemas;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::project::PROJECT_FILE;

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel codegen [--check] [--root <repo>] [--project <dart package>]\n                                               regenerate schemas/, Dart and Rust codecs (a); with --project also the\n                                               package's behaviours.g.dart/.g.json and schemas/behaviours/ (b, build_runner)\n  fynkel schema [list|describe <type>] [--json] the schema registry: document kinds, component types, fingerprint\n  fynkel test --headless [--project <dir>] [--scene <path|id>] [--cache <dir>] [--no-cook] [--steps N] [--seed S] [--satellites K] [--frame-dt SECS] [--json]\n                                               cook the project, run its startup scene (or --scene) in the Rust headless host from the cache and report;\n                                               outside a project: the procedural demo scene\n  fynkel state dump [--project <dir>] [--scene <path|id>] [--cache <dir>] [--no-cook] [--steps N] [--json]\n                                               cook, load a scene headlessly from the cache and print the runtime state by authoring id\n  fynkel document validate|format|new ...      strict JSON documents (see `fynkel document`)\n  fynkel scene inspect|validate|apply ...      scene documents (--prefabs <dir>, --expand; apply --patch <ops.json> [--dry-run])\n  fynkel prefab inspect|validate|apply ...     prefab documents\n  fynkel material inspect|validate ...         material documents, checked against the shader manifest\n  fynkel shader inspect|validate ...           shader interface manifests\n  fynkel asset list|resolve|init ...           the asset database: ids from .meta.json sidecars and documents\n  fynkel migrate <path|dir>... [--dry-run]     bring documents to the current formatVersion (originals kept as .bak)\n  fynkel import <file>... [--out <dir>]        import glTF/GLB sources to their normalized intermediate (sidecars updated)\n  fynkel cook [--project <dir>] [--cache <dir>] [--profile <name>] [--force] [--prune]\n                                               cook the project's assets into its content-addressed cache (.fynkel/cache); --assets <dir> for a bare root\n  fynkel cooked inspect <file.fyna>...         print a cooked container's META and chunk table\n  fynkel project describe|lock [--project <dir>] [--json]\n                                               the project file (fynkel.project.json) and its lock (fynkel.lock.json)\n  fynkel validate [--project <dir>] [--json]   validate the whole project: exit 0 ok, 1 errors, 2 usage/IO\n  fynkel render [--project <dir>] [--scene <path|id>] [--out <png>] [--size WxH] [--no-cook]\n                                               cook, then render the project's startup scene (or --scene) headlessly from the cache to a PNG\n  fynkel create <dir> [--name <package>] [--title <text>] [--org <domain>] [--no-flutter]\n                                               a new game project: Flutter app, fynkel.project.json, assets, one behaviour, AGENTS.md\n  fynkel run [--project <dir>] [--no-cook] [-- <flutter run args>]\n                                               validate and cook, then `flutter run` with FYNKEL_PROJECT set to the project"
    );
    ExitCode::from(1)
}

/// The repository root, or exit: for commands that write into the checkout.
fn require_repo_root(explicit: Option<PathBuf>) -> PathBuf {
    schemas::repo_root(explicit).unwrap_or_else(|| {
        eprintln!("fynkel: not inside the Fynkel repository; pass --root");
        std::process::exit(1);
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        return usage();
    };
    if cmd == "test" {
        return test_command(&args[1..]);
    }
    if cmd == "state" {
        return state_command(&args[1..]);
    }
    if cmd == "schema" && args.len() > 1 && !args[1].starts_with("--") {
        return project::run_schema(&args[1..]);
    }
    if matches!(
        cmd.as_str(),
        "document"
            | "scene"
            | "prefab"
            | "material"
            | "shader"
            | "asset"
            | "migrate"
            | "import"
            | "cook"
            | "cooked"
            | "project"
            | "validate"
            | "render"
            | "create"
            | "run"
    ) {
        let root = args
            .iter()
            .position(|a| a == "--root")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        let root = schemas::repo_root(root);
        let root = root.as_deref();
        return match cmd.as_str() {
            "asset" => asset::run(root, &args[1..]),
            "migrate" => migrate::run(root, &args[1..]),
            "import" => import::run(root, &args[1..]),
            "cook" => cook::run(root, &args[1..]),
            "cooked" => cook::run_cooked(&args[1..]),
            "project" => project::run_project(root, &args[1..]),
            "validate" => project::run_validate(root, &args[1..]),
            "render" => render::run(&args[1..]),
            "create" => create::run(root, &args[1..]),
            "run" => run::run(root, &args[1..]),
            "document" => document::run(root, &args[1..]),
            "scene" | "prefab" => scene::run(root, cmd, &args[1..]),
            other => material::run(root, other, &args[1..]),
        };
    }
    let mut check = false;
    let mut root = None;
    let mut project: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--root" => {
                i += 1;
                root = args.get(i).map(PathBuf::from);
            }
            "--project" => {
                i += 1;
                project = args.get(i).map(PathBuf::from);
            }
            other => {
                eprintln!("fynkel: unknown argument {other}");
                return usage();
            }
        }
        i += 1;
    }
    let root = require_repo_root(root);
    match cmd.as_str() {
        "codegen" => {
            let native = match codegen::run(&root, check) {
                Ok(codegen::Outcome::Written(n)) => {
                    println!(
                        "fynkel codegen: wrote {n} files (fingerprint {:#018x})",
                        fynkel_core::schema::schema_fingerprint()
                    );
                    ExitCode::SUCCESS
                }
                Ok(codegen::Outcome::UpToDate) => {
                    println!("fynkel codegen --check: up to date");
                    ExitCode::SUCCESS
                }
                Ok(codegen::Outcome::Drift(paths)) => {
                    eprintln!("fynkel codegen --check: generated files are stale:");
                    for p in paths {
                        eprintln!("  {}", p.display());
                    }
                    eprintln!("run `cargo run -p fynkel_cli -- codegen`");
                    ExitCode::from(2)
                }
                Err(e) => {
                    eprintln!("fynkel codegen: {e}");
                    ExitCode::from(1)
                }
            };
            let Some(project) = project else {
                return native;
            };
            let behaviours = match behaviours::run(&project, check) {
                Ok(behaviours::Outcome::Written { files, types }) => {
                    println!(
                        "fynkel codegen --project {}: wrote {files} files for {types} behaviour types",
                        project.display()
                    );
                    ExitCode::SUCCESS
                }
                Ok(behaviours::Outcome::UpToDate { types }) => {
                    println!(
                        "fynkel codegen --project {}: up to date ({types} behaviour types)",
                        project.display()
                    );
                    ExitCode::SUCCESS
                }
                Ok(behaviours::Outcome::Drift(paths)) => {
                    eprintln!(
                        "fynkel codegen --project {}: generated files are stale:",
                        project.display()
                    );
                    for p in paths {
                        eprintln!("  {}", p.display());
                    }
                    eprintln!(
                        "run `cargo run -p fynkel_cli -- codegen --project {}`",
                        project.display()
                    );
                    ExitCode::from(2)
                }
                Err(e) => {
                    eprintln!("fynkel codegen --project: {e}");
                    ExitCode::from(1)
                }
            };
            if native == ExitCode::SUCCESS {
                behaviours
            } else {
                native
            }
        }
        "schema" => {
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
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}

/// `fynkel test --headless`: runs the Rust headless host (blueprint §5.2).
/// Inside a project (or with `--project`/`--scene`/`--cache`) the project
/// is cooked and its scene runs from the cache; elsewhere the procedural
/// demo scene runs. The Dart headless game host (Behaviours) plugs in here
/// later; without `--headless` this is all there is, so it runs the same
/// thing and says so.
fn test_command(args: &[String]) -> ExitCode {
    use fynkel_host_headless::{run, HeadlessConfig};
    let mut cfg = HeadlessConfig::default();
    let mut target = cook::Target::default();
    let mut json = false;
    let mut headless = false;
    let mut i = 0;
    while i < args.len() {
        let value = |i: &mut usize| -> Option<&String> {
            *i += 1;
            args.get(*i)
        };
        match args[i].as_str() {
            "--headless" => headless = true,
            "--json" => json = true,
            "--no-cook" => target.no_cook = true,
            "--steps" => {
                cfg.steps = value(&mut i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(cfg.steps)
            }
            "--seed" => {
                cfg.seed = value(&mut i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(cfg.seed)
            }
            "--satellites" => {
                cfg.satellites = value(&mut i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(cfg.satellites)
            }
            "--frame-dt" => {
                cfg.frame_dt = value(&mut i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(cfg.frame_dt)
            }
            "--scene" => target.scene = value(&mut i).cloned(),
            "--project" => target.project = value(&mut i).map(PathBuf::from),
            "--cache" => target.cache = value(&mut i).map(PathBuf::from),
            "--prefabs" => {
                eprintln!("fynkel test: --prefabs is gone; prefabs come from the scene's project (cooked)");
                return usage();
            }
            "--root" => {
                i += 1;
            }
            other => {
                eprintln!("fynkel test: unknown argument {other}");
                return usage();
            }
        }
        i += 1;
    }
    if !headless && !json {
        eprintln!("fynkel test: runs the Rust headless host (pass --headless to be explicit)");
    }
    // Inside a project, the project's startup scene is what a test means.
    let scene_mode = target.project.is_some()
        || target.scene.is_some()
        || target.cache.is_some()
        || Path::new(PROJECT_FILE).is_file();
    if scene_mode {
        match cook::prepare("fynkel test", &target) {
            Ok(p) => {
                cfg.cache = Some(p.cache);
                cfg.scene = p.scene.map(|id| id.to_string());
            }
            Err(code) => return code,
        }
    }
    let report = run(&cfg);
    let ok = report.scene_ready && report.commands_rejected == 0 && report.diagnostics == 0;
    if json {
        println!("{}", report.to_json());
    } else {
        if let Some(scene) = &report.scene {
            for d in &scene.document_diagnostics {
                eprintln!("{}", diagnostic_line(d));
            }
            if scene.loaded {
                println!(
                    "loaded \"{}\" ({}) from {} ({}): {} entities, {} components rejected, {} assets retained",
                    scene.name,
                    scene.id,
                    scene.cache,
                    scene.path,
                    scene.entities,
                    scene.rejected_components,
                    scene.retained
                );
            } else {
                println!(
                    "fynkel test --headless: the scene did not load from {} — FAILED",
                    scene.cache
                );
                return ExitCode::from(1);
            }
        }
        println!(
            "fynkel test --headless: {} ticks in {:.1} ms, {} entities, {} commands applied, {} rejected, {} diagnostics, checksum {} — {}",
            report.tick,
            report.total_ms,
            report.live_entities,
            report.commands_applied,
            report.commands_rejected,
            report.diagnostics,
            report.checksum,
            if ok { "ok" } else { "FAILED" }
        );
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// One-line human form of a §14.2 diagnostic record (as JSON).
fn diagnostic_line(d: &serde_json::Value) -> String {
    let s = |k: &str| d.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let mut out = format!("{}[{}]", s("severity"), s("code"));
    if !s("document").is_empty() {
        out.push(' ');
        out.push_str(s("document"));
        if let (Some(l), Some(c)) = (
            d.get("line").and_then(|v| v.as_u64()),
            d.get("column").and_then(|v| v.as_u64()),
        ) {
            out.push_str(&format!(":{l}:{c}"));
        }
    }
    if !s("pointer").is_empty() {
        out.push(' ');
        out.push_str(s("pointer"));
    }
    out.push_str(": ");
    out.push_str(s("message"));
    out
}

/// `fynkel state dump [--project <dir>] [--scene <path|id>] [--cache <dir>] [--steps N] [--json]`:
/// cooks the project, loads the scene from the cache into a fresh headless
/// world, ticks it, and prints the state keyed by authoring id (never
/// runtime handles).
fn state_command(args: &[String]) -> ExitCode {
    use fynkel_host_headless::{dump_json, dump_text, run_with, HeadlessConfig};
    if args.first().map(String::as_str) != Some("dump") {
        eprintln!("usage: fynkel state dump [--project <dir>] [--scene <path|id>] [--cache <dir>] [--no-cook] [--steps N] [--json]");
        return ExitCode::from(2);
    }
    let mut cfg = HeadlessConfig {
        steps: 0,
        ..HeadlessConfig::default()
    };
    let mut target = cook::Target::default();
    let mut json = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--no-cook" => target.no_cook = true,
            "--scene" => {
                i += 1;
                target.scene = args.get(i).cloned();
            }
            "--project" => {
                i += 1;
                target.project = args.get(i).map(PathBuf::from);
            }
            "--cache" => {
                i += 1;
                target.cache = args.get(i).map(PathBuf::from);
            }
            "--prefabs" => {
                eprintln!("fynkel state dump: --prefabs is gone; prefabs come from the scene's project (cooked)");
                return ExitCode::from(2);
            }
            "--steps" => {
                i += 1;
                cfg.steps = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            "--root" => i += 1,
            other => {
                eprintln!("fynkel state dump: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    match cook::prepare("fynkel state dump", &target) {
        Ok(p) => {
            cfg.cache = Some(p.cache);
            cfg.scene = p.scene.map(|id| id.to_string());
        }
        Err(code) => return code,
    }
    let (report, world, loaded) = run_with(&cfg);
    let scene = report.scene.as_ref().expect("scene mode");
    let Some(loaded) = loaded else {
        if json {
            println!(
                "{}",
                serde_json::json!({ "ok": false, "diagnostics": scene.document_diagnostics })
            );
        } else {
            for d in &scene.document_diagnostics {
                eprintln!("{}", diagnostic_line(d));
            }
            eprintln!(
                "fynkel state dump: the scene did not load from {}",
                scene.cache
            );
        }
        return ExitCode::from(1);
    };
    if json {
        let mut out = dump_json(&world, &loaded);
        out["ok"] = serde_json::Value::Bool(scene.rejected_components == 0);
        out["diagnostics"] = serde_json::Value::Array(scene.document_diagnostics.clone());
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        for d in &scene.document_diagnostics {
            eprintln!("{}", diagnostic_line(d));
        }
        print!("{}", dump_text(&world, &loaded));
    }
    if scene.rejected_components == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
