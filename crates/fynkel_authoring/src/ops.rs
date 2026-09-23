//! Asset operations (blueprint §19.2e item 5): the session's own mutations
//! of the asset tree, next to the per-document transactions — a new scene,
//! prefab or material written as `Document::new`; rename and move with
//! the sidecar carried alongside so the id survives; delete, refused while
//! the asset is referenced unless forced, and then a move into
//! `.fynkel/trash/` so it can come back; the sidecar's import settings;
//! reimport. Every one rescans only the paths it touched, and the ones
//! that change the tree go on the session's **asset history**, an
//! undo/redo stack of its own (a document's ⌘Z never moves a file).
//!
//! Entry point: [`AuthoringSession::operate`] — `operate(kind, args)` →
//! JSON, the mutating twin of `query`. Refusals are `FYN-T0003`
//! (`SESSION_INVALID_REQUEST`) with the reason, `FYN-T0006` for I/O,
//! `FYN-E0007` when the project is read-only, `FYN-E0009` when there is
//! nothing to undo.

use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use fynkel_document::asset::{entry_json, sidecar_path, AssetEntry, AssetKind, Meta};
use fynkel_document::{diagnostic_json, Document, DocumentKind, Material, Node};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::project::to_contract;
use crate::{AuthoringSession, OpenProject};

/// The operation kinds `operate` applies.
pub const OPERATIONS: &[&str] = &[
    "newDocument",
    "renameAsset",
    "deleteAsset",
    "setImportSettings",
    "extractMaterials",
    "reimport",
    "undoAsset",
    "redoAsset",
    "setLayersAndTags",
];

/// Where deleted assets go, under the project directory (git-ignored by
/// the template like the rest of `.fynkel/`): `<trash>/<asset id>/<file>`.
pub const TRASH_DIR: &str = ".fynkel/trash";

/// One committed asset operation, replayable both ways.
#[derive(Clone, Debug, PartialEq)]
pub enum AssetOp {
    /// A document was written at `path` with exactly `text`.
    Created {
        path: PathBuf,
        id: Uuid,
        text: String,
    },
    /// `from` (and its sidecar when `sidecar`) moved to `to`.
    Moved {
        from: PathBuf,
        to: PathBuf,
        id: Uuid,
        sidecar: bool,
    },
    /// `path` (and its sidecar) moved into `trash`.
    Deleted {
        path: PathBuf,
        trash: PathBuf,
        id: Uuid,
        sidecar: bool,
    },
    /// The sidecar at `sidecar` had its `settings` replaced.
    Settings {
        sidecar: PathBuf,
        id: Uuid,
        before: Value,
        after: Value,
    },
    /// Files rewritten in place (`setLayersAndTags`: the project file and
    /// the scenes and prefabs whose layers or tags moved).
    Rewrote { files: Vec<FileRewrite> },
    /// Several operations as one step (`extractMaterials`: the new
    /// materials, then the model's remaps), undone in reverse; `id` is
    /// the asset the step is about.
    Group { ops: Vec<AssetOp>, id: Uuid },
}

/// One file's text before and after an operation rewrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct FileRewrite {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

/// A history entry: the operation and what the Edit menu calls it.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub op: AssetOp,
    pub label: String,
}

/// The session's asset history.
#[derive(Default, Debug)]
pub struct AssetHistory {
    undo: Vec<Done>,
    redo: Vec<Done>,
}

impl AssetHistory {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|d| d.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|d| d.label.as_str())
    }

    /// `{ canUndo, canRedo, undoLabel, redoLabel, undo: [labels], redo: [labels] }`.
    pub fn json(&self) -> Value {
        json!({
            "canUndo": self.can_undo(),
            "canRedo": self.can_redo(),
            "undoLabel": self.undo_label(),
            "redoLabel": self.redo_label(),
            "undo": self.undo.iter().map(|d| d.label.clone()).collect::<Vec<_>>(),
            "redo": self.redo.iter().map(|d| d.label.clone()).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn push(&mut self, done: Done) {
        self.undo.push(done);
        self.redo.clear();
    }
}

fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::SESSION_INVALID_REQUEST, message)
}

pub(crate) fn io(path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::SESSION_IO, message).with_document(path.display().to_string())
}

fn string_arg<'a>(args: &'a Value, key: &str, op: &str) -> Result<&'a str, Diagnostic> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("{op}: \"{key}\" (a string) is required")))
}

fn uuid_arg(args: &Value, key: &str, op: &str) -> Result<Uuid, Diagnostic> {
    let text = string_arg(args, key, op)?;
    text.parse::<Uuid>()
        .map_err(|e| invalid(format!("{op}: \"{key}\": {e}")))
}

/// The file name of a path, for labels.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A document's name from its file name: `props/coin.prefab.json` → `coin`.
fn stem(path: &Path, kind: DocumentKind) -> String {
    let name = file_name(path);
    name.strip_suffix(kind.suffix())
        .unwrap_or(&name)
        .to_string()
}

