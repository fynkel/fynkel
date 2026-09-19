//! `fynkel` — the Fynkel command-line tool.
//!
//! M1 scope: `fynkel codegen` (ADR-0012, native schema generator),
//! `fynkel schema`, and the seed of `fynkel test` (§13.3/§14.1): the Rust
//! headless host. M2 adds `fynkel document` (validate/format/new). Exit codes: 0 ok, 1 error, 2 `--check` found drift.

mod codegen;
mod document;
mod scene;

use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel codegen [--check] [--root <repo>]   regenerate schemas/, Dart and Rust codecs\n  fynkel schema [--root <repo>]                print the schema registry and fingerprint\n  fynkel test --headless [--steps N] [--seed S] [--satellites K] [--frame-dt SECS] [--json]\n                                               run the Rust headless host and report\n  fynkel document validate|format|new ...      strict JSON documents (see `fynkel document`)"
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
    if cmd == "document" || cmd == "scene" {
        let root = args
            .iter()
            .position(|a| a == "--root")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        let root = repo_root(root);
        return if cmd == "document" {
            document::run(&root, &args[1..])
        } else {
            scene::run(&root, &args[1..])
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
