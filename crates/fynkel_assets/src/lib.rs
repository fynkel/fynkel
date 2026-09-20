//! `fynkel_assets` — what a running game reads (blueprint §6, §19.2d item 3).
//!
//! A host never opens a source document: scenes, prefabs, meshes, textures,
//! materials and clips come from the cache `fynkel cook` wrote
//! (`index.json` + one `.fyna` container per asset), by asset id. This crate
//! is that one door:
//!
//! - [`CacheIndex`] reads `index.json`: which asset is in which object, the
//!   profile, the build that cooked it, the project's startup scene.
//! - [`Assets`] is the shared, thread-safe **residency store** a world and
//!   its renderer both hold: payloads load on first use and stay resident
//!   while something retains their asset; [`Assets::retain`] hands out a
//!   [`Residency`] (a loaded scene's closure of dependencies) whose release
//!   evicts what nothing else retains.
//! - [`Assets::scene`] describes a cooked scene chunk (already expanded by
//!   the cooker) into a `SceneDescription` the core spawns, with the
//!   authored behaviours for the SDK.
//!
//! Every miss is a `FYN-R` diagnostic that names the asset — a missing or
//! stale object is a load error, never a fallback to source files.

// As in `fynkel_document`: a `Diagnostic` is the error type on purpose.
#![allow(clippy::result_large_err)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fynkel_contracts::{AssetRef, Code, Diagnostic, Uuid};
use fynkel_cooked::payload::{decode_mesh, decode_texture};
pub use fynkel_cooked::payload::{MeshPayload, TexturePayload};
use fynkel_cooked::{ChunkFile, KIND_JSON, KIND_MATERIAL, KIND_MESH, KIND_TEXTURE};
pub use fynkel_core::material::PbrMaterial;
use fynkel_core::SceneDescription;
pub use fynkel_document::project::{Quality, QualityProfile};
pub use fynkel_document::AuthoredBehaviour;
use fynkel_document::{describe, Document, DocumentKind, Scene, SchemaSet};
use fynkel_wire::material::decode_material;

/// Largest payload the store will read (per chunk).
pub const MAX_CHUNK_BYTES: u64 = 256 << 20;
const MAX_META_BYTES: u64 = 16 << 20;

/// The cache index's project record.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectInfo {
    pub name: String,
    pub startup_scene: Option<Uuid>,
    /// The project's `quality` profiles as cooked (`None`: the project
    /// declares none; the renderer uses the engine defaults).
    pub quality: Option<Quality>,
}

/// One asset as the index lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub id: Uuid,
    /// The authoring path, relative to the asset root (for messages).
    pub path: String,
    pub name: String,
    /// `document/<kind>` or the importer id.
    pub kind: String,
    /// `cooked`, `cached` or `failed`.
    pub status: String,
    pub key: Option<String>,
    /// The object, absolute; `None` when the cook failed.
    pub file: Option<PathBuf>,
    pub dependencies: Vec<Uuid>,
}

/// `<cache>/index.json`, read once.
#[derive(Clone, Debug)]
pub struct CacheIndex {
    root: PathBuf,
    profile: String,
    schema_fingerprint: String,
    project: Option<ProjectInfo>,
    entries: HashMap<Uuid, IndexEntry>,
}

fn parse_uuid(s: &str) -> Option<Uuid> {
    s.parse().ok()
}

