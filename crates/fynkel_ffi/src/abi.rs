//! The C functions (`fy_*`) and the plain types they pass (stage 1 plan
//! §3.4). `include/fynkel.h` is generated from this file by cbindgen.
//!
//! Every function that can fail returns an [`FyStatus`] and writes its
//! results through out-pointers. Ids are `uint64_t`, with 0 meaning "none".
//! Colours are sRGB, 0–1 per channel. Angles are degrees.
//!
//! Handles: an `FyWorld` lives until `fy_world_free`; an `FyView` until
//! `fy_view_free`. A view keeps its world's memory alive, so they can be
//! freed in either order. Passing a handle after freeing it is undefined
//! behaviour; `package:fynkel` makes that impossible from Dart.

use std::ffi::{c_char, c_void};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use fynkel_present::{Headless, HeadlessFrames, ReadError};
use fynkel_render::{Fog, Gpu, Quality, QualityLevel, Renderer, ShadowFilter, ShadowLevel, Sky, SkyImage, SkyKind};

use crate::device::{Device, WorldGpu};
use fynkel_world::{
    AnimationEvent, Blend, Burst, BySpeed, Camera, Color, Component, HdrData, ImageData, ImageError, InheritMode,
    InheritVelocity, Light, LightKind, LimitVelocity, MAX_SHADER_TEXTURES, Material, MaterialId, MeshData, MeshId,
    MeshParts, MeshRenderer, ModelData, ObjectId, ParticleCollision, ParticleCommand, ParticleCurve, ParticleEvent,
    ParticleEventKind, ParticleNoise, ParticleRange, ParticleSettings, Quat, Ray, Raycaster, RenderMode, Shader,
    ShaderId, Shape, ShapeKind, Space, StopAction, SubEmitter, SubEmitterKind, TABLE, TextureId, TextureSheet,
    TrailSettings, Transform, Vec2, Vec3, VectorOverLifetime, World,
};

use crate::error::{Error, FyStatus, guard, last_error};
use crate::extract::Extract;
use crate::worker::Worker;

/// Bumped on any change to the functions or types in this file.
pub const FY_ABI_VERSION: u32 = 44;

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
    fn from_quality(q: Quality) -> Self {
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

    fn to_quality(self) -> Result<Quality, Error> {
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

fn level_named(level: u32) -> Option<QualityLevel> {
    match level {
        FY_QUALITY_LOW => Some(QualityLevel::Low),
        FY_QUALITY_MEDIUM => Some(QualityLevel::Medium),
        FY_QUALITY_HIGH => Some(QualityLevel::High),
        _ => None,
    }
}

fn level_of(level: QualityLevel) -> u32 {
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

// --- Conversions ---------------------------------------------------------------

impl From<FyVec2> for Vec2 {
    fn from(v: FyVec2) -> Self {
        Vec2::new(v.x, v.y)
    }
}

impl From<Vec2> for FyVec2 {
    fn from(v: Vec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl From<FyVec3> for Vec3 {
    fn from(v: FyVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

impl From<Vec3> for FyVec3 {
    fn from(v: Vec3) -> Self {
        Self { x: v.x, y: v.y, z: v.z }
    }
}

impl From<FyQuat> for Quat {
    fn from(q: FyQuat) -> Self {
        Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }
}

impl From<Quat> for FyQuat {
    fn from(q: Quat) -> Self {
        Self { x: q.x, y: q.y, z: q.z, w: q.w }
    }
}

impl From<FyColor> for Color {
    fn from(c: FyColor) -> Self {
        Color::rgba(c.r, c.g, c.b, c.a)
    }
}

impl From<Color> for FyColor {
    fn from(c: Color) -> Self {
        Self { r: c.r, g: c.g, b: c.b, a: c.a }
    }
}

/// A C struct that stands for a component.
trait CComponent: Copy {
    type Component: Component;
    fn to_component(self) -> Result<Self::Component, Error>;
    fn from_component(c: Self::Component) -> Self;
}

impl CComponent for FyTransform {
    type Component = Transform;
    fn to_component(self) -> Result<Transform, Error> {
        Ok(Transform { position: self.position.into(), rotation: self.rotation.into(), scale: self.scale.into() })
    }
    fn from_component(t: Transform) -> Self {
        Self { position: t.position.into(), rotation: t.rotation.into(), scale: t.scale.into() }
    }
}

impl CComponent for FyMeshRenderer {
    type Component = MeshRenderer;
    fn to_component(self) -> Result<MeshRenderer, Error> {
        Ok(MeshRenderer {
            mesh: MeshId::from_raw(self.mesh),
            material: MaterialId::from_raw(self.material),
            own_color: self.own_color,
            color: self.color.into(),
            cast_shadows: self.cast_shadows,
            receive_shadows: self.receive_shadows,
            own: self.own,
        })
    }
    fn from_component(r: MeshRenderer) -> Self {
        Self {
            mesh: r.mesh.map_or(0, MeshId::raw),
            material: r.material.map_or(0, MaterialId::raw),
            own_color: r.own_color,
            color: r.color.into(),
            cast_shadows: r.cast_shadows,
            receive_shadows: r.receive_shadows,
            own: r.own,
        }
    }
}

impl CComponent for FyCamera {
    type Component = Camera;
    fn to_component(self) -> Result<Camera, Error> {
        Ok(Camera {
            fov_y: self.fov_y,
            near: self.near_clip,
            far: self.far_clip,
            clear_color: self.clear_color.into(),
            orthographic: self.orthographic,
            size: self.size,
            culling_mask: self.culling_mask,
            show_sky: self.show_sky,
            bloom: self.bloom,
            bloom_strength: self.bloom_strength,
            bloom_threshold: self.bloom_threshold,
            ambient_occlusion: self.ambient_occlusion,
            ambient_occlusion_intensity: self.ambient_occlusion_intensity,
            ambient_occlusion_radius: self.ambient_occlusion_radius,
            pixels: self.pixels,
        })
    }
    fn from_component(c: Camera) -> Self {
        Self {
            fov_y: c.fov_y,
            near_clip: c.near,
            far_clip: c.far,
            clear_color: c.clear_color.into(),
            orthographic: c.orthographic,
            size: c.size,
            culling_mask: c.culling_mask,
            show_sky: c.show_sky,
            bloom: c.bloom,
            bloom_strength: c.bloom_strength,
            bloom_threshold: c.bloom_threshold,
            ambient_occlusion: c.ambient_occlusion,
            ambient_occlusion_intensity: c.ambient_occlusion_intensity,
            ambient_occlusion_radius: c.ambient_occlusion_radius,
            pixels: c.pixels,
        }
    }
}

impl CComponent for FyLight {
    type Component = Light;
    fn to_component(self) -> Result<Light, Error> {
        let kind = match self.kind {
            FY_LIGHT_DIRECTIONAL => LightKind::Directional,
            FY_LIGHT_AMBIENT => LightKind::Ambient,
            FY_LIGHT_POINT => LightKind::Point,
            FY_LIGHT_SPOT => LightKind::Spot,
            other => {
                return Err(Error::new(
                    FyStatus::InvalidValue,
                    format!("Light.kind {other} doesn't exist (0 = directional, 1 = ambient, 2 = point, 3 = spot)"),
                ));
            }
        };
        Ok(Light {
            kind,
            color: self.color.into(),
            intensity: self.intensity,
            range: self.range,
            spot_angle: self.spot_angle,
            softness: self.softness,
            shadows: self.shadows,
        })
    }
    fn from_component(l: Light) -> Self {
        let kind = match l.kind {
            LightKind::Directional => FY_LIGHT_DIRECTIONAL,
            LightKind::Ambient => FY_LIGHT_AMBIENT,
            LightKind::Point => FY_LIGHT_POINT,
            LightKind::Spot => FY_LIGHT_SPOT,
        };
        Self {
            kind,
            color: l.color.into(),
            intensity: l.intensity,
            range: l.range,
            spot_angle: l.spot_angle,
            softness: l.softness,
            shadows: l.shadows,
        }
    }
}

impl Default for FyLight {
    fn default() -> Self {
        Self::from_component(Light::DEFAULT)
    }
}

impl Default for FyMaterial {
    fn default() -> Self {
        Material::DEFAULT.into()
    }
}

impl FyMaterial {
    /// The world's material, or what's wrong with this one.
    fn to_material(self) -> Result<Material, Error> {
        let blend = match self.blend {
            FY_BLEND_OPAQUE => Blend::Opaque,
            FY_BLEND_TRANSPARENT => Blend::Transparent,
            FY_BLEND_ADDITIVE => Blend::Additive,
            other => return Err(Error::new(FyStatus::InvalidValue, format!("{other} isn't a kind of blend"))),
        };
        Ok(Material {
            base_color: self.base_color.into(),
            metallic: self.metallic,
            roughness: self.roughness,
            emissive: self.emissive.into(),
            emissive_intensity: self.emissive_intensity,
            uv_scale: self.uv_scale.into(),
            uv_offset: self.uv_offset.into(),
            alpha_cutoff: self.alpha_cutoff,
            double_sided: self.double_sided,
            blend,
            base_color_texture: TextureId::from_raw(self.base_color_texture),
            normal_texture: TextureId::from_raw(self.normal_texture),
            metallic_roughness_texture: TextureId::from_raw(self.metallic_roughness_texture),
            occlusion_texture: TextureId::from_raw(self.occlusion_texture),
            emissive_texture: TextureId::from_raw(self.emissive_texture),
            shader: ShaderId::from_raw(self.shader),
            shader_textures: self.shader_textures.map(TextureId::from_raw),
            reach: self.reach,
            outline_width: self.outline_width,
            outline_color: self.outline_color.into(),
        })
    }
}

impl From<Material> for FyMaterial {
    fn from(m: Material) -> Self {
        let raw = |id: Option<TextureId>| id.map_or(0, TextureId::raw);
        Self {
            base_color: m.base_color.into(),
            metallic: m.metallic,
            roughness: m.roughness,
            emissive: m.emissive.into(),
            emissive_intensity: m.emissive_intensity,
            uv_scale: m.uv_scale.into(),
            uv_offset: m.uv_offset.into(),
            alpha_cutoff: m.alpha_cutoff,
            double_sided: m.double_sided,
            blend: match m.blend {
                Blend::Opaque => FY_BLEND_OPAQUE,
                Blend::Transparent => FY_BLEND_TRANSPARENT,
                Blend::Additive => FY_BLEND_ADDITIVE,
            },
            base_color_texture: raw(m.base_color_texture),
            normal_texture: raw(m.normal_texture),
            metallic_roughness_texture: raw(m.metallic_roughness_texture),
            occlusion_texture: raw(m.occlusion_texture),
            emissive_texture: raw(m.emissive_texture),
            shader: m.shader.map_or(0, ShaderId::raw),
            shader_textures: m.shader_textures.map(raw),
            reach: m.reach,
            outline_width: m.outline_width,
            outline_color: m.outline_color.into(),
        }
    }
}

// --- Handles -----------------------------------------------------------------

/// A world. Opaque to C.
#[derive(Debug)]
pub struct FyWorld {
    world: Mutex<World>,
    /// Kept between `fy_raycast` calls so they don't allocate. Locked
    /// only while `world` is.
    raycaster: Mutex<Raycaster>,
    /// Its meshes and textures on the GPU, shared by its views: on the
    /// device its views draw on, made by the first. Held by the views, so
    /// they're freed with the last one, as a view's own were.
    gpu: Mutex<Weak<WorldGpu>>,
    /// The camera shake: which camera, and how far it's moved when drawn.
    shake: Mutex<Option<(ObjectId, Vec3)>>,
    /// The quality its views draw at, or `None` for the device's level's.
    quality: Mutex<Option<Quality>>,
    /// Its sky and fog, as its views draw them.
    sky: Mutex<Option<Sky>>,
    fog: Mutex<Option<Fog>>,
    /// What the last `fy_world_animate` reported, for
    /// `fy_animation_events_get`. Locked only while `world` is.
    events: Mutex<Vec<AnimationEvent>>,
    /// What the last `fy_world_particles_step` reported, for
    /// `fy_particle_events_get`. Locked only while `world` is.
    particle_events: Mutex<Vec<ParticleEvent>>,
}

impl FyWorld {
    /// The world, unless an earlier call panicked while changing it.
    fn lock(&self) -> Result<MutexGuard<'_, World>, Error> {
        self.world.lock().map_err(|_| {
            Error::new(
                FyStatus::Poisoned,
                "an earlier call panicked while changing this world; it can't be used any more",
            )
        })
    }
}

/// A view: a world drawn into a target by its own render worker, on the
/// process's GPU device, with the meshes and textures the world's other
/// views have too. Opaque to C.
#[derive(Debug)]
pub struct FyView {
    world: Arc<FyWorld>,
    gpu: Arc<WorldGpu>,
    /// The frame clock's side: the extract and the frame numbers.
    clock: Mutex<Clock>,
    worker: Worker,
    output: Output,
}

/// Where a view's frames can be had.
#[derive(Debug)]
enum Output {
    Headless(HeadlessFrames),
    #[cfg(target_vendor = "apple")]
    Apple(fynkel_present::ApplePresenter),
    #[cfg(target_vendor = "apple")]
    AppleLayer(fynkel_present::LayerPresenter),
    #[cfg(target_os = "android")]
    Android(fynkel_present::AndroidPresenter),
}

impl FyView {
    /// The platform side of an Android view, for the plugin's JNI calls.
    #[cfg(target_os = "android")]
    pub(crate) fn android(&self) -> Option<&fynkel_present::AndroidPresenter> {
        match &self.output {
            Output::Android(presenter) => Some(presenter),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct Clock {
    extract: Extract,
    last_number: u64,
}

/// `p` as a reference, or an error when it's null.
///
/// # Safety
///
/// `p` is null or points at a live `T`.
unsafe fn handle<'a, T>(p: *const T, what: &str) -> Result<&'a T, Error> {
    // SAFETY: the caller promises p is null or live.
    unsafe { p.as_ref() }.ok_or_else(|| Error::null(what))
}

/// Writes `value` through `out`, or fails when it's null.
///
/// # Safety
///
/// `out` is null or valid for a write of `T`.
unsafe fn write<T>(out: *mut T, value: T, what: &str) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: not null, and the caller promises it's valid for writes.
    unsafe { out.write(value) };
    Ok(())
}

/// Reads `*p`, or fails when it's null.
///
/// # Safety
///
/// `p` is null or valid for a read of `T`.
unsafe fn read<T: Copy>(p: *const T, what: &str) -> Result<T, Error> {
    // SAFETY: the caller promises p is null or valid.
    unsafe { p.as_ref() }.copied().ok_or_else(|| Error::null(what))
}

fn object(id: u64) -> Result<ObjectId, Error> {
    ObjectId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoObject, "0 is not an object id"))
}

fn mesh(id: u64) -> Result<MeshId, Error> {
    MeshId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoMesh, "0 is not a mesh id"))
}

fn material(id: u64) -> Result<MaterialId, Error> {
    MaterialId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoMaterial, "0 is not a material id"))
}

fn texture(id: u64) -> Result<TextureId, Error> {
    TextureId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoTexture, "0 is not a texture id"))
}

fn shader(id: u64) -> Result<ShaderId, Error> {
    ShaderId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoShader, "0 is not a shader id"))
}

// --- Library -----------------------------------------------------------------

/// [`FY_ABI_VERSION`]. `package:fynkel` checks it when it loads.
#[unsafe(no_mangle)]
pub extern "C" fn fy_abi_version() -> u32 {
    FY_ABI_VERSION
}

/// The message of the last failed call on this thread, as UTF-8 ("" before
/// any). Valid until the next failed call on this thread.
#[unsafe(no_mangle)]
pub extern "C" fn fy_last_error() -> *const c_char {
    last_error()
}

// --- Worlds and objects ------------------------------------------------------

/// A new, empty world. Free it with `fy_world_free`.
#[unsafe(no_mangle)]
pub extern "C" fn fy_world_new() -> *mut FyWorld {
    let world = FyWorld {
        world: Mutex::new(World::new()),
        raycaster: Mutex::default(),
        gpu: Mutex::default(),
        shake: Mutex::default(),
        quality: Mutex::default(),
        sky: Mutex::default(),
        fog: Mutex::default(),
        events: Mutex::default(),
        particle_events: Mutex::default(),
    };
    Arc::into_raw(Arc::new(world)).cast_mut()
}

/// Frees a world. Views of it keep working until they are freed too. Null
/// is ignored.
///
/// # Safety
///
/// `world` is null or a live world, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_free(world: *mut FyWorld) {
    if !world.is_null() {
        // A panic while dropping (in a part's own drop) must not unwind
        // into the caller, which aborts: what's left leaks instead, and the
        // message is the last error.
        let _ = guard(|| {
            // SAFETY: made by Arc::into_raw in fy_world_new; the caller
            // gives up its reference.
            drop(unsafe { Arc::from_raw(world) });
            Ok(())
        });
    }
}

/// Spawns an object with no components and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_spawn(world: *const FyWorld, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let id = world.lock()?.spawn();
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Despawns an object. Its id is never used again.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_despawn(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.despawn(object(id)?)?)
    })
}

/// Whether the object exists. False for a null world.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_exists(world: *const FyWorld, id: u64) -> bool {
    let mut exists = false;
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        exists = ObjectId::from_raw(id).is_some_and(|id| world.lock().is_ok_and(|w| w.contains(id)));
        Ok(())
    });
    exists
}

// --- Hierarchy ---------------------------------------------------------------

/// Copies `items` into `buf` when they fit in `cap`, and writes how many
/// there are to `out_count` either way, so a caller whose buffer was too
/// small can retry with one big enough. `buf` may be null when `cap` is 0.
///
/// # Safety
///
/// `buf` is null or valid for `cap` writes of `T`; `out_count` is null or
/// writable.
unsafe fn write_all<T>(
    items: impl ExactSizeIterator<Item = T>,
    buf: *mut T,
    cap: usize,
    out_count: *mut usize,
) -> Result<(), Error> {
    let count = items.len();
    if count <= cap && count > 0 {
        if buf.is_null() {
            return Err(Error::null("buf"));
        }
        for (i, item) in items.enumerate() {
            // SAFETY: not null, and i < count <= cap, which the caller
            // promises are writable.
            unsafe { buf.add(i).write(item) };
        }
    }
    // SAFETY: per the caller's contract.
    unsafe { write(out_count, count, "out_count") }
}

/// A UTF-8 string from `ptr` and `len` bytes. `ptr` may be null when `len`
/// is 0.
///
/// # Safety
///
/// `ptr` is null or valid for `len` reads.
unsafe fn utf8<'a>(ptr: *const u8, len: usize, what: &str) -> Result<&'a str, Error> {
    if len == 0 {
        return Ok("");
    }
    if ptr.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: not null, and the caller promises len readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).map_err(|e| Error::new(FyStatus::InvalidValue, format!("{what} is not UTF-8: {e}")))
}

/// Makes `child` a child of `parent`, or a root when `parent` is 0. Its
/// `Transform` is kept, so it now counts from the new parent. A parent that
/// is `child` or below it is refused with `FY_STATUS_INVALID_VALUE`.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_parent_set(world: *const FyWorld, child: u64, parent: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let parent = if parent == 0 { None } else { Some(object(parent)?) };
        Ok(world.lock()?.set_parent(object(child)?, parent)?)
    })
}

/// Writes the object's parent to `out`, or 0 for a root.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_parent_get(world: *const FyWorld, id: u64, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let parent = world.lock()?.parent(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, parent.map_or(0, ObjectId::raw), "out") }
    })
}

