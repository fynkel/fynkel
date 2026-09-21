//! `fynkel rig inspect|validate <path> [--json]` (§14.1, blueprint §19.2d
//! item 19b): a rig document against its project's asset database — the
//! schema and reference rules (`FYN-D05xx`, `A000x`), then the skeleton
//! and every clip imported in memory and matched by bone name exactly as
//! the cook does (`fynkel_import::rig::retarget`): `inspect` prints the
//! skeleton's joints and each clip's source, length and matched/dropped
//! channels; `validate` prints the diagnostics. A bone the skeleton lacks
//! is `FYN-A0106`. Same exit codes and `--json` shape as `fynkel document`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::asset::{AssetDatabase, AssetEntry, Meta};
use fynkel_document::rig::Rig;
use fynkel_document::{
    diagnostics_json, has_errors, Diagnostic, Document, DocumentKind, Project, SchemaSet,
};
use fynkel_import::rig::{joint_names, retarget, Retargeted, SkeletonNames};
use fynkel_import::{Intermediate, Limits};
use uuid::Uuid;

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel rig inspect <path.rig.json> [--json] [--schemas <dir>]   the skeleton's joints and every clip matched to it\n  fynkel rig validate <path.rig.json>... [--json] [--schemas <dir>]  schema, references and the bone-name match (FYN-A0106)"
    );
    ExitCode::from(2)
}

struct Opts {
    paths: Vec<PathBuf>,
    json: bool,
    schemas_dir: Option<PathBuf>,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        paths: Vec::new(),
        json: false,
        schemas_dir: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--schemas" => {
                i += 1;
                o.schemas_dir = args.get(i).map(PathBuf::from);
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel rig: unknown argument {other}");
                return None;
            }
            p => o.paths.push(PathBuf::from(p)),
        }
        i += 1;
    }
    Some(o)
}

pub fn run(root: Option<&Path>, args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        return usage();
    };
    let Some(opts) = parse(&args[1..]) else {
        return usage();
    };
    if opts.paths.is_empty() || (sub == "inspect" && opts.paths.len() != 1) {
        return usage();
    }
    match sub.as_str() {
        "inspect" => inspect(root, &opts),
        "validate" => validate(root, &opts),
        _ => usage(),
    }
}

/// One clip of a rig as inspected: where it comes from and how it
/// matched.
pub struct ClipReport {
    pub name: String,
    pub source: String,
    /// `None` when the clip could not be read (reported).
    pub seconds: Option<f32>,
    pub channels: usize,
    /// Markers the rig authors for the clip plus the source's own (M3-19c).
    pub markers: usize,
    pub retargeted: Option<Retargeted>,
}

/// A rig inspected against in-memory imports.
pub struct RigReport {
    pub rig: Rig,
    pub skeleton_path: String,
    pub skeleton: SkeletonNames,
    /// The skeleton's root joint names (distinct across its skins).
    pub roots: Vec<String>,
    pub entities: usize,
    pub clips: Vec<ClipReport>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Imports of the sources a rig names, once each.
struct Sources<'a> {
    db: &'a AssetDatabase,
    limits: Limits,
    cache: HashMap<Uuid, Option<Intermediate>>,
}

impl Sources<'_> {
    fn get(&mut self, entry: &AssetEntry, diags: &mut Vec<Diagnostic>) -> Option<&Intermediate> {
        if !self.cache.contains_key(&entry.id) {
            let imported = (|| {
                let sidecar = entry.sidecar(self.db.root())?;
                let meta = match Document::read(&sidecar, Some(DocumentKind::Meta)) {
                    Ok(doc) => Meta::from_document(doc),
                    Err(d) => {
                        diags.extend(d);
                        return None;
                    }
                };
                let mut imported =
                    fynkel_import::import(&entry.file(self.db.root()), &meta, &self.limits);
                let bad = imported.has_errors();
                diags.append(&mut imported.diagnostics);
                if bad {
                    None
                } else {
                    imported.intermediate
                }
            })();
            self.cache.insert(entry.id, imported);
        }
        self.cache[&entry.id].as_ref()
    }
}

