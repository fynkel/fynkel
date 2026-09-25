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
use fynkel_cooked::payload::{
    decode_animation, decode_clip, decode_environment, decode_skin, decode_stream, decode_texture,
    ANIM_INTERPOLATION_CUBIC_SPLINE, ANIM_INTERPOLATION_LINEAR, ANIM_INTERPOLATION_STEP,
    ANIM_PROPERTY_ROTATION, ANIM_PROPERTY_SCALE, ANIM_PROPERTY_TRANSLATION,
};
pub use fynkel_cooked::payload::{
    decode_mesh, encode_mesh, encode_skinned_mesh, EnvironmentPayload, MeshPayload, TexturePayload,
    Vertex,
};
use fynkel_cooked::{
    ChunkFile, KIND_ANIMATION, KIND_CLIP, KIND_ENVIRONMENT, KIND_JSON, KIND_MATERIAL, KIND_MESH,
    KIND_SKIN, KIND_STREAM, KIND_TEXTURE,
};
use fynkel_core::animation::{
    BlendPoint, BlendSpace, BlendSync, Channel, Clip, Interpolation, Joint, Marker, Property, Skin,
};
use fynkel_core::input::{ActionKind, ActionSpec};
pub use fynkel_core::material::PbrMaterial;
use fynkel_core::physics::PhysicsMaterial;
use fynkel_core::volume::VolumeProfile;
use fynkel_core::AudioClip;
use fynkel_core::SceneDescription;
pub use fynkel_document::project::{
    LayersAndTags, ModuleSettings, PhysicsSettings, Quality, QualityProfile,
};
pub use fynkel_document::AuthoredBehaviour;
use fynkel_document::{describe, Document, DocumentKind, Scene, SchemaSet};
use fynkel_wire::material::decode_material;

/// The sub-asset a cooked source's node hierarchy is stored under (the
/// glTF importer's `prefab`; M3-19a instantiates it like a prefab document).
pub const IMPORTED_PREFAB_SUB: &str = "prefab";

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
    /// The project's input actions as cooked (the world's binding table
    /// on load; empty when the project declares none).
    pub input_actions: Vec<ActionSpec>,
    /// The project's `physics` block as cooked (defaults when absent).
    pub physics: PhysicsSettings,
    /// The project's `layers` and `tags` as cooked (ADR-0020).
    pub layers_and_tags: LayersAndTags,
    /// The project's `modules` as cooked (ADR-0029 §3; all on when absent).
    pub modules: ModuleSettings,
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
            input_actions: p["inputActions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| {
                    Some(ActionSpec {
                        name: a["name"].as_str()?.to_string(),
                        kind: ActionKind::parse(a["type"].as_str()?)?,
                        bindings: a["bindings"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|b| b.as_str().map(str::to_string))
                            .collect(),
                    })
                })
                .collect(),
            physics: PhysicsSettings::from_json(&p["physics"]),
            modules: ModuleSettings::from_json(&p["modules"]),
            layers_and_tags: LayersAndTags::from_json(&serde_json::Value::Object(p.clone())),
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

/// A cooked prefab ready to instantiate (M3-08b): [`Assets::prefab`].
#[derive(Clone, Debug)]
pub struct CookedPrefab {
    pub id: Uuid,
    /// The prefab document's name.
    pub name: String,
    pub path: String,
    /// The root (ADR-0022): the one parentless entity.
    pub root: Uuid,
    pub description: SceneDescription,
    pub behaviours: Vec<AuthoredBehaviour>,
    pub diagnostics: Vec<Diagnostic>,
    pub closure: Vec<Uuid>,
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
    /// `Some` for an HDR image, `None` recorded for one without an `ENVM`.
    environments: HashMap<AssetRef, Option<Arc<EnvironmentPayload>>>,
    materials: HashMap<AssetRef, Arc<PbrMaterial>>,
    /// Materials made by the game at runtime (`world.createMaterial`,
    /// ADR-0026 §4) under ids the cache does not have: read before the
    /// cooked ones, never evicted or cleared — only the game removes one.
    runtime_materials: HashMap<AssetRef, Arc<PbrMaterial>>,
    /// Meshes made by the game at runtime (`world.createMesh`, ADR-0026
    /// §4), read before the cooked ones and kept like the materials.
    runtime_meshes: HashMap<AssetRef, Arc<MeshPayload>>,
    /// Bumps on every runtime mesh set or removed; `mesh_changes` holds
    /// the count each mesh last changed at, so a renderer re-uploads only
    /// what changed since it last looked.
    mesh_edits: u64,
    mesh_changes: HashMap<AssetRef, u64>,
    clips: HashMap<AssetRef, Arc<Clip>>,
    blends: HashMap<AssetRef, Arc<BlendSpace>>,
    volumes: HashMap<AssetRef, VolumeProfile>,
    physics_materials: HashMap<AssetRef, PhysicsMaterial>,
    skins: HashMap<AssetRef, Arc<Skin>>,
    sounds: HashMap<AssetRef, Arc<AudioClip>>,
    /// Residency counts by asset id (`retain` increments, release
    /// decrements; zero evicts).
    counts: HashMap<Uuid, u32>,
    evictions: u64,
    /// Bumps on every material or volume profile override or restore.
    live_edits: u64,
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
                environments: HashMap::new(),
                materials: HashMap::new(),
                runtime_materials: HashMap::new(),
                runtime_meshes: HashMap::new(),
                mesh_edits: 0,
                mesh_changes: HashMap::new(),
                clips: HashMap::new(),
                blends: HashMap::new(),
                volumes: HashMap::new(),
                physics_materials: HashMap::new(),
                skins: HashMap::new(),
                sounds: HashMap::new(),
                counts: HashMap::new(),
                evictions: 0,
                live_edits: 0,
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