/// The object's children, in the order they were added. Writes how many
/// there are to `out_count`, and copies them into `buf` only when they all
/// fit in `cap`.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_children_get(
    world: *const FyWorld,
    id: u64,
    buf: *mut u64,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let world = world.lock()?;
        let children = world.children(object(id)?)?.iter().map(|c| c.raw());
        // SAFETY: per this function's contract.
        unsafe { write_all(children, buf, cap, out_count) }
    })
}

/// Writes to `out` whether `id` is `root` or below it.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_is_part_of(world: *const FyWorld, id: u64, root: u64, out: *mut bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let part = world.lock()?.is_part_of(object(id)?, object(root)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, part, "out") }
    })
}

/// Turns the object on (`true`) or off. An object that is off hides itself
/// and everything below it: they aren't drawn, don't light or show the
/// scene, and aren't hit by rays. Its children keep their own flags.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_active_set(world: *const FyWorld, id: u64, active: bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_active(object(id)?, active)?)
    })
}

/// Writes the object's own flag to `out`, whatever its parents' are.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_active_get(world: *const FyWorld, id: u64, out: *mut bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let active = world.lock()?.active(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, active, "out") }
    })
}

/// Puts the object on `layer`, 0 to 31 (`FY_STATUS_INVALID_VALUE` past
/// that). Cameras draw, and rays hit, only the layers in their masks. Each
/// object has its own: a child doesn't take its parent's.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_layer_set(world: *const FyWorld, id: u64, layer: u32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_layer(object(id)?, layer)?)
    })
}

/// Writes the object's layer to `out`: 0 unless put on another.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_layer_get(world: *const FyWorld, id: u64, out: *mut u32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let layer = world.lock()?.layer(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, layer, "out") }
    })
}

/// Writes to `out` whether the object is shown: it and every parent above
/// it on.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_active_in_hierarchy_get(world: *const FyWorld, id: u64, out: *mut bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let shown = world.lock()?.active_in_hierarchy(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, shown, "out") }
    })
}

/// Writes to `out` the camera a view of the world draws from: the shown
/// object with the lowest id that has both a camera and a transform, or 0
/// when there is none.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_active(world: *const FyWorld, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let camera = world.lock()?.active_camera();
        // SAFETY: per this function's contract.
        unsafe { write(out, camera.map_or(0, ObjectId::raw), "out") }
    })
}

/// Shakes camera `camera` by `offset` (in the world, in metres) when a view
/// draws from it, without moving the object: what the world's camera
/// shake is. 0 for `camera` (or a null `offset`) stops it. Taken by each
/// `fy_view_frame` after.
///
/// # Safety
///
/// `world` is null or live; `offset` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_shake_set(world: *const FyWorld, camera: u64, offset: *const FyVec3) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract: null or readable.
        let offset = unsafe { offset.as_ref() }.copied();
        let shake = match (ObjectId::from_raw(camera), offset) {
            (Some(id), Some(by)) => {
                let by = Vec3::from(by);
                if !by.is_finite() {
                    return Err(Error::new(FyStatus::InvalidValue, format!("a shake of {by} isn't finite")));
                }
                Some((id, by))
            }
            _ => None,
        };
        *world.shake.lock().unwrap_or_else(PoisonError::into_inner) = shake;
        Ok(())
    })
}

/// A screen effect, for `fy_camera_effects_set`: a shader the world has
/// (`fy_shader_add`) with `fn effect`, `values_len` bytes of its values,
/// laid out as its `Values` (`fy_shader_describe`), and its textures in
/// the order it declares them, 0 for none (it reads white).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FyScreenEffect {
    pub shader: u64,
    pub values: *const u8,
    pub values_len: usize,
    pub textures: [u64; 4],
}

/// Gives the camera on `object` these `count` screen effects, in place of
/// any it had (0 takes them away): run over its picture in order, each
/// over what the one before made, when a view draws from it. A shader that
/// isn't an effect's is `FY_STATUS_INVALID_VALUE`; values that aren't as
/// many bytes as its shader's are `FY_STATUS_WRONG_SIZE`; a texture the
/// world hasn't, `FY_STATUS_NO_TEXTURE`. A shader or texture can't be
/// removed while an effect uses it.
///
/// # Safety
///
/// `world` is null or live; `effects` is null or valid for `count` reads
/// (null when `count` is 0), and each one's `values` for `values_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_effects_set(
    world: *const FyWorld,
    object: u64,
    effects: *const FyScreenEffect,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let effects: &[FyScreenEffect] = if count == 0 {
            &[]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(effects, "effects")?, count) }
        };
        let mut list = Vec::with_capacity(effects.len());
        for e in effects {
            let values = if e.values_len == 0 {
                Vec::new()
            } else {
                // SAFETY: per this function's contract.
                unsafe { std::slice::from_raw_parts(handle(e.values, "values")?, e.values_len) }.to_vec()
            };
            let mut textures = [None; MAX_SHADER_TEXTURES];
            for (slot, &id) in textures.iter_mut().zip(&e.textures) {
                *slot = if id == 0 { None } else { Some(texture(id)?) };
            }
            list.push(fynkel_world::ScreenEffect { shader: shader(e.shader)?, values, textures });
        }
        Ok(world.lock()?.set_camera_effects(self::object(object)?, list)?)
    })
}

/// Sets what the world's views draw, and how finely, or the device's
/// level's (`fy_quality_auto`) when `quality` is null, as a new world has.
/// Taken by each `fy_view_frame` after.
///
/// # Safety
///
/// `world` is null or live; `quality` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_quality_set(world: *const FyWorld, quality: *const FyQuality) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let quality = unsafe { quality.as_ref() }.map(|q| q.to_quality()).transpose()?;
        *world.quality.lock().unwrap_or_else(PoisonError::into_inner) = quality;
        Ok(())
    })
}

/// Writes a level's quality to `out`: `level` is `FY_QUALITY_LOW`,
/// `_MEDIUM` or `_HIGH`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_quality_preset(level: u32, out: *mut FyQuality) -> FyStatus {
    guard(|| {
        let level =
            level_named(level).ok_or_else(|| Error::new(FyStatus::InvalidValue, format!("{level} isn't a level")))?;
        // SAFETY: per this function's contract.
        unsafe { write(out, FyQuality::from_quality(level.quality()), "out") }
    })
}

/// Writes to `out` how many pipelines of materials' own shaders are being
/// made beside the frames: a view on screen draws without a shader's
/// objects while the driver compiles it, the first time it's drawn on the
/// device, rather than holding a frame up. 0 when every shader drawn so
/// far is ready, as a loading screen waits for. Makes the device if
/// there's none yet.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shaders_waiting(out: *mut u32) -> FyStatus {
    guard(|| {
        let device = Device::current().map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { write(out, device.kit.shaders_waiting(), "out") }
    })
}

/// Writes the level this device picks for a world without a quality of
/// its own to `out`: `FY_QUALITY_LOW`, `_MEDIUM` or `_HIGH`. Makes the
/// device if there's none yet.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_quality_auto(out: *mut u32) -> FyStatus {
    guard(|| {
        let device = Device::current().map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { write(out, level_of(device.kit.device_quality()), "out") }
    })
}

// --- Sky and fog --------------------------------------------------------------

/// `FySky::kind`: none; the camera's clear colour shows.
pub const FY_SKY_NONE: u32 = 0;
/// `FySky::kind`: one colour, `horizon`, seen and lighting as ambient light.
pub const FY_SKY_COLOR: u32 = 1;
/// `FySky::kind`: `zenith` straight up, `horizon` level, `ground` straight
/// down, shown as picked.
pub const FY_SKY_GRADIENT: u32 = 2;
/// `FySky::kind`: `image`, an equirectangular `.hdr`, tone mapped as the
/// scene is.
pub const FY_SKY_IMAGE: u32 = 3;

/// The world's sky. A gradient or an image lights the scene, a shiny
/// surface mirroring it and a rough one taking its colour; a plain colour
/// adds to the ambient light.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FySky {
    /// A `FY_SKY_*`.
    pub kind: u32,
    pub zenith: FyColor,
    pub horizon: FyColor,
    pub ground: FyColor,
    /// `FY_SKY_IMAGE`'s; the world takes a share of it.
    pub image: *const FySkyImage,
    /// What the image's light is multiplied by.
    pub intensity: f32,
    /// How far the image is turned about the vertical, in degrees: positive
    /// turns it to the right.
    pub rotation: f32,
    /// What the light the sky gives is multiplied by: 0 for none. What's
    /// seen of it is unchanged.
    pub lighting: f32,
}

/// The world's fog: what's further than `start` fades into `color`, all of
/// it by `end`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyFog {
    pub color: FyColor,
    /// Whether `color` is used: without it, the fog is the sky's horizon.
    pub use_color: bool,
    /// Metres from the camera.
    pub start: f32,
    pub end: f32,
    /// The fog thins with height, halving every `height` metres above
    /// y = 0: 0 for as thick everywhere.
    pub height: f32,
}

/// A sky's image on its own, before any world has it. Opaque to C. Made
/// with `fy_sky_image_decode` on any thread, freed with
/// `fy_sky_image_free`. Worlds that use it share it.
#[derive(Debug)]
pub struct FySkyImage(Arc<SkyImage>);

/// Decodes a Radiance `.hdr` (equirectangular: −Z the middle, straight up
/// the top row) of `len` bytes into a sky image handle and writes it to
/// `out`. Anything else, or one over 8192 pixels on a side, is refused
/// (`FY_STATUS_INVALID_VALUE`) with a message saying why.
///
/// Decoding takes tens of milliseconds: call it off the UI thread.
///
/// # Safety
///
/// `bytes` is valid for `len` bytes (it may be null when `len` is 0);
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_sky_image_decode(bytes: *const u8, len: usize, out: *mut *mut FySkyImage) -> FyStatus {
    guard(|| {
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        if out.is_null() {
            return Err(Error::null("out"));
        }
        let invalid = |message: String| Error::new(FyStatus::InvalidValue, message);
        let hdr = HdrData::decode(bytes).map_err(|e| invalid(e.to_string()))?;
        let image = SkyImage::from_rgb(hdr.width(), hdr.height(), hdr.rgb()).map_err(invalid)?;
        // SAFETY: checked non-null; the caller promises it's writable.
        unsafe { out.write(Box::into_raw(Box::new(FySkyImage(Arc::new(image))))) };
        Ok(())
    })
}

/// Frees a sky image handle. Worlds that use it keep their share. Null is
/// ignored.
///
/// # Safety
///
/// `image` is null or live, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_sky_image_free(image: *mut FySkyImage) {
    if !image.is_null() {
        // A panic here leaks the image rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in fy_sky_image_decode; the
            // caller gives it up.
            drop(unsafe { Box::from_raw(image) });
            Ok(())
        });
    }
}

/// Writes the sky image's width and height in pixels.
///
/// # Safety
///
/// `image` is null or live; the outs are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_sky_image_size(
    image: *const FySkyImage,
    out_width: *mut u32,
    out_height: *mut u32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let image = unsafe { handle(image, "image") }?;
        // SAFETY: per this function's contract.
        unsafe {
            write(out_width, image.0.width(), "out_width")?;
            write(out_height, image.0.height(), "out_height")
        }
    })
}

fn linear(c: FyColor) -> Vec3 {
    let [r, g, b, _] = Color::from(c).to_linear();
    Vec3::new(r, g, b)
}

/// Checks `value` is finite and at least 0.
fn not_negative(value: f32, what: &str) -> Result<f32, Error> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(Error::new(FyStatus::InvalidValue, format!("{what} {value} must be 0 or more")))
    }
}

/// Sets the world's sky, or clears it when `sky` is null or its kind is
/// `FY_SKY_NONE`. Taken by each `fy_view_frame` after; a new gradient or
/// image costs its views a few milliseconds of GPU work, once.
///
/// # Safety
///
/// `world` is null or live; `sky` is null or readable, its `image` null or
/// live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_sky_set(world: *const FyWorld, sky: *const FySky) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let sky = match unsafe { sky.as_ref() } {
            None => None,
            Some(s) if s.kind == FY_SKY_NONE => None,
            Some(s) => {
                let kind = match s.kind {
                    FY_SKY_COLOR => SkyKind::Color(linear(s.horizon)),
                    FY_SKY_GRADIENT => SkyKind::Gradient {
                        zenith: linear(s.zenith),
                        horizon: linear(s.horizon),
                        ground: linear(s.ground),
                    },
                    FY_SKY_IMAGE => {
                        // SAFETY: per this function's contract.
                        let image = unsafe { handle(s.image, "sky.image") }?;
                        if !s.rotation.is_finite() {
                            return Err(Error::new(FyStatus::InvalidValue, "sky.rotation must be finite"));
                        }
                        SkyKind::Image {
                            image: Arc::clone(&image.0),
                            intensity: not_negative(s.intensity, "sky.intensity")?,
                            rotation: s.rotation.to_radians(),
                        }
                    }
                    other => {
                        return Err(Error::new(FyStatus::InvalidValue, format!("{other} isn't a kind of sky")));
                    }
                };
                Some(Sky { kind, lighting: not_negative(s.lighting, "sky.lighting")? })
            }
        };
        *world.sky.lock().unwrap_or_else(PoisonError::into_inner) = sky;
        Ok(())
    })
}

/// Sets the world's fog, or clears it when `fog` is null. Taken by each
/// `fy_view_frame` after.
///
/// # Safety
///
/// `world` is null or live; `fog` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_fog_set(world: *const FyWorld, fog: *const FyFog) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let fog = match unsafe { fog.as_ref() } {
            None => None,
            Some(f) => {
                let start = not_negative(f.start, "fog.start")?;
                if !(f.end.is_finite() && f.end > start) {
                    return Err(Error::new(
                        FyStatus::InvalidValue,
                        format!("fog.end {} must be beyond its start {start}", f.end),
                    ));
                }
                let height = not_negative(f.height, "fog.height")?;
                Some(Fog {
                    color: f.use_color.then(|| linear(f.color)),
                    start,
                    end: f.end,
                    height: (height > 0.0).then_some(height),
                })
            }
        };
        *world.fog.lock().unwrap_or_else(PoisonError::into_inner) = fog;
        Ok(())
    })
}

/// Names the object with `len` bytes of UTF-8 at `name`, or clears its name
/// when `len` is 0. Names needn't be unique.
///
/// # Safety
///
/// `world` is null or live; `name` is null or valid for `len` reads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_name_set(world: *const FyWorld, id: u64, name: *const u8, len: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, name) = unsafe { (handle(world, "world")?, utf8(name, len, "name")?) };
        Ok(world.lock()?.set_name(object(id)?, Some(name))?)
    })
}

/// The object's name as UTF-8, without a terminating zero. Writes its
/// length in bytes to `out_len` (0 for no name), and copies it into `buf`
/// only when it fits in `cap`.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_name_get(
    world: *const FyWorld,
    id: u64,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let world = world.lock()?;
        let name = world.name(object(id)?)?.unwrap_or("");
        // SAFETY: per this function's contract.
        unsafe { write_all(name.bytes(), buf, cap, out_len) }
    })
}

/// Writes to `out` the first object below `root` named `name` (`len` bytes
/// of UTF-8), depth first in child order, or 0 when there's none. `root`
/// itself isn't searched.
///
/// # Safety
///
/// `world` is null or live; `name` is null or valid for `len` reads; `out`
/// is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_find(
    world: *const FyWorld,
    root: u64,
    name: *const u8,
    len: usize,
    out: *mut u64,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, name) = unsafe { (handle(world, "world")?, utf8(name, len, "name")?) };
        let found = world.lock()?.find(object(root)?, name)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, found.map_or(0, ObjectId::raw), "out") }
    })
}

/// The object's transform in the world: its own after its parents', as
/// Dart's `worldTransform` works it out. The position is exact; the
/// rotation is the rotations multiplied, and the scale the scales (Unity's
/// lossy scale), which is approximate when a parent with an uneven scale
/// has a rotated child, because the result is skewed and no position,
/// rotation and scale can say that. A parent scaled to 0 still gives a
/// rotation. An object without a `Transform` counts as not moved from its
/// parent.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_transform_get(world: *const FyWorld, id: u64, out: *mut FyTransform) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let t = world.lock()?.world_transform(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, FyTransform::from_component(t), "out") }
    })
}

/// The world transforms of `count` objects, `ids[i]`'s into `out[i]`, as
/// `fy_world_transform_get` gives each: what a package that follows many
/// objects (physics' colliders) reads in one call. Fails on the first id
/// that isn't an object, with what came before it written.
///
/// # Safety
///
/// `world` is null or live; `ids` is null or valid for `count` reads, and
/// `out` null or valid for `count` writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_transform_get_many(
    world: *const FyWorld,
    ids: *const u64,
    count: usize,
    out: *mut FyTransform,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        if count == 0 {
            return Ok(());
        }
        if ids.is_null() {
            return Err(Error::null("ids"));
        }
        if out.is_null() {
            return Err(Error::null("out"));
        }
        // SAFETY: not null, and valid for count reads and writes per the contract.
        let (ids, out) =
            unsafe { (std::slice::from_raw_parts(ids, count), std::slice::from_raw_parts_mut(out, count)) };
        let world = world.lock()?;
        for (&id, out) in ids.iter().zip(out) {
            *out = FyTransform::from_component(world.world_transform(object(id)?)?);
        }
        Ok(())
    })
}

// --- Raycasts ----------------------------------------------------------------

/// A half-line from `origin` along `direction`, which needn't be unit
/// length.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyRay {
    pub origin: FyVec3,
    pub direction: FyVec3,
}

/// Where a ray hit first. `object` is 0 when it hit nothing; the rest is
/// then 0 too.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyRayHit {
    pub object: u64,
    pub point: FyVec3,
    /// The face's normal, unit length, on the side the ray came from.
    pub normal: FyVec3,
    /// From the ray's origin to `point`.
    pub distance: f32,
}

/// Casts `ray` at the world as it is now and writes the closest object it
/// hits no further than `max_distance` (which may be infinite) to `out`.
/// What a ray can hit is what a view draws: objects with a `Transform` and
/// a mesh renderer with both a mesh and a material, from the side they're
/// drawn from (both sides for a double-sided material). Cut-outs are hit as
/// if solid. Only objects on a layer in `mask` (bit n for layer n) are hit:
/// `0xFFFFFFFF` for all.
///
/// `FY_STATUS_INVALID_VALUE` for a ray without a direction or with a
/// non-finite origin, and for a negative or NaN `max_distance`.
///
/// # Safety
///
/// `world` is null or live; `ray` is null or readable; `out` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_raycast(
    world: *const FyWorld,
    ray: *const FyRay,
    max_distance: f32,
    mask: u32,
    out: *mut FyRayHit,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (handle, r) = unsafe { (handle(world, "world")?, read(ray, "ray")?) };
        let ray = Ray::new(r.origin.into(), r.direction.into()).ok_or_else(|| {
            Error::new(FyStatus::InvalidValue, "a ray needs a finite origin and a direction that isn't zero")
        })?;
        if max_distance.is_nan() || max_distance < 0.0 {
            return Err(Error::new(FyStatus::InvalidValue, format!("max_distance can't be {max_distance}")));
        }
        let world = handle.lock()?;
        // The raycaster holds only matrices it recomputes, so one left
        // behind by a panic is as good as new.
        let mut caster = handle.raycaster.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let hit = caster.cast(&world, ray, max_distance, mask).map_or_else(FyRayHit::default, |h| FyRayHit {
            object: h.object.raw(),
            point: h.point.into(),
            normal: h.normal.into(),
            distance: h.distance,
        });
        // SAFETY: per this function's contract.
        unsafe { write(out, hit, "out") }
    })
}

