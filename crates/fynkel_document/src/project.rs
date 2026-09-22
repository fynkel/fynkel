//! The project model (blueprint §7, §13.5, §19.2c item 13):
//! `fynkel.project.json` (startup scene, input actions, quality profiles,
//! component registry, build targets) and `fynkel.lock.json` (the runtime,
//! tool, schema, document and importer versions the project was last
//! written with), plus project-wide validation — every document, sidecar
//! and reference under the asset root, checked in one pass.
//!
//! The lock is compared, never silently rewritten: a build whose versions
//! differ from the lock reports `FYN-P0011` and leaves the project alone
//! (`fynkel project lock` updates it deliberately).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use fynkel_contracts::{has_errors, AssetRef, Code, Diagnostic};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::asset::{AssetDatabase, AssetKind};
use crate::json::{self, Node};
use crate::material::{Material, ShaderLibrary, ShaderManifest};
use crate::prefab::{expand, Prefab, PrefabLibrary};
use crate::scene::Scene;
use crate::{Document, DocumentKind, SchemaSet};

pub const PROJECT_FILE: &str = "fynkel.project.json";
pub const LOCK_FILE: &str = "fynkel.lock.json";
pub const LOCK_SCHEMA_ID: &str = "https://fynkel.dev/schemas/lock.schema.json";
pub const DEFAULT_ASSETS_DIR: &str = "assets";
pub const TARGETS: [&str; 6] = ["macos", "ios", "android", "windows", "linux", "web"];

#[derive(Clone, Debug, PartialEq)]
pub struct InputAction {
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    pub bindings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QualityProfile {
    pub name: String,
    pub shadows: bool,
    pub shadow_resolution: u32,
    /// Screen-space ambient occlusion (R2).
    pub ambient_occlusion: bool,
    pub msaa: u32,
    pub render_scale: f64,
    pub anisotropy: u32,
    pub texture_max_size: u32,
}

impl QualityProfile {
    /// The engine defaults: what an absent field means in a profile and
    /// what a project without `quality` renders with (blueprint §15;
    /// `docs/renderer.md` §"Quality").
    pub fn engine_default() -> QualityProfile {
        QualityProfile {
            name: "default".to_string(),
            shadows: true,
            shadow_resolution: 2048,
            ambient_occlusion: true,
            msaa: 4,
            render_scale: 1.0,
            anisotropy: 8,
            texture_max_size: 4096,
        }
    }

    /// The profile as the cache index and `fynkel project describe` write
    /// it.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name, "shadows": self.shadows, "shadowResolution": self.shadow_resolution,
            "ambientOcclusion": self.ambient_occlusion, "msaa": self.msaa, "renderScale": self.render_scale, "anisotropy": self.anisotropy,
            "textureMaxSize": self.texture_max_size,
        })
    }

    /// Reads [`to_json`](Self::to_json)'s shape; absent fields take the
    /// engine defaults. `None` without a name.
    pub fn from_json(v: &serde_json::Value) -> Option<QualityProfile> {
        let d = QualityProfile::engine_default();
        let int = |key: &str, fallback: u32| v[key].as_u64().map(|n| n as u32).unwrap_or(fallback);
        Some(QualityProfile {
            name: v["name"].as_str()?.to_string(),
            shadows: v["shadows"].as_bool().unwrap_or(d.shadows),
            shadow_resolution: int("shadowResolution", d.shadow_resolution),
            ambient_occlusion: v["ambientOcclusion"]
                .as_bool()
                .unwrap_or(d.ambient_occlusion),
            msaa: int("msaa", d.msaa),
            render_scale: v["renderScale"].as_f64().unwrap_or(d.render_scale),
            anisotropy: int("anisotropy", d.anisotropy),
            texture_max_size: int("textureMaxSize", d.texture_max_size),
        })
    }
}

impl Default for QualityProfile {
    fn default() -> Self {
        QualityProfile::engine_default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    pub default: String,
    pub profiles: Vec<QualityProfile>,
}

impl Quality {
    pub fn profile(&self, name: &str) -> Option<&QualityProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// The `default` profile (validation guarantees it exists; the first
    /// profile otherwise).
    pub fn default_profile(&self) -> Option<&QualityProfile> {
        self.profile(&self.default).or(self.profiles.first())
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "default": self.default,
            "profiles": self.profiles.iter().map(QualityProfile::to_json).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(v: &serde_json::Value) -> Option<Quality> {
        Some(Quality {
            default: v["default"].as_str()?.to_string(),
            profiles: v["profiles"]
                .as_array()?
                .iter()
                .filter_map(QualityProfile::from_json)
                .collect(),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentEntry {
    pub type_id: String,
    /// `native` or `behaviour`.
    pub kind: String,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub name: String,
    pub enabled: bool,
    /// Cook profile.
    pub profile: String,
}

/// The `physics` block (M3-08): world gravity and the named collision
/// layers (`fynkel.Collider.layer`/`mask` bit `i` is `layers[i]`).
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsSettings {
    /// Metres per second squared.
    pub gravity: [f32; 3],
    /// At most 32 unique names; absent layers are addressed by bit.
    pub layers: Vec<String>,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81, 0.0],
            layers: vec!["default".to_string()],
        }
    }
}

impl PhysicsSettings {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "gravity": self.gravity, "layers": self.layers })
    }