/// Reads, validates and matches one rig; `Err` when it cannot be read
/// or its references do not hold (nothing to match then).
pub fn inspect_rig(
    path: &Path,
    schemas: &SchemaSet,
    db: &AssetDatabase,
) -> Result<RigReport, Vec<Diagnostic>> {
    let rig = Rig::from_document(Document::read(path, Some(DocumentKind::Rig))?);
    let mut diagnostics = rig.validate(schemas);
    diagnostics.extend(rig.validate_references(db));
    diagnostics.extend(db.check_references(&rig.document));
    if has_errors(&diagnostics) {
        return Err(diagnostics);
    }
    let mut sources = Sources {
        db,
        limits: Limits::default(),
        cache: HashMap::new(),
    };
    let skeleton_ref = rig.skeleton.clone().expect("validated");
    let skeleton_entry = db.resolve(&skeleton_ref).expect("validated").entry;
    let (skeleton, roots, entities) = match sources.get(skeleton_entry, &mut diagnostics) {
        Some(im) => (
            SkeletonNames::from_intermediate(im),
            {
                // One name per distinct root: several skins may share it.
                let mut roots: Vec<String> = Vec::new();
                for skin in &im.skins {
                    if !roots.contains(&skin.payload.skeleton) {
                        roots.push(skin.payload.skeleton.clone());
                    }
                }
                roots
            },
            im.prefab_entity_count(),
        ),
        None => return Err(diagnostics),
    };
    let mut clips = Vec::new();
    for clip in &rig.clips {
        let entry = db.resolve(&clip.asset).expect("validated").entry;
        let source = format!("{}#{}", entry.path, clip.asset.sub);
        let mut report = ClipReport {
            name: clip.name.clone(),
            source,
            seconds: None,
            channels: 0,
            markers: rig.markers_of(&clip.name).len(),
            retargeted: None,
        };
        if let Some(im) = sources.get(entry, &mut diagnostics) {
            match im.animations.iter().find(|a| a.sub_id == clip.asset.sub) {
                Some(a) => {
                    report.seconds = Some(a.payload.duration);
                    report.channels = a.payload.channels.len();
                    report.markers += a.payload.markers.len();
                    let (retargeted, mut diags) =
                        retarget(&clip.name, &a.payload, joint_names(im).as_ref(), &skeleton);
                    for d in &mut diags {
                        d.document = rig.document.path.clone();
                        d.pointer = Some(clip.pointer.clone());
                        if let Some(span) = rig.document.root.pointer_span(&clip.pointer) {
                            d.line = Some(span.start.line);
                            d.column = Some(span.start.column);
                        }
                    }
                    diagnostics.extend(diags);
                    report.retargeted = Some(retargeted);
                }
                None => diagnostics.push(
                    Diagnostic::error(
                        fynkel_document::Code::RIG_CLIP,
                        format!(
                            "clip {:?}: {} imports without a clip {}",
                            clip.name, entry.path, clip.asset.sub
                        ),
                    )
                    .with_document(rig.document.path.clone().unwrap_or_default())
                    .with_pointer(clip.pointer.clone())
                    .with_suggestion("the sidecar lists a sub-asset the import no longer produces; run `fynkel import` on the file"),
                ),
            }
        }
        clips.push(report);
    }
    Ok(RigReport {
        rig,
        skeleton_path: skeleton_entry.path.clone(),
        skeleton,
        roots,
        entities,
        clips,
        diagnostics,
    })
}

pub fn report_json(r: &RigReport) -> serde_json::Value {
    serde_json::json!({
        "ok": !has_errors(&r.diagnostics),
        "id": r.rig.id().to_string(),
        "name": r.rig.name(),
        "path": r.rig.document.path,
        "skeleton": {
            "asset": r.rig.skeleton.as_ref().map(|s| s.asset.to_string()),
            "path": r.skeleton_path,
            "joints": r.skeleton.joints.iter().collect::<Vec<_>>(),
            "roots": r.roots,
            "entities": r.entities,
        },
        "clips": r.clips.iter().map(|c| serde_json::json!({
            "name": c.name,
            "sub": Rig::clip_sub(&c.name),
            "source": c.source,
            "seconds": c.seconds,
            "channels": c.channels,
            "markers": c.markers,
            "matched": c.retargeted.as_ref().map(|t| t.matched),
            "dropped": c.retargeted.as_ref().map(|t| t.dropped.clone()).unwrap_or_default(),
            "missing": c.retargeted.as_ref().map(|t| t.missing.clone()).unwrap_or_default(),
        })).collect::<Vec<_>>(),
        "diagnostics": diagnostics_json(&r.diagnostics)["diagnostics"],
    })
}