// --- Debug drawing -------------------------------------------------------------

/// A line from `a` to `b` in world space.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyDebugLine {
    pub a: FyVec3,
    pub b: FyVec3,
    pub color: FyColor,
}

/// Replaces the lines views draw on top of the scene with these `count`
/// lines, until the next call: 0 lines clears them. A line with a point
/// that isn't finite is left out.
///
/// # Safety
///
/// `world` is null or live; `lines` is null or valid for `count` reads (it
/// may be null when `count` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_debug_lines_set(
    world: *const FyWorld,
    lines: *const FyDebugLine,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let lines: &[FyDebugLine] = if count == 0 {
            &[]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(lines, "lines")?, count) }
        };
        let lines =
            lines.iter().map(|l| fynkel_world::DebugLine { a: l.a.into(), b: l.b.into(), color: l.color.into() });
        world.lock()?.set_debug_lines(lines.filter(|l| l.a.is_finite() && l.b.is_finite()));
        Ok(())
    })
}

// --- Components --------------------------------------------------------------

/// # Safety
///
/// `world` is null or live; `value` is null or readable.
unsafe fn set<C: CComponent>(world: *const FyWorld, id: u64, value: *const C) -> FyStatus {
    guard(|| {
        // SAFETY: per the caller's contract.
        let (world, value) = unsafe { (handle(world, "world")?, read(value, "value")?) };
        let component = value.to_component()?;
        Ok(world.lock()?.insert(object(id)?, component)?)
    })
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
unsafe fn get<C: CComponent>(world: *const FyWorld, id: u64, out: *mut C) -> FyStatus {
    guard(|| {
        // SAFETY: per the caller's contract.
        let world = unsafe { handle(world, "world") }?;
        let component = world.lock()?.get::<C::Component>(object(id)?)?;
        // SAFETY: per the caller's contract.
        unsafe { write(out, C::from_component(component), "out") }
    })
}

/// # Safety
///
/// `world` is null or live.
unsafe fn remove<C: CComponent>(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per the caller's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.remove::<C::Component>(object(id)?)?;
        Ok(())
    })
}

/// Adds or replaces the object's `Transform`. The rotation must be a unit
/// quaternion and every value finite.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_transform_set(world: *const FyWorld, id: u64, value: *const FyTransform) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { set(world, id, value) }
}

/// Sets the transforms of `count` objects at once: `ids[i]` gets
/// `values[i]`. All are checked first, so if one is refused (an object
/// that doesn't exist, a value that isn't finite, a rotation that isn't a
/// unit) none is written. One call for a frame's moves instead of one each.
///
/// # Safety
///
/// `world` is null or live; `ids` and `values` are null or valid for
/// `count` reads (they may be null when `count` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_transform_set_many(
    world: *const FyWorld,
    ids: *const u64,
    values: *const FyTransform,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        if count == 0 {
            return Ok(());
        }
        if ids.is_null() {
            return Err(Error::null("ids"));
        }
        if values.is_null() {
            return Err(Error::null("values"));
        }
        // SAFETY: not null, and valid for count reads per the contract.
        let (ids, values) =
            unsafe { (std::slice::from_raw_parts(ids, count), std::slice::from_raw_parts(values, count)) };
        // 0 isn't an id: refused up front, so the pairs below can't fail.
        if ids.contains(&0) {
            return Err(object(0).unwrap_err());
        }
        let pairs = ids.iter().zip(values).map(|(&id, value)| {
            let transform = value.to_component().expect("any FyTransform converts; the world checks the values");
            (ObjectId::from_raw(id).expect("not 0"), transform)
        });
        Ok(world.lock()?.insert_all(pairs)?)
    })
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_transform_get(world: *const FyWorld, id: u64, out: *mut FyTransform) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { get(world, id, out) }
}

/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_transform_remove(world: *const FyWorld, id: u64) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { remove::<FyTransform>(world, id) }
}

/// Adds or replaces the object's `MeshRenderer`. Its mesh and material must
/// exist (or be 0).
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_renderer_set(
    world: *const FyWorld,
    id: u64,
    value: *const FyMeshRenderer,
) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { set(world, id, value) }
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_renderer_get(world: *const FyWorld, id: u64, out: *mut FyMeshRenderer) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { get(world, id, out) }
}

/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_renderer_remove(world: *const FyWorld, id: u64) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { remove::<FyMeshRenderer>(world, id) }
}

/// Adds or replaces the object's `Camera`.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_set(world: *const FyWorld, id: u64, value: *const FyCamera) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { set(world, id, value) }
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_get(world: *const FyWorld, id: u64, out: *mut FyCamera) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { get(world, id, out) }
}

/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_remove(world: *const FyWorld, id: u64) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { remove::<FyCamera>(world, id) }
}

/// Adds or replaces the object's `Light`.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_light_set(world: *const FyWorld, id: u64, value: *const FyLight) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { set(world, id, value) }
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_light_get(world: *const FyWorld, id: u64, out: *mut FyLight) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { get(world, id, out) }
}

/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_light_remove(world: *const FyWorld, id: u64) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { remove::<FyLight>(world, id) }
}

// --- Meshes and materials ----------------------------------------------------

/// A mesh on its own, before any world has it. Opaque to C. Made with
/// `fy_mesh_new` or `fy_mesh_new_shape` on any thread, freed with
/// `fy_mesh_free`. Worlds that add it share its data, so freeing it
/// doesn't take it from them.
#[derive(Debug)]
pub struct FyMesh(Arc<MeshData>, Vec<Arc<MeshData>>);

/// How many copies a mesh that changes keeps from before, for its next
/// changes to be written into: a world, and frames on their way to the
/// GPU, hold the last few.
const SPARE_MESHES: usize = 4;

impl FyMesh {
    fn new(data: Arc<MeshData>) -> Self {
        FyMesh(data, Vec::new())
    }

    /// Its data, to change. While another holds it (a world, a frame on its
    /// way to the GPU) the data is copied first: into a copy from before
    /// that nothing holds any longer, so a mesh changed each frame
    /// allocates nothing once it has a few, or else into a new one.
    fn own(&mut self) -> &mut MeshData {
        if Arc::get_mut(&mut self.0).is_none() {
            let free = self.1.iter_mut().position(|spare| Arc::get_mut(spare).is_some());
            let next = match free {
                Some(i) => {
                    let mut spare = self.1.swap_remove(i);
                    Arc::get_mut(&mut spare).expect("nothing else holds it").clone_from(&self.0);
                    spare
                }
                None => Arc::new((*self.0).clone()),
            };
            let held = std::mem::replace(&mut self.0, next);
            if self.1.len() == SPARE_MESHES {
                self.1.remove(0);
            }
            self.1.push(held);
        }
        Arc::get_mut(&mut self.0).expect("only this mesh holds it")
    }
}

/// Writes a new mesh handle to `out`.
///
/// # Safety
///
/// `out` is null or writable.
unsafe fn new_mesh(data: MeshData, out: *mut *mut FyMesh) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null("out"));
    }
    // SAFETY: not null, and the caller promises it's writable.
    unsafe { out.write(Box::into_raw(Box::new(FyMesh::new(Arc::new(data))))) };
    Ok(())
}

/// Makes a built-in shape (`FY_MESH_CUBE`, `FY_MESH_SPHERE`,
/// `FY_MESH_PLANE`, `FY_MESH_CAPSULE`, `FY_MESH_CYLINDER` or
/// `FY_MESH_QUAD`) and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_shape(shape: u32, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        let data = match shape {
            FY_MESH_CUBE => MeshData::cube(),
            // 720 triangles, as light as Unity's sphere (768): 2,208 (48 ×
            // 24) held the Note10+ at 23 fps with 1,000 of them.
            FY_MESH_SPHERE => MeshData::sphere(24, 16),
            FY_MESH_PLANE => MeshData::plane(),
            FY_MESH_CAPSULE => MeshData::capsule(24, 6),
            FY_MESH_CYLINDER => MeshData::cylinder(24),
            FY_MESH_QUAD => MeshData::quad(),
            other => {
                return Err(Error::new(
                    FyStatus::InvalidValue,
                    format!(
                        "mesh shape {other} doesn't exist (0 = cube, 1 = sphere, 2 = plane, 3 = capsule, 4 = \
                         cylinder, 5 = quad)"
                    ),
                ));
            }
        };
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// `count` items of `N` values at `p`, or `None` when `p` is null.
///
/// # Safety
///
/// `p` is null or valid for `count * N` reads.
unsafe fn items<'a, T, const N: usize>(p: *const T, count: usize, what: &str) -> Result<Option<&'a [[T; N]]>, Error> {
    if p.is_null() {
        return Ok(None);
    }
    let len = count.checked_mul(N).ok_or_else(|| Error::new(FyStatus::InvalidValue, format!("too many {what}")))?;
    // SAFETY: not null; the caller promises len readable items, and [T; N]
    // has T's alignment.
    let flat = unsafe { std::slice::from_raw_parts(p, len) };
    Ok(Some(flat.as_chunks::<N>().0))
}

/// Makes a mesh from arrays and writes it to `out`. Only positions and
/// indices are required:
/// - Missing normals are the average of the faces around each vertex,
///   weighted by angle, so shared vertices are smooth and sharp corners
///   need a vertex per face.
/// - Missing tangents are generated with MikkTSpace from the UVs; a vertex
///   whose faces need different tangents (a mirrored texture's seam) is
///   split.
/// - Missing UVs are (0, 0).
///
/// A broken mesh (indices out of range, not a multiple of 3, values that
/// aren't finite, arrays of the wrong length) is refused with
/// `FY_STATUS_INVALID_VALUE` and a message that says which item. The arrays
/// are copied and can be freed after the call.
///
/// Generating tangents takes about 1 µs per triangle, so a big mesh with UVs
/// is worth making off the UI thread. This touches no world and can run on
/// any thread.
///
/// # Safety
///
/// `mesh` is null or readable, and its non-null arrays are valid for the
/// counts it gives; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new(mesh: *const FyMeshData, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let m = unsafe { read(mesh, "mesh") }?;
        let n = m.vertex_count;
        // SAFETY: per this function's contract, for each array.
        let parts = unsafe {
            MeshParts {
                positions: items::<f32, 3>(m.positions, n, "positions")?.ok_or_else(|| Error::null("positions"))?,
                normals: items::<f32, 3>(m.normals, n, "normals")?,
                tangents: items::<f32, 4>(m.tangents, n, "tangents")?,
                uvs: items::<f32, 2>(m.uvs, n, "uvs")?,
                colors: items::<u8, 4>(m.colors, n, "colors")?,
                indices: items::<u32, 1>(m.indices, m.index_count, "indices")?
                    .ok_or_else(|| Error::null("indices"))?
                    .as_flattened(),
                submeshes: items::<u32, 1>(m.submesh_starts, m.submesh_start_count, "submesh_starts")?
                    .unwrap_or_default()
                    .as_flattened(),
                skin: None,
            }
        };
        let data = MeshData::new(parts).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a mesh with nothing in it, to change (`fy_mesh_set_positions` and
/// the rest), and writes it to `out`. It draws nothing until it has
/// triangles.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_empty(out: *mut *mut FyMesh) -> FyStatus {
    // SAFETY: per this function's contract.
    guard(|| unsafe { new_mesh(MeshData::empty(), out) })
}

// --- Shapes with settings, outlines, combining (stage 13 phase 2) ------------

/// The most segments a shape takes along any one direction.
const MAX_SEGMENTS: u32 = 1024;

/// `value` if it's finite and more than 0.
fn length(value: f32, what: &str) -> Result<f32, Error> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(Error::new(FyStatus::InvalidValue, format!("{what} is {value}; it must be more than 0")))
    }
}

/// `value` if it's finite and 0 or more.
fn at_least_0(value: f32, what: &str) -> Result<f32, Error> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(Error::new(FyStatus::InvalidValue, format!("{what} is {value}; it must be 0 or more")))
    }
}

/// `n` if it's from `least` to [`MAX_SEGMENTS`].
fn segment_count(n: u32, least: u32, what: &str) -> Result<u32, Error> {
    if (least..=MAX_SEGMENTS).contains(&n) {
        Ok(n)
    } else {
        Err(Error::new(FyStatus::InvalidValue, format!("{what} is {n}; it must be from {least} to {MAX_SEGMENTS}")))
    }
}

/// Makes a box of `size` centred on the origin, each face cut into a grid
/// of `segments_x`, `segments_y` and `segments_z` squares along X, Y and Z
/// (1 or more each), its UVs in metres times `uv_per_metre`, and writes it
/// to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_box(
    size: FyVec3,
    segments_x: u32,
    segments_y: u32,
    segments_z: u32,
    uv_per_metre: f32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let size = Vec3::new(length(size.x, "size.x")?, length(size.y, "size.y")?, length(size.z, "size.z")?);
        let counts = [
            segment_count(segments_x, 1, "segments_x")?,
            segment_count(segments_y, 1, "segments_y")?,
            segment_count(segments_z, 1, "segments_z")?,
        ];
        let data = MeshData::box_shape(size, counts, at_least_0(uv_per_metre, "uv_per_metre")?);
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a flat grid `width` along X by `depth` along Z facing +Y,
/// centred on the origin, cut into `segments_x` by `segments_z` squares,
/// its UVs in metres times `uv_per_metre`, and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_grid(
    width: f32,
    depth: f32,
    segments_x: u32,
    segments_z: u32,
    uv_per_metre: f32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let size = Vec2::new(length(width, "width")?, length(depth, "depth")?);
        let counts = [segment_count(segments_x, 1, "segments_x")?, segment_count(segments_z, 1, "segments_z")?];
        let data = MeshData::grid(size, counts, at_least_0(uv_per_metre, "uv_per_metre")?);
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a sphere of `radius` with `segments` slices around (3 or more)
/// and `rings` bands from pole to pole (2 or more), and writes it to
/// `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_sphere(radius: f32, segments: u32, rings: u32, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        let data = MeshData::sphere_sized(
            length(radius, "radius")?,
            segment_count(segments, 3, "segments")?,
            segment_count(rings, 2, "rings")?,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a capsule along Y, `height` from end to end (less than twice the
/// radius is a sphere) and of `radius`, with `segments` slices around (3
/// or more) and `rings` bands in each round end (1 or more), and writes it
/// to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_capsule(
    radius: f32,
    height: f32,
    segments: u32,
    rings: u32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let data = MeshData::capsule_sized(
            length(radius, "radius")?,
            at_least_0(height, "height")?,
            segment_count(segments, 3, "segments")?,
            segment_count(rings, 1, "rings")?,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a cylinder along Y, `height` high and of `radius`, with
/// `segments` slices around (3 or more), flat ends unless `caps` is
/// false, and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_cylinder(
    radius: f32,
    height: f32,
    segments: u32,
    caps: bool,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let data = MeshData::cylinder_sized(
            length(radius, "radius")?,
            length(height, "height")?,
            segment_count(segments, 3, "segments")?,
            caps,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a cone along Y, `height` high with its point at the top and a
/// base of `radius`, with `segments` slices around (3 or more), a flat
/// base unless `cap` is false, and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_cone(
    radius: f32,
    height: f32,
    segments: u32,
    cap: bool,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let data = MeshData::cone(
            length(radius, "radius")?,
            length(height, "height")?,
            segment_count(segments, 3, "segments")?,
            cap,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a ring around Y, its middle `radius` from the centre and its
/// tube of radius `tube`, with `segments` slices around Y and `sides`
/// around the tube (3 or more each), and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_torus(
    radius: f32,
    tube: f32,
    segments: u32,
    sides: u32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let data = MeshData::torus(
            length(radius, "radius")?,
            length(tube, "tube")?,
            segment_count(segments, 3, "segments")?,
            segment_count(sides, 3, "sides")?,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a flat disc of `radius` facing +Y with `segments` around (3 or
/// more), a ring when `inner` (less than `radius`) is more than 0, and
/// writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_disc(radius: f32, inner: f32, segments: u32, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        let radius = length(radius, "radius")?;
        let inner = at_least_0(inner, "inner")?;
        if inner >= radius {
            return Err(Error::new(FyStatus::InvalidValue, format!("inner is {inner}; it must be less than {radius}")));
        }
        let data = MeshData::disc(radius, inner, segment_count(segments, 3, "segments")?);
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// The loops of `points`: `loop_count` of them, each `loop_sizes[k]`
/// points long, one after another.
///
/// # Safety
///
/// `points` is valid for `point_count` items and `loop_sizes` for
/// `loop_count`.
unsafe fn loops<'a>(
    points: *const FyVec2,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
) -> Result<Vec<&'a [Vec2]>, Error> {
    if points.is_null() {
        return Err(Error::null("points"));
    }
    if loop_sizes.is_null() || loop_count == 0 {
        return Err(Error::null("loop_sizes"));
    }
    // SAFETY: not null, and the caller promises the counts; FyVec2 is two
    // f32s, as Vec2.
    let (points, sizes) = unsafe {
        (
            std::slice::from_raw_parts(points.cast::<Vec2>(), point_count),
            std::slice::from_raw_parts(loop_sizes, loop_count),
        )
    };
    let total: usize = sizes.iter().map(|&n| n as usize).sum();
    if total != point_count {
        return Err(Error::new(
            FyStatus::InvalidValue,
            format!("the loops have {total} points between them, but there are {point_count}"),
        ));
    }
    let mut from = 0;
    Ok(sizes
        .iter()
        .map(|&n| {
            let ring = &points[from..from + n as usize];
            from += n as usize;
            ring
        })
        .collect())
}

const _: () = assert!(std::mem::size_of::<FyVec2>() == std::mem::size_of::<Vec2>());

/// Makes a slab `depth` thick along Z from an outline less its holes, and
/// writes it to `out`. The `points` are the outline's, then each hole's;
/// `loop_sizes` says how many each has, the outline first (each 3 or
/// more). Either way round is fine. The front faces +Z; the ends show the
/// texture flat, `uv_per_metre` times a metre. Corners sharper than 40°
/// are sharp, the rest smooth.
///
/// # Safety
///
/// `points` is valid for `point_count` items and `loop_sizes` for
/// `loop_count`; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_extrude(
    points: *const FyVec2,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
    depth: f32,
    uv_per_metre: f32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let rings = unsafe { loops(points, point_count, loop_sizes, loop_count) }?;
        let data = MeshData::extrude(
            rings[0],
            &rings[1..],
            length(depth, "depth")?,
            at_least_0(uv_per_metre, "uv_per_metre")?,
        )
        .map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a mesh by turning `profile` around Y with `sides` slices (3 or
/// more), as on a lathe, and writes it to `out`. Each point is (distance
/// from Y, height); the outside is on the right of the way the profile
/// goes. Corners sharper than 40° are sharp, the rest smooth.
///
/// # Safety
///
/// `profile` is valid for `count` items; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_lathe(
    profile: *const FyVec2,
    count: usize,
    sides: u32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let size = [count as u32];
        // SAFETY: per this function's contract.
        let rings = unsafe { loops(profile, count, size.as_ptr(), 1) }?;
        if let Some(at) = rings[0].iter().position(|p| p.x < 0.0) {
            return Err(Error::new(
                FyStatus::InvalidValue,
                format!("profile point {at} is left of the axis (x below 0)"),
            ));
        }
        let data = MeshData::lathe(rings[0], segment_count(sides, 3, "sides")?)
            .map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes one mesh of `count` meshes, each placed by its transform, and
/// writes it to `out` (Unity's `CombineMeshes`). Submesh n of each goes
/// into submesh n of the result, so it's one draw per material. A
/// transform that mirrors turns its triangles round. A skinned mesh, or
/// one with no triangles, is refused.
///
/// # Safety
///
/// `meshes` and `transforms` are valid for `count` items, each mesh live;
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_combine(
    meshes: *const *const FyMesh,
    transforms: *const FyTransform,
    count: usize,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        if meshes.is_null() {
            return Err(Error::null("meshes"));
        }
        if transforms.is_null() {
            return Err(Error::null("transforms"));
        }
        // SAFETY: not null; the caller promises count items.
        let (meshes, transforms) =
            unsafe { (std::slice::from_raw_parts(meshes, count), std::slice::from_raw_parts(transforms, count)) };
        let mut parts = Vec::with_capacity(count);
        for (k, (&mesh, &t)) in meshes.iter().zip(transforms).enumerate() {
            // SAFETY: the caller promises each is live.
            let mesh = unsafe { mesh.as_ref() }.ok_or_else(|| Error::null(&format!("meshes[{k}]")))?;
            parts.push((&*mesh.0, t.to_component()?.matrix()));
        }
        let data = MeshData::combine(&parts).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// A path of `path_count` points, ten floats each, and an outline's
/// loops, as `fy_mesh_sweep` takes them.
///
/// # Safety
///
/// As `fy_mesh_sweep`.
unsafe fn sweep_parts<'a>(
    path: *const f32,
    path_count: usize,
    points: *const f32,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
) -> Result<(Vec<fynkel_world::PathPoint>, Vec<&'a [Vec2]>), Error> {
    // SAFETY: per this function's contract.
    let path = unsafe { items::<f32, 10>(path, path_count, "path") }?.ok_or_else(|| Error::null("path"))?;
    let path = path
        .iter()
        .map(|p| fynkel_world::PathPoint {
            position: Vec3::from_slice(&p[0..3]),
            forward: Vec3::from_slice(&p[3..6]),
            up: Vec3::from_slice(&p[6..9]),
            distance: p[9],
        })
        .collect();
    // SAFETY: per this function's contract; two floats are a FyVec2.
    let rings = unsafe { loops(points.cast::<FyVec2>(), point_count, loop_sizes, loop_count) }?;
    Ok((path, rings))
}

/// Makes a mesh of an outline swept along a path (a road, a rope, a pipe)
/// and writes it to `out`. The path is `path_count` points (2 or more),
/// ten floats each: its position, the way it goes (any length but 0),
/// which way is up across it, and how far along it is in metres. The
/// outline is drawn across the path at each, its X to the right and Y
/// up; `points` are its points, two floats each, then each hole's, and
/// `loop_sizes` says how many each has, the outline first (each 3 or
/// more, either way round). `caps` fills the ends; a path that closes
/// (its last point where its first is) has none. The texture repeats
/// `uv_per_metre` times a metre around the outline and along the path.
/// Corners of the outline sharper than 40° are sharp, the rest smooth.
///
/// # Safety
///
/// `path` is valid for `path_count * 10` floats, `points` for
/// `point_count * 2` and `loop_sizes` for `loop_count` items; `out` is
/// null or writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn fy_mesh_sweep(
    path: *const f32,
    path_count: usize,
    points: *const f32,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
    caps: bool,
    uv_per_metre: f32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (path, rings) = unsafe { sweep_parts(path, path_count, points, point_count, loop_sizes, loop_count) }?;
        let sweep = fynkel_world::Sweep {
            path: &path,
            outline: rings[0],
            holes: &rings[1..],
            caps,
            uv_per_metre: at_least_0(uv_per_metre, "uv_per_metre")?,
        };
        let data = MeshData::sweep(&sweep).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Sweeps into `mesh` in place of what it had, as `fy_mesh_sweep` makes
/// one: a rope swept again each frame. Worlds that have it keep what they
/// had until `fy_mesh_update`; when the triangles come out as they were,
/// they're kept. A skinned mesh is refused.
///
/// # Safety
///
/// `mesh` is null or live, used on no other thread meanwhile; the rest as
/// `fy_mesh_sweep`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn fy_mesh_set_sweep(
    mesh: *mut FyMesh,
    path: *const f32,
    path_count: usize,
    points: *const f32,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
    caps: bool,
    uv_per_metre: f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (path, rings) = unsafe { sweep_parts(path, path_count, points, point_count, loop_sizes, loop_count) }?;
        let sweep = fynkel_world::Sweep {
            path: &path,
            outline: rings[0],
            holes: &rings[1..],
            caps,
            uv_per_metre: at_least_0(uv_per_metre, "uv_per_metre")?,
        };
        // SAFETY: the caller promises it's null or live, and only theirs.
        let mesh = unsafe { mesh.as_mut() }.ok_or_else(|| Error::null("mesh"))?;
        mesh.own().sweep_into(&sweep).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))
    })
}