    /// Bumps on every material or volume profile override or restore; a
    /// renderer compares it to rebuild its materials and re-read its
    /// profiles.
    pub fn live_edits(&self) -> u64 {
        self.lock().live_edits
    }

    /// `m` in place of the cooked material `r` until [`restore_material`]
    /// or the store is replaced: the editor's live Inspector edit, drawn
    /// by every renderer on this store from its next frame.
    ///
    /// [`restore_material`]: Assets::restore_material
    pub fn override_material(&self, r: &AssetRef, m: PbrMaterial) {
        let mut s = self.lock();
        s.materials.insert(r.clone(), Arc::new(m));
        s.live_edits += 1;
    }

    /// `m` as the runtime material `r` (`world.createMaterial`, ADR-0026
    /// §4), or `None` to remove it: drawn by every renderer on this store
    /// like a cooked one, kept across clears and evictions.
    pub fn set_runtime_material(&self, r: &AssetRef, m: Option<PbrMaterial>) {
        let mut s = self.lock();
        match m {
            Some(m) => s.runtime_materials.insert(r.clone(), Arc::new(m)),
            None => s.runtime_materials.remove(r),
        };
        s.live_edits += 1;
    }

    /// The runtime materials and meshes of `from` (the store a load to
    /// another cache replaces), so what the game made outlives the switch.
    pub fn adopt_runtime_assets(&self, from: &Assets) {
        if Arc::ptr_eq(&self.inner, &from.inner) {
            return;
        }
        let (materials, meshes) = {
            let f = from.lock();
            (f.runtime_materials.clone(), f.runtime_meshes.clone())
        };
        let mut s = self.lock();
        s.runtime_materials.extend(materials);
        s.live_edits += 1;
        for (r, m) in meshes {
            s.mesh_edits += 1;
            let at = s.mesh_edits;
            s.mesh_changes.insert(r.clone(), at);
            s.runtime_meshes.insert(r, m);
        }
    }

    /// `m` as the runtime mesh `r` (`world.createMesh`, ADR-0026 §4), or
    /// `None` to remove it: drawn by every renderer on this store like a
    /// cooked mesh, kept across clears and evictions. `m`'s bounds are
    /// recomputed from its positions.
    pub fn set_runtime_mesh(&self, r: &AssetRef, m: Option<MeshPayload>) {
        let mut s = self.lock();
        match m {
            Some(mut m) => {
                let (min, max) = bounds_of(&m.vertices);
                m.bounds_min = min;
                m.bounds_max = max;
                s.runtime_meshes.insert(r.clone(), Arc::new(m))
            }
            None => s.runtime_meshes.remove(r),
        };
        s.meshes.remove(r);
        s.mesh_edits += 1;
        let at = s.mesh_edits;
        s.mesh_changes.insert(r.clone(), at);
    }

    /// Whether `r` is a mesh the game made or replaced at runtime.
    pub fn is_runtime_mesh(&self, r: &AssetRef) -> bool {
        self.lock().runtime_meshes.contains_key(r)
    }

    /// Bumps on every [`set_runtime_mesh`]; with [`meshes_changed_since`]
    /// a renderer drops the GPU copies of the meshes that changed.
    ///
    /// [`set_runtime_mesh`]: Assets::set_runtime_mesh
    /// [`meshes_changed_since`]: Assets::meshes_changed_since
    pub fn mesh_edits(&self) -> u64 {
        self.lock().mesh_edits
    }

    /// The meshes set or removed after the [`mesh_edits`] count `seen`.
    ///
    /// [`mesh_edits`]: Assets::mesh_edits
    pub fn meshes_changed_since(&self, seen: u64) -> Vec<AssetRef> {
        let s = self.lock();
        s.mesh_changes
            .iter()
            .filter(|(_, at)| **at > seen)
            .map(|(r, _)| r.clone())
            .collect()
    }

    /// The cooked material `r` again (read on its next use).
    pub fn restore_material(&self, r: &AssetRef) {
        let mut s = self.lock();
        s.materials.remove(r);
        s.live_edits += 1;
    }

