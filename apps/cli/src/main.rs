//! `fynkel` — the Fynkel command-line tool.
//!
//! M1 scope: `fynkel codegen` (ADR-0012, native schema generator),
//! `fynkel schema`, and the seed of `fynkel test` (§13.3/§14.1): the Rust
//! headless host. M2 adds `fynkel document` (validate/format/new), `fynkel
//! scene`, `fynkel prefab`, `fynkel material`, `fynkel shader`, `fynkel asset`,
//! `fynkel migrate`, `fynkel import`, `fynkel cook`, `fynkel cooked inspect`,
//! `--scene`/`--project` for the headless host, `fynkel state dump`,
//! `fynkel create` and `fynkel run`. M3 makes the cooked cache the only
//! thing the runtime reads: `test --headless`, `state dump`, `render`,
//! `thumbnail` and `run` cook the project first (`cook::prepare`). Outside the repository
//! the commands validate against the embedded schema set (`schemas.rs`).
//! Exit codes: 0 ok, 1 error, 2 usage or `--check` found drift.

// `fynkel render --json` nests deeply enough for serde_json's default.
#![recursion_limit = "256"]

mod asset;
mod assets_gen;
mod behaviours;
mod bundle;
mod codegen;
mod cook;
mod create;
mod document;
mod editor;
mod editor_launch;
mod import;
mod material;
mod migrate;
mod project;
mod render;
mod rig;
mod run;
mod scene;
mod schemas;
mod thumbnail;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::project::PROJECT_FILE;

/// The counting allocator (`fynkel_core::memory`, M3-16) so `fynkel test
/// --headless` reports the heap.
#[global_allocator]
static ALLOCATOR: fynkel_core::memory::CountingAllocator = fynkel_core::memory::CountingAllocator;

/// `fynkel codegen [--check] [--project <dir>] --json`: the same two
/// pipelines, reported as one JSON object on stdout —
/// `{ ok, check, native: { status, files?, types?, stale?[] }, project?:
/// { dir, status, files?, types?, stale?[], error? } }` where `status` is
/// `written`, `upToDate`, `stale` or `error`. Exit codes as without
/// `--json`: 0 ok, 2 stale, 1 a pipeline failed. The editor runs
/// `--check --project <game> --json` for its "run `fynkel codegen`" banner
/// (blueprint §19.2e item 4), and `--project <game> --project-only --json`
/// for Create → Behaviour. `root` None is `--project-only`: the native
/// pipeline is skipped (`native: { status: "skipped" }`), so a CLI older
/// or newer than the checkout never rewrites the engine's own files.
fn codegen_json(
    root: Option<&std::path::Path>,
    project: Option<&std::path::Path>,
    check: bool,
) -> ExitCode {
    use serde_json::json;
    fn stale(paths: &[PathBuf]) -> serde_json::Value {
        json!(paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>())
    }
    let Some(root) = root else {
        return codegen_json_project(
            json!({ "status": "skipped" }),
            ExitCode::SUCCESS,
            project,
            check,
        );
    };
    let (native, native_exit) = match codegen::run(root, check) {
        Ok(codegen::Outcome::Written(n)) => (
            json!({
                "status": "written",
                "files": n,
                "fingerprint": format!("{:#018x}", fynkel_core::schema::schema_fingerprint())
            }),
            ExitCode::SUCCESS,
        ),
        Ok(codegen::Outcome::UpToDate) => (json!({ "status": "upToDate" }), ExitCode::SUCCESS),
        Ok(codegen::Outcome::Drift(paths)) => (
            json!({ "status": "stale", "stale": stale(&paths) }),
            ExitCode::from(2),
        ),
        Err(e) => (json!({ "status": "error", "error": e }), ExitCode::from(1)),
    };
    codegen_json_project(native, native_exit, project, check)
}

