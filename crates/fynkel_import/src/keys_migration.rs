//! The project half of `meta/1-2/name-keys`
//! (`docs/design/stable-sub-asset-ids.md`, S2): moving a model's sidecar
//! from position keys to name keys needs the model file and every document
//! that points into it, so it cannot be a per-document step.
//!
//! [`migrate_documents`] reads the documents, imports each outdated model
//! sidecar's file **once** (with the old sidecar, so the import reports
//! `Imported::renames`), and migrates every document with a
//! [`Rewrite`] in front of its steps:
//!
//! - the model's sidecar: `subAssets` keys renamed, uuids kept, version 2;
//! - every `{ asset: <model>, sub: <old> }`: the new key;
//! - every id derived from a model node's position — override targets on
//!   an instance of the model's hierarchy (or of a rig's, which keeps its
//!   skeleton model's ids), and the ids that instance (and
//!   any prefab instancing a prefab holding it, however deep) derives —
//!   the id the name key derives.
//!
//! A model whose file cannot be imported is reported; its sidecar's own
//! step then refuses, and documents pointing into it keep their keys.
//!
//! The same pass carries the project half of `scene|prefab/4-5/one-root`
//! (ADR-0022, [`fynkel_document::one_root`]): an instance becomes its
//! prefab's root, which needs the prefab — or the hierarchy an imported
//! model derives, imported here — so it runs as a second rewrite when a
//! document's chain reaches version 4.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic};
use fynkel_document::asset::{source_path, Meta, FBX_IMPORTER, GLTF_IMPORTER};
use fynkel_document::json::{Node, Value};
use fynkel_document::migrate::Migrated;
use fynkel_document::one_root::{self, Plan};
use fynkel_document::prefab::{derive_id, Prefab};
use fynkel_document::{migrate_with, Document, DocumentKind, Migrations, Rewrite, SchemaSet};
use uuid::Uuid;

use crate::subkey::NAME_KEYS_FORMAT;
use crate::{import, Limits};

/// The rewrite's name in `applied[]`.
pub const REWRITE: &str = "meta/1-2/name-keys (project)";

/// One document's outcome.
pub struct Planned {
    pub path: PathBuf,
    pub migrated: Migrated,
}

/// What a model's migration renames.
#[derive(Default)]
struct Model {
    subs: HashMap<String, String>,
    ids: HashMap<Uuid, Uuid>,
}

/// Every document file under `dir` (scenes, prefabs, materials, sidecars,
/// the project file, …), hidden entries skipped, sorted.
pub fn document_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect(dir, &mut out)?;
    out.sort();
    Ok(out)
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

/// True for a sidecar at position keys whose importer names parts.
pub fn is_outdated_model_sidecar(doc: &Document) -> bool {
    doc.kind == DocumentKind::Meta
        && doc.envelope.format_version < NAME_KEYS_FORMAT
        && matches!(
            doc.root.get("importer").and_then(Node::as_str),
            Some(GLTF_IMPORTER | FBX_IMPORTER)
        )
}

/// Migrates `files` (see the module docs). Documents that fail to read are
/// reported and left out.
pub fn migrate_documents(
    files: &[PathBuf],
    migrations: &Migrations,
    schemas: &SchemaSet,
) -> (Vec<Planned>, Vec<Diagnostic>) {
    migrate_documents_with(files, &[], migrations, schemas)
}

