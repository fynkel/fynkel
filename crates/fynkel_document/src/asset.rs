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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use uuid::Uuid;

use crate::json::{Locator, Member, Node, Value};
#[cfg(feature = "validate")]
use crate::SchemaSet;
use crate::{Document, DocumentKind};

/// Importer for files no specialised importer claims: copied verbatim.
pub const FILE_IMPORTER: &str = "fynkel.file";
pub const GLTF_IMPORTER: &str = "fynkel.gltf";
pub const FBX_IMPORTER: &str = "fynkel.fbx";
pub const IMAGE_IMPORTER: &str = "fynkel.image";
pub const AUDIO_IMPORTER: &str = "fynkel.audio";
/// `.shader` files (ADR-0031): the header read and the WGSL compiled.
pub const SHADER_IMPORTER: &str = "fynkel.shader";

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
        "png" | "jpg" | "jpeg" | "hdr" => IMAGE_IMPORTER,
        "wav" | "ogg" => AUDIO_IMPORTER,
        "shader" => SHADER_IMPORTER,
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
    #[cfg(feature = "validate")]
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

    /// The model's material remaps (`settings.materials`,
    /// `docs/design/material-remapping.md`): an embedded material's key →
    /// the project material its renderers in the model's hierarchy use
    /// instead, in document order. A nil reference means "the embedded
    /// one" and is left out, as is an entry that is not an `assetRef`
    /// ([`Meta::validate_remaps`] reports that).
    pub fn material_remaps(&self) -> Vec<(String, AssetRef)> {
        let Some(members) = self.settings.get("materials").and_then(Node::as_object) else {
            return Vec::new();
        };
        members
            .iter()
            .filter_map(|m| Some((m.key.clone(), as_asset_ref(m.value.as_object()?)?)))
            .collect()
    }

    /// Sets `settings.materials[key]` to the material document `target`,
    /// or removes the remap (`None`); a table left empty is removed.
    pub fn set_material_remap(&mut self, key: &str, target: Option<Uuid>) {
        let mut settings = self.settings.to_serde();
        let Some(obj) = settings.as_object_mut() else {
            return;
        };
        let mut table = match obj.remove("materials") {
            Some(serde_json::Value::Object(t)) => t,
            _ => serde_json::Map::new(),
        };
        match target {
            Some(id) => {
                table.insert(
                    key.to_string(),
                    serde_json::json!({ "asset": id.to_string() }),
                );
            }
            None => {
                table.remove(key);
            }
        }
        if !table.is_empty() {
            obj.insert("materials".into(), serde_json::Value::Object(table));
        }
        self.settings = Node::from_serde(&settings);
    }

    /// The remap table's own rules: an object of `assetRef`s (`A0006`),
    /// each naming a material document (`A0007`). A target the database
    /// cannot resolve is left to [`AssetDatabase::check_references`]
    /// (`A0004`/`A0005`).
    pub fn validate_remaps(&self, db: &AssetDatabase) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = Vec::new();
        let Some(table) = self.settings.get("materials") else {
            return out;
        };
        let Some(members) = table.as_object() else {
            out.push(anchor(
                root,
                Diagnostic::error(
                    Code::ASSET_MALFORMED_ENTRY,
                    "settings.materials must be an object of embedded material key → material reference",
                ),
                "/settings/materials",
            ));
            return out;
        };
        for m in members {
            let pointer = format!("/settings/materials/{}", crate::scene::escape(&m.key));
            let nil = m.value.get("asset").and_then(Node::as_str)
                == Some("00000000-0000-0000-0000-000000000000");
            if nil {
                continue;
            }
            let Some(r) = m.value.as_object().and_then(as_asset_ref) else {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::ASSET_MALFORMED_ENTRY,
                        format!("the remap of {} is not an asset reference", m.key),
                    )
                    .with_suggestion("`{ \"asset\": <id of a .material.json> }`"),
                    &pointer,
                ));
                continue;
            };
            let Ok(resolved) = db.resolve(&r) else {
                continue;
            };
            let material = resolved.entry.kind == AssetKind::Document(DocumentKind::Material);
            if !material || resolved.sub.is_some() {
                let what = match &resolved.sub {
                    Some((sub, _)) => format!("{}#{sub}", resolved.entry.path),
                    None => format!("{} ({})", resolved.entry.path, resolved.entry.kind.name()),
                };
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::ASSET_WRONG_KIND,
                        format!(
                            "{} is remapped to {what}, which is not a material document",
                            m.key
                        ),
                    )
                    .with_id(r.to_string())
                    .with_suggestion(
                        "remap to a project .material.json (Extract Materials makes editable copies of the embedded ones)",
                    ),
                    &pointer,
                ));
            }
        }
        for d in &mut out {
            if d.document.is_none() {
                d.document = self.document.path.clone();
            }
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
    /// Where the file really is when it is not under the root: a built-in
    /// asset the cook adds to a project that has none of its own
    /// ([`AssetDatabase::add_builtin`]); `path` is then where the project
    /// would keep it. `None` for every scanned asset.
    pub origin: Option<PathBuf>,
    /// A data asset's type id (ADR-0027), what a typed picker filters on;
    /// `None` for every other asset.
    pub data_type: Option<String>,
}

