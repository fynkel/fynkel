//! Material and shader documents (blueprint §6 Materials and shaders,
//! §19.2c item 6, §19.2d item 2): typed views over `.material.json` and
//! `.shader.json` and the rules that tie a material to its shader.
//!
//! A material for the built-in `fynkel.pbr@1` *is* a
//! [`PbrMaterial`](fynkel_core::material::PbrMaterial): the document's
//! fields are the record's (every one optional, `Default` when absent —
//! `schemas/material.schema.json` is generated from it), and
//! [`Material::pbr`] is the typed value the cook and the renderer consume.
//! A [`ShaderManifest`] says what a custom shader exposes: passes, typed
//! parameters with defaults and ranges, texture slots with colour spaces;
//! such a material sets `parameters` and `textures` that
//! [`Material::validate`] checks against the manifest and
//! [`Material::resolve`] completes with the manifest defaults. The
//! built-in shader has a manifest too ([`ShaderLibrary::builtin`]) so
//! `inspect` shows both kinds the same way.
//!
//! Like [`crate::scene`], the tree stays the source of truth; the views are
//! lenient so a broken document still describes itself, every problem
//! becoming a diagnostic with a pointer.
//!
//! Semantic rules, materials (`FYN-D030x`):
//! - the shader must be known (`D0300`);
//! - PBR: every factor within its range (`D0303`); the formatVersion 1
//!   `parameters`/`textures` maps are refused with a hint to `fynkel
//!   migrate` (`D0307`);
//! - custom: every parameter must exist in the manifest (`D0301`), have
//!   the manifest's type — number / bool / vector of the right length,
//!   whole numbers for `int` (`D0302`) — and lie within its range
//!   (`D0303`); every texture slot must exist (`D0304`); required slots
//!   must be bound (`D0305`); a binding that overrides the slot's colour
//!   space is flagged (`D0308`, warning: an sRGB normal map is almost
//!   always a mistake);
//! - `alpha_mode` must name a pass the shader has (`D0306`).
//!
//! Semantic rules, manifests (`FYN-D031x`): defaults must match their
//! parameter type and range (`D0310`), `min` must not exceed `max`
//! (`D0311`).

use fynkel_contracts::schema::HasEnumSchema;
use fynkel_contracts::{Code, Diagnostic};
pub use fynkel_core::material::{AlphaMode, PbrMaterial};
use uuid::Uuid;

use crate::codec::DocumentCodec;
use crate::json::{Node, Value};
use crate::scene::escape;
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

/// The manifest of the built-in PBR shader, as a document.
pub const PBR_MANIFEST: &str = include_str!("../shaders/fynkel.pbr@1.shader.json");

/// `shaderId` of the built-in PBR shader.
pub const PBR_SHADER_ID: &str = "fynkel.pbr@1";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParameterType {
    Float,
    Int,
    Bool,
    Vec2,
    Vec3,
    Vec4,
    Color3,
    Color4,
}

impl ParameterType {
    pub const ALL: [ParameterType; 8] = [
        ParameterType::Float,
        ParameterType::Int,
        ParameterType::Bool,
        ParameterType::Vec2,
        ParameterType::Vec3,
        ParameterType::Vec4,
        ParameterType::Color3,
        ParameterType::Color4,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ParameterType::Float => "float",
            ParameterType::Int => "int",
            ParameterType::Bool => "bool",
            ParameterType::Vec2 => "vec2",
            ParameterType::Vec3 => "vec3",
            ParameterType::Vec4 => "vec4",
            ParameterType::Color3 => "color3",
            ParameterType::Color4 => "color4",
        }
    }

    pub fn parse(s: &str) -> Option<ParameterType> {
        ParameterType::ALL.into_iter().find(|t| t.name() == s)
    }

    /// Number of components for vector types.
    pub const fn arity(self) -> Option<usize> {
        match self {
            ParameterType::Vec2 => Some(2),
            ParameterType::Vec3 | ParameterType::Color3 => Some(3),
            ParameterType::Vec4 | ParameterType::Color4 => Some(4),
            _ => None,
        }
    }
}

/// A parameter value as authored. Numbers stay `f64` (what the JSON layer
/// gives) so nothing is lost before the renderer packs them.
#[derive(Clone, PartialEq, Debug)]
pub enum ParameterValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    Vector(Vec<f64>),
}

impl ParameterValue {
    /// Reads `node` as a value of `ty`; the error is a short reason.
    pub fn from_node(node: &Node, ty: ParameterType) -> Result<ParameterValue, String> {
        match ty {
            ParameterType::Float => node
                .as_f64()
                .map(ParameterValue::Float)
                .ok_or_else(|| "expected a number".to_string()),
            ParameterType::Int => match node.as_f64() {
                Some(v) if v.fract() == 0.0 => Ok(ParameterValue::Int(v as i64)),
                Some(_) => Err("expected a whole number".to_string()),
                None => Err("expected a whole number".to_string()),
            },
            ParameterType::Bool => node
                .as_bool()
                .map(ParameterValue::Bool)
                .ok_or_else(|| "expected true or false".to_string()),
            _ => {
                let n = ty.arity().unwrap();
                let items = node
                    .as_array()
                    .ok_or_else(|| format!("expected an array of {n} numbers"))?;
                if items.len() != n {
                    return Err(format!("expected {n} components, got {}", items.len()));
                }
                let mut out = Vec::with_capacity(n);
                for item in items {
                    out.push(
                        item.as_f64()
                            .ok_or_else(|| format!("expected an array of {n} numbers"))?,
                    );
                }
                Ok(ParameterValue::Vector(out))
            }
        }
    }

