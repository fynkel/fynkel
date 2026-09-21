//! Asset identity and the asset database (blueprint §19.2c item 8, §6
//! "Asset identity and import", ADR-0006).
//!
//! Every source file under a project's `assets/` directory carries a
//! `<file>.meta.json` sidecar ([`Meta`]) holding its stable id, importer and
//! sub-asset ids; documents (`.scene.json`, `.material.json`, ...) are their
//! own identity. The [`AssetDatabase`] is nothing but a scan of that tree —
//! there is no hidden state to lose — and answers "which file is this id"
//! and "does this `assetRef` resolve". Moving an asset means moving the file
//! together with its sidecar: the id, and every reference to it, survive.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use uuid::Uuid;

use crate::json::{Member, Node};
use crate::{Document, DocumentKind, SchemaSet};

/// Importer for files no specialised importer claims: copied verbatim.
pub const FILE_IMPORTER: &str = "fynkel.file";
pub const GLTF_IMPORTER: &str = "fynkel.gltf";
pub const FBX_IMPORTER: &str = "fynkel.fbx";
pub const IMAGE_IMPORTER: &str = "fynkel.image";
pub const AUDIO_IMPORTER: &str = "fynkel.audio";

/// The importer a fresh sidecar names, from the file extension. Importer
/// versions start at 1 and are owned by the importers (M2-10/11).
pub fn importer_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "gltf" | "glb" => GLTF_IMPORTER,
        "fbx" => FBX_IMPORTER,
        "png" | "jpg" | "jpeg" => IMAGE_IMPORTER,
        "wav" | "ogg" => AUDIO_IMPORTER,
        _ => FILE_IMPORTER,
    }
}

/// `<file>.meta.json` for a source file.
pub fn sidecar_path(source: &Path) -> PathBuf {
    let mut name = source.file_name().unwrap_or_default().to_os_string();
    name.push(".meta.json");
    source.with_file_name(name)
}

/// The source file a sidecar belongs to (`None` when the name is not a
/// sidecar name).
pub fn source_path(sidecar: &Path) -> Option<PathBuf> {
    let name = sidecar.file_name()?.to_str()?;
    let stem = name.strip_suffix(".meta.json")?;
    if stem.is_empty() {
        return None;
    }
    Some(sidecar.with_file_name(stem))
}

/// Typed view of a `.meta.json` sidecar.
#[derive(Clone, Debug)]
pub struct Meta {
    pub document: Document,
    pub importer: String,
    pub importer_version: u32,
    pub settings: Node,
    /// Sub-asset id → its stable uuid, in document order.
    pub sub_assets: Vec<(String, Uuid)>,
    parse_diagnostics: Vec<Diagnostic>,
}

impl Meta {
    /// A fresh sidecar for `source` (a new v4 id, the importer from the
    /// extension, empty settings and sub-assets).
    pub fn new(source: &Path) -> Meta {
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut document = Document::new(DocumentKind::Meta, &name);
        document
            .root
            .set("importer", Node::string(importer_for(source)));
        let importer = importer_for(source).to_string();
        Meta {
            document,
            importer,
            importer_version: 1,
            settings: Node::object(Vec::new()),
            sub_assets: Vec::new(),
            parse_diagnostics: Vec::new(),
        }
    }