    pub fn from_json(v: &serde_json::Value) -> Self {
        let d = Self::default();
        let gravity = v["gravity"]
            .as_array()
            .filter(|a| a.len() == 3)
            .map(|a| {
                let mut g = d.gravity;
                for (i, x) in a.iter().enumerate() {
                    g[i] = x.as_f64().unwrap_or(d.gravity[i] as f64) as f32;
                }
                g
            })
            .unwrap_or(d.gravity);
        let layers = v["layers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or(d.layers);
        Self { gravity, layers }
    }

    /// The bit mask of the named layers; an unknown name is `None`.
    pub fn mask(&self, names: &[&str]) -> Option<u32> {
        let mut mask = 0;
        for n in names {
            let i = self.layers.iter().position(|l| l == n)?;
            mask |= 1u32 << i;
        }
        Some(mask)
    }
}

/// Typed view of `fynkel.project.json`.
#[derive(Clone, Debug)]
pub struct Project {
    pub document: Document,
    pub startup_scene: Option<AssetRef>,
    /// Relative to the project directory.
    pub assets: String,
    pub input_actions: Vec<InputAction>,
    pub quality: Option<Quality>,
    /// `physics` block or the defaults (M3-08).
    pub physics: PhysicsSettings,
    /// `None` = every built-in component type.
    pub components: Option<Vec<ComponentEntry>>,
    pub targets: Vec<Target>,
    read_diagnostics: Vec<Diagnostic>,
}

impl Project {
    /// `base` plus the project's own schemas (`<dir>/schemas`, where
    /// `fynkel codegen --project` writes `behaviours/<id>.schema.json`),
    /// with every registry entry of kind `behaviour` declared so its type
    /// is known even without a schema.
    #[allow(clippy::result_large_err)]
    pub fn schemas(&self, base: &SchemaSet, dir: &Path) -> Result<SchemaSet, Diagnostic> {
        let mut schemas = base.extended_with(&dir.join(crate::runtime::PROJECT_SCHEMAS_DIR))?;
        for entry in self.components.iter().flatten() {
            if entry.kind == "behaviour" {
                schemas.declare_behaviour(&entry.type_id, entry.version);
            }
        }
        Ok(schemas)
    }
}

impl Project {
    pub fn from_document(document: Document) -> Project {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Project {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected fynkel.project.json, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let malformed = |pointer: &str, what: &str, diags: &mut Vec<Diagnostic>| {
            diags.push(anchor(
                root,
                Diagnostic::error(Code::PROJECT_MALFORMED_ENTRY, what.to_string()),
                pointer,
            ));
        };

        let startup_scene = match root.get("startupScene") {
            Some(n) => match n.as_object().and_then(as_asset_ref) {
                Some(r) => Some(r),
                None => {
                    malformed(
                        "/startupScene",
                        "startupScene must be { \"asset\": \"<uuid>\" }",
                        &mut diags,
                    );
                    None
                }
            },
            None => None,
        };
        let assets = match root.get("assets") {
            None => DEFAULT_ASSETS_DIR.to_string(),
            Some(n) => match n.as_str() {
                Some(s) => s.to_string(),
                None => {
                    malformed("/assets", "assets must be a directory name", &mut diags);
                    DEFAULT_ASSETS_DIR.to_string()
                }
            },
        };

        let mut input_actions = Vec::new();
        if let Some(members) = root.get("inputActions").and_then(Node::as_object) {
            for m in members {
                let pointer = format!("/inputActions/{}", crate::scene::escape(&m.key));
                let Some(kind) = m.value.get("type").and_then(Node::as_str) else {
                    malformed(&pointer, "an input action needs a type", &mut diags);
                    continue;
                };
                let bindings = m
                    .value
                    .get("bindings")
                    .and_then(Node::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|b| b.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                input_actions.push(InputAction {
                    name: m.key.clone(),
                    kind: kind.to_string(),
                    description: m
                        .value
                        .get("description")
                        .and_then(Node::as_str)
                        .map(str::to_string),
                    bindings,
                });
            }
        }

        let mut physics = PhysicsSettings::default();
        if let Some(p) = root.get("physics").and_then(Node::as_object) {
            let get = |key: &str| p.iter().find(|m| m.key == key).map(|m| &m.value);
            if let Some(g) = get("gravity") {
                match g.as_array() {
                    Some(items) if items.len() == 3 => {
                        for (i, x) in items.iter().enumerate() {
                            match x.as_f64() {
                                Some(v) => physics.gravity[i] = v as f32,
                                None => malformed(
                                    "/physics/gravity",
                                    "gravity must be three numbers",
                                    &mut diags,
                                ),
                            }
                        }
                    }
                    _ => malformed(
                        "/physics/gravity",
                        "gravity must be three numbers",
                        &mut diags,
                    ),
                }
            }
            if let Some(l) = get("layers") {
                match l.as_array() {
                    Some(items) => {
                        physics.layers = items
                            .iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect();
                        if physics.layers.len() != items.len() {
                            malformed("/physics/layers", "layers must be strings", &mut diags);
                        }
                    }
                    None => malformed("/physics/layers", "layers must be an array", &mut diags),
                }
            }
        }

        let quality = root.get("quality").and_then(|q| {
            let default = q.get("default").and_then(Node::as_str)?.to_string();
            let profiles = q
                .get("profiles")
                .and_then(Node::as_object)?
                .iter()
                .map(|m| {
                    let int = |key: &str, fallback: u32| {
                        m.value
                            .get(key)
                            .and_then(Node::as_i64)
                            .map(|v| v as u32)
                            .unwrap_or(fallback)
                    };
                    let d = QualityProfile::engine_default();
                    QualityProfile {
                        name: m.key.clone(),
                        shadows: m
                            .value
                            .get("shadows")
                            .and_then(Node::as_bool)
                            .unwrap_or(d.shadows),
                        shadow_resolution: int("shadowResolution", d.shadow_resolution),
                        ambient_occlusion: m
                            .value
                            .get("ambientOcclusion")
                            .and_then(Node::as_bool)
                            .unwrap_or(d.ambient_occlusion),
                        msaa: int("msaa", d.msaa),
                        render_scale: m
                            .value
                            .get("renderScale")
                            .and_then(Node::as_f64)
                            .unwrap_or(d.render_scale),
                        anisotropy: int("anisotropy", d.anisotropy),
                        texture_max_size: int("textureMaxSize", d.texture_max_size),
                    }
                })
                .collect();
            Some(Quality { default, profiles })
        });
        if root.get("quality").is_some() && quality.is_none() {
            malformed(
                "/quality",
                "quality needs a default and at least one profile",
                &mut diags,
            );
        }

        let components = root
            .get("components")
            .and_then(Node::as_object)
            .map(|members| {
                members
                    .iter()
                    .map(|m| ComponentEntry {
                        type_id: m.key.clone(),
                        kind: m
                            .value
                            .get("kind")
                            .and_then(Node::as_str)
                            .unwrap_or("native")
                            .to_string(),
                        version: m.value.get("version").and_then(Node::as_i64).unwrap_or(1) as u32,
                    })
                    .collect()
            });

        let mut targets = Vec::new();
        if let Some(members) = root.get("targets").and_then(Node::as_object) {
            for m in members {
                targets.push(Target {
                    name: m.key.clone(),
                    enabled: m
                        .value
                        .get("enabled")
                        .and_then(Node::as_bool)
                        .unwrap_or(true),
                    profile: m
                        .value
                        .get("profile")
                        .and_then(Node::as_str)
                        .unwrap_or("dev")
                        .to_string(),
                });
            }
        }

        for d in &mut diags {
            if d.document.is_none() {
                d.document = document.path.clone();
            }
        }
        Project {
            document,
            startup_scene,
            assets,
            input_actions,
            quality,
            physics,
            components,
            targets,
            read_diagnostics: diags,
        }
    }

    /// The project a file belongs to: the nearest ancestor directory of
    /// `path` holding `fynkel.project.json`, or `None` outside any project.
    pub fn dir_of(path: &Path) -> Option<PathBuf> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(path)
        };
        let mut dir = abs.parent()?.to_path_buf();
        loop {
            if dir.join(PROJECT_FILE).is_file() {
                return Some(dir);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    /// Reads `<dir>/fynkel.project.json`.
    pub fn read(dir: &Path) -> Result<Project, Vec<Diagnostic>> {
        let path = dir.join(PROJECT_FILE);
        if !path.is_file() {
            return Err(vec![Diagnostic::error(
                Code::PROJECT_MISSING,
                format!("no {PROJECT_FILE} in {}", dir.display()),
            )
            .with_document(path.display().to_string())
            .with_suggestion(
                "run from the project directory, or pass --project <dir>",
            )]);
        }
        Document::read(&path, Some(DocumentKind::Project)).map(Project::from_document)
    }

    pub fn id(&self) -> Uuid {
        self.document.envelope.id
    }

    pub fn name(&self) -> &str {
        &self.document.envelope.name
    }

    pub fn path(&self) -> Option<&str> {
        self.document.path.as_deref()
    }

    /// The asset root for a project at `dir`.
    pub fn assets_dir(&self, dir: &Path) -> PathBuf {
        dir.join(&self.assets)
    }

    pub fn read_diagnostics(&self) -> &[Diagnostic] {
        &self.read_diagnostics
    }

    pub fn enabled_targets(&self) -> impl Iterator<Item = &Target> {
        self.targets.iter().filter(|t| t.enabled)
    }

    /// Envelope + JSON Schema, read problems, and the rules that need no
    /// asset database (quality default, targets, asset root shape).
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for mut d in self
            .read_diagnostics
            .iter()
            .cloned()
            .chain(self.validate_semantics())
        {
            if d.code == Code::PROJECT_MALFORMED_ENTRY
                && d.pointer
                    .as_ref()
                    .is_some_and(|p| schema_pointers.contains(p))
            {
                continue;
            }
            if d.document.is_none() {
                d.document = self.document.path.clone();
            }
            out.push(d);
        }
        out
    }

    /// The database-free rules only.
    pub fn validate_semantics(&self) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = Vec::new();
        if let Some(q) = &self.quality {
            if !q.profiles.iter().any(|p| p.name == q.default) {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::PROJECT_QUALITY_DEFAULT,
                        format!(
                            "quality.default {:?} is not one of the profiles ({})",
                            q.default,
                            q.profiles
                                .iter()
                                .map(|p| p.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ),
                    "/quality/default",
                ));
            }
        }
        if !self.targets.is_empty() && self.enabled_targets().next().is_none() {
            out.push(anchor(
                root,
                Diagnostic::warning(Code::PROJECT_NO_TARGET, "every build target is disabled")
                    .with_suggestion("enable one, or drop `targets` to build for the host only"),
                "/targets",
            ));
        }
        if self.assets.split('/').any(|c| c == "..") || self.assets.starts_with('/') {
            out.push(anchor(
                root,
                Diagnostic::error(
                    Code::PROJECT_ASSET_ROOT,
                    format!("assets {:?} must stay inside the project", self.assets),
                ),
                "/assets",
            ));
        }
        // Bindings are compiled by the same grammar the runtime binds with
        // (`fynkel_core::input`), so a project that validates runs with
        // every binding in force.
        for a in &self.input_actions {
            let Some(kind) = fynkel_core::input::ActionKind::parse(&a.kind) else {
                continue; // the schema reports the type
            };
            for (i, b) in a.bindings.iter().enumerate() {
                if let Err(e) = fynkel_core::input::parse_binding(kind, b) {
                    out.push(anchor(
                        root,
                        Diagnostic::error(Code::PROJECT_INPUT_BINDING, e.to_string()),
                        &format!(
                            "/inputActions/{}/bindings/{i}",
                            crate::scene::escape(&a.name)
                        ),
                    ));
                }
            }
        }
        // Physics (M3-08): a layer is one bit of a 32-bit mask.
        if root.get("physics").is_some() {
            if self.physics.layers.len() > 32 {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::PROJECT_PHYSICS,
                        format!(
                            "physics.layers has {} names; a collision mask has 32 bits",
                            self.physics.layers.len()
                        ),
                    ),
                    "/physics/layers",
                ));
            }
            for (i, name) in self.physics.layers.iter().enumerate() {
                if name.is_empty() {
                    out.push(anchor(
                        root,
                        Diagnostic::error(Code::PROJECT_PHYSICS, "a layer name is empty"),
                        &format!("/physics/layers/{i}"),
                    ));
                } else if self.physics.layers[..i].contains(name) {
                    out.push(anchor(
                        root,
                        Diagnostic::error(
                            Code::PROJECT_PHYSICS,
                            format!("layer {name:?} is listed twice"),
                        ),
                        &format!("/physics/layers/{i}"),
                    ));
                }
            }
            if self.physics.gravity.iter().any(|g| !g.is_finite()) {
                out.push(anchor(
                    root,
                    Diagnostic::error(Code::PROJECT_PHYSICS, "gravity must be finite"),
                    "/physics/gravity",
                ));
            }
        }
        out
    }

    /// The rules that need the asset database and the component schemas:
    /// the startup scene resolves to a scene, native registry entries
    /// exist.
    pub fn validate_against(&self, db: &AssetDatabase, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = Vec::new();
        match &self.startup_scene {
            None => {}
            Some(r) if r.is_nil() => out.push(anchor(
                root,
                Diagnostic::error(Code::PROJECT_STARTUP_SCENE, "startupScene is unset"),
                "/startupScene/asset",
            )),
            Some(r) => match db.resolve(r) {
                Ok(resolved) => {
                    if resolved.entry.kind != AssetKind::Document(DocumentKind::Scene) {
                        out.push(anchor(
                            root,
                            Diagnostic::error(
                                Code::PROJECT_STARTUP_SCENE,
                                format!(
                                    "startupScene is {} ({}), not a scene",
                                    resolved.entry.path,
                                    resolved.entry.kind.name()
                                ),
                            )
                            .with_id(r.to_string()),
                            "/startupScene/asset",
                        ));
                    }
                }
                Err(d) => out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::PROJECT_STARTUP_SCENE,
                        format!("startupScene: {}", d.message),
                    )
                    .with_id(r.to_string()),
                    "/startupScene/asset",
                )),
            },
        }
        if let Some(entries) = &self.components {
            for e in entries {
                if e.kind == "native" && schemas.component(&e.type_id).is_none() {
                    out.push(anchor(
                        root,
                        Diagnostic::error(
                            Code::PROJECT_UNKNOWN_COMPONENT,
                            format!(
                                "no native component type {:?} in the loaded schemas",
                                e.type_id
                            ),
                        )
                        .with_suggestion(format!(
                            "known: {}",
                            schemas
                                .component_types()
                                .iter()
                                .map(|c| c.type_id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                        &format!("/components/{}", crate::scene::escape(&e.type_id)),
                    ));
                }
            }
        }
        for d in &mut out {
            if d.document.is_none() {
                d.document = self.document.path.clone();
            }
        }
        out
    }

    /// True when `type_id` may appear in this project's documents.
    pub fn allows_component(&self, type_id: &str) -> bool {
        match &self.components {
            None => true,
            Some(entries) => entries.iter().any(|e| e.type_id == type_id),
        }
    }
}

fn as_asset_ref(members: &[json::Member]) -> Option<AssetRef> {
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

/// `fynkel.lock.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lock {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub format_version: u32,
    pub runtime: LockRuntime,
    pub tools: LockTools,
    pub schema_fingerprint: String,
    pub documents: BTreeMap<String, u32>,
    pub importers: BTreeMap<String, u32>,
    pub cache: LockCache,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockRuntime {
    pub version: String,
    pub abi_version: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LockTools {
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LockCache {
    pub version: u32,
    pub container: u32,
}

impl Lock {
    /// The lock this build would write: workspace version, ABI version,
    /// schema fingerprint and document versions from the linked crates;
    /// importer and cache versions from the caller (they live in
    /// `fynkel_import`, which depends on this crate).
    pub fn expected(importers: &[(&str, u32)], cache_version: u32, container_version: u32) -> Lock {
        Lock {
            schema: LOCK_SCHEMA_ID.to_string(),
            format_version: 1,
            runtime: LockRuntime {
                version: env!("CARGO_PKG_VERSION").to_string(),
                abi_version: fynkel_contracts::ABI_VERSION as u32,
            },
            tools: LockTools {
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            schema_fingerprint: format!("{:#018x}", fynkel_core::schema::schema_fingerprint()),
            documents: DocumentKind::ALL
                .iter()
                .map(|k| (k.name().to_string(), k.supported_format_version()))
                .collect(),
            importers: importers.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            cache: LockCache {
                version: cache_version,
                container: container_version,
            },
        }
    }

    /// Parses lock text: strict JSON, the lock schema, then the typed view.
    pub fn parse(
        path: Option<&str>,
        text: &str,
        schemas: &SchemaSet,
    ) -> Result<Lock, Vec<Diagnostic>> {
        let with_doc = |d: Diagnostic| match path {
            Some(p) if d.document.is_none() => d.with_document(p),
            _ => d,
        };
        let root = json::parse(text).map_err(|d| vec![with_doc(d)])?;
        let mut diags = schemas
            .validate_schema(LOCK_SCHEMA_ID, &root)
            .unwrap_or_default()
            .into_iter()
            .map(|d| {
                let out = Diagnostic::error(Code::LOCK_MALFORMED, d.message);
                with_doc(match d.pointer {
                    Some(p) => out.with_pointer(p),
                    None => out,
                })
            })
            .collect::<Vec<_>>();
        if has_errors(&diags) {
            return Err(diags);
        }
        match serde_json::from_value::<Lock>(root.to_serde()) {
            Ok(lock) => Ok(lock),
            Err(e) => {
                diags.push(with_doc(Diagnostic::error(
                    Code::LOCK_MALFORMED,
                    format!("{e}"),
                )));
                Err(diags)
            }
        }
    }

    /// Reads `<dir>/fynkel.lock.json`.
    pub fn read(dir: &Path, schemas: &SchemaSet) -> Result<Lock, Vec<Diagnostic>> {
        let path = dir.join(LOCK_FILE);
        let shown = path.display().to_string();
        if !path.is_file() {
            return Err(vec![Diagnostic::error(
                Code::LOCK_MISSING,
                format!("no {LOCK_FILE} next to {PROJECT_FILE}"),
            )
            .with_document(shown)
            .with_suggestion(
                "run `fynkel project lock` to write one for this build",
            )]);
        }
        let text = std::fs::read_to_string(&path).map_err(|e| {
            vec![Diagnostic::error(Code::IO, format!("cannot read: {e}"))
                .with_document(shown.clone())]
        })?;
        Lock::parse(Some(&shown), &text, schemas)
    }

    /// Canonical text (sorted maps, two-space indent, trailing newline).
    pub fn to_text(&self) -> String {
        let value = serde_json::to_value(self).expect("lock serializes");
        json::write(&Node::from_serde(&value))
    }

    /// Every difference between this lock and `expected`, as `FYN-P0011`
    /// errors anchored at the differing field. Empty when they match.
    pub fn check(&self, expected: &Lock, path: Option<&str>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let mut differ = |pointer: &str, have: String, want: String| {
            let d = Diagnostic::error(
                Code::LOCK_MISMATCH,
                format!("{pointer}: the lock says {have}, this build is {want}"),
            )
            .with_pointer(pointer)
            .with_suggestion(
                "`fynkel project lock` rewrites the lock for this build once you mean to upgrade",
            );
            out.push(match path {
                Some(p) => d.with_document(p),
                None => d,
            });
        };
        if self.runtime.version != expected.runtime.version {
            differ(
                "/runtime/version",
                self.runtime.version.clone(),
                expected.runtime.version.clone(),
            );
        }
        if self.runtime.abi_version != expected.runtime.abi_version {
            differ(
                "/runtime/abiVersion",
                self.runtime.abi_version.to_string(),
                expected.runtime.abi_version.to_string(),
            );
        }
        if self.tools.version != expected.tools.version {
            differ(
                "/tools/version",
                self.tools.version.clone(),
                expected.tools.version.clone(),
            );
        }
        if self.schema_fingerprint != expected.schema_fingerprint {
            differ(
                "/schemaFingerprint",
                self.schema_fingerprint.clone(),
                expected.schema_fingerprint.clone(),
            );
        }
        for (name, want) in &expected.documents {
            match self.documents.get(name) {
                Some(have) if have == want => {}
                Some(have) => differ(
                    &format!("/documents/{name}"),
                    have.to_string(),
                    want.to_string(),
                ),
                None => differ(
                    &format!("/documents/{name}"),
                    "missing".to_string(),
                    want.to_string(),
                ),
            }
        }
        for (name, want) in &expected.importers {
            match self.importers.get(name) {
                Some(have) if have == want => {}
                Some(have) => differ(
                    &format!("/importers/{}", crate::scene::escape(name)),
                    have.to_string(),
                    want.to_string(),
                ),
                None => differ(
                    &format!("/importers/{}", crate::scene::escape(name)),
                    "missing".to_string(),
                    want.to_string(),
                ),
            }
        }
        if self.cache.version != expected.cache.version {
            differ(
                "/cache/version",
                self.cache.version.to_string(),
                expected.cache.version.to_string(),
            );
        }
        if self.cache.container != expected.cache.container {
            differ(
                "/cache/container",
                self.cache.container.to_string(),
                expected.cache.container.to_string(),
            );
        }
        out
    }
}

/// What project-wide validation found.
#[derive(Debug)]
pub struct ProjectReport {
    pub dir: PathBuf,
    pub project: Option<Project>,
    pub lock: Option<Lock>,
    /// Documents checked (scenes, prefabs, materials, shaders).
    pub documents: usize,
    /// Source assets checked (sidecars).
    pub sources: usize,
    /// References checked.
    pub references: usize,
    pub diagnostics: Vec<Diagnostic>,
}

impl ProjectReport {
    pub fn has_errors(&self) -> bool {
        has_errors(&self.diagnostics)
    }

    pub fn json(&self) -> serde_json::Value {
        let mut v = crate::diagnostics_json(&self.diagnostics);
        v["project"] = self
            .project
            .as_ref()
            .map(|p| {
                serde_json::json!({
                    "id": p.id().to_string(),
                    "name": p.name(),
                    "path": p.path(),
                })
            })
            .unwrap_or(serde_json::Value::Null);
        v["lock"] = self
            .lock
            .as_ref()
            .map(|l| serde_json::to_value(l).expect("lock serializes"))
            .unwrap_or(serde_json::Value::Null);
        v["documents"] = self.documents.into();
        v["sources"] = self.sources.into();
        v["references"] = self.references.into();
        v["errors"] = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == fynkel_contracts::Severity::Error)
            .count()
            .into();
        v["warnings"] = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == fynkel_contracts::Severity::Warning)
            .count()
            .into();
        v
    }
}

/// Validates a whole project at `dir`: the project file, the lock
/// (against `expected` when given), every document under the asset root
/// (schema + semantics, scenes expanded with the project's prefabs,
/// materials against the built-in shaders plus the project's manifests),
/// every sidecar (through the asset database scan) and every reference.
pub fn validate_project(
    dir: &Path,
    schemas: &SchemaSet,
    expected_lock: Option<&Lock>,
) -> ProjectReport {
    validate_project_with(dir, schemas, expected_lock, &NoSourcePrefabs)
}

/// Where `validate_project` gets an imported source's hierarchy when a
/// scene or prefab instantiates it (`prefab: { asset: <source>, sub:
/// "prefab" }`, M3-19a): this crate reads documents, the importer lives
/// above it, so the CLI and the cook pass `fynkel_import`'s provider and
/// a bare `validate_project` reports such instances as `D0401`.
pub trait SourcePrefabs {
    /// The prefab document the importer derives for `entry` (a source
    /// whose sidecar lists `sub`), `Ok(None)` when the source has no
    /// hierarchy, `Err` with the importer's diagnostics when it does not
    /// import. Called once per (source, sub) a document references.
    fn prefab(
        &self,
        root: &Path,
        entry: &crate::asset::AssetEntry,
        sub: &str,
    ) -> Result<Option<Document>, Vec<Diagnostic>>;
}

/// No importer: every imported-prefab instance is `D0401`.
pub struct NoSourcePrefabs;

impl SourcePrefabs for NoSourcePrefabs {
    fn prefab(
        &self,
        _root: &Path,
        _entry: &crate::asset::AssetEntry,
        _sub: &str,
    ) -> Result<Option<Document>, Vec<Diagnostic>> {
        Ok(None)
    }
}

/// [`validate_project`] with a [`SourcePrefabs`] provider.
pub fn validate_project_with(
    dir: &Path,
    schemas: &SchemaSet,
    expected_lock: Option<&Lock>,
    sources: &dyn SourcePrefabs,
) -> ProjectReport {
    let mut report = ProjectReport {
        dir: dir.to_path_buf(),
        project: None,
        lock: None,
        documents: 0,
        sources: 0,
        references: 0,
        diagnostics: Vec::new(),
    };
    let project = match Project::read(dir) {
        Ok(p) => p,
        Err(diags) => {
            report.diagnostics = diags;
            return report;
        }
    };
    report.diagnostics.extend(project.validate(schemas));
    // From here on the project's behaviour types are known.
    let schemas = match project.schemas(schemas, dir) {
        Ok(s) => s,
        Err(d) => {
            report.diagnostics.push(d);
            report.project = Some(project);
            return report;
        }
    };
    let schemas = &schemas;

    match Lock::read(dir, schemas) {
        Ok(lock) => {
            if let Some(expected) = expected_lock {
                let path = dir.join(LOCK_FILE).display().to_string();
                report.diagnostics.extend(lock.check(expected, Some(&path)));
            }
            report.lock = Some(lock);
        }
        Err(diags) => report.diagnostics.extend(diags),
    }

    let assets = project.assets_dir(dir);
    if !assets.is_dir() {
        report.diagnostics.push(
            Diagnostic::error(
                Code::PROJECT_ASSET_ROOT,
                format!("asset root {} is not a directory", assets.display()),
            )
            .with_document(dir.join(PROJECT_FILE).display().to_string())
            .with_pointer("/assets"),
        );
        report.project = Some(project);
        return report;
    }
    let (db, scan) = AssetDatabase::scan(&assets, schemas);
    report.diagnostics.extend(scan);
    report
        .diagnostics
        .extend(project.validate_against(&db, schemas));

    // Libraries the per-kind checks need: every prefab, every shader manifest.
    let mut prefabs = PrefabLibrary::empty();
    let mut shaders = ShaderLibrary::builtin();
    let mut docs: Vec<(DocumentKind, Document)> = Vec::new();
    for entry in db.entries() {
        match &entry.kind {
            AssetKind::Document(kind) => {
                match Document::read(&entry.file(db.root()), Some(*kind)) {
                    Ok(doc) => docs.push((*kind, doc)),
                    Err(diags) => report.diagnostics.extend(diags),
                }
            }
            AssetKind::Source { .. } => report.sources += 1,
        }
    }
    // Imported hierarchies the scenes and prefabs instantiate (M3-19a):
    // derived once per (source, sub) through the provider; a source that
    // does not import is reported here, once, and the instance is D0401.
    let mut wanted: Vec<(Uuid, String)> = Vec::new();
    for (kind, doc) in &docs {
        if !matches!(kind, DocumentKind::Scene | DocumentKind::Prefab) {
            continue;
        }
        for key in PrefabLibrary::wanted_sources(&Scene::from_document(doc.clone())) {
            if !wanted.contains(&key) {
                wanted.push(key);
            }
        }
    }
    report
        .diagnostics
        .extend(prefabs.add_sources(&db, &wanted, sources));
    for (kind, doc) in &docs {
        match kind {
            DocumentKind::Prefab => prefabs.add(Prefab::from_document(doc.clone())),
            DocumentKind::Shader => {
                let manifest = ShaderManifest::from_document(doc.clone());
                if !has_errors(&manifest.validate(schemas)) {
                    shaders.add(manifest);
                }
            }
            _ => {}
        }
    }
    // Nested prefabs expand once here (a cycle or a bad inner override is
    // reported whether or not a scene instantiates the prefab); the
    // scenes below expand against the flat library.
    let (prefabs, flat) = prefabs.flattened(schemas);
    report.diagnostics.extend(flat);

    let mut unregistered: HashSet<String> = HashSet::new();
    for (kind, doc) in docs {
        report.documents += 1;
        let refs = crate::asset::asset_refs(&doc.root);
        report.references += refs.iter().filter(|(_, r)| !r.is_nil()).count();
        report.diagnostics.extend(db.check_references(&doc));
        let path = doc.path.clone();
        match kind {
            DocumentKind::Scene => {
                let scene = Scene::from_document(doc);
                report.diagnostics.extend(scene.validate(schemas));
                if scene.instances().next().is_some() {
                    let expanded = expand(&scene, &prefabs, schemas);
                    report
                        .diagnostics
                        .extend(expanded.diagnostics.iter().cloned());
                }
                registry_check(
                    &project,
                    &scene.entities,
                    &path,
                    &scene.document.root,
                    &mut unregistered,
                    &mut report.diagnostics,
                );
            }
            DocumentKind::Prefab => {
                let prefab = Prefab::from_document(doc);
                report.diagnostics.extend(prefab.validate(schemas));
                registry_check(
                    &project,
                    &prefab.entities,
                    &path,
                    &prefab.document.root,
                    &mut unregistered,
                    &mut report.diagnostics,
                );
            }
            DocumentKind::Material => {
                report
                    .diagnostics
                    .extend(Material::from_document(doc).validate(schemas, &shaders));
            }
            DocumentKind::Shader => {
                report
                    .diagnostics
                    .extend(ShaderManifest::from_document(doc).validate(schemas));
            }
            DocumentKind::Rig => {
                let rig = crate::rig::Rig::from_document(doc);
                report.diagnostics.extend(rig.validate(schemas));
                report.diagnostics.extend(rig.validate_references(&db));
            }
            DocumentKind::Blend => {
                let blend = crate::blend::Blend::from_document(doc);
                report.diagnostics.extend(blend.validate(schemas));
                report.diagnostics.extend(blend.validate_references(&db));
            }
            DocumentKind::Project | DocumentKind::Meta => {
                report.diagnostics.extend(doc.validate(schemas));
            }
        }
    }
    report.project = Some(project);
    report
}

/// One `FYN-P0004` warning per (document, type) for component types the
/// registry does not list.
fn registry_check(
    project: &Project,
    entities: &[crate::scene::Entity],
    path: &Option<String>,
    root: &Node,
    seen: &mut HashSet<String>,
    out: &mut Vec<Diagnostic>,
) {
    if project.components.is_none() {
        return;
    }
    for e in entities {
        for c in &e.components {
            if project.allows_component(&c.type_id) {
                continue;
            }
            let key = format!("{}#{}", path.as_deref().unwrap_or(""), c.type_id);
            if !seen.insert(key) {
                continue;
            }
            let mut d = anchor(
                root,
                Diagnostic::warning(
                    Code::PROJECT_UNREGISTERED_COMPONENT,
                    format!(
                        "component type {:?} is not in the project's component registry",
                        c.type_id
                    ),
                )
                .with_id(c.id.to_string())
                .with_suggestion(format!(
                    "add it under `components` in {PROJECT_FILE}, or drop the registry to allow every built-in type"
                )),
                &format!("{}/type", c.pointer),
            );
            d.document = path.clone();
            out.push(d);
        }
    }
}

/// `fynkel project describe --json`.
pub fn describe_json(
    project: &Project,
    db: Option<&AssetDatabase>,
    lock: Option<&Lock>,
) -> serde_json::Value {
    let startup = project.startup_scene.as_ref().map(|r| {
        let mut v = serde_json::json!({ "asset": r.to_string() });
        if let Some(db) = db {
            if let Ok(resolved) = db.resolve(r) {
                v["path"] = resolved.entry.path.clone().into();
                v["name"] = resolved.entry.name.clone().into();
            }
        }
        v
    });
    serde_json::json!({
        "id": project.id().to_string(),
        "name": project.name(),
        "path": project.path(),
        "formatVersion": project.document.envelope.format_version,
        "assets": project.assets,
        "startupScene": startup,
        "inputActions": project.input_actions.iter().map(|a| serde_json::json!({
            "name": a.name, "type": a.kind, "description": a.description, "bindings": a.bindings,
        })).collect::<Vec<_>>(),
        "quality": project.quality.as_ref().map(Quality::to_json),
        "physics": project.physics.to_json(),
        "components": project.components.as_ref().map(|list| list.iter().map(|c| serde_json::json!({
            "type": c.type_id, "kind": c.kind, "version": c.version,
        })).collect::<Vec<_>>()),
        "targets": project.targets.iter().map(|t| serde_json::json!({
            "name": t.name, "enabled": t.enabled, "profile": t.profile,
        })).collect::<Vec<_>>(),
        "assetCount": db.map(|db| db.len()),
        "lock": lock.map(|l| serde_json::to_value(l).expect("lock serializes")),
    })
}

/// `fynkel project describe` text.
pub fn describe_text(project: &Project, db: Option<&AssetDatabase>, lock: Option<&Lock>) -> String {
    let mut out = String::new();
    out.push_str(&format!("project {} ({})\n", project.name(), project.id()));
    out.push_str(&format!(
        "  assets: {}{}\n",
        project.assets,
        db.map(|db| format!(" ({} asset(s))", db.len()))
            .unwrap_or_default()
    ));
    match &project.startup_scene {
        Some(r) => {
            let resolved = db.and_then(|db| db.resolve(r).ok());
            out.push_str(&format!(
                "  startup scene: {}{}\n",
                r,
                resolved
                    .map(|e| format!(" → {}", e.entry.path))
                    .unwrap_or_else(|| " (unresolved)".to_string())
            ));
        }
        None => out.push_str("  startup scene: none\n"),
    }
    out.push_str(&format!(
        "  input actions: {}\n",
        if project.input_actions.is_empty() {
            "none".to_string()
        } else {
            project
                .input_actions
                .iter()
                .map(|a| format!("{} ({}, {} binding(s))", a.name, a.kind, a.bindings.len()))
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    match &project.quality {
        Some(q) => out.push_str(&format!(
            "  quality: default {} of [{}]\n",
            q.default,
            q.profiles
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        None => out.push_str("  quality: engine defaults\n"),
    }
    out.push_str(&format!(
        "  physics: gravity [{}, {}, {}], layers [{}]\n",
        project.physics.gravity[0],
        project.physics.gravity[1],
        project.physics.gravity[2],
        project.physics.layers.join(", ")
    ));
    match &project.components {
        Some(list) => out.push_str(&format!(
            "  components: {} registered ({} native, {} behaviour)\n",
            list.len(),
            list.iter().filter(|c| c.kind == "native").count(),
            list.iter().filter(|c| c.kind == "behaviour").count()
        )),
        None => out.push_str("  components: every built-in type\n"),
    }
    out.push_str(&format!(
        "  targets: {}\n",
        if project.targets.is_empty() {
            "host only".to_string()
        } else {
            project
                .targets
                .iter()
                .map(|t| {
                    format!(
                        "{}{} (profile {})",
                        t.name,
                        if t.enabled { "" } else { " [disabled]" },
                        t.profile
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    match lock {
        Some(l) => out.push_str(&format!(
            "  lock: runtime {} abi {} schema {} tools {}\n",
            l.runtime.version, l.runtime.abi_version, l.schema_fingerprint, l.tools.version
        )),
        None => out.push_str("  lock: none\n"),
    }
    out
}

fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    match root.pointer_span(pointer) {
        Some(s) => d.at(s.start.line, s.start.column),
        None => d,
    }
}