pub fn report_text(r: &RigReport) -> String {
    let mut s = format!(
        "{} ({})\n  skeleton {}: {} joint(s){}, {} entities\n",
        r.rig.name(),
        r.rig.document.path.as_deref().unwrap_or("?"),
        r.skeleton_path,
        r.skeleton.joints.len(),
        if r.roots.is_empty() {
            String::new()
        } else {
            format!(" (root {})", r.roots.join(", "))
        },
        r.entities
    );
    s.push_str(&format!("  clips ({})\n", r.clips.len()));
    let width = r
        .clips
        .iter()
        .map(|c| c.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    for c in &r.clips {
        let (seconds, matched) = match (&c.seconds, &c.retargeted) {
            (Some(sec), Some(t)) => (
                format!("{sec:.2} s"),
                format!(
                    "{}/{} channel(s){}{}",
                    t.matched,
                    c.channels,
                    if t.dropped.is_empty() {
                        String::new()
                    } else {
                        format!(", {} dropped", t.dropped.len())
                    },
                    if t.missing.is_empty() {
                        String::new()
                    } else {
                        format!(", {} bone(s) missing", t.missing.len())
                    }
                ),
            ),
            _ => ("?".to_string(), "not imported".to_string()),
        };
        s.push_str(&format!(
            "    {:<width$}  {:<40}  {:>8}  {}{}\n",
            c.name,
            c.source,
            seconds,
            matched,
            if c.markers == 0 {
                String::new()
            } else {
                format!(", {} marker(s)", c.markers)
            }
        ));
    }
    s
}

/// The project a rig lies in, its schemas and asset database.
fn open(
    root: Option<&Path>,
    opts: &Opts,
    path: &Path,
) -> Result<(SchemaSet, AssetDatabase), Vec<Diagnostic>> {
    let schemas =
        crate::schemas::load_for(root, opts.schemas_dir.as_deref(), path).map_err(|d| vec![d])?;
    let Some(dir) = Project::dir_of(path) else {
        return Err(vec![Diagnostic::error(
            fynkel_document::Code::PROJECT_MISSING,
            "the rig is not inside a project (no fynkel.project.json above it)",
        )
        .with_document(path.display().to_string())
        .with_suggestion(
            "a rig names assets of its project's database; move it under the project's assets/",
        )]);
    };
    let project = Project::read(&dir)?;
    let (db, scan) = AssetDatabase::scan(&project.assets_dir(&dir), &schemas);
    if has_errors(&scan) {
        return Err(scan);
    }
    Ok((schemas, db))
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

fn inspect(root: Option<&Path>, opts: &Opts) -> ExitCode {
    let path = &opts.paths[0];
    let (schemas, db) = match open(root, opts, path) {
        Ok(x) => x,
        Err(d) => return report(&d, opts.json),
    };
    let r = match inspect_rig(path, &schemas, &db) {
        Ok(r) => r,
        Err(d) => return report(&d, opts.json),
    };
    if opts.json {
        println!("{}", report_json(&r));
    } else {
        print!("{}", report_text(&r));
        for d in &r.diagnostics {
            eprintln!("{d}");
        }
    }
    exit(&r.diagnostics)
}

fn validate(root: Option<&Path>, opts: &Opts) -> ExitCode {
    let mut diags = Vec::new();
    let mut ok = 0;
    for path in &opts.paths {
        let (schemas, db) = match open(root, opts, path) {
            Ok(x) => x,
            Err(d) => {
                diags.extend(d);
                continue;
            }
        };
        match inspect_rig(path, &schemas, &db) {
            Ok(r) => {
                if !has_errors(&r.diagnostics) {
                    ok += 1;
                }
                diags.extend(r.diagnostics);
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
        eprintln!("fynkel rig validate: {ok}/{} rig(s) ok", opts.paths.len());
    }
    exit(&diags)
}
