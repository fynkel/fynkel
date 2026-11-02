//! The plain types the C functions pass: vectors, colours, transforms,
//! components, quality, materials and mesh data.

use fynkel_render::{Quality, QualityLevel, ShadowFilter, ShadowLevel};
use fynkel_world::{MAX_SHADER_TEXTURES, MeshRenderer};

use crate::error::{Error, FyStatus};

use super::*;

// --- Plain types ---------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyVec2 {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A rotation, as a unit quaternion.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyQuat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// sRGB, 0–1 per channel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyTransform {
    pub position: FyVec3,
    pub rotation: FyQuat,
    pub scale: FyVec3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyMeshRenderer {
    /// 0 for none.
    pub mesh: u64,
    /// 0 for none.
    pub material: u64,
    /// Whether `color` replaces the material's base colour for this object.
    pub own_color: bool,
    /// The object's own base colour (sRGB, as a material's), with
    /// `own_color`.
    pub color: FyColor,
    /// Whether it blocks lights that cast shadows.
    pub cast_shadows: bool,
    /// Whether shadows fall on it.
    pub receive_shadows: bool,
    /// Numbers of its own for its material's shader: its `PerObject`, in
    /// the order the shader lists them.
    pub own: [f32; 4],
}

/// `Default` is the engine's: no mesh or material, casting and receiving
/// shadows.
impl Default for FyMeshRenderer {
    fn default() -> Self {
        Self::from_component(MeshRenderer::default())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyCamera {
    /// Vertical field of view in degrees, 1–179.
    pub fov_y: f32,
    /// Not `near`: Windows headers define `near` and `far` as macros.
    pub near_clip: f32,
    pub far_clip: f32,
    pub clear_color: FyColor,
    /// Parallel projection; `fov_y` is then unused.
    pub orthographic: bool,
    /// Half the view's height in metres when orthographic.
    pub size: f32,
    /// The layers it draws: bit n for layer n. `0xFFFFFFFF` for all.
    pub culling_mask: u32,
    /// Shows the world's sky (`fy_world_sky_set`) behind everything;
    /// without it, `clear_color`.
    pub show_sky: bool,
    /// Light brighter than white glows into what's around it, where the
    /// world's quality draws bloom: how much (0 none, 1 about what it gives off) and
    /// from how bright (1: only what's brighter than white).
    pub bloom: bool,
    pub bloom_strength: f32,
    pub bloom_threshold: f32,
    /// Surfaces darken where they meet, where the world's quality draws
    /// ambient occlusion: how dark (0 to 4)
    /// and how far, in metres.
    pub ambient_occlusion: bool,
    pub ambient_occlusion_intensity: f32,
    pub ambient_occlusion_radius: f32,
    /// The pixel look: the scene drawn this many pixels high, then
    /// enlarged to the view with hard edges. 0 for the view's own size; up
    /// to 16384.
    pub pixels: u32,
}

/// `FyLight::kind`.
pub const FY_LIGHT_DIRECTIONAL: u32 = 0;
/// `FyLight::kind`.
pub const FY_LIGHT_AMBIENT: u32 = 1;
/// `FyLight::kind`.
pub const FY_LIGHT_POINT: u32 = 2;
/// `FyLight::kind`.
pub const FY_LIGHT_SPOT: u32 = 3;

/// A quality level: the device's own (`fy_quality_auto`).
pub const FY_QUALITY_AUTO: u32 = 0;
/// A quality level: the cheapest, for phones (Mali, Adreno) and slow or
/// unknown GPUs.
pub const FY_QUALITY_LOW: u32 = 1;
/// A quality level: between, for a phone that keeps up with it.
pub const FY_QUALITY_MEDIUM: u32 = 2;
/// A quality level: Apple's GPUs and desktops'.
pub const FY_QUALITY_HIGH: u32 = 3;

/// `FyQuality::shadow_softness`: one sample, sharp.
pub const FY_SHADOWS_HARD: u32 = 0;
/// `FyQuality::shadow_softness`: 4 samples, across about 2 texels.
pub const FY_SHADOWS_SOFT: u32 = 1;
/// `FyQuality::shadow_softness`: 9 samples, across about 3 texels.
pub const FY_SHADOWS_SOFTER: u32 = 2;

/// What a world's views draw, and how finely (`fy_world_quality_set`). A
/// level's, to start from: `fy_quality_preset`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyQuality {
    /// The scene drawn into a half-float image, with a last pass after it,
    /// rather than straight to the screen. Bloom draws into it either way.
    pub hdr: bool,
    /// Cameras' bloom is drawn.
    pub bloom: bool,
    /// Cameras' ambient occlusion is drawn.
    pub ambient_occlusion: bool,
    /// Point and spot lights give shiny surfaces a highlight.
    pub lamp_highlights: bool,
    /// Lights' shadows are drawn; without, none are, and the rest is
    /// unused.
    pub shadows: bool,
    /// The sun's shadow maps, nearest the camera first: 1 to 3.
    pub shadow_cascades: u32,
    /// Each of the sun's maps' side, in texels: a power of two, 256 to 4096.
    pub shadow_resolution: u32,
    /// `FY_SHADOWS_*`.
    pub shadow_softness: u32,
    /// How far from the camera the sun's shadows reach, in metres.
    pub shadow_distance: f32,
    /// The most spot lights casting shadows in a frame, the nearest the
    /// camera: 0 to 4.
    pub spot_shadows: u32,
    /// Each spot light's map's side, as `shadow_resolution`.
    pub spot_shadow_resolution: u32,
    /// Samples a pixel is drawn with, smoothing edges (MSAA): 1, 2 or 4.
    /// A device that can't draw a count draws the next below it.
    pub samples: u32,
    /// Particles with softness fade where they meet what's behind them;
    /// without, they're hard.
    pub soft_particles: bool,
}

impl FyQuality {
    pub(super) fn from_quality(q: Quality) -> Self {
        let s = q.shadows.or(Quality::LOW.shadows).expect("low has shadows");
        Self {
            hdr: q.hdr,
            bloom: q.bloom,
            ambient_occlusion: q.ambient_occlusion,
            lamp_highlights: q.lamp_highlights,
            shadows: q.shadows.is_some(),
            shadow_cascades: s.cascades as u32,
            shadow_resolution: s.map_size,
            shadow_softness: match s.filter {
                ShadowFilter::Hard => FY_SHADOWS_HARD,
                ShadowFilter::Soft => FY_SHADOWS_SOFT,
                ShadowFilter::Softer => FY_SHADOWS_SOFTER,
            },
            shadow_distance: s.distance,
            spot_shadows: s.spot_maps as u32,
            spot_shadow_resolution: s.spot_map_size,
            samples: q.samples,
            soft_particles: q.soft_particles,
        }
    }

