//! `fynkel_authoring` — the editor's session over `fynkel_document`
//! (ADR-0016 "Resolutions at the start of M4"; blueprint §19.2e item 1).
//!
//! One [`AuthoringSession`] holds one open project ([`OpenProject`]: the
//! project file, the lock check, the schemas with the project's behaviour
//! schemas, the asset database and every reference in it, the shader
//! manifests, the committed behaviour metadata) and any number of open
//! documents, each a `fynkel_document::authoring::Editor` behind a small
//! integer [`Handle`]. Every mutation is one of the editor's transactions;
//! every read is JSON, so the C ABI in `fynkel_native` (feature
//! `authoring`) and the Dart package over it stay thin and the CLI can
//! link this crate directly.
//!
//! The session never runs game code. The edit-preview world of item 6
//! ([`preview`]) is a `fynkel_core::World` in edit mode — commands,
//! propagation and the snapshot, no system — described from the live
//! documents by [`AuthoringSession::describe`] and drawn from the cooked
//! cache [`OpenProject::cook`] fills. Diagnostics of the session itself
//! are `FYN-T####`; document content keeps its `FYN-D`/`FYN-E`.

#![allow(clippy::result_large_err)] // `Diagnostic` is the workspace's error type

pub mod behaviours;
pub mod instance;
pub mod layers;
pub mod live;
pub mod log;
pub mod ops;
pub mod preview;
pub mod project;
pub mod query;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use fynkel_document::authoring::{Applied, Command, Editor, Request, Transaction};
use fynkel_document::{
    diagnostic_json, Document, DocumentCodec, DocumentKind, Node, ProjectReport,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub use log::LOG_FILE;
pub use ops::{AssetHistory, AssetOp, Done, OPERATIONS, TRASH_DIR};
pub use preview::{Hit, Line, Preview, Ray, SyncReport, View};
pub use project::{OpenProject, Reference, METADATA_FILE};

/// An open document in a session; never reused within a session.
pub type Handle = u32;

/// The command names the session applies (`fynkel_document::authoring::Command`
/// JSON forms), for a client that wants to list them.
pub const COMMANDS: &[&str] = &[
    "createObject",
    "deleteObject",
    "renameObject",
    "setObjectActive",
    "reparentObject",
    "duplicateObject",
    "addComponent",
    "removeComponent",
    "setField",
    "unsetField",
    "assignAsset",
    "instantiatePrefab",
    "setPrefabOverride",
    "revertPrefabOverride",
    "unpackPrefab",
    "setDocumentField",
    "unsetDocumentField",
    "createMaterial",
    "saveDocument",
];

/// One open document: the editor and where its file is.
pub struct OpenDocument {
    /// Canonical absolute path of the file — for an imported hierarchy
    /// (`<id>#prefab`, M4-07) the source's or the rig document's.
    pub path: PathBuf,
    /// `Some("prefab")` when this is an imported hierarchy opened through
    /// `open_document("<id>#prefab")`: the prefab document the importer
    /// derives, under the derived `node/<i>` ids, read-only.
    pub sub: Option<String>,
    /// Why this document cannot be edited, when it cannot (the imported
    /// hierarchy's case; a read-only *project* is reported by the session).
    pub read_only: Option<String>,
    pub editor: Editor,
}

impl OpenDocument {
    /// The document as a client names it: the path, `#<sub>` for an
    /// imported hierarchy.
    pub fn shown(&self) -> String {
        match &self.sub {
            Some(sub) => format!("{}#{sub}", self.path.display()),
            None => self.path.display().to_string(),
        }
    }
}

#[derive(Default)]
pub struct AuthoringSession {
    project: Option<OpenProject>,
    documents: BTreeMap<Handle, OpenDocument>,
    next_handle: Handle,
    /// The asset operations' own undo/redo (`ops`), per project.
    assets_history: AssetHistory,
    /// Open documents the last asset operation rewrote on disk, for its
    /// result's `reload`.
    reload_after: Vec<Handle>,
    /// This opening of the project in the authoring log ([`log`]).
    log_session: Uuid,
}

impl AuthoringSession {
    pub fn new() -> AuthoringSession {
        AuthoringSession {
            project: None,
            documents: BTreeMap::new(),
            next_handle: 1,
            assets_history: AssetHistory::default(),
            reload_after: Vec::new(),
            log_session: Uuid::nil(),
        }
    }

    // ------------------------------------------------------------ project

    /// Opens the project at `dir`, closing any open project and its
    /// documents first. See [`OpenProject::open`] for what fails and what
    /// is merely reported.
    pub fn open_project(&mut self, dir: &Path) -> Result<&OpenProject, Vec<Diagnostic>> {
        self.close_project();
        let project = OpenProject::open(dir)?;
        self.project = Some(project);
        self.log_session = Uuid::new_v4();
        self.log(json!({ "op": "open" }));
        Ok(self.project.as_ref().unwrap())
    }

    /// Closes the project and every open document (unsaved edits are lost
    /// — the client asks first).
    pub fn close_project(&mut self) {
        self.documents.clear();
        self.assets_history = AssetHistory::default();
        self.project = None;
    }

    pub fn project(&self) -> Option<&OpenProject> {
        self.project.as_ref()
    }

    pub fn project_mut(&mut self) -> Option<&mut OpenProject> {
        self.project.as_mut()
    }

    /// The text of every open document with unsaved edits, by its path —
    /// what [`OpenProject::cook_live`] cooks in place of the files when
    /// Play runs what the editor holds (ADR-0021 P5). An imported
    /// hierarchy (read-only, derived) is never among them.
    pub fn unsaved_texts(&self) -> std::collections::HashMap<PathBuf, String> {
        self.documents
            .values()
            .filter(|d| d.sub.is_none() && d.editor.is_dirty())
            .filter_map(|d| {
                let text = d.editor.document().write().ok()?;
                Some((d.path.clone(), text))
            })
            .collect()
    }

    /// Appends one entry to the open project's authoring log ([`log`]).
    pub(crate) fn log(&self, entry: Value) {
        if let Some(project) = &self.project {
            log::append(&project.dir, self.log_session, entry);
        }
    }

    /// `path` as the log names it: relative to the project when inside it.
    fn logged_path(&self, path: &Path) -> String {
        let project = self.project.as_ref().map(|p| p.dir.as_path());
        match project.and_then(|dir| path.strip_prefix(dir).ok()) {
            Some(rel) => rel.to_string_lossy().replace('\\', "/"),
            None => path.display().to_string(),
        }
    }

    fn require_project(&self) -> Result<&OpenProject, Diagnostic> {
        self.project.as_ref().ok_or_else(|| {
            Diagnostic::error(Code::SESSION_NO_PROJECT, "no project is open")
                .with_suggestion("open a project directory first")
        })
    }

    /// The whole-project validation `fynkel validate` runs (`ProjectReport`),
    /// with the importer's prefab provider and this build's expected lock.
    pub fn validate_project(&self) -> Result<ProjectReport, Diagnostic> {
        let p = self.require_project()?;
        let expected = fynkel_import::expected_lock();
        let sources = fynkel_import::ImportedPrefabs::default();
        Ok(fynkel_document::validate_project_with(
            &p.dir,
            &fynkel_document::SchemaSet::embedded(),
            Some(&expected),
            &sources,
        ))
    }

    /// Rescans the asset root; see [`OpenProject::rescan`].
    pub fn rescan(&mut self, paths: &[PathBuf]) -> Result<Vec<Diagnostic>, Diagnostic> {
        self.require_project()?;
        Ok(self.project.as_mut().unwrap().rescan(paths))
    }

    // ---------------------------------------------------------- documents

    /// Opens a scene, prefab or material document by path (absolute, or
    /// relative to the project directory) or by asset id (UUID text).
    /// Idempotent: a document already open returns its handle. Other
    /// document kinds, newer and outdated documents are refused with the
    /// editor's `FYN-E0007`.
    ///
    /// `<id>#prefab` (or `<path>#prefab`) opens an **imported hierarchy**
    /// (M4-07): the prefab document the importer derives from a model
    /// source (`hero.glb`) or a rig document's skeleton — the one an
    /// instance `{ asset: <id>, sub: "prefab" }` expands — as a read-only
    /// prefab document (`readOnly` names the source); a source without a
    /// hierarchy or a rig whose skeleton is not an imported model is
    /// `FYN-D0401`.
    pub fn open_document(&mut self, path_or_id: &str) -> Result<Handle, Vec<Diagnostic>> {
        if let Some((asset, sub)) = path_or_id.rsplit_once('#') {
            if !sub.is_empty() && !asset.is_empty() {
                return self.open_imported(asset, sub);
            }
        }
        let project = self.require_project().map_err(|d| vec![d])?;
        let path = match path_or_id.parse::<Uuid>() {
            Ok(id) => {
                let entry = project.assets.get(id).ok_or_else(|| {
                    vec![Diagnostic::error(
                        Code::SESSION_IO,
                        format!("no asset with id {id} in the project"),
                    )
                    .with_id(id.to_string())]
                })?;
                entry.file(project.assets.root())
            }
            Err(_) => {
                let p = Path::new(path_or_id);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    project.dir.join(p)
                }
            }
        };
        let canonical = path.canonicalize().map_err(|e| {
            vec![
                Diagnostic::error(Code::SESSION_IO, format!("cannot open: {e}"))
                    .with_document(path.display().to_string()),
            ]
        })?;
        if let Some((h, _)) = self
            .documents
            .iter()
            .find(|(_, d)| d.path == canonical && d.sub.is_none())
        {
            return Ok(*h);
        }
        let editor = self.read_editor(&canonical)?;
        Ok(self.insert(OpenDocument {
            path: canonical,
            sub: None,
            read_only: None,
            editor,
        }))
    }

    fn insert(&mut self, document: OpenDocument) -> Handle {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.documents.insert(handle, document);
        handle
    }

    /// `open_document("<id or path>#<sub>")`: the imported hierarchy.
    fn open_imported(&mut self, asset: &str, sub: &str) -> Result<Handle, Vec<Diagnostic>> {
        let project = self.require_project().map_err(|d| vec![d])?;
        let id = match asset.parse::<Uuid>() {
            Ok(id) => id,
            Err(_) => {
                // A path: the entry whose file it is.
                let p = Path::new(asset);
                let full = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    project.dir.join(p)
                };
                let canonical = full.canonicalize().map_err(|e| {
                    vec![
                        Diagnostic::error(Code::SESSION_IO, format!("cannot open: {e}"))
                            .with_document(full.display().to_string()),
                    ]
                })?;
                project
                    .assets
                    .entries()
                    .iter()
                    .find(|e| {
                        e.file(project.assets.root())
                            .canonicalize()
                            .is_ok_and(|f| f == canonical)
                    })
                    .map(|e| e.id)
                    .ok_or_else(|| {
                        vec![Diagnostic::error(
                            Code::SESSION_IO,
                            format!("{} is not an asset of the project", full.display()),
                        )
                        .with_document(full.display().to_string())]
                    })?
            }
        };
        let entry = project.assets.get(id).ok_or_else(|| {
            vec![Diagnostic::error(
                Code::SESSION_IO,
                format!("no asset with id {id} in the project"),
            )
            .with_id(id.to_string())]
        })?;
        let path = entry.file(project.assets.root());
        let canonical = path.canonicalize().unwrap_or(path);
        if let Some((h, _)) = self
            .documents
            .iter()
            .find(|(_, d)| d.path == canonical && d.sub.as_deref() == Some(sub))
        {
            return Ok(*h);
        }
        let (editor, read_only) = self.read_imported(id, sub)?;
        Ok(self.insert(OpenDocument {
            path: canonical,
            sub: Some(sub.to_string()),
            read_only: Some(read_only),
            editor,
        }))
    }

    /// The imported hierarchy `{ asset: id, sub }` names, as an editor,
    /// and why it is read-only.
    fn read_imported(&self, id: Uuid, sub: &str) -> Result<(Editor, String), Vec<Diagnostic>> {
        use fynkel_document::{AssetKind, PrefabLibrary};
        let project = self.require_project().map_err(|d| vec![d])?;
        let entry = project.assets.get(id).ok_or_else(|| {
            vec![Diagnostic::error(
                Code::SESSION_IO,
                format!("no asset with id {id} in the project"),
            )
            .with_id(id.to_string())]
        })?;
        let shown = format!("{}#{sub}", entry.path);
        let not_available = |why: &str| {
            vec![Diagnostic::error(
                Code::PREFAB_UNKNOWN,
                format!("prefab {id}#{sub} is not available: {why}"),
            )
            .with_document(shown.clone())
            .with_id(id.to_string())
            .with_suggestion(
                "an imported hierarchy is a model source's or a rig document's \"prefab\" sub-asset",
            )]
        };
        let what = match &entry.kind {
            AssetKind::Source { .. } => "source",
            AssetKind::Document(DocumentKind::Rig) => "rig",
            AssetKind::Document(kind) => {
                return Err(not_available(&format!(
                    "a {} document has no imported hierarchy",
                    kind.name()
                )))
            }
        };
        if sub != fynkel_document::rig::RIG_PREFAB_SUB {
            return Err(not_available(&format!(
                "the hierarchy is the \"prefab\" sub-asset, not {sub:?}"
            )));
        }
        let mut library = PrefabLibrary::empty();
        let diags = library.add_sources(
            &project.assets,
            &[(id, sub.to_string())],
            &fynkel_import::ImportedPrefabs::default(),
        );
        if fynkel_document::has_errors(&diags) {
            return Err(diags);
        }
        let prefab = library
            .get_ref(&AssetRef::with_sub(crate::project::to_contract(id), sub))
            .ok_or_else(|| {
                not_available(match what {
                    "rig" => "the rig's skeleton is not an imported model with a node hierarchy",
                    _ => "the source imports without a node hierarchy",
                })
            })?;
        let mut document = prefab.document.clone();
        document.path = Some(shown.clone());
        let editor = Editor::open_with(document, &project.schemas, project.shaders())?;
        Ok((editor, imported_reason(&shown, what)))
    }

    fn read_editor(&self, path: &Path) -> Result<Editor, Vec<Diagnostic>> {
        let project = self.require_project().map_err(|d| vec![d])?;
        let document = Document::read(path, None)?;
        Editor::open_with(document, &project.schemas, project.shaders())
    }

    /// Re-reads the document from disk; the history is dropped and the
    /// revision is the file's. Unsaved edits are lost — the client asks.
    /// An imported hierarchy is imported again.
    pub fn reload(&mut self, handle: Handle) -> Result<(), Vec<Diagnostic>> {
        let doc = self.document(handle).map_err(|d| vec![d])?;
        let path = doc.path.clone();
        let editor = match doc.sub.clone() {
            Some(sub) => {
                let id = self.imported_id(handle).map_err(|d| vec![d])?;
                self.read_imported(id, &sub)?.0
            }
            None => self.read_editor(&path)?,
        };
        self.documents.get_mut(&handle).unwrap().editor = editor;
        Ok(())
    }

    /// The asset id an imported-hierarchy handle was opened from.
    fn imported_id(&self, handle: Handle) -> Result<Uuid, Diagnostic> {
        let project = self.require_project()?;
        let doc = self.document(handle)?;
        project
            .assets
            .entries()
            .iter()
            .find(|e| {
                e.file(project.assets.root())
                    .canonicalize()
                    .is_ok_and(|f| f == doc.path)
            })
            .map(|e| e.id)
            .ok_or_else(|| {
                Diagnostic::error(
                    Code::SESSION_IO,
                    format!(
                        "{} is no longer an asset of the project",
                        doc.path.display()
                    ),
                )
                .with_document(doc.shown())
            })
    }

    pub fn close_document(&mut self, handle: Handle) -> Result<(), Diagnostic> {
        self.documents
            .remove(&handle)
            .map(|_| ())
            .ok_or_else(|| unknown_handle(handle))
    }

    pub fn document(&self, handle: Handle) -> Result<&OpenDocument, Diagnostic> {
        self.documents
            .get(&handle)
            .ok_or_else(|| unknown_handle(handle))
    }

    pub fn document_mut(&mut self, handle: Handle) -> Result<&mut OpenDocument, Diagnostic> {
        self.documents
            .get_mut(&handle)
            .ok_or_else(|| unknown_handle(handle))
    }

    /// Open handles in opening order.
    pub fn handles(&self) -> Vec<Handle> {
        self.documents.keys().copied().collect()
    }

    /// The handle of the open document at `path` (not an imported
    /// hierarchy of it), if any.
    pub fn handle_of(&self, path: &Path) -> Option<Handle> {
        let canonical = path.canonicalize().ok()?;
        self.documents
            .iter()
            .find(|(_, d)| d.path == canonical && d.sub.is_none())
            .map(|(h, _)| *h)
    }

    fn writable(&self, handle: Handle) -> Result<(), Diagnostic> {
        let project = self.require_project()?;
        let doc = self.document(handle)?;
        if let Some(reason) = &doc.read_only {
            return Err(Diagnostic::error(
                Code::EDIT_NOT_EDITABLE,
                format!("the document is read-only: {reason}"),
            )
            .with_document(doc.shown()));
        }
        if let Some(reason) = &project.read_only {
            return Err(Diagnostic::error(
                Code::EDIT_NOT_EDITABLE,
                format!("the project is read-only: {reason}"),
            )
            .with_document(doc.path.display().to_string())
            .with_suggestion(
                "open the project with the Fynkel that wrote its lock, or `fynkel project lock` with this one once you mean to downgrade",
            ));
        }
        Ok(())
    }

    /// Applies one transaction to the document (`Editor::apply`): the
    /// conflict, rollback and validation rules of `fynkel_document`.
    /// A relative path in `createMaterial` or `saveDocument` is taken from
    /// the project directory (the editor's process may run from anywhere),
    /// and the paths a committed transaction wrote are rescanned so the
    /// asset database sees the new material or the saved document at once.
    /// `Err` only when the handle is unknown or the document or project is
    /// read-only. `unpackPrefab` sees the prefabs as the preview does —
    /// open prefab documents from their editors, the rest from disk, the
    /// imported hierarchies through the importer ([`Self::describe`]).
    pub fn apply(&mut self, handle: Handle, mut request: Request) -> Result<Applied, Diagnostic> {
        self.writable(handle)?;
        let unpacks = request
            .commands
            .iter()
            .any(|c| matches!(c.command, Command::UnpackPrefab { .. }));
        let library = if unpacks {
            let project = self.project.as_ref().unwrap();
            let doc = self.documents.get(&handle).unwrap();
            self.prefab_library(project, &doc.editor.scene()).0
        } else {
            fynkel_document::PrefabLibrary::empty()
        };
        let project = self.project.as_ref().unwrap();
        let doc = self.documents.get_mut(&handle).unwrap();
        let mut written: Vec<PathBuf> = Vec::new();
        for requested in &mut request.commands {
            match &mut requested.command {
                Command::CreateMaterial { path, .. } => {
                    if path.is_relative() {
                        *path = project.dir.join(&*path);
                    }
                    written.push(path.clone());
                }
                Command::SaveDocument { path: Some(path) } => {
                    if path.is_relative() {
                        *path = project.dir.join(&*path);
                    }
                    written.push(path.clone());
                }
                Command::SaveDocument { path: None } => written.push(doc.path.clone()),
                _ => {}
            }
        }
        let commands: Vec<Value> = request
            .commands
            .iter()
            .map(|r| r.command.to_json())
            .collect();
        let applied = doc
            .editor
            .apply_with_prefabs(request, &project.schemas, &library);
        if applied.ok {
            let path = doc.path.clone();
            self.log(json!({
                "op": "apply",
                "document": self.logged_path(&path),
                "transactionId": applied.transaction_id.to_string(),
                "baseRevision": applied.base_revision.as_str(),
                "revision": applied.revision.as_str(),
                "commands": commands,
                "affectedIds": applied
                    .mutations
                    .iter()
                    .map(|m| m.affected_ids.iter().map(|u| u.to_string()).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            }));
        }
        if applied.ok && !written.is_empty() {
            let written: Vec<PathBuf> = written
                .into_iter()
                .map(|p| p.canonicalize().unwrap_or(p))
                .collect();
            self.project.as_mut().unwrap().rescan(&written);
        }
        Ok(applied)
    }

    /// [`AuthoringSession::apply`] from the patch JSON `fynkel scene apply`
    /// reads (an array of commands, or `{ transactionId?, baseRevision?,
    /// commands }`). A malformed request is `FYN-E0004` from the parser.
    pub fn apply_json(
        &mut self,
        handle: Handle,
        request: &Value,
    ) -> Result<Value, Vec<Diagnostic>> {
        let request = Request::from_json(request)?;
        let applied = self.apply(handle, request).map_err(|d| vec![d])?;
        let mut v = applied.json();
        v["state"] = self.state_json(handle).map_err(|d| vec![d])?;
        Ok(v)
    }

    pub fn undo(&mut self, handle: Handle) -> Result<Transaction, Diagnostic> {
        self.writable(handle)?;
        let t = self.documents.get_mut(&handle).unwrap().editor.undo()?;
        self.log_history("undo", handle, &t);
        Ok(t)
    }

    pub fn redo(&mut self, handle: Handle) -> Result<Transaction, Diagnostic> {
        self.writable(handle)?;
        let t = self.documents.get_mut(&handle).unwrap().editor.redo()?;
        self.log_history("redo", handle, &t);
        Ok(t)
    }

    fn log_history(&self, op: &str, handle: Handle, t: &Transaction) {
        let path = self.documents[&handle].path.clone();
        self.log(json!({
            "op": op,
            "document": self.logged_path(&path),
            "transactionId": t.id.to_string(),
            "commands": t.mutations.iter().map(|m| m.command).collect::<Vec<_>>(),
        }));
    }

    /// The JSON of an undo or redo: the transaction replayed and the
    /// document's state now.
    pub fn transaction_json(&self, handle: Handle, t: &Transaction) -> Result<Value, Diagnostic> {
        let mut affected: Vec<String> = Vec::new();
        for m in &t.mutations {
            for id in &m.affected_ids {
                let s = id.to_string();
                if !affected.contains(&s) {
                    affected.push(s);
                }
            }
        }
        Ok(json!({
            "transactionId": t.id.to_string(),
            "baseRevision": t.base_revision.as_str(),
            "revision": t.revision.as_str(),
            "commands": t.mutations.iter().map(|m| m.command).collect::<Vec<_>>(),
            "affectedIds": affected,
            "state": self.state_json(handle)?,
        }))
    }

    /// Writes the document to `path` or where it was read from, then
    /// rescans that path so the database sees the new name or id.
    pub fn save(&mut self, handle: Handle, path: Option<&Path>) -> Result<PathBuf, Diagnostic> {
        self.writable(handle)?;
        let written = self.documents.get_mut(&handle).unwrap().editor.save(path)?;
        let written = written.canonicalize().unwrap_or(written);
        let doc = self.documents.get_mut(&handle).unwrap();
        if doc.path != written {
            // Save As: the editor writes there from now on.
            doc.editor.relocate(&written);
        }
        doc.path = written.clone();
        self.project
            .as_mut()
            .unwrap()
            .rescan(std::slice::from_ref(&written));
        self.log(json!({ "op": "save", "document": self.logged_path(&written) }));
        Ok(written)
    }

    /// The document's validation now (baseline errors included) plus its
    /// asset references checked against the database.
    pub fn validate(&self, handle: Handle) -> Result<Vec<Diagnostic>, Diagnostic> {
        let project = self.require_project()?;
        let doc = self.document(handle)?;
        let mut out = doc.editor.validate(&project.schemas);
        out.extend(project.assets.check_references(doc.editor.document()));
        Ok(out)
    }

    /// The runtime description of the document `handle` as the Scene view
    /// shows it (M4-06): the **live** document (unsaved edits included) —
    /// a scene, or a prefab read as a scene — with every prefab instance
    /// expanded against the prefabs *as they are now*: an open prefab
    /// document contributes its editor's content, the others their file;
    /// imported hierarchies (`{ asset, sub: "prefab" }`) come from the
    /// importer as in project validation. `description` is what
    /// `World::load_scene` takes; `behaviours` the authored behaviour
    /// components (the preview draws nothing for them); `diagnostics` the
    /// expansion's and the description's (an unknown component type, a
    /// value that does not decode). A material document is refused
    /// (`FYN-T0003`): it has no entities to preview.
    pub fn describe(&self, handle: Handle) -> Result<fynkel_document::DescribedScene, Diagnostic> {
        self.describe_with_anchors(handle).map(|(d, _)| d)
    }

    /// [`describe`](Self::describe) plus the **anchor map**: every entity
    /// the expansion made (a prefab instance's inner entities, nested
    /// instances included) → the *document* entity that carries the
    /// instance. The Scene view selects and hides whole instances, so a
    /// pick inside one answers with its anchor.
    pub fn describe_with_anchors(
        &self,
        handle: Handle,
    ) -> Result<(fynkel_document::DescribedScene, HashMap<Uuid, Uuid>), Diagnostic> {
        let project = self.require_project()?;
        let doc = self.document(handle)?;
        let document = doc.editor.document();
        let scene = match document.kind {
            DocumentKind::Scene => doc.editor.scene(),
            DocumentKind::Prefab => {
                fynkel_document::Prefab::from_document(document.clone()).scene()
            }
            other => {
                return Err(Diagnostic::error(
                    Code::SESSION_INVALID_REQUEST,
                    format!("a {} document has no scene to preview", other.name()),
                )
                .with_document(doc.path.display().to_string())
                .with_suggestion("open a scene or a prefab in the Scene view"));
            }
        };
        let (library, mut diagnostics) = self.prefab_library(project, &scene);
        let expanded = fynkel_document::expand(&scene, &library, &project.schemas);
        diagnostics.extend(expanded.diagnostics.iter().cloned());
        let mut described = fynkel_document::describe(&expanded.scene(), &project.schemas);
        diagnostics.append(&mut described.diagnostics);
        described.diagnostics = diagnostics;
        // Inner → anchor, then chased to the outermost anchor (a nested
        // instance's anchor is itself an expanded entity).
        let mut anchors: HashMap<Uuid, Uuid> = HashMap::new();
        for instance in &expanded.instances {
            for (_, scene_id) in &instance.entities {
                anchors.insert(*scene_id, instance.anchor);
            }
        }
        let resolved: HashMap<Uuid, Uuid> = anchors
            .keys()
            .map(|id| {
                let mut a = anchors[id];
                let mut hops = 0;
                while let Some(next) = anchors.get(&a) {
                    a = *next;
                    hops += 1;
                    if hops > 64 {
                        break;
                    }
                }
                (*id, a)
            })
            .collect();
        Ok((described, resolved))
    }

    /// Every prefab document of the project — the open ones from their
    /// editors, the rest from disk — plus the imported hierarchies `scene`
    /// and those prefabs instantiate.
    fn prefab_library(
        &self,
        project: &OpenProject,
        scene: &fynkel_document::Scene,
    ) -> (fynkel_document::PrefabLibrary, Vec<Diagnostic>) {
        use fynkel_document::{AssetKind, Prefab, PrefabLibrary};
        let mut library = PrefabLibrary::empty();
        let mut diagnostics = Vec::new();
        let mut open: Vec<PathBuf> = Vec::new();
        for d in self.documents.values() {
            // An open imported hierarchy is what the importer gives below.
            if d.editor.document().kind == DocumentKind::Prefab && d.sub.is_none() {
                library.add(Prefab::from_document(d.editor.document().clone()));
                open.push(d.path.clone());
            }
        }
        for entry in project.assets.entries() {
            if entry.kind != AssetKind::Document(DocumentKind::Prefab) {
                continue;
            }
            let file = entry.file(project.assets.root());
            let canonical = file.canonicalize().unwrap_or_else(|_| file.clone());
            if open.contains(&canonical) {
                continue;
            }
            match Document::read(&file, Some(DocumentKind::Prefab)) {
                Ok(doc) => library.add(Prefab::from_document(doc)),
                Err(diags) => diagnostics.extend(diags),
            }
        }
        let mut wanted = PrefabLibrary::wanted_sources(scene);
        for prefab in library.prefabs() {
            for key in PrefabLibrary::wanted_sources(&prefab.scene()) {
                if !wanted.contains(&key) {
                    wanted.push(key);
                }
            }
        }
        diagnostics.extend(library.add_sources(
            &project.assets,
            &wanted,
            &fynkel_import::ImportedPrefabs::default(),
        ));
        (library, diagnostics)
    }

    // ---------------------------------------------------------------- JSON

    /// The document's state without its content: `{ handle, path,
    /// relativePath, sub, kind, id, name, formatVersion, revision, dirty,
    /// canUndo, canRedo, history, readOnly }`. An imported hierarchy has
    /// `sub: "prefab"`, `#prefab` on its `relativePath`, and `readOnly`
    /// naming the source; otherwise `readOnly` is the project's.
    pub fn state_json(&self, handle: Handle) -> Result<Value, Diagnostic> {
        let project = self.require_project()?;
        let doc = self.document(handle)?;
        let document = doc.editor.document();
        let relative = project
            .relative_to_assets(&doc.path)
            .map(|r| match &doc.sub {
                Some(sub) => format!("{r}#{sub}"),
                None => r,
            });
        Ok(json!({
            "handle": handle,
            "path": doc.path.display().to_string(),
            "relativePath": relative,
            "sub": doc.sub,
            "kind": document.kind.name(),
            "id": document.envelope.id.to_string(),
            "name": live_name(document),
            "formatVersion": document.envelope.format_version,
            "revision": doc.editor.revision().as_str(),
            "dirty": doc.editor.is_dirty(),
            "canUndo": doc.editor.can_undo(),
            "canRedo": doc.editor.can_redo(),
            "history": doc.editor.history().len(),
            "readOnly": doc.read_only.clone().or_else(|| project.read_only.clone()),
        }))
    }

    /// [`AuthoringSession::state_json`] plus `root`: the whole document
    /// as JSON (the panels read it; number lexemes become numbers).
    pub fn document_json(&self, handle: Handle) -> Result<Value, Diagnostic> {
        let mut v = self.state_json(handle)?;
        let root = &self.document(handle)?.editor.document().root;
        v["root"] = root.to_serde();
        v["roots"] = Value::Array(root_ids(root).into_iter().map(Value::String).collect());
        Ok(v)
    }

    /// [`AuthoringSession::state_json`] plus `entities`: the named entities
    /// as JSON (`null` for one the document no longer has) and `roots`, the
    /// ordered root ids — what the Hierarchy re-reads after a transaction
    /// from its `affectedIds`, instead of the whole document.
    pub fn entities_json(&self, handle: Handle, ids: &[String]) -> Result<Value, Diagnostic> {
        let mut v = self.state_json(handle)?;
        let root = &self.document(handle)?.editor.document().root;
        let entities = root.get("entities");
        v["entities"] = Value::Object(
            ids.iter()
                .map(|id| {
                    let e = entities
                        .and_then(|n| n.get(id))
                        .map(Node::to_serde)
                        .unwrap_or(Value::Null);
                    (id.clone(), e)
                })
                .collect(),
        );
        v["roots"] = Value::Array(root_ids(root).into_iter().map(Value::String).collect());
        Ok(v)
    }

    /// The typed view `fynkel scene|prefab inspect --json` prints — for a
    /// material, its shader and fields — plus the document's diagnostics.
    pub fn inspect_json(&self, handle: Handle) -> Result<Value, Diagnostic> {
        let doc = self.document(handle)?;
        let document = doc.editor.document();
        let mut v = match document.kind {
            DocumentKind::Prefab => fynkel_document::prefab::inspect_json(
                &fynkel_document::Prefab::from_document(document.clone()),
            ),
            DocumentKind::Material => {
                let m = fynkel_document::Material::from_document(document.clone());
                json!({
                    "kind": "material",
                    "id": document.envelope.id.to_string(),
                    "name": live_name(document),
                    "formatVersion": document.envelope.format_version,
                    "revision": doc.editor.revision().as_str(),
                    "shader": m.shader,
                    "alphaMode": fynkel_document::material::alpha_mode_name(m.alpha_mode),
                    "alphaCutoff": m.alpha_cutoff,
                    "doubleSided": m.double_sided,
                    "fields": document.root.to_serde(),
                })
            }
            _ => fynkel_document::scene::inspect_json(&doc.editor.scene()),
        };
        v["name"] = live_name(document).into();
        let diagnostics = self.validate(handle)?;
        v["diagnostics"] = diagnostics
            .iter()
            .map(diagnostic_json)
            .collect::<Vec<_>>()
            .into();
        Ok(v)
    }

    /// The revision (and identity) of a document **file** as it is on disk
    /// now — not of any open editor. The conflict dialog names it next to
    /// the editor's revision and the one the tab was opened from. `path` is
    /// absolute or relative to the project directory; reading failures are
    /// the reader's diagnostics.
    pub fn file_revision(&self, path: &str) -> Result<Value, Vec<Diagnostic>> {
        let project = self.require_project().map_err(|d| vec![d])?;
        let p = Path::new(path);
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            project.dir.join(p)
        };
        let document = Document::read(&full, None)?;
        let revision = fynkel_document::Revision::of(&document).map_err(|d| vec![d])?;
        Ok(json!({
            "path": full.canonicalize().unwrap_or(full).display().to_string(),
            "kind": document.kind.name(),
            "id": document.envelope.id.to_string(),
            "name": live_name(&document),
            "formatVersion": document.envelope.format_version,
            "revision": revision.as_str(),
        }))
    }

    /// Every open document's state.
    pub fn documents_json(&self) -> Vec<Value> {
        self.documents
            .keys()
            .filter_map(|h| self.state_json(*h).ok())
            .collect()
    }
}