    pub fn from_document(document: Document) -> Meta {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Meta {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!(
                        "expected a .meta.json sidecar, got {}",
                        document.kind.name()
                    ),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let malformed = |msg: &str, pointer: &str| {
            anchor(
                root,
                Diagnostic::error(Code::ASSET_MALFORMED_ENTRY, msg),
                pointer,
            )
        };
        let importer = match root.get("importer").and_then(Node::as_str) {
            Some(s) => s.to_string(),
            None => {
                diags.push(malformed("importer must be a string", "/importer"));
                String::new()
            }
        };
        let importer_version = match root.get("importerVersion").and_then(Node::as_i64) {
            Some(v) if v >= 1 && v <= u32::MAX as i64 => v as u32,
            _ => {
                diags.push(malformed(
                    "importerVersion must be a positive integer",
                    "/importerVersion",
                ));
                1
            }
        };
        let settings = match root.get("settings") {
            Some(n) if n.as_object().is_some() => n.clone(),
            Some(_) => {
                diags.push(malformed("settings must be an object", "/settings"));
                Node::object(Vec::new())
            }
            None => Node::object(Vec::new()),
        };
        let mut sub_assets = Vec::new();
        if let Some(n) = root.get("subAssets") {
            match n.as_object() {
                Some(members) => {
                    for m in members {
                        let pointer = format!("/subAssets/{}", crate::scene::escape(&m.key));
                        match m.value.as_str().map(crate::parse_uuid) {
                            Some(Ok(id)) => sub_assets.push((m.key.clone(), id)),
                            _ => diags.push(malformed("sub-asset id must be a uuid", &pointer)),
                        }
                    }
                }
                None => diags.push(malformed("subAssets must be an object", "/subAssets")),
            }
        }
        for d in &mut diags {
            if d.document.is_none() {
                d.document = document.path.clone();
            }
        }
        Meta {
            document,
            importer,
            importer_version,
            settings,
            sub_assets,
            parse_diagnostics: diags,
        }
    }

    pub fn id(&self) -> Uuid {
        self.document.envelope.id
    }

    /// Schema validation plus the sidecar's own rules.
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for d in &self.parse_diagnostics {
            if !d.pointer.as_ref().is_some_and(|p| pointers.contains(p)) {
                out.push(d.clone());
            }
        }
        // A sub-asset may not reuse the asset's own id or another sub's.
        let mut seen: Vec<Uuid> = vec![self.id()];
        for (sub, id) in &self.sub_assets {
            let pointer = format!("/subAssets/{}", crate::scene::escape(sub));
            if seen.contains(id) {
                out.push(
                    anchor(
                        &self.document.root,
                        Diagnostic::error(
                            Code::ASSET_DUPLICATE_ID,
                            format!("sub-asset \"{sub}\" reuses id {id}"),
                        )
                        .with_id(id.to_string()),
                        &pointer,
                    )
                    .with_document(self.document.path.clone().unwrap_or_default()),
                );
            }
            seen.push(*id);
        }
        out
    }

    /// The sidecar as a document, `sub_assets` written back in order.
    pub fn to_document(&self) -> Document {
        let mut doc = self.document.clone();
        doc.root.set("importer", Node::string(&self.importer));
        doc.root.set(
            "importerVersion",
            Node::integer(self.importer_version as i64),
        );
        doc.root.set("settings", self.settings.clone());
        doc.root.set(
            "subAssets",
            Node::object(
                self.sub_assets
                    .iter()
                    .map(|(k, v)| (k.clone(), Node::string(v.to_string())))
                    .collect(),
            ),
        );
        doc
    }
}

/// What an id in the database stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// A Fynkel document; its envelope id is the asset id.
    Document(DocumentKind),
    /// A source file with a sidecar.
    Source {
        importer: String,
        importer_version: u32,
    },
}

