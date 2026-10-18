//! Materials: shared data that objects point to by id. Meshes are
//! `fynkel_mesh::MeshData` and textures `fynkel_load::ImageData`, shared
//! the same way.

use crate::describe::{Describe, Field, FieldKind, Value};
use glam::Vec2;

use crate::{Color, ShaderId, TextureId};

/// How many textures of its own a material's shader can read.
pub const MAX_SHADER_TEXTURES: usize = 4;

/// A metallic-roughness PBR material: glTF's, so a loaded model's
/// materials are these. Each texture multiplies its factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// The surface colour (sRGB). Its alpha is how opaque it is: it
    /// matters for cut-outs and for surfaces that blend.
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
    /// Texture coordinates are multiplied by this, per axis: 10 repeats the
    /// textures ten times across a mesh with 0–1 UVs.
    pub uv_scale: Vec2,
    /// Then this is added, so a part of a texture (an atlas) can fill a
    /// mesh: glTF's texture transform, without its rotation.
    pub uv_offset: Vec2,
    /// 0 for an opaque surface. Otherwise a cut-out (glTF's MASK): the
    /// surface is there only where the base colour's alpha (times the
    /// texture's) reaches this.
    pub alpha_cutoff: f32,
    /// Both sides of each triangle are drawn, the back lit as the front
    /// (glTF's `doubleSided`): for leaves, paper, flags. Otherwise a
    /// triangle seen from behind isn't drawn.
    pub double_sided: bool,
    /// Whether what's behind shows through, and how (glTF's BLEND is
    /// [`Blend::Transparent`]).
    pub blend: Blend,
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
    /// The material's own shader (stage 10), or `None` for the built-in
    /// one. Its values are the world's, by the material
    /// (`World::set_material_values`).
    pub shader: Option<ShaderId>,
    /// The textures the shader declares, in the order it declares them.
    /// Past its count they're unused.
    pub shader_textures: [Option<TextureId>; MAX_SHADER_TEXTURES],
    /// How far, in metres, its shader's `vertex` can move a vertex from
    /// where the mesh has it: the object's bounds grow by this much, so
    /// it isn't culled while what moved is still in view.
    pub reach: f32,
}

impl Material {
    pub const DEFAULT: Self = Self {
        base_color: Color::WHITE,
        metallic: 0.0,
        roughness: 0.5,
        emissive: Color::BLACK,
        emissive_intensity: 1.0,
        uv_scale: Vec2::ONE,
        uv_offset: Vec2::ZERO,
        alpha_cutoff: 0.0,
        double_sided: false,
        blend: Blend::Opaque,
        base_color_texture: None,
        normal_texture: None,
        metallic_roughness_texture: None,
        occlusion_texture: None,
        emissive_texture: None,
        shader: None,
        shader_textures: [None; MAX_SHADER_TEXTURES],
        reach: 0.0,
    };

    pub const fn color(base_color: Color) -> Self {
        Self { base_color, ..Self::DEFAULT }
    }

    /// Every built-in texture slot, `None` where it's empty.
    pub fn textures(&self) -> [Option<TextureId>; 5] {
        [
            self.base_color_texture,
            self.normal_texture,
            self.metallic_roughness_texture,
            self.occlusion_texture,
            self.emissive_texture,
        ]
    }

    /// Whether it reads `texture`, in a built-in slot or its shader's.
    pub fn uses_texture(&self, texture: TextureId) -> bool {
        self.textures().contains(&Some(texture)) || self.shader_textures.contains(&Some(texture))
    }
}

/// How a [`Material`]'s surface covers what's behind it. See-through
/// surfaces are drawn after the solid ones, the furthest object first;
/// they cast no shadow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Blend {
    /// Covers it (or, with an alpha cutoff, where the alpha reaches it).
    #[default]
    Opaque,
    /// Laid over it by the alpha, as glass or water is.
    Transparent,
    /// Adds its light, times the alpha, as a glow does.
    Additive,
}

impl Blend {
    const NAMES: &'static [&'static str] = &["Opaque", "Transparent", "Additive"];
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
            // Negative flips the texture; 0 stretches one texel over all.
            Field::new("uv_scale", FieldKind::Vec2, Value::Vec2(D.uv_scale)).range(-1e4, 1e4),
            Field::new("uv_offset", FieldKind::Vec2, Value::Vec2(D.uv_offset)).range(-1e4, 1e4),
            Field::new("alpha_cutoff", FieldKind::F32, Value::F32(D.alpha_cutoff)).range(0.0, 1.0),
            Field::new("double_sided", FieldKind::Bool, Value::Bool(D.double_sided)),
            Field::new("base_color_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("normal_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("metallic_roughness_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("occlusion_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("emissive_texture", FieldKind::Texture, Value::Texture(None)),
            Field::new("blend", FieldKind::Enum(Blend::NAMES), Value::Enum(D.blend as u32)),
            Field::new("shader_texture_0", FieldKind::Texture, Value::Texture(None)),
            Field::new("shader_texture_1", FieldKind::Texture, Value::Texture(None)),
            Field::new("shader_texture_2", FieldKind::Texture, Value::Texture(None)),
            Field::new("shader_texture_3", FieldKind::Texture, Value::Texture(None)),
            Field::new("reach", FieldKind::F32, Value::F32(D.reach)).range(0.0, 1e4),
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
            5 => Value::Vec2(self.uv_scale),
            6 => Value::Vec2(self.uv_offset),
            7 => Value::F32(self.alpha_cutoff),
            8 => Value::Bool(self.double_sided),
            9..=13 => Value::Texture(self.textures()[index - 9]),
            14 => Value::Enum(self.blend as u32),
            15..=18 => Value::Texture(self.shader_textures[index - 15]),
            19 => Value::F32(self.reach),
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
            (5, Value::Vec2(v)) => self.uv_scale = v,
            (6, Value::Vec2(v)) => self.uv_offset = v,
            (7, Value::F32(v)) => self.alpha_cutoff = v,
            (8, Value::Bool(b)) => self.double_sided = b,
            (9, Value::Texture(t)) => self.base_color_texture = t,
            (10, Value::Texture(t)) => self.normal_texture = t,
            (11, Value::Texture(t)) => self.metallic_roughness_texture = t,
            (12, Value::Texture(t)) => self.occlusion_texture = t,
            (13, Value::Texture(t)) => self.emissive_texture = t,
            (14, Value::Enum(0)) => self.blend = Blend::Opaque,
            (14, Value::Enum(1)) => self.blend = Blend::Transparent,
            (14, Value::Enum(2)) => self.blend = Blend::Additive,
            (15..=18, Value::Texture(t)) => self.shader_textures[index - 15] = t,
            (19, Value::F32(v)) => self.reach = v,
            _ => unreachable!("validated by Describe::set"),
        }
    }
}

/// A material's own shader as the world keeps it (stage 10): what the
/// world checks its materials against, and the compiled shader, which only
/// the renderer looks into.
#[derive(Clone)]
pub struct Shader {
    /// Its file's name, for messages.
    pub name: String,
    /// How many bytes its `Values` take: each material using it has this
    /// many.
    pub values_size: usize,
    /// How many textures of its own it reads.
    pub textures: usize,
    /// The compiled shader. The renderer's own type: the world only hands
    /// it on.
    pub compiled: std::sync::Arc<dyn std::any::Any + Send + Sync>,
}

impl std::fmt::Debug for Shader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shader")
            .field("name", &self.name)
            .field("values_size", &self.values_size)
            .field("textures", &self.textures)
            .finish_non_exhaustive()
    }
}
