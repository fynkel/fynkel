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
    AnimationEvent, Blend, Camera, Color, Component, HdrData, ImageData, ImageError, Light, LightKind, Material,
    MaterialId, MeshData, MeshId, MeshParts, MeshRenderer, ModelData, ObjectId, Quat, Ray, Raycaster, TextureId,
    Transform, Vec2, Vec3, World,
};

use crate::error::{Error, FyStatus, guard, last_error};
use crate::extract::Extract;
use crate::worker::Worker;

/// Bumped on any change to the functions or types in this file.
pub const FY_ABI_VERSION: u32 = 31;

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
}

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
/// tangent (xyz, then w = 1 or -1), 2 per UV. Normals, tangents and UVs
/// may be null, and are then computed (see `fy_mesh_new`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FyMeshData {
    pub positions: *const f32,
    pub normals: *const f32,
    pub tangents: *const f32,
    pub uvs: *const f32,
    pub vertex_count: usize,
    /// Three per triangle, counter-clockwise seen from the front.
    pub indices: *const u32,
    pub index_count: usize,
}

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
pub struct FyMesh(Arc<MeshData>);

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
    unsafe { out.write(Box::into_raw(Box::new(FyMesh(Arc::new(data))))) };
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
                indices: items::<u32, 1>(m.indices, m.index_count, "indices")?
                    .ok_or_else(|| Error::null("indices"))?
                    .as_flattened(),
                skin: None,
            }
        };
        let data = MeshData::new(parts).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
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
        unsafe { write(out, Box::into_raw(Box::new(FyMesh(mesh))), "out") }
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

/// What an animator plays, from `fy_animator_get`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyAnimatorState {
    /// The clip last played or faded to, or -1 for none.
    pub clip: i32,
    /// Seconds into it.
    pub time: f32,
    /// Its length in seconds.
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
/// `fy_model_clip`) from its start: at once in place of what plays, with
/// `fade` 0, or fading in over `fade` seconds as the rest fade out. Fading
/// to the clip last played or faded to changes nothing; to one fading out,
/// it fades back in from where it is. An object that isn't a model
/// copy with clips is `FY_STATUS_NO_COMPONENT`.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_play(
    world: *const FyWorld,
    root: u64,
    clip: u32,
    fade: f32,
    looping: bool,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.play(object(root)?, clip, fade, looping)?)
    })
}

/// Stops every clip of `root`'s; its parts keep the pose they're in.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_stop(world: *const FyWorld, root: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.stop(object(root)?)?)
    })
}

/// How fast `root`'s clips play: 1 as made, 0 paused, negative backwards.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_speed_set(world: *const FyWorld, root: u64, speed: f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_animation_speed(object(root)?, speed)?)
    })
}

/// Moves `root`'s current clip to `time` seconds, kept within its length.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_time_set(world: *const FyWorld, root: u64, time: f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_animation_time(object(root)?, time)?)
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

/// Writes what `root` plays to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_get(world: *const FyWorld, root: u64, out: *mut FyAnimatorState) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let state = world.lock()?.animator_state(object(root)?)?;
        let state = FyAnimatorState {
            clip: state.clip.map_or(-1, |c| c as i32),
            time: state.time,
            length: state.length,
            speed: state.speed,
        };
        // SAFETY: per this function's contract.
        unsafe { write(out, state, "out") }
    })
}

/// Writes clip `clip`'s weight in `root`'s pose to `out`: 1 alone, 0 not
/// playing, between while it fades.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_weight(world: *const FyWorld, root: u64, clip: u32, out: *mut f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let weight = world.lock()?.clip_weight(object(root)?, clip)?;
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
    if width == 0 || height == 0 {
        return Err(Error::new(FyStatus::InvalidValue, format!("a view can't be {width} × {height}")));
    }
    let device = Device::current().map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
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
    let renderer = Renderer::sharing(gpu.assets.clone(), fynkel_present::Target::format(&target));
    let worker = Worker::start_for(gpu.clone(), renderer, target)
        .map_err(|e| Error::new(FyStatus::WorkerFailed, format!("couldn't start the render worker: {e}")))?;
    let view = FyView { world, gpu, clock: Mutex::default(), worker, output };
    // SAFETY: checked non-null above; the caller promises it's writable.
    unsafe { out.write(Box::into_raw(Box::new(view))) };
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
        if width == 0 || height == 0 {
            return Err(Error::new(FyStatus::InvalidValue, format!("a view can't be {width} × {height}")));
        }
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