impl AssetEntry {
    /// Absolute path of the source file / document.
    pub fn file(&self, root: &Path) -> PathBuf {
        match &self.origin {
            Some(origin) => origin.clone(),
            None => root.join(&self.path),
        }
    }

    /// Absolute path of the sidecar (sources only).
    pub fn sidecar(&self, root: &Path) -> Option<PathBuf> {
        match self.kind {
            AssetKind::Source { .. } => Some(sidecar_path(&self.file(root))),
            AssetKind::Document(_) => None,
        }
    }
}

/// Where a game's package assets appear among its own (ADR-0029 §2, Unity's
/// Packages folder): `Packages/<package>/<path in its asset folder>`.
pub const PACKAGES_MOUNT: &str = "Packages";

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
    /// Entry path → index, so [`AssetDatabase::by_path`] is not a walk of
    /// every entry (the editor asks it once per reference on a refresh).
    by_path: HashMap<String, usize>,
    /// Sub-asset uuid → (entry, sub id).
    subs: HashMap<Uuid, (usize, String)>,
    /// Files a duplicate id kept out (`FYN-A0003`), re-read by
    /// [`AssetDatabase::update`] in case their owner is gone.
    rejected: Vec<PathBuf>,
}

impl AssetDatabase {
    /// Scans `root` (recursively, hidden files and directories skipped,
    /// paths in sorted order). Sidecars are validated against `schemas`;
    /// documents are read for their envelope only — `fynkel validate`
    /// (M2-13) checks their bodies. A duplicate id keeps the first path
    /// (`FYN-A0003` on the second); a file whose envelope does not parse is
    /// reported and left out.
    #[cfg(feature = "validate")]
    pub fn scan(root: &Path, schemas: &SchemaSet) -> (AssetDatabase, Vec<Diagnostic>) {
        Self::scan_preferring(root, schemas, &HashSet::new())
    }

    /// [`AssetDatabase::scan`] where the files at `known` (paths relative
    /// to the root, as [`AssetEntry::path`]) are read first: when two
    /// files claim one id — a copy made in Finder keeps its original's —
    /// the one the project knew keeps it, whichever sorts first
    /// (foundation 5.2). The other is `FYN-A0003`.
    #[cfg(feature = "validate")]
    pub fn scan_preferring(
        root: &Path,
        schemas: &SchemaSet,
        known: &HashSet<String>,
    ) -> (AssetDatabase, Vec<Diagnostic>) {
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
        if !known.is_empty() {
            let is_known = |path: &PathBuf| {
                let source = source_path(path).unwrap_or_else(|| path.clone());
                known.contains(&relative(root, &source))
            };
            // Stable: sorted order within each group.
            files.sort_by_key(|p| !is_known(p));
        }
        for path in &files {
            db.scan_file(path, schemas, &mut diags);
        }
        (db, diags)
    }

    /// The files a duplicate id kept out (`FYN-A0003`); the next
    /// [`AssetDatabase::update`] reads them again.
    pub fn rejected(&self) -> &[PathBuf] {
        &self.rejected
    }

