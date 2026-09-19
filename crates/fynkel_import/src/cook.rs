//! Cooking (blueprint §6, §19.2c item 12): every asset in a database →
//! one cooked container (`fynkel_cooked`, `.fyna`) in the content-addressed
//! [`Cache`], for one target profile. Documents are validated and stored
//! as their canonical JSON; sources go through their importer and come out
//! as typed chunks (meshes, textures, clips, material and prefab
//! documents); `fynkel.file` sources are stored as they are.
//!
//! Dependencies are what a document references (`assetRef`s, resolved
//! against the database); a source depends on nothing but its own files.
//! Assets are cooked in dependency order and every dependency's key is
//! part of the dependent's key, so editing a texture recooks exactly the
//! material that uses it and the scene that uses that material — and
//! nothing else (the test in `tests/cook.rs` pins this down). A cycle, an
//! unresolved reference or a failed dependency fails the asset; the rest
//! of the database still cooks.
//!
//! The cooker never writes outside the cache: sidecars are not updated
//! (that is `fynkel import`), sources are not touched.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use fynkel_contracts::{has_errors, Code, Diagnostic};
use fynkel_cooked::{Writer, KIND_BLOB, KIND_CLIP, KIND_JSON, KIND_MESH, KIND_TEXTURE};
use fynkel_document::asset::{asset_refs, AssetDatabase, AssetEntry, AssetKind, Meta};
use fynkel_document::{json, Document, DocumentKind, SchemaSet};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::cache::{
    hash_bytes, Cache, CacheKey, KeyInputs, Manifest, ManifestChunk, ManifestDependency,
    ManifestInput, CACHE_VERSION,
};
use crate::intermediate::{AudioData, ColorSpace, Intermediate, MeshData, TextureData};
use crate::Limits;

/// The profile a cook targets when none is named.
pub const DEFAULT_PROFILE: &str = "dev";

/// Texture chunk formats (`TEXR` payload, `format` field).
pub use fynkel_cooked::payload::{
    decode_clip, decode_mesh, decode_texture, ClipPayload as ClipChunk, MeshPayload as MeshChunk,
    TexturePayload as TextureChunk, TEXTURE_FORMAT_RGBA8_LINEAR, TEXTURE_FORMAT_RGBA8_SRGB,
};

#[derive(Clone, Debug)]
pub struct CookOptions {
    /// Part of every key: cooking for `ios` and for `dev` never share an
    /// object even while their outputs are identical (item 16 makes them
    /// differ).
    pub profile: String,
    pub limits: Limits,
    /// Cook even when the object is already in the cache.
    pub force: bool,
}

impl Default for CookOptions {
    fn default() -> Self {
        CookOptions {
            profile: DEFAULT_PROFILE.to_string(),
            limits: Limits::default(),
            force: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The object was already in the cache under this key.
    Cached,
    /// Cooked and written on this run.
    Cooked,
    /// Not cooked; the report's diagnostics say why.
    Failed,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Cached => "cached",
            Status::Cooked => "cooked",
            Status::Failed => "failed",
        }
    }
}

/// One asset's outcome.
#[derive(Clone, Debug)]
pub struct CookedAsset {
    pub id: Uuid,
    /// Relative to the assets root.
    pub path: String,
    pub kind: String,
    /// `None` when the asset failed before its key could be computed
    /// (a dependency failed, or its own files could not be read).
    pub key: Option<CacheKey>,
    /// The object in the cache (present for `Cached` and `Cooked`).
    pub file: Option<PathBuf>,
    pub status: Status,
    /// Direct dependencies, sorted.
    pub dependencies: Vec<Uuid>,
    pub chunks: Vec<ManifestChunk>,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct CookReport {
    pub profile: String,
    /// In database (path) order.
    pub assets: Vec<CookedAsset>,
    pub diagnostics: Vec<Diagnostic>,
}

impl CookReport {
    pub fn has_errors(&self) -> bool {
        has_errors(&self.diagnostics)
    }

    pub fn count(&self, status: Status) -> usize {
        self.assets.iter().filter(|a| a.status == status).count()
    }

    pub fn get(&self, id: Uuid) -> Option<&CookedAsset> {
        self.assets.iter().find(|a| a.id == id)
    }

