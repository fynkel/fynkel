//! `fynkel` — the Fynkel command-line tool.
//!
//! M1 scope: `fynkel codegen` (ADR-0012, native schema generator),
//! `fynkel schema`, and the seed of `fynkel test` (§13.3/§14.1): the Rust
//! headless host. M2 adds `fynkel document` (validate/format/new), `fynkel
//! scene`, `fynkel prefab`, `fynkel material`, `fynkel shader`, `--scene` for the headless
//! host and `fynkel state dump`. Exit
//! codes: 0 ok, 1 error, 2 `--check` found drift.

mod asset;
mod codegen;
mod document;
mod material;
mod migrate;
mod scene;

use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel codegen [--check] [--root <repo>]   regenerate schemas/, Dart and Rust codecs\n  fynkel schema [--root <repo>]                print the schema registry and fingerprint\n  fynkel test --headless [--scene <path>] [--prefabs <dir>] [--steps N] [--seed S] [--satellites K] [--frame-dt SECS] [--json]\n                                               run the Rust headless host (demo scene or a .scene.json) and report\n  fynkel state dump --scene <path> [--prefabs <dir>] [--steps N] [--json]\n                                               load a scene headlessly and print the runtime state by authoring id\n  fynkel document validate|format|new ...      strict JSON documents (see `fynkel document`)\n  fynkel scene inspect|validate ...            scene documents (--prefabs <dir>, --expand)\n  fynkel prefab inspect|validate ...           prefab documents\n  fynkel material inspect|validate ...         material documents, checked against the shader manifest\n  fynkel shader inspect|validate ...           shader interface manifests\n  fynkel asset list|resolve|init ...           the asset database: ids from .meta.json sidecars and documents\n  fynkel migrate <path|dir>... [--dry-run]     bring documents to the current formatVersion (originals kept as .bak)"
    );
    ExitCode::from(1)
}

fn repo_root(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    // Walk up from cwd to the workspace root (Cargo.toml with [workspace]).
    let mut dir = std::env::current_dir().expect("cwd");
    loop {
        let cargo = dir.join("Cargo.toml");
        if cargo.is_file()
            && std::fs::read_to_string(&cargo)
                .map(|s| s.contains("[workspace]"))
                .unwrap_or(false)
        {
            return dir;
        }
        if !dir.pop() {
            eprintln!("fynkel: not inside the Fynkel repository; pass --root");
            std::process::exit(1);
        }
    }
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
    if matches!(
        cmd.as_str(),
        "document" | "scene" | "prefab" | "material" | "shader" | "asset" | "migrate"
    ) {
        let root = args
            .iter()
            .position(|a| a == "--root")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        let root = repo_root(root);
        return match cmd.as_str() {
            "asset" => asset::run(&root, &args[1..]),
            "migrate" => migrate::run(&root, &args[1..]),
            "document" => document::run(&root, &args[1..]),
            "scene" | "prefab" => scene::run(&root, cmd, &args[1..]),
            other => material::run(&root, other, &args[1..]),
        };
    }
    let mut check = false;
    let mut root = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--root" => {
                i += 1;
                root = args.get(i).map(PathBuf::from);
            }
            other => {
                eprintln!("fynkel: unknown argument {other}");
                return usage();
            }
        }
        i += 1;
    }
    let root = repo_root(root);
    match cmd.as_str() {
        "codegen" => match codegen::run(&root, check) {
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
        },
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
/// The Dart headless game host (Behaviours) plugs in here later; without
/// `--headless` this is all M1 has, so it runs the same thing and says so.
fn test_command(args: &[String]) -> ExitCode {
    use fynkel_host_headless::{run, HeadlessConfig};
    let mut cfg = HeadlessConfig::default();
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
            "--scene" => cfg.scene = value(&mut i).map(PathBuf::from),
            "--prefabs" => cfg.prefabs = value(&mut i).map(PathBuf::from),
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
        eprintln!("fynkel test: M1 runs the Rust headless host (pass --headless to be explicit)");
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
                    "loaded \"{}\" ({}) from {}: {} entities, {} components rejected",
                    scene.name, scene.id, scene.path, scene.entities, scene.rejected_components
                );
            } else {
                println!(
                    "fynkel test --headless: {} did not load — FAILED",
                    scene.path
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

/// `fynkel state dump --scene <path> [--prefabs <dir>] [--steps N] [--json]`: loads the
/// document into a fresh headless world, ticks it, and prints the state
/// keyed by authoring id (never runtime handles).
fn state_command(args: &[String]) -> ExitCode {
    use fynkel_host_headless::{dump_json, dump_text, run_with, HeadlessConfig};
    if args.first().map(String::as_str) != Some("dump") {
        eprintln!("usage: fynkel state dump --scene <path> [--prefabs <dir>] [--steps N] [--json]");
        return ExitCode::from(2);
    }
    let mut cfg = HeadlessConfig {
        steps: 0,
        ..HeadlessConfig::default()
    };
    let mut json = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--scene" => {
                i += 1;
                cfg.scene = args.get(i).map(PathBuf::from);
            }
            "--prefabs" => {
                i += 1;
                cfg.prefabs = args.get(i).map(PathBuf::from);
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
    if cfg.scene.is_none() {
        eprintln!("fynkel state dump: --scene <path> is required");
        return ExitCode::from(2);
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
            eprintln!("fynkel state dump: {} did not load", scene.path);
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