    /// `p` in place of the cooked volume profile `r` until
    /// [`restore_volume`], as [`override_material`] for a material.
    ///
    /// [`restore_volume`]: Assets::restore_volume
    /// [`override_material`]: Assets::override_material
    pub fn override_volume(&self, r: &AssetRef, p: VolumeProfile) {
        let mut s = self.lock();
        s.volumes.insert(r.clone(), p);
        s.live_edits += 1;
    }

    /// The cooked volume profile `r` again (read on its next use).
    pub fn restore_volume(&self, r: &AssetRef) {
        let mut s = self.lock();
        s.volumes.remove(r);
        s.live_edits += 1;
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
                    .map(|t| t.texels.len() as u64)
                    .sum::<u64>()
                + s.environments
                    .values()
                    .flatten()
                    .map(|e| (e.specular.len() + e.irradiance.len()) as u64 * 2)
                    .sum::<u64>(),
            evictions: s.evictions,
        }
    }

    /// Registers the mesh of every `shape: mesh` collider in `scene` with
    /// the physics adapter (M3-08), so the world's first step can build
    /// them. Returns the registered mesh count; a mesh that cannot be
    /// loaded or is over the triangle limit is a diagnostic
    /// (the store's, or the adapter's reason as `FYN-R0007`) and the
    /// collider is skipped at the step
    /// (`MESH_UNAVAILABLE`).
    ///
    /// Every collider's physics material (ADR-0025) is registered too,
    /// read from the store each time — so a reload after the document was
    /// edited and recooked gives the live colliders its new values. One
    /// that cannot be read is a diagnostic (the store's) and the collider
    /// keeps its own friction and restitution.
    pub fn register_mesh_colliders(
        &self,
        scene: &SceneDescription,
        physics: &mut fynkel_core::Physics,
    ) -> (usize, Vec<Diagnostic>) {
        use fynkel_core::components::ComponentValue;
        use fynkel_core::ColliderShape;
        let mut registered = 0;
        let mut diags = Vec::new();
        let mut seen = HashSet::new();
        let mut seen_materials = HashSet::new();
        for e in &scene.entities {
            for c in &e.components {
                let ComponentValue::Collider(c) = c else {
                    continue;
                };
                if let Some(key) = c.material.as_ref().filter(|k| !k.asset.is_nil()) {
                    if seen_materials.insert(key.clone()) {
                        match self.physics_material(key).and_then(|m| {
                            physics.register_material(key.clone(), m).map_err(|msg| {
                                Diagnostic::error(
                                    Code::RUNTIME_PAYLOAD_MALFORMED,
                                    format!("physics material {key}: {msg}"),
                                )
                                .with_id(key.to_string())
                            })
                        }) {
                            Ok(()) => {}
                            Err(d) => diags.push(d),
                        }
                    }
                }
                if c.shape != ColliderShape::Mesh {
                    continue;
                }
                let Some(key) = &c.mesh else {
                    continue;
                };
                if !seen.insert(key.clone()) || physics.has_mesh(key) {
                    continue;
                }
                match self.mesh(key) {
                    Ok(m) => {
                        let positions: Vec<[f32; 3]> =
                            m.vertices.iter().map(|v| v.position).collect();
                        match physics.register_mesh(key.clone(), &positions, &m.indices) {
                            Ok(_) => registered += 1,
                            Err(msg) => diags.push(
                                Diagnostic::error(
                                    Code::RUNTIME_PAYLOAD_MALFORMED,
                                    format!("mesh collider {key}: {msg}"),
                                )
                                .with_id(key.to_string()),
                            ),
                        }
                    }
                    Err(d) => diags.push(d),
                }
            }
        }
        (registered, diags)
    }

    /// A collider set by a command rather than a load (code adding one,
    /// ADR-0026 §4): its physics material and, for `shape: mesh`, its mesh
    /// registered as [`register_mesh_colliders`] does for a scene — once;
    /// what is registered already is left alone. The store's diagnostics
    /// for what cannot be read (the collider then reports at the step).
    ///
    /// [`register_mesh_colliders`]: Assets::register_mesh_colliders
    pub fn register_collider(
        &self,
        c: &fynkel_core::Collider,
        physics: &mut fynkel_core::Physics,
    ) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        if let Some(key) = c.material.as_ref().filter(|k| !k.asset.is_nil()) {
            if !physics.has_material(key) {
                match self.physics_material(key) {
                    Ok(m) => {
                        if let Err(msg) = physics.register_material(key.clone(), m) {
                            diags.push(
                                Diagnostic::error(
                                    Code::RUNTIME_PAYLOAD_MALFORMED,
                                    format!("physics material {key}: {msg}"),
                                )
                                .with_id(key.to_string()),
                            );
                        }
                    }
                    Err(d) => diags.push(d),
                }
            }
        }
        if c.shape != fynkel_core::ColliderShape::Mesh {
            return diags;
        }
        let Some(key) = c.mesh.as_ref().filter(|k| !k.asset.is_nil()) else {
            return diags;
        };
        if physics.has_mesh(key) {
            return diags;
        }
        match self.mesh(key) {
            Ok(m) => {
                let positions: Vec<[f32; 3]> = m.vertices.iter().map(|v| v.position).collect();
                if let Err(msg) = physics.register_mesh(key.clone(), &positions, &m.indices) {
                    diags.push(
                        Diagnostic::error(
                            Code::RUNTIME_PAYLOAD_MALFORMED,
                            format!("mesh collider {key}: {msg}"),
                        )
                        .with_id(key.to_string()),
                    );
                }
            }
            Err(d) => diags.push(d),
        }
        diags
    }

    /// Registers with `world` every prefab the `Emitter`s in `scene` name
    /// (M3-10), recursively through the prefabs' own emitters, so the
    /// catalogue can instantiate them at the tick: each prefab is read from
    /// the cache once, its mesh colliders registered as for a scene. An
    /// unreadable prefab is a diagnostic (the store's) and the emitter
    /// reports `PREFAB_UNAVAILABLE` at its first tick. Returns the number
    /// of prefabs registered by this call.
    pub fn register_prefabs(
        &self,
        scene: &SceneDescription,
        world: &mut fynkel_core::World,
    ) -> (usize, Vec<Diagnostic>) {
        use fynkel_core::components::ComponentValue;
        let mut registered = 0;
        let mut diags = Vec::new();
        let mut seen = HashSet::new();
        let mut pending: Vec<Uuid> = Vec::new();
        let mut collect = |scene: &SceneDescription, pending: &mut Vec<Uuid>| {
            for e in &scene.entities {
                for c in &e.components {
                    if let ComponentValue::Emitter(em) = c {
                        if !em.prefab.asset.is_nil() && seen.insert(em.prefab.asset) {
                            pending.push(em.prefab.asset);
                        }
                    }
                }
            }
        };
        collect(scene, &mut pending);
        while let Some(id) = pending.pop() {
            if world.has_prefab(id) {
                continue;
            }
            match self.prefab(id) {
                Ok(p) => {
                    let (_, mesh_diags) =
                        self.register_mesh_colliders(&p.description, world.physics_mut());
                    diags.extend(mesh_diags);
                    let (_, _, anim_diags) = self.register_animation(&p.description, world);
                    diags.extend(anim_diags);
                    collect(&p.description, &mut pending);
                    world.register_prefab(id, p.description);
                    registered += 1;
                }
                Err(d) => diags.extend(d),
            }
        }
        (registered, diags)
    }

    /// Registers with `world` every clip an `Animator` in `scene` names and
    /// every skin a `Skin` component names (M3-19), so the animation
    /// system finds them at the first tick. An unreadable clip or skin is
    /// a diagnostic (the store's) and the component reports
    /// `CLIP_UNAVAILABLE` / `SKIN_UNAVAILABLE` at its first tick. Returns
    /// `(clips, skins)` registered by this call.
    pub fn register_animation(
        &self,
        scene: &SceneDescription,
        world: &mut fynkel_core::World,
    ) -> (usize, usize, Vec<Diagnostic>) {
        use fynkel_core::components::ComponentValue;
        let mut clips = 0;
        let mut skins = 0;
        let mut diags = Vec::new();
        for e in &scene.entities {
            for c in &e.components {
                match c {
                    ComponentValue::Animator(a) => match self.register_clip(&a.clip, world) {
                        Ok(true) => clips += 1,
                        Ok(false) => {}
                        Err(d) => diags.push(d),
                    },
                    ComponentValue::Skin(k) => match self.register_skin(&k.skin, world) {
                        Ok(true) => skins += 1,
                        Ok(false) => {}
                        Err(d) => diags.push(d),
                    },
                    _ => {}
                }
            }
        }
        (clips, skins, diags)
    }

    /// Registers with `world` every sound an `AudioSource` in `scene`
    /// names (M3-11), so the audio system finds them at the first tick.
    /// An unreadable sound is a diagnostic (the store's) and the source
    /// reports `SOUND_UNAVAILABLE` at its first tick. Returns the number
    /// registered by this call.
    pub fn register_audio(
        &self,
        scene: &SceneDescription,
        world: &mut fynkel_core::World,
    ) -> (usize, Vec<Diagnostic>) {
        use fynkel_core::components::ComponentValue;
        let mut sounds = 0;
        let mut diags = Vec::new();
        for e in &scene.entities {
            for c in &e.components {
                if let ComponentValue::AudioSource(a) = c {
                    match self.register_sound(&a.clip, world) {
                        Ok(true) => sounds += 1,
                        Ok(false) => {}
                        Err(d) => diags.push(d),
                    }
                }
            }
        }
        (sounds, diags)
    }

    /// Registers the sound `r` with `world` unless it is nil or already
    /// there; `Ok(true)` when this call registered it.
    pub fn register_sound(
        &self,
        r: &AssetRef,
        world: &mut fynkel_core::World,
    ) -> Result<bool, Diagnostic> {
        if r.asset.is_nil() || world.has_sound(r) {
            return Ok(false);
        }
        let sound = self.sound(r)?;
        world
            .register_sound(r.clone(), sound)
            .map_err(|msg| self.malformed(r, "CLIP").with_suggestion(msg))?;
        Ok(true)
    }

    /// Registers the clip `r` with `world` unless it is nil or already
    /// there; `Ok(true)` when this call registered it. A blend space
    /// (M3-19c: `r` names a `.blend.json`) is registered with every
    /// clip of its points; a point's clip that does not read is the
    /// error and the space is not registered.
    pub fn register_clip(
        &self,
        r: &AssetRef,
        world: &mut fynkel_core::World,
    ) -> Result<bool, Diagnostic> {
        if r.asset.is_nil() || world.has_clip(r) || world.has_blend(r) {
            return Ok(false);
        }
        if self.is_blend(r) {
            let space = self.blend(r)?;
            for p in &space.points {
                if !world.has_clip(&p.clip) {
                    let clip = self.clip(&p.clip)?;
                    world.register_clip(p.clip.clone(), (*clip).clone());
                }
            }
            world.register_blend(r.clone(), (*space).clone());
            return Ok(true);
        }
        let clip = self.clip(r)?;
        world.register_clip(r.clone(), (*clip).clone());
        Ok(true)
    }

    /// Whether `r` names a blend-space document (no sub).
    pub fn is_blend(&self, r: &AssetRef) -> bool {
        r.sub.is_empty()
            && self
                .lock()
                .index
                .get(r.asset)
                .is_some_and(|e| e.kind == "document/blend")
    }

    /// Registers the skin `r` with `world` unless it is nil or already
    /// there; `Ok(true)` when this call registered it.
    pub fn register_skin(
        &self,
        r: &AssetRef,
        world: &mut fynkel_core::World,
    ) -> Result<bool, Diagnostic> {
        if r.asset.is_nil() || world.has_skin(r) {
            return Ok(false);
        }
        let skin = self.skin(r)?;
        world.register_skin(r.clone(), (*skin).clone());
        Ok(true)
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
        s.environments.retain(|r, _| !gone.contains(&r.asset));
        s.materials.retain(|r, _| !gone.contains(&r.asset));
        s.clips.retain(|r, _| !gone.contains(&r.asset));
        s.blends.retain(|r, _| !gone.contains(&r.asset));
        s.volumes.retain(|r, _| !gone.contains(&r.asset));
        s.physics_materials.retain(|r, _| !gone.contains(&r.asset));
        s.skins.retain(|r, _| !gone.contains(&r.asset));
        s.sounds.retain(|r, _| !gone.contains(&r.asset));
        s.metas.retain(|id, _| !gone.contains(id));
        s.evictions += 1;
    }

    /// Drops every resident payload (counts stay; payloads reload on use).
    pub fn clear(&self) {
        let mut s = self.lock();
        s.meshes.clear();
        s.textures.clear();
        s.environments.clear();
        s.materials.clear();
        s.clips.clear();
        s.blends.clear();
        s.volumes.clear();
        s.physics_materials.clear();
        s.skins.clear();
        s.metas.clear();
        s.evictions += 1;
    }

    /// Makes `m` resident as the mesh `r` without a cooked object behind
    /// it: a tool's stand-in geometry (`fynkel thumbnail`'s preview sphere
    /// when the project has no primitives), drawn by every renderer on
    /// this store like a cooked mesh. `r` should name an id the cache
    /// does not have; the mesh goes with the other payloads on [`clear`]
    /// or when a release evicts `r`'s id.
    ///
    /// [`clear`]: Assets::clear
    pub fn insert_mesh(&self, r: &AssetRef, m: MeshPayload) {
        self.lock().meshes.insert(r.clone(), Arc::new(m));
    }

    pub fn mesh(&self, r: &AssetRef) -> Result<Arc<MeshPayload>, Diagnostic> {
        {
            let s = self.lock();
            if let Some(m) = s.runtime_meshes.get(r).or_else(|| s.meshes.get(r)) {
                return Ok(m.clone());
            }
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

    /// The prefiltered environment cooked next to an HDR image (E1):
    /// `Ok(None)` for a texture that has none (an 8-bit sky lights nothing
    /// — the constant ambient stays), an error only when the object cannot
    /// be read.
    pub fn environment(&self, r: &AssetRef) -> Result<Option<Arc<EnvironmentPayload>>, Diagnostic> {
        if let Some(e) = self.lock().environments.get(r) {
            return Ok(e.clone());
        }
        let payload = if self.has_chunk(r, KIND_ENVIRONMENT)? {
            let bytes = self.chunk(r, KIND_ENVIRONMENT)?;
            let payload = decode_environment(&bytes).ok_or_else(|| self.malformed(r, "ENVM"))?;
            Some(Arc::new(payload))
        } else {
            None
        };
        self.lock().environments.insert(r.clone(), payload.clone());
        Ok(payload)
    }

    /// The cooked `PbrMaterial`. A custom-shader material has no `MATL`
    /// chunk (`FYN-R0006`).
    pub fn material(&self, r: &AssetRef) -> Result<PbrMaterial, Diagnostic> {
        {
            let s = self.lock();
            if let Some(m) = s.runtime_materials.get(r).or_else(|| s.materials.get(r)) {
                return Ok((**m).clone());
            }
        }
        let bytes = self.chunk(r, KIND_MATERIAL)?;
        let m = decode_material(&bytes).ok_or_else(|| self.malformed(r, "MATL"))?;
        self.lock().materials.insert(r.clone(), Arc::new(m.clone()));
        Ok(m)
    }

    /// The cooked sound as the audio engine's clip (M3-11): a `CLIP`
    /// chunk's interleaved `f32` frames, mono or stereo — or, for a sound
    /// the sidecar marked `stream: true`, the `STRM` chunk's encoded
    /// source, decoded by the engine as it plays.
    pub fn sound(&self, r: &AssetRef) -> Result<Arc<AudioClip>, Diagnostic> {
        if let Some(c) = self.lock().sounds.get(r) {
            return Ok(c.clone());
        }
        let clip = if self.has_chunk(r, KIND_STREAM)? {
            let bytes = self.chunk(r, KIND_STREAM)?;
            let p = decode_stream(&bytes).ok_or_else(|| self.malformed(r, "STRM"))?;
            AudioClip::streamed(p.sample_rate, p.channels, p.frames, &p.codec, p.bytes)
                .map_err(|msg| self.malformed(r, "STRM").with_suggestion(msg))?
        } else {
            let bytes = self.chunk(r, KIND_CLIP)?;
            let payload = decode_clip(&bytes).ok_or_else(|| self.malformed(r, "CLIP"))?;
            AudioClip::new(payload.sample_rate, payload.channels, payload.samples)
                .map_err(|msg| self.malformed(r, "CLIP").with_suggestion(msg))?
        };
        self.lock().sounds.insert(r.clone(), clip.clone());
        Ok(clip)
    }

    /// Whether the object has a chunk of `kind` for `r`'s sub-asset
    /// (`Err` when the object itself is missing or unreadable).
    fn has_chunk(&self, r: &AssetRef, kind: [u8; 4]) -> Result<bool, Diagnostic> {
        let mut s = self.lock();
        let entry = s
            .index
            .object(r.asset)
            .map_err(|d| d.with_pointer(r.sub.clone()))?
            .clone();
        let meta = s.meta(&entry)?;
        Ok(meta
            .chunks
            .iter()
            .any(|(k, sub, _)| *k == kind && *sub == r.sub))
    }

    /// The cooked animation clip (an `ANIM` chunk, `anim/<a>`) as the
    /// core's `Clip`, validated (M3-19).
    pub fn clip(&self, r: &AssetRef) -> Result<Arc<Clip>, Diagnostic> {
        if let Some(c) = self.lock().clips.get(r) {
            return Ok(c.clone());
        }
        let bytes = self.chunk(r, KIND_ANIMATION)?;
        let payload = decode_animation(&bytes).ok_or_else(|| self.malformed(r, "ANIM"))?;
        let clip = Clip {
            name: r.to_string(),
            duration: payload.duration,
            channels: payload
                .channels
                .into_iter()
                .map(|c| {
                    Some(Channel {
                        target: c.target,
                        property: match c.property {
                            ANIM_PROPERTY_TRANSLATION => Property::Translation,
                            ANIM_PROPERTY_ROTATION => Property::Rotation,
                            ANIM_PROPERTY_SCALE => Property::Scale,
                            _ => return None,
                        },
                        interpolation: match c.interpolation {
                            ANIM_INTERPOLATION_STEP => Interpolation::Step,
                            ANIM_INTERPOLATION_LINEAR => Interpolation::Linear,
                            ANIM_INTERPOLATION_CUBIC_SPLINE => Interpolation::CubicSpline,
                            _ => return None,
                        },
                        times: c.times,
                        values: c.values,
                    })
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| self.malformed(r, "ANIM"))?,
            markers: payload
                .markers
                .into_iter()
                .map(|m| Marker {
                    time: m.time,
                    name: m.name,
                })
                .collect(),
        };
        clip.validate().map_err(|why| {
            let mut d = self.malformed(r, "ANIM");
            d.message = format!("{}: {why}", d.message);
            d
        })?;
        let clip = Arc::new(clip);
        self.lock().clips.insert(r.clone(), clip.clone());
        Ok(clip)
    }

    /// The cooked blend space (M3-19c: the document's JSON chunk) as the
    /// core's `BlendSpace`, validated. The cook checked the document
    /// against the schema and the database, so this only reads it.
    pub fn blend(&self, r: &AssetRef) -> Result<Arc<BlendSpace>, Diagnostic> {
        if let Some(b) = self.lock().blends.get(r) {
            return Ok(b.clone());
        }
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
        if entry.kind != "document/blend" || !r.sub.is_empty() {
            return Err(Diagnostic::error(
                Code::RUNTIME_CHUNK_MISSING,
                format!(
                    "{}: {r} is a {} asset, not a blend space",
                    entry.path, entry.kind
                ),
            )
            .with_id(r.asset.to_string()));
        }
        let bytes = self.read_chunk(&entry, &meta, KIND_JSON, "")?;
        let text = String::from_utf8(bytes).map_err(|_| self.malformed_entry(&entry, "JSON"))?;
        let doc = Document::parse(DocumentKind::Blend, Some(&entry.path), &text)
            .map_err(|_| self.malformed_entry(&entry, "JSON"))?;
        let blend = fynkel_document::Blend::from_document(doc);
        let space = BlendSpace {
            name: blend.name().to_string(),
            parameter: blend.parameter.clone(),
            points: blend
                .points
                .iter()
                .map(|p| BlendPoint {
                    at: p.at,
                    clip: p.clip.clone(),
                })
                .collect(),
            sync: BlendSync::Phase,
        };
        space.validate().map_err(|why| {
            let mut d = self.malformed_entry(&entry, "JSON");
            d.message = format!("{}: {why}", d.message);
            d
        })?;
        let space = Arc::new(space);
        self.lock().blends.insert(r.clone(), space.clone());
        Ok(space)
    }

    /// The cooked volume profile (R3a: the document's JSON chunk) as the
    /// core's `VolumeProfile`. The cook checked the document against the
    /// schema, so this only reads it.
    pub fn volume_profile(&self, r: &AssetRef) -> Result<VolumeProfile, Diagnostic> {
        if let Some(v) = self.lock().volumes.get(r) {
            return Ok(*v);
        }
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
        if entry.kind != "document/volume" || !r.sub.is_empty() {
            return Err(Diagnostic::error(
                Code::RUNTIME_CHUNK_MISSING,
                format!(
                    "{}: {r} is a {} asset, not a volume profile",
                    entry.path, entry.kind
                ),
            )
            .with_id(r.asset.to_string()));
        }
        let bytes = self.read_chunk(&entry, &meta, KIND_JSON, "")?;
        let text = String::from_utf8(bytes).map_err(|_| self.malformed_entry(&entry, "JSON"))?;
        let doc = Document::parse(DocumentKind::Volume, Some(&entry.path), &text)
            .map_err(|_| self.malformed_entry(&entry, "JSON"))?;
        let volume = fynkel_document::VolumeDocument::from_document(doc);
        if let Some(d) = volume.read_diagnostics().first() {
            let mut m = self.malformed_entry(&entry, "JSON");
            m.message = format!("{}: {}", m.message, d.message);
            return Err(m);
        }
        let profile = volume.profile;
        self.lock().volumes.insert(r.clone(), profile);
        Ok(profile)
    }

    /// The cooked physics material (ADR-0025: the document's JSON chunk)
    /// as physics' `PhysicsMaterial`. The cook checked the document
    /// against the schema, so this only reads it.
    pub fn physics_material(&self, r: &AssetRef) -> Result<PhysicsMaterial, Diagnostic> {
        if let Some(m) = self.lock().physics_materials.get(r) {
            return Ok(*m);
        }
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
        if entry.kind != "document/physics_material" || !r.sub.is_empty() {
            return Err(Diagnostic::error(
                Code::RUNTIME_CHUNK_MISSING,
                format!(
                    "{}: {r} is a {} asset, not a physics material",
                    entry.path, entry.kind
                ),
            )
            .with_id(r.asset.to_string()));
        }
        let bytes = self.read_chunk(&entry, &meta, KIND_JSON, "")?;
        let text = String::from_utf8(bytes).map_err(|_| self.malformed_entry(&entry, "JSON"))?;
        let doc = Document::parse(DocumentKind::PhysicsMaterial, Some(&entry.path), &text)
            .map_err(|_| self.malformed_entry(&entry, "JSON"))?;
        let view = fynkel_document::PhysicsMaterialDocument::from_document(doc);
        if let Some(d) = view.read_diagnostics().first() {
            let mut m = self.malformed_entry(&entry, "JSON");
            m.message = format!("{}: {}", m.message, d.message);
            return Err(m);
        }
        let material = view.material;
        self.lock().physics_materials.insert(r.clone(), material);
        Ok(material)
    }

    /// The cooked JSON of a document asset (ADR-0027 §6: a data asset's,
    /// or any document kind's) with its kind name (`data`, `volume`…): the
    /// document's JSON chunk as the cook wrote it. Not cached: the SDK
    /// keeps what it makes of it.
    pub fn document_json(
        &self,
        id: fynkel_contracts::Uuid,
    ) -> Result<(String, String), Diagnostic> {
        let (entry, meta) = {
            let mut s = self.lock();
            let entry = s.index.object(id)?.clone();
            let meta = s.meta(&entry)?;
            (entry, meta)
        };
        let Some(kind) = entry.kind.strip_prefix("document/") else {
            return Err(Diagnostic::error(
                Code::RUNTIME_CHUNK_MISSING,
                format!(
                    "{}: {id} is a {} asset, not a document",
                    entry.path, entry.kind
                ),
            )
            .with_id(id.to_string()));
        };
        let bytes = self.read_chunk(&entry, &meta, KIND_JSON, "")?;
        let text = String::from_utf8(bytes).map_err(|_| self.malformed_entry(&entry, "JSON"))?;
        Ok((kind.to_string(), text))
    }

    /// The cooked skin (a `SKIN` chunk, `skin/<s>`) as the core's `Skin`,
    /// validated (M3-19).
    pub fn skin(&self, r: &AssetRef) -> Result<Arc<Skin>, Diagnostic> {
        if let Some(k) = self.lock().skins.get(r) {
            return Ok(k.clone());
        }
        let bytes = self.chunk(r, KIND_SKIN)?;
        let payload = decode_skin(&bytes).ok_or_else(|| self.malformed(r, "SKIN"))?;
        let skin = Skin {
            skeleton: payload.skeleton,
            joints: payload
                .joints
                .into_iter()
                .map(|j| Joint {
                    name: j.name,
                    parent: j.parent,
                    inverse_bind: glam::Mat4::from_cols_array(&j.inverse_bind),
                    rest: fynkel_core::Transform {
                        position: glam::Vec3::from_array(j.rest_translation),
                        rotation: glam::Quat::from_array(j.rest_rotation),
                        scale: glam::Vec3::from_array(j.rest_scale),
                    },
                })
                .collect(),
        };
        skin.validate().map_err(|why| {
            let mut d = self.malformed(r, "SKIN");
            d.message = format!("{}: {why}", d.message);
            d
        })?;
        let skin = Arc::new(skin);
        self.lock().skins.insert(r.clone(), skin.clone());
        Ok(skin)
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

    /// The cooked prefab `id` as a `SceneDescription` (its entities, roots
    /// parentless) plus the authored behaviours and the roots, for
    /// `World::instantiate` (M3-08b). The cook expanded nested instances
    /// and checked every component, so this only decodes; `Err` as for
    /// [`Assets::scene`] (`FYN-R0006` for an asset that is not a prefab,
    /// `R0008` when the chunk does not describe: recook).
    pub fn prefab(&self, id: Uuid) -> Result<CookedPrefab, Vec<Diagnostic>> {
        let (entry, meta) = {
            let mut s = self.lock();
            let entry = s.index.object(id).map_err(|d| vec![d])?.clone();
            let meta = s.meta(&entry).map_err(|d| vec![d])?;
            (entry, meta)
        };
        // A prefab document, or an imported source whose cooked object
        // carries its node hierarchy as the `prefab` sub-asset (M3-19a:
        // `{ asset: <source>, sub: "prefab" }` in a document, the source's
        // id from `World.instantiate`).
        let sub = if entry.kind == "document/prefab" {
            ""
        } else if meta
            .chunks
            .iter()
            .any(|(k, s, _)| *k == KIND_JSON && s == IMPORTED_PREFAB_SUB)
        {
            IMPORTED_PREFAB_SUB
        } else {
            return Err(vec![Diagnostic::error(
                Code::RUNTIME_CHUNK_MISSING,
                format!(
                    "{}: {id} is a {} asset without a node hierarchy, not a prefab",
                    entry.path, entry.kind
                ),
            )
            .with_id(id.to_string())
            .with_suggestion(
                "instantiate a prefab document or an imported model (glTF) whose sidecar lists the `prefab` sub-asset",
            )]);
        };
        let bytes = self
            .read_chunk(&entry, &meta, KIND_JSON, sub)
            .map_err(|d| vec![d])?;
        let text =
            String::from_utf8(bytes).map_err(|_| vec![self.malformed_entry(&entry, "JSON")])?;
        let path = entry.path.clone();
        let doc =
            Document::parse(DocumentKind::Prefab, Some(&path), &text).map_err(|mut diags| {
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
        let prefab = fynkel_document::prefab::Prefab::from_document(doc);
        let mut diagnostics = prefab.read_diagnostics().to_vec();
        if prefab.has_instances() {
            diagnostics.push(self.scene_invalid(&entry));
        }
        let described = describe(&prefab.scene(), &schemas);
        diagnostics.extend(described.diagnostics);
        if fynkel_contracts::has_errors(&diagnostics) {
            diagnostics.insert(0, self.scene_invalid(&entry));
            return Err(diagnostics);
        }
        let closure = self.lock().index.closure(id);
        Ok(CookedPrefab {
            id,
            name: described.description.name.clone(),
            path,
            root: Uuid::from_bytes(*prefab.root.as_bytes()),
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
            if kind_bytes == KIND_JSON
                && (c["documentKind"] == "scene" || c["documentKind"] == "prefab")
            {
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

/// The axis-aligned bounds of `vertices`' positions (zero for none).
fn bounds_of(vertices: &[fynkel_cooked::payload::Vertex]) -> ([f32; 3], [f32; 3]) {
    let Some(first) = vertices.first() else {
        return ([0.0; 3], [0.0; 3]);
    };
    let (mut min, mut max) = (first.position, first.position);
    for v in vertices {
        for k in 0..3 {
            min[k] = min[k].min(v.position[k]);
            max[k] = max[k].max(v.position[k]);
        }
    }
    (min, max)
}