/// The `data` an authored native component starts with: its Rust
/// `Default` through the document codec — what "Create with component"
/// and "Add Component" write. `None` for a type that is not a built-in
/// authored component (a behaviour's defaults come from its schema).
/// The test `defaults_cover_every_authored_component` keeps this list in
/// step with `fynkel_document::generated::AUTHORED_COMPONENTS`.
pub fn component_default(type_id: &str) -> Option<Node> {
    Some(match type_id {
        "fynkel.Transform" => <::fynkel_scene::Transform as Default>::default().to_node(),
        "fynkel.MeshRenderer" => {
            <::fynkel_core::components::MeshRenderer as Default>::default().to_node()
        }
        "fynkel.Camera" => <::fynkel_core::components::Camera as Default>::default().to_node(),
        "fynkel.DirectionalLight" => {
            <::fynkel_core::components::DirectionalLight as Default>::default().to_node()
        }
        "fynkel.PointLight" => {
            <::fynkel_core::components::PointLight as Default>::default().to_node()
        }
        "fynkel.SpotLight" => {
            <::fynkel_core::components::SpotLight as Default>::default().to_node()
        }
        "fynkel.Environment" => {
            <::fynkel_core::components::Environment as Default>::default().to_node()
        }
        "fynkel.RigidBody" => {
            <::fynkel_physics::components::RigidBody as Default>::default().to_node()
        }
        "fynkel.Collider" => {
            <::fynkel_physics::components::Collider as Default>::default().to_node()
        }
        "fynkel.CharacterController" => {
            <::fynkel_physics::components::CharacterController as Default>::default().to_node()
        }
        "fynkel.ThirdPersonCamera" => {
            <::fynkel_core::components::ThirdPersonCamera as Default>::default().to_node()
        }
        "fynkel.Rotator" => <::fynkel_core::components::Rotator as Default>::default().to_node(),
        "fynkel.Oscillate" => {
            <::fynkel_core::components::Oscillate as Default>::default().to_node()
        }
        "fynkel.Follow" => <::fynkel_core::components::Follow as Default>::default().to_node(),
        "fynkel.LookAtCamera" => {
            <::fynkel_core::components::LookAtCamera as Default>::default().to_node()
        }
        "fynkel.LifetimeDespawn" => {
            <::fynkel_core::components::LifetimeDespawn as Default>::default().to_node()
        }
        "fynkel.Emitter" => <::fynkel_core::components::Emitter as Default>::default().to_node(),
        "fynkel.TriggerZone" => {
            <::fynkel_core::components::TriggerZone as Default>::default().to_node()
        }
        "fynkel.Animator" => <::fynkel_core::components::Animator as Default>::default().to_node(),
        "fynkel.Skin" => <::fynkel_core::components::Skin as Default>::default().to_node(),
        "fynkel.AudioSource" => {
            <::fynkel_audio::components::AudioSource as Default>::default().to_node()
        }
        "fynkel.AudioListener" => {
            <::fynkel_audio::components::AudioListener as Default>::default().to_node()
        }
        "fynkel.Volume" => <::fynkel_core::components::Volume as Default>::default().to_node(),
        "fynkel.Layer" => <::fynkel_scene::Layer as Default>::default().to_node(),
        "fynkel.Tags" => <::fynkel_scene::Tags as Default>::default().to_node(),
        _ => return None,
    })
}

