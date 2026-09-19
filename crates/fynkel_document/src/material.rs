//! Material and shader documents (blueprint §6 Materials and shaders,
//! §19.2c item 6): typed views over `.material.json` and `.shader.json`
//! and the rules that tie a material to its shader's interface manifest.
//!
//! A [`ShaderManifest`] says what a shader exposes: passes, typed
//! parameters with defaults and ranges, texture slots with colour spaces.
//! Built-in shaders ship their manifest inside the engine
//! ([`ShaderLibrary::builtin`], `fynkel.pbr@1`); custom ones are read from
//! documents and added to the library. A [`Material`] names a shader and
//! sets values; [`Material::validate`] checks them against the manifest
//! and [`Material::resolve`] fills in every default so a consumer (the
//! renderer, `inspect`) sees the full parameter set.
//!
//! Like [`crate::scene`], the tree stays the source of truth; the views are
//! lenient so a broken document still describes itself, every problem
//! becoming a diagnostic with a pointer.
//!
//! Semantic rules, materials (`FYN-D030x`):
//! - the shader must be known (`D0300`);
//! - every parameter must exist in the manifest (`D0301`), have the
//!   manifest's type — number / bool / vector of the right length, whole
//!   numbers for `int` (`D0302`) — and lie within its range (`D0303`);
//! - every texture slot must exist (`D0304`); required slots must be
//!   bound (`D0305`);
//! - `alphaMode` must name a pass the shader has (`D0306`);
//! - a binding that overrides the slot's colour space is flagged (`D0308`,
//!   warning: an sRGB normal map is almost always a mistake).
//!
//! Semantic rules, manifests (`FYN-D031x`): defaults must match their
//! parameter type and range (`D0310`), `min` must not exceed `max`
//! (`D0311`).

use fynkel_contracts::{Code, Diagnostic};
use uuid::Uuid;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AlphaMode {
    #[default]
    Opaque,
    Mask,
    Blend,
}

