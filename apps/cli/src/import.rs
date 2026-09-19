//! `fynkel import <file>... [--out <dir>] [--dry-run] [--json]` (§14.1,
//! blueprint §19.2c item 10): runs a source file through its importer and
//! writes the normalized intermediate to `<out>/<asset id>.fyni`
//! (`--out` defaults to `.fynkel/cache/import`, which version control
//! ignores). A file without a sidecar gets one first (as `fynkel asset
//! init` would); a sidecar whose `subAssets` fall behind what the file
//! produces is rewritten with the new ids, existing ids kept. `--dry-run`
//! imports and prints the summary but writes nothing. Exit 0 when every
//! file imported (warnings allowed), 1 on any error, 2 on usage.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::asset::{sidecar_path, Meta};
use fynkel_document::{diagnostics_json, has_errors, Diagnostic, Document};
use fynkel_import::{Imported, Limits};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel import <file>... [--out <dir>] [--dry-run] [--json] [--schemas <dir>]\n                                               import source files (glTF/GLB) to their normalized intermediate;\n                                               sidecars are created or updated with the sub-asset ids"
    );
    ExitCode::from(2)
}

struct Opts {
    files: Vec<PathBuf>,
    out: PathBuf,
    dry_run: bool,
    json: bool,
    schemas_dir: Option<PathBuf>,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        files: Vec::new(),
        out: PathBuf::from(".fynkel/cache/import"),
        dry_run: false,
        json: false,
        schemas_dir: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--dry-run" => o.dry_run = true,
            "--out" => {
                i += 1;
                o.out = PathBuf::from(args.get(i)?);
            }
            "--schemas" => {
                i += 1;
                o.schemas_dir = Some(PathBuf::from(args.get(i)?));
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel import: unknown argument {other}");
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
    if opts.files.is_empty() {
        return usage();
    }
    let schemas = match crate::schemas::load(root, opts.schemas_dir.as_deref()) {
        Ok(s) => s,
        Err(d) => return finish(&[d], opts.json),
    };
    let limits = Limits::default();

    let mut diags = Vec::new();
    let mut reports = Vec::new();
    for path in &opts.files {
        let shown = path.display().to_string();
        let mut report = serde_json::json!({ "file": shown, "ok": false });
        let io = |msg: String, at: &Path| {
            Diagnostic::error(fynkel_document::Code::IO, msg)
                .with_document(at.display().to_string())
        };
        if !path.is_file() {
            diags.push(io("not a file".to_string(), path));
            reports.push(report);
            continue;
        }

        // The sidecar: read and check it, or make a fresh one.
        let sidecar = sidecar_path(path);
        let (mut meta, created) = if sidecar.exists() {
            let doc = match Document::read(&sidecar, None) {
                Ok(d) => d,
                Err(d) => {
                    diags.extend(d);
                    reports.push(report);
                    continue;
                }
            };
            let meta = Meta::from_document(doc);
            let sidecar_diags = meta.validate(&schemas);
            let bad = has_errors(&sidecar_diags);
            diags.extend(sidecar_diags);
            if bad {
                reports.push(report);
                continue;
            }
            (meta, false)
        } else {
            (Meta::new(path), true)
        };
        report["id"] = meta.id().to_string().into();
        report["importer"] = meta.importer.clone().into();
        report["sidecar"] = sidecar.display().to_string().into();
        report["sidecarCreated"] = created.into();

        let imported: Imported = fynkel_import::import(path, &meta, &limits);
        diags.extend(imported.diagnostics.iter().cloned());
        let sidecar_changed = created || imported.sidecar_changed(&meta);
        let Some(intermediate) = imported.intermediate else {
            reports.push(report);
            continue;
        };
        meta.sub_assets = imported.sub_assets;
        report["ok"] = true.into();
        report["sidecarWritten"] = (sidecar_changed && !opts.dry_run).into();
        report["summary"] = intermediate.summary_json();

        let out_file = opts.out.join(format!("{}.fyni", meta.id()));
        report["intermediate"] = out_file.display().to_string().into();
        if !opts.json {
            print!("{}", intermediate.summary_text());
        }
        if opts.dry_run {
            if !opts.json {
                eprintln!(
                    "{shown}: dry run, nothing written{}",
                    if sidecar_changed {
                        " (the sidecar's subAssets would change)"
                    } else {
                        ""
                    }
                );
            }
            reports.push(report);
            continue;
        }

        if sidecar_changed {
            let write = meta.to_document().write().and_then(|text| {
                std::fs::write(&sidecar, text)
                    .map_err(|e| io(format!("cannot write sidecar: {e}"), &sidecar))
            });
            if let Err(d) = write {
                diags.push(d);
                report["ok"] = false.into();
                reports.push(report);
                continue;
            }
            if !opts.json {
                eprintln!(
                    "{shown}: sidecar {} {}",
                    sidecar.display(),
                    if created { "created" } else { "updated" }
                );
            }
        }
        let bytes = intermediate.to_bytes();
        let write = std::fs::create_dir_all(&opts.out)
            .map_err(|e| io(format!("cannot create: {e}"), &opts.out))
            .and_then(|()| {
                std::fs::write(&out_file, &bytes)
                    .map_err(|e| io(format!("cannot write: {e}"), &out_file))
            });
        match write {
            Ok(()) => {
                report["bytes"] = bytes.len().into();
                if !opts.json {
                    eprintln!("{shown}: {} bytes -> {}", bytes.len(), out_file.display());
                }
            }
            Err(d) => {
                diags.push(d);
                report["ok"] = false.into();
            }
        }
        reports.push(report);
    }

    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["dryRun"] = opts.dry_run.into();
        v["files"] = reports.into();
        println!("{v}");
        return exit(&diags);
    }
    for d in &diags {
        eprintln!("{d}");
    }
    let ok = reports.iter().filter(|r| r["ok"] == true).count();
    eprintln!(
        "fynkel import: {ok}/{} file(s) imported{}",
        opts.files.len(),
        if opts.dry_run { " (dry run)" } else { "" }
    );
    exit(&diags)
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
