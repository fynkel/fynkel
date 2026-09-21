//! `fynkel import <file>... [--out <dir>] [--dry-run] [--json]` (§14.1,
//! blueprint §19.2c item 10): runs a source file through its importer and
//! writes the normalized intermediate to `<out>/<asset id>.fyni`
//! (`--out` defaults to `.fynkel/cache/import`, which version control
//! ignores). A file without a sidecar gets one first (as `fynkel asset
//! init` would); a sidecar whose `subAssets` fall behind what the file
//! produces is rewritten with the new ids, existing ids kept. `--dry-run`
//! imports and prints the summary but writes nothing. `--prefab` (M3-19a)
//! also writes the source's node hierarchy — the `prefab` sub-asset the
//! cook carries and a scene instances as `{ asset: <source>, sub:
//! "prefab" }` — as an editable `<stem>.prefab.json` next to the source
//! with a fresh document id, for the cases that want authored, stable ids
//! (never over an existing file). `--rig <name>` (M3-19b) writes
//! `<name>.rig.json` next to the first file: the first file is the
//! skeleton, every file's clips join under identifier names (a file with
//! one clip is named after the file — Mixamo's `Fast Run.fbx` is
//! `fast_run` —, a file with several after each clip), and every clip is
//! matched to the skeleton by bone name here already, as the cook will
//! (`FYN-A0106`). An existing rig of that name keeps its id and gets its
//! skeleton and clips rewritten, so scenes naming it keep working. Exit 0
//! when every file imported (warnings allowed), 1 on any error, 2 on
//! usage.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::asset::{sidecar_path, Meta};
use fynkel_document::{diagnostics_json, has_errors, Diagnostic, Document};
use fynkel_import::{Imported, Limits};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel import <file>... [--out <dir>] [--prefab] [--rig <name>] [--dry-run] [--json] [--schemas <dir>]\n                                               import source files (glTF/GLB, FBX) to their normalized intermediate;\n                                               sidecars are created or updated with the sub-asset ids;\n                                               --prefab also writes the hierarchy as <stem>.prefab.json next to the source;\n                                               --rig writes <name>.rig.json next to the first file: its skeleton, every file's clips"
    );
    ExitCode::from(2)
}

struct Opts {
    files: Vec<PathBuf>,
    out: PathBuf,
    dry_run: bool,
    prefab: bool,
    rig: Option<String>,
    json: bool,
    schemas_dir: Option<PathBuf>,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        files: Vec::new(),
        out: PathBuf::from(".fynkel/cache/import"),
        dry_run: false,
        prefab: false,
        rig: None,
        json: false,
        schemas_dir: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--dry-run" => o.dry_run = true,
            "--prefab" => o.prefab = true,
            "--rig" => {
                i += 1;
                let name = args.get(i)?.clone();
                if !fynkel_document::rig::is_clip_name(&name) {
                    eprintln!("fynkel import: --rig {name:?} is not an identifier (letters, digits and _)");
                    return None;
                }
                o.rig = Some(name);
            }
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
    let mut parts: Vec<RigPart> = Vec::new();
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
        if opts.rig.is_some() {
            parts.push(RigPart::new(path, meta.id(), &intermediate));
        }