/// [`migrate_documents`] with `context`: more documents of the same
/// project, read for what `files` point at (the prefabs and models an
/// instance names) but neither migrated nor reported. Migrating one scene
/// of a project needs them; paths already in `files` are skipped.
pub fn migrate_documents_with(
    files: &[PathBuf],
    context: &[PathBuf],
    migrations: &Migrations,
    schemas: &SchemaSet,
) -> (Vec<Planned>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let mut docs: Vec<(PathBuf, Document)> = Vec::new();
    for path in files {
        match Document::read(path, None) {
            Ok(d) => docs.push((path.clone(), d)),
            Err(d) => diags.extend(d),
        }
    }
    let migrating = docs.len();
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let known: HashSet<PathBuf> = files.iter().map(|p| canonical(p)).collect();
    for path in context {
        if known.contains(&canonical(path)) {
            continue;
        }
        if let Ok(d) = Document::read(path, None) {
            docs.push((path.clone(), d));
        }
    }

    // Each outdated model, imported once through its old sidecar.
    let mut models: HashMap<Uuid, Model> = HashMap::new();
    for (path, doc) in &docs {
        if !is_outdated_model_sidecar(doc) {
            continue;
        }
        let shown = path.display().to_string();
        let Some(source) = source_path(path).filter(|s| s.exists()) else {
            diags.push(
                Diagnostic::error(
                    Code::MIGRATION_FAILED,
                    "the model file next to this sidecar is missing; its sub-asset keys cannot be renamed",
                )
                .with_document(shown),
            );
            continue;
        };
        let meta = Meta::from_document(doc.clone());
        let imported = import(&source, &meta, &Limits::default());
        if imported.has_errors() {
            diags.push(
                Diagnostic::error(
                    Code::MIGRATION_FAILED,
                    format!(
                        "{} does not import, so its sub-asset keys cannot be renamed; the sidecar and the documents pointing into it are left as they are",
                        source.display()
                    ),
                )
                .with_document(shown),
            );
            diags.extend(
                imported
                    .diagnostics
                    .into_iter()
                    .filter(|d| d.severity == fynkel_contracts::Severity::Error),
            );
            continue;
        }
        models.insert(
            meta.id(),
            Model {
                subs: imported.renames.subs.into_iter().collect(),
                ids: imported.renames.ids.into_iter().collect(),
            },
        );
    }

    // Local id maps of the prefabs, through their instances (memoised).
    let prefabs: HashMap<Uuid, &Document> = docs
        .iter()
        .filter(|(_, d)| d.kind == DocumentKind::Prefab)
        .map(|(_, d)| (d.envelope.id, d))
        .collect();
    // A rig's `prefab` is its skeleton model's hierarchy, ids verbatim.
    let rigs: HashMap<Uuid, Uuid> = docs
        .iter()
        .filter(|(_, d)| d.kind == DocumentKind::Rig)
        .filter_map(|(_, d)| {
            let skeleton = d.root.get("skeleton")?.get("asset")?.as_str()?;
            Some((d.envelope.id, Uuid::parse_str(skeleton).ok()?))
        })
        .collect();
    let mut maps = Maps {
        models: &models,
        rigs: &rigs,
        prefabs: &prefabs,
        done: HashMap::new(),
        busy: HashSet::new(),
    };

    // The hierarchies imported models derive (one root each), for the
    // instances of `{ asset: <model or rig>, sub: "prefab" }`.
    let metas: HashMap<Uuid, &PathBuf> = docs
        .iter()
        .filter(|(_, d)| d.kind == DocumentKind::Meta)
        .map(|(p, d)| (d.envelope.id, p))
        .collect();
    let hierarchies: RefCell<HashMap<Uuid, Option<Prefab>>> = RefCell::new(HashMap::new());
    let imported = |asset: Uuid, sub: &str| -> Option<Prefab> {
        if sub != "prefab" {
            return None;
        }
        let model = *rigs.get(&asset).unwrap_or(&asset);
        if let Some(p) = hierarchies.borrow().get(&model) {
            return p.clone();
        }
        let prefab = metas.get(&model).and_then(|sidecar| {
            let source = source_path(sidecar)?;
            let meta = Meta::from_document(Document::read(sidecar, Some(DocumentKind::Meta)).ok()?);
            let imported = import(&source, &meta, &Limits::default());
            let prefab = imported.intermediate?.prefab?;
            let text = serde_json::to_string(&prefab).ok()?;
            Document::parse(DocumentKind::Prefab, None, &text)
                .ok()
                .map(Prefab::from_document)
        });
        hierarchies.borrow_mut().insert(model, prefab.clone());
        prefab
    };
    let plan = Plan::new(docs.iter().map(|(_, d)| d), &imported);
    let one_root = |d: &mut Document| plan.rewrite(d);
    let one_root = Rewrite {
        name: one_root::REWRITE.to_string(),
        description: "a prefab has one root and an instance is that root: several roots get a new one, an instance's own components become overrides of the root".to_string(),
        at: Some(4),
        apply: &one_root,
    };

    let mut out = Vec::new();
    for (path, doc) in &docs[..migrating] {
        // Ids to replace in this document: the instanced asset's own local
        // ids (override targets) and the ids each instance derives.
        let mut ids: HashMap<String, String> = HashMap::new();
        for (instance, asset) in instances(&doc.root) {
            let Some(local) = maps.of(&asset) else {
                continue;
            };
            for (old, new) in local.iter() {
                ids.insert(old.to_string(), new.to_string());
                ids.insert(
                    derive_id(instance, *old).to_string(),
                    derive_id(instance, *new).to_string(),
                );
            }
        }
        let own = (doc.kind == DocumentKind::Meta)
            .then(|| models.get(&doc.envelope.id))
            .flatten();
        let apply = |d: &mut Document| -> Result<bool, String> {
            let mut changed = false;
            if let Some(model) = own {
                if let Some(members) = d.root.get_mut("subAssets").and_then(Node::as_object_mut) {
                    for m in members.iter_mut() {
                        if let Some(new) = model.subs.get(&m.key) {
                            m.key = new.clone();
                        }
                    }
                }
                d.root
                    .set("formatVersion", Node::integer(NAME_KEYS_FORMAT as i64));
                d.envelope.format_version = NAME_KEYS_FORMAT;
                changed = true; // the version moves even when no key does
            }
            changed |= rewrite(&mut d.root, &models, &ids);
            Ok(changed)
        };
        let rewrite = Rewrite {
            name: REWRITE.to_string(),
            description: "sub-asset keys of the project's models are their parts' names; references and the ids derived from model nodes follow".to_string(),
            at: None,
            apply: &apply,
        };
        out.push(Planned {
            path: path.clone(),
            migrated: migrate_with(doc, migrations, schemas, &[&rewrite, &one_root]),
        });
    }
    (out, diags)
}