    /// Every asset path of this database ([`AssetEntry::path`]): what a
    /// later [`AssetDatabase::scan_preferring`] keeps ids for.
    pub fn paths(&self) -> HashSet<String> {
        self.entries.iter().map(|e| e.path.clone()).collect()
    }

    /// The entry that owns `id`: the asset with that id, or the source
    /// with that sub-asset.
    pub fn owner(&self, id: Uuid) -> Option<&AssetEntry> {
        self.by_id
            .get(&id)
            .or_else(|| self.subs.get(&id).map(|(i, _)| i))
            .map(|i| &self.entries[*i])
    }

    /// Re-reads the files at `paths` (absolute, or relative to the root; a
    /// source or its sidecar names the pair, a directory names everything
    /// under it, a path that is gone drops what it held) and replaces their
    /// entries, leaving every other entry untouched — what a file watcher's
    /// report needs instead of a full [`AssetDatabase::scan`] (which reads
    /// every file: ~50 ms for 2 000 assets, against well under a
    /// millisecond here). Files a duplicate id kept out earlier are re-read
    /// too, so deleting the first owner of an id admits the second. Returns
    /// the diagnostics of the re-read files only; a caller that keeps the
    /// scan's diagnostics drops the ones anchored at these paths (the file,
    /// its sidecar, its source) first. One difference from a full scan: a
    /// duplicate id is judged against the entries already held, so the file
    /// that arrives later loses whatever its path sorts as.
    #[cfg(feature = "validate")]
    pub fn update(&mut self, paths: &[PathBuf], schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let mut files: Vec<PathBuf> = Vec::new();
        let mut drop_paths: HashSet<String> = HashSet::new();
        let mut drop_prefixes: Vec<String> = Vec::new();
        for p in paths {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                root.join(p)
            };
            let abs = canonical_or_parent(&abs);
            let Ok(rel) = abs.strip_prefix(&root) else {
                continue; // outside the asset root
            };
            let rel = relative(&root, &root.join(rel));
            if rel.is_empty() {
                continue;
            }
            // Hidden files and folders (`.DS_Store`, `.git`) are not
            // assets, as in a full scan.
            if rel.split('/').any(|c| c.starts_with('.')) {
                continue;
            }
            if abs.is_dir() {
                drop_prefixes.push(format!("{rel}/"));
                let _ = collect(&abs, &mut files);
                continue;
            }
            let (source, sidecar) = match source_path(&abs) {
                Some(source) => (source, abs.clone()),
                None => (abs.clone(), sidecar_path(&abs)),
            };
            drop_paths.insert(relative(&root, &source));
            if !abs.exists() {
                // A directory that is gone reports as a path with no file.
                drop_prefixes.push(format!("{rel}/"));
            }
            if source.is_file() {
                files.push(source);
            }
            if sidecar.is_file() {
                files.push(sidecar);
            }
        }
        files.extend(std::mem::take(&mut self.rejected));
        files.sort();
        files.dedup();
        for f in &files {
            drop_paths.insert(relative(
                &root,
                &source_path(f).unwrap_or_else(|| f.clone()),
            ));
        }
        self.entries.retain(|e| {
            !drop_paths.contains(&e.path) && !drop_prefixes.iter().any(|p| e.path.starts_with(p))
        });
        self.reindex();
        // `scan_file` relates paths to `self.root`; give it the canonical one
        // the files were resolved against.
        let kept_root = std::mem::replace(&mut self.root, root);
        for f in &files {
            self.scan_file(f, schemas, &mut diags);
        }
        self.root = kept_root;
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        self.reindex();
        diags
    }

    /// Adds the source `file` (with its sidecar beside it, outside the
    /// root) as if it were at `path` under the root: a built-in asset —
    /// `Fynkel/Primitives.glb` — the cook ships with a project that has not
    /// installed its own copy (ADR-0026 §4). Nothing when the id is taken
    /// already (the project's own copy wins); the sidecar's diagnostics
    /// otherwise.
    #[cfg(feature = "validate")]
    pub fn add_builtin(&mut self, file: &Path, path: &str, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let meta = match Document::read(&sidecar_path(file), Some(DocumentKind::Meta)) {
            Ok(doc) => Meta::from_document(doc),
            Err(d) => return d,
        };
        diags.extend(meta.validate(schemas));
        if fynkel_contracts::has_errors(&diags)
            || self.owner_path(meta.id()).is_some()
            || self.by_path.contains_key(path)
        {
            return diags;
        }
        let entry = AssetEntry {
            id: meta.id(),
            path: path.to_string(),
            name: meta.document.envelope.name.clone(),
            kind: AssetKind::Source {
                importer: meta.importer.clone(),
                importer_version: meta.importer_version,
            },
            sub_assets: meta.sub_assets.iter().cloned().collect(),
            origin: Some(file.to_path_buf()),
            data_type: None,
        };
        let shown = file.display().to_string();
        self.insert(
            file,
            entry,
            &meta.sub_assets,
            &shown,
            &meta.document.root,
            &mut diags,
        );
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        self.reindex();
        diags
    }

    /// Mounts `scanned` — a database of one package's asset folder
    /// (ADR-0029 §2) — at `Packages/<name>/`: each entry keeps its id and
    /// sub-assets, its path gains the mount and its `origin` is the file
    /// itself, as a built-in's is. An id already here (another package's)
    /// is `FYN-A0003` and keeps the first.
    pub fn mount(&mut self, name: &str, scanned: &AssetDatabase) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for entry in scanned.entries() {
            let file = entry.file(scanned.root());
            if let Some(owner) = self.owner_path(entry.id) {
                diags.push(
                    Diagnostic::error(
                        Code::ASSET_DUPLICATE_ID,
                        format!("id {} is already used by {owner}", entry.id),
                    )
                    .with_document(file.display().to_string()),
                );
                continue;
            }
            let mut mounted = entry.clone();
            mounted.path = format!("{}/{name}/{}", PACKAGES_MOUNT, entry.path);
            mounted.origin = Some(file);
            let index = self.entries.len();
            for (sub, id) in &mounted.sub_assets {
                self.subs.insert(*id, (index, sub.clone()));
            }
            self.by_id.insert(mounted.id, index);
            self.by_path.insert(mounted.path.clone(), index);
            self.entries.push(mounted);
        }
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        self.reindex();
        diags
    }

    /// This database with `other`'s entries (a game's package assets)
    /// whose id and path are free: the project's own copy of an asset wins
    /// (Import Package Assets keeps the ids, so the copy takes over every
    /// reference).
    pub fn merged_with(&self, other: &AssetDatabase) -> AssetDatabase {
        let mut out = self.clone();
        for entry in other.entries() {
            if out.owner_path(entry.id).is_some() || out.by_path.contains_key(&entry.path) {
                continue;
            }
            let mut entry = entry.clone();
            entry.origin = Some(entry.file(other.root()));
            out.entries.push(entry);
        }
        out.entries.sort_by(|a, b| a.path.cmp(&b.path));
        out.reindex();
        out
    }

    fn reindex(&mut self) {
        self.by_id.clear();
        self.by_path.clear();
        self.subs.clear();
        for (i, e) in self.entries.iter().enumerate() {
            self.by_id.insert(e.id, i);
            self.by_path.insert(e.path.clone(), i);
            for (sub, id) in &e.sub_assets {
                self.subs.insert(*id, (i, sub.clone()));
            }
        }
    }

    /// Reads one file of the tree: a sidecar makes its source's entry, a
    /// document its own; a source without a sidecar is reported.
    #[cfg(feature = "validate")]
    fn scan_file(&mut self, path: &Path, schemas: &SchemaSet, diags: &mut Vec<Diagnostic>) {
        let root = self.root.clone();
        let root = root.as_path();
        let shown = path.display().to_string();
        let rel = relative(root, path);
        if let Some(source) = source_path(path) {
            if !source.is_file() {
                // A warning (foundation 5.1): the file is gone, so its id
                // resolves to nothing and references to it load as none.
                diags.push(
                    Diagnostic::warning(
                        Code::ASSET_ORPHANED_SIDECAR,
                        format!(
                            "sidecar has no source file ({} is missing)",
                            relative(root, &source)
                        ),
                    )
                    .with_document(shown)
                    .with_suggestion(
                        "move the sidecar with its file, or delete it if the file is gone; until then references to it load as none",
                    ),
                );
                return;
            }
            let meta = match Document::read(path, Some(DocumentKind::Meta)) {
                Ok(doc) => Meta::from_document(doc),
                Err(d) => {
                    diags.extend(d);
                    return;
                }
            };
            let own = meta.validate(schemas);
            let ok = !fynkel_contracts::has_errors(&own);
            diags.extend(own);
            if !ok {
                return;
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
                origin: None,
                data_type: None,
            };
            self.insert(
                path,
                entry,
                &meta.sub_assets,
                &shown,
                &meta.document.root,
                diags,
            );
        } else if let Some(kind) = DocumentKind::from_path(path) {
            let doc = match Document::read(path, Some(kind)) {
                Ok(doc) => doc,
                Err(d) => {
                    diags.extend(d);
                    return;
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
                origin: None,
                data_type: (kind == DocumentKind::Data)
                    .then(|| {
                        doc.root
                            .get("type")
                            .and_then(|t| t.as_str())
                            .map(str::to_string)
                    })
                    .flatten(),
            };
            self.insert(path, entry, &subs, &shown, &doc.root, diags);
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

    fn insert(
        &mut self,
        file: &Path,
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
                self.rejected.push(file.to_path_buf());
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
        self.by_path.insert(entry.path.clone(), index);
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
        self.by_path.get(path).map(|i| &self.entries[*i])
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
        let at = Locator::new(&doc.root);
        for (pointer, r) in refs {
            if r.is_nil() {
                continue;
            }
            if let Err(d) = self.resolve(&r) {
                let d = at.anchor(d, &pointer);
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

/// Sets every `assetRef` in a tree that `missing` picks (given its
/// pointer) to the nil id, its `sub` dropped: how the cook writes a
/// reference to an asset that is gone or failed, so it loads as none, as
/// a missing reference does in Unity (foundation 5.1). Returns how many
/// it cleared.
pub fn clear_asset_refs(
    root: &mut Node,
    missing: &mut dyn FnMut(&str, &AssetRef) -> bool,
) -> usize {
    fn walk(
        node: &mut Node,
        pointer: String,
        missing: &mut dyn FnMut(&str, &AssetRef) -> bool,
    ) -> usize {
        match &mut node.value {
            Value::Object(members) => {
                if let Some(r) = as_asset_ref(members) {
                    if r.is_nil() || !missing(&pointer, &r) {
                        return 0;
                    }
                    members.retain(|m| m.key != "sub");
                    for m in members.iter_mut() {
                        if m.key == "asset" {
                            m.value = Node::string(Uuid::nil().to_string());
                        }
                    }
                    return 1;
                }
                let mut n = 0;
                for m in members.iter_mut() {
                    let at = format!("{pointer}/{}", crate::scene::escape(&m.key));
                    n += walk(&mut m.value, at, missing);
                }
                n
            }
            Value::Array(items) => {
                let mut n = 0;
                for (i, item) in items.iter_mut().enumerate() {
                    n += walk(item, format!("{pointer}/{i}"), missing);
                }
                n
            }
            _ => 0,
        }
    }
    walk(root, String::new(), missing)
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

/// The canonical path of `path`, or — for a path that is gone — its
/// canonical parent joined with its name, so a deleted file under a
/// symlinked root (macOS's `/tmp`) still compares with the root.
fn canonical_or_parent(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => canonical_or_parent(parent).join(name),
        _ => path.to_path_buf(),
    }
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
    // A package's asset (ADR-0029 §2): which package, and where it is.
    if let (Some(package), Some(file)) = (crate::packages::package_of(&e.path), &e.origin) {
        v["package"] = package.into();
        v["file"] = file.display().to_string().into();
    }
    match &e.kind {
        AssetKind::Document(k) => {
            v["document"] = k.name().into();
            if let Some(t) = &e.data_type {
                v["dataType"] = t.as_str().into();
            }
            if !e.sub_assets.is_empty() {
                v["subAssets"] = e
                    .sub_assets
                    .iter()
                    .map(|(k, id)| (k.clone(), serde_json::Value::String(id.to_string())))
                    .collect::<serde_json::Map<_, _>>()
                    .into();
            }
        }
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
