//! `fynkel migrate <path|dir>... [--dry-run] [--json]` (§14.1, blueprint
//! §19.2c item 9): brings documents to the current `formatVersion`. A
//! directory means every document under it (`.scene.json`, `.prefab.json`,
//! `.material.json`, `.shader.json`, `.meta.json`, `fynkel.project.json`;
//! hidden entries skipped). `--dry-run` prints the unified diff per document
//! and writes nothing; otherwise each changed file is rewritten with the
//! original kept as `<file>.bak`. Newer documents are reported (`FYN-D0013`)
//! and never touched. Exit 0 when every document is (now) current, 1 on
//! any error, 2 on usage.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::migrate::migrated_json;
use fynkel_document::{
    diagnostics_json, has_errors, migrate, Diagnostic, Document, DocumentKind, Migrations,
};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel migrate <path|dir>... [--dry-run] [--json] [--schemas <dir>]\n                                               bring documents to the current formatVersion (originals kept as <file>.bak)\n  fynkel migrate --list [--json]              the migration steps this build knows"
    );
    ExitCode::from(2)
}

struct Opts {
    paths: Vec<PathBuf>,
    dry_run: bool,
    list: bool,
    json: bool,
    schemas_dir: Option<PathBuf>,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        paths: Vec::new(),
        dry_run: false,
        list: false,
        json: false,
        schemas_dir: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--dry-run" => o.dry_run = true,
            "--list" => o.list = true,
            "--schemas" => {
                i += 1;
                o.schemas_dir = Some(PathBuf::from(args.get(i)?));
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel migrate: unknown argument {other}");
                return None;
            }
            p => o.paths.push(PathBuf::from(p)),
        }
        i += 1;
    }
    Some(o)
}

pub fn run(root: Option<&Path>, args: &[String]) -> ExitCode {
    let Some(opts) = parse(args) else {
        return usage();
    };
    let migrations = Migrations::builtin();
    if opts.list {
        return list(&opts, &migrations);
    }
    if opts.paths.is_empty() {
        return usage();
    }
    let schemas = match crate::schemas::load(root, opts.schemas_dir.as_deref()) {
        Ok(s) => s,
        Err(d) => return finish(&[d], opts.json),
    };

    let mut diags = Vec::new();
    let mut files = Vec::new();
    for p in &opts.paths {
        if p.is_dir() {
            let mut found = Vec::new();
            if let Err(e) = collect(p, &mut found) {
                diags.push(
                    Diagnostic::error(
                        fynkel_document::Code::IO,
                        format!("cannot read directory: {e}"),
                    )
                    .with_document(p.display().to_string()),
                );
            }
            found.sort();
            files.extend(found);
        } else {
            files.push(p.clone());
        }
    }

    let mut reports = Vec::new();
    let mut changed = 0usize;
    for path in &files {
        let shown = path.display().to_string();
        let doc = match Document::read(path, None) {
            Ok(d) => d,
            Err(d) => {
                diags.extend(d);
                continue;
            }
        };
        let m = migrate(&doc, &migrations, &schemas);
        let mut report = migrated_json(&m);
        report["path"] = shown.clone().into();
        report["written"] = false.into();
        if m.has_errors() {
            diags.extend(m.diagnostics.iter().cloned());
            reports.push(report);
            continue;
        }
        diags.extend(m.diagnostics.iter().cloned());
        if !m.changed() {
            if !opts.json && files.len() == 1 {
                eprintln!("{shown}: formatVersion {} is current", m.to);
            }
            reports.push(report);
            continue;
        }
        changed += 1;
        let steps: Vec<&str> = m.applied.iter().map(|a| a.name.as_str()).collect();
        if opts.dry_run {
            if !opts.json {
                eprintln!(
                    "{shown}: formatVersion {} -> {} via {} (dry run, not written)",
                    m.from,
                    m.to,
                    steps.join(", ")
                );
                print!("{}", m.diff);
            }
        } else {
            match m.write_with_backup(path) {
                Ok(backup) => {
                    report["written"] = true.into();
                    report["backup"] = backup.display().to_string().into();
                    if !opts.json {
                        eprintln!(
                            "{shown}: formatVersion {} -> {} via {}; original kept as {}",
                            m.from,
                            m.to,
                            steps.join(", "),
                            backup.display()
                        );
                    }
                }
                Err(d) => diags.push(d),
            }
        }
        reports.push(report);
    }

    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["dryRun"] = opts.dry_run.into();
        v["documents"] = reports.into();
        println!("{v}");
        return exit(&diags);
    }
    for d in &diags {
        eprintln!("{d}");
    }
    if !has_errors(&diags) {
        eprintln!(
            "fynkel migrate: {} document(s), {} {}",
            files.len(),
            changed,
            if opts.dry_run {
                "would change"
            } else {
                "migrated"
            }
        );
    }
    exit(&diags)
}

fn list(opts: &Opts, migrations: &Migrations) -> ExitCode {
    if opts.json {
        let steps: Vec<serde_json::Value> = migrations
            .steps()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "kind": s.kind.name(), "from": s.from, "to": s.to(),
                    "name": s.id(), "description": s.description,
                })
            })
            .collect();
        let current: serde_json::Map<String, serde_json::Value> = DocumentKind::ALL
            .iter()
            .map(|k| (k.name().to_string(), k.supported_format_version().into()))
            .collect();
        println!(
            "{}",
            serde_json::json!({ "ok": true, "current": current, "steps": steps })
        );
        return ExitCode::SUCCESS;
    }
    for k in DocumentKind::ALL {
        println!(
            "{:<9} current formatVersion {}",
            k.name(),
            k.supported_format_version()
        );
    }
    for s in migrations.steps() {
        println!("{}  {}", s.id(), s.description);
    }
    ExitCode::SUCCESS
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            collect(&path, out)?;
        } else if DocumentKind::from_path(&path).is_some() {
            out.push(path);
        }
    }
    Ok(())
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