/// Changes `mesh`'s own data, as `change` says. Worlds that have it keep
/// what they had until `fy_mesh_update`. The data is copied when another
/// holds it (a world, a frame on its way to the GPU), so the first change
/// after an update copies the mesh and the rest change it in place; the
/// copy goes into one from before when nothing holds that any longer.
///
/// # Safety
///
/// `mesh` is null or live, and not used on another thread meanwhile.
unsafe fn change_mesh(mesh: *mut FyMesh, change: fynkel_world::MeshChange<'_>) -> Result<(), Error> {
    // SAFETY: the caller promises it's null or live, and only theirs.
    let mesh = unsafe { mesh.as_mut() }.ok_or_else(|| Error::null("mesh"))?;
    mesh.own().change(change).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))
}

/// Sets the vertices' positions, three floats each (x, y, z). A different
/// count from the mesh's resets every other channel: normals and tangents
/// are worked out again, UVs go to (0, 0) and colours to white; then its
/// triangles must not point past the last vertex, so set those first when
/// there are fewer. The array is copied.
///
/// # Safety
///
/// `mesh` is null or live, used on no other thread meanwhile;
/// `positions` is valid for `count * 3` floats (or null with `count` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_positions(mesh: *mut FyMesh, positions: *const f32, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let positions = unsafe { items::<f32, 3>(positions, count, "positions") }?.unwrap_or_default();
        let change = fynkel_world::MeshChange { positions: Some(positions), ..Default::default() };
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, change) }
    })
}

/// Sets the vertices' normals, three floats each, one per vertex. Each is
/// made unit length; none may be zero length.
///
/// # Safety
///
/// As `fy_mesh_set_positions`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_normals(mesh: *mut FyMesh, normals: *const f32, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let normals = unsafe { items::<f32, 3>(normals, count, "normals") }?.unwrap_or_default();
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, fynkel_world::MeshChange { normals: Some(normals), ..Default::default() }) }
    })
}

/// Sets the vertices' tangents, four floats each (a direction, then w = 1
/// or -1), one per vertex.
///
/// # Safety
///
/// As `fy_mesh_set_positions`, for `count * 4` floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_tangents(mesh: *mut FyMesh, tangents: *const f32, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let tangents = unsafe { items::<f32, 4>(tangents, count, "tangents") }?.unwrap_or_default();
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, fynkel_world::MeshChange { tangents: Some(tangents), ..Default::default() }) }
    })
}

/// Sets the vertices' texture coordinates, two floats each, one per vertex.
///
/// # Safety
///
/// As `fy_mesh_set_positions`, for `count * 2` floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_uvs(mesh: *mut FyMesh, uvs: *const f32, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let uvs = unsafe { items::<f32, 2>(uvs, count, "uvs") }?.unwrap_or_default();
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, fynkel_world::MeshChange { uvs: Some(uvs), ..Default::default() }) }
    })
}

/// Sets the vertices' colours, four bytes each (sRGB red, green and blue,
/// then alpha), one per vertex. They multiply the material's colour.
///
/// # Safety
///
/// As `fy_mesh_set_positions`, for `count * 4` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_colors(mesh: *mut FyMesh, colors: *const u8, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let colors = unsafe { items::<u8, 4>(colors, count, "colors") }?.unwrap_or_default();
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, fynkel_world::MeshChange { colors: Some(colors), ..Default::default() }) }
    })
}

/// Sets the triangles: three indices each, counter-clockwise seen from the
/// front, every submesh's one after another, and where each submesh after
/// the first starts (as `FyMeshData`'s). None is allowed: the mesh then
/// draws nothing.
///
/// # Safety
///
/// As `fy_mesh_set_positions`; `indices` is valid for `index_count` u32s
/// and `starts` for `start_count` (each null with its count 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_triangles(
    mesh: *mut FyMesh,
    indices: *const u32,
    index_count: usize,
    starts: *const u32,
    start_count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (indices, starts) = unsafe {
            (
                items::<u32, 1>(indices, index_count, "indices")?.unwrap_or_default().as_flattened(),
                items::<u32, 1>(starts, start_count, "starts")?.unwrap_or_default().as_flattened(),
            )
        };
        let change = fynkel_world::MeshChange { indices: Some((indices, starts)), ..Default::default() };
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, change) }
    })
}

/// Works out the mesh's normals (`FY_MESH_NORMALS`), from the faces around
/// each vertex weighted by their area, and its tangents
/// (`FY_MESH_TANGENTS`), from its UVs without splitting a vertex. Normals
/// first, when both.
///
/// # Safety
///
/// `mesh` is null or live, used on no other thread meanwhile.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_recalculate(mesh: *mut FyMesh, what: u32) -> FyStatus {
    guard(|| {
        let change = fynkel_world::MeshChange {
            recalculate_normals: what & FY_MESH_NORMALS != 0,
            recalculate_tangents: what & FY_MESH_TANGENTS != 0,
            ..Default::default()
        };
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, change) }
    })
}

/// Says the mesh changes often (Unity's `MarkDynamic`): each view keeps it
/// on the GPU with room to grow and writes changes where it is.
///
/// # Safety
///
/// `mesh` is null or live, used on no other thread meanwhile.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_mark_dynamic(mesh: *mut FyMesh) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { mesh.as_mut() }.ok_or_else(|| Error::null("mesh"))?;
        if !mesh.0.is_dynamic() {
            mesh.own().mark_dynamic();
        }
        Ok(())
    })
}

/// Gives the world `mesh` as it is now under `id`, which it was added as
/// (`fy_mesh_add`): its objects are drawn with it, and each view uploads it
/// again, from the next frame. Nothing is copied.
///
/// # Safety
///
/// `world` and `mesh` are null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_update(world: *const FyWorld, id: u64, mesh: *const FyMesh) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, data) = unsafe { (handle(world, "world")?, handle(mesh, "mesh")?) };
        Ok(world.lock()?.set_mesh(self::mesh(id)?, Arc::clone(&data.0))?)
    })
}

/// Copies the mesh's normals (3 floats each), tangents (4), UVs (2) and
/// colours (4 bytes) to the arrays given, each for `vertex_count`
/// vertices, which must be `fy_mesh_counts`' number. A null array is
/// skipped.
///
/// # Safety
///
/// `mesh` is null or live; each array is null or writable for its items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_read_vertices(
    mesh: *const FyMesh,
    normals: *mut f32,
    tangents: *mut f32,
    uvs: *mut f32,
    colors: *mut u8,
    vertex_count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        let vertices = mesh.0.vertices();
        if vertex_count != vertices.len() {
            return Err(Error::new(
                FyStatus::WrongSize,
                format!("the mesh has {} vertices, not {vertex_count}", vertices.len()),
            ));
        }
        /// Each vertex's `N` values from `get`, to `out` if it isn't null.
        ///
        /// # Safety
        ///
        /// `out` is null or writable for `vertices.len() * N` values.
        unsafe fn copy<T: Copy, const N: usize>(
            out: *mut T,
            vertices: &[fynkel_world::Vertex],
            get: impl Fn(&fynkel_world::Vertex) -> [T; N],
        ) {
            if out.is_null() {
                return;
            }
            // SAFETY: not null, and writable for this many per the contract.
            let out = unsafe { std::slice::from_raw_parts_mut(out, vertices.len() * N) };
            for (out, v) in out.chunks_exact_mut(N).zip(vertices) {
                out.copy_from_slice(&get(v));
            }
        }
        // SAFETY: per this function's contract.
        unsafe {
            copy(normals, vertices, |v| v.normal);
            copy(tangents, vertices, |v| v.tangent);
            copy(uvs, vertices, |v| v.uv);
            copy(colors, vertices, |v| v.color);
        }
        Ok(())
    })
}

/// Writes how many submeshes the mesh has: 1 when it doesn't split.
///
/// # Safety
///
/// `mesh` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_submesh_count(mesh: *const FyMesh, out: *mut usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        // SAFETY: per this function's contract.
        unsafe { write(out, mesh.0.submeshes().len(), "out") }
    })
}

/// Writes where each submesh starts in the indices, and how many it has,
/// for `count` submeshes, which must be `fy_mesh_submesh_count`'s.
///
/// # Safety
///
/// `mesh` is null or live; `starts` and `counts` are each null or writable
/// for `count` u32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_read_submeshes(
    mesh: *const FyMesh,
    starts: *mut u32,
    counts: *mut u32,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        let submeshes = mesh.0.submeshes();
        if count != submeshes.len() {
            return Err(Error::new(
                FyStatus::WrongSize,
                format!("the mesh has {} submeshes, not {count}", submeshes.len()),
            ));
        }
        for (i, range) in submeshes.iter().enumerate() {
            // SAFETY: each is null or writable for count u32s per the contract.
            unsafe {
                if !starts.is_null() {
                    starts.add(i).write(range.start);
                }
                if !counts.is_null() {
                    counts.add(i).write(range.end - range.start);
                }
            }
        }
        Ok(())
    })
}

/// Frees a mesh handle. Worlds that added it keep their share. Null is
/// ignored.
///
/// # Safety
///
/// `mesh` is null or live, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_free(mesh: *mut FyMesh) {
    if !mesh.is_null() {
        // A panic here leaks the mesh rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in new_mesh; the caller gives it up.
            drop(unsafe { Box::from_raw(mesh) });
            Ok(())
        });
    }
}

/// Writes how many vertices and indices the mesh has. There can be more
/// vertices than were given, when generated tangents split some.
///
/// # Safety
///
/// `mesh` is null or live; the outs are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_counts(
    mesh: *const FyMesh,
    out_vertices: *mut usize,
    out_indices: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        // SAFETY: per this function's contract.
        unsafe {
            write(out_vertices, mesh.0.vertices().len(), "out_vertices")?;
            write(out_indices, mesh.0.indices().len(), "out_indices")
        }
    })
}

/// Copies the mesh's vertex positions, three floats each, to `positions`
/// and its indices to `indices`: `vertex_count` and `index_count` must be
/// `fy_mesh_counts`' numbers. A null array (with its count 0) is skipped.
///
/// # Safety
///
/// `mesh` is null or live; `positions` is null or writable for
/// `vertex_count * 3` floats, `indices` for `index_count` u32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_read(
    mesh: *const FyMesh,
    positions: *mut f32,
    vertex_count: usize,
    indices: *mut u32,
    index_count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        let (vertices, mesh_indices) = (mesh.0.vertices(), mesh.0.indices());
        if !positions.is_null() {
            if vertex_count != vertices.len() {
                return Err(Error::new(
                    FyStatus::WrongSize,
                    format!("the mesh has {} vertices, not {vertex_count}", vertices.len()),
                ));
            }
            // SAFETY: not null, and writable for vertex_count * 3 floats per the contract.
            let out = unsafe { std::slice::from_raw_parts_mut(positions, vertex_count * 3) };
            for (out, vertex) in out.chunks_exact_mut(3).zip(vertices) {
                out.copy_from_slice(&vertex.position);
            }
        }
        if !indices.is_null() {
            if index_count != mesh_indices.len() {
                return Err(Error::new(
                    FyStatus::WrongSize,
                    format!("the mesh has {} indices, not {index_count}", mesh_indices.len()),
                ));
            }
            // SAFETY: not null, and writable for index_count u32s per the contract.
            unsafe { std::slice::from_raw_parts_mut(indices, index_count) }.copy_from_slice(mesh_indices);
        }
        Ok(())
    })
}

/// Gives the world a share of `mesh` (nothing is copied) and writes the
/// world's id for it to `out`. Adding the same handle twice gives two ids.
///
/// # Safety
///
/// `world` and `mesh` are null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_add(world: *const FyWorld, mesh: *const FyMesh, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, mesh) = unsafe { (handle(world, "world")?, handle(mesh, "mesh")?) };
        let id = world.lock()?.add_mesh(Arc::clone(&mesh.0));
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Removes a mesh. Refused while an object uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_mesh(mesh(id)?)?)
    })
}

/// Gives the submeshes of `object`'s mesh after the first a material each
/// (its `MeshRenderer`'s material draws the first), as Unity's
/// `Renderer.materials`: `count` ids at `materials`, each a material the
/// world has. A submesh without one isn't drawn. None (null with 0) takes
/// them away.
///
/// # Safety
///
/// `world` is null or live; `materials` is valid for `count` u64s (or null
/// with `count` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_submesh_materials_set(
    world: *const FyWorld,
    object: u64,
    materials: *const u64,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let ids = unsafe { items::<u64, 1>(materials, count, "materials") }?.unwrap_or_default();
        let ids = ids.iter().map(|&[id]| material(id)).collect::<Result<Vec<_>, _>>()?;
        Ok(world.lock()?.set_submesh_materials(self::object(object)?, &ids)?)
    })
}

/// Adds a material and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable; `out` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_add(world: *const FyWorld, value: *const FyMaterial, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, value) = unsafe { (handle(world, "world")?, read(value, "value")?) };
        let id = world.lock()?.add_material(value.to_material()?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Changes a material. Every object using it changes with it.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_set(world: *const FyWorld, id: u64, value: *const FyMaterial) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, value) = unsafe { (handle(world, "world")?, read(value, "value")?) };
        Ok(world.lock()?.set_material(material(id)?, value.to_material()?)?)
    })
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_get(world: *const FyWorld, id: u64, out: *mut FyMaterial) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let value = world.lock()?.material(material(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, value.into(), "out") }
    })
}

/// Removes a material. Refused while an object uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_material(material(id)?)?)
    })
}

/// Writes `len` bytes into a material's values from `offset`, where its
/// shader's `Values` puts the one being set (`fy_shader_describe`). Past
/// their end is `FY_STATUS_WRONG_SIZE`.
///
/// # Safety
///
/// `world` is null or live; `bytes` is valid for `len` reads (it may be
/// null when `len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_values_set(
    world: *const FyWorld,
    id: u64,
    offset: usize,
    bytes: *const u8,
    len: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        Ok(world.lock()?.set_material_values(material(id)?, offset, bytes)?)
    })
}

// --- Shaders -----------------------------------------------------------------

/// A material's own shader, compiled (`fy_shader_new`), until
/// `fy_shader_free`. Worlds that add it share it.
#[derive(Debug)]
pub struct FyShader(Shader);

/// Compiles `source` (`source_len` bytes of WGSL) for a material, or for a
/// screen effect when it has `fn effect` (`fy_camera_effects_set`):
/// checked and joined to the engine's code. `name` is its file's, for messages.
/// Writes the new handle to `out`. A shader that doesn't compile is
/// `FY_STATUS_INVALID_VALUE`, its message `name:line:column: what`, the
/// line its file's own.
///
/// # Safety
///
/// `name` and `source` are valid for their lengths' reads (null when the
/// length is 0); `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_new(
    name: *const u8,
    name_len: usize,
    source: *const u8,
    source_len: usize,
    out: *mut *mut FyShader,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (name, source) = unsafe { (utf8(name, name_len, "name")?, utf8(source, source_len, "source")?) };
        if out.is_null() {
            return Err(Error::null("out"));
        }
        let compiled = fynkel_render::CustomShader::new(name, source)
            .map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        let shader = Shader {
            name: name.to_owned(),
            values_size: compiled.values_size as usize,
            textures: compiled.textures.len(),
            effect: compiled.effect,
            compiled: Arc::new(compiled),
        };
        // SAFETY: checked non-null; the caller promises it's writable.
        unsafe { out.write(Box::into_raw(Box::new(FyShader(shader)))) };
        Ok(())
    })
}

