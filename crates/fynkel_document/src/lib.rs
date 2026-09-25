//! `fynkel_document` — authoring documents (ADR-0006) and, from M2-14 on,
//! the authoring command service (ADR-0016).
//!
//! M2-02 scope: the strict JSON layer with spans ([`json`]), the document
//! envelope shared by every kind ([`Document`], [`Envelope`]), JSON Schema
//! 2020-12 validation behind one entry point ([`SchemaSet`]), and the §14.2
//! diagnostic record (in `fynkel_contracts`) with JSON output for the CLI.
//!
//! Envelope (blueprint §19.2c scope rules): every document is one object
//! with `$schema`, `formatVersion`, `id` (UUID), `name` and a kind-specific
//! body. Unknown keys are preserved in value and position; the canonical
//! writer only hoists the envelope keys to the front, in that order.

#![forbid(unsafe_code)]
// A `Diagnostic` is the error type on purpose: rich, and only ever built on
// the failure path of tooling, where its size is irrelevant.
#![allow(clippy::result_large_err)]

pub mod asset;
pub mod authoring;
pub mod codec;
pub mod data;
pub mod editor_part;
#[rustfmt::skip]
#[allow(clippy::all)]
pub mod generated;
pub mod blend;
pub mod json;
pub mod material;
pub mod migrate;
pub mod one_root;
pub mod physics_material;
pub mod prefab;
pub mod project;
pub mod reference_kinds;
pub mod rig;
pub mod runtime;
pub mod scene;
pub mod schema;
pub mod settings;
pub mod volume;

use std::path::Path;

pub use asset::{AssetDatabase, AssetEntry, AssetKind, Meta, Resolved};
pub use authoring::{
    Applied, Command, ComponentSpec, Edit, Editor, Mutation, Request, Requested, Revision,
    Transaction,
};
pub use blend::Blend;
pub use codec::{DocumentCodec, FieldError};
pub use data::DataDocument;
pub use fynkel_contracts::{has_errors, Code, Diagnostic, Severity};
pub use json::{Node, Value};
pub use material::{Material, ResolvedMaterial, ShaderLibrary, ShaderManifest};
pub use migrate::{migrate, migrate_with, Migrated, Migration, Migrations, Rewrite};
pub use physics_material::PhysicsMaterialDocument;
pub use prefab::{expand, Expanded, Prefab, PrefabLibrary};
pub use project::{
    validate_project, validate_project_with, Lock, NoSourcePrefabs, Project, ProjectReport,
    SourcePrefabs,
};
pub use rig::Rig;
pub use runtime::{describe, AuthoredBehaviour, DescribedScene};
pub use scene::Scene;
pub use schema::{BehaviourInfo, ComponentInfo, SchemaSet};
pub use volume::VolumeDocument;

/// Document kinds and their file-name conventions (§6, §7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DocumentKind {
    Scene,
    Prefab,
    Material,
    Shader,
    Project,
    /// `.meta.json` asset sidecar (ADR-0006).
    Meta,
    /// `.rig.json`: a character assembled from imported models (M3-19b).
    Rig,
    /// `.blend.json`: clips along one parameter (M3-19c).
    Blend,
    /// `.volume.json`: a post-processing profile (R3a).
    Volume,
    /// `.physicsmaterial.json`: friction and bounciness colliders share
    /// (ADR-0025).
    PhysicsMaterial,
    /// `.data.json`: a data asset, an instance of a game-declared type
    /// (ADR-0027).
    Data,
}