impl OpenProject {
    /// Resolves an asset-root-relative or absolute path to an absolute one
    /// inside the asset root; a path elsewhere is refused.
    fn inside_assets(&self, path: &str, op: &str) -> Result<PathBuf, Diagnostic> {
        let root = self.assets_dir();
        let root = root.canonicalize().unwrap_or(root);
        let p = Path::new(path);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        };
        let abs = normalize(&abs);
        if !abs.starts_with(&root) {
            return Err(invalid(format!(
                "{op}: {} is outside the asset root {}",
                abs.display(),
                root.display()
            )));
        }
        if abs == root {
            return Err(invalid(format!(
                "{op}: the path names the asset root itself"
            )));
        }
        Ok(abs)
    }

    /// The entry for `id`, or `FYN-A0004`.
    fn entry(&self, id: Uuid) -> Result<&AssetEntry, Diagnostic> {
        self.assets
            .resolve(&AssetRef::new(to_contract(id)))
            .map(|r| r.entry)
    }

    /// The dependents of `id` as `related` diagnostics, one per reference.
    fn dependents_related(&self, id: Uuid) -> Vec<Diagnostic> {
        self.dependents(id)
            .iter()
            .map(|r| {
                let mut d = Diagnostic::error(
                    Code::ASSET_UNRESOLVED,
                    format!(
                        "{} references {}{}",
                        r.path,
                        r.asset.asset,
                        if r.asset.sub.is_empty() {
                            String::new()
                        } else {
                            format!("#{}", r.asset.sub)
                        }
                    ),
                )
                .with_document(self.assets_dir().join(&r.path).display().to_string())
                .with_pointer(&r.pointer);
                d.id = Some(r.document.to_string());
                d
            })
            .collect()
    }
}