/// Frees a shader handle. Worlds that added it keep their share. Null is
/// ignored.
///
/// # Safety
///
/// `shader` is null or live, and not used after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_free(shader: *mut FyShader) {
    if !shader.is_null() {
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in fy_shader_new; the caller
            // gives it up.
            drop(unsafe { Box::from_raw(shader) });
            Ok(())
        });
    }
}

/// What a shader takes, as UTF-8 lines, so a material can be given it by
/// name: `lit` or `unlit`; `values <bytes>`; `value <name> <f32|i32|u32>
/// <parts> <offset>` (parts: 1 for a number, 2 to 4 for a vector; the
/// offset in bytes) for each of its `Values`, an array's with `<count>
/// <stride>` after (its length; the bytes from one to the next); `texture <name>
/// <srgb|linear>` for each texture, in order; `own <name>` for each of its
/// `PerObject`, in order; `moves` if it has a `vertex`; `lights` if it
/// has a `light`; `behind` if it reads what's behind it (`scene_depth`,
/// `scene_color`); `effect` if it's a screen effect's. Its length goes to
/// `out_len`; it's copied into `buf` only when it fits in `cap`.
///
/// # Safety
///
/// `shader` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_describe(
    shader: *const FyShader,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let shader = unsafe { handle(shader, "shader") }?;
        let compiled = shader.0.compiled.downcast_ref::<fynkel_render::CustomShader>().expect("made by fy_shader_new");
        let text = describe_shader(compiled);
        // SAFETY: per this function's contract.
        unsafe { write_all(text.bytes(), buf, cap, out_len) }
    })
}

fn describe_shader(shader: &fynkel_render::CustomShader) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    let lit = matches!(shader.lighting, fynkel_render::Lighting::Lit);
    let _ = writeln!(text, "{}", if lit { "lit" } else { "unlit" });
    let _ = writeln!(text, "values {}", shader.values_size);
    for v in &shader.values {
        let kind = match v.kind {
            fynkel_render::ValueKind::F32 => "f32",
            fynkel_render::ValueKind::I32 => "i32",
            fynkel_render::ValueKind::U32 => "u32",
        };
        match v.count {
            1 => writeln!(text, "value {} {kind} {} {}", v.name, v.size, v.offset),
            count => writeln!(text, "value {} {kind} {} {} {count} {}", v.name, v.size, v.offset, v.stride),
        }
        .ok();
    }
    for t in &shader.textures {
        let space = if t.space == fynkel_render::ColorSpace::Srgb { "srgb" } else { "linear" };
        let _ = writeln!(text, "texture {} {space}", t.name);
    }
    for name in &shader.own {
        let _ = writeln!(text, "own {name}");
    }
    if shader.moves {
        let _ = writeln!(text, "moves");
    }
    if shader.lights {
        let _ = writeln!(text, "lights");
    }
    if shader.reads_behind {
        let _ = writeln!(text, "behind");
    }
    if shader.effect {
        let _ = writeln!(text, "effect");
    }
    text
}

/// Gives the world a share of `shader` and writes the id materials use
/// for it to `out`.
///
/// # Safety
///
/// `world` and `shader` are null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_add(world: *const FyWorld, shader: *const FyShader, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, shader) = unsafe { (handle(world, "world")?, handle(shader, "shader")?) };
        let id = world.lock()?.add_shader(shader.0.clone());
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Puts `shader` in place of the world's shader `id`, as when its file
/// changes: its materials draw with it from the next frame, their values
/// kept where they fit.
///
/// # Safety
///
/// `world` and `shader` are null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_set(world: *const FyWorld, id: u64, shader: *const FyShader) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, new) = unsafe { (handle(world, "world")?, handle(shader, "shader")?) };
        Ok(world.lock()?.set_shader(self::shader(id)?, new.0.clone())?)
    })
}

/// Removes a shader from the world. Refused (`FY_STATUS_IN_USE`) while a
/// material uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_shader(shader(id)?)?)
    })
}

/// Sets the time a world's shaders see (`frame_time()`), in seconds, and
/// how far it moved since the last frame (`frame_delta()`): the game's,
/// so it stops while it's paused.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_time_set(world: *const FyWorld, time: f32, delta: f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        if !time.is_finite() || !delta.is_finite() {
            return Err(Error::new(FyStatus::InvalidValue, "the time must be finite"));
        }
        world.lock()?.set_shader_time(time, delta);
        Ok(())
    })
}

// --- Textures ----------------------------------------------------------------

/// An image on its own, before any world has it. Opaque to C. Made with
/// `fy_texture_decode` or `fy_texture_new_rgba` on any thread, freed with
/// `fy_texture_free`. Worlds that add it share its pixels, so freeing it
/// doesn't take it from them.
#[derive(Debug)]
pub struct FyTexture(Arc<ImageData>);

/// Writes a new texture handle to `out`.
///
/// # Safety
///
/// `out` is null or writable.
unsafe fn new_texture(image: Result<ImageData, ImageError>, out: *mut *mut FyTexture) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null("out"));
    }
    let image = image.map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
    // SAFETY: checked non-null; the caller promises it's writable.
    unsafe { out.write(Box::into_raw(Box::new(FyTexture(Arc::new(image))))) };
    Ok(())
}

/// Decodes a PNG or JPEG of `len` bytes into a texture handle and writes
/// it to `out`. Any PNG layout becomes 8-bit RGBA. A broken or unsupported
/// file, or one over 8192 pixels on a side, is refused
/// (`FY_STATUS_INVALID_VALUE`) with a message saying why.
///
/// Decoding a big image takes tens of milliseconds: call it off the UI
/// thread.
///
/// # Safety
///
/// `bytes` is valid for `len` bytes (it may be null when `len` is 0);
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_decode(bytes: *const u8, len: usize, out: *mut *mut FyTexture) -> FyStatus {
    guard(|| {
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        // SAFETY: per this function's contract.
        unsafe { new_texture(ImageData::decode(bytes), out) }
    })
}

/// Makes a texture handle from `len` bytes of 8-bit RGBA, rows from the
/// top, and writes it to `out`. `len` must be `width × height × 4`.
///
/// # Safety
///
/// `pixels` is valid for `len` bytes (it may be null when `len` is 0);
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_new_rgba(
    width: u32,
    height: u32,
    pixels: *const u8,
    len: usize,
    out: *mut *mut FyTexture,
) -> FyStatus {
    guard(|| {
        let pixels = if len == 0 {
            Vec::new()
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(pixels, "pixels")?, len) }.to_vec()
        };
        // SAFETY: per this function's contract.
        unsafe { new_texture(ImageData::from_rgba(width, height, pixels), out) }
    })
}

/// Frees a texture handle. Worlds that added it keep their share. Null is
/// ignored.
///
/// # Safety
///
/// `texture` is null or live, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_free(texture: *mut FyTexture) {
    if !texture.is_null() {
        // A panic here leaks the texture rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in new_texture; the caller gives
            // it up.
            drop(unsafe { Box::from_raw(texture) });
            Ok(())
        });
    }
}

/// Writes the texture's width and height in pixels.
///
/// # Safety
///
/// `texture` is null or live; the outs are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_size(
    texture: *const FyTexture,
    out_width: *mut u32,
    out_height: *mut u32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let texture = unsafe { handle(texture, "texture") }?;
        // SAFETY: per this function's contract.
        unsafe {
            write(out_width, texture.0.width(), "out_width")?;
            write(out_height, texture.0.height(), "out_height")
        }
    })
}

/// Gives the world a share of `texture` (nothing is copied) and writes the
/// id materials use for it to `out`. Each view uploads it to the GPU the
/// first frame a material uses it.
///
/// # Safety
///
/// `world` and `texture` are null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_add(world: *const FyWorld, texture: *const FyTexture, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, texture) = unsafe { (handle(world, "world")?, handle(texture, "texture")?) };
        let id = world.lock()?.add_texture(Arc::clone(&texture.0));
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Removes a texture from the world; each view frees its GPU copy on its
/// next frame. Refused (`FY_STATUS_IN_USE`) while a material uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_texture(texture(id)?)?)
    })
}

// --- Models ------------------------------------------------------------------

/// A model read from a `.glb`, before any world has it. Opaque to C. Made
/// with `fy_model_load` on any thread, freed with `fy_model_free`. Its
/// meshes and textures are shared with the handles `fy_model_mesh` and
/// `fy_model_texture` give, and with every world that spawns it.
#[derive(Debug)]
pub struct FyModel(Arc<ModelData>);

/// How many of each thing a model has, from `fy_model_counts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FyModelCounts {
    pub meshes: usize,
    pub textures: usize,
    pub materials: usize,
    /// Lines saying what was skipped (`fy_model_warning`).
    pub warnings: usize,
    /// Animations (`fy_model_clip`).
    pub clips: usize,
}

/// Reads a `.glb` (binary glTF 2.0) of `len` bytes and writes the model to
/// `out`. Its images are decoded and its meshes' tangents generated here,
/// so a big model takes a while: call it off the UI thread.
///
/// What can't be drawn yet (skins, see-through materials, ...)
/// is skipped with a warning (`fy_model_warning`). A file that can't be
/// read at all, or needs a glTF extension the engine doesn't know, is
/// refused (`FY_STATUS_INVALID_VALUE`) with a message saying why.
///
/// # Safety
///
/// `bytes` is valid for `len` bytes (it may be null when `len` is 0);
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_load(bytes: *const u8, len: usize, out: *mut *mut FyModel) -> FyStatus {
    guard(|| {
        if out.is_null() {
            return Err(Error::null("out"));
        }
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        let model = ModelData::from_glb(bytes).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: checked non-null; the caller promises it's writable.
        unsafe { out.write(Box::into_raw(Box::new(FyModel(Arc::new(model))))) };
        Ok(())
    })
}

/// Frees a model handle. Mesh and texture handles taken from it, and
/// worlds that spawned it, keep their share. Null is ignored.
///
/// # Safety
///
/// `model` is null or live, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_free(model: *mut FyModel) {
    if !model.is_null() {
        // A panic here leaks the model rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in fy_model_load; the caller gives
            // it up.
            drop(unsafe { Box::from_raw(model) });
            Ok(())
        });
    }
}

/// # Safety
///
/// `model` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_counts(model: *const FyModel, out: *mut FyModelCounts) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let counts = FyModelCounts {
            meshes: model.meshes().len(),
            textures: model.images().len(),
            materials: model.materials().len(),
            warnings: model.warnings().len(),
            clips: model.clips().len(),
        };
        // SAFETY: per this function's contract.
        unsafe { write(out, counts, "out") }
    })
}

/// `items[index]`, or an error naming what there are fewer of.
fn nth<'a, T>(items: &'a [T], index: usize, what: &str) -> Result<&'a T, Error> {
    items.get(index).ok_or_else(|| {
        Error::new(FyStatus::InvalidValue, format!("the model has {} {what}; there's no {what} {index}", items.len()))
    })
}

/// Writes a new handle to the model's mesh `index` to `out`, sharing its
/// data.
///
/// # Safety
///
/// `model` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_mesh(model: *const FyModel, index: usize, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let mesh = Arc::clone(nth(model.meshes(), index, "meshes")?);
        // SAFETY: per this function's contract.
        unsafe { write(out, Box::into_raw(Box::new(FyMesh::new(mesh))), "out") }
    })
}

/// Writes a new handle to the model's image `index` to `out`, sharing its
/// pixels.
///
/// # Safety
///
/// `model` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_texture(model: *const FyModel, index: usize, out: *mut *mut FyTexture) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let image = Arc::clone(nth(model.images(), index, "textures")?);
        // SAFETY: per this function's contract.
        unsafe { write(out, Box::into_raw(Box::new(FyTexture(image))), "out") }
    })
}

/// Writes the model's material `index` to `out`, its colours converted to
/// sRGB. Each texture field is the model's texture index plus 1 (0 for
/// none), not a world's id: the caller maps them to its own.
///
/// # Safety
///
/// `model` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_material(model: *const FyModel, index: usize, out: *mut FyMaterial) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let data = nth(model.materials(), index, "materials")?;
        let numbers: Vec<TextureId> =
            (1..=model.images().len() as u64).map(|n| TextureId::from_raw(n).expect("from 1")).collect();
        let material: FyMaterial = Material::from_model(data, &numbers).into();
        // SAFETY: per this function's contract.
        unsafe { write(out, material, "out") }
    })
}

/// The name the file gives material `index` ("" for none), as UTF-8
/// without a terminating zero: its length goes to `out_len`, and it's
/// copied into `buf` only when it fits in `cap`.
///
/// # Safety
///
/// `model` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_material_name(
    model: *const FyModel,
    index: usize,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let name = &nth(model.materials(), index, "materials")?.name;
        // SAFETY: per this function's contract.
        unsafe { write_all(name.bytes(), buf, cap, out_len) }
    })
}

/// Warning `index`: one line saying what was skipped, as UTF-8 like
/// `fy_model_material_name`.
///
/// # Safety
///
/// `model` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_warning(
    model: *const FyModel,
    index: usize,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let warning = nth(model.warnings(), index, "warnings")?;
        // SAFETY: per this function's contract.
        unsafe { write_all(warning.bytes(), buf, cap, out_len) }
    })
}

/// Clip `index`'s name, as UTF-8 like `fy_model_material_name`, and its
/// length in seconds to `out_length`.
///
/// # Safety
///
/// `model` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` and `out_length` are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_clip(
    model: *const FyModel,
    index: usize,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
    out_length: *mut f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let clip = nth(model.clips(), index, "clips")?;
        // SAFETY: per this function's contract.
        unsafe {
            write(out_length, clip.length, "out_length")?;
            write_all(clip.name.bytes(), buf, cap, out_len)
        }
    })
}

/// `count` ids at `p`, each turned into a typed id by `id`.
///
/// # Safety
///
/// `p` is null or valid for `count` reads (it may be null when `count` is
/// 0).
unsafe fn ids<T>(p: *const u64, count: usize, what: &str, id: fn(u64) -> Result<T, Error>) -> Result<Vec<T>, Error> {
    if count == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: per this function's contract.
    let raw = unsafe { std::slice::from_raw_parts(handle(p, what)?, count) };
    raw.iter().map(|&r| id(r)).collect()
}

/// Spawns the model: a root object at `transform`, named after the model's
/// scene, with one object per node below it, named and placed as in the
/// file. A node drawing one primitive gets a mesh renderer; one drawing
/// several gets an unnamed child for each. Writes the root's id to `out`.
///
/// `meshes` and `materials` are this world's ids for the model's meshes
/// (`fy_mesh_add` of each `fy_model_mesh`) and materials (`fy_material_add`
/// of each `fy_model_material`, its textures mapped), in the model's
/// order. Spawning again with the same ids shares everything. On error
/// nothing is spawned.
///
/// # Safety
///
/// `world` and `model` are null or live; `meshes` and `materials` are
/// valid for their counts; `transform` is null or readable; `out` is null
/// or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_spawn(
    world: *const FyWorld,
    model: *const FyModel,
    meshes: *const u64,
    mesh_count: usize,
    materials: *const u64,
    material_count: usize,
    transform: *const FyTransform,
    out: *mut u64,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, model, transform) =
            unsafe { (handle(world, "world")?, &handle(model, "model")?.0, read(transform, "transform")?) };
        // SAFETY: per this function's contract.
        let (meshes, materials) = unsafe {
            (ids(meshes, mesh_count, "meshes", mesh)?, ids(materials, material_count, "materials", material)?)
        };
        let root = world.lock()?.spawn_model(model, &meshes, &materials, transform.to_component()?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, root.raw(), "out") }
    })
}

// --- Animation ---------------------------------------------------------------

/// What an animator's layer plays, from `fy_animator_get`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyAnimatorState {
    /// The clip last played or faded to, or -1 for none or a blend.
    pub clip: i32,
    /// The blend last played or faded to, or -1 for none or a clip.
    pub blend: i32,
    /// Seconds into it; for a blend, into a round of it.
    pub time: f32,
    /// Its length in seconds; for a blend, a round's at its number now.
    pub length: f32,
    pub speed: f32,
}

/// What one of a reporting animator's clips did in `fy_world_animate`:
/// its time went from `from` to `to`, passing its end `loops` times on
/// the way (a looping clip), or reaching it (`ended`, one that doesn't).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyAnimationEvent {
    pub root: u64,
    pub clip: u32,
    pub loops: u32,
    pub from: f32,
    pub to: f32,
    pub ended: bool,
}

/// Plays clip `clip` of the model copy `root` (a model's index, as
/// `fy_model_clip`) on its layer `layer` (0 the base, up to 7) from its
/// start: at once in place of what the layer plays, with `fade` 0, or
/// fading in over `fade` seconds as the rest fade out, from the pose
/// shown. Fading to the motion last played or faded to changes nothing;
/// to one fading out, it fades back in from where it is. An object that
/// isn't a model copy with clips is `FY_STATUS_NO_COMPONENT`. `fade` is
/// read through a pointer, as `fy_world_animate`'s `dt`: a game may call
/// it each frame.
///
/// # Safety
///
/// `world` is null or live; `fade` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_play(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    clip: u32,
    fade: *const f32,
    looping: bool,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, fade) = unsafe { (handle(world, "world")?, read(fade, "fade")?) };
        Ok(world.lock()?.play_on(object(root)?, layer, fynkel_world::Motion::Clip(clip), fade, looping)?)
    })
}

/// Makes a blend of `root`'s clips: `count` clips, each at its threshold
/// (any order), mixed by the number `fy_animator_blend` gives, their pace
/// kept together. Writes its number to `out`; an animator keeps up to 64.
///
/// # Safety
///
/// `world` is null or live; `clips` and `thresholds` are null or valid for
/// `count` reads; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_blend_define(
    world: *const FyWorld,
    root: u64,
    clips: *const u32,
    thresholds: *const f32,
    count: usize,
    out: *mut u32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let pairs: Vec<(u32, f32)> = if count == 0 {
            Vec::new()
        } else {
            // SAFETY: per this function's contract.
            let (clips, thresholds) = unsafe {
                (
                    std::slice::from_raw_parts(handle(clips, "clips")?, count),
                    std::slice::from_raw_parts(handle(thresholds, "thresholds")?, count),
                )
            };
            clips.iter().copied().zip(thresholds.iter().copied()).collect()
        };
        let blend = world.lock()?.define_blend(object(root)?, &pairs)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, blend, "out") }
    })
}

/// Sets the number blend `blend` mixes its clips by, `at[0]`, and plays
/// it on `root`'s layer `layer` as `fy_animator_play` plays a clip, with
/// the fade `at[1]` (always looping): a game calls it each frame with the
/// speed, and a fade only starts when the layer played something else.
///
/// # Safety
///
/// `world` is null or live; `at` is null or valid for 2 reads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_blend(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    blend: u32,
    at: *const f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract: two floats.
        let [at, fade] = unsafe { read(at.cast::<[f32; 2]>(), "at") }?;
        let mut world = world.lock()?;
        let root = object(root)?;
        world.set_blend(root, blend, at)?;
        Ok(world.play_on(root, layer, fynkel_world::Motion::Blend(blend), fade, true)?)
    })
}