impl DocumentKind {
    pub const ALL: [DocumentKind; 11] = [
        DocumentKind::Scene,
        DocumentKind::Prefab,
        DocumentKind::Material,
        DocumentKind::Shader,
        DocumentKind::Project,
        DocumentKind::Meta,
        DocumentKind::Rig,
        DocumentKind::Blend,
        DocumentKind::Volume,
        DocumentKind::PhysicsMaterial,
        DocumentKind::Data,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            DocumentKind::Scene => "scene",
            DocumentKind::Prefab => "prefab",
            DocumentKind::Material => "material",
            DocumentKind::Shader => "shader",
            DocumentKind::Project => "project",
            DocumentKind::Meta => "meta",
            DocumentKind::Rig => "rig",
            DocumentKind::Blend => "blend",
            DocumentKind::Volume => "volume",
            DocumentKind::PhysicsMaterial => "physics_material",
            DocumentKind::Data => "data",
        }
    }

    /// File-name suffix; `fynkel.project.json` is the one fixed name.
    pub const fn suffix(self) -> &'static str {
        match self {
            DocumentKind::Scene => ".scene.json",
            DocumentKind::Prefab => ".prefab.json",
            DocumentKind::Material => ".material.json",
            DocumentKind::Shader => ".shader.json",
            DocumentKind::Project => "fynkel.project.json",
            DocumentKind::Meta => ".meta.json",
            DocumentKind::Rig => ".rig.json",
            DocumentKind::Blend => ".blend.json",
            DocumentKind::Volume => ".volume.json",
            DocumentKind::PhysicsMaterial => ".physicsmaterial.json",
            DocumentKind::Data => ".data.json",
        }
    }

    /// Schema file name inside `schemas/`.
    pub fn schema_file(self) -> String {
        format!("{}.schema.json", self.name())
    }

    /// Canonical `$schema` URL (also the schema's `$id`).
    pub fn schema_id(self) -> String {
        format!("https://fynkel.dev/schemas/{}", self.schema_file())
    }

    pub fn from_path(path: &Path) -> Option<DocumentKind> {
        let name = path.file_name()?.to_str()?;
        if name == "fynkel.project.json" {
            return Some(DocumentKind::Project);
        }
        DocumentKind::ALL
            .into_iter()
            .find(|k| *k != DocumentKind::Project && name.ends_with(k.suffix()))
    }

    pub fn parse_name(name: &str) -> Option<DocumentKind> {
        DocumentKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// Newest `formatVersion` this build reads and writes for the kind.
    /// Material 2 (M3-02): the `fynkel.PbrMaterial` fields replaced the
    /// `parameters`/`textures` maps for `fynkel.pbr@1`. Scene and prefab 2
    /// (M3-04): `fynkel.Camera` carries `exposure_mode`/`exposure`; 3
    /// (M3-19c): `fynkel.Animator` carries `weight`/`additive`/`root_motion`;
    /// 4 (ADR-0020): a collider's layer is its object's `fynkel.Layer`, and
    /// cameras and point/spot lights carry `culling_mask`; 5 (ADR-0022): a
    /// prefab has one `root`, and an instance is that root. Project 2
    /// (ADR-0020): `layers` and `tags` at the top level. Meta 2 (stable
    /// sub-asset ids): a model's sub-asset keys are the parts' names.
    pub const fn supported_format_version(self) -> u32 {
        match self {
            DocumentKind::Scene | DocumentKind::Prefab => 5,
            DocumentKind::Project | DocumentKind::Meta => 2,
            DocumentKind::Material => 2,
            _ => 1,
        }
    }
}

/// The fields every document carries.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Envelope {
    pub schema: Option<String>,
    pub format_version: u32,
    pub id: uuid::Uuid,
    pub name: String,
}

pub const ENVELOPE_KEYS: [&str; 4] = ["$schema", "formatVersion", "id", "name"];

/// A parsed document: the full tree (unknown data included) plus the typed
/// envelope. `root` is the single source of truth; `envelope` is a view
/// refreshed by [`Document::write`].
#[derive(Clone, Debug)]
pub struct Document {
    pub kind: DocumentKind,
    /// As given to the tool; used in diagnostics.
    pub path: Option<String>,
    pub root: Node,
    pub envelope: Envelope,
}