impl AssetKind {
    pub fn name(&self) -> String {
        match self {
            AssetKind::Document(k) => k.name().to_string(),
            AssetKind::Source { importer, .. } => importer.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    pub id: Uuid,
    /// Relative to the database root, `/`-separated.
    pub path: String,
    pub name: String,
    pub kind: AssetKind,
    /// Sub-asset id → uuid: a source's from its sidecar, a rig document's
    /// derived from its clips (`prefab`, `anim/<name>`); empty for the
    /// other documents.
    pub sub_assets: BTreeMap<String, Uuid>,
}

impl AssetEntry {
    /// Absolute path of the source file / document.
    pub fn file(&self, root: &Path) -> PathBuf {
        root.join(&self.path)
    }

    /// Absolute path of the sidecar (sources only).
    pub fn sidecar(&self, root: &Path) -> Option<PathBuf> {
        match self.kind {
            AssetKind::Source { .. } => Some(sidecar_path(&self.file(root))),
            AssetKind::Document(_) => None,
        }
    }
}

/// A resolved `assetRef`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved<'a> {
    pub entry: &'a AssetEntry,
    /// The sub-asset when the reference (or the id itself) names one.
    pub sub: Option<(String, Uuid)>,
}

/// Every asset under one directory, by id and by path. Rebuilt from the
/// files on every [`AssetDatabase::scan`]; never persisted.
#[derive(Clone, Debug, Default)]
pub struct AssetDatabase {
    root: PathBuf,
    /// Sorted by path.
    entries: Vec<AssetEntry>,
    by_id: HashMap<Uuid, usize>,
    /// Sub-asset uuid → (entry, sub id).
    subs: HashMap<Uuid, (usize, String)>,
}

impl AssetDatabase {
    /// Scans `root` (recursively, hidden files and directories skipped,
    /// paths in sorted order). Sidecars are validated against `schemas`;
    /// documents are read for their envelope only — `fynkel validate`
    /// (M2-13) checks their bodies. A duplicate id keeps the first path
    /// (`FYN-A0003` on the second); a file whose envelope does not parse is
    /// reported and left out.
    pub fn scan(root: &Path, schemas: &SchemaSet) -> (AssetDatabase, Vec<Diagnostic>) {
        let mut db = AssetDatabase {
            root: root.to_path_buf(),
            ..Default::default()
        };
        let mut diags = Vec::new();
        let mut files = Vec::new();
        if let Err(e) = collect(root, &mut files) {
            diags.push(
                Diagnostic::error(Code::IO, format!("cannot scan assets: {e}"))
                    .with_document(root.display().to_string()),
            );
            return (db, diags);
        }
        files.sort();

        for path in &files {
            let shown = path.display().to_string();
            let rel = relative(root, path);
            if let Some(source) = source_path(path) {
                if !source.is_file() {
                    diags.push(
                        Diagnostic::error(
                            Code::ASSET_ORPHANED_SIDECAR,
                            format!(
                                "sidecar has no source file ({} is missing)",
                                relative(root, &source)
                            ),
                        )
                        .with_document(shown)
                        .with_suggestion(
                            "move the sidecar with its file, or delete it if the file is gone",
                        ),
                    );
                    continue;
                }
                let meta = match Document::read(path, Some(DocumentKind::Meta)) {
                    Ok(doc) => Meta::from_document(doc),
                    Err(d) => {
                        diags.extend(d);
                        continue;
                    }
                };
                let own = meta.validate(schemas);
                let ok = !fynkel_contracts::has_errors(&own);
                diags.extend(own);
                if !ok {
                    continue;
                }
                let entry = AssetEntry {
                    id: meta.id(),
                    path: relative(root, &source),
                    name: meta.document.envelope.name.clone(),
                    kind: AssetKind::Source {
                        importer: meta.importer.clone(),
                        importer_version: meta.importer_version,
                    },
                    sub_assets: meta.sub_assets.iter().cloned().collect(),
                };
                db.insert(
                    entry,
                    &meta.sub_assets,
                    &shown,
                    &meta.document.root,
                    &mut diags,
                );
            } else if let Some(kind) = DocumentKind::from_path(path) {
                let doc = match Document::read(path, Some(kind)) {
                    Ok(doc) => doc,
                    Err(d) => {
                        diags.extend(d);
                        continue;
                    }
                };
                // A rig exposes the skeleton's hierarchy and its named
                // clips as its own sub-assets (M3-19b), with ids derived
                // from the document id, so they resolve like a source's.
                let subs = if kind == DocumentKind::Rig {
                    crate::rig::Rig::from_document(doc.clone()).sub_assets()
                } else {
                    Vec::new()
                };
                let entry = AssetEntry {
                    id: doc.envelope.id,
                    path: rel,
                    name: doc.envelope.name.clone(),
                    kind: AssetKind::Document(kind),
                    sub_assets: subs.iter().cloned().collect(),
                };
                db.insert(entry, &subs, &shown, &doc.root, &mut diags);
            } else if !sidecar_path(path).is_file() {
                diags.push(
                    Diagnostic::error(
                        Code::ASSET_MISSING_SIDECAR,
                        "source file has no .meta.json sidecar",
                    )
                    .with_document(shown)
                    .with_suggestion(format!(
                        "run `fynkel asset init {rel}` to give it an id, or move it out of assets/"
                    )),
                );
            }
        }
        (db, diags)
    }