/// Stops what `root`'s layer `layer` plays: at once with `fade` 0, its
/// parts keeping the pose they're in, or fading out over `fade` seconds
/// to what's underneath. `fade` is read through a pointer.
///
/// # Safety
///
/// `world` is null or live; `fade` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_stop(world: *const FyWorld, root: u64, layer: u32, fade: *const f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, fade) = unsafe { (handle(world, "world")?, read(fade, "fade")?) };
        Ok(world.lock()?.stop_on(object(root)?, layer, fade)?)
    })
}

/// Sets `root`'s layer `layer`: the part it moves from, with everything
/// below it (`mask`, `mask_len` UTF-8 bytes; empty for the whole model),
/// whether it adds to what's underneath rather than replacing it, and how
/// much of it shows, 0 to 1. The base layer, 0, takes the whole model, in
/// place, at 1.
///
/// # Safety
///
/// `world` is null or live; `mask` is null or valid for `mask_len` reads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_layer_set(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    mask: *const u8,
    mask_len: usize,
    additive: bool,
    weight: f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, mask) = unsafe { (handle(world, "world")?, utf8(mask, mask_len, "mask")?) };
        let mask = if mask.is_empty() { None } else { Some(mask) };
        Ok(world.lock()?.set_animation_layer(object(root)?, layer, mask, additive, weight)?)
    })
}

/// How fast `root`'s clips play: 1 as made, 0 paused, negative backwards.
/// Read through a pointer: a game may set it each frame.
///
/// # Safety
///
/// `world` is null or live; `speed` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_speed_set(world: *const FyWorld, root: u64, speed: *const f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, speed) = unsafe { (handle(world, "world")?, read(speed, "speed")?) };
        Ok(world.lock()?.set_animation_speed(object(root)?, speed)?)
    })
}

/// Moves the current motion of `root`'s layer `layer` to `time` seconds,
/// kept within its length.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_time_set(world: *const FyWorld, root: u64, layer: u32, time: f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_animation_time_on(object(root)?, layer, time)?)
    })
}

/// Whether `fy_world_animate` reports what `root`'s clips do.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_report_set(world: *const FyWorld, root: u64, on: bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_animation_reporting(object(root)?, on)?)
    })
}

/// Writes what `root`'s layer `layer` plays to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_get(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    out: *mut FyAnimatorState,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let state = world.lock()?.animator_state_on(object(root)?, layer)?;
        let (clip, blend) = match state.motion {
            Some(fynkel_world::Motion::Clip(c)) => (c as i32, -1),
            Some(fynkel_world::Motion::Blend(b)) => (-1, b as i32),
            None => (-1, -1),
        };
        let state = FyAnimatorState { clip, blend, time: state.time, length: state.length, speed: state.speed };
        // SAFETY: per this function's contract.
        unsafe { write(out, state, "out") }
    })
}

/// Writes clip `clip`'s weight in the mix of `root`'s layer `layer` to
/// `out`: 1 alone, 0 not playing, between while it fades or shares a
/// blend.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_weight(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    clip: u32,
    out: *mut f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let weight = world.lock()?.clip_weight_on(object(root)?, layer, clip)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, weight, "out") }
    })
}

/// The ids of `root`'s parts some clip moves, each once, as
/// `fy_children_get` writes them: all when they fit in `cap`, and their
/// number to `out_count`.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_parts(
    world: *const FyWorld,
    root: u64,
    buf: *mut u64,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let parts = world.lock()?.animated_parts(object(root)?)?;
        // SAFETY: per this function's contract.
        unsafe { write_all(parts.iter().map(|p| p.raw()), buf, cap, out_count) }
    })
}

/// Advances every animator by the seconds at `dt` and poses its model's
/// parts, once a frame before it's drawn. Writes how many events the
/// reporting animators gave to `out_count`; `fy_animation_events_get`
/// reads them. `dt` is read through a pointer: a double passed to a call
/// is boxed, each frame, by Dart's JIT.
///
/// # Safety
///
/// `world` is null or live; `dt` is null or readable; `out_count` is null
/// or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_animate(world: *const FyWorld, dt: *const f32, out_count: *mut usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, dt) = unsafe { (handle(world, "world")?, read(dt, "dt")?) };
        let mut inner = world.lock()?;
        let mut events = world.events.lock().unwrap_or_else(PoisonError::into_inner);
        events.clear();
        inner.animate(dt, &mut events);
        // SAFETY: per this function's contract.
        unsafe { write(out_count, events.len(), "out_count") }
    })
}

/// The last `fy_world_animate`'s events, as `fy_children_get` writes ids:
/// all of them when they fit in `cap`, and their number to `out_count`.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animation_events_get(
    world: *const FyWorld,
    buf: *mut FyAnimationEvent,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let _inner = world.lock()?;
        let events = world.events.lock().unwrap_or_else(PoisonError::into_inner);
        let items = events.iter().map(|e| FyAnimationEvent {
            root: e.root.raw(),
            clip: e.clip,
            loops: e.loops,
            from: e.from,
            to: e.to,
            ended: e.ended,
        });
        // SAFETY: per this function's contract.
        unsafe { write_all(items, buf, cap, out_count) }
    })
}

// --- Particles ---------------------------------------------------------------

/// A number for each particle: `min` when they're the same, else one at
/// random from `min` to `max`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyRange {
    pub min: f32,
    pub max: f32,
}

impl From<FyRange> for ParticleRange {
    fn from(r: FyRange) -> Self {
        Self { min: r.min, max: r.max }
    }
}

/// `FyParticleSettings::space`: particles move with the object.
pub const FY_PARTICLE_SPACE_LOCAL: u32 = 0;
/// Particles stay where they were made as the object moves.
pub const FY_PARTICLE_SPACE_WORLD: u32 = 1;

/// `FyParticleSettings::stop_action`, and `FyParticleEvent::action`.
pub const FY_PARTICLE_STOP_NONE: u32 = 0;
pub const FY_PARTICLE_STOP_DISABLE: u32 = 1;
pub const FY_PARTICLE_STOP_DESTROY: u32 = 2;
pub const FY_PARTICLE_STOP_CALLBACK: u32 = 3;

/// `FyParticleShape::kind`.
pub const FY_PARTICLE_SHAPE_POINT: u32 = 0;
pub const FY_PARTICLE_SHAPE_SPHERE: u32 = 1;
pub const FY_PARTICLE_SHAPE_HEMISPHERE: u32 = 2;
pub const FY_PARTICLE_SHAPE_CONE: u32 = 3;
pub const FY_PARTICLE_SHAPE_BOX: u32 = 4;
pub const FY_PARTICLE_SHAPE_CIRCLE: u32 = 5;
pub const FY_PARTICLE_SHAPE_EDGE: u32 = 6;

/// The steps of `FyParticleSettings`' curves over a particle's life.
pub const FY_PARTICLE_TABLE: u32 = 64;

/// The most bursts in `FyParticleSettings`.
pub const FY_PARTICLE_MAX_BURSTS: u32 = 8;

/// `FyCurve::mode`: between `min` and `max`.
pub const FY_CURVE_CONSTANT: u32 = 0;
/// Between `tables[0]` and `tables[1]` over a particle's life.
pub const FY_CURVE_CURVES: u32 = 1;

/// A number over a particle's life (Unity's `MinMaxCurve`): each
/// particle's is somewhere between two constants or two curves, by a
/// random number of its own.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyCurve {
    /// `FY_CURVE_*`.
    pub mode: u32,
    pub min: f32,
    pub max: f32,
    pub tables: [[f32; FY_PARTICLE_TABLE as usize]; 2],
}

impl FyCurve {
    /// `value` for every particle, all its life.
    pub const fn constant(value: f32) -> Self {
        Self { mode: FY_CURVE_CONSTANT, min: value, max: value, tables: [[value; FY_PARTICLE_TABLE as usize]; 2] }
    }

    fn curve(&self, what: &str) -> Result<ParticleCurve, Error> {
        match self.mode {
            FY_CURVE_CONSTANT => Ok(ParticleCurve::Constant(ParticleRange { min: self.min, max: self.max })),
            FY_CURVE_CURVES => Ok(ParticleCurve::Curves(Box::new(self.tables))),
            _ => Err(Error::new(FyStatus::InvalidValue, format!("{what}'s mode isn't one of the FY_CURVE_* values"))),
        }
    }
}

/// A vector over a particle's life, an axis each, in `space`
/// (`FY_PARTICLE_SPACE_*`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleVector {
    pub x: FyCurve,
    pub y: FyCurve,
    pub z: FyCurve,
    pub space: u32,
}

impl FyParticleVector {
    fn vector(&self, what: &str) -> Result<VectorOverLifetime, Error> {
        Ok(VectorOverLifetime {
            x: self.x.curve(what)?,
            y: self.y.curve(what)?,
            z: self.z.curve(what)?,
            space: particle_space(self.space).ok_or_else(|| {
                Error::new(FyStatus::InvalidValue, format!("{what}.space isn't one of the FY_PARTICLE_SPACE_* values"))
            })?,
        })
    }
}

/// A speed past `limit` is brought down by `dampen`, 0 to 1; `drag` slows
/// every particle by that much of its speed a second.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleLimit {
    pub limit: FyCurve,
    pub dampen: f32,
    pub drag: FyCurve,
}

/// A wander through a smooth random field: `strength` in metres a
/// second, `frequency` swirls a metre, the field changing `scroll_speed`
/// a second, 1 to 4 `octaves`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleNoise {
    pub strength: FyCurve,
    pub frequency: f32,
    pub scroll_speed: f32,
    pub octaves: u32,
}

/// The material's texture as `columns` × `rows` frames, `frame_over_time`
/// (0 the first, 1 past the last) `cycles` times a life, starting
/// `start_frame` on; with `random_row`, a row each.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleSheet {
    pub columns: u32,
    pub rows: u32,
    pub frame_over_time: FyCurve,
    pub start_frame: FyRange,
    pub cycles: f32,
    pub random_row: bool,
}

/// `FyParticleSettings::render_mode`: a square facing the camera.
pub const FY_PARTICLE_RENDER_BILLBOARD: u32 = 0;
/// Long the way it moves, `length_scale` and `speed_scale`.
pub const FY_PARTICLE_RENDER_STRETCHED: u32 = 1;
/// Lying flat, facing up.
pub const FY_PARTICLE_RENDER_HORIZONTAL: u32 = 2;
/// Standing up, turned to the camera.
pub const FY_PARTICLE_RENDER_VERTICAL: u32 = 3;
/// `mesh`, turned by its rotation round up.
pub const FY_PARTICLE_RENDER_MESH: u32 = 4;

/// Where particles are made and which way they go. Shapes face up (+Y).
/// Angles are radians.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleShape {
    /// `FY_PARTICLE_SHAPE_*`.
    pub kind: u32,
    pub radius: f32,
    /// 0 on the rim alone to 1 all through.
    pub radius_thickness: f32,
    /// A cone's, from up to its side: 0 to under π/2.
    pub angle: f32,
    /// Up to 2π.
    pub arc: f32,
    /// A box's.
    pub size: FyVec3,
    /// 0 to 1.
    pub random_direction: f32,
}

/// `FyParticleInherit::mode`: the velocity a particle starts with.
pub const FY_PARTICLE_INHERIT_INITIAL: u32 = 0;
/// The velocity its object has each frame, carrying it along.
pub const FY_PARTICLE_INHERIT_CURRENT: u32 = 1;

/// A particle takes `curve` of its object's velocity (Unity's *Inherit
/// Velocity*): `FY_PARTICLE_INHERIT_*`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleInherit {
    pub mode: u32,
    pub curve: FyCurve,
}

/// The most planes in `FyParticleCollision`.
pub const FY_PARTICLE_MAX_PLANES: u32 = 6;

/// Bouncing off planes: each the object's, through its position, facing
/// its up. As `fynkel_world::ParticleCollision`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleCollision {
    /// How many of `planes` there are.
    pub plane_count: u32,
    pub planes: [u64; FY_PARTICLE_MAX_PLANES as usize],
    pub dampen: FyCurve,
    pub bounce: FyCurve,
    pub lifetime_loss: FyCurve,
    pub min_kill_speed: f32,
    pub radius_scale: f32,
    /// Its hits are events (`FY_PARTICLE_EVENT_COLLISION`).
    pub send_messages: bool,
}

/// The most sub emitters in `FyParticleSettings`.
pub const FY_PARTICLE_MAX_SUB_EMITTERS: u32 = 4;

/// `FyParticleSubEmitter::kind`: as each particle is born, all its life.
pub const FY_PARTICLE_SUB_BIRTH: u32 = 0;
/// As one hits a plane.
pub const FY_PARTICLE_SUB_COLLISION: u32 = 1;
/// As one dies.
pub const FY_PARTICLE_SUB_DEATH: u32 = 2;

/// `FyParticleSubEmitter::inherit`, or'd: the particle's colour, size and
/// rotation, times the sub system's own.
pub const FY_PARTICLE_INHERIT_COLOR: u32 = 1;
pub const FY_PARTICLE_INHERIT_SIZE: u32 = 2;
pub const FY_PARTICLE_INHERIT_ROTATION: u32 = 4;

/// Another object's system that emits where this one's particles are
/// born, hit or die, with `probability`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleSubEmitter {
    pub system: u64,
    /// `FY_PARTICLE_SUB_*`.
    pub kind: u32,
    /// `FY_PARTICLE_INHERIT_COLOR` and the others.
    pub inherit: u32,
    pub probability: f32,
}

/// `count` particles at once, `time` seconds into the timeline, `cycles`
/// times (0 for as long as it plays) every `interval` seconds, each time
/// with `probability`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleBurst {
    pub time: f32,
    pub count: FyRange,
    pub cycles: u32,
    pub interval: f32,
    pub probability: f32,
}

/// A particle system's settings: Unity's main, emission and shape
/// modules, its over-lifetime and by-speed modules, noise, a texture
/// sheet, and how it's drawn. Sent when they change, not each frame.
/// Seconds, metres and radians; colours sRGB.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleSettings {
    pub duration: f32,
    pub looping: bool,
    pub prewarm: bool,
    pub play_on_awake: bool,
    pub unscaled_time: bool,
    pub start_delay: f32,
    pub start_lifetime: FyRange,
    pub start_speed: FyRange,
    pub start_size: FyRange,
    pub start_rotation: FyRange,
    /// Each particle's is between the two at random.
    pub start_color: [FyColor; 2],
    pub gravity_modifier: f32,
    /// `FY_PARTICLE_SPACE_*`.
    pub space: u32,
    pub simulation_speed: f32,
    pub max_particles: u32,
    /// `FY_PARTICLE_STOP_*`.
    pub stop_action: u32,
    /// The same particles each play when `seeded`.
    pub seeded: bool,
    pub seed: u32,
    pub rate_over_time: f32,
    /// How many of `bursts` there are.
    pub burst_count: u32,
    pub bursts: [FyParticleBurst; FY_PARTICLE_MAX_BURSTS as usize],
    pub shape: FyParticleShape,
    /// A particle's size times `size_table` over its life, when set.
    pub has_size_table: bool,
    pub size_table: [f32; FY_PARTICLE_TABLE as usize],
    /// A particle's colour times `color_table` over its life, when set.
    pub has_color_table: bool,
    pub color_table: [FyColor; FY_PARTICLE_TABLE as usize],
    pub has_velocity_over_lifetime: bool,
    pub velocity_over_lifetime: FyParticleVector,
    pub has_limit_velocity: bool,
    pub limit_velocity: FyParticleLimit,
    pub has_force_over_lifetime: bool,
    pub force_over_lifetime: FyParticleVector,
    /// Radians a second.
    pub has_rotation_over_lifetime: bool,
    pub rotation_over_lifetime: FyCurve,
    /// A particle's size times `size_by_speed`, from `size_speeds.min` m/s
    /// (its first step) to `size_speeds.max` (its last).
    pub has_size_by_speed: bool,
    pub size_by_speed: [f32; FY_PARTICLE_TABLE as usize],
    pub size_speeds: FyRange,
    pub has_color_by_speed: bool,
    pub color_by_speed: [FyColor; FY_PARTICLE_TABLE as usize],
    pub color_speeds: FyRange,
    pub has_noise: bool,
    pub noise: FyParticleNoise,
    pub has_texture_sheet: bool,
    pub texture_sheet: FyParticleSheet,
    /// What it's drawn with, or 0 for nothing drawn.
    pub material: u64,
    /// `FY_PARTICLE_RENDER_*`.
    pub render_mode: u32,
    /// A stretched particle's length: `length_scale` times its size, plus
    /// `speed_scale` times its speed.
    pub length_scale: f32,
    pub speed_scale: f32,
    /// `FY_PARTICLE_RENDER_MESH`'s mesh.
    pub mesh: u64,
    /// Lit as a surface facing the camera.
    pub lit: bool,
    /// Metres it fades out in front of what's behind it; 0 for none.
    pub softness: f32,
    /// Particles a metre its object moves.
    pub rate_over_distance: f32,
    pub has_inherit_velocity: bool,
    pub inherit_velocity: FyParticleInherit,
    pub has_collision: bool,
    pub collision: FyParticleCollision,
    /// How many of `sub_emitters` there are.
    pub sub_emitter_count: u32,
    pub sub_emitters: [FyParticleSubEmitter; FY_PARTICLE_MAX_SUB_EMITTERS as usize],
}

impl Default for FyParticleSettings {
    /// Unity's: five seconds looping, ten a second living five seconds
    /// each, at 5 m/s, a size of 1, white, from a cone of 25°.
    fn default() -> Self {
        let d = ParticleSettings::default();
        let range = |r: ParticleRange| FyRange { min: r.min, max: r.max };
        Self {
            duration: d.duration,
            looping: d.looping,
            prewarm: d.prewarm,
            play_on_awake: d.play_on_awake,
            unscaled_time: d.unscaled_time,
            start_delay: d.start_delay,
            start_lifetime: range(d.start_lifetime),
            start_speed: range(d.start_speed),
            start_size: range(d.start_size),
            start_rotation: range(d.start_rotation),
            start_color: d.start_color.map(FyColor::from),
            gravity_modifier: d.gravity_modifier,
            space: FY_PARTICLE_SPACE_LOCAL,
            simulation_speed: d.simulation_speed,
            max_particles: d.max_particles,
            stop_action: FY_PARTICLE_STOP_NONE,
            seeded: false,
            seed: 0,
            rate_over_time: d.rate_over_time,
            burst_count: 0,
            bursts: [FyParticleBurst::default(); FY_PARTICLE_MAX_BURSTS as usize],
            shape: FyParticleShape {
                kind: FY_PARTICLE_SHAPE_CONE,
                radius: d.shape.radius,
                radius_thickness: d.shape.radius_thickness,
                angle: d.shape.angle,
                arc: d.shape.arc,
                size: d.shape.size.into(),
                random_direction: d.shape.random_direction,
            },
            has_size_table: false,
            size_table: [1.0; FY_PARTICLE_TABLE as usize],
            has_color_table: false,
            color_table: [FyColor::from(Color::WHITE); FY_PARTICLE_TABLE as usize],
            has_velocity_over_lifetime: false,
            velocity_over_lifetime: FyParticleVector::ZERO,
            has_limit_velocity: false,
            limit_velocity: FyParticleLimit {
                limit: FyCurve::constant(1.0),
                dampen: 0.0,
                drag: FyCurve::constant(0.0),
            },
            has_force_over_lifetime: false,
            force_over_lifetime: FyParticleVector::ZERO,
            has_rotation_over_lifetime: false,
            rotation_over_lifetime: FyCurve::constant(0.0),
            has_size_by_speed: false,
            size_by_speed: [1.0; FY_PARTICLE_TABLE as usize],
            size_speeds: FyRange { min: 0.0, max: 1.0 },
            has_color_by_speed: false,
            color_by_speed: [FyColor::from(Color::WHITE); FY_PARTICLE_TABLE as usize],
            color_speeds: FyRange { min: 0.0, max: 1.0 },
            has_noise: false,
            noise: FyParticleNoise { strength: FyCurve::constant(1.0), frequency: 0.5, scroll_speed: 0.0, octaves: 1 },
            has_texture_sheet: false,
            texture_sheet: FyParticleSheet {
                columns: 1,
                rows: 1,
                frame_over_time: FyCurve::constant(0.0),
                start_frame: FyRange::default(),
                cycles: 1.0,
                random_row: false,
            },
            material: 0,
            render_mode: FY_PARTICLE_RENDER_BILLBOARD,
            length_scale: 2.0,
            speed_scale: 0.0,
            mesh: 0,
            lit: false,
            softness: 0.0,
            rate_over_distance: 0.0,
            has_inherit_velocity: false,
            inherit_velocity: FyParticleInherit { mode: FY_PARTICLE_INHERIT_INITIAL, curve: FyCurve::constant(0.0) },
            has_collision: false,
            collision: FyParticleCollision {
                plane_count: 0,
                planes: [0; FY_PARTICLE_MAX_PLANES as usize],
                dampen: FyCurve::constant(0.0),
                bounce: FyCurve::constant(1.0),
                lifetime_loss: FyCurve::constant(0.0),
                min_kill_speed: 0.0,
                radius_scale: 1.0,
                send_messages: false,
            },
            sub_emitter_count: 0,
            sub_emitters: [FyParticleSubEmitter::default(); FY_PARTICLE_MAX_SUB_EMITTERS as usize],
        }
    }
}