    pub(super) fn to_quality(self) -> Result<Quality, Error> {
        let filter = match self.shadow_softness {
            FY_SHADOWS_HARD => ShadowFilter::Hard,
            FY_SHADOWS_SOFT => ShadowFilter::Soft,
            FY_SHADOWS_SOFTER => ShadowFilter::Softer,
            other => {
                return Err(Error::new(FyStatus::InvalidValue, format!("{other} isn't a shadow softness")));
            }
        };
        let quality = Quality {
            hdr: self.hdr,
            bloom: self.bloom,
            ambient_occlusion: self.ambient_occlusion,
            lamp_highlights: self.lamp_highlights,
            shadows: self.shadows.then_some(ShadowLevel {
                cascades: self.shadow_cascades as usize,
                map_size: self.shadow_resolution,
                filter,
                spot_maps: self.spot_shadows as usize,
                spot_map_size: self.spot_shadow_resolution,
                distance: self.shadow_distance,
            }),
            samples: self.samples,
            soft_particles: self.soft_particles,
        };
        match quality.invalid() {
            Some(why) => Err(Error::new(FyStatus::InvalidValue, format!("quality: {why}"))),
            None => Ok(quality),
        }
    }
}

pub(super) fn level_named(level: u32) -> Option<QualityLevel> {
    match level {
        FY_QUALITY_LOW => Some(QualityLevel::Low),
        FY_QUALITY_MEDIUM => Some(QualityLevel::Medium),
        FY_QUALITY_HIGH => Some(QualityLevel::High),
        _ => None,
    }
}

pub(super) fn level_of(level: QualityLevel) -> u32 {
    match level {
        QualityLevel::Low => FY_QUALITY_LOW,
        QualityLevel::Medium => FY_QUALITY_MEDIUM,
        QualityLevel::High => FY_QUALITY_HIGH,
    }
}

/// `Default` is the engine's default light: a white sun of intensity 1,
/// with point and spot settings ready for a change of kind.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyLight {
    /// `FY_LIGHT_DIRECTIONAL`, `FY_LIGHT_AMBIENT`, `FY_LIGHT_POINT` or
    /// `FY_LIGHT_SPOT`.
    pub kind: u32,
    pub color: FyColor,
    pub intensity: f32,
    /// Point and spot lights: how far the light reaches, in metres.
    pub range: f32,
    /// Spot lights: the cone's full angle, in degrees.
    pub spot_angle: f32,
    /// Spot lights: how much of the cone fades, from its edge in, 0 to 1.
    pub softness: f32,
    /// Directional and spot lights: whether it casts shadows.
    pub shadows: bool,
}

/// `FyMaterial::blend`: covers what's behind.
pub const FY_BLEND_OPAQUE: u32 = 0;
/// Laid over what's behind by its alpha (glTF's BLEND).
pub const FY_BLEND_TRANSPARENT: u32 = 1;
/// Adds its light, times its alpha, to what's behind.
pub const FY_BLEND_ADDITIVE: u32 = 2;