/// The project half of [`codegen_json`] and the printed report.
fn codegen_json_project(
    native: serde_json::Value,
    native_exit: ExitCode,
    project: Option<&std::path::Path>,
    check: bool,
) -> ExitCode {
    use serde_json::json;
    fn stale(paths: &[PathBuf]) -> serde_json::Value {
        json!(paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>())
    }
    let (project_json, project_exit) = match project {
        None => (None, ExitCode::SUCCESS),
        Some(dir) => {
            let dir_text = dir.display().to_string();
            let command = format!("fynkel codegen --project {dir_text}");
            match behaviours::run(dir, check) {
                Ok(behaviours::Outcome::Written { files, types, data }) => (
                    Some(
                        json!({ "dir": dir_text, "status": "written", "files": files, "types": types, "dataTypes": data }),
                    ),
                    ExitCode::SUCCESS,
                ),
                Ok(behaviours::Outcome::UpToDate { types, data }) => (
                    Some(
                        json!({ "dir": dir_text, "status": "upToDate", "types": types, "dataTypes": data }),
                    ),
                    ExitCode::SUCCESS,
                ),
                Ok(behaviours::Outcome::Drift(paths)) => (
                    Some(
                        json!({ "dir": dir_text, "status": "stale", "stale": stale(&paths), "command": command }),
                    ),
                    ExitCode::from(2),
                ),
                Err(e) => (
                    Some(
                        json!({ "dir": dir_text, "status": "error", "error": e, "command": command }),
                    ),
                    ExitCode::from(1),
                ),
            }
        }
    };
    let ok = native_exit == ExitCode::SUCCESS && project_exit == ExitCode::SUCCESS;
    let mut out = json!({ "ok": ok, "check": check, "native": native });
    if let Some(p) = project_json {
        out["project"] = p;
    }
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
    if native_exit != ExitCode::SUCCESS {
        native_exit
    } else {
        project_exit
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel codegen [--check] [--root <repo>] [--project <dart package> [--project-only]] [--json]\n                                               regenerate schemas/, Dart and Rust codecs (a); with --project also the\n                                               package's behaviours.g.dart/.g.json and schemas/behaviours/ (b, build_runner);\n                                               --project-only: (b) alone, the engine's files untouched\n                                               --json prints one report object instead (the editor's staleness banner)\n  fynkel codegen --watch --project <dir>       (b) kept running: regenerates on every saved script, one JSON line per build\n                                               (`building`, `built`, `failed`); ends when stdin closes (the editor)\n  fynkel schema [list|describe <type>] [--json] the schema registry: document kinds, component types, fingerprint\n  fynkel test --headless [--project <dir>] [--scene <path|id>] [--cache <dir>] [--no-cook] [--input <file.input.json>] [--steps N] [--seed S] [--satellites K] [--frame-dt SECS] [--json]\n                                               cook the project, run its startup scene (or --scene) in the Rust headless host from the cache and report;\n                                               --input replays a recorded input stream against the project's actions; outside a project: the procedural demo scene\n  fynkel state dump [--project <dir>] [--scene <path|id>] [--cache <dir>] [--no-cook] [--steps N] [--json]\n                                               cook, load a scene headlessly from the cache and print the runtime state by authoring id\n  fynkel document validate|format|new ...      strict JSON documents (see `fynkel document`)\n  fynkel scene inspect|validate|apply ...      scene documents (--prefabs <dir>, --expand; apply --patch <ops.json> [--dry-run])\n  fynkel prefab inspect|validate|apply ...     prefab documents\n  fynkel material inspect|validate ...         material documents, checked against the shader manifest\n  fynkel shader inspect|validate ...           shader interface manifests\n  fynkel asset list|resolve|init ...           the asset database: ids from .meta.json sidecars and documents\n  fynkel migrate <path|dir>... [--dry-run]     bring documents to the current formatVersion (originals kept as .bak)\n  fynkel import <file>... [--out <dir>] [--rig <name>]\n                                               import glTF/GLB/FBX sources to their normalized intermediate (sidecars updated);\n                                               --rig writes <name>.rig.json: the first file's skeleton with every file's clips\n  fynkel rig inspect|validate <path.rig.json>  a rig document: its skeleton and clips matched by bone name\n  fynkel cook [--project <dir>] [--cache <dir>] [--profile <name>] [--force] [--prune]\n                                               cook the project's assets into its content-addressed cache (.fynkel/cache); --assets <dir> for a bare root\n  fynkel cooked inspect <file.fyna>...         print a cooked container's META and chunk table\n  fynkel project describe|lock [--project <dir>] [--json]\n                                               the project file (fynkel.project.json) and its lock (fynkel.lock.json)\n  fynkel validate [--project <dir>] [--json]   validate the whole project: exit 0 ok, 1 errors, 2 usage/IO\n  fynkel render [--project <dir>] [--scene <path|id>] [--out <png>] [--size WxH] [--no-cook]\n                                               cook, then render the project's startup scene (or --scene) headlessly from the cache to a PNG\n  fynkel thumbnail [--project <dir>] [--size <px>] --out-dir <dir> [--json] <assetId>[#<sub>]...\n                                               cook, then draw asset thumbnails (prefab, model, mesh, material on a sphere) to <out-dir>/<assetId>[__<sub>].png\n  fynkel create <dir> [--name <package>] [--title <text>] [--org <domain>] [--no-flutter]\n                                               a new game project: Flutter app, fynkel.project.json, assets, one behaviour, AGENTS.md\n  fynkel run [--project <dir>] [--no-cook] [-- <flutter run args>]\n                                               validate and cook, then `flutter run` with FYNKEL_PROJECT set to the project\n  fynkel editor [<project dir>] [--plain] [--handover] [--flutter <path>]\n                                               the editor with the project's game compiled in: Play runs inside it, hot reload on save (ADR-0021)"
    );
    ExitCode::from(1)
}

/// `fynkel codegen --project <dir> --assets-only [--check] [--json]`:
/// only `lib/generated/assets.g.dart` (ADR-0026 §3), no build_runner — what
/// the editor runs after an asset is created, imported, renamed or
/// deleted. `--json` prints `{ ok, check, assets: { status, error? } }`
/// with `status` `written`, `upToDate`, `stale`, `skipped` (not a
/// project's package) or `error`. Exit codes as for `codegen`.
fn assets_only_run(project: &Path, check: bool, json: bool) -> ExitCode {
    let mut drift = Vec::new();
    let (status, code, error) = match behaviours::write_assets(project, check, &mut drift) {
        Err(e) => ("error", ExitCode::from(1), Some(e)),
        Ok(_) if !project.join("fynkel.project.json").is_file() => {
            ("skipped", ExitCode::SUCCESS, None)
        }
        Ok(_) if drift.is_empty() => ("upToDate", ExitCode::SUCCESS, None),
        Ok(_) if check => ("stale", ExitCode::from(2), None),
        Ok(_) => ("written", ExitCode::SUCCESS, None),
    };
    if json {
        let mut assets = serde_json::json!({ "status": status });
        if let Some(e) = &error {
            assets["error"] = serde_json::json!(e);
        }
        println!(
            "{}",
            serde_json::json!({ "ok": code == ExitCode::SUCCESS, "check": check, "assets": assets })
        );
    } else if let Some(e) = error {
        eprintln!("fynkel codegen --assets-only: {e}");
    } else {
        println!(
            "fynkel codegen --project {} --assets-only: {status}",
            project.display()
        );
    }
    code
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
            | "thumbnail"
            | "create"
            | "run"
            | "editor"
            | "rig"
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
            "rig" => rig::run(root, &args[1..]),
            "cook" => cook::run(root, &args[1..]),
            "cooked" => cook::run_cooked(&args[1..]),
            "project" => project::run_project(root, &args[1..]),
            "validate" => project::run_validate(root, &args[1..]),
            "render" => render::run(&args[1..]),
            "thumbnail" => thumbnail::run(&args[1..]),
            "create" => create::run(root, &args[1..]),
            "run" => run::run(root, &args[1..]),
            "editor" => editor::run(root, &args[1..]),
            "document" => document::run(root, &args[1..]),
            "scene" | "prefab" => scene::run(root, cmd, &args[1..]),
            other => material::run(root, other, &args[1..]),
        };
    }
    let mut check = false;
    let mut json = false;
    let mut root = None;
    let mut project: Option<PathBuf> = None;
    let mut project_only = false;
    let mut assets_only = false;
    let mut watch = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--json" => json = true,
            "--project-only" => project_only = true,
            "--assets-only" => assets_only = true,
            "--watch" => watch = true,
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
    if watch {
        let Some(project) = project.as_deref().filter(|_| cmd == "codegen" && !check) else {
            eprintln!("fynkel codegen: --watch needs --project <dir> and no --check");
            return usage();
        };
        return match behaviours::watch(project) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                println!("{}", serde_json::json!({ "event": "exited", "error": e }));
                ExitCode::from(1)
            }
        };
    }
    if assets_only {
        let Some(project) = project.as_deref().filter(|_| cmd == "codegen") else {
            eprintln!("fynkel codegen: --assets-only needs --project <dir>");
            return usage();
        };
        return assets_only_run(project, check, json);
    }
    if project_only && project.is_none() {
        eprintln!("fynkel codegen: --project-only needs --project <dir>");
        return usage();
    }
    let root = if project_only {
        PathBuf::new()
    } else {
        require_repo_root(root)
    };
    match cmd.as_str() {
        "codegen" if json => codegen_json(
            (!project_only).then_some(root.as_path()),
            project.as_deref(),
            check,
        ),
        "codegen" => {
            let native = if project_only {
                ExitCode::SUCCESS
            } else {
                match codegen::run(&root, check) {
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
                }
            };
            let Some(project) = project else {
                return native;
            };
            let behaviours = match behaviours::run(&project, check) {
                Ok(behaviours::Outcome::Written { files, types, data }) => {
                    println!(
                        "fynkel codegen --project {}: wrote {files} files for {}",
                        project.display(),
                        behaviours::describe_counts(types, data)
                    );
                    ExitCode::SUCCESS
                }
                Ok(behaviours::Outcome::UpToDate { types, data }) => {
                    println!(
                        "fynkel codegen --project {}: up to date ({})",
                        project.display(),
                        behaviours::describe_counts(types, data)
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
            "--input" => match value(&mut i) {
                Some(v) => cfg.input = Some(PathBuf::from(v)),
                None => return usage(),
            },
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
    // A recording that does not parse, or a binding the table could not
    // compile, fails the run: the stream would not drive what it was
    // recorded against.
    let input_ok = report
        .input
        .as_ref()
        .is_none_or(|i| i.binding_errors.is_empty());
    let ok =
        report.scene_ready && report.commands_rejected == 0 && report.diagnostics == 0 && input_ok;
    if json {
        println!("{}", report.to_json());
    } else {
        if let Some(input) = &report.input {
            for [action, binding, message] in &input.binding_errors {
                if action.is_empty() {
                    eprintln!("fynkel test --headless: --input {message}");
                } else {
                    eprintln!(
                        "fynkel test --headless: action `{action}` binding `{binding}`: {message}"
                    );
                }
            }
            if input.binding_errors.is_empty() {
                println!(
                    "replayed {} ({} frames, {} events): {}",
                    input.recording,
                    input.frames,
                    input.events,
                    if input.actions.is_empty() {
                        "no actions".to_string()
                    } else {
                        input
                            .actions
                            .iter()
                            .map(|a| match a.kind.as_str() {
                                "button" => format!(
                                    "{} pressed {} released {} held {} ticks",
                                    a.name, a.pressed, a.released, a.held_ticks
                                ),
                                "axis1d" => format!(
                                    "{} held {} ticks, ends at {:.2}",
                                    a.name, a.held_ticks, a.x
                                ),
                                _ => format!(
                                    "{} held {} ticks, ends at ({:.2}, {:.2})",
                                    a.name, a.held_ticks, a.x, a.y
                                ),
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                );
            }
        }
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
        if let Some(p) = &report.physics {
            println!(
                "physics: {} bodies, {} colliders ({} mesh), {} characters, collisions {} enter / {} stay / {} exit, triggers {} enter / {} exit",
                p.bodies,
                p.colliders,
                p.mesh_colliders,
                p.characters,
                p.collision_enter,
                p.collision_stay,
                p.collision_exit,
                p.trigger_enter,
                p.trigger_exit
            );
        }
        if let Some(a) = &report.animation {
            println!(
                "animation: {} animators + skins, {} clips, {} skins, {} skinned meshes ({} joints), {} ended, {} markers, {} diagnostics",
                a.entities, a.clips, a.skins, a.skinned_meshes, a.joints, a.ended, a.markers, a.diagnostics
            );
        }
        if let Some(a) = &report.audio {
            println!(
                "audio: {} sources, {} sounds ({} streamed), {} voices started ({} stolen, {} refused), {} ended, {} diagnostics",
                a.sources, a.sounds, a.streams, a.started, a.stolen, a.refused, a.ended, a.diagnostics
            );
        }
        if !report.systems.is_empty() {
            let parts: Vec<String> = report
                .systems
                .iter()
                .filter(|s| s.system != "step")
                .map(|s| format!("{} {:.0} µs ({})", s.system, s.mean_us, s.entities))
                .collect();
            println!("systems (mean per tick, last count): {}", parts.join(", "));
        }
        let t = |t: &fynkel_host_headless::Timing| {
            format!("p50 {} / p95 {} / max {} µs", t.p50_us, t.p95_us, t.max_us)
        };
        println!(
            "timing: step {}; extract {}; frame {}",
            t(&report.step_time),
            t(&report.extract_time),
            t(&report.frame_time)
        );
        let m = &report.memory;
        println!(
            "memory: native {:.1} MB (peak {:.1}), resident {:.1} MB, assets {:.1} MB",
            m.native_bytes as f64 / 1048576.0,
            m.native_peak_bytes as f64 / 1048576.0,
            m.resident_bytes as f64 / 1048576.0,
            m.residency_bytes as f64 / 1048576.0
        );
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