/// `a/b/../c` → `a/c`, without touching the file system (the target of a
/// rename does not exist yet).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl AuthoringSession {
    /// Applies one asset operation by name — `newDocument`, `renameAsset`,
    /// `deleteAsset`, `setImportSettings`, `reimport`, `undoAsset`,
    /// `redoAsset` — with its arguments; see the module docs. The result
    /// carries `ok`, `op`, the `label` the history shows, the affected
    /// `entry` (when it still exists), `canUndo`/`canRedo` and any
    /// `diagnostics` that are warnings. `Err` when the operation is refused;
    /// nothing was touched then (a rename whose second half failed is
    /// undone before it reports).
    pub fn operate(&mut self, kind: &str, args: &Value) -> Result<Value, Vec<Diagnostic>> {
        let one = |d: Diagnostic| vec![d];
        self.require_project().map_err(one)?;
        if let Some(reason) = &self.project().unwrap().read_only {
            return Err(one(Diagnostic::error(
                Code::EDIT_NOT_EDITABLE,
                format!("the project is read-only: {reason}"),
            )
            .with_suggestion(
                "open the project with the Fynkel that wrote its lock",
            )));
        }
        match kind {
            "newDocument" => self.new_document(args).map_err(one),
            "renameAsset" => self.rename_asset(args).map_err(one),
            "deleteAsset" => self.delete_asset(args),
            "setImportSettings" => self.set_import_settings(args),
            "extractMaterials" => self.extract_materials(args),
            "reimport" => self.reimport(args).map_err(one),
            "undoAsset" => self.undo_asset().map_err(one),
            "redoAsset" => self.redo_asset().map_err(one),
            crate::layers::OP => self.set_layers_and_tags(args).map_err(one),
            other => Err(one(Diagnostic::error(
                Code::SESSION_UNKNOWN_QUERY,
                format!("unknown operation {other:?}"),
            )
            .with_suggestion(format!("one of: {}", OPERATIONS.join(", "))))),
        }
    }

    /// The asset history (`assetHistory` query).
    pub fn asset_history(&self) -> &AssetHistory {
        &self.assets_history
    }

    /// The result of an operation; `reload` lists the open documents it
    /// rewrote on disk (the client re-reads them).
    pub(crate) fn finish(
        &mut self,
        op: &str,
        label: &str,
        id: Option<Uuid>,
        mut extra: Value,
    ) -> Value {
        let project = self.project().expect("operate needs a project");
        let entry = id
            .and_then(|id| project.assets.get(id))
            .map(entry_json)
            .unwrap_or(Value::Null);
        let mut v = json!({
            "ok": true,
            "op": op,
            "label": label,
            "entry": entry,
            "assetCount": project.assets.len(),
            "canUndo": self.assets_history.can_undo(),
            "canRedo": self.assets_history.can_redo(),
            "undoLabel": self.assets_history.undo_label(),
            "redoLabel": self.assets_history.redo_label(),
            "reload": std::mem::take(&mut self.reload_after),
        });
        if let (Some(v), Some(e)) = (v.as_object_mut(), extra.as_object_mut()) {
            v.append(e);
        }
        v
    }

    // ------------------------------------------------------------ new

    /// `newDocument { kind: scene|prefab|material, path, name?, shader? }`:
    /// `path` is relative to the asset root (the kind's suffix is added
    /// when missing); `name` defaults to the file's stem; a material may
    /// name a `shader` (the built-in PBR by default). Refused when the
    /// file or a file of another kind's name exists.
    fn new_document(&mut self, args: &Value) -> Result<Value, Diagnostic> {
        const OP: &str = "newDocument";
        let kind_name = string_arg(args, "kind", OP)?;
        let kind = match DocumentKind::parse_name(kind_name) {
            Some(k @ (DocumentKind::Scene | DocumentKind::Prefab | DocumentKind::Material)) => k,
            _ => {
                return Err(invalid(format!(
                    "{OP}: kind must be \"scene\", \"prefab\" or \"material\", not {kind_name:?}"
                )))
            }
        };
        let given = string_arg(args, "path", OP)?;
        let project = self.project().unwrap();
        let mut path = project.inside_assets(given, OP)?;
        match DocumentKind::from_path(&path) {
            Some(k) if k == kind => {}
            Some(other) => {
                return Err(invalid(format!(
                    "{OP}: {} names a {} document; a {} ends with {}",
                    file_name(&path),
                    other.name(),
                    kind.name(),
                    kind.suffix()
                )))
            }
            None => {
                let name = format!("{}{}", file_name(&path), kind.suffix());
                path.set_file_name(name);
            }
        }
        if path.exists() {
            return Err(invalid(format!(
                "{OP}: {} already exists",
                project.relative_to_assets(&path).unwrap_or_default()
            ))
            .with_document(path.display().to_string()));
        }
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| stem(&path, kind));
        let mut doc = Document::new(kind, &name);
        if kind == DocumentKind::Material {
            if let Some(shader) = args.get("shader").and_then(Value::as_str) {
                doc.root.set("shader", Node::string(shader));
                if shader != fynkel_document::material::PBR_SHADER_ID {
                    fynkel_document::material::strip_pbr_fields(&mut doc.root);
                }
            }
            let material = Material::from_document(doc.clone());
            let diags = material.validate_semantics(&project.shaders());
            if fynkel_contracts::has_errors(&diags) {
                let mut d = invalid(format!("{OP}: the material would not validate"));
                for e in diags {
                    d = d.with_related(e);
                }
                return Err(d);
            }
        }
        doc.path = Some(path.display().to_string());
        let text = doc.write()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| io(parent, format!("cannot create the folder: {e}")))?;
        }
        std::fs::write(&path, &text).map_err(|e| io(&path, format!("cannot write: {e}")))?;
        let path = path.canonicalize().unwrap_or(path);
        let id = doc.envelope.id;
        let label = format!("New {} {}", kind.name(), file_name(&path));
        self.project_mut()
            .unwrap()
            .rescan(std::slice::from_ref(&path));
        self.assets_history.push(Done {
            op: AssetOp::Created {
                path: path.clone(),
                id,
                text,
            },
            label: label.clone(),
        });
        Ok(self.finish(
            OP,
            &label,
            Some(id),
            json!({ "path": path.display().to_string(), "id": id.to_string() }),
        ))
    }

    // --------------------------------------------------------- rename

    /// `renameAsset { asset, path }`: moves the file to `path` (relative to
    /// the asset root; an existing folder, or a path ending in `/`, keeps
    /// the file's name) with its sidecar, so the id and every reference
    /// survive. The kind is kept — a document keeps its suffix, a source
    /// its extension (the sidecar names the importer). Refused when the
    /// target (or its sidecar) exists. An open document follows to the
    /// new path with its history.
    fn rename_asset(&mut self, args: &Value) -> Result<Value, Diagnostic> {
        const OP: &str = "renameAsset";
        let id = uuid_arg(args, "asset", OP)?;
        let target = string_arg(args, "path", OP)?;
        let project = self.project().unwrap();
        let entry = project.entry(id)?.clone();
        let root = project.assets.root().to_path_buf();
        let from = entry.file(&root);
        let from = from.canonicalize().unwrap_or(from);
        let mut to = project.inside_assets(target, OP)?;
        if target.ends_with('/') || to.is_dir() {
            to = to.join(file_name(&from));
        }
        if to == from {
            return Err(invalid(format!(
                "{OP}: {} is where the asset already is",
                entry.path
            )));
        }
        let is_source = matches!(entry.kind, AssetKind::Source { .. });
        match &entry.kind {
            AssetKind::Document(kind) => {
                if DocumentKind::from_path(&to) != Some(*kind) {
                    return Err(invalid(format!(
                        "{OP}: a {} document keeps its {} suffix; {} does not end with it",
                        kind.name(),
                        kind.suffix(),
                        file_name(&to)
                    )));
                }
            }
            AssetKind::Source { importer, .. } => {
                let ext = |p: &Path| {
                    p.extension()
                        .map(|e| e.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default()
                };
                if ext(&to) != ext(&from) {
                    return Err(invalid(format!(
                        "{OP}: keep the .{} extension — the sidecar names importer {importer}",
                        ext(&from)
                    )));
                }
            }
        }
        let label = if to.parent() == from.parent() {
            format!("Rename {} → {}", file_name(&from), file_name(&to))
        } else {
            format!(
                "Move {} → {}/",
                file_name(&from),
                project
                    .relative_to_assets(to.parent().unwrap_or(&root))
                    .unwrap_or_default()
            )
        };
        self.move_asset(&from, &to, is_source, OP)?;
        if let Some((_, doc)) = self.documents.iter_mut().find(|(_, d)| d.path == from) {
            doc.path = to.clone();
            doc.editor.relocate(&to);
        }
        self.project_mut()
            .unwrap()
            .rescan(&[from.clone(), to.clone()]);
        self.assets_history.push(Done {
            op: AssetOp::Moved {
                from: from.clone(),
                to: to.clone(),
                id,
                sidecar: is_source,
            },
            label: label.clone(),
        });
        Ok(self.finish(
            OP,
            &label,
            Some(id),
            json!({ "from": from.display().to_string(), "to": to.display().to_string() }),
        ))
    }

    /// Moves `from` to `to` and, for a source, the sidecar alongside; the
    /// first half is undone when the second fails. Refuses to overwrite.
    fn move_asset(
        &self,
        from: &Path,
        to: &Path,
        sidecar: bool,
        op: &str,
    ) -> Result<(), Diagnostic> {
        if to.exists() {
            return Err(invalid(format!("{op}: {} already exists", to.display()))
                .with_document(to.display().to_string()));
        }
        if sidecar && sidecar_path(to).exists() {
            return Err(invalid(format!(
                "{op}: {} already exists",
                sidecar_path(to).display()
            ))
            .with_document(sidecar_path(to).display().to_string()));
        }
        if !from.exists() {
            return Err(io(from, format!("{op}: the file is gone")));
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| io(parent, format!("{op}: cannot create the folder: {e}")))?;
        }
        std::fs::rename(from, to).map_err(|e| io(from, format!("{op}: cannot move: {e}")))?;
        if sidecar {
            let (sf, st) = (sidecar_path(from), sidecar_path(to));
            if sf.exists() {
                if let Err(e) = std::fs::rename(&sf, &st) {
                    let _ = std::fs::rename(to, from);
                    return Err(io(&sf, format!("{op}: cannot move the sidecar: {e}")));
                }
            }
        }
        Ok(())
    }

    // --------------------------------------------------------- delete

    /// `deleteAsset { asset, force? }`: refused with every dependent listed
    /// while a document references the asset (or one of its sub-assets),
    /// unless `force`; refused while the asset is open as a document. The
    /// file and its sidecar move to `.fynkel/trash/<id>/` — undo brings
    /// them back. Warnings (the dependents of a forced delete) are in the
    /// result's `diagnostics`.
    fn delete_asset(&mut self, args: &Value) -> Result<Value, Vec<Diagnostic>> {
        const OP: &str = "deleteAsset";
        let one = |d: Diagnostic| vec![d];
        let id = uuid_arg(args, "asset", OP).map_err(one)?;
        let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
        let project = self.project().unwrap();
        let entry = project.entry(id).map_err(one)?.clone();
        let root = project.assets.root().to_path_buf();
        let from = entry.file(&root);
        let from = from.canonicalize().unwrap_or(from);
        let related = project.dependents_related(id);
        if !related.is_empty() && !force {
            let mut d = invalid(format!(
                "{OP}: {} is referenced by {} document field{}",
                entry.path,
                related.len(),
                if related.len() == 1 { "" } else { "s" }
            ))
            .with_document(from.display().to_string())
            .with_suggestion(
                "clear the references first, or delete with force to leave them dangling",
            );
            for r in related {
                d = d.with_related(r);
            }
            return Err(one(d));
        }
        if self.documents.values().any(|d| d.path == from) {
            return Err(one(invalid(format!(
                "{OP}: {} is open as a document",
                entry.path
            ))
            .with_document(from.display().to_string())
            .with_suggestion("close its tab first")));
        }
        let is_source = matches!(entry.kind, AssetKind::Source { .. });
        let trash_dir = project.dir.join(TRASH_DIR).join(id.to_string());
        let trash = trash_dir.join(file_name(&from));
        if trash.exists() {
            // A second delete of the same id (after an undo and a new copy): keep both.
            let _ = std::fs::remove_dir_all(&trash_dir);
        }
        self.move_asset(&from, &trash, is_source, OP).map_err(one)?;
        let label = format!("Delete {}", file_name(&from));
        self.project_mut()
            .unwrap()
            .rescan(std::slice::from_ref(&from));
        self.assets_history.push(Done {
            op: AssetOp::Deleted {
                path: from.clone(),
                trash: trash.clone(),
                id,
                sidecar: is_source,
            },
            label: label.clone(),
        });
        let warnings: Vec<Value> = related
            .into_iter()
            .map(|d| {
                diagnostic_json(
                    &Diagnostic::warning(d.code, d.message.clone())
                        .with_document(d.document.clone().unwrap_or_default()),
                )
            })
            .collect();
        Ok(self.finish(
            OP,
            &label,
            None,
            json!({
                "path": from.display().to_string(),
                "trash": trash.display().to_string(),
                "id": id.to_string(),
                "diagnostics": warnings,
            }),
        ))
    }

    // ------------------------------------------------------- settings

    /// `setImportSettings { asset, settings }`: replaces the sidecar's
    /// `settings` object (validated against the sidecar schema; an
    /// importer's own rules — `colorSpace` must be `srgb` or `linear` —
    /// are checked by `reimport`). The sidecar is rewritten canonically.
    fn set_import_settings(&mut self, args: &Value) -> Result<Value, Vec<Diagnostic>> {
        const OP: &str = "setImportSettings";
        let one = |d: Diagnostic| vec![d];
        let id = uuid_arg(args, "asset", OP).map_err(one)?;
        let settings = match args.get("settings") {
            Some(v @ Value::Object(_)) => v.clone(),
            _ => {
                return Err(one(invalid(format!(
                    "{OP}: \"settings\" must be a JSON object"
                ))))
            }
        };
        let (sidecar, mut meta) = self.sidecar_of(id, OP)?;
        let before = meta.settings.to_serde();
        meta.settings = Node::from_serde(&settings);
        let doc = meta.to_document();
        let checked = Meta::from_document(doc.clone());
        let project = self.project().unwrap();
        let mut diags = checked.validate(&project.schemas);
        diags.extend(checked.validate_remaps(&project.assets));
        diags.extend(project.assets.check_references(&doc));
        if fynkel_contracts::has_errors(&diags) {
            let mut d = invalid(format!("{OP}: the sidecar would not validate"))
                .with_document(sidecar.display().to_string());
            for e in diags {
                d = d.with_related(e);
            }
            return Err(one(d));
        }
        let text = doc.write().map_err(one)?;
        std::fs::write(&sidecar, text)
            .map_err(|e| one(io(&sidecar, format!("{OP}: cannot write the sidecar: {e}"))))?;
        let label = format!(
            "Import settings of {}",
            file_name(&fynkel_document::asset::source_path(&sidecar).unwrap_or_default())
        );
        self.project_mut()
            .unwrap()
            .rescan(std::slice::from_ref(&sidecar));
        self.assets_history.push(Done {
            op: AssetOp::Settings {
                sidecar: sidecar.clone(),
                id,
                before,
                after: settings.clone(),
            },
            label: label.clone(),
        });
        Ok(self.finish(
            OP,
            &label,
            Some(id),
            json!({ "sidecar": sidecar.display().to_string(), "settings": settings }),
        ))
    }

    // ---------------------------------------------- extract materials

    /// `extractMaterials { asset, folder? }`: writes each embedded material
    /// of the model `asset` that is not remapped yet as an editable
    /// `<name>.material.json` (a fresh id, the imported values, its
    /// textures still the model's images) into `folder` (relative to the
    /// asset root; `Materials/` next to the model by default), and remaps
    /// the model to them — one step on the asset history
    /// (`docs/design/material-remapping.md` §5). Refused when the model
    /// does not import or every material is remapped already.
    fn extract_materials(&mut self, args: &Value) -> Result<Value, Vec<Diagnostic>> {
        const OP: &str = "extractMaterials";
        let one = |d: Diagnostic| vec![d];
        let id = uuid_arg(args, "asset", OP).map_err(one)?;
        let (sidecar, mut meta) = self.sidecar_of(id, OP)?;
        let project = self.project().unwrap();
        let entry = project.entry(id).map_err(one)?;
        if !fynkel_document::rig::is_model(&entry.kind) {
            return Err(one(invalid(format!(
                "{OP}: {} is not an imported model (glTF, FBX)",
                entry.path
            ))));
        }
        let Some(source) = fynkel_document::asset::source_path(&sidecar) else {
            return Err(one(io(&sidecar, format!("{OP}: not a sidecar name"))));
        };
        let folder = match args.get("folder").and_then(Value::as_str) {
            Some(f) if !f.is_empty() => project.inside_assets(f, OP).map_err(one)?,
            _ => fynkel_import::extract::default_folder(&source),
        };
        let extracted = fynkel_import::extract::extract_materials(
            &source,
            &meta,
            &folder,
            &fynkel_import::Limits::default(),
        )
        .map_err(|diags| {
            let mut d = invalid(format!("{OP}: {} does not import", file_name(&source)));
            for e in diags
                .into_iter()
                .filter(|d| d.severity == fynkel_contracts::Severity::Error)
            {
                d = d.with_related(e);
            }
            vec![d]
        })?;
        if extracted.is_empty() {
            return Err(one(invalid(format!(
                "{OP}: every embedded material of {} is remapped already",
                file_name(&source)
            ))
            .with_suggestion("clear a remap to extract that material again")));
        }
        std::fs::create_dir_all(&folder)
            .map_err(|e| one(io(&folder, format!("{OP}: cannot create the folder: {e}"))))?;
        let mut ops = Vec::new();
        let mut written: Vec<PathBuf> = Vec::new();
        for e in &extracted {
            if let Err(err) = std::fs::write(&e.path, &e.text) {
                for w in &written {
                    let _ = std::fs::remove_file(w);
                }
                return Err(one(io(&e.path, format!("{OP}: cannot write: {err}"))));
            }
            let path = e.path.canonicalize().unwrap_or(e.path.clone());
            written.push(path.clone());
            ops.push(AssetOp::Created {
                path,
                id: e.id,
                text: e.text.clone(),
            });
        }
        let before = meta.settings.to_serde();
        for e in &extracted {
            meta.set_material_remap(&e.key, Some(e.id));
        }
        let after = meta.settings.to_serde();
        let text = meta.to_document().write().map_err(one)?;
        if let Err(err) = std::fs::write(&sidecar, text) {
            for w in &written {
                let _ = std::fs::remove_file(w);
            }
            return Err(one(io(
                &sidecar,
                format!("{OP}: cannot write the sidecar: {err}"),
            )));
        }
        ops.push(AssetOp::Settings {
            sidecar: sidecar.clone(),
            id,
            before,
            after: after.clone(),
        });
        let mut touched = written.clone();
        touched.push(sidecar.clone());
        self.project_mut().unwrap().rescan(&touched);
        let label = format!("Extract Materials of {}", file_name(&source));
        self.assets_history.push(Done {
            op: AssetOp::Group { ops, id },
            label: label.clone(),
        });
        let materials: Vec<Value> = extracted
            .iter()
            .zip(&written)
            .map(|(e, path)| {
                json!({ "key": e.key, "id": e.id.to_string(), "path": path.display().to_string() })
            })
            .collect();
        Ok(self.finish(
            OP,
            &label,
            Some(id),
            json!({ "sidecar": sidecar.display().to_string(), "settings": after, "materials": materials }),
        ))
    }

    /// The sidecar path and parsed `Meta` of the source `id`.
    fn sidecar_of(&self, id: Uuid, op: &str) -> Result<(PathBuf, Meta), Vec<Diagnostic>> {
        let one = |d: Diagnostic| vec![d];
        let project = self.project().unwrap();
        let entry = project.entry(id).map_err(one)?;
        let Some(sidecar) = entry.sidecar(project.assets.root()) else {
            return Err(one(invalid(format!(
                "{op}: {} is a {} document, not an imported source",
                entry.path,
                entry.kind.name()
            ))));
        };
        let sidecar = sidecar.canonicalize().unwrap_or(sidecar);
        let doc = Document::read(&sidecar, Some(DocumentKind::Meta))?;
        Ok((sidecar, Meta::from_document(doc)))
    }

    // ------------------------------------------------------- reimport

    /// `reimport { asset }`: runs the source through its importer in memory
    /// with the sidecar's settings; the sidecar's `subAssets` are brought
    /// up to date when they changed (existing ids kept). Result: `{ ok,
    /// summary, subAssets, sidecarWritten, diagnostics }` — `ok: false`
    /// with the importer's diagnostics when it failed, the sidecar
    /// untouched then. Not on the history (a reimport is reproducible).
    fn reimport(&mut self, args: &Value) -> Result<Value, Diagnostic> {
        const OP: &str = "reimport";
        let id = uuid_arg(args, "asset", OP)?;
        let (sidecar, mut meta) = self.sidecar_of(id, OP).map_err(|mut v| v.remove(0))?;
        let Some(source) = fynkel_document::asset::source_path(&sidecar) else {
            return Err(io(&sidecar, format!("{OP}: not a sidecar name")));
        };
        let imported = fynkel_import::import(&source, &meta, &fynkel_import::Limits::default());
        let diagnostics: Vec<Value> = imported.diagnostics.iter().map(diagnostic_json).collect();
        let Some(intermediate) = &imported.intermediate else {
            return Ok(json!({
                "ok": false,
                "op": OP,
                "path": source.display().to_string(),
                "diagnostics": diagnostics,
            }));
        };
        let changed = imported.sidecar_changed(&meta);
        if changed {
            meta.sub_assets = imported.sub_assets.clone();
            let text = meta.to_document().write()?;
            std::fs::write(&sidecar, text)
                .map_err(|e| io(&sidecar, format!("{OP}: cannot write the sidecar: {e}")))?;
            self.project_mut()
                .unwrap()
                .rescan(std::slice::from_ref(&sidecar));
        }
        let label = format!("Reimport {}", file_name(&source));
        Ok(self.finish(
            OP,
            &label,
            Some(id),
            json!({
                "path": source.display().to_string(),
                "sidecar": sidecar.display().to_string(),
                "sidecarWritten": changed,
                "summary": intermediate.summary_json(),
                "subAssets": meta.sub_assets.iter().map(|(k, v)| (k.clone(), Value::String(v.to_string()))).collect::<serde_json::Map<_, _>>(),
                "diagnostics": diagnostics,
            }),
        ))
    }

    // ------------------------------------------------------ undo/redo

    fn undo_asset(&mut self) -> Result<Value, Diagnostic> {
        let Some(done) = self.assets_history.undo.pop() else {
            return Err(Diagnostic::error(
                Code::EDIT_NO_HISTORY,
                "no asset operation to undo",
            ));
        };
        match self.replay(&done, true) {
            Ok(id) => {
                self.assets_history.redo.push(done.clone());
                let label = format!("Undo {}", done.label);
                Ok(self.finish("undoAsset", &label, id, json!({ "undone": done.label })))
            }
            Err(d) => {
                self.assets_history.undo.push(done);
                Err(d)
            }
        }
    }

    fn redo_asset(&mut self) -> Result<Value, Diagnostic> {
        let Some(done) = self.assets_history.redo.pop() else {
            return Err(Diagnostic::error(
                Code::EDIT_NO_HISTORY,
                "no asset operation to redo",
            ));
        };
        match self.replay(&done, false) {
            Ok(id) => {
                self.assets_history.undo.push(done.clone());
                let label = format!("Redo {}", done.label);
                Ok(self.finish("redoAsset", &label, id, json!({ "redone": done.label })))
            }
            Err(d) => {
                self.assets_history.redo.push(done);
                Err(d)
            }
        }
    }

    /// Replays `done` backwards (`inverse`) or forwards; the id of the
    /// asset that exists afterwards, when one does.
    fn replay(&mut self, done: &Done, inverse: bool) -> Result<Option<Uuid>, Diagnostic> {
        let op = if inverse { "undoAsset" } else { "redoAsset" };
        match &done.op {
            AssetOp::Created { path, id, text } => {
                if inverse {
                    let now = std::fs::read_to_string(path).ok();
                    match now {
                        None => {}
                        Some(t) if t == *text => std::fs::remove_file(path)
                            .map_err(|e| io(path, format!("{op}: cannot remove: {e}")))?,
                        Some(_) => {
                            return Err(invalid(format!(
                                "{op}: {} was edited since it was created; delete it yourself if you mean to",
                                file_name(path)
                            ))
                            .with_document(path.display().to_string()))
                        }
                    }
                    self.project_mut()
                        .unwrap()
                        .rescan(std::slice::from_ref(path));
                    Ok(None)
                } else {
                    if path.exists() {
                        return Err(invalid(format!("{op}: {} exists again", file_name(path)))
                            .with_document(path.display().to_string()));
                    }
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            io(parent, format!("{op}: cannot create the folder: {e}"))
                        })?;
                    }
                    std::fs::write(path, text)
                        .map_err(|e| io(path, format!("{op}: cannot write: {e}")))?;
                    self.project_mut()
                        .unwrap()
                        .rescan(std::slice::from_ref(path));
                    Ok(Some(*id))
                }
            }
            AssetOp::Moved {
                from,
                to,
                id,
                sidecar,
            } => {
                let (a, b) = if inverse { (to, from) } else { (from, to) };
                self.move_asset(a, b, *sidecar, op)?;
                if let Some((_, doc)) = self.documents.iter_mut().find(|(_, d)| d.path == *a) {
                    doc.path = b.clone();
                    doc.editor.relocate(b);
                }
                self.project_mut().unwrap().rescan(&[a.clone(), b.clone()]);
                Ok(Some(*id))
            }
            AssetOp::Deleted {
                path,
                trash,
                id,
                sidecar,
            } => {
                let (a, b) = if inverse {
                    (trash, path)
                } else {
                    (path, trash)
                };
                if !inverse && self.documents.values().any(|d| d.path == *path) {
                    return Err(invalid(format!(
                        "{op}: {} is open as a document",
                        file_name(path)
                    ))
                    .with_suggestion("close its tab first"));
                }
                self.move_asset(a, b, *sidecar, op)?;
                self.project_mut()
                    .unwrap()
                    .rescan(std::slice::from_ref(path));
                Ok(if inverse { Some(*id) } else { None })
            }
            AssetOp::Settings {
                sidecar,
                id,
                before,
                after,
            } => {
                let wanted = if inverse { before } else { after };
                let doc = Document::read(sidecar, Some(DocumentKind::Meta))
                    .map_err(|mut v| v.remove(0))?;
                let mut meta = Meta::from_document(doc);
                meta.settings = Node::from_serde(wanted);
                let text = meta.to_document().write()?;
                std::fs::write(sidecar, text)
                    .map_err(|e| io(sidecar, format!("{op}: cannot write the sidecar: {e}")))?;
                self.project_mut()
                    .unwrap()
                    .rescan(std::slice::from_ref(sidecar));
                Ok(Some(*id))
            }
            AssetOp::Rewrote { files } => {
                self.reload_after = self.write_rewrites(files, inverse, op)?;
                Ok(None)
            }
            AssetOp::Group { ops, id } => {
                // Undo refuses up front when a created file was edited
                // since, so a group is never left half undone.
                if inverse {
                    for o in ops {
                        if let AssetOp::Created { path, text, .. } = o {
                            if std::fs::read_to_string(path).is_ok_and(|t| t != *text) {
                                return Err(invalid(format!(
                                    "{op}: {} was edited since it was created; delete it yourself if you mean to",
                                    file_name(path)
                                ))
                                .with_document(path.display().to_string()));
                            }
                        }
                    }
                    for o in ops.iter().rev() {
                        self.replay(
                            &Done {
                                op: o.clone(),
                                label: done.label.clone(),
                            },
                            true,
                        )?;
                    }
                } else {
                    for o in ops {
                        self.replay(
                            &Done {
                                op: o.clone(),
                                label: done.label.clone(),
                            },
                            false,
                        )?;
                    }
                }
                Ok(Some(*id))
            }
        }
    }

    // --------------------------------------------------------- detail

    /// `assetDetail { asset }`: the entry plus what the Asset panel shows —
    /// the file's size and modification time; for a source its sidecar
    /// (`importer`, `importerVersion`, `settings`, `subAssets`); for a
    /// material its shader and factors (the swatch); for a scene or prefab
    /// its entity count; for a rig its skeleton and clips; the dependents.
    pub fn asset_detail(&self, id: Uuid) -> Result<Value, Vec<Diagnostic>> {
        let one = |d: Diagnostic| vec![d];
        let project = self.require_project().map_err(one)?;
        let entry = project.entry(id).map_err(one)?;
        let root = project.assets.root();
        let file = entry.file(root);
        let mut v = entry_json(entry);
        v["file"] = file_json(&file);
        v["dependents"] = Value::Array(project.dependents(id).iter().map(|r| r.json()).collect());
        match &entry.kind {
            AssetKind::Source { .. } => {
                let sidecar = entry.sidecar(root).expect("a source has a sidecar path");
                let doc = Document::read(&sidecar, Some(DocumentKind::Meta))?;
                let meta = Meta::from_document(doc);
                v["sidecar"] = json!({
                    "path": sidecar.display().to_string(),
                    "importer": meta.importer,
                    "importerVersion": meta.importer_version,
                    "settings": meta.settings.to_serde(),
                    "subAssets": meta.sub_assets.iter().map(|(k, v)| (k.clone(), Value::String(v.to_string()))).collect::<serde_json::Map<_, _>>(),
                });
            }
            AssetKind::Document(kind) => {
                let doc = Document::read(&file, Some(*kind))?;
                match kind {
                    DocumentKind::Material => {
                        let m = Material::from_document(doc.clone());
                        v["material"] = json!({
                            "shader": m.shader,
                            "alphaMode": fynkel_document::material::alpha_mode_name(m.alpha_mode),
                            "alphaCutoff": m.alpha_cutoff,
                            "doubleSided": m.double_sided,
                            "fields": doc.root.to_serde(),
                        });
                    }
                    DocumentKind::Scene | DocumentKind::Prefab => {
                        v["entities"] = doc
                            .root
                            .get("entities")
                            .and_then(Node::as_object)
                            .map(|m| m.len())
                            .unwrap_or(0)
                            .into();
                    }
                    _ => {
                        v["fields"] = doc.root.to_serde();
                    }
                }
            }
        }
        Ok(v)
    }
}

/// `{ size, modified }` of a file (`modified` as seconds since the epoch), or null.
fn file_json(path: &Path) -> Value {
    match std::fs::metadata(path) {
        Ok(m) => json!({
            "size": m.len(),
            "modified": m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64()),
        }),
        Err(_) => Value::Null,
    }
}