struct Maps<'a> {
    models: &'a HashMap<Uuid, Model>,
    rigs: &'a HashMap<Uuid, Uuid>,
    prefabs: &'a HashMap<Uuid, &'a Document>,
    done: HashMap<Uuid, HashMap<Uuid, Uuid>>,
    busy: HashSet<Uuid>,
}

impl Maps<'_> {
    /// Old → new local ids of what an instance's `asset` names: a model's
    /// or a rig's hierarchy (`sub: "prefab"`) or a prefab document (no
    /// `sub`).
    fn of(&mut self, asset: &(Uuid, String)) -> Option<HashMap<Uuid, Uuid>> {
        let (id, sub) = asset;
        if sub == "prefab" {
            let model = self.rigs.get(id).unwrap_or(id);
            return self.models.get(model).map(|m| m.ids.clone());
        }
        if !sub.is_empty() {
            return None;
        }
        if let Some(m) = self.done.get(id) {
            return Some(m.clone());
        }
        let doc = *self.prefabs.get(id)?;
        if !self.busy.insert(*id) {
            return None; // a cycle: D0402's business
        }
        let mut local = HashMap::new();
        for (instance, inner) in instances(&doc.root) {
            if let Some(inner) = self.of(&inner) {
                for (old, new) in inner {
                    local.insert(derive_id(instance, old), derive_id(instance, new));
                }
            }
        }
        self.busy.remove(id);
        self.done.insert(*id, local.clone());
        Some(local)
    }
}

/// `(instance id, (asset id, sub))` of every instance anchor in a scene or
/// prefab body.
fn instances(root: &Node) -> Vec<(Uuid, (Uuid, String))> {
    let Some(entities) = root.get("entities").and_then(Node::as_object) else {
        return Vec::new();
    };
    entities
        .iter()
        .filter_map(|e| {
            let p = e.value.get("prefab")?;
            let instance = Uuid::parse_str(p.get("instance")?.as_str()?).ok()?;
            let asset = p.get("asset")?;
            let id = Uuid::parse_str(asset.get("asset")?.as_str()?).ok()?;
            let sub = asset
                .get("sub")
                .and_then(Node::as_str)
                .unwrap_or("")
                .to_string();
            Some((instance, (id, sub)))
        })
        .collect()
}

/// Renames `sub`s of references into migrated models and replaces moved
/// ids anywhere in string values. True when something changed.
fn rewrite(node: &mut Node, models: &HashMap<Uuid, Model>, ids: &HashMap<String, String>) -> bool {
    let mut changed = false;
    match &mut node.value {
        Value::Object(members) => {
            let model = members
                .iter()
                .find(|m| m.key == "asset")
                .and_then(|m| m.value.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .and_then(|id| models.get(&id));
            if let Some(model) = model {
                if let Some(m) = members.iter_mut().find(|m| m.key == "sub") {
                    if let Value::String(sub) = &mut m.value.value {
                        if let Some(new) = model.subs.get(sub.as_str()) {
                            *sub = new.clone();
                            changed = true;
                        }
                    }
                }
            }
            for m in members.iter_mut() {
                changed |= rewrite(&mut m.value, models, ids);
            }
        }
        Value::Array(items) => {
            for item in items {
                changed |= rewrite(item, models, ids);
            }
        }
        Value::String(s) => {
            if let Some(new) = ids.get(s.as_str()) {
                *s = new.clone();
                changed = true;
            }
        }
        _ => {}
    }
    changed
}
