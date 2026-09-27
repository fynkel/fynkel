//! Document migrations (blueprint §19.2c item 9, §6): a document whose
//! `formatVersion` is behind the current one is brought up to date by an
//! ordered chain of per-kind steps, each `from → from + 1`. A migration is
//! a pure function on the JSON tree; the framework sets `formatVersion`,
//! validates the result, produces a unified diff for `--dry-run`, and
//! writes with a `.bak` copy of the original. Newer documents are never
//! touched (`FYN-D0013`).

use std::path::{Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic};

use crate::json::{Member, Node, Value};
use crate::{Document, DocumentKind, SchemaSet};

/// One step: `kind` documents at `from` become `from + 1`.
#[derive(Clone)]
pub struct Migration {
    pub kind: DocumentKind,
    pub from: u32,
    /// Short stable name, reported in `applied[]` (`scene/0-1/entities-keyed-by-id`).
    pub name: &'static str,
    pub description: &'static str,
    /// Rewrites the body in place. `formatVersion` is set by the framework.
    pub apply: fn(&mut Node) -> Result<(), String>,
}

impl Migration {
    pub fn to(&self) -> u32 {
        self.from + 1
    }

    pub fn id(&self) -> String {
        format!(
            "{}/{}-{}/{}",
            self.kind.name(),
            self.from,
            self.to(),
            self.name
        )
    }
}

/// The registry: every known step, looked up by kind and version.
#[derive(Clone, Default)]
pub struct Migrations {
    steps: Vec<Migration>,
}

impl Migrations {
    pub fn empty() -> Migrations {
        Migrations::default()
    }

    /// The steps shipped with this build.
    pub fn builtin() -> Migrations {
        let mut m = Migrations::empty();
        m.add(Migration {
            kind: DocumentKind::Scene,
            from: 0,
            name: "entities-keyed-by-id",
            description: "entities and components were arrays carrying their id; they become objects keyed by it (order kept)",
            apply: scene_0_to_1,
        });
        m.add(Migration {
            kind: DocumentKind::Material,
            from: 1,
            name: "pbr-fields",
            description: "fynkel.pbr@1 materials carry the PbrMaterial fields (base_color, metallic, …, <slot>_texture) instead of parameters/textures maps; alphaMode/alphaCutoff become alpha_mode/alpha_cutoff, cull becomes double_sided, castShadows/receiveShadows moved to fynkel.MeshRenderer",
            apply: material_1_to_2,
        });
        m.add(Migration {
            kind: DocumentKind::Project,
            from: 1,
            name: "layers",
            description: "the collision layers became the project's layers (ADR-0020): physics.layers moves to the top-level layers, unchanged, so every bit keeps its meaning",
            apply: project_layers_1_to_2,
        });
        m.add(Migration {
            kind: DocumentKind::Meta,
            from: 1,
            name: "name-keys",
            description: "sub-asset keys come from the part's name in the file, not its position (mesh/ramp, not mesh/5; docs/design/stable-sub-asset-ids.md); a model's sidecar is migrated with its file and every document pointing into it, the others only change version",
            apply: meta_name_keys_1_to_2,
        });
        for kind in [DocumentKind::Scene, DocumentKind::Prefab] {
            m.add(Migration {
                kind,
                from: 1,
                name: "camera-exposure",
                description: "fynkel.Camera gained exposure_mode (auto | manual) and exposure (EV100); every camera gets the defaults auto / 12",
                apply: camera_exposure_1_to_2,
            });
            m.add(Migration {
                kind,
                from: 2,
                name: "animator-layers",
                description: "fynkel.Animator gained weight, additive and root_motion (M3-19c); every animator gets the defaults 1 / false / false",
                apply: animator_layers_2_to_3,
            });
            m.add(Migration {
                kind,
                from: 3,
                name: "object-layers",
                description: "a collider's layer became its object's (ADR-0020): fynkel.Collider and fynkel.CharacterController lose `layer`, an entity whose colliders were not on layer 0 gains a fynkel.Layer (the lowest bit when several), and fynkel.Camera, fynkel.PointLight and fynkel.SpotLight gain culling_mask (all layers)",
                apply: object_layers_3_to_4,
            });
            m.add(Migration {
                kind,
                from: 4,
                name: "one-root",
                description: "a prefab has one root and an instance is that root (ADR-0022): `roots` becomes `root` (several get a new root named after the prefab), and an instance's own components become overrides of the root — its Transform composed with the root's; needs the project (`fynkel migrate <project dir>`) when an instance has components or a prefab several roots",
                apply: crate::one_root::step_4_to_5,
            });
            m.add(Migration {
                kind,
                from: 5,
                name: "rigidbody-constraints",
                description: "fynkel.RigidBody gained constraints (Unity's RigidbodyConstraints, foundation 2.5); every body, and every RigidBody an override adds, gets 0 (nothing frozen)",
                apply: rigidbody_constraints_5_to_6,
            });
        }
        m
    }