impl CacheIndex {
    /// Reads the index and checks it was cooked by this build (schema
    /// fingerprint, cache version): a stale cache is `FYN-R0002`, a missing
    /// or malformed one `FYN-R0001`.
    pub fn open(cache_dir: &Path) -> Result<CacheIndex, Diagnostic> {
        let index_path = cache_dir.join("index.json");
        let shown = index_path.display().to_string();
        let text = std::fs::read_to_string(&index_path).map_err(|e| {
            Diagnostic::error(
                Code::RUNTIME_CACHE_UNREADABLE,
                format!("{}: {e}", cache_dir.display()),
            )
            .with_document(shown.clone())
            .with_suggestion("run `fynkel cook` (or `fynkel run`, which cooks first)")
        })?;
        let index: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            Diagnostic::error(Code::RUNTIME_CACHE_UNREADABLE, format!("not JSON: {e}"))
                .with_document(shown.clone())
        })?;
        if index["fynkel"] != "cache-index" {
            return Err(Diagnostic::error(
                Code::RUNTIME_CACHE_UNREADABLE,
                "not a cache index (no `fynkel: cache-index`)",
            )
            .with_document(shown));
        }
        let fingerprint = format!("{:#018x}", fynkel_core::schema::schema_fingerprint());
        let cooked_with = index["schemaFingerprint"].as_str().unwrap_or("");
        if cooked_with != fingerprint {
            return Err(Diagnostic::error(
                Code::RUNTIME_CACHE_STALE,
                format!(
                    "the cache was cooked for schema fingerprint {} but this build is {fingerprint}",
                    if cooked_with.is_empty() { "(unknown)" } else { cooked_with }
                ),
            )
            .with_document(shown)
            .with_pointer("/schemaFingerprint")
            .with_suggestion("run `fynkel cook` again with this build"));
        }
        let mut entries = HashMap::new();
        for a in index["assets"].as_array().into_iter().flatten() {
            let Some(id) = a["id"].as_str().and_then(parse_uuid) else {
                continue;
            };
            entries.insert(
                id,
                IndexEntry {
                    id,
                    path: a["path"].as_str().unwrap_or("").to_string(),
                    name: a["name"].as_str().unwrap_or("").to_string(),
                    kind: a["kind"].as_str().unwrap_or("").to_string(),
                    status: a["status"].as_str().unwrap_or("").to_string(),
                    key: a["key"].as_str().map(str::to_string),
                    file: a["file"].as_str().map(|f| cache_dir.join(f)),
                    dependencies: a["dependencies"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|d| d.as_str().and_then(parse_uuid))
                        .collect(),
                },
            );
        }
        let project = index["project"].as_object().map(|p| ProjectInfo {
            name: p["name"].as_str().unwrap_or("").to_string(),
            startup_scene: p["startupScene"].as_str().and_then(parse_uuid),
            quality: Quality::from_json(&p["quality"]),
        });
        Ok(CacheIndex {
            root: cache_dir.to_path_buf(),
            profile: index["profile"].as_str().unwrap_or("").to_string(),
            schema_fingerprint: fingerprint,
            project,
            entries,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn schema_fingerprint(&self) -> &str {
        &self.schema_fingerprint
    }

    pub fn project(&self) -> Option<&ProjectInfo> {
        self.project.as_ref()
    }

    /// The startup scene the project file named when the cache was cooked.
    pub fn startup_scene(&self) -> Result<Uuid, Diagnostic> {
        self.project
            .as_ref()
            .and_then(|p| p.startup_scene)
            .ok_or_else(|| {
                Diagnostic::error(
                    Code::RUNTIME_NO_STARTUP_SCENE,
                    "the cache names no startup scene",
                )
                .with_document(self.root.join("index.json").display().to_string())
                .with_suggestion(
                    "cook from the project root (`fynkel cook` reads fynkel.project.json), or load a scene by id",
                )
            })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: Uuid) -> Option<&IndexEntry> {
        self.entries.get(&id)
    }

    pub fn entries(&self) -> impl Iterator<Item = &IndexEntry> {
        self.entries.values()
    }

    /// The entry with a cooked object: `FYN-R0003` when the id is unknown,
    /// `FYN-R0004` when its cook failed.
    pub fn object(&self, id: Uuid) -> Result<&IndexEntry, Diagnostic> {
        let e = self.entries.get(&id).ok_or_else(|| {
            Diagnostic::error(
                Code::RUNTIME_ASSET_MISSING,
                format!("asset {id} is not in the cache"),
            )
            .with_id(id.to_string())
            .with_suggestion("run `fynkel cook`; a reference to an asset the database never had fails `fynkel validate`")
        })?;
        if e.file.is_none() {
            return Err(Diagnostic::error(
                Code::RUNTIME_ASSET_FAILED,
                format!("{}: the cook of {id} failed; there is no object", e.path),
            )
            .with_id(id.to_string())
            .with_suggestion("`fynkel cook` reports why"));
        }
        Ok(e)
    }

    /// `id` and everything it depends on, transitively (a scene's closure
    /// is what a load retains).
    pub fn closure(&self, id: Uuid) -> Vec<Uuid> {
        let mut seen = HashSet::new();
        let mut order = Vec::new();
        let mut stack = vec![id];
        while let Some(next) = stack.pop() {
            if !seen.insert(next) {
                continue;
            }
            order.push(next);
            if let Some(e) = self.entries.get(&next) {
                stack.extend(e.dependencies.iter().copied());
            }
        }
        order
    }
}

/// A cooked scene as the runtime loads it.
#[derive(Clone, Debug)]
pub struct CookedScene {
    pub id: Uuid,
    pub name: String,
    /// The object's cache key (hex).
    pub key: String,
    /// The authoring path from the index.
    pub path: String,
    pub description: SceneDescription,
    /// Document order; the SDK attaches these.
    pub behaviours: Vec<AuthoredBehaviour>,
    /// Warnings from describing the chunk.
    pub diagnostics: Vec<Diagnostic>,
    /// The scene and its transitive dependencies, what [`Assets::retain`]
    /// keeps resident for it.
    pub closure: Vec<Uuid>,
}

/// One cooked object's chunk table, parsed once.
struct ObjectMeta {
    file: PathBuf,
    /// (kind, sub, index)
    chunks: Vec<([u8; 4], String, usize)>,
    scene_info: Option<serde_json::Value>,
}

struct Store {
    index: CacheIndex,
    metas: HashMap<Uuid, Arc<ObjectMeta>>,
    meshes: HashMap<AssetRef, Arc<MeshPayload>>,
    textures: HashMap<AssetRef, Arc<TexturePayload>>,
    materials: HashMap<AssetRef, Arc<PbrMaterial>>,
    /// Residency counts by asset id (`retain` increments, release
    /// decrements; zero evicts).
    counts: HashMap<Uuid, u32>,
    evictions: u64,
}

/// Residency numbers (`fynkel_world_load_report`, HUDs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResidencyStats {
    /// Assets something retains.
    pub retained: usize,
    pub meshes: usize,
    pub textures: usize,
    pub materials: usize,
    /// CPU bytes of the resident mesh and texture payloads.
    pub bytes: u64,
    /// Evictions since the store opened.
    pub evictions: u64,
}