impl Document {
    /// A fresh document with a new id and the smallest valid body for the
    /// kind (an empty scene, an untextured PBR material, ...).
    pub fn new(kind: DocumentKind, name: &str) -> Document {
        let id = uuid::Uuid::new_v4();
        let mut members = vec![
            ("$schema".into(), Node::string(kind.schema_id())),
            (
                "formatVersion".into(),
                Node::integer(kind.supported_format_version() as i64),
            ),
            ("id".into(), Node::string(id.to_string())),
            ("name".into(), Node::string(name)),
        ];
        match kind {
            DocumentKind::Scene => members.push(("entities".into(), Node::object(Vec::new()))),
            DocumentKind::Prefab => {
                // One root (ADR-0022): an object named after the prefab
                // with the default Transform.
                let root = uuid::Uuid::new_v4().to_string();
                let transform = Node::object(vec![
                    ("type".into(), Node::string("fynkel.Transform")),
                    ("version".into(), Node::integer(1)),
                    ("data".into(), prefab::identity_transform()),
                ]);
                let entity = Node::object(vec![
                    ("name".into(), Node::string(name)),
                    ("parent".into(), Node::synthetic(Value::Null)),
                    ("children".into(), Node::synthetic(Value::Array(Vec::new()))),
                    (
                        "components".into(),
                        Node::object(vec![(uuid::Uuid::new_v4().to_string(), transform)]),
                    ),
                ]);
                members.push((
                    "entities".into(),
                    Node::object(vec![(root.clone(), entity)]),
                ));
                members.push(("root".into(), Node::string(root)));
            }
            DocumentKind::Material => {
                members.push(("shader".into(), Node::string(material::PBR_SHADER_ID)));
            }
            DocumentKind::Meta => {
                members.push(("importer".into(), Node::string(asset::FILE_IMPORTER)));
                members.push(("importerVersion".into(), Node::integer(1)));
                members.push(("settings".into(), Node::object(Vec::new())));
                members.push(("subAssets".into(), Node::object(Vec::new())));
            }
            DocumentKind::Rig => {
                members.push((
                    "skeleton".into(),
                    Node::object(vec![(
                        "asset".into(),
                        Node::string(uuid::Uuid::nil().to_string()),
                    )]),
                ));
                members.push(("clips".into(), Node::object(Vec::new())));
            }
            DocumentKind::Blend => {
                members.push(("parameter".into(), Node::string("speed")));
                members.push((
                    "points".into(),
                    Node::synthetic(Value::Array(vec![Node::object(vec![
                        ("at".into(), Node::integer(0)),
                        (
                            "clip".into(),
                            Node::object(vec![(
                                "asset".into(),
                                Node::string(uuid::Uuid::nil().to_string()),
                            )]),
                        ),
                    ])])),
                ));
                members.push(("sync".into(), Node::string("phase")));
            }
            DocumentKind::Volume => {
                // A starting look (the engine has every effect off without
                // a volume): R2's ambient occlusion on, bloom ready to be
                // given an intensity; every field shows what a profile can
                // override (delete one to inherit it).
                members.push((
                    "ambientOcclusion".into(),
                    Node::object(vec![
                        ("radius".into(), Node::float(0.6)),
                        ("intensity".into(), Node::float(1.35)),
                    ]),
                ));
                members.push((
                    "bloom".into(),
                    Node::object(vec![
                        ("intensity".into(), Node::float(0.0)),
                        ("threshold".into(), Node::float(0.9)),
                        ("scatter".into(), Node::float(0.7)),
                        (
                            "tint".into(),
                            Node::synthetic(Value::Array(vec![
                                Node::float(1.0),
                                Node::float(1.0),
                                Node::float(1.0),
                            ])),
                        ),
                    ]),
                ));
            }
            DocumentKind::PhysicsMaterial => {
                // Unity's default material, spelled out (every field is
                // optional; an absent one takes the same default).
                let m = fynkel_physics::PhysicsMaterial::default();
                // The f32's shortest text, so the file reads 0.6.
                let short = |v: f32| Node::float(v.to_string().parse().unwrap_or(v as f64));
                members.push(("friction".into(), short(m.friction)));
                members.push(("bounciness".into(), short(m.bounciness)));
                members.push((
                    "friction_combine".into(),
                    Node::string(m.friction_combine.name()),
                ));
                members.push((
                    "bounce_combine".into(),
                    Node::string(m.bounce_combine.name()),
                ));
            }
            DocumentKind::Data => {
                // A placeholder type; `Document::new_data` names the real one.
                members.push(("type".into(), Node::string(data::PLACEHOLDER_TYPE)));
                members.push(("version".into(), Node::integer(1)));
                members.push(("data".into(), Node::object(Vec::new())));
            }
            _ => {}
        }
        let root = Node::object(members);
        Document {
            kind,
            path: None,
            root,
            envelope: Envelope {
                schema: Some(kind.schema_id()),
                format_version: kind.supported_format_version(),
                id,
                name: name.to_string(),
            },
        }
    }