    pub fn json(&self) -> Value {
        let mut v = fynkel_document::diagnostics_json(&self.diagnostics);
        v["profile"] = self.profile.clone().into();
        v["cached"] = self.count(Status::Cached).into();
        v["cooked"] = self.count(Status::Cooked).into();
        v["failed"] = self.count(Status::Failed).into();
        v["assets"] = self
            .assets
            .iter()
            .map(asset_json)
            .collect::<Vec<_>>()
            .into();
        v
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        for a in &self.assets {
            out.push_str(&format!(
                "{:<7} {} ({}){}\n",
                a.status.name(),
                a.path,
                a.kind,
                match &a.key {
                    Some(k) => format!(" {} {} B", &k.hex()[..12], a.bytes),
                    None => String::new(),
                }
            ));
        }
        out.push_str(&format!(
            "profile {}: {} cooked, {} cached, {} failed\n",
            self.profile,
            self.count(Status::Cooked),
            self.count(Status::Cached),
            self.count(Status::Failed)
        ));
        out
    }
}

pub fn asset_json(a: &CookedAsset) -> Value {
    json!({
        "id": a.id.to_string(),
        "path": a.path,
        "kind": a.kind,
        "status": a.status.name(),
        "key": a.key.map(|k| k.hex()),
        "file": a.file.as_ref().map(|f| f.display().to_string()),
        "dependencies": a.dependencies.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
        "chunks": a.chunks.iter().map(|c| json!({
            "kind": c.kind, "name": c.name, "length": c.length, "crc32": c.crc32
        })).collect::<Vec<_>>(),
        "bytes": a.bytes,
    })
}

/// What is known about an asset before cooking: its own inputs and its
/// dependencies, or why it cannot be cooked.
struct Prepared {
    kind: String,
    version: u32,
    source: CacheKey,
    extra_inputs: Vec<(String, CacheKey)>,
    settings: String,
    dependencies: Vec<Uuid>,
    /// The loaded document (documents) or sidecar (sources).
    input: Input,
    failed: bool,
}

enum Input {
    Document(Document),
    Source { meta: Meta, bytes: Vec<u8> },
    None,
}

/// Cooks every asset of `db` into `cache` for `opts.profile` and writes
/// the cache's `index.json`.
pub fn cook(
    db: &AssetDatabase,
    schemas: &SchemaSet,
    cache: &Cache,
    opts: &CookOptions,
) -> CookReport {
    let mut diagnostics = Vec::new();
    let entries = db.entries();
    let by_id: HashMap<Uuid, usize> = entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();

    // 1. Inputs and dependencies for every asset.
    let prepared: Vec<Prepared> = entries
        .iter()
        .map(|e| prepare(db, schemas, e, &mut diagnostics))
        .collect();

    // 2. Dependency order: Tarjan's strongly connected components come
    // out dependencies first; a component with more than one member (or a
    // self-reference) is a cycle.
    let deps: Vec<Vec<usize>> = prepared
        .iter()
        .map(|p| {
            p.dependencies
                .iter()
                .filter_map(|d| by_id.get(d).copied())
                .collect()
        })
        .collect();
    let (order, in_cycle) = Tarjan::run(&deps);
    for (i, e) in entries.iter().enumerate() {
        if in_cycle[i] {
            diagnostics.push(
                Diagnostic::error(
                    Code::IMPORT_INVALID_DATA,
                    format!(
                        "{} is part of a reference cycle; it cannot be cooked",
                        e.path
                    ),
                )
                .with_document(e.file(db.root()).display().to_string())
                .with_id(e.id.to_string()),
            );
        }
    }

    // 3. Cook in that order; keys need the dependencies' keys.
    let mut keys: HashMap<Uuid, CacheKey> = HashMap::new();
    let mut results: Vec<Option<CookedAsset>> = (0..entries.len()).map(|_| None).collect();
    for i in order {
        let entry = &entries[i];
        let p = &prepared[i];
        let base = CookedAsset {
            id: entry.id,
            path: entry.path.clone(),
            kind: p.kind.clone(),
            key: None,
            file: None,
            status: Status::Failed,
            dependencies: p.dependencies.clone(),
            chunks: Vec::new(),
            bytes: 0,
        };
        if p.failed || in_cycle[i] {
            results[i] = Some(base);
            continue;
        }
        let mut dep_keys = Vec::with_capacity(p.dependencies.len());
        let mut blocked = false;
        for d in &p.dependencies {
            match keys.get(d) {
                Some(k) => dep_keys.push((*d, *k)),
                None => {
                    let dep_path = by_id
                        .get(d)
                        .map(|j| entries[*j].path.clone())
                        .unwrap_or_else(|| d.to_string());
                    diagnostics.push(
                        Diagnostic::error(
                            Code::IMPORT_SKIPPED,
                            format!("{}: not cooked because {dep_path} failed", entry.path),
                        )
                        .with_document(entry.file(db.root()).display().to_string())
                        .with_id(entry.id.to_string()),
                    );
                    blocked = true;
                    break;
                }
            }
        }
        if blocked {
            results[i] = Some(base);
            continue;
        }
        let inputs = KeyInputs {
            asset: entry.id,
            kind: p.kind.clone(),
            version: p.version,
            source: p.source,
            extra_inputs: p.extra_inputs.clone(),
            settings: p.settings.clone(),
            dependencies: dep_keys,
            profile: opts.profile.clone(),
        };
        let key = inputs.key();
        let mut result = CookedAsset {
            key: Some(key),
            file: Some(cache.object_path(&key)),
            ..base
        };
        if !opts.force && cache.has(&key) {
            match cache.read_manifest(&key) {
                Ok(m) => {
                    result.status = Status::Cached;
                    result.chunks = m.chunks;
                    result.bytes = m.bytes;
                    keys.insert(entry.id, key);
                    results[i] = Some(result);
                    continue;
                }
                // An unreadable manifest is a damaged cache entry: recook it.
                Err(d) => diagnostics.push(
                    Diagnostic::warning(d.code, d.message).with_document(
                        d.document
                            .unwrap_or_else(|| cache.manifest_path(&key).display().to_string()),
                    ),
                ),
            }
        }
        match cook_one(db, entry, p, &inputs, &key, opts, &mut diagnostics) {
            Some((bytes, manifest)) => match cache.write(&key, &bytes, &manifest) {
                Ok(()) => {
                    result.status = Status::Cooked;
                    result.chunks = manifest.chunks;
                    result.bytes = manifest.bytes;
                    keys.insert(entry.id, key);
                }
                Err(d) => {
                    diagnostics.push(d);
                    result.file = None;
                }
            },
            None => result.file = None,
        }
        results[i] = Some(result);
    }

    let report = CookReport {
        profile: opts.profile.clone(),
        assets: results
            .into_iter()
            .map(|r| r.expect("every asset visited"))
            .collect(),
        diagnostics,
    };
    let mut report = report;
    if let Err(d) = write_index(cache, &report) {
        report.diagnostics.push(d);
    }
    report
}

struct Tarjan<'a> {
    deps: &'a [Vec<usize>],
    index: Vec<Option<usize>>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next: usize,
    order: Vec<usize>,
    in_cycle: Vec<bool>,
}