/// The root entity ids in sibling order: a prefab's `roots` array, or —
/// for a scene, which has none — the entities with a null `parent` in
/// document order.
fn root_ids(root: &Node) -> Vec<String> {
    if let Some(items) = root.get("roots").and_then(Node::as_array) {
        return items
            .iter()
            .filter_map(|n| n.as_str().map(str::to_string))
            .collect();
    }
    root.get("entities")
        .and_then(Node::as_object)
        .map(|members| {
            members
                .iter()
                .filter(|m| {
                    m.value
                        .get("parent")
                        .map(|p| p.as_str().is_none())
                        .unwrap_or(true)
                })
                .map(|m| m.key.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Why an imported hierarchy (`<shown>`, a `source` or a `rig`) is read-only.
pub(crate) fn imported_reason(shown: &str, kind: &str) -> String {
    let what = match kind {
        "rig" => "a rig document's skeleton",
        _ => "a model source",
    };
    format!(
        "{shown} is an imported hierarchy ({what}): its entities are derived from the file at import, so it cannot be edited here — instance it and override fields, or `fynkel import --prefab` for an editable copy"
    )
}

fn unknown_handle(handle: Handle) -> Diagnostic {
    Diagnostic::error(
        Code::SESSION_UNKNOWN_DOCUMENT,
        format!("no open document with handle {handle}"),
    )
    .with_suggestion("the document was closed or never opened; open it again")
}

/// The document's name as edited: the envelope is parsed when the file is
/// read, while `setDocumentField { pointer: "/name" }` edits the root, so
/// the root wins whenever it carries a string there.
fn live_name(document: &Document) -> String {
    document
        .root
        .pointer("/name")
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| document.envelope.name.clone())
}