impl FyParticleSettings {
    fn settings(&self) -> Result<ParticleSettings, Error> {
        let bad =
            |what: &str| Error::new(FyStatus::InvalidValue, format!("{what} isn't one of the FY_PARTICLE_* values"));
        let space = particle_space(self.space).ok_or_else(|| bad("space"))?;
        let render_mode = match self.render_mode {
            FY_PARTICLE_RENDER_BILLBOARD => RenderMode::Billboard,
            FY_PARTICLE_RENDER_STRETCHED => {
                RenderMode::Stretched { length_scale: self.length_scale, speed_scale: self.speed_scale }
            }
            FY_PARTICLE_RENDER_HORIZONTAL => RenderMode::Horizontal,
            FY_PARTICLE_RENDER_VERTICAL => RenderMode::Vertical,
            FY_PARTICLE_RENDER_MESH => RenderMode::Mesh(mesh(self.mesh)?),
            _ => return Err(bad("render_mode")),
        };
        let by_speed = |speeds: FyRange| (speeds.min, speeds.max);
        let stop_action = stop_action(self.stop_action).ok_or_else(|| bad("stop_action"))?;
        let kind = match self.shape.kind {
            FY_PARTICLE_SHAPE_POINT => ShapeKind::Point,
            FY_PARTICLE_SHAPE_SPHERE => ShapeKind::Sphere,
            FY_PARTICLE_SHAPE_HEMISPHERE => ShapeKind::Hemisphere,
            FY_PARTICLE_SHAPE_CONE => ShapeKind::Cone,
            FY_PARTICLE_SHAPE_BOX => ShapeKind::Box,
            FY_PARTICLE_SHAPE_CIRCLE => ShapeKind::Circle,
            FY_PARTICLE_SHAPE_EDGE => ShapeKind::Edge,
            _ => return Err(bad("shape.kind")),
        };
        if self.burst_count > FY_PARTICLE_MAX_BURSTS {
            return Err(Error::new(FyStatus::InvalidValue, "a particle system has at most 8 bursts"));
        }
        if self.sub_emitter_count > FY_PARTICLE_MAX_SUB_EMITTERS {
            return Err(Error::new(FyStatus::InvalidValue, "a particle system has at most 4 sub emitters"));
        }
        let c = &self.collision;
        if self.has_collision && c.plane_count > FY_PARTICLE_MAX_PLANES {
            return Err(Error::new(FyStatus::InvalidValue, "a particle system collides with at most 6 planes"));
        }
        let s = &self.shape;
        Ok(ParticleSettings {
            duration: self.duration,
            looping: self.looping,
            prewarm: self.prewarm,
            start_delay: self.start_delay,
            start_lifetime: self.start_lifetime.into(),
            start_speed: self.start_speed.into(),
            start_size: self.start_size.into(),
            start_rotation: self.start_rotation.into(),
            start_color: self.start_color.map(Color::from),
            gravity_modifier: self.gravity_modifier,
            space,
            simulation_speed: self.simulation_speed,
            unscaled_time: self.unscaled_time,
            play_on_awake: self.play_on_awake,
            max_particles: self.max_particles,
            stop_action,
            seed: self.seeded.then_some(self.seed),
            rate_over_time: self.rate_over_time,
            bursts: self.bursts[..self.burst_count as usize]
                .iter()
                .map(|b| Burst {
                    time: b.time,
                    count: b.count.into(),
                    cycles: b.cycles,
                    interval: b.interval,
                    probability: b.probability,
                })
                .collect(),
            shape: Shape {
                kind,
                radius: s.radius,
                radius_thickness: s.radius_thickness,
                angle: s.angle,
                arc: s.arc,
                size: s.size.into(),
                random_direction: s.random_direction,
            },
            size_over_lifetime: self.has_size_table.then(|| Box::new(self.size_table)),
            color_over_lifetime: self.has_color_table.then(|| Box::new(self.color_table.map(Color::from))),
            velocity_over_lifetime: self
                .has_velocity_over_lifetime
                .then(|| self.velocity_over_lifetime.vector("velocity_over_lifetime"))
                .transpose()?,
            limit_velocity: self
                .has_limit_velocity
                .then(|| -> Result<_, Error> {
                    let l = &self.limit_velocity;
                    Ok(LimitVelocity { limit: l.limit.curve("limit")?, dampen: l.dampen, drag: l.drag.curve("drag")? })
                })
                .transpose()?,
            force_over_lifetime: self
                .has_force_over_lifetime
                .then(|| self.force_over_lifetime.vector("force_over_lifetime"))
                .transpose()?,
            rotation_over_lifetime: self
                .has_rotation_over_lifetime
                .then(|| self.rotation_over_lifetime.curve("rotation_over_lifetime"))
                .transpose()?,
            size_by_speed: self.has_size_by_speed.then(|| {
                let (min_speed, max_speed) = by_speed(self.size_speeds);
                BySpeed { table: Box::new(self.size_by_speed), min_speed, max_speed }
            }),
            color_by_speed: self.has_color_by_speed.then(|| {
                let (min_speed, max_speed) = by_speed(self.color_speeds);
                BySpeed { table: Box::new(self.color_by_speed.map(Color::from)), min_speed, max_speed }
            }),
            noise: self
                .has_noise
                .then(|| -> Result<_, Error> {
                    let n = &self.noise;
                    Ok(ParticleNoise {
                        strength: n.strength.curve("noise.strength")?,
                        frequency: n.frequency,
                        scroll_speed: n.scroll_speed,
                        octaves: n.octaves,
                    })
                })
                .transpose()?,
            texture_sheet: self
                .has_texture_sheet
                .then(|| -> Result<_, Error> {
                    let t = &self.texture_sheet;
                    Ok(TextureSheet {
                        columns: t.columns,
                        rows: t.rows,
                        frame_over_time: t.frame_over_time.curve("texture_sheet.frame_over_time")?,
                        start_frame: t.start_frame.into(),
                        cycles: t.cycles,
                        random_row: t.random_row,
                    })
                })
                .transpose()?,
            material: MaterialId::from_raw(self.material),
            render_mode,
            lit: self.lit,
            softness: self.softness,
            rate_over_distance: self.rate_over_distance,
            inherit_velocity: self
                .has_inherit_velocity
                .then(|| -> Result<_, Error> {
                    let i = &self.inherit_velocity;
                    let mode = match i.mode {
                        FY_PARTICLE_INHERIT_INITIAL => InheritMode::Initial,
                        FY_PARTICLE_INHERIT_CURRENT => InheritMode::Current,
                        _ => return Err(bad("inherit_velocity.mode")),
                    };
                    Ok(InheritVelocity { mode, curve: i.curve.curve("inherit_velocity.curve")? })
                })
                .transpose()?,
            collision: self
                .has_collision
                .then(|| -> Result<_, Error> {
                    Ok(ParticleCollision {
                        planes: c.planes[..c.plane_count as usize]
                            .iter()
                            .map(|&p| object(p))
                            .collect::<Result<_, _>>()?,
                        dampen: c.dampen.curve("collision.dampen")?,
                        bounce: c.bounce.curve("collision.bounce")?,
                        lifetime_loss: c.lifetime_loss.curve("collision.lifetime_loss")?,
                        min_kill_speed: c.min_kill_speed,
                        radius_scale: c.radius_scale,
                        send_messages: c.send_messages,
                    })
                })
                .transpose()?,
            sub_emitters: self.sub_emitters[..self.sub_emitter_count as usize]
                .iter()
                .map(|e| {
                    let kind = match e.kind {
                        FY_PARTICLE_SUB_BIRTH => SubEmitterKind::Birth,
                        FY_PARTICLE_SUB_COLLISION => SubEmitterKind::Collision,
                        FY_PARTICLE_SUB_DEATH => SubEmitterKind::Death,
                        _ => return Err(bad("sub_emitters.kind")),
                    };
                    Ok(SubEmitter { system: object(e.system)?, kind, inherit: e.inherit, probability: e.probability })
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

impl FyParticleVector {
    const ZERO: Self = Self {
        x: FyCurve::constant(0.0),
        y: FyCurve::constant(0.0),
        z: FyCurve::constant(0.0),
        space: FY_PARTICLE_SPACE_LOCAL,
    };
}

fn particle_space(value: u32) -> Option<Space> {
    match value {
        FY_PARTICLE_SPACE_LOCAL => Some(Space::Local),
        FY_PARTICLE_SPACE_WORLD => Some(Space::World),
        _ => None,
    }
}

fn stop_action(value: u32) -> Option<StopAction> {
    Some(match value {
        FY_PARTICLE_STOP_NONE => StopAction::None,
        FY_PARTICLE_STOP_DISABLE => StopAction::Disable,
        FY_PARTICLE_STOP_DESTROY => StopAction::Destroy,
        FY_PARTICLE_STOP_CALLBACK => StopAction::Callback,
        _ => return None,
    })
}

fn stop_action_value(action: StopAction) -> u32 {
    match action {
        StopAction::None => FY_PARTICLE_STOP_NONE,
        StopAction::Disable => FY_PARTICLE_STOP_DISABLE,
        StopAction::Destroy => FY_PARTICLE_STOP_DESTROY,
        StopAction::Callback => FY_PARTICLE_STOP_CALLBACK,
    }
}

/// What a particle system is doing, from `fy_particles_get`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleState {
    /// How many particles it has.
    pub count: u32,
    /// From play until it's stopped and its last particle is gone.
    pub playing: bool,
    pub paused: bool,
    pub emitting: bool,
}

/// `FyParticleEvent::kind`: a system was done (stopped, its last particle
/// gone), with a stop action for the game to do.
pub const FY_PARTICLE_EVENT_DONE: u32 = 0;
/// A system's particles hit a plane, with `send_messages` on (Unity's
/// `OnParticleCollision`), `count` of them this step.
pub const FY_PARTICLE_EVENT_COLLISION: u32 = 1;

/// What happened in the last `fy_world_particles_step`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleEvent {
    pub object: u64,
    /// `FY_PARTICLE_EVENT_*`.
    pub kind: u32,
    /// `FY_PARTICLE_EVENT_DONE`'s: `FY_PARTICLE_STOP_*`, not
    /// `FY_PARTICLE_STOP_NONE`.
    pub action: u32,
    /// `FY_PARTICLE_EVENT_COLLISION`'s: the plane's object, and how many
    /// hit it.
    pub plane: u64,
    pub count: u32,
}

/// `fy_particles_command`: plays from the start (or on from a pause).
pub const FY_PARTICLES_PLAY: u32 = 0;
/// Emits no more; its particles live out their lives.
pub const FY_PARTICLES_STOP: u32 = 1;
/// Emits no more, and its particles go at once.
pub const FY_PARTICLES_STOP_AND_CLEAR: u32 = 2;
pub const FY_PARTICLES_PAUSE: u32 = 3;
/// Its particles go; it plays on.
pub const FY_PARTICLES_CLEAR: u32 = 4;
/// Makes `count` particles now, playing or not.
pub const FY_PARTICLES_EMIT: u32 = 5;

/// Gives the object a particle system with `settings`, or changes the one
/// it has, keeping its particles and where it is. A new one plays when
/// it's first stepped, if it plays on awake. Settings that can't be are
/// `FY_STATUS_INVALID_VALUE` and change nothing.
///
/// # Safety
///
/// `world` is null or live; `settings` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_set(
    world: *const FyWorld,
    id: u64,
    settings: *const FyParticleSettings,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, settings) =
            unsafe { (handle(world, "world")?, settings.as_ref().ok_or_else(|| Error::null("settings"))?) };
        let settings = settings.settings()?;
        world.lock()?.set_particles(object(id)?, settings)?;
        Ok(())
    })
}

/// Takes the object's particle system away, its particles too.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.remove_particles(object(id)?)?;
        Ok(())
    })
}

/// Plays, stops, pauses, clears or emits from the object's system:
/// `FY_PARTICLES_*`. `count` is `FY_PARTICLES_EMIT`'s, ignored otherwise.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_command(world: *const FyWorld, id: u64, command: u32, count: u32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let command = match command {
            FY_PARTICLES_PLAY => ParticleCommand::Play,
            FY_PARTICLES_STOP => ParticleCommand::Stop { clear: false },
            FY_PARTICLES_STOP_AND_CLEAR => ParticleCommand::Stop { clear: true },
            FY_PARTICLES_PAUSE => ParticleCommand::Pause,
            FY_PARTICLES_CLEAR => ParticleCommand::Clear,
            FY_PARTICLES_EMIT => ParticleCommand::Emit(count),
            _ => return Err(Error::new(FyStatus::InvalidValue, "command isn't one of FY_PARTICLES_*")),
        };
        world.lock()?.particle_command(object(id)?, command)?;
        Ok(())
    })
}

/// Moves the object's system `seconds` on at once (Unity's `Simulate`).
/// `seconds` is read through a pointer, as `fy_world_animate`'s `dt`.
///
/// # Safety
///
/// `world` is null or live; `seconds` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_simulate(world: *const FyWorld, id: u64, seconds: *const f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, seconds) = unsafe { (handle(world, "world")?, read(seconds, "seconds")?) };
        world.lock()?.simulate_particles(object(id)?, seconds)?;
        Ok(())
    })
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_get(world: *const FyWorld, id: u64, out: *mut FyParticleState) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let inner = world.lock()?;
        let system = inner.particles(object(id)?)?;
        let state = FyParticleState {
            count: system.count() as u32,
            playing: system.is_playing(),
            paused: system.is_paused(),
            emitting: system.is_emitting(),
        };
        // SAFETY: per this function's contract.
        unsafe { write(out, state, "out") }
    })
}

/// Moves every particle system on, once a frame, after the animators and
/// before the game's late update (where Unity steps them): `dt` seconds
/// of the game's time, `unscaled` for those that ask; and the trails, by
/// `dt`. Writes how many events there were (systems done, with a stop
/// action, and planes hit) to `out_count`; `fy_particle_events_get` reads
/// them. Both times are read through
/// pointers, as `fy_world_animate`'s `dt`.
///
/// # Safety
///
/// `world` is null or live; `dt` and `unscaled` are null or readable;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_particles_step(
    world: *const FyWorld,
    dt: *const f32,
    unscaled: *const f32,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, dt, unscaled) = unsafe { (handle(world, "world")?, read(dt, "dt")?, read(unscaled, "unscaled")?) };
        let mut inner = world.lock()?;
        let mut events = world.particle_events.lock().unwrap_or_else(PoisonError::into_inner);
        events.clear();
        if inner.has_particles() || inner.has_trails() {
            inner.step_particles(dt, unscaled, &mut events);
        }
        // SAFETY: per this function's contract.
        unsafe { write(out_count, events.len(), "out_count") }
    })
}

/// The last `fy_world_particles_step`'s events, as `fy_children_get`
/// writes ids.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particle_events_get(
    world: *const FyWorld,
    buf: *mut FyParticleEvent,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let _inner = world.lock()?;
        let events = world.particle_events.lock().unwrap_or_else(PoisonError::into_inner);
        let items = events.iter().map(|e| match e.kind {
            ParticleEventKind::Done(action) => FyParticleEvent {
                object: e.object.raw(),
                kind: FY_PARTICLE_EVENT_DONE,
                action: stop_action_value(action),
                ..FyParticleEvent::default()
            },
            ParticleEventKind::Collision { plane, count } => FyParticleEvent {
                object: e.object.raw(),
                kind: FY_PARTICLE_EVENT_COLLISION,
                plane: plane.raw(),
                count,
                ..FyParticleEvent::default()
            },
        });
        // SAFETY: per this function's contract.
        unsafe { write_all(items, buf, cap, out_count) }
    })
}

// --- Trails ------------------------------------------------------------------

/// A trail's settings (Unity's `TrailRenderer`), as
/// `fynkel_world::TrailSettings`: seconds and metres, colours sRGB. Its
/// width and colour go over its length, head to tail.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyTrailSettings {
    pub time: f32,
    pub min_vertex_distance: f32,
    pub width: f32,
    /// Its width times `width_table` along it, when set.
    pub has_width_table: bool,
    pub width_table: [f32; FY_PARTICLE_TABLE as usize],
    /// Its colour along it, when set; else white.
    pub has_color_table: bool,
    pub color_table: [FyColor; FY_PARTICLE_TABLE as usize],
    pub emitting: bool,
    /// What it's drawn with, or 0 for nothing drawn.
    pub material: u64,
}

impl Default for FyTrailSettings {
    /// Unity's: five seconds, a point each 0.1 m, a metre wide, white.
    fn default() -> Self {
        let d = TrailSettings::default();
        Self {
            time: d.time,
            min_vertex_distance: d.min_vertex_distance,
            width: d.width,
            has_width_table: false,
            width_table: [1.0; FY_PARTICLE_TABLE as usize],
            has_color_table: false,
            color_table: [FyColor::from(Color::WHITE); FY_PARTICLE_TABLE as usize],
            emitting: d.emitting,
            material: 0,
        }
    }
}

impl FyTrailSettings {
    fn settings(&self) -> TrailSettings {
        TrailSettings {
            time: self.time,
            min_vertex_distance: self.min_vertex_distance,
            width: self.width,
            width_curve: self.has_width_table.then(|| Box::new(self.width_table)),
            color: self.has_color_table.then(|| Box::new(self.color_table.map(Color::from))),
            emitting: self.emitting,
            material: MaterialId::from_raw(self.material),
        }
    }
}

const _: () = assert!(TABLE == FY_PARTICLE_TABLE as usize);