impl<'a> Tarjan<'a> {
    fn run(deps: &'a [Vec<usize>]) -> (Vec<usize>, Vec<bool>) {
        let n = deps.len();
        let mut t = Tarjan {
            deps,
            index: vec![None; n],
            low: vec![0; n],
            on_stack: vec![false; n],
            stack: Vec::new(),
            next: 0,
            order: Vec::with_capacity(n),
            in_cycle: vec![false; n],
        };
        for v in 0..n {
            if t.index[v].is_none() {
                t.strong(v);
            }
        }
        (t.order, t.in_cycle)
    }

    fn strong(&mut self, v: usize) {
        self.index[v] = Some(self.next);
        self.low[v] = self.next;
        self.next += 1;
        self.stack.push(v);
        self.on_stack[v] = true;
        for &w in &self.deps[v] {
            if self.index[w].is_none() {
                self.strong(w);
                self.low[v] = self.low[v].min(self.low[w]);
            } else if self.on_stack[w] {
                self.low[v] = self.low[v].min(self.index[w].unwrap());
            }
        }
        if Some(self.low[v]) == self.index[v] {
            let mut component = Vec::new();
            loop {
                let w = self.stack.pop().expect("v is on the stack");
                self.on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            let cyclic = component.len() > 1 || self.deps[v].contains(&v);
            component.sort_unstable();
            for &w in &component {
                self.in_cycle[w] = cyclic;
            }
            self.order.extend(component);
        }
    }
}

fn prepare(
    db: &AssetDatabase,
    schemas: &SchemaSet,
    entry: &AssetEntry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Prepared {
    let file = entry.file(db.root());
    let shown = file.display().to_string();
    let mut failed = false;
    let io_fail = |e: std::io::Error| {
        Diagnostic::error(Code::IO, format!("cannot read: {e}")).with_document(shown.clone())
    };
    match &entry.kind {
        AssetKind::Document(kind) => {
            let mut dependencies = BTreeSet::new();
            let mut input = Input::None;
            let mut source = hash_bytes(b"");
            match std::fs::read(&file) {
                Ok(bytes) => {
                    source = hash_bytes(&bytes);
                    match Document::read(&file, Some(*kind)) {
                        Ok(doc) => {
                            let diags = doc.validate(schemas);
                            if has_errors(&diags) {
                                failed = true;
                            }
                            diagnostics.extend(diags);
                            let unresolved = db.check_references(&doc);
                            if has_errors(&unresolved) {
                                failed = true;
                            }
                            diagnostics.extend(unresolved);
                            for (_, r) in asset_refs(&doc.root) {
                                if r.is_nil() {
                                    continue;
                                }
                                if let Ok(resolved) = db.resolve(&r) {
                                    dependencies.insert(resolved.entry.id);
                                }
                            }
                            input = Input::Document(doc);
                        }
                        Err(diags) => {
                            diagnostics.extend(diags);
                            failed = true;
                        }
                    }
                }
                Err(e) => {
                    diagnostics.push(io_fail(e));
                    failed = true;
                }
            }
            let version = match &input {
                Input::Document(d) => d.envelope.format_version,
                _ => 0,
            };
            Prepared {
                kind: format!("document/{}", kind.name()),
                version,
                source,
                extra_inputs: Vec::new(),
                settings: "{}".to_string(),
                dependencies: dependencies.into_iter().collect(),
                input,
                failed,
            }
        }
        AssetKind::Source {
            importer,
            importer_version,
        } => {
            let sidecar = entry.sidecar(db.root()).expect("a source has a sidecar");
            let meta = match Document::read(&sidecar, Some(DocumentKind::Meta)) {
                Ok(doc) => {
                    let meta = Meta::from_document(doc);
                    let diags = meta.validate(schemas);
                    if has_errors(&diags) {
                        failed = true;
                    }
                    diagnostics.extend(diags);
                    Some(meta)
                }
                Err(diags) => {
                    diagnostics.extend(diags);
                    failed = true;
                    None
                }
            };
            let bytes = match std::fs::read(&file) {
                Ok(b) => b,
                Err(e) => {
                    diagnostics.push(io_fail(e));
                    failed = true;
                    Vec::new()
                }
            };
            let source = hash_bytes(&bytes);
            let extra_inputs = if is_gltf_json(&file) {
                gltf_extra_inputs(&file, &bytes)
            } else {
                Vec::new()
            };
            let settings = meta
                .as_ref()
                .map(|m| json::write(&m.settings))
                .unwrap_or_else(|| "{}".to_string());
            Prepared {
                kind: importer.clone(),
                version: *importer_version,
                source,
                extra_inputs,
                settings,
                dependencies: Vec::new(),
                input: match meta {
                    Some(meta) if !failed => Input::Source { meta, bytes },
                    _ => Input::None,
                },
                failed,
            }
        }
    }
}

fn is_gltf_json(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("gltf"))
}

/// The files a `.gltf` may read besides itself: every non-`data:`
/// `buffers[].uri` and `images[].uri`, hashed (a missing file hashes as
/// "missing", so its appearance later changes the key too). Whether the
/// importer is allowed to read them is its business; they are inputs
/// either way.
fn gltf_extra_inputs(path: &Path, bytes: &[u8]) -> Vec<(String, CacheKey)> {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    let mut names = BTreeSet::new();
    for list in ["buffers", "images"] {
        if let Some(items) = v.get(list).and_then(Value::as_array) {
            for item in items {
                if let Some(uri) = item.get("uri").and_then(Value::as_str) {
                    if !uri.starts_with("data:") && !uri.is_empty() {
                        names.insert(uri.to_string());
                    }
                }
            }
        }
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    names
        .into_iter()
        .map(|uri| {
            let plain = !uri.contains("://") && !uri.starts_with('/') && !uri.contains("..");
            let hash = if plain {
                match std::fs::read(dir.join(&uri)) {
                    Ok(b) => hash_bytes(&b),
                    Err(_) => hash_bytes(b"missing"),
                }
            } else {
                hash_bytes(b"denied")
            };
            (uri, hash)
        })
        .collect()
}

/// Cooks one asset to container bytes and its manifest.
fn cook_one(
    db: &AssetDatabase,
    entry: &AssetEntry,
    p: &Prepared,
    inputs: &KeyInputs,
    key: &CacheKey,
    opts: &CookOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(Vec<u8>, Manifest)> {
    let file = entry.file(db.root());
    let shown = file.display().to_string();
    let mut chunks: Vec<ChunkPlan> = Vec::new();
    let mut importer = None;
    let mut importer_version = None;
    let mut sub_assets: BTreeMap<String, Uuid> = BTreeMap::new();

    match &p.input {
        Input::Document(doc) => {
            let text = match doc.write() {
                Ok(t) => t,
                Err(d) => {
                    diagnostics.push(d.with_document(shown));
                    return None;
                }
            };
            chunks.push(ChunkPlan {
                kind: KIND_JSON,
                name: entry.name.clone(),
                sub: None,
                info: json!({ "documentKind": doc.kind.name() }),
                payload: text.into_bytes(),
            });
        }
        Input::Source { meta, bytes } => {
            importer = Some(meta.importer.clone());
            importer_version = Some(meta.importer_version);
            for (sub, id) in &meta.sub_assets {
                sub_assets.insert(sub.clone(), *id);
            }
            if meta.importer == fynkel_document::asset::FILE_IMPORTER {
                chunks.push(ChunkPlan {
                    kind: KIND_BLOB,
                    name: entry.name.clone(),
                    sub: None,
                    info: json!({ "extension": file.extension().and_then(|e| e.to_str()).unwrap_or("") }),
                    payload: bytes.clone(),
                });
            } else {
                let mut imported = crate::import(&file, meta, &opts.limits);
                let bad = imported.has_errors();
                diagnostics.append(&mut imported.diagnostics);
                let intermediate = imported.intermediate.take()?;
                if bad {
                    return None;
                }
                if imported.sidecar_changed(meta) {
                    diagnostics.push(
                        Diagnostic::warning(
                            Code::IMPORT_SKIPPED,
                            format!(
                                "{}: the sidecar's subAssets are behind the import ({} listed, {} produced); references to the missing ones will not resolve",
                                entry.path,
                                meta.sub_assets.len(),
                                imported.sub_assets.len()
                            ),
                        )
                        .with_document(shown.clone())
                        .with_suggestion(format!("run `fynkel import {}` to update the sidecar", entry.path)),
                    );
                }
                for (sub, id) in &intermediate.sub_assets {
                    sub_assets.entry(sub.clone()).or_insert(*id);
                }
                plan_intermediate(
                    &intermediate,
                    &entry.name,
                    &opts.limits,
                    &mut chunks,
                    diagnostics,
                    &shown,
                );
            }
        }
        Input::None => return None,
    }

    // The META chunk: everything a loader needs before touching a payload.
    let mut chunk_meta: Vec<Value> = Vec::with_capacity(chunks.len() + 1);
    chunk_meta.push(json!({ "index": 0, "kind": "META", "name": "meta" }));
    for (i, c) in chunks.iter().enumerate() {
        let mut m = json!({
            "index": i + 1,
            "kind": String::from_utf8_lossy(&c.kind),
            "name": c.name,
        });
        if let Some(sub) = &c.sub {
            m["sub"] = sub.clone().into();
            if let Some(id) = sub_assets.get(sub) {
                m["id"] = id.to_string().into();
            }
        }
        if let Some(obj) = c.info.as_object() {
            for (k, v) in obj {
                m[k] = v.clone();
            }
        }
        chunk_meta.push(m);
    }
    let meta_json = json!({
        "fynkel": "cooked",
        "version": fynkel_cooked::VERSION,
        "id": entry.id.to_string(),
        "name": entry.name,
        "path": entry.path,
        "kind": p.kind,
        "importer": importer,
        "importerVersion": importer_version,
        "profile": opts.profile,
        "key": key.hex(),
        "dependencies": p.dependencies.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
        "subAssets": sub_assets.iter().map(|(k, v)| (k.clone(), Value::from(v.to_string()))).collect::<serde_json::Map<_, _>>(),
        "chunks": chunk_meta,
    });
    let mut writer = Writer::new(
        serde_json::to_string(&meta_json)
            .expect("meta serializes")
            .as_bytes(),
    );
    for c in &chunks {
        writer.push(c.kind, &c.payload);
    }
    let bytes = writer.finish();

    // Read the result back: the table's checksums feed the manifest, and a
    // container that does not parse must never reach the cache.
    let container = match fynkel_cooked::Container::parse(&bytes) {
        Ok(c) => c,
        Err(e) => {
            diagnostics.push(
                Diagnostic::error(Code::IMPORT_INVALID_DATA, format!("cooked container: {e}"))
                    .with_document(shown),
            );
            return None;
        }
    };
    let manifest_chunks = container
        .chunks()
        .iter()
        .enumerate()
        .map(|(i, c)| ManifestChunk {
            kind: c.kind_str(),
            name: if i == 0 {
                "meta".to_string()
            } else {
                chunks[i - 1].name.clone()
            },
            length: c.length,
            crc32: c.crc32,
        })
        .collect();
    let manifest = Manifest {
        key: key.hex(),
        id: entry.id.to_string(),
        path: entry.path.clone(),
        name: entry.name.clone(),
        kind: p.kind.clone(),
        version: p.version,
        profile: opts.profile.clone(),
        source_hash: inputs.source.hex(),
        extra_inputs: inputs
            .extra_inputs
            .iter()
            .map(|(name, hash)| ManifestInput {
                name: name.clone(),
                hash: hash.hex(),
            })
            .collect(),
        settings: inputs.settings.clone(),
        dependencies: inputs
            .dependencies
            .iter()
            .map(|(id, key)| ManifestDependency {
                id: id.to_string(),
                key: key.hex(),
            })
            .collect(),
        chunks: manifest_chunks,
        bytes: bytes.len() as u64,
    };
    Some((bytes, manifest))
}

struct ChunkPlan {
    kind: [u8; 4],
    name: String,
    sub: Option<String>,
    /// Extra META fields for this chunk (dimensions, counts, …).
    info: Value,
    payload: Vec<u8>,
}

/// Lays an intermediate out as chunks: meshes, then materials, then
/// textures (decoded embedded images first, then the asset's own), then
/// clips, then the prefab.
fn plan_intermediate(
    im: &Intermediate,
    asset_name: &str,
    limits: &Limits,
    chunks: &mut Vec<ChunkPlan>,
    diagnostics: &mut Vec<Diagnostic>,
    shown: &str,
) {
    for mesh in &im.meshes {
        chunks.push(ChunkPlan {
            kind: KIND_MESH,
            name: mesh.name.clone(),
            sub: Some(mesh.sub_id.clone()),
            info: json!({
                "vertices": mesh.vertices.len(),
                "indices": mesh.indices.len(),
                "material": mesh.material,
            }),
            payload: encode_mesh(mesh),
        });
    }
    for material in &im.materials {
        chunks.push(ChunkPlan {
            kind: KIND_JSON,
            name: material.name.clone(),
            sub: Some(material.sub_id.clone()),
            info: json!({ "documentKind": "material" }),
            payload: serde_json::to_string(&material.document)
                .expect("material serializes")
                .into_bytes(),
        });
    }
    let usage = image_usage(im);
    for image in &im.images {
        let color_space = match usage.get(&image.sub_id) {
            Some((true, false)) | None => ColorSpace::Srgb,
            Some((false, true)) => ColorSpace::Linear,
            Some((true, true)) => {
                diagnostics.push(
                    Diagnostic::warning(
                        Code::IMPORT_SKIPPED,
                        format!(
                            "{}: used both as a colour and as a data texture; cooked as sRGB",
                            image.sub_id
                        ),
                    )
                    .with_document(shown.to_string()),
                );
                ColorSpace::Srgb
            }
            Some((false, false)) => ColorSpace::Srgb,
        };
        match crate::image::decode(&image.bytes, &image.name, color_space, limits) {
            Ok(texture) => chunks.push(texture_chunk(&texture, Some(image.sub_id.clone()))),
            Err(d) => diagnostics.push(
                Diagnostic::warning(
                    d.code,
                    format!("{}: {}; not cooked", image.sub_id, d.message),
                )
                .with_document(shown.to_string()),
            ),
        }
    }
    for texture in &im.textures {
        let mut c = texture_chunk(texture, texture.sub_id.clone());
        if texture.sub_id.is_none() {
            c.name = asset_name.to_string();
        }
        chunks.push(c);
    }
    for clip in &im.clips {
        chunks.push(ChunkPlan {
            kind: KIND_CLIP,
            name: if clip.sub_id.is_none() {
                asset_name.to_string()
            } else {
                clip.name.clone()
            },
            sub: clip.sub_id.clone(),
            info: json!({
                "sampleRate": clip.sample_rate,
                "channels": clip.channels,
                "frames": clip.frames(),
                "codec": clip.codec,
            }),
            payload: encode_clip(clip),
        });
    }
    if let Some(prefab) = &im.prefab {
        chunks.push(ChunkPlan {
            kind: KIND_JSON,
            name: prefab
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(asset_name)
                .to_string(),
            sub: Some("prefab".to_string()),
            info: json!({ "documentKind": "prefab", "entities": im.prefab_entity_count() }),
            payload: serde_json::to_string(prefab)
                .expect("prefab serializes")
                .into_bytes(),
        });
    }
}

/// image sub id → (used as colour, used as data), from the material
/// documents' texture slots. `baseColor` and `emissive` are colour; the
/// rest carry data.
fn image_usage(im: &Intermediate) -> HashMap<String, (bool, bool)> {
    let mut usage: HashMap<String, (bool, bool)> = HashMap::new();
    for material in &im.materials {
        let Some(slots) = material.document.get("textures").and_then(Value::as_object) else {
            continue;
        };
        for (slot, binding) in slots {
            let Some(sub) = binding
                .get("texture")
                .and_then(|t| t.get("sub"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let colour = matches!(slot.as_str(), "baseColor" | "emissive");
            let e = usage.entry(sub.to_string()).or_insert((false, false));
            if colour {
                e.0 = true;
            } else {
                e.1 = true;
            }
        }
    }
    usage
}

fn texture_chunk(t: &TextureData, sub: Option<String>) -> ChunkPlan {
    ChunkPlan {
        kind: KIND_TEXTURE,
        name: t.name.clone(),
        sub,
        info: json!({
            "width": t.width,
            "height": t.height,
            "colorSpace": t.color_space.name(),
            "sourceHasAlpha": t.source_has_alpha,
            "mipLevels": 1,
        }),
        payload: encode_texture(t),
    }
}

/// `MESH` (see `fynkel_cooked::payload`).
pub fn encode_mesh(mesh: &MeshData) -> Vec<u8> {
    fynkel_cooked::payload::encode_mesh(
        &mesh.vertices,
        &mesh.indices,
        mesh.bounds_min,
        mesh.bounds_max,
    )
}

/// `TEXR` (see `fynkel_cooked::payload`).
pub fn encode_texture(t: &TextureData) -> Vec<u8> {
    fynkel_cooked::payload::encode_texture(
        t.width,
        t.height,
        match t.color_space {
            ColorSpace::Srgb => TEXTURE_FORMAT_RGBA8_SRGB,
            ColorSpace::Linear => TEXTURE_FORMAT_RGBA8_LINEAR,
        },
        &t.rgba8,
    )
}

/// `CLIP` (see `fynkel_cooked::payload`).
pub fn encode_clip(clip: &AudioData) -> Vec<u8> {
    fynkel_cooked::payload::encode_clip(clip.sample_rate, clip.channels as u32, &clip.samples)
}

fn write_index(cache: &Cache, report: &CookReport) -> Result<(), Diagnostic> {
    let index = json!({
        "fynkel": "cache-index",
        "cacheVersion": CACHE_VERSION,
        "profile": report.profile,
        "assets": report.assets.iter().map(|a| json!({
            "id": a.id.to_string(),
            "path": a.path,
            "kind": a.kind,
            "status": a.status.name(),
            "key": a.key.map(|k| k.hex()),
            "file": a.file.as_ref().and_then(|f| f.strip_prefix(cache.root()).ok()).map(|f| f.to_string_lossy().replace('\\', "/")),
            "dependencies": a.dependencies.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    let path = cache.index_path();
    std::fs::create_dir_all(cache.root()).map_err(|e| {
        Diagnostic::error(Code::IO, format!("{e}"))
            .with_document(cache.root().display().to_string())
    })?;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&index).expect("index serializes"),
    )
    .map_err(|e| {
        Diagnostic::error(Code::IO, format!("{e}")).with_document(path.display().to_string())
    })
}