/// A metallic-roughness material (glTF's). Each texture multiplies its
/// factor; 0 is no texture. `Default` is the engine's default material,
/// not zeros (a UV scale of 0 would show one texel).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyMaterial {
    /// sRGB. Its alpha is how opaque it is: it matters for cut-outs and
    /// for surfaces that blend.
    pub base_color: FyColor,
    /// 0–1.
    pub metallic: f32,
    /// 0–1.
    pub roughness: f32,
    /// sRGB; alpha is ignored.
    pub emissive: FyColor,
    /// 0 or more.
    pub emissive_intensity: f32,
    /// Multiplies texture coordinates, per axis; -1e4 to 1e4.
    pub uv_scale: FyVec2,
    /// Added to texture coordinates after the scale; -1e4 to 1e4.
    pub uv_offset: FyVec2,
    /// 0 for opaque; otherwise the alpha below which the surface is cut
    /// away. 0–1.
    pub alpha_cutoff: f32,
    /// Both sides of each triangle are drawn.
    pub double_sided: bool,
    /// `FY_BLEND_*`: how the surface covers what's behind it.
    pub blend: u32,
    /// Read as sRGB.
    pub base_color_texture: u64,
    /// Tangent space, glTF's convention. Linear.
    pub normal_texture: u64,
    /// Roughness in G, metallic in B. Linear.
    pub metallic_roughness_texture: u64,
    /// In R. Linear.
    pub occlusion_texture: u64,
    /// Read as sRGB.
    pub emissive_texture: u64,
    /// The material's own shader (`fy_shader_add`), or 0 for the built-in
    /// one. Its values are set with `fy_material_values_set`.
    pub shader: u64,
    /// The textures its shader declares, in their order; 0 for none.
    pub shader_textures: [u64; 4],
    /// How far, in metres, its shader's `vertex` can move a vertex: the
    /// object's bounds grow by it for culling. 0 to 1e4.
    pub reach: f32,
    /// How wide a line drawn around it is, in metres; 0 for none. 0 to
    /// 100. Opaque surfaces only.
    pub outline_width: f32,
    /// sRGB; alpha is ignored.
    pub outline_color: FyColor,
}

// The renderer's limit, the world's and the C struct's agree.
const _: () = assert!(MAX_SHADER_TEXTURES == 4 && fynkel_render::MAX_TEXTURES == 4);

/// `fy_mesh_new_shape`'s shape: a 1×1×1 cube.
pub const FY_MESH_CUBE: u32 = 0;
/// `fy_mesh_new_shape`'s shape: a sphere of radius 0.5.
pub const FY_MESH_SPHERE: u32 = 1;
/// `fy_mesh_new_shape`'s shape: a 1×1 plane facing +Y.
pub const FY_MESH_PLANE: u32 = 2;
/// `fy_mesh_new_shape`'s shape: a capsule along Y, 2 high, of radius 0.5.
pub const FY_MESH_CAPSULE: u32 = 3;
/// `fy_mesh_new_shape`'s shape: a cylinder along Y, 2 high, of radius 0.5.
pub const FY_MESH_CYLINDER: u32 = 4;
/// `fy_mesh_new_shape`'s shape: a 1×1 quad in XY facing +Z.
pub const FY_MESH_QUAD: u32 = 5;

/// A mesh made from code, for `fy_mesh_new`. Each array holds
/// `vertex_count` items of packed floats: 3 per position and normal, 4 per
/// tangent (xyz, then w = 1 or -1), 2 per UV; or 4 bytes per colour (sRGB
/// RGBA). Normals, tangents, UVs and colours may be null: normals and
/// tangents are then computed (see `fy_mesh_new`), UVs are (0, 0) and
/// colours white.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FyMeshData {
    pub positions: *const f32,
    pub normals: *const f32,
    pub tangents: *const f32,
    pub uvs: *const f32,
    pub colors: *const u8,
    pub vertex_count: usize,
    /// Three per triangle, counter-clockwise seen from the front: every
    /// submesh's, one after another.
    pub indices: *const u32,
    pub index_count: usize,
    /// Where each submesh after the first starts in `indices`, in order,
    /// each a multiple of 3; null with 0 for a mesh that doesn't split.
    pub submesh_starts: *const u32,
    pub submesh_start_count: usize,
}

/// `fy_mesh_recalculate`'s normals.
pub const FY_MESH_NORMALS: u32 = 1;
/// `fy_mesh_recalculate`'s tangents.
pub const FY_MESH_TANGENTS: u32 = 2;
/// `fy_mesh_recalculate`'s normals, but later: by whoever reads them
/// next, as each view uploading it.
pub const FY_MESH_NORMALS_LATER: u32 = 4;
