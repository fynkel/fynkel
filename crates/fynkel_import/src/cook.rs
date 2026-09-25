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
use fynkel_cooked::{
    Writer, KIND_ANIMATION, KIND_BLOB, KIND_CLIP, KIND_ENVIRONMENT, KIND_JSON, KIND_MATERIAL,
    KIND_MESH, KIND_SKIN, KIND_STREAM, KIND_TEXTURE,
};
use fynkel_document::asset::{asset_refs, AssetDatabase, AssetEntry, AssetKind, Meta};
use fynkel_document::prefab::{expand, Prefab, PrefabLibrary};
use fynkel_document::{describe, json, Document, DocumentKind, Scene, SchemaSet};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::cache::{
    hash_bytes, Cache, CacheKey, KeyInputs, Manifest, ManifestChunk, ManifestDependency,
    ManifestInput, Stamps, CACHE_VERSION,
};
use crate::intermediate::{AudioData, ColorSpace, Intermediate, MeshData, TextureData};
use crate::Limits;

/// The profile a cook targets when none is named.
pub const DEFAULT_PROFILE: &str = "dev";

/// Texture chunk formats (`TEXR` payload, `format` field).
pub use fynkel_cooked::payload::{
    decode_clip, decode_environment, decode_mesh, decode_stream, decode_texture,
    ClipPayload as ClipChunk, EnvironmentPayload as EnvironmentChunk, MeshPayload as MeshChunk,
    StreamPayload as StreamChunk, TexturePayload as TextureChunk, TEXTURE_FORMAT_RGBA16F_LINEAR,
    TEXTURE_FORMAT_RGBA8_LINEAR, TEXTURE_FORMAT_RGBA8_SRGB,
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
    /// The project the database belongs to, recorded in the cache index so
    /// a host can find the startup scene without the project file (M3-03:
    /// hosts read the cache only). `None` cooks a bare asset root.
    pub project: Option<CookProject>,
    /// Documents to cook as this text instead of their file, by canonical
    /// path — the editor's unsaved documents when Play cooks what the
    /// editor holds (ADR-0021 P5). The file is neither read nor written;
    /// the key hashes the text, so the next cook from disk sees the file
    /// again as a change.
    pub live: HashMap<PathBuf, String>,
}

/// What the cache index says about its project.
#[derive(Clone, Debug, PartialEq)]
pub struct CookProject {
    pub name: String,
    /// The startup scene's asset id (`fynkel_world_load_project` loads it).
    pub startup_scene: Option<Uuid>,
    /// The project's quality profiles, carried into the index for the
    /// renderer (`fynkel_assets::ProjectInfo::quality`; §19.2d item 5).
    pub quality: Option<fynkel_document::project::Quality>,
    /// The project's input actions, carried into the index for the
    /// runtime's binding table (`fynkel_assets::ProjectInfo::input_actions`;
    /// §19.2d item 7).
    pub input_actions: Vec<fynkel_document::project::InputAction>,
    /// The project's `physics` block (gravity), carried into the index for
    /// the runtime (`fynkel_assets::ProjectInfo::physics`; M3-08).
    pub physics: fynkel_document::project::PhysicsSettings,
    /// The project's `layers` and `tags` (ADR-0020), carried the same way.
    pub layers_and_tags: fynkel_document::project::LayersAndTags,
}

