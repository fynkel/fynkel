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
//! The session never runs game code and never touches a cooked cache
//! (that is the edit-preview world of item 6). Diagnostics of the session
//! itself are `FYN-T####`; document content keeps its `FYN-D`/`FYN-E`.

#![allow(clippy::result_large_err)] // `Diagnostic` is the workspace's error type

pub mod ops;
pub mod project;
pub mod query;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic};
use fynkel_document::authoring::{Applied, Command, Editor, Request, Transaction};
use fynkel_document::{
    diagnostic_json, Document, DocumentCodec, DocumentKind, Node, ProjectReport,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub use ops::{AssetHistory, AssetOp, Done, OPERATIONS, TRASH_DIR};
pub use project::{OpenProject, Reference, METADATA_FILE};

/// An open document in a session; never reused within a session.
pub type Handle = u32;

/// The command names the session applies (`fynkel_document::authoring::Command`
/// JSON forms), for a client that wants to list them.
pub const COMMANDS: &[&str] = &[
    "createObject",
    "deleteObject",
    "renameObject",
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
    "setDocumentField",
    "unsetDocumentField",
    "createMaterial",
    "saveDocument",
];

/// One open document: the editor and where its file is.
pub struct OpenDocument {
    /// Canonical absolute path of the file.
    pub path: PathBuf,
    pub editor: Editor,
}

#[derive(Default)]
pub struct AuthoringSession {
    project: Option<OpenProject>,
    documents: BTreeMap<Handle, OpenDocument>,
    next_handle: Handle,
    /// The asset operations' own undo/redo (`ops`), per project.
    assets_history: AssetHistory,
}

impl AuthoringSession {
    pub fn new() -> AuthoringSession {
        AuthoringSession {
            project: None,
            documents: BTreeMap::new(),
            next_handle: 1,
            assets_history: AssetHistory::default(),
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
    pub fn open_document(&mut self, path_or_id: &str) -> Result<Handle, Vec<Diagnostic>> {
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
        if let Some((h, _)) = self.documents.iter().find(|(_, d)| d.path == canonical) {
            return Ok(*h);
        }
        let editor = self.read_editor(&canonical)?;
        let handle = self.next_handle;
        self.next_handle += 1;
        self.documents.insert(
            handle,
            OpenDocument {
                path: canonical,
                editor,
            },
        );
        Ok(handle)
    }

    fn read_editor(&self, path: &Path) -> Result<Editor, Vec<Diagnostic>> {
        let project = self.require_project().map_err(|d| vec![d])?;
        let document = Document::read(path, None)?;
        Editor::open_with(document, &project.schemas, project.shaders())
    }

    /// Re-reads the document from disk; the history is dropped and the
    /// revision is the file's. Unsaved edits are lost — the client asks.
    pub fn reload(&mut self, handle: Handle) -> Result<(), Vec<Diagnostic>> {
        let path = self.document(handle).map_err(|d| vec![d])?.path.clone();
        let editor = self.read_editor(&path)?;
        self.documents.get_mut(&handle).unwrap().editor = editor;
        Ok(())
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

    /// The handle of the open document at `path`, if any.
    pub fn handle_of(&self, path: &Path) -> Option<Handle> {
        let canonical = path.canonicalize().ok()?;
        self.documents
            .iter()
            .find(|(_, d)| d.path == canonical)
            .map(|(h, _)| *h)
    }

    fn writable(&self, handle: Handle) -> Result<(), Diagnostic> {
        let project = self.require_project()?;
        let doc = self.document(handle)?;
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
    /// `Err` only when the handle is unknown or the project is read-only.
    pub fn apply(&mut self, handle: Handle, mut request: Request) -> Result<Applied, Diagnostic> {
        self.writable(handle)?;
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
        let applied = doc.editor.apply(request, &project.schemas);
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
        self.documents.get_mut(&handle).unwrap().editor.undo()
    }

    pub fn redo(&mut self, handle: Handle) -> Result<Transaction, Diagnostic> {
        self.writable(handle)?;
        self.documents.get_mut(&handle).unwrap().editor.redo()
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

    // ---------------------------------------------------------------- JSON

    /// The document's state without its content: `{ handle, path,
    /// relativePath, kind, id, name, formatVersion, revision, dirty,
    /// canUndo, canRedo, history, readOnly }`.
    pub fn state_json(&self, handle: Handle) -> Result<Value, Diagnostic> {
        let project = self.require_project()?;
        let doc = self.document(handle)?;
        let document = doc.editor.document();
        Ok(json!({
            "handle": handle,
            "path": doc.path.display().to_string(),
            "relativePath": project.relative_to_assets(&doc.path),
            "kind": document.kind.name(),
            "id": document.envelope.id.to_string(),
            "name": live_name(document),
            "formatVersion": document.envelope.format_version,
            "revision": doc.editor.revision().as_str(),
            "dirty": doc.editor.is_dirty(),
            "canUndo": doc.editor.can_undo(),
            "canRedo": doc.editor.can_redo(),
            "history": doc.editor.history().len(),
            "readOnly": project.read_only,
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
