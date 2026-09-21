//! The open project: everything the session knows about a project
//! directory besides its open documents — the project file, the lock
//! (and whether it makes the project read-only), the schema set with the
//! project's behaviour schemas, the asset database with the references
//! every document makes, the shader manifests a material may name, the
//! committed behaviour metadata and the raw schema sources the Inspector
//! reads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use fynkel_document::asset::{asset_refs, entry_json};
use fynkel_document::{
    AssetDatabase, AssetKind, Document, DocumentKind, Lock, Project, SchemaSet, ShaderLibrary,
    ShaderManifest,
};
use serde_json::{json, Value};
use uuid::Uuid;

/// `<project>/lib/generated/behaviours.g.json`: what `fynkel codegen`
/// writes about the project's behaviours (ADR-0012), read here as data.
pub const METADATA_FILE: &str = "lib/generated/behaviours.g.json";

/// One `assetRef` inside a document on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    /// The referencing document's id.
    pub document: Uuid,
    /// The referencing document's path, relative to the asset root.
    pub path: String,
    /// RFC 6901 pointer of the reference inside the document.
    pub pointer: String,
    pub asset: AssetRef,
}

impl Reference {
    pub fn json(&self) -> Value {
        let mut v = json!({
            "document": self.document.to_string(),
            "path": self.path,
            "pointer": self.pointer,
            "asset": self.asset.asset.to_string(),
        });
        if !self.asset.sub.is_empty() {
            v["sub"] = self.asset.sub.clone().into();
        }
        v
    }
}

pub struct OpenProject {
    pub dir: PathBuf,
    pub project: Project,
    /// The lock when it parsed.
    pub lock: Option<Lock>,
    /// Why the project is read-only (the lock was written by a newer
    /// Fynkel), or `None` when it is editable.
    pub read_only: Option<String>,
    /// The embedded schemas plus the project's own.
    pub schemas: SchemaSet,
    pub assets: AssetDatabase,
    /// Every shader manifest document under the asset root (a material
    /// may name them; rebuilt into a library per open document).
    shader_docs: Vec<Document>,
    /// Every reference every document under the asset root makes.
    pub references: Vec<Reference>,
    /// `behaviours.g.json` when the project has one.
    pub metadata: Option<Value>,
    /// Schema JSON by type id: the embedded component schemas and the
    /// project's `schemas/` files whose `x-fynkel.kind` is `component` or
    /// `behaviour`.
    pub schema_sources: HashMap<String, Value>,
    /// What opening the project reported: the project file's own
    /// diagnostics, the lock check, the metadata and schema reads, then
    /// the asset scan and the project's checks against the database (the
    /// last two replaced on every rescan).
    pub diagnostics: Vec<Diagnostic>,
    /// The part of `diagnostics` a rescan keeps.
    base_diagnostics: Vec<Diagnostic>,
}

impl OpenProject {
    /// Opens the project at `dir`. `Err` only when there is no usable
    /// project file or asset root (`FYN-P` diagnostics); everything else
    /// — a missing or mismatched lock, sidecar problems, unreadable
    /// metadata — is reported in `diagnostics` and the project opens.
    pub fn open(dir: &Path) -> Result<OpenProject, Vec<Diagnostic>> {
        let dir = dir
            .canonicalize()
            .map_err(|e| vec![io_error(dir, format!("cannot open project directory: {e}"))])?;
        let project = Project::read(&dir)?;
        let mut diagnostics: Vec<Diagnostic> = project.read_diagnostics().to_vec();
        let base = SchemaSet::embedded();
        diagnostics.extend(project.validate(&base));
        let schemas = project.schemas(&base, &dir).map_err(|d| vec![d])?;

        let expected = fynkel_import::expected_lock();
        let lock_path = dir.join(fynkel_document::project::LOCK_FILE);
        let mut lock = None;
        let mut read_only = None;
        match Lock::read(&dir, &schemas) {
            Ok(l) => {
                diagnostics.extend(l.check(&expected, Some(&lock_path.display().to_string())));
                read_only = newer_than_this_build(&l, &expected);
                lock = Some(l);
            }
            Err(diags) => diagnostics.extend(diags),
        }

        let assets_dir = project.assets_dir(&dir);
        if !assets_dir.is_dir() {
            return Err(vec![Diagnostic::error(
                Code::PROJECT_ASSET_ROOT,
                format!("asset root {} is not a directory", assets_dir.display()),
            )
            .with_document(
                dir.join(fynkel_document::project::PROJECT_FILE)
                    .display()
                    .to_string(),
            )
            .with_pointer("/assets")]);
        }
        let mut this = OpenProject {
            dir: dir.clone(),
            project,
            lock,
            read_only,
            schemas,
            assets: AssetDatabase::default(),
            shader_docs: Vec::new(),
            references: Vec::new(),
            metadata: None,
            schema_sources: HashMap::new(),
            diagnostics: Vec::new(),
            base_diagnostics: diagnostics,
        };
        this.read_metadata();
        this.read_schema_sources();
        this.rescan(&[]);
        Ok(this)
    }