impl Default for CookOptions {
    fn default() -> Self {
        CookOptions {
            profile: DEFAULT_PROFILE.to_string(),
            limits: Limits::default(),
            force: false,
            project: None,
            live: HashMap::new(),
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
    /// What the key covers and what must cook first: every resolved
    /// reference.
    dependencies: Vec<Uuid>,
    /// What a load of the cooked object needs (the index's
    /// `dependencies`): the same, except for a rig, whose clip sources
    /// are copied into its object and stay out of the bundle (M3-19b).
    runtime_dependencies: Vec<Uuid>,
    /// The loaded document (documents) or sidecar (sources). A source's
    /// bytes are not held: the import reads the file when (and only when)
    /// the asset must cook, one at a time.
    input: Input,
    failed: bool,
}

enum Input {
    Document(Document),
    Source { meta: Meta },
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
    // The built-in shapes, unless the project has its own (ADR-0026 §4).
    let with_builtins = crate::builtin::with_builtins(db, cache.root(), schemas, &mut diagnostics);
    let db = with_builtins.as_ref().unwrap_or(db);
    let entries = db.entries();
    let by_id: HashMap<Uuid, usize> = entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();

    // 1. Inputs and dependencies for every asset. A source unchanged since
    // the last cook (size and time) is not read again for its hash.
    let mut stamps = Stamps::load(cache, opts.force);
    let prepared: Vec<Prepared> = entries
        .iter()
        .map(|e| prepare(db, schemas, e, &opts.live, &mut stamps, &mut diagnostics))
        .collect();
    if let Err(d) = stamps.save(cache) {
        diagnostics.push(
            Diagnostic::warning(d.code, d.message).with_document(
                d.document
                    .unwrap_or_else(|| cache.stamps_path().display().to_string()),
            ),
        );
    }

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

    // Every prefab document that read, for expanding scenes (a scene's
    // key covers its prefabs through the reference dependencies).
    let mut library = PrefabLibrary::empty();
    for p in &prepared {
        if let Input::Document(doc) = &p.input {
            if doc.kind == DocumentKind::Prefab && !p.failed {
                library.add(Prefab::from_document(doc.clone()));
            }
        }
    }
    // 3. Cook in that order; keys need the dependencies' keys. Sources
    // go first (they depend on nothing, so the order stays topological):
    // a source's cooked `prefab` chunk — its node hierarchy — joins the
    // library before any scene or prefab that instantiates it
    // (`{ asset: <source>, sub: "prefab" }`, M3-19a), whether the source
    // was cooked now or read back from the cache.
    let (sources, documents): (Vec<usize>, Vec<usize>) = order
        .into_iter()
        .partition(|i| matches!(entries[*i].kind, AssetKind::Source { .. }));
    let mut keys: HashMap<Uuid, CacheKey> = HashMap::new();
    let mut results: Vec<Option<CookedAsset>> = (0..entries.len()).map(|_| None).collect();
    for i in sources {
        cook_index(
            i,
            db,
            schemas,
            &library,
            entries,
            &prepared,
            &in_cycle,
            &by_id,
            cache,
            opts,
            &mut keys,
            &mut results,
            &mut diagnostics,
        );
        let entry = &entries[i];
        // Only a `JSON` chunk can be the prefab; a texture or a sound is
        // not opened again to find that out.
        if let Some(file) = results[i]
            .as_ref()
            .filter(|r| r.chunks.iter().any(|c| c.kind == "JSON"))
            .and_then(|r| r.file.as_ref())
        {
            match imported_prefab(file) {
                Ok(Some(doc)) => library.add_imported(Prefab::from_document(doc), entry.id, "prefab"),
                Ok(None) => {}
                Err(msg) => diagnostics.push(
                    Diagnostic::warning(
                        Code::IMPORT_INVALID_DATA,
                        format!("{}: its cooked prefab does not read back ({msg}); scenes instancing it will not expand", entry.path),
                    )
                    .with_document(entry.file(db.root()).display().to_string())
                    .with_id(entry.id.to_string()),
                ),
            }
        }
    }
    // A rig's `prefab` is its skeleton source's hierarchy under the rig's
    // own id (M3-19b), so `{ asset: <rig>, sub: "prefab" }` expands like
    // the source's; a rig whose skeleton did not cook fails below.
    for p in &prepared {
        let Input::Document(doc) = &p.input else {
            continue;
        };
        if doc.kind != DocumentKind::Rig || p.failed {
            continue;
        }
        let rig = fynkel_document::Rig::from_document(doc.clone());
        let Some(skeleton) = rig.skeleton.as_ref().filter(|s| s.sub.is_empty()) else {
            continue;
        };
        let source = fynkel_contracts::AssetRef {
            asset: skeleton.asset,
            sub: fynkel_document::rig::RIG_PREFAB_SUB.to_string(),
        };
        let Some(prefab) = library.get_ref(&source) else {
            continue;
        };
        let shown = format!("{}#prefab", doc.path.as_deref().unwrap_or(""));
        match prefab
            .document
            .write()
            .map_err(|d| vec![d])
            .and_then(|text| rig.adopt_prefab(&text, &shown))
        {
            Ok(adopted) => library.add_imported(
                Prefab::from_document(adopted),
                rig.id(),
                fynkel_document::rig::RIG_PREFAB_SUB,
            ),
            Err(d) => diagnostics.extend(d),
        }
    }
    // Nested prefabs expand once; a prefab that cannot (a cycle, a bad
    // inner override) stays as it is and fails its own cook below, where
    // the diagnostics are re-derived against its cooked object.
    let (library, _) = library.flattened(schemas);
    for i in documents {
        cook_index(
            i,
            db,
            schemas,
            &library,
            entries,
            &prepared,
            &in_cycle,
            &by_id,
            cache,
            opts,
            &mut keys,
            &mut results,
            &mut diagnostics,
        );
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
    if let Err(d) = write_index(cache, &report, opts.project.as_ref(), db) {
        report.diagnostics.push(d);
    }
    report
}

/// The `prefab` JSON chunk of a cooked source (`Ok(None)` when it has
/// none: a texture, a sound, a file), parsed as a prefab document.
fn imported_prefab(file: &Path) -> Result<Option<Document>, String> {
    let mut cf = fynkel_cooked::ChunkFile::open_path(file).map_err(|e| e.to_string())?;
    let meta = cf.meta(1 << 20).map_err(|e| e.to_string())?;
    let meta: Value = serde_json::from_slice(&meta).map_err(|e| e.to_string())?;
    let index = meta["chunks"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["kind"] == "JSON" && c["sub"] == "prefab")
        .and_then(|c| c["index"].as_u64());
    let Some(index) = index else {
        return Ok(None);
    };
    let bytes = cf
        .read(index as usize, 64 << 20)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let path = file.display().to_string();
    Document::parse(DocumentKind::Prefab, Some(&path), &text)
        .map(Some)
        .map_err(|d| d.first().map(|d| d.message.clone()).unwrap_or_default())
}

/// One asset of `cook_project`'s order: its key from the dependencies'
/// keys, the cached object when the key is in the cache, `cook_one`
/// otherwise; the result lands in `results[i]` and its key in `keys`.
#[allow(clippy::too_many_arguments)]
fn cook_index(
    i: usize,
    db: &AssetDatabase,
    schemas: &SchemaSet,
    library: &PrefabLibrary,
    entries: &[AssetEntry],
    prepared: &[Prepared],
    in_cycle: &[bool],
    by_id: &HashMap<Uuid, usize>,
    cache: &Cache,
    opts: &CookOptions,
    keys: &mut HashMap<Uuid, CacheKey>,
    results: &mut [Option<CookedAsset>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    'cook: {
        let entry = &entries[i];
        let p = &prepared[i];
        let base = CookedAsset {
            id: entry.id,
            path: entry.path.clone(),
            kind: p.kind.clone(),
            key: None,
            file: None,
            status: Status::Failed,
            dependencies: p.runtime_dependencies.clone(),
            chunks: Vec::new(),
            bytes: 0,
        };
        if p.failed || in_cycle[i] {
            results[i] = Some(base);
            break 'cook;
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
            break 'cook;
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
                    break 'cook;
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
        match cook_one(
            db,
            schemas,
            library,
            entry,
            p,
            &inputs,
            &key,
            cache,
            opts,
            diagnostics,
        ) {
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
}

/// Where a project's cache lives: `<dir>/.fynkel/cache`.
pub fn project_cache_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".fynkel").join("cache")
}

/// `fynkel cook` for a project directory: reads `fynkel.project.json`,
/// validates it, scans its asset root with the project's behaviour
/// schemas, records the project (name, startup scene) in the index and
/// cooks into `cache` (default [`project_cache_dir`]). `Err` when the
/// project file, the database or the project's references have errors
/// (nothing is cooked then); a cook with failed assets is an `Ok` report
/// whose `has_errors()` says so.
pub fn cook_project(
    project_dir: &Path,
    cache: Option<&Path>,
    opts: &CookOptions,
) -> Result<CookReport, Vec<Diagnostic>> {
    use fynkel_document::Project;
    let project = Project::read(project_dir)?;
    let schemas = project
        .schemas(&SchemaSet::embedded(), project_dir)
        .map_err(|d| vec![d])?;
    let mut diagnostics = project.read_diagnostics().to_vec();
    diagnostics.extend(project.validate(&schemas));
    if has_errors(&diagnostics) {
        return Err(diagnostics);
    }
    let assets = project.assets_dir(project_dir);
    let (db, scan) = AssetDatabase::scan(&assets, &schemas);
    diagnostics.extend(scan);
    let cache_dir = cache
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project_cache_dir(project_dir));
    // References to the built-in shapes resolve without an installed copy.
    let db =
        crate::builtin::with_builtins(&db, &cache_dir, &schemas, &mut diagnostics).unwrap_or(db);
    diagnostics.extend(project.validate_against(&db, &schemas));
    if has_errors(&diagnostics) {
        return Err(diagnostics);
    }
    let startup_scene = project
        .startup_scene
        .as_ref()
        .and_then(|r| db.resolve(r).ok())
        .map(|r| r.entry.id);
    let opts = CookOptions {
        project: Some(CookProject {
            name: project.name().to_string(),
            startup_scene,
            quality: project.quality.clone(),
            input_actions: project.input_actions.clone(),
            physics: project.physics.clone(),
            layers_and_tags: project.layers_and_tags.clone(),
        }),
        ..opts.clone()
    };
    let mut report = cook(&db, &schemas, &Cache::at(&cache_dir), &opts);
    // Scan warnings (a sidecar behind its import, say) belong to the cook.
    let mut all = diagnostics;
    all.append(&mut report.diagnostics);
    report.diagnostics = all;
    Ok(report)
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
    live: &HashMap<PathBuf, String>,
    stamps: &mut Stamps,
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
            let text = match live.get(&file.canonicalize().unwrap_or_else(|_| file.clone())) {
                Some(text) => Ok(text.clone()),
                None => std::fs::read_to_string(&file),
            };
            match text {
                Ok(text) => {
                    source = hash_bytes(text.as_bytes());
                    match Document::parse(*kind, Some(&shown), &text) {
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
            let dependencies: Vec<Uuid> = dependencies.into_iter().collect();
            let runtime_dependencies = match &input {
                Input::Document(doc) if *kind == DocumentKind::Rig => {
                    fynkel_document::Rig::from_document(doc.clone())
                        .skeleton
                        .and_then(|s| db.resolve(&s).ok().map(|r| r.entry.id))
                        .into_iter()
                        .collect()
                }
                _ => dependencies.clone(),
            };
            Prepared {
                kind: format!("document/{}", kind.name()),
                version,
                source,
                extra_inputs: Vec::new(),
                // A document's cooked form depends on the component schemas
                // (a scene is described and expanded against them), so the
                // fingerprint is an input: a schema change recooks it.
                settings: document_settings(),
                dependencies,
                runtime_dependencies,
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
                    let mut diags = meta.validate(schemas);
                    diags.extend(meta.validate_remaps(db));
                    diags.extend(db.check_references(&meta.document));
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
            let names: fn(&[u8]) -> Vec<String> = if is_gltf_json(&file) {
                gltf_extra_files
            } else if has_extension(&file, "fbx") {
                crate::fbx::external_files
            } else {
                |_| Vec::new()
            };
            let (source, extra) = match stamps.hash(&file, names) {
                Ok(h) => h,
                Err(e) => {
                    diagnostics.push(io_fail(e));
                    failed = true;
                    (hash_bytes(b""), Vec::new())
                }
            };
            let extra_inputs = hash_extra_inputs(&file, extra, stamps);
            // What the import reads from the sidecar besides the file: its
            // settings, its key scheme (`subkey`) and the sub-asset ids it
            // writes into the cooked documents.
            let settings = meta
                .as_ref()
                .map(|m| {
                    let subs: Vec<String> = m
                        .sub_assets
                        .iter()
                        .map(|(k, id)| format!("{k}={id}"))
                        .collect();
                    format!(
                        "{}\nformatVersion={}\n{}",
                        json::write(&m.settings),
                        m.document.envelope.format_version,
                        subs.join("\n")
                    )
                })
                .unwrap_or_else(|| "{}".to_string());
            // The importer's own version: a sidecar records the version that
            // last wrote its settings, not the code that imports it now.
            let version = crate::IMPORTERS
                .iter()
                .find(|(name, _)| name == importer)
                .map_or(*importer_version, |(_, v)| *v);
            // A remapped material is loaded with the model (its hierarchy
            // points at it) but is not a key input: the cooked model holds
            // only its id, which the settings already cover — and a
            // material extracted from the model points back at its images.
            let runtime_dependencies: Vec<Uuid> = meta
                .as_ref()
                .map(|m| {
                    m.material_remaps()
                        .iter()
                        .filter_map(|(_, r)| db.resolve(r).ok().map(|r| r.entry.id))
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default();
            Prepared {
                kind: importer.clone(),
                version,
                source,
                extra_inputs,
                settings,
                dependencies: Vec::new(),
                runtime_dependencies,
                input: match meta {
                    Some(meta) if !failed => Input::Source { meta },
                    _ => Input::None,
                },
                failed,
            }
        }
    }
}

/// The `settings` key input of every document: the schema fingerprint.
fn document_settings() -> String {
    format!("{{\"schemaFingerprint\":\"{}\"}}", schema_fingerprint_hex())
}

/// This build's schema fingerprint as the lock and the cache index print it.
pub fn schema_fingerprint_hex() -> String {
    format!("{:#018x}", fynkel_core::schema::schema_fingerprint())
}

fn is_gltf_json(path: &Path) -> bool {
    has_extension(path, "gltf")
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// The files a `.gltf` may read besides itself: every non-`data:`
/// `buffers[].uri` and `images[].uri` (an `.fbx`'s are its file textures
/// that are not embedded, `fbx::external_files`). Whether the importer is
/// allowed to read them is its business; they are inputs either way.
fn gltf_extra_files(bytes: &[u8]) -> Vec<String> {
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
    names.into_iter().collect()
}

/// Each relative file next to `path`, hashed: its bytes when it is a
/// plain relative path that exists, "missing" when it does not (so its
/// appearance later changes the key too), "denied" when the importer
/// would refuse the path anyway.
fn hash_extra_inputs(
    path: &Path,
    names: Vec<String>,
    stamps: &mut Stamps,
) -> Vec<(String, CacheKey)> {
    let dir = path.parent().unwrap_or(Path::new("."));
    names
        .into_iter()
        .map(|uri| {
            let plain = !uri.contains("://") && !uri.starts_with('/') && !uri.contains("..");
            let hash = if plain {
                match stamps.hash(&dir.join(&uri), |_| Vec::new()) {
                    Ok((h, _)) => h,
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
#[allow(clippy::too_many_arguments)]
fn cook_one(
    db: &AssetDatabase,
    schemas: &SchemaSet,
    library: &PrefabLibrary,
    entry: &AssetEntry,
    p: &Prepared,
    inputs: &KeyInputs,
    key: &CacheKey,
    cache: &Cache,
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
        Input::Document(doc) if doc.kind == DocumentKind::Scene => {
            chunks.push(scene_chunk(
                doc,
                schemas,
                library,
                &entry.name,
                diagnostics,
            )?);
        }
        Input::Document(doc) if doc.kind == DocumentKind::Prefab => {
            chunks.push(prefab_chunk(
                doc,
                schemas,
                library,
                &entry.name,
                diagnostics,
            )?);
        }
        Input::Document(doc) if doc.kind == DocumentKind::Rig => {
            rig_chunks(
                doc,
                db,
                schemas,
                library,
                entry,
                inputs,
                cache,
                &mut chunks,
                &mut sub_assets,
                diagnostics,
            )?;
        }
        Input::Document(doc) => {
            let text = match doc.write() {
                Ok(t) => t,
                Err(d) => {
                    diagnostics.push(d.with_document(shown));
                    return None;
                }
            };
            let mut info = json!({ "documentKind": doc.kind.name() });
            // A blend space (M3-19c) is written as it is, once its points
            // are in order and name clips: the runtime reads the document.
            if doc.kind == DocumentKind::Blend {
                let blend = fynkel_document::Blend::from_document(doc.clone());
                let mut checked = blend.validate(schemas);
                checked.extend(blend.validate_references(db));
                let bad = has_errors(&checked);
                diagnostics.extend(checked);
                if bad {
                    return None;
                }
                info["parameter"] = json!(blend.parameter);
                info["points"] = json!(blend.points.len());
                info["sync"] = json!(blend.sync);
            }
            // A volume profile (R3a) likewise: its fields in range and
            // readable, then written as it is for the renderer to blend.
            if doc.kind == DocumentKind::Volume {
                let volume = fynkel_document::VolumeDocument::from_document(doc.clone());
                let checked = volume.validate(schemas);
                let bad = has_errors(&checked);
                diagnostics.extend(checked);
                if bad {
                    return None;
                }
            }
            // A physics material (ADR-0025) likewise, for the physics
            // world to read at load.
            if doc.kind == DocumentKind::PhysicsMaterial {
                let material = fynkel_document::PhysicsMaterialDocument::from_document(doc.clone());
                let checked = material.validate(schemas);
                let bad = has_errors(&checked);
                diagnostics.extend(checked);
                if bad {
                    return None;
                }
            }
            chunks.push(ChunkPlan {
                kind: KIND_JSON,
                name: entry.name.clone(),
                sub: None,
                info,
                payload: text.into_bytes(),
            });
            if let Some(c) = material_chunk(doc, &entry.name, None) {
                chunks.push(c);
            }
        }
        Input::Source { meta } => {
            importer = Some(meta.importer.clone());
            importer_version = Some(meta.importer_version);
            for (sub, id) in &meta.sub_assets {
                sub_assets.insert(sub.clone(), *id);
            }
            if meta.importer == fynkel_document::asset::FILE_IMPORTER {
                let bytes = match std::fs::read(&file) {
                    Ok(b) => b,
                    Err(e) => {
                        diagnostics.push(
                            Diagnostic::error(Code::IO, format!("cannot read: {e}"))
                                .with_document(shown.clone()),
                        );
                        return None;
                    }
                };
                // The key hashed the file a moment ago; a copy that changed
                // since would be cached under the old content's key.
                if hash_bytes(&bytes) != p.source {
                    diagnostics.push(
                        Diagnostic::error(
                            Code::IO,
                            format!("{} changed during the cook; cook again", entry.path),
                        )
                        .with_document(shown.clone()),
                    );
                    return None;
                }
                chunks.push(ChunkPlan {
                    kind: KIND_BLOB,
                    name: entry.name.clone(),
                    sub: None,
                    info: json!({ "extension": file.extension().and_then(|e| e.to_str()).unwrap_or("") }),
                    payload: bytes,
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
        "dependencies": p.runtime_dependencies.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
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

/// A rig's cooked object (M3-19b) is self-contained for what a game
/// names: its own `JSON` (the document), the skeleton source's hierarchy
/// as its `prefab` chunk (adopted under the rig's id, through the library
/// like any prefab — expanded and described), and one `ANIM` chunk per
/// clip, `anim/<name>`, read from the clip source's cooked object and
/// matched to the skeleton by bone name ([`crate::rig::retarget`]; a
/// mismatch is `FYN-A0106` and fails the rig). Meshes, skins and
/// materials stay in the skeleton source, which the rig depends on at
/// runtime; the clip sources do not enter the bundle.
#[allow(clippy::too_many_arguments)]
fn rig_chunks(
    doc: &Document,
    db: &AssetDatabase,
    schemas: &SchemaSet,
    library: &PrefabLibrary,
    entry: &AssetEntry,
    inputs: &KeyInputs,
    cache: &Cache,
    chunks: &mut Vec<ChunkPlan>,
    sub_assets: &mut BTreeMap<String, Uuid>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<()> {
    use fynkel_document::rig::{Rig, RIG_PREFAB_SUB};
    let shown = entry.file(db.root()).display().to_string();
    let rig = Rig::from_document(doc.clone());
    let checked = rig.validate_references(db);
    let bad = has_errors(&checked);
    diagnostics.extend(checked);
    if bad {
        return None;
    }
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
    for (sub, id) in rig.sub_assets() {
        sub_assets.insert(sub, id);
    }
    let object_of = |asset: fynkel_contracts::AssetId| -> Option<PathBuf> {
        let id = Uuid::from_bytes(asset.0);
        inputs
            .dependencies
            .iter()
            .find(|(d, _)| *d == id)
            .map(|(_, key)| cache.object_path(key))
    };
    let failed = |diagnostics: &mut Vec<Diagnostic>, message: String| {
        diagnostics.push(
            Diagnostic::error(Code::IMPORT_INVALID_DATA, message)
                .with_document(shown.clone())
                .with_id(entry.id.to_string()),
        );
    };

    // The skeleton: its hierarchy (the library's adopted copy) and its
    // names.
    let skeleton = rig.skeleton.clone()?;
    let own = fynkel_contracts::AssetRef {
        asset: fynkel_contracts::Uuid::from_bytes(*rig.id().as_bytes()),
        sub: RIG_PREFAB_SUB.to_string(),
    };
    let Some(prefab) = library.get_ref(&own) else {
        failed(
            diagnostics,
            format!(
                "{}: the skeleton {} has no cooked hierarchy (its prefab sub-asset)",
                entry.path, skeleton
            ),
        );
        return None;
    };
    let mut hierarchy = prefab_chunk(&prefab.document, schemas, library, &entry.name, diagnostics)?;
    hierarchy.sub = Some(RIG_PREFAB_SUB.to_string());
    hierarchy.info["skeleton"] = Value::from(skeleton.asset.to_string());
    chunks.push(hierarchy);
    let Some(skeleton_object) = object_of(skeleton.asset) else {
        failed(
            diagnostics,
            format!("{}: the skeleton {} is not cooked", entry.path, skeleton),
        );
        return None;
    };
    let names = match crate::rig::cooked::skeleton_names(&skeleton_object) {
        Ok(n) => n,
        Err(e) => {
            failed(
                diagnostics,
                format!(
                    "{}: the skeleton's cooked object does not read ({e})",
                    entry.path
                ),
            );
            return None;
        }
    };

    // The clips, each against the skeleton.
    let mut ok = true;
    for clip in &rig.clips {
        let source_shown = db
            .get(Uuid::from_bytes(clip.asset.asset.0))
            .map(|e| e.path.clone())
            .unwrap_or_else(|| clip.asset.asset.to_string());
        let Some(object) = object_of(clip.asset.asset) else {
            failed(
                diagnostics,
                format!(
                    "{}: clip {:?}: {source_shown} is not cooked",
                    entry.path, clip.name
                ),
            );
            ok = false;
            continue;
        };
        let read = crate::rig::cooked::animation(&object, &clip.asset.sub)
            .and_then(|a| crate::rig::cooked::joint_names(&object).map(|joints| (a, joints)));
        let (payload, joints) = match read {
            Ok((Some(a), joints)) => (a, joints),
            Ok((None, _)) => {
                failed(
                    diagnostics,
                    format!(
                        "{}: clip {:?}: {source_shown} has no cooked clip {}",
                        entry.path, clip.name, clip.asset.sub
                    ),
                );
                ok = false;
                continue;
            }
            Err(e) => {
                failed(
                    diagnostics,
                    format!(
                        "{}: clip {:?}: {source_shown}'s cooked object does not read ({e})",
                        entry.path, clip.name
                    ),
                );
                ok = false;
                continue;
            }
        };
        let (mut retargeted, mut diags) =
            crate::rig::retarget(&clip.name, &payload, joints.as_ref(), &names);
        // The rig's own markers join the source's (M3-19c); one past the
        // clip's end is dropped with a warning.
        let duration = retargeted.payload.duration;
        for m in rig.markers_of(&clip.name) {
            if m.at > duration {
                diags.push(
                    Diagnostic::warning(
                        Code::RIG_MALFORMED_ENTRY,
                        format!(
                            "marker {:?} at {} s lies past the end of clip {:?} ({duration} s); dropped",
                            m.name, m.at, clip.name
                        ),
                    )
                    .with_pointer(format!("/markers/{}", clip.name)),
                );
                continue;
            }
            retargeted
                .payload
                .markers
                .push(fynkel_cooked::payload::AnimationMarker {
                    time: m.at,
                    name: m.name.clone(),
                });
        }
        retargeted
            .payload
            .markers
            .sort_by(|a, b| a.time.total_cmp(&b.time));
        for d in &mut diags {
            d.document = Some(shown.clone());
            d.pointer = Some(clip.pointer.clone());
            if let Some(span) = doc.root.pointer_span(&clip.pointer) {
                d.line = Some(span.start.line);
                d.column = Some(span.start.column);
            }
        }
        ok &= !has_errors(&diags);
        diagnostics.extend(diags);
        chunks.push(ChunkPlan {
            kind: KIND_ANIMATION,
            name: clip.name.clone(),
            sub: Some(Rig::clip_sub(&clip.name)),
            info: json!({
                "channels": retargeted.payload.channels.len(),
                "seconds": retargeted.payload.duration,
                "source": format!("{}#{}", source_shown, clip.asset.sub),
                "dropped": retargeted.dropped,
                "markers": retargeted.payload.markers.len(),
            }),
            payload: fynkel_cooked::payload::encode_animation(&retargeted.payload),
        });
    }
    ok.then_some(())
}

/// A scene's `JSON` chunk is the **expanded** scene (M3-03): every prefab
/// instance replaced by its entities with the ids the document layer
/// derives, so a host spawns it without prefabs or the expander. The META
/// records the entity count and the behaviour types the scene authors
/// (`known` ones only, i.e. the project registry's), which the runtime
/// declares when it describes the chunk. Expansion or description errors
/// fail the scene.
fn scene_chunk(
    doc: &Document,
    schemas: &SchemaSet,
    library: &PrefabLibrary,
    name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ChunkPlan> {
    let shown = doc.path.clone().unwrap_or_default();
    let mut scene = Scene::from_document(doc.clone());
    // What the runtime would refuse (`FYN-R0008`) is refused here: an
    // invalid scene is a failed cook, never a cooked object.
    let checked = scene.validate(schemas);
    let bad = has_errors(&checked);
    diagnostics.extend(checked);
    if bad {
        return None;
    }
    let instances = scene.instances().count();
    if instances > 0 {
        let expanded = expand(&scene, library, schemas);
        let bad = expanded.has_errors();
        diagnostics.extend(expanded.diagnostics.iter().cloned());
        if bad {
            return None;
        }
        scene = expanded.scene();
    }
    let described = describe(&scene, schemas);
    let bad = described.has_errors();
    diagnostics.extend(described.diagnostics.iter().cloned());
    if bad {
        return None;
    }
    let mut behaviour_types: Vec<(String, u32)> = described
        .behaviours
        .iter()
        .filter(|b| b.known)
        .map(|b| (b.type_id.clone(), b.version))
        .collect();
    behaviour_types.sort();
    behaviour_types.dedup();
    let text = match scene.document.write() {
        Ok(t) => t,
        Err(d) => {
            diagnostics.push(d.with_document(shown));
            return None;
        }
    };
    Some(ChunkPlan {
        kind: KIND_JSON,
        name: name.to_string(),
        sub: None,
        info: json!({
            "documentKind": "scene",
            "entities": described.description.entities.len(),
            "instances": instances,
            "expanded": true,
            "behaviourTypes": behaviour_types.iter().map(|(t, v)| json!({ "type": t, "version": v })).collect::<Vec<_>>(),
        }),
        payload: text.into_bytes(),
    })
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
/// clips, skins, animations, then the prefab.
fn plan_intermediate(
    im: &Intermediate,
    asset_name: &str,
    limits: &Limits,
    chunks: &mut Vec<ChunkPlan>,
    diagnostics: &mut Vec<Diagnostic>,
    shown: &str,
) {
    for mesh in &im.meshes {
        // Tangents the source did not carry are generated here, once, so
        // the renderer never has to (M3-04; closes the M2 compromise).
        let mut vertices = mesh.vertices.clone();
        let tangents = crate::tangents::ensure(&mut vertices, &mesh.indices);
        chunks.push(ChunkPlan {
            kind: KIND_MESH,
            name: mesh.name.clone(),
            sub: Some(mesh.sub_id.clone()),
            info: json!({
                "vertices": mesh.vertices.len(),
                "indices": mesh.indices.len(),
                "material": mesh.material,
                "tangents": tangents.name(),
                "skinned": mesh.is_skinned(),
            }),
            payload: fynkel_cooked::payload::encode_skinned_mesh(
                &vertices,
                &mesh.indices,
                mesh.bounds_min,
                mesh.bounds_max,
                &mesh.skin,
            ),
        });
    }
    for material in &im.materials {
        let text = serde_json::to_string(&material.document).expect("material serializes");
        chunks.push(ChunkPlan {
            kind: KIND_JSON,
            name: material.name.clone(),
            sub: Some(material.sub_id.clone()),
            info: json!({ "documentKind": "material" }),
            payload: text.clone().into_bytes(),
        });
        match Document::parse(DocumentKind::Material, Some(&material.sub_id), &text) {
            Ok(doc) => {
                if let Some(c) = material_chunk(&doc, &material.name, Some(material.sub_id.clone()))
                {
                    chunks.push(c);
                }
            }
            Err(ds) => {
                for d in ds {
                    diagnostics.push(d.with_document(shown.to_string()));
                }
            }
        }
    }
    for image in &im.images {
        match crate::image::decode(&image.bytes, &image.name, Some(image.color_space), limits) {
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
        let name = if texture.sub_id.is_none() {
            asset_name.to_string()
        } else {
            texture.name.clone()
        };
        let mut c = texture_chunk(texture, texture.sub_id.clone());
        c.name = name.clone();
        chunks.push(c);
        if let Some(hdr) = &texture.hdr {
            // An HDR image is an environment: its prefiltered products
            // (E1) sit next to the texture under the same sub-asset.
            let env = crate::environment::prefilter(texture.width, texture.height, hdr);
            chunks.push(ChunkPlan {
                kind: KIND_ENVIRONMENT,
                name,
                sub: texture.sub_id.clone(),
                info: json!({
                    "specularSize": env.specular_size,
                    "specularMips": env.specular_mips,
                    "irradianceSize": env.irradiance_size,
                    "meanIrradiance": env.mean_irradiance,
                }),
                payload: fynkel_cooked::payload::encode_environment(&env),
            });
        }
    }
    for clip in &im.clips {
        let name = if clip.sub_id.is_none() {
            asset_name.to_string()
        } else {
            clip.name.clone()
        };
        let mut info = json!({
            "sampleRate": clip.sample_rate,
            "channels": clip.channels,
            "frames": clip.frames(),
            "codec": clip.codec,
        });
        match &clip.stream {
            // A streamed sound (M3-11 part 2): the source bytes, decoded
            // at play time; the info says so and how much is in there.
            Some(source) => {
                info["streamed"] = json!(true);
                info["bytes"] = json!(source.len());
                chunks.push(ChunkPlan {
                    kind: KIND_STREAM,
                    name,
                    sub: clip.sub_id.clone(),
                    info,
                    payload: fynkel_cooked::payload::encode_stream(
                        clip.sample_rate,
                        clip.channels as u32,
                        clip.frames(),
                        &clip.codec,
                        source,
                    ),
                });
            }
            None => chunks.push(ChunkPlan {
                kind: KIND_CLIP,
                name,
                sub: clip.sub_id.clone(),
                info,
                payload: encode_clip(clip),
            }),
        }
    }
    for skin in &im.skins {
        chunks.push(ChunkPlan {
            kind: KIND_SKIN,
            name: skin.name.clone(),
            sub: Some(skin.sub_id.clone()),
            info: json!({
                "joints": skin.payload.joints.len(),
                "skeleton": skin.payload.skeleton,
            }),
            payload: fynkel_cooked::payload::encode_skin(&skin.payload),
        });
    }
    for animation in &im.animations {
        chunks.push(ChunkPlan {
            kind: KIND_ANIMATION,
            name: animation.name.clone(),
            sub: Some(animation.sub_id.clone()),
            info: json!({
                "channels": animation.payload.channels.len(),
                "seconds": animation.payload.duration,
            }),
            payload: fynkel_cooked::payload::encode_animation(&animation.payload),
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

/// The `JSON` chunk of a prefab: the document with its own instances
/// expanded (nested prefabs, M3-08b) and every component decodable, so
/// the runtime instantiates it without a validator (`World.instantiate`).
/// Refused like a scene when it would not load.
fn prefab_chunk(
    doc: &Document,
    schemas: &SchemaSet,
    library: &PrefabLibrary,
    name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ChunkPlan> {
    let shown = doc.path.clone().unwrap_or_default();
    let prefab = Prefab::from_document(doc.clone());
    let checked = prefab.validate(schemas);
    let bad = has_errors(&checked);
    diagnostics.extend(checked);
    if bad {
        return None;
    }
    let instances = prefab
        .entities
        .iter()
        .filter(|e| e.prefab.is_some())
        .count();
    let flat = if instances > 0 {
        let expanded = expand(&prefab.scene(), library, schemas);
        let bad = expanded.has_errors();
        diagnostics.extend(expanded.diagnostics.iter().cloned());
        if bad {
            return None;
        }
        Prefab::from_document(Document {
            kind: DocumentKind::Prefab,
            path: doc.path.clone(),
            root: expanded.document.root,
            envelope: doc.envelope.clone(),
        })
    } else {
        prefab
    };
    let described = describe(&flat.scene(), schemas);
    let bad = described.has_errors();
    diagnostics.extend(described.diagnostics.iter().cloned());
    if bad {
        return None;
    }
    let mut behaviour_types: Vec<(String, u32)> = described
        .behaviours
        .iter()
        .filter(|b| b.known)
        .map(|b| (b.type_id.clone(), b.version))
        .collect();
    behaviour_types.sort();
    behaviour_types.dedup();
    let text = match flat.document.write() {
        Ok(t) => t,
        Err(d) => {
            diagnostics.push(d.with_document(shown));
            return None;
        }
    };
    Some(ChunkPlan {
        kind: KIND_JSON,
        name: name.to_string(),
        sub: None,
        info: json!({
            "documentKind": "prefab",
            "entities": flat.entities.len(),
            "root": flat.root.to_string(),
            "instances": instances,
            "expanded": true,
            "behaviourTypes": behaviour_types.iter().map(|(t, v)| json!({ "type": t, "version": v })).collect::<Vec<_>>(),
        }),
        payload: text.into_bytes(),
    })
}

/// The `MATL` chunk of a `fynkel.pbr@1` material document: the typed
/// `PbrMaterial` (`fynkel_wire::material`), what the renderer reads. A
/// custom-shader material has only its `JSON` chunk.
fn material_chunk(doc: &Document, name: &str, sub: Option<String>) -> Option<ChunkPlan> {
    let material = fynkel_document::Material::from_document(doc.clone());
    let pbr = material.pbr.as_ref()?;
    Some(ChunkPlan {
        kind: KIND_MATERIAL,
        name: name.to_string(),
        sub,
        info: json!({
            "shader": material.shader,
            "alphaMode": fynkel_document::material::alpha_mode_name(pbr.alpha_mode),
            "textures": pbr.textures().iter().filter(|(_, _, t)| t.is_some()).map(|(slot, _, _)| *slot).collect::<Vec<_>>(),
        }),
        payload: fynkel_wire::material::encode_material(pbr),
    })
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
            "hdr": t.is_hdr(),
            "mipLevels": fynkel_cooked::payload::full_mip_count(t.width, t.height),
        }),
        payload: encode_texture(t),
    }
}

/// `MESH` (see `fynkel_cooked::payload`), with the skin weights of a
/// skinned mesh as the trailing section.
pub fn encode_mesh(mesh: &MeshData) -> Vec<u8> {
    fynkel_cooked::payload::encode_skinned_mesh(
        &mesh.vertices,
        &mesh.indices,
        mesh.bounds_min,
        mesh.bounds_max,
        &mesh.skin,
    )
}

/// `TEXR` (see `fynkel_cooked::payload`): the image and its full mip
/// chain down to 1×1 (M3-08b), so the renderer never filters at load. An
/// HDR image (E1) is written as linear RGBA16F, its chain averaged in
/// `f32`.
pub fn encode_texture(t: &TextureData) -> Vec<u8> {
    let mip_levels = fynkel_cooked::payload::full_mip_count(t.width, t.height);
    if let Some(hdr) = &t.hdr {
        let chain = mip_chain_f32(t.width, t.height, hdr);
        let mut halves = Vec::with_capacity(chain.len() / 3 * 8);
        for texel in chain.chunks_exact(3) {
            for v in texel {
                halves.extend_from_slice(&fynkel_cooked::payload::f32_to_f16(*v).to_le_bytes());
            }
            halves.extend_from_slice(&fynkel_cooked::payload::f32_to_f16(1.0).to_le_bytes());
        }
        return fynkel_cooked::payload::encode_texture(
            t.width,
            t.height,
            TEXTURE_FORMAT_RGBA16F_LINEAR,
            mip_levels,
            &halves,
        );
    }
    let (format, srgb) = match t.color_space {
        ColorSpace::Srgb => (TEXTURE_FORMAT_RGBA8_SRGB, true),
        ColorSpace::Linear => (TEXTURE_FORMAT_RGBA8_LINEAR, false),
    };
    let chain = mip_chain(t.width, t.height, &t.rgba8, srgb);
    fynkel_cooked::payload::encode_texture(t.width, t.height, format, mip_levels, &chain)
}

/// `mip_chain` for `f32` RGB: every level of `base` concatenated, each a
/// 2×2 box filter of the one above.
pub fn mip_chain_f32(width: u32, height: u32, base: &[f32]) -> Vec<f32> {
    let levels = fynkel_cooked::payload::full_mip_count(width, height);
    let mut out = Vec::with_capacity(base.len() * 4 / 3 + 3);
    out.extend_from_slice(base);
    let (mut w, mut h) = (width as usize, height as usize);
    let mut prev = base.to_vec();
    for _ in 1..levels {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0f32; nw * nh * 3];
        for y in 0..nh {
            for x in 0..nw {
                let (x0, y0) = (2 * x, 2 * y);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                for c in 0..3 {
                    next[(y * nw + x) * 3 + c] = 0.25
                        * (prev[(y0 * w + x0) * 3 + c]
                            + prev[(y0 * w + x1) * 3 + c]
                            + prev[(y1 * w + x0) * 3 + c]
                            + prev[(y1 * w + x1) * 3 + c]);
                }
            }
        }
        out.extend_from_slice(&next);
        prev = next;
        (w, h) = (nw, nh);
    }
    out
}

/// Every level of `base` (`width`×`height` RGBA8) concatenated, each a
/// 2×2 box filter of the one above (an odd trailing row or column is
/// dropped, as GL does). An sRGB image is averaged in linear light and
/// re-encoded, so a mip of a checkerboard is mid-grey, not dark; alpha is
/// linear either way.
pub fn mip_chain(width: u32, height: u32, base: &[u8], srgb: bool) -> Vec<u8> {
    let levels = fynkel_cooked::payload::full_mip_count(width, height);
    let mut out = Vec::with_capacity(base.len() * 4 / 3 + 4);
    out.extend_from_slice(base);
    let (mut w, mut h) = (width as usize, height as usize);
    let mut prev = base.to_vec();
    let to_linear: Vec<f32> = (0..256).map(|v| srgb_to_linear(v as f32 / 255.0)).collect();
    for _ in 1..levels {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                let (x0, y0) = (2 * x, 2 * y);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                let mut sum = [0f32; 4];
                for (sx, sy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                    let at = (sy * w + sx) * 4;
                    for c in 0..3 {
                        sum[c] += if srgb {
                            to_linear[prev[at + c] as usize]
                        } else {
                            prev[at + c] as f32 / 255.0
                        };
                    }
                    sum[3] += prev[at + 3] as f32 / 255.0;
                }
                let at = (y * nw + x) * 4;
                for c in 0..3 {
                    let v = sum[c] * 0.25;
                    let v = if srgb { linear_to_srgb(v) } else { v };
                    next[at + c] = (v * 255.0 + 0.5) as u8;
                }
                next[at + 3] = (sum[3] * 0.25 * 255.0 + 0.5) as u8;
            }
        }
        out.extend_from_slice(&next);
        prev = next;
        (w, h) = (nw, nh);
    }
    out
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// `CLIP` (see `fynkel_cooked::payload`).
pub fn encode_clip(clip: &AudioData) -> Vec<u8> {
    fynkel_cooked::payload::encode_clip(clip.sample_rate, clip.channels as u32, &clip.samples)
}

fn write_index(
    cache: &Cache,
    report: &CookReport,
    project: Option<&CookProject>,
    db: &AssetDatabase,
) -> Result<(), Diagnostic> {
    let index = json!({
        "fynkel": "cache-index",
        "cacheVersion": CACHE_VERSION,
        "containerVersion": fynkel_cooked::VERSION,
        "schemaFingerprint": schema_fingerprint_hex(),
        "profile": report.profile,
        "project": project.map(|p| json!({
            "name": p.name,
            "startupScene": p.startup_scene.map(|u| u.to_string()),
            "quality": p.quality.as_ref().map(|q| q.to_json()),
            "inputActions": p.input_actions.iter().map(|a| json!({
                "name": a.name,
                "type": a.kind,
                "description": a.description,
                "bindings": a.bindings,
            })).collect::<Vec<_>>(),
            "physics": p.physics.to_json(),
            "layers": p.layers_and_tags.layers,
            "tags": p.layers_and_tags.tags,
        })),
        "assets": report.assets.iter().map(|a| json!({
            "id": a.id.to_string(),
            "path": a.path,
            "name": db.get(a.id).map(|e| e.name.as_str()).unwrap_or(""),
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