/// The residency store: cheap to clone, one cache behind every clone.
#[derive(Clone)]
pub struct Assets {
    inner: Arc<Mutex<Store>>,
}

impl std::fmt::Debug for Assets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.lock();
        f.debug_struct("Assets")
            .field("root", &s.index.root)
            .field("assets", &s.index.entries.len())
            .field("retained", &s.counts.len())
            .finish()
    }
}

fn object_error(e: &IndexEntry, what: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::error(
        Code::RUNTIME_OBJECT_UNREADABLE,
        format!("{}: cooked object: {what}", e.path),
    )
    .with_id(e.id.to_string())
    .with_document(
        e.file
            .as_ref()
            .map(|f| f.display().to_string())
            .unwrap_or_default(),
    )
    .with_suggestion("run `fynkel cook --force`")
}

impl Assets {
    /// Opens the cache (see [`CacheIndex::open`]); nothing is loaded yet.
    pub fn open(cache_dir: &Path) -> Result<Assets, Diagnostic> {
        Ok(Assets::from_index(CacheIndex::open(cache_dir)?))
    }

    pub fn from_index(index: CacheIndex) -> Assets {
        Assets {
            inner: Arc::new(Mutex::new(Store {
                index,
                metas: HashMap::new(),
                meshes: HashMap::new(),
                textures: HashMap::new(),
                materials: HashMap::new(),
                counts: HashMap::new(),
                evictions: 0,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Store> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether `other` is a clone of this store (the same cache and the
    /// same resident objects).
    pub fn same_store(&self, other: &Assets) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn root(&self) -> PathBuf {
        self.lock().index.root.clone()
    }

    pub fn profile(&self) -> String {
        self.lock().index.profile.clone()
    }

    pub fn project(&self) -> Option<ProjectInfo> {
        self.lock().index.project.clone()
    }

    pub fn startup_scene(&self) -> Result<Uuid, Diagnostic> {
        self.lock().index.startup_scene()
    }

    pub fn entry(&self, id: Uuid) -> Option<IndexEntry> {
        self.lock().index.get(id).cloned()
    }

    pub fn closure(&self, id: Uuid) -> Vec<Uuid> {
        self.lock().index.closure(id)
    }

    pub fn is_resident(&self, id: Uuid) -> bool {
        self.lock().counts.get(&id).copied().unwrap_or(0) > 0
    }

    /// Bumps once per eviction pass; a renderer compares it to drop GPU
    /// copies of what left the store.
    pub fn evictions(&self) -> u64 {
        self.lock().evictions
    }

    pub fn stats(&self) -> ResidencyStats {
        let s = self.lock();
        ResidencyStats {
            retained: s.counts.values().filter(|c| **c > 0).count(),
            meshes: s.meshes.len(),
            textures: s.textures.len(),
            materials: s.materials.len(),
            bytes: s
                .meshes
                .values()
                .map(|m| {
                    (m.vertices.len() * std::mem::size_of::<fynkel_cooked::payload::Vertex>()
                        + m.indices.len() * 4) as u64
                })
                .sum::<u64>()
                + s.textures
                    .values()
                    .map(|t| t.rgba8.len() as u64)
                    .sum::<u64>(),
            evictions: s.evictions,
        }
    }

    /// Keeps `ids` (a scene's closure) resident until the returned token is
    /// released or dropped. Nothing is loaded here — payloads still load on
    /// first use — but releasing evicts every payload of an asset no other
    /// token retains.
    pub fn retain(&self, ids: &[Uuid]) -> Residency {
        let mut s = self.lock();
        for id in ids {
            *s.counts.entry(*id).or_insert(0) += 1;
        }
        Residency {
            assets: self.clone(),
            ids: ids.to_vec(),
        }
    }

    fn release(&self, ids: &[Uuid]) {
        let mut s = self.lock();
        let mut gone = Vec::new();
        for id in ids {
            if let Some(c) = s.counts.get_mut(id) {
                *c = c.saturating_sub(1);
                if *c == 0 {
                    s.counts.remove(id);
                    gone.push(*id);
                }
            }
        }
        if gone.is_empty() {
            return;
        }
        let gone: HashSet<Uuid> = gone.into_iter().collect();
        s.meshes.retain(|r, _| !gone.contains(&r.asset));
        s.textures.retain(|r, _| !gone.contains(&r.asset));
        s.materials.retain(|r, _| !gone.contains(&r.asset));
        s.metas.retain(|id, _| !gone.contains(id));
        s.evictions += 1;
    }

    /// Drops every resident payload (counts stay; payloads reload on use).
    pub fn clear(&self) {
        let mut s = self.lock();
        s.meshes.clear();
        s.textures.clear();
        s.materials.clear();
        s.metas.clear();
        s.evictions += 1;
    }

    pub fn mesh(&self, r: &AssetRef) -> Result<Arc<MeshPayload>, Diagnostic> {
        if let Some(m) = self.lock().meshes.get(r) {
            return Ok(m.clone());
        }
        let bytes = self.chunk(r, KIND_MESH)?;
        let payload = decode_mesh(&bytes).ok_or_else(|| self.malformed(r, "MESH"))?;
        let payload = Arc::new(payload);
        self.lock().meshes.insert(r.clone(), payload.clone());
        Ok(payload)
    }

    pub fn texture(&self, r: &AssetRef) -> Result<Arc<TexturePayload>, Diagnostic> {
        if let Some(t) = self.lock().textures.get(r) {
            return Ok(t.clone());
        }
        let bytes = self.chunk(r, KIND_TEXTURE)?;
        let payload = decode_texture(&bytes).ok_or_else(|| self.malformed(r, "TEXR"))?;
        let payload = Arc::new(payload);
        self.lock().textures.insert(r.clone(), payload.clone());
        Ok(payload)
    }

    /// The cooked `PbrMaterial`. A custom-shader material has no `MATL`
    /// chunk (`FYN-R0006`).
    pub fn material(&self, r: &AssetRef) -> Result<PbrMaterial, Diagnostic> {
        if let Some(m) = self.lock().materials.get(r) {
            return Ok((**m).clone());
        }
        let bytes = self.chunk(r, KIND_MATERIAL)?;
        let m = decode_material(&bytes).ok_or_else(|| self.malformed(r, "MATL"))?;
        self.lock().materials.insert(r.clone(), Arc::new(m.clone()));
        Ok(m)
    }

    /// Describes the cooked scene `id`: the expanded scene chunk becomes a
    /// `SceneDescription` plus the authored behaviours. The behaviour types
    /// the cook recorded are declared known; anything else non-native is a
    /// `FYN-D0205` warning as for a source scene. `Err` carries every
    /// diagnostic under a `FYN-R0008` header when the chunk does not
    /// describe (a cache from another build: recook).
    pub fn scene(&self, id: Uuid) -> Result<CookedScene, Vec<Diagnostic>> {
        let (entry, meta) = {
            let mut s = self.lock();
            let entry = s.index.object(id).map_err(|d| vec![d])?.clone();
            let meta = s.meta(&entry).map_err(|d| vec![d])?;
            (entry, meta)
        };
        if entry.kind != "document/scene" {
            return Err(vec![Diagnostic::error(
                Code::RUNTIME_CHUNK_MISSING,
                format!(
                    "{}: {id} is a {} asset, not a scene",
                    entry.path, entry.kind
                ),
            )
            .with_id(id.to_string())]);
        }
        let bytes = self
            .read_chunk(&entry, &meta, KIND_JSON, "")
            .map_err(|d| vec![d])?;
        let text =
            String::from_utf8(bytes).map_err(|_| vec![self.malformed_entry(&entry, "JSON")])?;
        let path = entry.path.clone();
        let doc =
            Document::parse(DocumentKind::Scene, Some(&path), &text).map_err(|mut diags| {
                diags.insert(0, self.scene_invalid(&entry));
                diags
            })?;
        let mut schemas = SchemaSet::embedded();
        if let Some(info) = &meta.scene_info {
            for b in info["behaviourTypes"].as_array().into_iter().flatten() {
                if let Some(t) = b["type"].as_str() {
                    schemas.declare_behaviour(t, b["version"].as_u64().unwrap_or(1) as u32);
                }
            }
        }
        let scene = Scene::from_document(doc);
        let mut diagnostics = scene.validate(&schemas);
        let described = describe(&scene, &schemas);
        diagnostics.extend(described.diagnostics);
        if fynkel_contracts::has_errors(&diagnostics) {
            diagnostics.insert(0, self.scene_invalid(&entry));
            return Err(diagnostics);
        }
        let closure = self.lock().index.closure(id);
        Ok(CookedScene {
            id,
            name: described.description.name.clone(),
            key: entry.key.clone().unwrap_or_default(),
            path,
            description: described.description,
            behaviours: described.behaviours,
            diagnostics,
            closure,
        })
    }

    fn scene_invalid(&self, entry: &IndexEntry) -> Diagnostic {
        Diagnostic::error(
            Code::RUNTIME_SCENE_INVALID,
            format!(
                "{}: the cooked scene does not load in this build",
                entry.path
            ),
        )
        .with_id(entry.id.to_string())
        .with_suggestion(
            "run `fynkel cook --force`; `fynkel validate` reports the document's problems",
        )
    }

    fn malformed(&self, r: &AssetRef, kind: &str) -> Diagnostic {
        let entry = self.lock().index.get(r.asset).cloned();
        match entry {
            Some(e) => self.malformed_entry(&e, kind).with_pointer(r.sub.clone()),
            None => Diagnostic::error(
                Code::RUNTIME_PAYLOAD_MALFORMED,
                format!("{r}: malformed {kind} payload"),
            ),
        }
    }

    fn malformed_entry(&self, e: &IndexEntry, kind: &str) -> Diagnostic {
        Diagnostic::error(
            Code::RUNTIME_PAYLOAD_MALFORMED,
            format!("{}: malformed {kind} payload", e.path),
        )
        .with_id(e.id.to_string())
        .with_suggestion("run `fynkel cook --force` (the cache was written by another version)")
    }

    /// The payload of the chunk of `kind` whose `sub` is the reference's.
    fn chunk(&self, r: &AssetRef, kind: [u8; 4]) -> Result<Vec<u8>, Diagnostic> {
        let (entry, meta) = {
            let mut s = self.lock();
            let entry = s
                .index
                .object(r.asset)
                .map_err(|d| d.with_pointer(r.sub.clone()))?
                .clone();
            let meta = s.meta(&entry)?;
            (entry, meta)
        };
        self.read_chunk(&entry, &meta, kind, &r.sub)
    }

    fn read_chunk(
        &self,
        entry: &IndexEntry,
        meta: &ObjectMeta,
        kind: [u8; 4],
        sub: &str,
    ) -> Result<Vec<u8>, Diagnostic> {
        let want = String::from_utf8_lossy(&kind).to_string();
        let index = meta
            .chunks
            .iter()
            .find(|(k, s, _)| *k == kind && s == sub)
            .map(|(_, _, i)| *i)
            .ok_or_else(|| {
                Diagnostic::error(
                    Code::RUNTIME_CHUNK_MISSING,
                    format!(
                        "{}: no {want} chunk for sub-asset \"{sub}\" in the cooked object",
                        entry.path
                    ),
                )
                .with_id(entry.id.to_string())
                .with_pointer(sub)
            })?;
        let mut file = ChunkFile::open_path(&meta.file).map_err(|e| object_error(entry, e))?;
        file.read(index, MAX_CHUNK_BYTES)
            .map_err(|e| object_error(entry, format!("chunk {index}: {e}")))
    }
}

impl Store {
    /// The object's chunk table (cached per asset while resident).
    fn meta(&mut self, entry: &IndexEntry) -> Result<Arc<ObjectMeta>, Diagnostic> {
        if let Some(m) = self.metas.get(&entry.id) {
            return Ok(m.clone());
        }
        let file = entry.file.clone().expect("object() checked the file");
        let mut cf = ChunkFile::open_path(&file).map_err(|e| object_error(entry, e))?;
        let meta = cf
            .meta(MAX_META_BYTES)
            .map_err(|e| object_error(entry, format!("META: {e}")))?;
        let meta: serde_json::Value =
            serde_json::from_slice(&meta).map_err(|e| object_error(entry, format!("META: {e}")))?;
        let mut chunks = Vec::new();
        let mut scene_info = None;
        for c in meta["chunks"].as_array().into_iter().flatten() {
            let (Some(kind), Some(index)) = (c["kind"].as_str(), c["index"].as_u64()) else {
                continue;
            };
            let kind_bytes: [u8; 4] = match kind.as_bytes().try_into() {
                Ok(k) => k,
                Err(_) => continue,
            };
            if kind_bytes == KIND_JSON && c["documentKind"] == "scene" {
                scene_info = Some(c.clone());
            }
            chunks.push((
                kind_bytes,
                c["sub"].as_str().unwrap_or("").to_string(),
                index as usize,
            ));
        }
        let m = Arc::new(ObjectMeta {
            file,
            chunks,
            scene_info,
        });
        self.metas.insert(entry.id, m.clone());
        Ok(m)
    }
}

/// What keeps a set of assets resident: a loaded scene holds one for its
/// closure. Releasing (or dropping) it evicts every payload of an asset no
/// other token retains.
pub struct Residency {
    assets: Assets,
    ids: Vec<Uuid>,
}

impl Residency {
    pub fn ids(&self) -> &[Uuid] {
        &self.ids
    }

    pub fn release(self) {}
}

impl Drop for Residency {
    fn drop(&mut self) {
        let ids = std::mem::take(&mut self.ids);
        self.assets.release(&ids);
    }
}

impl std::fmt::Debug for Residency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Residency")
            .field("ids", &self.ids.len())
            .finish()
    }
}