    /// The asset root.
    pub fn assets_dir(&self) -> PathBuf {
        self.project.assets_dir(&self.dir)
    }

    /// The shader library a material document is validated against: the
    /// built-ins plus every manifest under the asset root that validates.
    pub fn shaders(&self) -> ShaderLibrary {
        let mut shaders = ShaderLibrary::builtin();
        for doc in &self.shader_docs {
            let manifest = ShaderManifest::from_document(doc.clone());
            if !fynkel_contracts::has_errors(&manifest.validate(&self.schemas)) {
                shaders.add(manifest);
            }
        }
        shaders
    }

    /// Rescans the asset root and refreshes the reference index for the
    /// documents at `paths` (absolute, or relative to the project
    /// directory; every document when empty). Returns the scan's
    /// diagnostics, which also replace the scan part of
    /// [`OpenProject::diagnostics`].
    pub fn rescan(&mut self, paths: &[PathBuf]) -> Vec<Diagnostic> {
        let assets_dir = self.assets_dir();
        let (assets, scan) = AssetDatabase::scan(&assets_dir, &self.schemas);
        self.assets = assets;
        let mut all = self.base_diagnostics.clone();
        all.extend(scan.iter().cloned());
        all.extend(self.project.validate_against(&self.assets, &self.schemas));
        self.diagnostics = all;
        if paths.is_empty() {
            self.index_documents(None);
        } else {
            let rel: Vec<String> = paths
                .iter()
                .filter_map(|p| self.relative_to_assets(p))
                .collect();
            self.index_documents(Some(&rel));
        }
        scan
    }