    /// Components as numbers (a bool is `0`/`1`), for range checks.
    pub fn components(&self) -> Vec<f64> {
        match self {
            ParameterValue::Float(v) => vec![*v],
            ParameterValue::Int(v) => vec![*v as f64],
            ParameterValue::Bool(b) => vec![if *b { 1.0 } else { 0.0 }],
            ParameterValue::Vector(v) => v.clone(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ParameterValue::Float(v) => serde_json::json!(v),
            ParameterValue::Int(v) => serde_json::json!(v),
            ParameterValue::Bool(b) => serde_json::json!(b),
            ParameterValue::Vector(v) => serde_json::json!(v),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorSpace {
    Srgb,
    Linear,
}

impl ColorSpace {
    pub const fn name(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "srgb",
            ColorSpace::Linear => "linear",
        }
    }

    pub fn parse(s: &str) -> Option<ColorSpace> {
        match s {
            "srgb" => Some(ColorSpace::Srgb),
            "linear" => Some(ColorSpace::Linear),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pass {
    Opaque,
    Mask,
    Blend,
    Shadow,
}

impl Pass {
    pub const fn name(self) -> &'static str {
        match self {
            Pass::Opaque => "opaque",
            Pass::Mask => "mask",
            Pass::Blend => "blend",
            Pass::Shadow => "shadow",
        }
    }

    pub fn parse(s: &str) -> Option<Pass> {
        match s {
            "opaque" => Some(Pass::Opaque),
            "mask" => Some(Pass::Mask),
            "blend" => Some(Pass::Blend),
            "shadow" => Some(Pass::Shadow),
            _ => None,
        }
    }
}

/// The document name of an alpha mode (`opaque` | `mask` | `blend`).
pub fn alpha_mode_name(m: AlphaMode) -> &'static str {
    AlphaMode::ENUM.variants[m.to_index() as usize]
}

pub fn parse_alpha_mode(s: &str) -> Option<AlphaMode> {
    AlphaMode::ENUM
        .variants
        .iter()
        .position(|v| *v == s)
        .and_then(|i| AlphaMode::from_index(i as u32))
}

/// The pass a material with this alpha mode renders in.
pub const fn pass_of(m: AlphaMode) -> Pass {
    match m {
        AlphaMode::Opaque => Pass::Opaque,
        AlphaMode::Mask => Pass::Mask,
        AlphaMode::Blend => Pass::Blend,
    }
}

/// Document field names that belong to the PBR model only (not shared with
/// custom-shader materials).
pub const PBR_ONLY_FIELDS: [&str; 12] = [
    "base_color",
    "metallic",
    "roughness",
    "emissive",
    "emissive_strength",
    "normal_scale",
    "occlusion_strength",
    "base_color_texture",
    "normal_texture",
    "metallic_roughness_texture",
    "occlusion_texture",
    "emissive_texture",
];

/// Removes the PBR-only fields from a material body (a fresh document
/// re-pointed at a custom shader).
pub fn strip_pbr_fields(root: &mut Node) {
    if let Some(members) = root.as_object_mut() {
        members.retain(|m| !PBR_ONLY_FIELDS.contains(&m.key.as_str()));
    }
}

/// One parameter of a shader interface.
#[derive(Clone, PartialEq, Debug)]
pub struct ParameterSpec {
    pub name: String,
    pub ty: ParameterType,
    /// `None` when the manifest's default is malformed (reported).
    pub default: Option<ParameterValue>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub description: String,
    pub pointer: String,
}

/// One texture slot of a shader interface.
#[derive(Clone, PartialEq, Debug)]
pub struct TextureSlot {
    pub name: String,
    pub color_space: ColorSpace,
    pub required: bool,
    /// Fallback texel name (`white`, `black`, `normal`, `gray`) when unbound.
    pub default: Option<String>,
    pub description: String,
    pub pointer: String,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ShaderSource {
    Builtin(String),
    Wgsl(fynkel_contracts::AssetRef),
    /// A `.shader` file (ADR-0031): the interface is its header, the code
    /// its body ([`ShaderManifest::shader`]).
    File,
    /// Malformed or missing (reported).
    Unknown,
}

/// Typed read view of a shader manifest.
#[derive(Clone, Debug)]
pub struct ShaderManifest {
    pub document: Document,
    /// What a material's `shader` names: the built-in's `name@version`, or
    /// a `.shader` file's asset id (hyphenated).
    pub shader_id: String,
    /// The name people see: a `.shader` file's menu path
    /// (`Examples/Water`), the built-in's document name.
    pub name: String,
    /// The parsed file, for a `.shader` (ADR-0031); `None` for a JSON
    /// manifest (the built-in).
    pub shader: Option<std::sync::Arc<fynkel_shader::Shader>>,
    pub source: ShaderSource,
    pub passes: Vec<Pass>,
    pub features: Vec<String>,
    /// Manifest order.
    pub parameters: Vec<ParameterSpec>,
    /// Manifest order.
    pub textures: Vec<TextureSlot>,
    read_diagnostics: Vec<Diagnostic>,
}

impl ShaderManifest {
    pub fn from_document(document: Document) -> ShaderManifest {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Shader {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a shader manifest, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let anchor = |d: Diagnostic, pointer: &str| anchor(root, d, pointer);
        let malformed = |msg: &str, pointer: &str| {
            anchor(
                Diagnostic::error(Code::SHADER_MALFORMED_ENTRY, msg),
                pointer,
            )
        };

        let shader_id = match root.get("shaderId").and_then(Node::as_str) {
            Some(s) => s.to_string(),
            None => {
                diags.push(malformed("shaderId must be a string", "/shaderId"));
                String::new()
            }
        };
        let source = match root.get("source").and_then(Node::as_object) {
            Some(members) => {
                let builtin = members
                    .iter()
                    .find(|m| m.key == "builtin")
                    .and_then(|m| m.value.as_str());
                let wgsl = members.iter().any(|m| m.key == "wgsl");
                match (builtin, wgsl) {
                    (Some(b), false) => ShaderSource::Builtin(b.to_string()),
                    (None, true) => match crate::codec::asset_ref_field(members, "wgsl") {
                        Ok(r) => ShaderSource::Wgsl(r),
                        Err(e) => {
                            diags.push(malformed(
                                &format!("source.wgsl: {}", e.message),
                                &format!("/source{}", e.pointer),
                            ));
                            ShaderSource::Unknown
                        }
                    },
                    _ => {
                        diags.push(malformed(
                            "source must be { \"builtin\": name } or { \"wgsl\": assetRef }",
                            "/source",
                        ));
                        ShaderSource::Unknown
                    }
                }
            }
            None => {
                diags.push(malformed("source must be an object", "/source"));
                ShaderSource::Unknown
            }
        };
        let mut passes = Vec::new();
        match root.get("passes").and_then(Node::as_array) {
            Some(items) => {
                for (i, item) in items.iter().enumerate() {
                    match item.as_str().and_then(Pass::parse) {
                        Some(p) if !passes.contains(&p) => passes.push(p),
                        Some(p) => diags.push(malformed(
                            &format!("pass {:?} is listed twice", p.name()),
                            &format!("/passes/{i}"),
                        )),
                        None => diags.push(malformed(
                            "expected opaque | mask | blend | shadow",
                            &format!("/passes/{i}"),
                        )),
                    }
                }
            }
            None => diags.push(malformed("passes must be an array", "/passes")),
        }
        let features = root
            .get("features")
            .and_then(Node::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|n| n.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let mut parameters = Vec::new();
        match root.get("parameters").and_then(Node::as_object) {
            Some(members) => {
                for m in members {
                    let pointer = format!("/parameters/{}", escape(&m.key));
                    let Some(fields) = m.value.as_object() else {
                        diags.push(malformed("parameter must be an object", &pointer));
                        continue;
                    };
                    let get = |k: &str| fields.iter().find(|f| f.key == k).map(|f| &f.value);
                    let Some(ty) = get("type")
                        .and_then(Node::as_str)
                        .and_then(ParameterType::parse)
                    else {
                        diags.push(malformed(
                            "type must be float | int | bool | vec2 | vec3 | vec4 | color3 | color4",
                            &format!("{pointer}/type"),
                        ));
                        continue;
                    };
                    let default = match get("default") {
                        Some(node) => match ParameterValue::from_node(node, ty) {
                            Ok(v) => Some(v),
                            Err(why) => {
                                diags.push(anchor(
                                    Diagnostic::error(
                                        Code::SHADER_BAD_DEFAULT,
                                        format!("default of {} {:?}: {why}", ty.name(), m.key),
                                    ),
                                    &format!("{pointer}/default"),
                                ));
                                None
                            }
                        },
                        None => {
                            diags.push(malformed("default is required", &pointer));
                            None
                        }
                    };
                    parameters.push(ParameterSpec {
                        name: m.key.clone(),
                        ty,
                        default,
                        min: get("min").and_then(Node::as_f64),
                        max: get("max").and_then(Node::as_f64),
                        description: get("description")
                            .and_then(Node::as_str)
                            .unwrap_or("")
                            .to_string(),
                        pointer,
                    });
                }
            }
            None => diags.push(malformed("parameters must be an object", "/parameters")),
        }

        let mut textures = Vec::new();
        match root.get("textures").and_then(Node::as_object) {
            Some(members) => {
                for m in members {
                    let pointer = format!("/textures/{}", escape(&m.key));
                    let Some(fields) = m.value.as_object() else {
                        diags.push(malformed("texture slot must be an object", &pointer));
                        continue;
                    };
                    let get = |k: &str| fields.iter().find(|f| f.key == k).map(|f| &f.value);
                    let Some(color_space) = get("colorSpace")
                        .and_then(Node::as_str)
                        .and_then(ColorSpace::parse)
                    else {
                        diags.push(malformed(
                            "colorSpace must be srgb | linear",
                            &format!("{pointer}/colorSpace"),
                        ));
                        continue;
                    };
                    textures.push(TextureSlot {
                        name: m.key.clone(),
                        color_space,
                        required: get("required").and_then(Node::as_bool).unwrap_or(false),
                        default: get("default").and_then(Node::as_str).map(str::to_string),
                        description: get("description")
                            .and_then(Node::as_str)
                            .unwrap_or("")
                            .to_string(),
                        pointer,
                    });
                }
            }
            None => diags.push(malformed("textures must be an object", "/textures")),
        }

        for d in &mut diags {
            if d.document.is_none() {
                d.document = document.path.clone();
            }
        }
        let name = document.envelope.name.clone();
        ShaderManifest {
            document,
            shader_id,
            name,
            shader: None,
            source,
            passes,
            features,
            parameters,
            textures,
            read_diagnostics: diags,
        }
    }

    /// The interface of a `.shader` file (ADR-0031) whose asset id is `id`:
    /// its header's properties as parameters and slots, its alpha modes
    /// (and `shadows`) as passes. Materials name it by `id`.
    pub fn from_shader(id: Uuid, shader: fynkel_shader::Shader) -> ShaderManifest {
        use fynkel_shader::{PropertyKind, ValueType};
        let interface = &shader.interface;
        let mut document = Document::new(DocumentKind::Shader, &interface.name);
        document.path = shader.path.clone();
        document.envelope.id = id;
        document.root.set("id", Node::string(id.to_string()));
        let mut passes: Vec<Pass> = interface
            .alpha
            .iter()
            .map(|m| match m {
                fynkel_shader::AlphaMode::Opaque => Pass::Opaque,
                fynkel_shader::AlphaMode::Mask => Pass::Mask,
                fynkel_shader::AlphaMode::Blend => Pass::Blend,
            })
            .collect();
        if interface.shadows {
            passes.push(Pass::Shadow);
        }
        let mut parameters = Vec::new();
        let mut textures = Vec::new();
        for p in &interface.properties {
            let description = p.tooltip.clone().unwrap_or_default();
            match &p.kind {
                PropertyKind::Value {
                    ty,
                    default,
                    min,
                    max,
                } => {
                    let pty = match ty {
                        ValueType::Float => ParameterType::Float,
                        ValueType::Int => ParameterType::Int,
                        ValueType::Bool => ParameterType::Bool,
                        ValueType::Vec2 => ParameterType::Vec2,
                        ValueType::Vec3 => ParameterType::Vec3,
                        ValueType::Vec4 => ParameterType::Vec4,
                        ValueType::Color3 => ParameterType::Color3,
                        ValueType::Color => ParameterType::Color4,
                    };
                    let value = match ty {
                        ValueType::Float => ParameterValue::Float(default[0]),
                        ValueType::Int => ParameterValue::Int(default[0] as i64),
                        ValueType::Bool => ParameterValue::Bool(default[0] != 0.0),
                        _ => ParameterValue::Vector(default.clone()),
                    };
                    parameters.push(ParameterSpec {
                        name: p.name.clone(),
                        ty: pty,
                        default: Some(value),
                        min: *min,
                        max: *max,
                        description,
                        pointer: String::new(),
                    });
                }
                PropertyKind::Texture {
                    fallback,
                    color_space,
                    required,
                } => textures.push(TextureSlot {
                    name: p.name.clone(),
                    color_space: match color_space {
                        fynkel_shader::ColorSpace::Srgb => ColorSpace::Srgb,
                        fynkel_shader::ColorSpace::Linear => ColorSpace::Linear,
                    },
                    required: *required,
                    default: Some(fallback.name().to_string()),
                    description,
                    pointer: String::new(),
                }),
            }
        }
        ShaderManifest {
            document,
            shader_id: id.to_string(),
            name: interface.name.clone(),
            source: ShaderSource::File,
            passes,
            features: Vec::new(),
            parameters,
            textures,
            shader: Some(std::sync::Arc::new(shader)),
            read_diagnostics: Vec::new(),
        }
    }

    /// Parses manifest text (no file involved; `path` is for diagnostics).
    pub fn parse(path: Option<&str>, text: &str) -> Result<ShaderManifest, Vec<Diagnostic>> {
        Document::parse(DocumentKind::Shader, path, text).map(ShaderManifest::from_document)
    }

    /// How messages name the shader: a `.shader`'s menu path, the
    /// built-in's id.
    pub fn label(&self) -> &str {
        match self.source {
            ShaderSource::File => &self.name,
            _ => &self.shader_id,
        }
    }

    pub fn parameter(&self, name: &str) -> Option<&ParameterSpec> {
        self.parameters.iter().find(|p| p.name == name)
    }

    pub fn texture(&self, name: &str) -> Option<&TextureSlot> {
        self.textures.iter().find(|t| t.name == name)
    }

    pub fn has_pass(&self, pass: Pass) -> bool {
        self.passes.contains(&pass)
    }

    pub fn read_diagnostics(&self) -> &[Diagnostic] {
        &self.read_diagnostics
    }

    /// Envelope + JSON Schema, read problems, and the manifest rules.
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        if self.source == ShaderSource::File {
            // A `.shader`'s header was checked when it was read.
            return self.read_diagnostics.clone();
        }
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        let mut rest = self.read_diagnostics.clone();
        rest.extend(self.validate_semantics());
        sort_by_position(&mut rest);
        for mut d in rest {
            // JSON Schema already said it (wrong type / missing field).
            if d.code == Code::SHADER_MALFORMED_ENTRY
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

    /// The manifest rules only.
    pub fn validate_semantics(&self) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = Vec::new();
        for p in &self.parameters {
            if let (Some(min), Some(max)) = (p.min, p.max) {
                if min > max {
                    out.push(anchor(
                        root,
                        Diagnostic::error(
                            Code::SHADER_BAD_RANGE,
                            format!("{:?}: min {min} is greater than max {max}", p.name),
                        ),
                        &format!("{}/min", p.pointer),
                    ));
                    continue;
                }
            }
            if let Some(default) = &p.default {
                if let Some(why) = out_of_range(default, p) {
                    out.push(anchor(
                        root,
                        Diagnostic::error(
                            Code::SHADER_BAD_DEFAULT,
                            format!("default of {:?} {why}", p.name),
                        ),
                        &format!("{}/default", p.pointer),
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
}

/// The shaders a material may name: built-ins plus any manifests added by
/// the caller (a project's custom shaders).
#[derive(Clone, Debug, Default)]
pub struct ShaderLibrary {
    manifests: Vec<ShaderManifest>,
}

impl ShaderLibrary {
    /// No shaders at all; for tests.
    pub fn empty() -> ShaderLibrary {
        ShaderLibrary::default()
    }

    /// The engine's built-in shaders: `fynkel.pbr@1`.
    pub fn builtin() -> ShaderLibrary {
        let pbr = ShaderManifest::parse(Some("<builtin>/fynkel.pbr@1.shader.json"), PBR_MANIFEST)
            .expect("built-in PBR manifest parses");
        debug_assert!(pbr.read_diagnostics().is_empty());
        ShaderLibrary {
            manifests: vec![pbr],
        }
    }

    /// Adds (or replaces, by `shaderId`) a manifest.
    pub fn add(&mut self, manifest: ShaderManifest) {
        match self
            .manifests
            .iter_mut()
            .find(|m| m.shader_id == manifest.shader_id)
        {
            Some(slot) => *slot = manifest,
            None => self.manifests.push(manifest),
        }
    }

    pub fn get(&self, shader_id: &str) -> Option<&ShaderManifest> {
        self.manifests.iter().find(|m| m.shader_id == shader_id)
    }

    pub fn manifests(&self) -> &[ShaderManifest] {
        &self.manifests
    }

    /// Every shader as people see it, sorted, for messages: the built-in's
    /// id, a file's `Menu/Path (id)`.
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .manifests
            .iter()
            .map(|m| match m.source {
                ShaderSource::File => format!("{} ({})", m.name, m.shader_id),
                _ => m.shader_id.clone(),
            })
            .collect();
        v.sort_unstable();
        v
    }

    /// `shaderId`s, sorted, for messages.
    pub fn ids(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .manifests
            .iter()
            .map(|m| m.shader_id.as_str())
            .collect();
        v.sort_unstable();
        v
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Sampler {
    pub filter: String,
    pub mipmaps: bool,
    pub wrap: String,
    pub anisotropy: u32,
}

impl Default for Sampler {
    fn default() -> Sampler {
        Sampler {
            filter: "linear".into(),
            mipmaps: true,
            wrap: "repeat".into(),
            anisotropy: 1,
        }
    }
}

/// A texture bound to a slot, as authored.
#[derive(Clone, PartialEq, Debug)]
pub struct TextureBinding {
    pub slot: String,
    pub texture: fynkel_contracts::AssetRef,
    /// Overrides the slot's colour space when set.
    pub color_space: Option<ColorSpace>,
    pub uv_set: u32,
    pub sampler: Sampler,
    pub pointer: String,
}

/// A parameter value as authored, with where it came from.
#[derive(Clone, PartialEq, Debug)]
pub struct AuthoredParameter {
    pub name: String,
    /// The raw node; typed by the manifest during validation / resolution.
    pub node: Node,
    pub pointer: String,
}

/// Typed read view of a material document.
#[derive(Clone, Debug)]
pub struct Material {
    pub document: Document,
    pub shader: String,
    /// The typed model when `shader` is `fynkel.pbr@1` (defaults filled;
    /// a field that does not decode keeps its default and is reported).
    pub pbr: Option<PbrMaterial>,
    /// Custom shaders: document order (empty for PBR).
    pub parameters: Vec<AuthoredParameter>,
    /// Custom shaders: document order (empty for PBR).
    pub textures: Vec<TextureBinding>,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f64,
    pub double_sided: bool,
    read_diagnostics: Vec<Diagnostic>,
}

impl Material {
    pub fn from_document(document: Document) -> Material {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Material {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a material document, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let anchor = |d: Diagnostic, pointer: &str| anchor(root, d, pointer);
        let malformed = |msg: &str, pointer: &str| {
            anchor(
                Diagnostic::error(Code::MATERIAL_MALFORMED_ENTRY, msg),
                pointer,
            )
        };

        // The built-in by its `name@version`, or a `.shader` file by
        // asset reference (ADR-0031) — kept as its hyphenated id.
        let shader_node = root.get("shader");
        let shader = match shader_node.and_then(Node::as_str) {
            Some(s) => s.to_string(),
            None => match root
                .as_object()
                .filter(|_| shader_node.is_some_and(|n| n.as_object().is_some()))
                .map(|members| crate::codec::asset_ref_field(members, "shader"))
            {
                Some(Ok(r)) => r.asset.to_string(),
                _ => {
                    diags.push(
                        malformed(
                            "shader must be \"fynkel.pbr@1\" or a .shader file's asset reference",
                            "/shader",
                        )
                        .with_suggestion(format!(
                            "add \"shader\": \"{PBR_SHADER_ID}\" or \"shader\": {{ \"asset\": \"<the .shader's id>\" }}"
                        )),
                    );
                    String::new()
                }
            },
        };
        let is_pbr = shader == PBR_SHADER_ID;
        let mut pbr = None;
        let mut parameters = Vec::new();
        let mut textures = Vec::new();
        if is_pbr {
            for legacy in ["parameters", "textures"] {
                if root.get(legacy).is_some() {
                    diags.push(
                        malformed(
                            &format!(
                                "{PBR_SHADER_ID} materials carry the PbrMaterial fields (base_color, metallic, …, <slot>_texture); `{legacy}` is the formatVersion 1 form"
                            ),
                            &format!("/{legacy}"),
                        )
                        .with_suggestion("run `fynkel migrate <path>` to convert the document"),
                    );
                }
            }
            // Field by field so one bad value does not hide the rest: the
            // codec reports the first failure, so retry without that field.
            let mut body = root.clone();
            strip_legacy(&mut body);
            let value = loop {
                match PbrMaterial::from_node(&body) {
                    Ok(v) => break v,
                    Err(e) => {
                        let field = e
                            .pointer
                            .trim_start_matches('/')
                            .split('/')
                            .next()
                            .unwrap_or("")
                            .to_string();
                        diags.push(malformed(&format!("{field}: {}", e.message), &e.pointer));
                        if field.is_empty() || !remove_member(&mut body, &field) {
                            break PbrMaterial::default();
                        }
                    }
                }
            };
            pbr = Some(value);
        } else {
            for field in PBR_ONLY_FIELDS {
                if root.get(field).is_some() {
                    diags.push(
                        malformed(
                            &format!(
                                "`{field}` belongs to {PBR_SHADER_ID}; a custom shader's values go in `parameters` and `textures`"
                            ),
                            &format!("/{field}"),
                        )
                        .with_suggestion(format!("move the value into `parameters` (or `textures`) as {:?} names it", shader)),
                    );
                }
            }
            match root.get("parameters").map(|n| &n.value) {
                Some(Value::Object(members)) => {
                    for m in members {
                        parameters.push(AuthoredParameter {
                            name: m.key.clone(),
                            node: m.value.clone(),
                            pointer: format!("/parameters/{}", escape(&m.key)),
                        });
                    }
                }
                Some(_) => diags.push(malformed("parameters must be an object", "/parameters")),
                None => {}
            }
            match root.get("textures").map(|n| &n.value) {
                Some(Value::Object(members)) => {
                    for m in members {
                        let pointer = format!("/textures/{}", escape(&m.key));
                        let Some(fields) = m.value.as_object() else {
                            diags.push(malformed("texture binding must be an object", &pointer));
                            continue;
                        };
                        let texture = match crate::codec::asset_ref_field(fields, "texture") {
                            Ok(r) => r,
                            Err(e) => {
                                diags.push(malformed(
                                    &format!("texture: {}", e.message),
                                    &format!("{pointer}{}", e.pointer),
                                ));
                                continue;
                            }
                        };
                        let get = |k: &str| fields.iter().find(|f| f.key == k).map(|f| &f.value);
                        let color_space = match get("colorSpace") {
                            Some(n) => match n.as_str().and_then(ColorSpace::parse) {
                                Some(c) => Some(c),
                                None => {
                                    diags.push(malformed(
                                        "colorSpace must be srgb | linear",
                                        &format!("{pointer}/colorSpace"),
                                    ));
                                    None
                                }
                            },
                            None => None,
                        };
                        let mut sampler = Sampler::default();
                        if let Some(s) = get("sampler").and_then(Node::as_object) {
                            let sget = |k: &str| s.iter().find(|f| f.key == k).map(|f| &f.value);
                            if let Some(v) = sget("filter").and_then(Node::as_str) {
                                sampler.filter = v.to_string();
                            }
                            if let Some(v) = sget("mipmaps").and_then(Node::as_bool) {
                                sampler.mipmaps = v;
                            }
                            if let Some(v) = sget("wrap").and_then(Node::as_str) {
                                sampler.wrap = v.to_string();
                            }
                            if let Some(v) = sget("anisotropy").and_then(Node::as_i64) {
                                sampler.anisotropy = v.clamp(1, 16) as u32;
                            }
                        }
                        textures.push(TextureBinding {
                            slot: m.key.clone(),
                            texture,
                            color_space,
                            uv_set: get("uvSet").and_then(Node::as_i64).unwrap_or(0).max(0) as u32,
                            sampler,
                            pointer,
                        });
                    }
                }
                Some(_) => diags.push(malformed("textures must be an object", "/textures")),
                None => {}
            }
        }
        let (alpha_mode, alpha_cutoff, double_sided) = match &pbr {
            Some(p) => (p.alpha_mode, p.alpha_cutoff as f64, p.double_sided),
            None => {
                let alpha_mode = match root.get("alpha_mode") {
                    Some(n) => match n.as_str().and_then(parse_alpha_mode) {
                        Some(m) => m,
                        None => {
                            diags.push(malformed(
                                "alpha_mode must be opaque | mask | blend",
                                "/alpha_mode",
                            ));
                            AlphaMode::default()
                        }
                    },
                    None => AlphaMode::default(),
                };
                let alpha_cutoff = root
                    .get("alpha_cutoff")
                    .and_then(Node::as_f64)
                    .unwrap_or(0.5);
                let double_sided = root
                    .get("double_sided")
                    .and_then(Node::as_bool)
                    .unwrap_or(false);
                (alpha_mode, alpha_cutoff, double_sided)
            }
        };

        for d in &mut diags {
            if d.document.is_none() {
                d.document = document.path.clone();
            }
        }
        Material {
            document,
            shader,
            pbr,
            parameters,
            textures,
            alpha_mode,
            alpha_cutoff,
            double_sided,
            read_diagnostics: diags,
        }
    }

    pub fn parse(path: Option<&str>, text: &str) -> Result<Material, Vec<Diagnostic>> {
        Document::parse(DocumentKind::Material, path, text).map(Material::from_document)
    }

    pub fn id(&self) -> Uuid {
        self.document.envelope.id
    }

    pub fn read_diagnostics(&self) -> &[Diagnostic] {
        &self.read_diagnostics
    }

    pub fn binding(&self, slot: &str) -> Option<&TextureBinding> {
        self.textures.iter().find(|t| t.slot == slot)
    }

    /// Everything: envelope + JSON Schema ([`Document::validate`]), read
    /// problems, then the material-vs-manifest rules.
    pub fn validate(&self, schemas: &SchemaSet, shaders: &ShaderLibrary) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        let mut rest = self.read_diagnostics.clone();
        rest.extend(self.validate_semantics(shaders));
        sort_by_position(&mut rest);
        for mut d in rest {
            if d.code == Code::MATERIAL_MALFORMED_ENTRY
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

    /// The material-vs-manifest rules only (no schema pass).
    pub fn validate_semantics(&self, shaders: &ShaderLibrary) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let anchor = |d: Diagnostic, pointer: &str| anchor(root, d, pointer);
        let mut out = Vec::new();
        let Some(manifest) = shaders.get(&self.shader) else {
            if !self.shader.is_empty() {
                let by_name = self
                    .document
                    .root
                    .get("shader")
                    .and_then(Node::as_str)
                    .is_some();
                let d = if by_name {
                    // A custom `name@version` from before ADR-0031.
                    Diagnostic::error(
                        Code::MATERIAL_UNKNOWN_SHADER,
                        format!("unknown shader {:?}", self.shader),
                    )
                    .with_suggestion(format!(
                        "the built-in is \"{PBR_SHADER_ID}\"; a custom shader is a .shader file, named by its asset reference: \"shader\": {{ \"asset\": \"<id>\" }} (known: {})",
                        shaders.names().join(", ")
                    ))
                } else {
                    Diagnostic::error(
                        Code::MATERIAL_UNKNOWN_SHADER,
                        format!("no .shader file has the id {}", self.shader),
                    )
                    .with_suggestion(format!("known shaders: {}", shaders.names().join(", ")))
                };
                out.push(anchor(d, "/shader"));
            }
            return finish(out, &self.document);
        };

        if let Some(pbr) = &self.pbr {
            for (field, why) in pbr.violations() {
                let pointer = if root.get(field).is_some() {
                    format!("/{field}")
                } else {
                    String::new()
                };
                out.push(anchor(
                    Diagnostic::error(Code::MATERIAL_PARAMETER_RANGE, why),
                    &pointer,
                ));
            }
        }

        for p in &self.parameters {
            let Some(spec) = manifest.parameter(&p.name) else {
                out.push(anchor(
                    Diagnostic::error(
                        Code::MATERIAL_UNKNOWN_PARAMETER,
                        format!("{} has no parameter {:?}", manifest.label(), p.name),
                    )
                    .with_suggestion(format!(
                        "parameters: {}",
                        manifest
                            .parameters
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    &p.pointer,
                ));
                continue;
            };
            let value = match ParameterValue::from_node(&p.node, spec.ty) {
                Ok(v) => v,
                Err(why) => {
                    out.push(anchor(
                        Diagnostic::error(
                            Code::MATERIAL_PARAMETER_TYPE,
                            format!("{:?} is {}: {why}", p.name, spec.ty.name()),
                        ),
                        &p.pointer,
                    ));
                    continue;
                }
            };
            if let Some(why) = out_of_range(&value, spec) {
                out.push(anchor(
                    Diagnostic::error(
                        Code::MATERIAL_PARAMETER_RANGE,
                        format!("{:?} {why}", p.name),
                    ),
                    &p.pointer,
                ));
            }
        }

        for t in &self.textures {
            let Some(slot) = manifest.texture(&t.slot) else {
                out.push(anchor(
                    Diagnostic::error(
                        Code::MATERIAL_UNKNOWN_TEXTURE,
                        format!("{} has no texture slot {:?}", manifest.label(), t.slot),
                    )
                    .with_suggestion(format!(
                        "slots: {}",
                        manifest
                            .textures
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    &t.pointer,
                ));
                continue;
            };
            if let Some(cs) = t.color_space {
                if cs != slot.color_space {
                    out.push(anchor(
                        Diagnostic::warning(
                            Code::MATERIAL_COLOR_SPACE_OVERRIDE,
                            format!(
                                "slot {:?} is {} but this binding says {}",
                                t.slot,
                                slot.color_space.name(),
                                cs.name()
                            ),
                        )
                        .with_suggestion(
                            "drop colorSpace unless the texture really is encoded that way",
                        ),
                        &format!("{}/colorSpace", t.pointer),
                    ));
                }
            }
        }
        for slot in manifest.textures.iter().filter(|s| s.required) {
            if self.binding(&slot.name).is_none() {
                out.push(anchor(
                    Diagnostic::error(
                        Code::MATERIAL_MISSING_TEXTURE,
                        format!("{} requires texture {:?}", manifest.label(), slot.name),
                    )
                    .with_suggestion(format!(
                        "add \"textures\": {{ \"{}\": {{ \"texture\": {{ \"asset\": <uuid> }} }} }}",
                        slot.name
                    )),
                    if root.get("textures").is_some() {
                        "/textures"
                    } else {
                        ""
                    },
                ));
            }
        }

        if !manifest.has_pass(pass_of(self.alpha_mode)) {
            out.push(anchor(
                Diagnostic::error(
                    Code::MATERIAL_UNSUPPORTED_ALPHA_MODE,
                    format!(
                        "{} has no {} pass",
                        manifest.label(),
                        alpha_mode_name(self.alpha_mode)
                    ),
                )
                .with_suggestion(format!(
                    "passes: {}",
                    manifest
                        .passes
                        .iter()
                        .map(|p| p.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                if root.get("alpha_mode").is_some() {
                    "/alpha_mode"
                } else {
                    ""
                },
            ));
        }
        finish(out, &self.document)
    }

    /// A `.shader` asset's material (ADR-0031) as the renderer draws it:
    /// checked against `shader`'s header — its values' types and ranges,
    /// its alpha modes, its required textures, the document diagnostics —
    /// then the property block in the shader's layout with the defaults
    /// filled and one slot per texture property — `shader` being the
    /// shader the material names. The cook's `MATC`, and
    /// `world.createShaderMaterial`'s at runtime. The payload is `None`
    /// when there are errors; the diagnostics (warnings too) either way.
    pub fn custom_payload(
        &self,
        shader: &fynkel_shader::Shader,
    ) -> (
        Option<fynkel_cooked::payload::CustomMaterialPayload>,
        Vec<Diagnostic>,
    ) {
        use fynkel_cooked::payload as pl;
        let mut library = ShaderLibrary::builtin();
        // `shader` is the one the material names (its `shader` asset id);
        // any other name is reported unknown by the check below.
        let id = self.shader.parse::<Uuid>().unwrap_or_default();
        library.add(ShaderManifest::from_shader(id, shader.clone()));
        let diagnostics = self.validate_semantics(&library);
        if fynkel_contracts::has_errors(&diagnostics) {
            return (None, diagnostics);
        }
        let Some(resolved) = self.resolve(&library) else {
            return (None, diagnostics);
        };
        let values: std::collections::HashMap<&str, Vec<f64>> = resolved
            .parameters
            .iter()
            .map(|p| (p.name.as_str(), p.value.components()))
            .collect();
        let block = shader
            .layout
            .pack(&shader.interface, |n| values.get(n).map(Vec::as_slice));
        let slots = resolved
            .textures
            .iter()
            .map(|t| {
                let sampler = t
                    .binding
                    .as_ref()
                    .map(|b| b.sampler.clone())
                    .unwrap_or_default();
                pl::MaterialSlot {
                    texture: t
                        .binding
                        .as_ref()
                        .map(|b| (b.texture.asset.0, b.texture.sub.clone())),
                    color_space: match t.color_space {
                        ColorSpace::Srgb => pl::COLOR_SPACE_SRGB,
                        ColorSpace::Linear => pl::COLOR_SPACE_LINEAR,
                    },
                    fallback: match t.fallback.as_deref() {
                        Some("black") => pl::FALLBACK_BLACK,
                        Some("normal") => pl::FALLBACK_NORMAL,
                        Some("gray") => pl::FALLBACK_GRAY,
                        _ => pl::FALLBACK_WHITE,
                    },
                    filter: (sampler.filter == "nearest") as u32,
                    mipmaps: sampler.mipmaps,
                    wrap: match sampler.wrap.as_str() {
                        "clamp" => 1,
                        "mirror" => 2,
                        _ => 0,
                    },
                    anisotropy: sampler.anisotropy,
                    uv_set: t.binding.as_ref().map_or(0, |b| b.uv_set),
                }
            })
            .collect();
        let payload = pl::CustomMaterialPayload {
            shader: id.into_bytes(),
            alpha_mode: match self.alpha_mode {
                AlphaMode::Opaque => pl::ALPHA_OPAQUE,
                AlphaMode::Mask => pl::ALPHA_MASK,
                AlphaMode::Blend => pl::ALPHA_BLEND,
            },
            alpha_cutoff: self.alpha_cutoff as f32,
            double_sided: self.double_sided,
            block,
            slots,
        };
        (Some(payload), diagnostics)
    }

    /// Every parameter and slot of the manifest with the material's value
    /// or the manifest default: what `inspect` shows (the renderer reads
    /// [`Material::pbr`] through the cache instead). Values that do not
    /// fit the manifest fall back to the default (validation reports
    /// them). A PBR material's fields are presented as the built-in
    /// manifest's parameters and slots. `None` when the shader is unknown.
    pub fn resolve(&self, shaders: &ShaderLibrary) -> Option<ResolvedMaterial> {
        let manifest = shaders.get(&self.shader)?;
        let (parameters_in, textures_in) = match &self.pbr {
            Some(pbr) => pbr_as_manifest_values(pbr, &self.document.root),
            None => (self.parameters.clone(), self.textures.clone()),
        };
        let parameters = manifest
            .parameters
            .iter()
            .map(|spec| {
                let authored = parameters_in
                    .iter()
                    .find(|p| p.name == spec.name)
                    .and_then(|p| ParameterValue::from_node(&p.node, spec.ty).ok())
                    .filter(|v| out_of_range(v, spec).is_none());
                ResolvedParameter {
                    name: spec.name.clone(),
                    ty: spec.ty,
                    authored: authored.is_some(),
                    value: authored
                        .or_else(|| spec.default.clone())
                        .unwrap_or(ParameterValue::Float(0.0)),
                }
            })
            .collect();
        let textures = manifest
            .textures
            .iter()
            .map(|slot| {
                let binding = textures_in.iter().find(|t| t.slot == slot.name).cloned();
                ResolvedTexture {
                    slot: slot.name.clone(),
                    color_space: binding
                        .as_ref()
                        .and_then(|b| b.color_space)
                        .unwrap_or(slot.color_space),
                    fallback: slot.default.clone(),
                    binding,
                }
            })
            .collect();
        Some(ResolvedMaterial {
            id: self.id(),
            name: self.document.envelope.name.clone(),
            shader_id: manifest.shader_id.clone(),
            pass: pass_of(self.alpha_mode),
            pbr: self.pbr.clone(),
            parameters,
            textures,
            alpha_mode: self.alpha_mode,
            alpha_cutoff: self.alpha_cutoff,
            double_sided: self.double_sided,
        })
    }
}

/// A PBR material's fields as the built-in manifest's parameters and slots
/// (for [`Material::resolve`]); `authored` follows whether the document
/// sets the field.
fn pbr_as_manifest_values(
    pbr: &PbrMaterial,
    root: &Node,
) -> (Vec<AuthoredParameter>, Vec<TextureBinding>) {
    let node = pbr.to_node();
    let param = |name: &str, field: &str| -> Option<AuthoredParameter> {
        root.get(field)?;
        Some(AuthoredParameter {
            name: name.to_string(),
            node: node.get(field)?.clone(),
            pointer: format!("/{field}"),
        })
    };
    let parameters = [
        ("baseColor", "base_color"),
        ("metallic", "metallic"),
        ("roughness", "roughness"),
        ("emissive", "emissive"),
        ("emissiveStrength", "emissive_strength"),
        ("normalScale", "normal_scale"),
        ("occlusionStrength", "occlusion_strength"),
    ]
    .into_iter()
    .filter_map(|(name, field)| param(name, field))
    .collect();
    let textures = [
        ("baseColor", "base_color_texture"),
        ("normal", "normal_texture"),
        ("metallicRoughness", "metallic_roughness_texture"),
        ("occlusion", "occlusion_texture"),
        ("emissive", "emissive_texture"),
    ]
    .into_iter()
    .zip(pbr.textures())
    .filter_map(|((slot, field), (_, _, texture))| {
        Some(TextureBinding {
            slot: slot.to_string(),
            texture: texture?.clone(),
            color_space: None,
            uv_set: 0,
            sampler: Sampler::default(),
            pointer: format!("/{field}"),
        })
    })
    .collect();
    (parameters, textures)
}

fn strip_legacy(root: &mut Node) {
    if let Some(members) = root.as_object_mut() {
        members.retain(|m| m.key != "parameters" && m.key != "textures");
    }
}

fn remove_member(root: &mut Node, key: &str) -> bool {
    match root.as_object_mut() {
        Some(members) => {
            let before = members.len();
            members.retain(|m| m.key != key);
            members.len() != before
        }
        None => false,
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedParameter {
    pub name: String,
    pub ty: ParameterType,
    pub value: ParameterValue,
    /// Set by the material (as opposed to the manifest default).
    pub authored: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedTexture {
    pub slot: String,
    pub binding: Option<TextureBinding>,
    /// Effective colour space (binding override or slot default).
    pub color_space: ColorSpace,
    /// Manifest fallback texel when unbound.
    pub fallback: Option<String>,
}

/// A material with every manifest default filled in.
#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedMaterial {
    pub id: Uuid,
    pub name: String,
    pub shader_id: String,
    pub pass: Pass,
    /// The typed model for `fynkel.pbr@1`.
    pub pbr: Option<PbrMaterial>,
    pub parameters: Vec<ResolvedParameter>,
    pub textures: Vec<ResolvedTexture>,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f64,
    pub double_sided: bool,
}

impl ResolvedMaterial {
    pub fn parameter(&self, name: &str) -> Option<&ResolvedParameter> {
        self.parameters.iter().find(|p| p.name == name)
    }

    pub fn texture(&self, slot: &str) -> Option<&ResolvedTexture> {
        self.textures.iter().find(|t| t.slot == slot)
    }

    /// `inspect --json` payload.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": "material",
            "id": self.id.to_string(),
            "name": self.name,
            "shader": self.shader_id,
            "pass": self.pass.name(),
            "alphaMode": alpha_mode_name(self.alpha_mode),
            "alphaCutoff": self.alpha_cutoff,
            "doubleSided": self.double_sided,
            "parameters": self.parameters.iter().map(|p| serde_json::json!({
                "name": p.name, "type": p.ty.name(), "value": p.value.to_json(), "authored": p.authored,
            })).collect::<Vec<_>>(),
            "textures": self.textures.iter().map(|t| serde_json::json!({
                "slot": t.slot,
                "colorSpace": t.color_space.name(),
                "texture": t.binding.as_ref().map(|b| b.texture.to_string()),
                "sampler": t.binding.as_ref().map(|b| serde_json::json!({
                    "filter": b.sampler.filter, "mipmaps": b.sampler.mipmaps,
                    "wrap": b.sampler.wrap, "anisotropy": b.sampler.anisotropy,
                })),
                "uvSet": t.binding.as_ref().map(|b| b.uv_set),
                "fallback": t.fallback,
            })).collect::<Vec<_>>(),
        })
    }

    /// Human `inspect` output.
    pub fn to_text(&self) -> String {
        let mut s = format!(
            "{} ({})\n  shader {}  pass {}  alpha {}{}  double-sided {}\n",
            self.name,
            self.id,
            self.shader_id,
            self.pass.name(),
            alpha_mode_name(self.alpha_mode),
            if self.alpha_mode == AlphaMode::Mask {
                format!(" cutoff {}", self.alpha_cutoff)
            } else {
                String::new()
            },
            self.double_sided
        );
        s.push_str("  parameters\n");
        for p in &self.parameters {
            s.push_str(&format!(
                "    {:<20} {:<7} {}{}\n",
                p.name,
                p.ty.name(),
                p.value.to_json(),
                if p.authored { "" } else { "  (default)" }
            ));
        }
        s.push_str("  textures\n");
        for t in &self.textures {
            match &t.binding {
                Some(b) => s.push_str(&format!(
                    "    {:<20} {:<7} {}  {} {} aniso {}\n",
                    t.slot,
                    t.color_space.name(),
                    b.texture,
                    b.sampler.filter,
                    b.sampler.wrap,
                    b.sampler.anisotropy
                )),
                None => s.push_str(&format!(
                    "    {:<20} {:<7} (unbound{})\n",
                    t.slot,
                    t.color_space.name(),
                    t.fallback
                        .as_ref()
                        .map(|f| format!(", {f}"))
                        .unwrap_or_default()
                )),
            }
        }
        s
    }
}

/// `inspect --json` payload for a manifest.
pub fn manifest_json(m: &ShaderManifest) -> serde_json::Value {
    serde_json::json!({
        "kind": "shader",
        "id": m.document.envelope.id.to_string(),
        "name": m.document.envelope.name,
        "shaderId": m.shader_id,
        "source": match &m.source {
            ShaderSource::Builtin(b) => serde_json::json!({ "builtin": b }),
            ShaderSource::Wgsl(r) => serde_json::json!({ "wgsl": r.to_string() }),
            ShaderSource::File => serde_json::json!({ "file": m.document.path }),
            ShaderSource::Unknown => serde_json::Value::Null,
        },
        "shaderName": m.name,
        "lighting": m.shader.as_ref().map_or("lit", |s| s.interface.lighting.name()),
        "passes": m.passes.iter().map(|p| p.name()).collect::<Vec<_>>(),
        "features": m.features,
        "parameters": m.parameters.iter().map(|p| serde_json::json!({
            "name": p.name, "type": p.ty.name(),
            "default": p.default.as_ref().map(ParameterValue::to_json),
            "min": p.min, "max": p.max, "description": p.description,
        })).collect::<Vec<_>>(),
        "textures": m.textures.iter().map(|t| serde_json::json!({
            "name": t.name, "colorSpace": t.color_space.name(), "required": t.required,
            "default": t.default, "description": t.description,
        })).collect::<Vec<_>>(),
    })
}

/// Reads and compiles the `.shader` file at `file` (ADR-0031), whose
/// asset id is `id`; `path` is what diagnostics name.
pub fn read_shader_file(
    id: Uuid,
    path: &str,
    file: &std::path::Path,
) -> Result<ShaderManifest, Vec<Diagnostic>> {
    let text = std::fs::read_to_string(file).map_err(|e| {
        vec![Diagnostic::error(Code::IO, format!("cannot read {path}: {e}")).with_document(path)]
    })?;
    fynkel_shader::check(Some(path), &text).map(|shader| ShaderManifest::from_shader(id, shader))
}

/// What a `.shader.json` in a project gets (ADR-0031): no longer a
/// project document, with the `.shader` header that says the same.
pub fn legacy_manifest(m: &ShaderManifest) -> Diagnostic {
    let d = Diagnostic::error(
        Code::UNKNOWN_DOCUMENT_KIND,
        "a .shader.json manifest is no longer a project document: a shader is one .shader file (ADR-0031)",
    )
    .with_pointer("")
    .with_suggestion(format!(
        "write <name>.shader with this header and the code, then point its materials at it by asset reference:\n{}",
        header_for(m)
    ));
    match &m.document.path {
        Some(p) => d.with_document(p.clone()),
        None => d,
    }
}

/// The `.shader` header that says what a `.shader.json` manifest said
/// (ADR-0031: manifests are no longer project documents) — what the
/// diagnostic about one suggests.
pub fn header_for(m: &ShaderManifest) -> String {
    use std::fmt::Write;
    let mut s = format!(
        "shader \"{}\" {{\n",
        if m.name.is_empty() {
            &m.shader_id
        } else {
            &m.name
        }
    );
    let alpha: Vec<&str> = m
        .passes
        .iter()
        .filter(|p| **p != Pass::Shadow)
        .map(|p| p.name())
        .collect();
    if !alpha.is_empty() && alpha.len() < 3 {
        let _ = writeln!(s, "    alpha {}", alpha.join(", "));
    }
    if !m.passes.contains(&Pass::Shadow) {
        s.push_str("    shadows off\n");
    }
    s.push_str("    properties {\n");
    let num = |x: f64| {
        let t = format!("{x}");
        t
    };
    for p in &m.parameters {
        let ty = match p.ty {
            ParameterType::Color4 => "color",
            t => t.name(),
        };
        let _ = write!(s, "        {} {ty}", p.name);
        if let Some(v) = &p.default {
            let c = v.components();
            let text = match v {
                ParameterValue::Bool(b) => b.to_string(),
                _ if c.len() == 1 => num(c[0]),
                _ => format!(
                    "({})",
                    c.iter().map(|x| num(*x)).collect::<Vec<_>>().join(", ")
                ),
            };
            let _ = write!(s, " = {text}");
        }
        if let (Some(a), Some(b)) = (p.min, p.max) {
            let _ = write!(s, " range({}, {})", num(a), num(b));
        }
        if !p.description.is_empty() {
            let _ = write!(s, " {:?}", p.description);
        }
        s.push('\n');
    }
    for t in &m.textures {
        let _ = write!(s, "        {} texture", t.name);
        if let Some(d) = &t.default {
            let _ = write!(s, " = {d}");
        }
        let _ = write!(s, " {}", t.color_space.name());
        if t.required {
            s.push_str(" required");
        }
        if !t.description.is_empty() {
            let _ = write!(s, " {:?}", t.description);
        }
        s.push('\n');
    }
    s.push_str("    }\n}\n\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    return s;\n}\n");
    s
}

/// Why `value` falls outside `spec`'s range, if it does.
fn out_of_range(value: &ParameterValue, spec: &ParameterSpec) -> Option<String> {
    if matches!(value, ParameterValue::Bool(_)) {
        return None;
    }
    for (i, c) in value.components().iter().enumerate() {
        let which = if value.components().len() > 1 {
            format!("component {i} = {c}")
        } else {
            format!("= {c}")
        };
        if let Some(min) = spec.min {
            if *c < min {
                return Some(format!("{which} is below min {min}"));
            }
        }
        if let Some(max) = spec.max {
            if *c > max {
                return Some(format!("{which} is above max {max}"));
            }
        }
    }
    None
}

/// Document order, so read problems and semantic rules interleave the way
/// the file reads.
fn sort_by_position(diags: &mut [Diagnostic]) {
    diags.sort_by(|a, b| (a.line, a.column, &a.pointer).cmp(&(b.line, b.column, &b.pointer)));
}

/// Positions the diagnostic at `pointer`, or at the nearest ancestor that
/// exists (a missing field is reported at its parent's position).
fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    let mut at = pointer.to_string();
    loop {
        if let Some(s) = root.pointer_span(&at) {
            return d.at(s.start.line, s.start.column);
        }
        match at.rfind('/') {
            Some(i) => at.truncate(i),
            None => return d,
        }
    }
}

fn finish(mut out: Vec<Diagnostic>, document: &Document) -> Vec<Diagnostic> {
    for d in &mut out {
        if d.document.is_none() {
            d.document = document.path.clone();
        }
    }
    out
}
