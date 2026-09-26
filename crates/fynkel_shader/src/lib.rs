//! `.shader` files (ADR-0031).
//!
//! A shader is one text file: a short header that declares the shader's
//! name, lighting, alpha modes and **properties**, then plain WGSL that
//! defines `surface()` (and optionally `vertex()`) against the
//! [`CONTRACT`]. The engine writes everything else: the property struct,
//! the texture bindings, the light code and the entry points.
//!
//! ```text
//! shader "Examples/Water" {
//!     lighting lit
//!     alpha opaque, blend
//!     properties {
//!         deep      color = (0.02, 0.10, 0.20, 1)  "Colour of deep water"
//!         roughness float = 0.1  range(0, 1)
//!         ripples   texture = normal
//!     }
//! }
//!
//! fn surface(in: SurfaceInput) -> Surface { … }
//! ```
//!
//! This crate is the one place that knows what a shader is: [`parse`]
//! reads the header into an [`Interface`], [`Layout`] places the
//! properties in the uniform block, [`contract::prelude`] writes the WGSL
//! the body is compiled against, and [`check`] compiles the whole with
//! naga — every error at its line and column **in the `.shader` file**.
//! The cooker, the renderer and `fynkel shader validate` all go through
//! here, so they cannot disagree.
//!
//! Diagnostics: `FYN-D0315` the header does not read (syntax, an unknown
//! word, a duplicate or badly named property, a limit), `FYN-D0310` a
//! default not of its type or outside its range, `FYN-D0311` `min > max`,
//! `FYN-D0313` the WGSL does not compile, `FYN-D0314` `surface`/`vertex`
//! missing or with the wrong signature.

#![allow(clippy::result_large_err)] // `Diagnostic` is the workspace's error type

pub mod check;
pub mod contract;
mod header;
pub mod layout;

pub use check::{assemble, check, Assembled};
pub use contract::CONTRACT;
pub use header::parse;
pub use layout::{Field, Layout};

/// At most this many texture properties: group 1 then holds one uniform,
/// eight textures and eight samplers — under Metal's sixteen samplers a
/// stage on the phone.
pub const MAX_TEXTURES: usize = 8;

/// The property block's largest size in bytes.
pub const MAX_BLOCK_BYTES: u32 = 256;

/// `lit` runs the engine's PBR light over the surface; `unlit` writes its
/// colour as the engine's unlit path does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lighting {
    Lit,
    Unlit,
}

impl Lighting {
    pub const fn name(self) -> &'static str {
        match self {
            Lighting::Lit => "lit",
            Lighting::Unlit => "unlit",
        }
    }
}

/// The alpha modes a material of this shader may choose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}

impl AlphaMode {
    pub const ALL: [AlphaMode; 3] = [AlphaMode::Opaque, AlphaMode::Mask, AlphaMode::Blend];

    pub const fn name(self) -> &'static str {
        match self {
            AlphaMode::Opaque => "opaque",
            AlphaMode::Mask => "mask",
            AlphaMode::Blend => "blend",
        }
    }

    pub fn parse(s: &str) -> Option<AlphaMode> {
        AlphaMode::ALL.into_iter().find(|m| m.name() == s)
    }
}

/// A value property's type. `color` is RGBA, `color3` RGB; both are
/// linear and shown with the colour picker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueType {
    Float,
    Int,
    Bool,
    Vec2,
    Vec3,
    Vec4,
    Color3,
    Color,
}

impl ValueType {
    pub const ALL: [ValueType; 8] = [
        ValueType::Float,
        ValueType::Int,
        ValueType::Bool,
        ValueType::Vec2,
        ValueType::Vec3,
        ValueType::Vec4,
        ValueType::Color3,
        ValueType::Color,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ValueType::Float => "float",
            ValueType::Int => "int",
            ValueType::Bool => "bool",
            ValueType::Vec2 => "vec2",
            ValueType::Vec3 => "vec3",
            ValueType::Vec4 => "vec4",
            ValueType::Color3 => "color3",
            ValueType::Color => "color",
        }
    }

    pub fn parse(s: &str) -> Option<ValueType> {
        ValueType::ALL.into_iter().find(|t| t.name() == s)
    }

    /// How many numbers the value has.
    pub const fn components(self) -> usize {
        match self {
            ValueType::Float | ValueType::Int | ValueType::Bool => 1,
            ValueType::Vec2 => 2,
            ValueType::Vec3 | ValueType::Color3 => 3,
            ValueType::Vec4 | ValueType::Color => 4,
        }
    }

    /// The type the body sees in `material.<name>`.
    pub const fn wgsl(self) -> &'static str {
        match self {
            ValueType::Float => "f32",
            ValueType::Int => "i32",
            ValueType::Bool => "bool",
            ValueType::Vec2 => "vec2<f32>",
            ValueType::Vec3 | ValueType::Color3 => "vec3<f32>",
            ValueType::Vec4 | ValueType::Color => "vec4<f32>",
        }
    }

    /// The value a property takes when the header gives none: zero, false,
    /// or white for the colours (Unity's default colour).
    pub fn zero(self) -> Vec<f64> {
        match self {
            ValueType::Color3 | ValueType::Color => vec![1.0; self.components()],
            _ => vec![0.0; self.components()],
        }
    }
}