    /// Registers a step; a second step for the same kind and version replaces the first.
    pub fn add(&mut self, step: Migration) {
        self.steps
            .retain(|s| !(s.kind == step.kind && s.from == step.from));
        self.steps.push(step);
        self.steps.sort_by_key(|s| (s.kind.name(), s.from));
    }

    pub fn steps(&self) -> &[Migration] {
        &self.steps
    }

    pub fn step(&self, kind: DocumentKind, from: u32) -> Option<&Migration> {
        self.steps.iter().find(|s| s.kind == kind && s.from == from)
    }

    /// The chain from `from` to `to`, or the first version with no step.
    pub fn path(&self, kind: DocumentKind, from: u32, to: u32) -> Result<Vec<&Migration>, u32> {
        let mut out = Vec::new();
        let mut v = from;
        while v < to {
            match self.step(kind, v) {
                Some(s) => {
                    out.push(s);
                    v = s.to();
                }
                None => return Err(v),
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub from: u32,
    pub to: u32,
    pub name: String,
    pub description: String,
}

/// The outcome of [`migrate`]: the (possibly unchanged) document, the steps
/// taken, a unified diff of the canonical text, and diagnostics — an error
/// among them means `document` is the original, untouched.
#[derive(Clone, Debug)]
pub struct Migrated {
    pub document: Document,
    pub from: u32,
    pub to: u32,
    pub applied: Vec<Applied>,
    pub diff: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl Migrated {
    pub fn changed(&self) -> bool {
        !self.applied.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        fynkel_contracts::has_errors(&self.diagnostics)
    }

    /// Writes the migrated document over `path`, after copying the original
    /// to `<path>.bak` (replacing an older backup). Refuses when nothing
    /// changed or the migration has errors. Returns the backup path.
    pub fn write_with_backup(&self, path: &Path) -> Result<PathBuf, Diagnostic> {
        let shown = path.display().to_string();
        if self.has_errors() {
            return Err(Diagnostic::error(
                Code::MIGRATION_FAILED,
                "not written: the migration reported errors",
            )
            .with_document(shown));
        }
        if !self.changed() {
            return Err(Diagnostic::error(
                Code::MIGRATION_FAILED,
                "not written: the document is already at the current version",
            )
            .with_document(shown));
        }
        let text = self.document.write()?;
        let backup = backup_path(path);
        std::fs::copy(path, &backup).map_err(|e| {
            Diagnostic::error(
                Code::IO,
                format!("cannot back up to {}: {e}", backup.display()),
            )
            .with_document(shown.clone())
        })?;
        std::fs::write(path, text).map_err(|e| {
            Diagnostic::error(Code::IO, format!("cannot write: {e}")).with_document(shown)
        })?;
        Ok(backup)
    }
}

/// `<file>.bak` next to the file.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// A change that needs more than the document itself — another file, the
/// whole project — applied before the per-kind steps (the stable
/// sub-asset keys: a model's sidecar and the documents pointing into it)
/// or, with `at`, just before the step from that version (one root per
/// prefab: the documents it reads are at that version's layout).
/// `apply` returns whether it changed anything; before the steps it may
/// move the document's `formatVersion` (root and envelope both), and the
/// steps then continue from there.
pub struct Rewrite<'a> {
    pub name: String,
    pub description: String,
    pub at: Option<u32>,
    pub apply: &'a dyn Fn(&mut Document) -> Result<bool, String>,
}

/// Brings `doc` to its kind's current `formatVersion`. The result is
/// validated against `schemas`; a step that fails, a missing step, or an
/// invalid result leaves `document` as the original and reports why.
pub fn migrate(doc: &Document, migrations: &Migrations, schemas: &SchemaSet) -> Migrated {
    migrate_with(doc, migrations, schemas, &[])
}

/// [`migrate`] with [`Rewrite`]s (see `at`). A rewrite that changes the
/// document counts as an applied step even when the version is current.
pub fn migrate_with(
    doc: &Document,
    migrations: &Migrations,
    schemas: &SchemaSet,
    rewrites: &[&Rewrite],
) -> Migrated {
    let from = doc.envelope.format_version;
    let to = doc.kind.supported_format_version();
    let shown = doc.path.clone();
    let with_doc = |d: Diagnostic| match &shown {
        Some(p) if d.document.is_none() => d.with_document(p.clone()),
        _ => d,
    };
    let unchanged = |diagnostics: Vec<Diagnostic>| Migrated {
        document: doc.clone(),
        from,
        to,
        applied: Vec::new(),
        diff: String::new(),
        diagnostics,
    };

    if from > to {
        return unchanged(vec![with_doc(
            anchor(
                doc,
                Diagnostic::error(
                    Code::UNSUPPORTED_FORMAT_VERSION,
                    format!(
                        "formatVersion {from} is newer than the supported {to}; the document is read-only"
                    ),
                )
                .with_suggestion("update the Fynkel tools"),
                "/formatVersion",
            ),
        )]);
    }
    let mut out = doc.clone();
    let mut applied = Vec::new();
    for rewrite in rewrites.iter().filter(|r| r.at.is_none()) {
        match (rewrite.apply)(&mut out) {
            Ok(true) => applied.push(Applied {
                from,
                to: out.envelope.format_version,
                name: rewrite.name.clone(),
                description: rewrite.description.clone(),
            }),
            Ok(false) => {}
            Err(msg) => {
                return unchanged(vec![with_doc(
                    Diagnostic::error(
                        Code::MIGRATION_FAILED,
                        format!("{} failed: {msg}", rewrite.name),
                    )
                    .with_pointer(""),
                )]);
            }
        }
    }
    let from_steps = out.envelope.format_version;
    if from_steps == to && applied.is_empty() {
        return unchanged(Vec::new());
    }
    let chain = match migrations.path(doc.kind, from_steps, to) {
        Ok(chain) => chain,
        Err(stuck) => {
            return unchanged(vec![with_doc(anchor(
                doc,
                Diagnostic::error(
                    Code::NO_MIGRATION_PATH,
                    format!(
                        "no migration from {} formatVersion {stuck} to {}",
                        doc.kind.name(),
                        if stuck + 1 == to {
                            to.to_string()
                        } else {
                            format!("{} (needed on the way to {to})", stuck + 1)
                        }
                    ),
                )
                .with_suggestion("the document may predate this tool's oldest supported version"),
                "/formatVersion",
            ))]);
        }
    };

    let before = match doc.write() {
        Ok(t) => t,
        Err(d) => return unchanged(vec![with_doc(d)]),
    };
    for step in chain {
        for rewrite in rewrites.iter().filter(|r| r.at == Some(step.from)) {
            match (rewrite.apply)(&mut out) {
                Ok(true) => applied.push(Applied {
                    from: step.from,
                    to: step.from,
                    name: rewrite.name.clone(),
                    description: rewrite.description.clone(),
                }),
                Ok(false) => {}
                Err(msg) => {
                    return unchanged(vec![with_doc(
                        Diagnostic::error(
                            Code::MIGRATION_FAILED,
                            format!("{} failed: {msg}", rewrite.name),
                        )
                        .with_pointer(""),
                    )]);
                }
            }
        }
        if let Err(msg) = (step.apply)(&mut out.root) {
            return unchanged(vec![with_doc(
                Diagnostic::error(
                    Code::MIGRATION_FAILED,
                    format!("{} failed: {msg}", step.id()),
                )
                .with_pointer("")
                .with_suggestion(
                    "fix the document by hand at the reported place, then migrate again",
                ),
            )]);
        }
        out.root
            .set("formatVersion", Node::integer(step.to() as i64));
        out.envelope.format_version = step.to();
        applied.push(Applied {
            from: step.from,
            to: step.to(),
            name: step.id(),
            description: step.description.to_string(),
        });
    }

    // A migration must produce a document the current schema accepts.
    let diagnostics: Vec<Diagnostic> = out.validate(schemas);
    if fynkel_contracts::has_errors(&diagnostics) {
        let mut diags = vec![with_doc(
            Diagnostic::error(
                Code::MIGRATION_FAILED,
                format!(
                    "the migrated document does not validate against {}; nothing written",
                    doc.kind.schema_file()
                ),
            )
            .with_pointer(""),
        )];
        diags.extend(diagnostics);
        return unchanged(diags);
    }
    let after = match out.write() {
        Ok(t) => t,
        Err(d) => return unchanged(vec![with_doc(d)]),
    };
    let label = shown.clone().unwrap_or_else(|| "document".to_string());
    let diff = similar::TextDiff::from_lines(&before, &after)
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("{label} (formatVersion {from})"),
            &format!("{label} (formatVersion {to})"),
        )
        .to_string();
    Migrated {
        document: out,
        from,
        to,
        applied,
        diff,
        diagnostics,
    }
}

fn anchor(doc: &Document, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    match doc.root.pointer_span(pointer) {
        Some(s) => d.at(s.start.line, s.start.column),
        None => d,
    }
}

/// `migrate --json` shape for one document.
pub fn migrated_json(m: &Migrated) -> serde_json::Value {
    serde_json::json!({
        "kind": m.document.kind.name(),
        "from": m.from,
        "to": m.to,
        "applied": m.applied.iter().map(|a| serde_json::json!({
            "from": a.from, "to": a.to, "name": a.name, "description": a.description,
        })).collect::<Vec<_>>(),
        "diff": m.diff,
    })
}

// ---------------------------------------------------------------------------
// Built-in steps
// ---------------------------------------------------------------------------

/// Scene 0 → 1: `entities` (and each entity's `components`) were arrays of
/// objects carrying their own `id`; they become objects keyed by that id,
/// in the same order, with the `id` member dropped. Anything else is kept.
fn scene_0_to_1(root: &mut Node) -> Result<(), String> {
    let entities = root.get_mut("entities").ok_or("entities is missing")?;
    if entities.as_object().is_some() {
        return Ok(()); // already keyed (a v0 writer that got ahead of itself)
    }
    let items = entities
        .as_array()
        .ok_or("/entities: expected an array")?
        .to_vec();
    let mut keyed = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let (id, mut entity) = take_id(item, &format!("/entities/{i}"))?;
        if let Some(components) = entity.get_mut("components") {
            if let Some(list) = components.as_array().map(<[Node]>::to_vec) {
                let mut by_id = Vec::with_capacity(list.len());
                for (j, c) in list.into_iter().enumerate() {
                    let (cid, component) = take_id(c, &format!("/entities/{i}/components/{j}"))?;
                    by_id.push((cid, component));
                }
                *components = Node::object(by_id);
            }
        }
        keyed.push((id, entity));
    }
    *entities = Node::object(keyed);
    Ok(())
}

/// Splits `{ "id": "<uuid>", ...rest }` into the id and the rest.
fn take_id(node: Node, at: &str) -> Result<(String, Node), String> {
    let members: Vec<Member> = node
        .as_object()
        .ok_or_else(|| format!("{at}: expected an object"))?
        .to_vec();
    let mut id = None;
    let mut rest = Vec::with_capacity(members.len());
    for m in members {
        if m.key == "id" {
            id = Some(
                m.value
                    .as_str()
                    .ok_or_else(|| format!("{at}/id: expected a string"))?
                    .to_string(),
            );
        } else {
            rest.push((m.key, m.value));
        }
    }
    let id = id.ok_or_else(|| format!("{at}: id is missing"))?;
    crate::parse_uuid(&id).map_err(|e| format!("{at}/id: {e}"))?;
    Ok((id, Node::object(rest)))
}

/// Material 1 → 2 (M3-02, `fynkel_core::material::PbrMaterial` as the
/// source of truth): for `fynkel.pbr@1` the `parameters` map becomes the
/// factor fields and each `textures` binding its `<slot>_texture`
/// reference (sampler, uvSet and colorSpace overrides are dropped: the
/// cache carries the colour space, samplers come from the texture's
/// import settings). For every material `alphaMode`/`alphaCutoff` become
/// `alpha_mode`/`alpha_cutoff`, `cull: none` becomes `double_sided: true`
/// (`front` has no equivalent and is dropped), and the shadow flags go
/// (they live on `fynkel.MeshRenderer`). Member order is kept; the maps
/// expand in place.
fn material_1_to_2(root: &mut Node) -> Result<(), String> {
    const PARAMETERS: [(&str, &str); 7] = [
        ("baseColor", "base_color"),
        ("metallic", "metallic"),
        ("roughness", "roughness"),
        ("emissive", "emissive"),
        ("emissiveStrength", "emissive_strength"),
        ("normalScale", "normal_scale"),
        ("occlusionStrength", "occlusion_strength"),
    ];
    const TEXTURES: [(&str, &str); 5] = [
        ("baseColor", "base_color_texture"),
        ("normal", "normal_texture"),
        ("metallicRoughness", "metallic_roughness_texture"),
        ("occlusion", "occlusion_texture"),
        ("emissive", "emissive_texture"),
    ];
    let pbr = root.get("shader").and_then(Node::as_str) == Some(crate::material::PBR_SHADER_ID);
    let members: Vec<Member> = root.as_object().ok_or("expected an object")?.to_vec();
    let mut out: Vec<(String, Node)> = Vec::with_capacity(members.len() + 8);
    for m in members {
        match m.key.as_str() {
            "alphaMode" => out.push(("alpha_mode".into(), m.value)),
            "alphaCutoff" => out.push(("alpha_cutoff".into(), m.value)),
            "cull" => {
                let none = m.value.as_str() == Some("none");
                out.push(("double_sided".into(), Node::synthetic(Value::Bool(none))));
            }
            "castShadows" | "receiveShadows" => {}
            "parameters" if pbr => {
                let params = m
                    .value
                    .as_object()
                    .ok_or("/parameters: expected an object")?;
                for p in params {
                    let (_, field) = PARAMETERS
                        .iter()
                        .find(|(old, _)| *old == p.key)
                        .ok_or_else(|| {
                            format!("/parameters/{}: fynkel.pbr@1 has no such parameter", p.key)
                        })?;
                    out.push(((*field).to_string(), p.value.clone()));
                }
            }
            "textures" if pbr => {
                let slots = m.value.as_object().ok_or("/textures: expected an object")?;
                for t in slots {
                    let (_, field) =
                        TEXTURES
                            .iter()
                            .find(|(old, _)| *old == t.key)
                            .ok_or_else(|| {
                                format!(
                                    "/textures/{}: fynkel.pbr@1 has no such texture slot",
                                    t.key
                                )
                            })?;
                    let texture = t
                        .value
                        .get("texture")
                        .ok_or_else(|| format!("/textures/{}: texture is missing", t.key))?;
                    out.push(((*field).to_string(), texture.clone()));
                }
            }
            _ => out.push((m.key, m.value)),
        }
    }
    *root = Node::object(out);
    Ok(())
}

/// Scene/prefab 2 → 3 (M3-19c, animation depth): every `fynkel.Animator`
/// component's `data` gains `weight: 1`, `additive: false` and
/// `root_motion: false` (the defaults — a plain animator plays as before)
/// unless the member is already there.
fn animator_layers_2_to_3(root: &mut Node) -> Result<(), String> {
    for_each_component_data(root, "fynkel.Animator", &mut |data| {
        if data.get("weight").is_none() {
            data.set("weight", Node::float(1.0));
        }
        if data.get("additive").is_none() {
            data.set("additive", Node::synthetic(Value::Bool(false)));
        }
        if data.get("root_motion").is_none() {
            data.set("root_motion", Node::synthetic(Value::Bool(false)));
        }
    })
}

/// The `data` of every component of type `ty` in a v1+ scene or prefab
/// (entities and components keyed by id); anything else is left alone.
fn for_each_component_data(
    root: &mut Node,
    ty: &str,
    f: &mut dyn FnMut(&mut Node),
) -> Result<(), String> {
    // A missing `entities` is the schema's complaint, not this step's.
    let Some(entities) = root.get_mut("entities") else {
        return Ok(());
    };
    let entities = entities
        .as_object_mut()
        .ok_or("/entities: expected an object")?;
    for entity in entities.iter_mut() {
        let Some(components) = entity.value.get_mut("components") else {
            continue;
        };
        let Some(components) = components.as_object_mut() else {
            continue;
        };
        for c in components.iter_mut() {
            if c.value.get("type").and_then(Node::as_str) != Some(ty) {
                continue;
            }
            let data = c.value.get_mut("data").ok_or_else(|| {
                format!(
                    "/entities/{}/components/{}: {ty} has no data",
                    entity.key, c.key
                )
            })?;
            if data.as_object().is_none() {
                return Err(format!(
                    "/entities/{}/components/{}/data: expected an object",
                    entity.key, c.key
                ));
            }
            f(data);
        }
    }
    Ok(())
}

/// `meta/1-2/name-keys`: nothing moves in a sidecar whose importer names
/// no parts. A model's (`fynkel.gltf`, `fynkel.fbx`) keys are renamed from
/// its file by the project rewrite (`fynkel_import::keys_migration`), which
/// moves the version itself; reaching this step means that rewrite could
/// not run.
fn meta_name_keys_1_to_2(root: &mut Node) -> Result<(), String> {
    match root.get("importer").and_then(Node::as_str) {
        Some(importer @ ("fynkel.gltf" | "fynkel.fbx")) => Err(format!(
            "a {importer} sidecar's sub-asset keys are renamed from its model file, together with every document that points into it: run `fynkel migrate` on the project's directory (the model file must be readable)"
        )),
        _ => Ok(()),
    }
}

/// Scene/prefab 1 → 2 (M3-04, renderer V1 exposure): every
/// `fynkel.Camera` component's `data` gains `exposure_mode: "auto"` and
/// `exposure: 12` (the defaults — nothing about how the scene looked
/// changes) unless the member is already there. Entities and components
/// are objects keyed by id (v1); anything else in the document is kept.
fn camera_exposure_1_to_2(root: &mut Node) -> Result<(), String> {
    for_each_component_data(root, "fynkel.Camera", &mut |data| {
        if data.get("exposure_mode").is_none() {
            data.set("exposure_mode", Node::string("auto"));
        }
        if data.get("exposure").is_none() {
            data.set("exposure", Node::float(12.0));
        }
    })
}

/// Scene/prefab 5 → 6 (foundation 2.5): every `fynkel.RigidBody`'s `data`
/// — an entity's component or an `addComponent` override's — gains
/// `constraints: 0` (nothing frozen, as before) unless it is there.
fn rigidbody_constraints_5_to_6(root: &mut Node) -> Result<(), String> {
    const TY: &str = "fynkel.RigidBody";
    let fill = |data: &mut Node| {
        if data.as_object().is_some() && data.get("constraints").is_none() {
            data.set("constraints", Node::integer(0));
        }
    };
    for_each_component_data(root, TY, &mut |d| fill(d))?;
    let Some(entities) = root.get_mut("entities").and_then(Node::as_object_mut) else {
        return Ok(());
    };
    for entity in entities.iter_mut() {
        let Some(overrides) = entity
            .value
            .get_mut("prefab")
            .and_then(|p| p.get_mut("overrides"))
            .and_then(Node::as_array_mut)
        else {
            continue;
        };
        for o in overrides.iter_mut() {
            if o.get("op").and_then(Node::as_str) == Some("addComponent")
                && o.get("type").and_then(Node::as_str) == Some(TY)
            {
                if let Some(data) = o.get_mut("data") {
                    fill(data);
                }
            }
        }
    }
    Ok(())
}

/// The components whose `layer` bits moved to the object (ADR-0020).
const LAYERED: [&str; 2] = ["fynkel.Collider", "fynkel.CharacterController"];
/// The components that gained `culling_mask` (ADR-0020).
const CULLING: [&str; 3] = ["fynkel.Camera", "fynkel.PointLight", "fynkel.SpotLight"];

/// Scene/prefab 3 → 4 (ADR-0020, tags and layers): a collider's `layer`
/// membership bits become its object's `fynkel.Layer` — the index of the
/// lowest set bit, and no component at all for layer 0 (the default) —
/// and `fynkel.Camera`, `fynkel.PointLight` and `fynkel.SpotLight` gain
/// `culling_mask: 4294967295` (every layer). The new component's id is
/// derived from the entity's (UUID v5), so migrating twice, or a prefab
/// and its copies, gives the same id. The same rewrite applies to the
/// `data` of an `addComponent` override; a `setField`/`unsetField` of a
/// collider's `layer` cannot be carried over (the override does not name
/// its component's type), so it fails the migration with its pointer.
fn object_layers_3_to_4(root: &mut Node) -> Result<(), String> {
    let Some(entities) = root.get_mut("entities") else {
        return Ok(());
    };
    let entities = entities
        .as_object_mut()
        .ok_or("/entities: expected an object")?;
    for entity in entities.iter_mut() {
        let at = format!("/entities/{}", entity.key);
        let mut bits: Option<u32> = None;
        if let Some(components) = entity
            .value
            .get_mut("components")
            .and_then(Node::as_object_mut)
        {
            for c in components.iter_mut() {
                let ty = c
                    .value
                    .get("type")
                    .and_then(Node::as_str)
                    .map(str::to_string);
                if let (Some(ty), Some(data)) = (ty, c.value.get_mut("data")) {
                    rewrite_data(&ty, data, &mut bits);
                }
            }
            if let Some(layer) = bits.and_then(lowest_layer) {
                let id = crate::parse_uuid(&entity.key).map_err(|e| format!("{at}: {e}"))?;
                components.push(Member {
                    key: layer_component_id(id).to_string(),
                    key_span: Default::default(),
                    value: layer_component(layer),
                });
            }
        }
        let Some(overrides) = entity
            .value
            .get_mut("prefab")
            .and_then(|p| p.get_mut("overrides"))
            .and_then(Node::as_array_mut)
        else {
            continue;
        };
        let mut out = Vec::with_capacity(overrides.len());
        for (i, o) in std::mem::take(overrides).into_iter().enumerate() {
            let op = o
                .get("op")
                .and_then(Node::as_str)
                .unwrap_or_default()
                .to_string();
            let field = o.get("field").and_then(Node::as_str).unwrap_or_default();
            if (op == "setField" || op == "unsetField") && field == "layer" {
                return Err(format!(
                    "{at}/prefab/overrides/{i}: a `{op}` of a collider's `layer` has no equivalent after ADR-0020 (the layer is the object's now: give the entity a fynkel.Layer override instead)"
                ));
            }
            let mut o = o;
            let mut added = None;
            if op == "addComponent" {
                let ty = o.get("type").and_then(Node::as_str).map(str::to_string);
                let mut bits = None;
                if let (Some(ty), Some(data)) = (ty, o.get_mut("data")) {
                    rewrite_data(&ty, data, &mut bits);
                }
                if let Some(layer) = bits.and_then(lowest_layer) {
                    let component = o
                        .get("component")
                        .and_then(Node::as_str)
                        .and_then(|c| crate::parse_uuid(c).ok())
                        .ok_or_else(|| {
                            format!("{at}/prefab/overrides/{i}/component: expected a UUID")
                        })?;
                    let target = o.get("entity").cloned().unwrap_or_else(|| Node::string(""));
                    added = Some(Node::object(vec![
                        ("op".into(), Node::string("addComponent")),
                        ("entity".into(), target),
                        (
                            "component".into(),
                            Node::string(layer_component_id(component).to_string()),
                        ),
                        ("type".into(), Node::string("fynkel.Layer")),
                        ("version".into(), Node::integer(1)),
                        (
                            "data".into(),
                            Node::object(vec![("layer".into(), Node::integer(layer as i64))]),
                        ),
                    ]));
                }
            }
            out.push(o);
            out.extend(added);
        }
        *overrides = out;
    }
    Ok(())
}

/// Strips a collider's `layer` into `bits` (OR-ed across the entity's
/// colliders) and gives a camera or light its `culling_mask`.
fn rewrite_data(ty: &str, data: &mut Node, bits: &mut Option<u32>) {
    if LAYERED.contains(&ty) {
        if let Some(members) = data.as_object_mut() {
            if let Some(i) = members.iter().position(|m| m.key == "layer") {
                let m = members.remove(i);
                let b = m.value.as_i64().unwrap_or(1) as u32;
                *bits = Some(bits.unwrap_or(0) | b);
            }
        }
    } else if CULLING.contains(&ty) && data.get("culling_mask").is_none() {
        data.set("culling_mask", Node::integer(u32::MAX as i64));
    }
}

/// The layer index of the lowest set bit; `None` for layer 0 (the
/// default, which needs no component) or no bits at all.
fn lowest_layer(bits: u32) -> Option<u32> {
    (bits != 0 && bits & 1 == 0).then(|| bits.trailing_zeros())
}

/// The id of the `fynkel.Layer` the migration gives `owner` (an entity, or
/// the component an `addComponent` override adds).
fn layer_component_id(owner: uuid::Uuid) -> uuid::Uuid {
    uuid::Uuid::new_v5(&owner, b"fynkel.Layer")
}

fn layer_component(layer: u32) -> Node {
    Node::object(vec![
        ("type".into(), Node::string("fynkel.Layer")),
        ("version".into(), Node::integer(1)),
        (
            "data".into(),
            Node::object(vec![("layer".into(), Node::integer(layer as i64))]),
        ),
    ])
}

/// Project 1 → 2 (ADR-0020): `physics.layers` moves to the top level as
/// `layers`, in the same order (bit `i` keeps its meaning). A project
/// without it keeps the default `["default"]` and gets no member.
fn project_layers_1_to_2(root: &mut Node) -> Result<(), String> {
    let Some(physics) = root.get_mut("physics").and_then(Node::as_object_mut) else {
        return Ok(());
    };
    let Some(i) = physics.iter().position(|m| m.key == "layers") else {
        return Ok(());
    };
    let layers = physics.remove(i).value;
    if root.get("layers").is_none() {
        root.set("layers", layers);
    }
    Ok(())
}