impl AlphaMode {
    pub const fn name(self) -> &'static str {
        match self {
            AlphaMode::Opaque => "opaque",
            AlphaMode::Mask => "mask",
            AlphaMode::Blend => "blend",
        }
    }

    pub fn parse(s: &str) -> Option<AlphaMode> {
        match s {
            "opaque" => Some(AlphaMode::Opaque),
            "mask" => Some(AlphaMode::Mask),
            "blend" => Some(AlphaMode::Blend),
            _ => None,
        }
    }

    /// The pass a material with this mode renders in.
    pub const fn pass(self) -> Pass {
        match self {
            AlphaMode::Opaque => Pass::Opaque,
            AlphaMode::Mask => Pass::Mask,
            AlphaMode::Blend => Pass::Blend,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CullMode {
    #[default]
    Back,
    Front,
    None,
}

impl CullMode {
    pub const fn name(self) -> &'static str {
        match self {
            CullMode::Back => "back",
            CullMode::Front => "front",
            CullMode::None => "none",
        }
    }

    pub fn parse(s: &str) -> Option<CullMode> {
        match s {
            "back" => Some(CullMode::Back),
            "front" => Some(CullMode::Front),
            "none" => Some(CullMode::None),
            _ => None,
        }
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
    /// Malformed or missing (reported).
    Unknown,
}

/// Typed read view of a shader manifest.
#[derive(Clone, Debug)]
pub struct ShaderManifest {
    pub document: Document,
    pub shader_id: String,
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
        ShaderManifest {
            document,
            shader_id,
            source,
            passes,
            features,
            parameters,
            textures,
            read_diagnostics: diags,
        }
    }

    /// Parses manifest text (no file involved; `path` is for diagnostics).
    pub fn parse(path: Option<&str>, text: &str) -> Result<ShaderManifest, Vec<Diagnostic>> {
        Document::parse(DocumentKind::Shader, path, text).map(ShaderManifest::from_document)
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
    /// Document order.
    pub parameters: Vec<AuthoredParameter>,
    /// Document order.
    pub textures: Vec<TextureBinding>,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f64,
    pub cull: CullMode,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
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

        let shader = match root.get("shader").and_then(Node::as_str) {
            Some(s) => s.to_string(),
            None => {
                diags.push(
                    malformed(
                        "shader must be a string such as \"fynkel.pbr@1\"",
                        "/shader",
                    )
                    .with_suggestion(format!("add \"shader\": \"{PBR_SHADER_ID}\"")),
                );
                String::new()
            }
        };
        let mut parameters = Vec::new();
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
        let mut textures = Vec::new();
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
        let alpha_mode = match root.get("alphaMode") {
            Some(n) => match n.as_str().and_then(AlphaMode::parse) {
                Some(m) => m,
                None => {
                    diags.push(malformed(
                        "alphaMode must be opaque | mask | blend",
                        "/alphaMode",
                    ));
                    AlphaMode::default()
                }
            },
            None => AlphaMode::default(),
        };
        let cull = match root.get("cull") {
            Some(n) => match n.as_str().and_then(CullMode::parse) {
                Some(m) => m,
                None => {
                    diags.push(malformed("cull must be back | front | none", "/cull"));
                    CullMode::default()
                }
            },
            None => CullMode::default(),
        };
        let alpha_cutoff = root
            .get("alphaCutoff")
            .and_then(Node::as_f64)
            .unwrap_or(0.5);
        let cast_shadows = root
            .get("castShadows")
            .and_then(Node::as_bool)
            .unwrap_or(true);
        let receive_shadows = root
            .get("receiveShadows")
            .and_then(Node::as_bool)
            .unwrap_or(true);

        for d in &mut diags {
            if d.document.is_none() {
                d.document = document.path.clone();
            }
        }
        Material {
            document,
            shader,
            parameters,
            textures,
            alpha_mode,
            alpha_cutoff,
            cull,
            cast_shadows,
            receive_shadows,
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
                out.push(anchor(
                    Diagnostic::error(
                        Code::MATERIAL_UNKNOWN_SHADER,
                        format!("unknown shader {:?}", self.shader),
                    )
                    .with_suggestion(format!("known shaders: {}", shaders.ids().join(", "))),
                    "/shader",
                ));
            }
            return finish(out, &self.document);
        };

        for p in &self.parameters {
            let Some(spec) = manifest.parameter(&p.name) else {
                out.push(anchor(
                    Diagnostic::error(
                        Code::MATERIAL_UNKNOWN_PARAMETER,
                        format!("{} has no parameter {:?}", manifest.shader_id, p.name),
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
                        format!("{} has no texture slot {:?}", manifest.shader_id, t.slot),
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
                        format!("{} requires texture {:?}", manifest.shader_id, slot.name),
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

        if !manifest.has_pass(self.alpha_mode.pass()) {
            out.push(anchor(
                Diagnostic::error(
                    Code::MATERIAL_UNSUPPORTED_ALPHA_MODE,
                    format!(
                        "{} has no {} pass",
                        manifest.shader_id,
                        self.alpha_mode.name()
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
                if root.get("alphaMode").is_some() {
                    "/alphaMode"
                } else {
                    ""
                },
            ));
        }
        finish(out, &self.document)
    }

    /// Every parameter and slot of the manifest with the material's value
    /// or the manifest default: what a renderer consumes. Values that do
    /// not fit the manifest fall back to the default (validation reports
    /// them). `None` when the shader is unknown.
    pub fn resolve(&self, shaders: &ShaderLibrary) -> Option<ResolvedMaterial> {
        let manifest = shaders.get(&self.shader)?;
        let parameters = manifest
            .parameters
            .iter()
            .map(|spec| {
                let authored = self
                    .parameters
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
                let binding = self.binding(&slot.name).cloned();
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
            pass: self.alpha_mode.pass(),
            parameters,
            textures,
            alpha_mode: self.alpha_mode,
            alpha_cutoff: self.alpha_cutoff,
            cull: self.cull,
            cast_shadows: self.cast_shadows,
            receive_shadows: self.receive_shadows,
        })
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
    pub parameters: Vec<ResolvedParameter>,
    pub textures: Vec<ResolvedTexture>,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f64,
    pub cull: CullMode,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
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
            "alphaMode": self.alpha_mode.name(),
            "alphaCutoff": self.alpha_cutoff,
            "cull": self.cull.name(),
            "castShadows": self.cast_shadows,
            "receiveShadows": self.receive_shadows,
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
            "{} ({})\n  shader {}  pass {}  alpha {}{}  cull {}  shadows cast={} receive={}\n",
            self.name,
            self.id,
            self.shader_id,
            self.pass.name(),
            self.alpha_mode.name(),
            if self.alpha_mode == AlphaMode::Mask {
                format!(" cutoff {}", self.alpha_cutoff)
            } else {
                String::new()
            },
            self.cull.name(),
            self.cast_shadows,
            self.receive_shadows
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
            ShaderSource::Unknown => serde_json::Value::Null,
        },
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

fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    match root.pointer_span(pointer) {
        Some(s) => d.at(s.start.line, s.start.column),
        None => d,
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