/// The texel an unbound texture property samples.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fallback {
    White,
    Black,
    Normal,
    Gray,
}

impl Fallback {
    pub const ALL: [Fallback; 4] = [
        Fallback::White,
        Fallback::Black,
        Fallback::Normal,
        Fallback::Gray,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Fallback::White => "white",
            Fallback::Black => "black",
            Fallback::Normal => "normal",
            Fallback::Gray => "gray",
        }
    }

    pub fn parse(s: &str) -> Option<Fallback> {
        Fallback::ALL.into_iter().find(|f| f.name() == s)
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
}

#[derive(Clone, PartialEq, Debug)]
pub enum PropertyKind {
    /// A number, flag, vector or colour: a field of the uniform block.
    Value {
        ty: ValueType,
        /// `ty.components()` numbers (a bool is 0 or 1).
        default: Vec<f64>,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// A texture slot: `sample_<name>(uv)` in the body.
    Texture {
        fallback: Fallback,
        color_space: ColorSpace,
        required: bool,
    },
}

/// One line of the header's `properties { … }`.
#[derive(Clone, PartialEq, Debug)]
pub struct Property {
    pub name: String,
    pub kind: PropertyKind,
    pub tooltip: Option<String>,
    /// Where the property's name is in the file (1-based).
    pub line: u32,
    pub column: u32,
}

impl Property {
    pub fn is_texture(&self) -> bool {
        matches!(self.kind, PropertyKind::Texture { .. })
    }

    /// The Inspector's label, Unity's nicified name: `baseColor` and
    /// `base_color` both read "Base Color".
    pub fn label(&self) -> String {
        nicify(&self.name)
    }
}

/// What a shader offers its materials — the header, read.
#[derive(Clone, PartialEq, Debug)]
pub struct Interface {
    /// The menu path in the material's Shader dropdown (`Examples/Water`).
    pub name: String,
    pub lighting: Lighting,
    /// In the order written; never empty.
    pub alpha: Vec<AlphaMode>,
    /// Whether its (non-blend) materials cast the sun's shadow.
    pub shadows: bool,
    pub properties: Vec<Property>,
}

impl Interface {
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }

    /// The texture properties, in file order — slot `i` is bound at
    /// `@binding(1 + 2i)` (texture) and `@binding(2 + 2i)` (sampler).
    pub fn textures(&self) -> impl Iterator<Item = &Property> {
        self.properties.iter().filter(|p| p.is_texture())
    }

    pub fn allows(&self, mode: AlphaMode) -> bool {
        self.alpha.contains(&mode)
    }
}

/// A parsed `.shader` file.
#[derive(Clone, PartialEq, Debug)]
pub struct Shader {
    /// The path given to [`parse`], for diagnostics.
    pub path: Option<String>,
    pub interface: Interface,
    pub layout: Layout,
    /// The whole file as read.
    pub text: String,
    /// Byte offset where the body (the WGSL after the header) starts.
    pub body_start: usize,
}

impl Shader {
    /// The WGSL after the header.
    pub fn body(&self) -> &str {
        &self.text[self.body_start..]
    }
}

/// `baseColor`, `base_color` → "Base Color".
pub fn nicify(name: &str) -> String {
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in name.chars() {
        if c == '_' {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            prev = Some(c);
            continue;
        }
        let boundary = match prev {
            Some(p) => {
                (c.is_uppercase() && (p.is_lowercase() || p.is_ascii_digit()))
                    || (c.is_ascii_digit() && p.is_alphabetic())
            }
            None => false,
        };
        if boundary && !out.ends_with(' ') {
            out.push(' ');
        }
        if out.is_empty() || out.ends_with(' ') {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        prev = Some(c);
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_nicified_as_unity_does() {
        assert_eq!(nicify("baseColor"), "Base Color");
        assert_eq!(nicify("base_color"), "Base Color");
        assert_eq!(nicify("roughness"), "Roughness");
        assert_eq!(nicify("uv2Scale"), "Uv 2 Scale");
        assert_eq!(nicify("MainTex"), "Main Tex");
    }
}