        let out_file = opts.out.join(format!("{}.fyni", meta.id()));
        report["intermediate"] = out_file.display().to_string().into();
        if !opts.json {
            print!("{}", intermediate.summary_text());
        }
        let prefab_file = opts.prefab.then(|| {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "model".to_string());
            path.with_file_name(format!("{stem}.prefab.json"))
        });
        if let Some(out) = &prefab_file {
            report["prefab"] = out.display().to_string().into();
            if intermediate.prefab.is_none() {
                diags.push(
                    Diagnostic::error(
                        fynkel_document::Code::IMPORT_SKIPPED,
                        "the source has no node hierarchy to write as a prefab".to_string(),
                    )
                    .with_document(shown.clone()),
                );
                report["ok"] = false.into();
                reports.push(report);
                continue;
            }
            if out.exists() {
                diags.push(io(
                    "already exists; remove it to write the imported hierarchy again".to_string(),
                    out,
                ));
                report["ok"] = false.into();
                reports.push(report);
                continue;
            }
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
        if let Some(out) = &prefab_file {
            let text = intermediate
                .prefab
                .as_ref()
                .map(|p| authored_prefab(p, &shown))
                .unwrap_or_default();
            match std::fs::write(out, text) {
                Ok(()) => {
                    if !opts.json {
                        eprintln!(
                            "{shown}: {} entities -> {}",
                            intermediate.prefab_entity_count(),
                            out.display()
                        );
                    }
                }
                Err(e) => {
                    diags.push(io(format!("cannot write: {e}"), out));
                    report["ok"] = false.into();
                    reports.push(report);
                    continue;
                }
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

    let rig = match &opts.rig {
        Some(name) if !has_errors(&diags) && parts.len() == opts.files.len() => {
            Some(write_rig(name, &parts, opts.dry_run, opts.json, &mut diags))
        }
        Some(name) => {
            diags.push(
                Diagnostic::error(
                    fynkel_document::Code::IMPORT_SKIPPED,
                    format!("{name}.rig.json not written: not every file imported"),
                )
                .with_document(opts.files[0].display().to_string()),
            );
            None
        }
        None => None,
    };

    if opts.json {
        let mut v = diagnostics_json(&diags);
        v["dryRun"] = opts.dry_run.into();
        v["files"] = reports.into();
        if let Some(rig) = rig {
            v["rig"] = rig;
        }
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

/// What `--rig` keeps of one imported file: enough to name its clips and
/// match them to the first file's skeleton.
struct RigPart {
    path: PathBuf,
    asset: uuid::Uuid,
    names: fynkel_import::rig::SkeletonNames,
    joints: Option<std::collections::BTreeSet<String>>,
    /// `(sub-asset id, clip name in the file, payload)`.
    animations: Vec<(String, String, fynkel_import::AnimationPayload)>,
}

impl RigPart {
    fn new(path: &Path, asset: uuid::Uuid, im: &fynkel_import::Intermediate) -> RigPart {
        RigPart {
            path: path.to_path_buf(),
            asset,
            names: fynkel_import::rig::SkeletonNames::from_intermediate(im),
            joints: fynkel_import::rig::joint_names(im),
            animations: im
                .animations
                .iter()
                .map(|a| (a.sub_id.clone(), a.name.clone(), a.payload.clone()))
                .collect(),
        }
    }
}

/// The rig document `--rig <name>` writes next to the first file, and its
/// clips' match against the skeleton (the first file). Returns the
/// `--json` report.
fn write_rig(
    name: &str,
    parts: &[RigPart],
    dry_run: bool,
    json: bool,
    diags: &mut Vec<Diagnostic>,
) -> serde_json::Value {
    use fynkel_document::rig::{clip_name_from, Rig};
    let skeleton = &parts[0];
    let out = skeleton.path.with_file_name(format!("{name}.rig.json"));
    let shown = out.display().to_string();
    let mut report = serde_json::json!({ "file": shown, "ok": false, "written": false });
    if skeleton.names.is_empty() {
        diags.push(
            Diagnostic::error(
                fynkel_document::Code::RIG_SKELETON,
                "the first file has no hierarchy to be the rig's skeleton".to_string(),
            )
            .with_document(skeleton.path.display().to_string())
            .with_suggestion("name the model with the character's skin and bones first"),
        );
        return report;
    }

    // The existing document's id survives; everything else is rewritten.
    let (existing_id, existing_created) = if out.is_file() {
        match Document::read(&out, Some(fynkel_document::DocumentKind::Rig)) {
            Ok(doc) => (Some(doc.envelope.id), false),
            Err(d) => {
                diags.extend(d);
                return report;
            }
        }
    } else {
        (None, true)
    };
    let id = existing_id.unwrap_or_else(uuid::Uuid::new_v4);

    // Clip names: the file's stem when it carries one clip, else each
    // clip's own name; later duplicates get _2, _3, ...
    let mut clips: Vec<(String, uuid::Uuid, String)> = Vec::new();
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
    for part in parts {
        for (sub, clip_name, _) in &part.animations {
            let base = if part.animations.len() == 1 {
                clip_name_from(
                    &part
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
            } else {
                clip_name_from(clip_name)
            };
            let mut unique = base.clone();
            let mut n = 2;
            while !taken.insert(unique.clone()) {
                unique = format!("{base}_{n}");
                n += 1;
            }
            clips.push((unique, part.asset, sub.clone()));
        }
    }
    let value = serde_json::json!({
        "$schema": fynkel_document::DocumentKind::Rig.schema_id(),
        "formatVersion": 1,
        "id": id.to_string(),
        "name": name,
        "description": format!(
            "Written by `fynkel import --rig {name}` from {}; the skeleton is {}, the clips come from every file given.",
            parts.iter().map(|p| p.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()).collect::<Vec<_>>().join(", "),
            skeleton.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
        ),
        "skeleton": { "asset": skeleton.asset.to_string() },
        "clips": clips.iter().map(|(n, asset, sub)| (n.clone(), serde_json::json!({ "asset": asset.to_string(), "sub": sub }))).collect::<serde_json::Map<_, _>>(),
    });
    let doc = match Document::parse(
        fynkel_document::DocumentKind::Rig,
        Some(&shown),
        &serde_json::to_string(&value).unwrap_or_default(),
    ) {
        Ok(d) => d,
        Err(d) => {
            diags.extend(d);
            return report;
        }
    };
    let rig = Rig::from_document(doc);
    report["id"] = id.to_string().into();
    report["created"] = existing_created.into();

    // Every clip against the skeleton, as the cook will match it.
    let mut clip_reports = Vec::new();
    let mut ok = true;
    for (clip_name, asset, sub) in &clips {
        let part = parts
            .iter()
            .find(|p| p.asset == *asset)
            .expect("from parts");
        let (_, _, payload) = part
            .animations
            .iter()
            .find(|(s, _, _)| s == sub)
            .expect("from parts");
        let (retargeted, mut clip_diags) =
            fynkel_import::rig::retarget(clip_name, payload, part.joints.as_ref(), &skeleton.names);
        for d in &mut clip_diags {
            d.document = Some(shown.clone());
            d.pointer = Some(format!("/clips/{clip_name}"));
        }
        ok &= !has_errors(&clip_diags);
        diags.extend(clip_diags);
        if !json {
            eprintln!(
                "  {clip_name:<16} {}#{sub}  {:.2} s  {}/{} channel(s){}{}",
                part.path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                payload.duration,
                retargeted.matched,
                payload.channels.len(),
                if retargeted.dropped.is_empty() {
                    String::new()
                } else {
                    format!(", {} dropped", retargeted.dropped.len())
                },
                if retargeted.missing.is_empty() {
                    String::new()
                } else {
                    format!(", {} bone(s) missing", retargeted.missing.len())
                }
            );
        }
        clip_reports.push(serde_json::json!({
            "name": clip_name,
            "sub": Rig::clip_sub(clip_name),
            "source": format!("{}#{sub}", part.path.display()),
            "seconds": payload.duration,
            "channels": payload.channels.len(),
            "matched": retargeted.matched,
            "dropped": retargeted.dropped,
            "missing": retargeted.missing,
        }));
    }
    report["clips"] = clip_reports.into();
    report["ok"] = ok.into();
    if dry_run {
        if !json {
            eprintln!("{shown}: dry run, not written");
        }
        return report;
    }
    let write = rig.document.write().and_then(|text| {
        std::fs::write(&out, text).map_err(|e| {
            Diagnostic::error(fynkel_document::Code::IO, format!("cannot write: {e}"))
                .with_document(shown.clone())
        })
    });
    match write {
        Ok(()) => {
            report["written"] = true.into();
            if !json {
                eprintln!(
                    "{shown}: {} ({} clip(s), skeleton {})",
                    if existing_created {
                        "created"
                    } else {
                        "updated"
                    },
                    clips.len(),
                    skeleton
                        .path
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default()
                );
            }
        }
        Err(d) => {
            diags.push(d);
            report["ok"] = false.into();
        }
    }
    report
}

/// The imported hierarchy as an authored document: a fresh id (the
/// `prefab` sub-asset keeps its own), the entity ids unchanged (they are
/// the prefab's template ids; instances get fresh ones), a description
/// that says where it came from.
fn authored_prefab(prefab: &serde_json::Value, source: &str) -> String {
    let mut doc = prefab.clone();
    doc["id"] = uuid::Uuid::new_v4().to_string().into();
    doc["description"] = format!(
        "Written by `fynkel import --prefab` from {source}; edit freely, re-run to start over."
    )
    .into();
    // Through the document writer, so the file reads like every other
    // authored document.
    let raw = serde_json::to_string(&doc).unwrap_or_default();
    Document::parse(fynkel_document::DocumentKind::Prefab, None, &raw)
        .ok()
        .and_then(|d| d.write().ok())
        .unwrap_or(raw)
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