    fn insert(
        &mut self,
        entry: AssetEntry,
        subs: &[(String, Uuid)],
        shown: &str,
        root: &Node,
        diags: &mut Vec<Diagnostic>,
    ) {
        let mut claims: Vec<(Uuid, String, Option<String>)> =
            vec![(entry.id, "/id".to_string(), None)];
        for (sub, id) in subs {
            claims.push((
                *id,
                format!("/subAssets/{}", crate::scene::escape(sub)),
                Some(sub.clone()),
            ));
        }
        for (id, pointer, _) in &claims {
            if let Some(other) = self.owner_path(*id) {
                diags.push(
                    anchor(
                        root,
                        Diagnostic::error(
                            Code::ASSET_DUPLICATE_ID,
                            format!("id {id} is already used by {other}"),
                        )
                        .with_id(id.to_string())
                        .with_suggestion(
                            "a copied asset needs a new id: delete this sidecar and run `fynkel asset init` on the copy",
                        ),
                        pointer,
                    )
                    .with_document(shown.to_string()),
                );
                return;
            }
        }
        let index = self.entries.len();
        for (id, _, sub) in claims {
            match sub {
                None => {
                    self.by_id.insert(id, index);
                }
                Some(sub) => {
                    self.subs.insert(id, (index, sub));
                }
            }
        }
        self.entries.push(entry);
    }