    /// `path` (absolute, or relative to the project directory) relative to
    /// the asset root, `/`-separated; `None` outside it.
    pub fn relative_to_assets(&self, path: &Path) -> Option<String> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.dir.join(path)
        };
        let abs = abs.canonicalize().unwrap_or(abs);
        let root = self.assets_dir();
        let root = root.canonicalize().unwrap_or(root);
        let rel = abs.strip_prefix(&root).ok()?;
        Some(
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// The documents that reference `asset` (the id of an asset or of a
    /// sub-asset), from the files on disk.
    pub fn dependents(&self, asset: Uuid) -> Vec<&Reference> {
        let sub_of = self
            .assets
            .resolve(&AssetRef::new(to_contract(asset)))
            .ok()
            .and_then(|r| r.sub.map(|(sub, _)| (r.entry.id, sub)));
        self.references
            .iter()
            .filter(|r| {
                let id = from_contract(r.asset.asset);
                id == asset
                    || sub_of
                        .as_ref()
                        .is_some_and(|(entry, sub)| id == *entry && r.asset.sub == *sub)
            })
            .collect()
    }

    /// The database entries as `fynkel asset list --json` prints them,
    /// optionally only one kind (`"document"` / `"source"`) or one document
    /// kind (`"scene"`, `"prefab"`, ...).
    pub fn assets_json(&self, kind: Option<&str>, document: Option<&str>) -> Vec<Value> {
        self.assets
            .entries()
            .iter()
            .filter(|e| {
                matches!(
                    (kind, &e.kind),
                    (None, _)
                        | (Some("document"), AssetKind::Document(_))
                        | (Some("source"), AssetKind::Source { .. })
                )
            })
            .filter(|e| match (document, &e.kind) {
                (None, _) => true,
                (Some(name), AssetKind::Document(k)) => k.name() == name,
                _ => false,
            })
            .map(entry_json)
            .collect()
    }

    /// The `fynkel project describe --json` shape plus what the session
    /// adds: the directory, read-only state, counts and the type lists.
    pub fn json(&self) -> Value {
        let mut v = fynkel_document::project::describe_json(
            &self.project,
            Some(&self.assets),
            self.lock.as_ref(),
        );
        v["dir"] = self.dir.display().to_string().into();
        v["assetsDir"] = self.assets_dir().display().to_string().into();
        v["readOnly"] = self.read_only.clone().into();
        v["hasMetadata"] = self.metadata.is_some().into();
        v["referenceCount"] = self.references.len().into();
        v["componentTypes"] = self
            .schemas
            .component_types()
            .iter()
            .map(|c| c.type_id.clone())
            .collect::<Vec<_>>()
            .into();
        v["behaviourTypes"] = self
            .schemas
            .behaviour_types()
            .iter()
            .map(|b| b.type_id.clone())
            .collect::<Vec<_>>()
            .into();
        v["diagnostics"] =
            fynkel_document::diagnostics_json(&self.diagnostics)["diagnostics"].clone();
        v["errors"] = self
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .count()
            .into();
        v
    }

    /// Component and behaviour types with their schemas, for the Inspector.
    pub fn schemas_json(&self) -> Value {
        json!({
            "components": self.schemas.component_types().iter().map(|c| json!({
                "type": c.type_id,
                "id": c.numeric_id,
                "authored": c.authored,
                "wireSize": c.wire_size,
                "allowed": self.project.allows_component(&c.type_id),
                "schema": self.schema_sources.get(&c.type_id),
            })).collect::<Vec<_>>(),
            "behaviours": self.schemas.behaviour_types().iter().map(|b| json!({
                "type": b.type_id,
                "version": b.version,
                "dart": b.dart,
                "hasSchema": b.has_schema,
                "allowed": self.project.allows_component(&b.type_id),
                "schema": self.schema_sources.get(&b.type_id),
            })).collect::<Vec<_>>(),
        })
    }

    /// One type's schema JSON (`title` = the type id), with its `x-fynkel`
    /// metadata and per-field hints as generated; `None` for an unknown
    /// type or a behaviour declared without a schema.
    pub fn schema(&self, type_id: &str) -> Option<&Value> {
        self.schema_sources.get(type_id)
    }

    // ------------------------------------------------------------ indexing

    /// Reads the documents under the asset root for their references and
    /// shader manifests — all of them, or the ones at the given relative
    /// paths (their previous entries dropped first).
    fn index_documents(&mut self, only: Option<&[String]>) {
        match only {
            None => {
                self.references.clear();
                self.shader_docs.clear();
            }
            Some(paths) => {
                self.references.retain(|r| !paths.contains(&r.path));
                let shader_paths: Vec<Option<String>> = self
                    .shader_docs
                    .iter()
                    .map(|d| {
                        d.path
                            .as_deref()
                            .and_then(|p| self.relative_to_assets(Path::new(p)))
                    })
                    .collect();
                let mut i = 0;
                self.shader_docs.retain(|_| {
                    let keep = shader_paths[i]
                        .as_ref()
                        .is_none_or(|rel| !paths.contains(rel));
                    i += 1;
                    keep
                });
            }
        }
        let root = self.assets.root().to_path_buf();
        let entries: Vec<(Uuid, String, DocumentKind)> = self
            .assets
            .entries()
            .iter()
            .filter_map(|e| match &e.kind {
                AssetKind::Document(k) => Some((e.id, e.path.clone(), *k)),
                AssetKind::Source { .. } => None,
            })
            .filter(|(_, path, _)| only.is_none_or(|paths| paths.contains(path)))
            .collect();
        for (id, path, kind) in entries {
            let Ok(doc) = Document::read(&root.join(&path), Some(kind)) else {
                // The scan already reported it.
                continue;
            };
            for (pointer, asset) in asset_refs(&doc.root) {
                if asset.is_nil() {
                    continue;
                }
                self.references.push(Reference {
                    document: id,
                    path: path.clone(),
                    pointer,
                    asset,
                });
            }
            if kind == DocumentKind::Shader {
                self.shader_docs.push(doc);
            }
        }
        self.references
            .sort_by(|a, b| (&a.path, &a.pointer).cmp(&(&b.path, &b.pointer)));
    }

    fn read_metadata(&mut self) {
        let diagnostics = &mut self.base_diagnostics;
        let path = self.dir.join(METADATA_FILE);
        let declares_behaviours = self
            .project
            .components
            .iter()
            .flatten()
            .any(|c| c.kind == "behaviour");
        if !path.is_file() {
            if declares_behaviours {
                diagnostics.push(
                    Diagnostic::warning(
                        Code::SESSION_IO,
                        format!(
                            "no {METADATA_FILE}: the Inspector has no metadata for the project's behaviours"
                        ),
                    )
                    .with_document(path.display().to_string())
                    .with_suggestion("run `fynkel codegen --project .` to generate it"),
                );
            }
            return;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
        {
            Ok(v) if v.is_object() => self.metadata = Some(v),
            Ok(_) => diagnostics.push(
                Diagnostic::warning(Code::SESSION_IO, "behaviour metadata is not a JSON object")
                    .with_document(path.display().to_string())
                    .with_suggestion("run `fynkel codegen --project .` to regenerate it"),
            ),
            Err(why) => diagnostics.push(
                Diagnostic::warning(
                    Code::SESSION_IO,
                    format!("cannot read behaviour metadata: {why}"),
                )
                .with_document(path.display().to_string())
                .with_suggestion("run `fynkel codegen --project .` to regenerate it"),
            ),
        }
    }

    fn read_schema_sources(&mut self) {
        for (_, text) in fynkel_document::generated::EMBEDDED_SCHEMAS {
            if let Ok(v) = serde_json::from_str::<Value>(text) {
                self.take_schema_source(v);
            }
        }
        let mut files = Vec::new();
        collect_schema_files(
            &self.dir.join(fynkel_document::runtime::PROJECT_SCHEMAS_DIR),
            &mut files,
        );
        files.sort();
        for file in files {
            match std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
            {
                Ok(v) => self.take_schema_source(v),
                Err(why) => self.base_diagnostics.push(
                    Diagnostic::warning(Code::SESSION_IO, format!("cannot read schema: {why}"))
                        .with_document(file.display().to_string()),
                ),
            }
        }
    }

    fn take_schema_source(&mut self, v: Value) {
        let kind = v
            .get("x-fynkel")
            .and_then(|m| m.get("kind"))
            .and_then(Value::as_str);
        if !matches!(kind, Some("component") | Some("behaviour")) {
            return;
        }
        let type_id = v
            .get("title")
            .and_then(Value::as_str)
            .or_else(|| {
                v.get("x-fynkel")
                    .and_then(|m| m.get("id"))
                    .and_then(Value::as_str)
            })
            .map(str::to_string);
        if let Some(type_id) = type_id {
            self.schema_sources.insert(type_id, v);
        }
    }
}

fn collect_schema_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_schema_files(&path, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".schema.json"))
        {
            out.push(path);
        }
    }
}