/// Gives the object a trail with `settings`, or changes the one it has,
/// keeping its points. Settings that can't be are
/// `FY_STATUS_INVALID_VALUE` and change nothing.
///
/// # Safety
///
/// `world` is null or live; `settings` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_trail_set(world: *const FyWorld, id: u64, settings: *const FyTrailSettings) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, settings) =
            unsafe { (handle(world, "world")?, settings.as_ref().ok_or_else(|| Error::null("settings"))?) };
        world.lock()?.set_trail(object(id)?, settings.settings())?;
        Ok(())
    })
}

/// Takes the object's trail away.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_trail_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.remove_trail(object(id)?)?;
        Ok(())
    })
}

/// Takes every point of the object's trail away (Unity's `Clear`).
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_trail_clear(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.clear_trail(object(id)?)?;
        Ok(())
    })
}

/// How many points the object's trail has, the head's not counted.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_trail_count(world: *const FyWorld, id: u64, out: *mut usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let count = world.lock()?.trail(object(id)?)?.count();
        // SAFETY: per this function's contract.
        unsafe { write(out, count, "out") }
    })
}

// --- Views -------------------------------------------------------------------

/// Starts a view of the world behind `world`: the target `make` builds on
/// the process's GPU device, a renderer sharing the world's meshes and
/// textures, and the render worker.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
unsafe fn new_view<T: fynkel_present::Target + 'static>(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
    make: impl FnOnce(&Gpu, u32, u32) -> Result<(T, Output), Error>,
) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null("out"));
    }
    if world.is_null() {
        return Err(Error::null("world"));
    }
    // SAFETY: not null, and made by fy_world_new (Arc::into_raw); this adds
    // the view's own reference.
    let world = unsafe {
        Arc::increment_strong_count(world);
        Arc::from_raw(world)
    };
    let device = Device::current().map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
    check_view_size(&device, width, height)?;
    let gpu = {
        let mut held = world.gpu.lock().map_err(|_| Error::new(FyStatus::Poisoned, "a view of this world panicked"))?;
        match held.upgrade() {
            Some(gpu) if Arc::ptr_eq(&gpu.device, &device) => gpu,
            // No view now, or the views were on a device that was lost:
            // what's uploaded starts over.
            _ => {
                let gpu = WorldGpu::new(&device);
                *held = Arc::downgrade(&gpu);
                gpu
            }
        }
    };
    let (target, output) = make(&device.gpu, width, height)?;
    let mut renderer = Renderer::sharing(gpu.assets.clone(), fynkel_present::Target::format(&target));
    // On screen, a shader the driver hasn't seen is compiled beside the
    // frames, its objects left out till it's ready, rather than holding a
    // frame up for a third of a second. Headless views wait, so what they
    // read back is the whole scene.
    renderer.set_wait_for_shaders(matches!(output, Output::Headless(_)));
    let worker = Worker::start_for(gpu.clone(), renderer, target)
        .map_err(|e| Error::new(FyStatus::WorkerFailed, format!("couldn't start the render worker: {e}")))?;
    let view = FyView { world, gpu, clock: Mutex::default(), worker, output };
    // SAFETY: checked non-null above; the caller promises it's writable.
    unsafe { out.write(Box::into_raw(Box::new(view))) };
    Ok(())
}

/// Whether `device` can draw a view of `width` × `height`: past its largest
/// texture, making the targets would lose the device for every view.
fn check_view_size(device: &Device, width: u32, height: u32) -> Result<(), Error> {
    let most = device.gpu.device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > most || height > most {
        return Err(Error::new(
            FyStatus::InvalidValue,
            format!("a view can't be {width} × {height}: each side is from 1 to {most} on this device"),
        ));
    }
    Ok(())
}

/// A view of `world` that draws `width` × `height` frames offscreen and
/// reads them back (`fy_view_read`). Starts its render worker. Free it with
/// `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_headless(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        unsafe {
            new_view(world, width, height, out, |gpu, width, height| {
                let (target, frames) = Headless::new(&gpu.device, &gpu.queue, width, height);
                Ok((target, Output::Headless(frames)))
            })
        }
    })
}

/// A view of `world` that draws into a ring of IOSurfaces for Flutter to
/// show (macOS and iOS; `FY_STATUS_UNSUPPORTED` elsewhere). The Flutter
/// plugin shows them with `fy_apple_acquire` and learns of new frames
/// through `fy_apple_set_frame_ready`. Free it with `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_apple(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        #[cfg(target_vendor = "apple")]
        // SAFETY: per this function's contract.
        // Dart calls in on a thread with no autorelease pool, so what
        // Metal autoreleases while the device and surfaces are made would
        // outlive the view: drain it here.
        return fynkel_present::autorelease_pool(|| unsafe {
            new_view(world, width, height, out, |gpu, width, height| {
                let (target, presenter) = fynkel_present::Apple::new(&gpu.device, &gpu.queue, width, height)
                    .map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
                Ok((target, Output::Apple(presenter)))
            })
        });
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (world, width, height, out);
            Err(Error::new(FyStatus::Unsupported, "Apple views exist only on macOS and iOS"))
        }
    })
}

/// A view of `world` that draws straight to a `CAMetalLayer` the Flutter
/// plugin puts under Flutter's view (iOS and macOS;
/// `FY_STATUS_UNSUPPORTED` elsewhere). It draws nothing until the plugin
/// gives it a layer (`fy_apple_layer_attach`), and each layer brings its
/// own size: `fy_view_resize` changes nothing. Free it with
/// `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_apple_layer(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        #[cfg(target_vendor = "apple")]
        // SAFETY: per this function's contract.
        return fynkel_present::autorelease_pool(|| unsafe {
            new_view(world, width, height, out, |gpu, width, height| {
                let (target, presenter) =
                    fynkel_present::Layer::new(&gpu.instance, &gpu.adapter, &gpu.device, &gpu.queue, width, height);
                Ok((target, Output::AppleLayer(presenter)))
            })
        });
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (world, width, height, out);
            Err(Error::new(FyStatus::Unsupported, "Apple views exist only on macOS and iOS"))
        }
    })
}

/// A view of `world` that draws into an Android `Surface` for Flutter to
/// show (Android only; `FY_STATUS_UNSUPPORTED` elsewhere). It draws nothing
/// until the Flutter plugin gives it a surface, and each new surface brings
/// its own size: the plugin's JNI calls (`FynkelNative.attach` and
/// `detach`) do both, and `fy_view_resize` changes nothing. `width` ×
/// `height` is the size until the first surface. Free it with
/// `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_android(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        #[cfg(target_os = "android")]
        // SAFETY: per this function's contract.
        return unsafe {
            new_view(world, width, height, out, |gpu, width, height| {
                let (target, presenter) =
                    fynkel_present::Android::new(&gpu.instance, &gpu.adapter, &gpu.device, &gpu.queue, width, height);
                Ok((target, Output::Android(presenter)))
            })
        };
        #[cfg(not(target_os = "android"))]
        {
            let _ = (world, width, height, out);
            Err(Error::new(FyStatus::Unsupported, "Android views exist only on Android"))
        }
    })
}

/// Stops a view's render worker (after the frame it's drawing) and frees
/// the view. Null is ignored.
///
/// # Safety
///
/// `view` is null or a live view, not used again after this, and no other
/// thread is inside a call on it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_free(view: *mut FyView) {
    if !view.is_null() {
        // A panic here leaks the view rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in new_view; the caller gives
            // it up. In a pool, like creation, for what Metal autoreleases.
            fynkel_present::autorelease_pool(|| drop(unsafe { Box::from_raw(view) }));
            Ok(())
        });
    }
}

/// One frame: copies the world into a scene and hands it to the render
/// worker, without waiting for the GPU. Writes the frame's number (1, 2, …)
/// to `out_number`.
///
/// # Safety
///
/// `view` is null or live; `out_number` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_frame(view: *const FyView, out_number: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        if out_number.is_null() {
            return Err(Error::null("out_number"));
        }
        let mut clock = view.clock.lock().map_err(|_| Error::new(FyStatus::Poisoned, "the view's clock panicked"))?;
        let clock = &mut *clock;
        let mut scene = view.worker.scene();
        clock.extract.shake = *view.world.shake.lock().unwrap_or_else(PoisonError::into_inner);
        clock.extract.quality = *view.world.quality.lock().unwrap_or_else(PoisonError::into_inner);
        clock.extract.sky.clone_from(&view.world.sky.lock().unwrap_or_else(PoisonError::into_inner));
        clock.extract.fog = *view.world.fog.lock().unwrap_or_else(PoisonError::into_inner);
        let world = match view.world.lock() {
            Ok(world) => world,
            Err(error) => {
                view.worker.recycle(scene);
                return Err(error);
            }
        };
        if let Err(message) = clock.extract.run_shared(&world, &mut scene, &mut view.gpu.ledger()) {
            view.worker.recycle(scene);
            return Err(Error::new(FyStatus::InvalidValue, message));
        }
        drop(world);
        let number = clock.last_number + 1;
        view.worker.post(number, scene, &mut Vec::new())?;
        clock.last_number = number;
        // SAFETY: checked non-null above; the caller promises it's writable.
        unsafe { out_number.write(number) };
        Ok(())
    })
}

/// Headless views: waits up to `timeout_ms` for frame `number` (or a later
/// one) to finish, copies its pixels into `rgba` (width × height × 4 bytes,
/// RGBA8 sRGB, rows top to bottom) and writes the number of the frame read
/// to `out_number`. Blocks; never call it from a UI thread.
///
/// # Safety
///
/// `view` is null or live; `rgba` is null or valid for `len` bytes of
/// writes; `out_number` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_read(
    view: *const FyView,
    number: u64,
    timeout_ms: u32,
    rgba: *mut u8,
    len: usize,
    out_number: *mut u64,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        if rgba.is_null() {
            return Err(Error::null("rgba"));
        }
        if out_number.is_null() {
            return Err(Error::null("out_number"));
        }
        #[allow(irrefutable_let_patterns)] // only headless views on desktop Linux and Windows
        let Output::Headless(frames) = &view.output else {
            return Err(Error::new(FyStatus::InvalidValue, "not a headless view"));
        };
        // SAFETY: not null, and the caller promises len writable bytes.
        let rgba = unsafe { std::slice::from_raw_parts_mut(rgba, len) };
        let read = frames.read(number, Duration::from_millis(timeout_ms.into()), rgba).map_err(|e| {
            // A frame that never came may be the worker's fault.
            if let Some(failure) = view.worker.failure() {
                return failure;
            }
            let status = match e {
                ReadError::Timeout { .. } => FyStatus::Timeout,
                ReadError::WrongSize { .. } => FyStatus::WrongSize,
            };
            Error::new(status, e.to_string())
        })?;
        // SAFETY: checked non-null above; the caller promises it's writable.
        unsafe { out_number.write(read) };
        Ok(())
    })
}

/// Draws the view's frames from the next `fy_view_frame` on from the camera
/// on object `camera`, or from the world's main camera (`fy_camera_active`)
/// when it's 0, as a new view does. A view with a camera of its own draws
/// black while that object is hidden, gone, or has no camera or transform:
/// a minimap whose camera goes stays a minimap, not a second main view.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_camera_set(view: *const FyView, camera: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        let mut clock = view.clock.lock().map_err(|_| Error::new(FyStatus::Poisoned, "the view's clock panicked"))?;
        clock.extract.camera = ObjectId::from_raw(camera);
        Ok(())
    })
}

/// Draws the frames from the next `fy_view_frame` on at `width` × `height`
/// pixels. Frames already on their way finish at the old size and, on
/// Apple, are never shown.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_resize(view: *const FyView, width: u32, height: u32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        check_view_size(&view.gpu.device, width, height)?;
        view.worker.resize(width, height)
    })
}

/// Whether the test hooks may run: only in a test process, one with
/// `FLUTTER_TEST` (set by `flutter test`) or `FYNKEL_TEST_HOOKS` in its
/// environment. A game's hook calls fail with `FY_STATUS_UNSUPPORTED`.
fn test_hooks() -> Result<(), Error> {
    if std::env::var_os("FLUTTER_TEST").is_some() || std::env::var_os("FYNKEL_TEST_HOOKS").is_some() {
        return Ok(());
    }
    Err(Error::new(FyStatus::Unsupported, "test hooks run only in tests (FLUTTER_TEST or FYNKEL_TEST_HOOKS set)"))
}

/// For tests only (see `fy_view_fail_worker`): destroys the GPU device, as
/// a driver reset would. Every view shares it, so soon after,
/// `fy_view_frame` returns `FY_STATUS_DEVICE_LOST` for each; a view made
/// after has a new one.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_lose_device(view: *const FyView) -> FyStatus {
    guard(|| {
        test_hooks()?;
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        view.worker.lose_device();
        Ok(())
    })
}

/// For tests only: makes the view's render worker panic before its next
/// frame, as a bug in it would, so soon after `fy_view_frame` returns
/// `FY_STATUS_WORKER_FAILED`. Runs only in a test process (`FLUTTER_TEST`
/// or `FYNKEL_TEST_HOOKS` set); `FY_STATUS_UNSUPPORTED` otherwise.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_fail_worker(view: *const FyView) -> FyStatus {
    guard(|| {
        test_hooks()?;
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        view.worker.panic();
        Ok(())
    })
}

/// What a view did since it was made.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FyViewStats {
    /// Scenes handed to the render worker by `fy_view_frame`.
    pub posted: u64,
    /// Scenes drawn.
    pub rendered: u64,
    /// Scenes replaced by a newer one before they were drawn.
    pub coalesced: u64,
    /// Scenes not drawn because the target had nowhere to draw.
    pub skipped: u64,
    /// Apple: new frames taken by Flutter (`fy_apple_acquire`). Android:
    /// frames given to the surface. 0 for headless views.
    pub presented: u64,
    /// Apple: frames finished but never shown, because newer ones were
    /// waiting. 0 for other views (Android's queue is the OS's).
    pub dropped: u64,
    /// GPU memory the world's textures hold, in bytes: every mip level, as
    /// stored, once however many views the world has. Up to date as of the
    /// view's last frame drawn.
    pub texture_bytes: u64,
    /// GPU memory the world's meshes hold, in bytes.
    pub mesh_bytes: u64,
    /// GPU memory the sky's light holds for this view, in bytes: 0 for no
    /// sky or a plain one.
    pub sky_bytes: u64,
    /// Microseconds the view's render thread spent on scenes, from taking
    /// one to being done with it: busy, against waiting for the next. On
    /// Android it includes waiting for the GPU to finish each frame.
    pub busy_micros: u64,
}

/// # Safety
///
/// `view` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_stats(view: *const FyView, out: *mut FyViewStats) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        let w = view.worker.stats();
        let mut stats = FyViewStats {
            posted: w.posted,
            rendered: w.rendered,
            coalesced: w.coalesced,
            skipped: w.skipped,
            texture_bytes: w.texture_bytes,
            mesh_bytes: w.mesh_bytes,
            sky_bytes: w.sky_bytes,
            busy_micros: w.busy_micros,
            ..Default::default()
        };
        #[cfg(target_vendor = "apple")]
        if let Output::Apple(presenter) = &view.output {
            let ring = presenter.stats();
            (stats.presented, stats.dropped) = (ring.presented, ring.dropped);
        }
        #[cfg(target_vendor = "apple")]
        if let Output::AppleLayer(presenter) = &view.output {
            stats.presented = presenter.presented();
        }
        #[cfg(target_os = "android")]
        if let Some(presenter) = view.android() {
            stats.presented = presenter.presented();
        }
        // SAFETY: per this function's contract.
        unsafe { write(out, stats, "out") }
    })
}

/// `fy_apple_acquire`: a surface was written to `out_surface`.
pub const FY_APPLE_SHOWN: u32 = 1;

/// `fy_apple_acquire`: another finished frame is waiting; ask Flutter for
/// one more frame.
pub const FY_APPLE_MORE: u32 = 2;

/// For Flutter's raster thread: the IOSurface to show now. Returns
/// `FY_APPLE_SHOWN` (plus `FY_APPLE_MORE` when another frame waits) and
/// writes the surface, retained once for the caller, to `out_surface`; or 0
/// with nothing written, when there's no frame yet or `view` isn't an Apple
/// view. Never blocks on the GPU and allocates nothing.
///
/// # Safety
///
/// `view` is null or live; `out_surface` is null or writable. The caller
/// releases the surface (`CFRelease`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_apple_acquire(view: *const FyView, out_surface: *mut *mut c_void) -> u32 {
    #[cfg(target_vendor = "apple")]
    {
        let acquire = || -> u32 {
            // SAFETY: per this function's contract.
            let Ok(view) = (unsafe { handle(view, "view") }) else { return 0 };
            let Output::Apple(presenter) = &view.output else { return 0 };
            if out_surface.is_null() {
                return 0;
            }
            let Some(shown) = presenter.acquire() else { return 0 };
            let more = if shown.more { FY_APPLE_MORE } else { 0 };
            // SAFETY: checked non-null above; the caller promises it's
            // writable.
            unsafe { out_surface.write(shown.into_raw()) };
            FY_APPLE_SHOWN | more
        };
        // The raster thread is Flutter's: nothing may unwind into it.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(acquire)).unwrap_or(0)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = (view, out_surface);
        0
    }
}

/// Called on a GPU thread each time an Apple view finishes a frame.
pub type FyFrameReady = Option<unsafe extern "C" fn(userdata: *mut c_void)>;

/// Sets the function an Apple view calls, on a GPU thread, each time a
/// frame finishes (null for none). Waits for a call in progress, so once
/// this returns the old `userdata` is no longer used.
///
/// # Safety
///
/// `view` is null or live; `function` must be safe to call with `userdata`
/// from any thread until it is replaced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_apple_set_frame_ready(
    view: *const FyView,
    function: FyFrameReady,
    userdata: *mut c_void,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        #[cfg(target_vendor = "apple")]
        if let Output::Apple(presenter) = &view.output {
            // SAFETY: promised by this function's caller.
            unsafe { presenter.set_frame_ready(function, userdata) };
            return Ok(());
        }
        let _ = (view, function, userdata);
        Err(Error::new(FyStatus::Unsupported, "not an Apple view"))
    })
}

/// Draws an Apple layer view's next frames to `layer` (a `CAMetalLayer`)
/// at `width` × `height` pixels, after a frame in progress on the old one.
/// For the Flutter plugin, on the main thread.
///
/// # Safety
///
/// `view` is null or live; `layer` is null or a live `CAMetalLayer`, kept
/// alive until `fy_apple_layer_detach` or the next attach returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_apple_layer_attach(
    view: *const FyView,
    layer: *mut c_void,
    width: u32,
    height: u32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        #[cfg(target_vendor = "apple")]
        if let Output::AppleLayer(presenter) = &view.output {
            let layer = std::ptr::NonNull::new(layer).ok_or_else(|| Error::null("layer"))?;
            // SAFETY: promised by this function's caller.
            return fynkel_present::autorelease_pool(|| unsafe { presenter.attach(layer, width, height) })
                .map_err(|e| Error::new(FyStatus::NoGpu, e));
        }
        let _ = (view, layer, width, height);
        Err(Error::new(FyStatus::Unsupported, "not an Apple layer view"))
    })
}

/// Stops an Apple layer view drawing to its layer, after a frame in
/// progress: the layer may then go. Frames are skipped until the next
/// `fy_apple_layer_attach`.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_apple_layer_detach(view: *const FyView) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        #[cfg(target_vendor = "apple")]
        if let Output::AppleLayer(presenter) = &view.output {
            presenter.detach();
            return Ok(());
        }
        let _ = view;
        Err(Error::new(FyStatus::Unsupported, "not an Apple layer view"))
    })
}