    /// Parses strict JSON and the envelope. `Err` carries every envelope
    /// problem found (or the single syntax error); the document cannot be
    /// used without a valid envelope. Body validation is [`Document::validate`].
    pub fn parse(
        kind: DocumentKind,
        path: Option<&str>,
        text: &str,
    ) -> Result<Document, Vec<Diagnostic>> {
        let with_doc = |d: Diagnostic| match path {
            Some(p) => d.with_document(p),
            None => d,
        };
        let root = json::parse(text).map_err(|d| vec![with_doc(d)])?;
        let (envelope, diags) = parse_envelope(kind, &root);
        match envelope {
            Some(envelope) if !has_errors(&diags) => Ok(Document {
                kind,
                path: path.map(str::to_string),
                root,
                envelope,
            }),
            _ => Err(diags.into_iter().map(with_doc).collect()),
        }
    }

    /// Reads and parses a file; the kind comes from the file name unless given.
    pub fn read(path: &Path, kind: Option<DocumentKind>) -> Result<Document, Vec<Diagnostic>> {
        let shown = path.display().to_string();
        let kind = match kind.or_else(|| DocumentKind::from_path(path)) {
            Some(k) => k,
            None => {
                return Err(vec![Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    "cannot tell the document kind from the file name",
                )
                .with_document(shown)
                .with_suggestion("use .scene.json, .prefab.json, .material.json, .shader.json, .meta.json or fynkel.project.json")])
            }
        };
        let text = std::fs::read_to_string(path).map_err(|e| {
            vec![Diagnostic::error(Code::IO, format!("cannot read: {e}"))
                .with_document(shown.clone())]
        })?;
        Document::parse(kind, Some(&shown), &text)
    }

    /// Envelope warnings (schema reference mismatch, newer format version)
    /// plus JSON Schema validation of the whole document. Semantic checks
    /// per kind are layered on by the kind modules (M2-03 onwards).
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        if self.is_outdated() {
            // The body follows an older layout; the current schema would
            // only produce noise. `fynkel migrate` brings it up to date.
            let d = Diagnostic::error(
                Code::OUTDATED_FORMAT_VERSION,
                format!(
                    "formatVersion {} is older than the current {}; the document must be migrated before it can be validated or loaded",
                    self.envelope.format_version,
                    self.kind.supported_format_version()
                ),
            )
            .with_pointer("/formatVersion")
            .with_suggestion(format!(
                "run `fynkel migrate {}` (add --dry-run to preview the change)",
                self.path.as_deref().unwrap_or("<path>")
            ));
            let d = match self.root.pointer_span("/formatVersion") {
                Some(s) => d.at(s.start.line, s.start.column),
                None => d,
            };
            return vec![match &self.path {
                Some(p) => d.with_document(p.clone()),
                None => d,
            }];
        }
        let (_, envelope_diags) = parse_envelope(self.kind, &self.root);
        let envelope_pointers: Vec<String> = envelope_diags
            .iter()
            .filter_map(|d| d.pointer.clone())
            .collect();
        out.extend(envelope_diags);
        for d in schemas.validate(self.kind, &self.root) {
            // The typed envelope check already said it, with a better message.
            if d.pointer
                .as_ref()
                .is_some_and(|p| envelope_pointers.contains(p))
            {
                continue;
            }
            out.push(d);
        }
        for d in &mut out {
            if d.document.is_none() {
                d.document = self.path.clone();
            }
        }
        out
    }

    /// True when the file was written by a newer tool: it may be read and
    /// validated, never rewritten (§6).
    pub fn is_read_only(&self) -> bool {
        self.envelope.format_version > self.kind.supported_format_version()
    }

    /// True when the file follows an older layout (`fynkel migrate`).
    pub fn is_outdated(&self) -> bool {
        self.envelope.format_version < self.kind.supported_format_version()
    }

    /// Canonical text (§6). Envelope keys first, in order; everything else
    /// keeps its position. Refuses (returns `Err`) for read-only documents.
    pub fn write(&self) -> Result<String, Diagnostic> {
        if self.is_read_only() {
            return Err(Diagnostic::error(
                Code::UNSUPPORTED_FORMAT_VERSION,
                format!(
                    "formatVersion {} is newer than the supported {}; refusing to rewrite",
                    self.envelope.format_version,
                    self.kind.supported_format_version()
                ),
            )
            .with_pointer("/formatVersion"));
        }
        let mut root = self.root.clone();
        if let Some(members) = root.as_object_mut() {
            let mut hoisted = Vec::with_capacity(members.len());
            for key in ENVELOPE_KEYS {
                if let Some(i) = members.iter().position(|m| m.key == key) {
                    hoisted.push(members.remove(i));
                }
            }
            hoisted.append(members);
            *members = hoisted;
        }
        Ok(json::write(&root))
    }

    /// Writes the canonical text to `path` (no backup here; migrations do that).
    pub fn save(&self, path: &Path) -> Result<(), Diagnostic> {
        let text = self.write()?;
        std::fs::write(path, text).map_err(|e| {
            Diagnostic::error(Code::IO, format!("cannot write: {e}"))
                .with_document(path.display().to_string())
        })
    }
}