    fn owner_path(&self, id: Uuid) -> Option<String> {
        if let Some(i) = self.by_id.get(&id) {
            return Some(self.entries[*i].path.clone());
        }
        self.subs
            .get(&id)
            .map(|(i, sub)| format!("{}#{}", self.entries[*i].path, sub))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries in path order.
    pub fn entries(&self) -> &[AssetEntry] {
        &self.entries
    }

    pub fn get(&self, id: Uuid) -> Option<&AssetEntry> {
        self.by_id.get(&id).map(|i| &self.entries[*i])
    }

    /// The entry for a path relative to the root (`/`-separated).
    pub fn by_path(&self, path: &str) -> Option<&AssetEntry> {
        self.entries.iter().find(|e| e.path == path)
    }

    /// Resolves a reference: the asset by id, then `sub` against its
    /// declared sub-assets. An id that is itself a sub-asset's uuid resolves
    /// to that sub-asset (with an empty `sub`).
    pub fn resolve(&self, r: &AssetRef) -> Result<Resolved<'_>, Diagnostic> {
        let asset = Uuid::from_bytes(r.asset.0);
        if let Some(entry) = self.get(asset) {
            if r.sub.is_empty() {
                return Ok(Resolved { entry, sub: None });
            }
            return match entry.sub_assets.get(&r.sub) {
                Some(id) => Ok(Resolved {
                    entry,
                    sub: Some((r.sub.clone(), *id)),
                }),
                None => Err(Diagnostic::error(
                    Code::ASSET_UNKNOWN_SUB,
                    format!("{} ({}) has no sub-asset \"{}\"", entry.path, asset, r.sub),
                )
                .with_id(r.to_string())
                .with_suggestion(if entry.sub_assets.is_empty() {
                    "the asset declares no sub-assets; import it first".to_string()
                } else {
                    format!(
                        "declared: {}",
                        entry
                            .sub_assets
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })),
            };
        }
        if let Some((i, sub)) = self.subs.get(&asset) {
            let entry = &self.entries[*i];
            if r.sub.is_empty() {
                return Ok(Resolved {
                    entry,
                    sub: Some((sub.clone(), asset)),
                });
            }
            return Err(Diagnostic::error(
                Code::ASSET_UNKNOWN_SUB,
                format!(
                    "{} is the sub-asset {}#{}; it has no sub-asset \"{}\" of its own",
                    asset, entry.path, sub, r.sub
                ),
            )
            .with_id(r.to_string()));
        }
        Err(
            Diagnostic::error(Code::ASSET_UNRESOLVED, format!("no asset with id {asset}"))
                .with_id(r.to_string())
                .with_suggestion(format!(
                    "{} asset(s) under {}; the file may be missing or its sidecar lost",
                    self.entries.len(),
                    self.root.display()
                )),
        )
    }

    /// Checks every `assetRef` in a document (any object shaped
    /// `{ "asset": "<uuid>", "sub"?: "<id>" }`, nil ids meaning "unset"
    /// skipped) against the database. Diagnostics are anchored at the
    /// reference.
    pub fn check_references(&self, doc: &Document) -> Vec<Diagnostic> {
        let mut refs = Vec::new();
        collect_refs(&doc.root, String::new(), &mut refs);
        let mut out = Vec::new();
        for (pointer, r) in refs {
            if r.is_nil() {
                continue;
            }
            if let Err(d) = self.resolve(&r) {
                let d = anchor(&doc.root, d, &pointer);
                out.push(match &doc.path {
                    Some(p) => d.with_document(p.clone()),
                    None => d,
                });
            }
        }
        out
    }
}

/// Every `assetRef`-shaped object in a tree with its pointer.
pub fn asset_refs(root: &Node) -> Vec<(String, AssetRef)> {
    let mut refs = Vec::new();
    collect_refs(root, String::new(), &mut refs);
    refs
}

fn collect_refs(node: &Node, pointer: String, out: &mut Vec<(String, AssetRef)>) {
    if let Some(members) = node.as_object() {
        if let Some(r) = as_asset_ref(members) {
            out.push((pointer, r));
            return;
        }
        for m in members {
            collect_refs(
                &m.value,
                format!("{pointer}/{}", crate::scene::escape(&m.key)),
                out,
            );
        }
    } else if let Some(items) = node.as_array() {
        for (i, item) in items.iter().enumerate() {
            collect_refs(item, format!("{pointer}/{i}"), out);
        }
    }
}

fn as_asset_ref(members: &[Member]) -> Option<AssetRef> {
    let mut asset = None;
    let mut sub = String::new();
    for m in members {
        match m.key.as_str() {
            "asset" => {
                let id = crate::parse_uuid(m.value.as_str()?).ok()?;
                asset = Some(fynkel_contracts::Uuid::from_bytes(*id.as_bytes()));
            }
            "sub" => sub = m.value.as_str()?.to_string(),
            _ => return None,
        }
    }
    Some(AssetRef { asset: asset?, sub })
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
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    match root.pointer_span(pointer) {
        Some(s) => d.at(s.start.line, s.start.column),
        None => d,
    }
}

/// `fynkel asset list --json` entry shape.
pub fn entry_json(e: &AssetEntry) -> serde_json::Value {
    let mut v = serde_json::json!({
        "id": e.id.to_string(),
        "path": e.path,
        "name": e.name,
        "kind": match &e.kind {
            AssetKind::Document(_) => "document",
            AssetKind::Source { .. } => "source",
        },
    });
    match &e.kind {
        AssetKind::Document(k) => v["document"] = k.name().into(),
        AssetKind::Source {
            importer,
            importer_version,
        } => {
            v["importer"] = importer.as_str().into();
            v["importerVersion"] = (*importer_version).into();
            v["subAssets"] = e
                .sub_assets
                .iter()
                .map(|(k, id)| (k.clone(), serde_json::Value::String(id.to_string())))
                .collect::<serde_json::Map<_, _>>()
                .into();
        }
    }
    v
}

pub fn resolved_json(r: &Resolved<'_>) -> serde_json::Value {
    let mut v = entry_json(r.entry);
    if let Some((sub, id)) = &r.sub {
        v["sub"] = serde_json::json!({ "id": sub, "uuid": id.to_string() });
    }
    v
}