fn io_error(path: &Path, message: String) -> Diagnostic {
    Diagnostic::error(Code::SESSION_IO, message).with_document(path.display().to_string())
}

/// The reason the lock makes the project read-only: it names a runtime,
/// tool or lock format newer than this build (§6: a newer document is
/// read or refused, never rewritten). A lock that is merely *different*
/// (an older build, another importer set) is a `FYN-P0011` mismatch and
/// leaves the project editable.
fn newer_than_this_build(lock: &Lock, expected: &Lock) -> Option<String> {
    if lock.format_version > expected.format_version {
        return Some(format!(
            "the lock's formatVersion {} is newer than the supported {}",
            lock.format_version, expected.format_version
        ));
    }
    if lock.runtime.abi_version > expected.runtime.abi_version {
        return Some(format!(
            "the lock names ABI version {}; this build speaks {}",
            lock.runtime.abi_version, expected.runtime.abi_version
        ));
    }
    if version_newer(&lock.runtime.version, &expected.runtime.version) {
        return Some(format!(
            "the lock names runtime {}; this build is {}",
            lock.runtime.version, expected.runtime.version
        ));
    }
    if version_newer(&lock.tools.version, &expected.tools.version) {
        return Some(format!(
            "the lock names tools {}; this build is {}",
            lock.tools.version, expected.tools.version
        ));
    }
    None
}

/// `a` is a strictly newer dotted version than `b` (numeric segments,
/// missing segments 0; a segment that is not a number compares as text).
pub fn version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<(u64, String)> {
        s.split(['.', '-', '+'])
            .map(|seg| match seg.parse::<u64>() {
                Ok(n) => (n, String::new()),
                Err(_) => (u64::MAX, seg.to_string()),
            })
            .collect()
    };
    let (a, b) = (parse(a), parse(b));
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).cloned().unwrap_or((0, String::new()));
        let y = b.get(i).cloned().unwrap_or((0, String::new()));
        if x != y {
            return x > y;
        }
    }
    false
}

pub(crate) fn to_contract(id: Uuid) -> fynkel_contracts::Uuid {
    fynkel_contracts::Uuid::from_bytes(*id.as_bytes())
}

pub(crate) fn from_contract(id: fynkel_contracts::Uuid) -> Uuid {
    Uuid::from_bytes(id.0)
}