/// Typed envelope read with §14.2 diagnostics. Returns the envelope when
/// every required field is usable, and every problem found (warnings
/// included) regardless.
pub fn parse_envelope(kind: DocumentKind, root: &Node) -> (Option<Envelope>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let at = |d: Diagnostic, pointer: &str| -> Diagnostic {
        let d = d.with_pointer(pointer);
        match root.pointer_span(pointer) {
            Some(s) => d.at(s.start.line, s.start.column),
            None => d.at(root.span.start.line, root.span.start.column),
        }
    };

    let schema = match root.get("$schema") {
        None => None,
        Some(n) => match n.as_str() {
            Some(s) => {
                if !s.ends_with(&kind.schema_file()) {
                    diags.push(at(
                        Diagnostic::warning(
                            Code::SCHEMA_REFERENCE_MISMATCH,
                            format!("$schema {s:?} does not name {}", kind.schema_file()),
                        )
                        .with_suggestion(format!("expected {}", kind.schema_id())),
                        "/$schema",
                    ));
                }
                Some(s.to_string())
            }
            None => {
                diags.push(at(
                    Diagnostic::error(Code::ENVELOPE_WRONG_TYPE, "$schema must be a string"),
                    "/$schema",
                ));
                None
            }
        },
    };

    let format_version = match root.get("formatVersion") {
        None => {
            diags.push(at(
                Diagnostic::error(Code::ENVELOPE_MISSING_FIELD, "formatVersion is required")
                    .with_suggestion(format!(
                        "add \"formatVersion\": {}",
                        kind.supported_format_version()
                    )),
                "",
            ));
            None
        }
        Some(n) => match n.as_i64() {
            Some(v) if v >= 0 && v <= u32::MAX as i64 => {
                let v = v as u32;
                if v > kind.supported_format_version() {
                    diags.push(at(
                        Diagnostic::error(
                            Code::UNSUPPORTED_FORMAT_VERSION,
                            format!(
                                "formatVersion {v} is newer than the supported {}; the document is read-only",
                                kind.supported_format_version()
                            ),
                        )
                        .with_suggestion("update the Fynkel tools"),
                        "/formatVersion",
                    ));
                }
                Some(v)
            }
            _ => {
                diags.push(at(
                    Diagnostic::error(
                        Code::ENVELOPE_WRONG_TYPE,
                        "formatVersion must be a non-negative integer",
                    ),
                    "/formatVersion",
                ));
                None
            }
        },
    };

    let id = match root.get("id") {
        None => {
            diags.push(at(
                Diagnostic::error(Code::ENVELOPE_MISSING_FIELD, "id is required")
                    .with_suggestion("add a fresh UUID, e.g. from `fynkel document new`"),
                "",
            ));
            None
        }
        Some(n) => {
            match n.as_str().map(parse_uuid) {
                Some(Ok(u)) => Some(u),
                Some(Err(why)) => {
                    diags.push(at(
                    Diagnostic::error(Code::MALFORMED_ID, format!("id is not a canonical UUID: {why}"))
                        .with_suggestion("use lower-case 8-4-4-4-12 hex, e.g. 70d7ba8b-f7fb-4eba-920d-ec9e6b968330"),
                    "/id",
                ));
                    None
                }
                None => {
                    diags.push(at(
                        Diagnostic::error(Code::ENVELOPE_WRONG_TYPE, "id must be a string"),
                        "/id",
                    ));
                    None
                }
            }
        }
    };

    let name = match root.get("name") {
        None => {
            diags.push(at(
                Diagnostic::error(Code::ENVELOPE_MISSING_FIELD, "name is required"),
                "",
            ));
            None
        }
        Some(n) => match n.as_str() {
            Some(s) => Some(s.to_string()),
            None => {
                diags.push(at(
                    Diagnostic::error(Code::ENVELOPE_WRONG_TYPE, "name must be a string"),
                    "/name",
                ));
                None
            }
        },
    };

    let envelope = match (format_version, id, name) {
        (Some(format_version), Some(id), Some(name)) => Some(Envelope {
            schema,
            format_version,
            id,
            name,
        }),
        _ => None,
    };
    (envelope, diags)
}

