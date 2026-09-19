//! `fynkel` — the Fynkel command-line tool.
//!
//! M1 scope: `fynkel codegen` (ADR-0012, native schema generator) and
//! `fynkel schema`. Exit codes: 0 ok, 1 error, 2 `--check` found drift.

mod codegen;

use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel codegen [--check] [--root <repo>]   regenerate schemas/, Dart and Rust codecs\n  fynkel schema [--root <repo>]                print the schema registry and fingerprint"
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
