//! Materials: shared data that objects point to by id. Meshes are
//! `fynkel_mesh::MeshData` and textures `fynkel_load::ImageData`, shared
//! the same way.

use crate::describe::{Describe, Field, FieldKind, Value};
use crate::{Color, TextureId};

/// A metallic-roughness PBR material: glTF's, so a loaded model's
/// materials are these. Each texture multiplies its factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// The surface colour (sRGB). Its alpha matters only for cut-outs.
    pub base_color: Color,
    /// 0 = dielectric (plastic, wood), 1 = metal.
    pub metallic: f32,
    /// 0 = mirror-smooth, 1 = fully rough.
    pub roughness: f32,
    /// Light the surface gives off, unlit by anything (sRGB; black for
    /// none). Alpha is ignored.
    pub emissive: Color,
    /// Multiplies `emissive`, as a light's intensity does its colour.
    pub emissive_intensity: f32,
    /// Texture coordinates are multiplied by this: 10 repeats the textures
    /// ten times across a mesh with 0–1 UVs.
    pub uv_scale: f32,
    /// 0 for an opaque surface. Otherwise a cut-out (glTF's MASK): the
    /// surface is there only where the base colour's alpha (times the
    /// texture's) reaches this.
    pub alpha_cutoff: f32,
    /// Read as sRGB, like `base_color`.
    pub base_color_texture: Option<TextureId>,
    /// A tangent-space normal map (glTF's convention: +X along the image's
    /// right, +Y up it). Read as linear data.
    pub normal_texture: Option<TextureId>,
    /// Roughness in green, metallic in blue (glTF's packing). Linear.
    pub metallic_roughness_texture: Option<TextureId>,
    /// Ambient occlusion in red. Linear. Darkens only the ambient light.
    pub occlusion_texture: Option<TextureId>,
    /// Read as sRGB, like `emissive`.
    pub emissive_texture: Option<TextureId>,
}

impl Material {
    pub const DEFAULT: Self = Self {
        base_color: Color::WHITE,
        metallic: 0.0,
        roughness: 0.5,
        emissive: Color::BLACK,
        emissive_intensity: 1.0,
        uv_scale: 1.0,
        alpha_cutoff: 0.0,
        base_color_texture: None,
        normal_texture: None,
        metallic_roughness_texture: None,
        occlusion_texture: None,
        emissive_texture: None,
    };

    pub const fn color(base_color: Color) -> Self {
        Self { base_color, ..Self::DEFAULT }
    }

    /// Every texture slot, `None` where it's empty.
    pub fn textures(&self) -> [Option<TextureId>; 5] {
        [
            self.base_color_texture,
            self.normal_texture,
            self.metallic_roughness_texture,
            self.occlusion_texture,
            self.emissive_texture,
        ]
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Describe for Material {
    const NAME: &'static str = "Material";

    fn fields() -> &'static [Field] {
        const D: Material = Material::DEFAULT;
        const FIELDS: &[Field] = &[
            Field::new("base_color", FieldKind::Color, Value::Color(D.base_color)),
            Field::new("metallic", FieldKind::F32, Value::F32(D.metallic)).range(0.0, 1.0),
            Field::new("roughness", FieldKind::F32, Value::F32(D.roughness)).range(0.0, 1.0),
            Field::new("emissive", FieldKind::Color, Value::Color(D.emissive)),
            Field::new("emissive_intensity", FieldKind::F32, Value::F32(D.emissive_intensity)).range(0.0, 1e6),
            Field::new("uv_scale", FieldKind::F32, Value::F32(D.uv_scale)).range(1e-4, 1e4),
            Field::new("alpha_cutoff", FieldKind::F32, Value::F32(D.alpha_cutoff)).range(0.0, 1.0),
            Field::new("base_color_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("normal_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("metallic_roughness_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("occlusion_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("emissive_texture", FieldKind::Texture, Value::Texture(None)),
        ];
        FIELDS
    }

    fn get(&self, index: usize) -> Option<Value> {
        Some(match index {
            0 => Value::Color(self.base_color),
            1 => Value::F32(self.metallic),
            2 => Value::F32(self.roughness),
            3 => Value::Color(self.emissive),
            4 => Value::F32(self.emissive_intensity),
            5 => Value::F32(self.uv_scale),
            6 => Value::F32(self.alpha_cutoff),
            7..=11 => Value::Texture(self.textures()[index - 7]),
            _ => return None,
        })
    }

    fn write(&mut self, index: usize, value: Value) {
        match (index, value) {
            (0, Value::Color(c)) => self.base_color = c,
            (1, Value::F32(v)) => self.metallic = v,
            (2, Value::F32(v)) => self.roughness = v,
            (3, Value::Color(c)) => self.emissive = c,
            (4, Value::F32(v)) => self.emissive_intensity = v,
            (5, Value::F32(v)) => self.uv_scale = v,
            (6, Value::F32(v)) => self.alpha_cutoff = v,
            (7, Value::Texture(t)) => self.base_color_texture = t,
            (8, Value::Texture(t)) => self.normal_texture = t,
            (9, Value::Texture(t)) => self.metallic_roughness_texture = t,
            (10, Value::Texture(t)) => self.occlusion_texture = t,
            (11, Value::Texture(t)) => self.emissive_texture = t,
            _ => unreachable!("validated by Describe::set"),
        }
    }
}