/// Canonical UUID text only: lower-case, hyphenated, no braces or URN prefix.
pub fn parse_uuid(s: &str) -> Result<uuid::Uuid, &'static str> {
    if s.len() != 36 {
        return Err("expected 36 characters");
    }
    if s.chars().any(|c| c.is_ascii_uppercase()) {
        return Err("upper-case hex");
    }
    let u = uuid::Uuid::try_parse(s).map_err(|_| "not a UUID")?;
    if u.is_nil() {
        return Err("the nil UUID is reserved");
    }
    Ok(u)
}

/// `--json` shape for a list of diagnostics: `{ "ok": bool, "diagnostics": [...] }`.
pub fn diagnostics_json(list: &[Diagnostic]) -> serde_json::Value {
    serde_json::json!({
        "ok": !has_errors(list),
        "diagnostics": list.iter().map(diagnostic_json).collect::<Vec<_>>(),
    })
}

/// One §14.2 record as JSON; absent fields are omitted, not null.
pub fn diagnostic_json(d: &Diagnostic) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("code".into(), d.code.as_str().into());
    m.insert("severity".into(), d.severity.as_str().into());
    m.insert("message".into(), d.message.clone().into());
    if let Some(v) = &d.document {
        m.insert("document".into(), v.clone().into());
    }
    if let Some(v) = &d.pointer {
        m.insert("pointer".into(), v.clone().into());
    }
    if let Some(v) = &d.id {
        m.insert("id".into(), v.clone().into());
    }
    if let Some(v) = d.line {
        m.insert("line".into(), v.into());
    }
    if let Some(v) = d.column {
        m.insert("column".into(), v.into());
    }
    if let Some(v) = &d.suggestion {
        m.insert("suggestion".into(), v.clone().into());
    }
    if !d.related.is_empty() {
        m.insert(
            "related".into(),
            serde_json::Value::Array(d.related.iter().map(diagnostic_json).collect()),
        );
    }
    serde_json::Value::Object(m)
}
