//! The forward renderer: 4× MSAA, reversed-Z, in passes.
//!
//! A frame has two halves. [`Renderer::prepare`] is plain CPU work: it
//! turns the scene into per-draw instance data and the frame uniforms, in
//! buffers it reuses, so a steady frame allocates nothing.
//! [`Renderer::encode`] writes that where the GPU reads it (see
//! `frame_ring`) and records the passes, in order:
//!
//! Lights: every sun lights every object. Point and spot lights go into a
//! list for the frame, the ones in view, nearest the camera first, up to
//! [`Renderer::MAX_LIGHTS`]. The view is cut into cells (`clusters`), each
//! holding every one of those that reaches it; a pixel loops over its
//! cell's.
//!
//! Shadows (`shadows`): the first sun with shadows casts them through
//! [`ShadowLevel::cascades`](crate::ShadowLevel) maps fitted to the view,
//! and the spot lights with shadows nearest the camera through a map each,
//! up to the level's [`ShadowLevel::spot_maps`](crate::ShadowLevel).
//!
//! 0. The shadow maps: each one's casters' depth, as its light sees them.
//!    Then light ambient occlusion, when the camera has it (`ao`).
//! 1. The scene: every draw, then the debug lines on top. With
//!    [`Quality::hdr`] or bloom it is drawn into a half-float image (the
//!    scene image); otherwise, straight into the view. When a see-through
//!    surface's shader reads what's behind it, in two: the solid scene
//!    and the sky, its depth kept and its colour resolved into an image
//!    for it to read; then that put back, and the see-through and the
//!    lines drawn over it (behind.wgsl).
//! 2. Bloom, when the camera has it and there is a scene image (`bloom`).
//!    The image's alpha says how much of each pixel is light, which may
//!    glow: what's drawn and lit, and an image sky. Colours shown as
//!    they're picked (the clear colour, a gradient sky) are 0, so a white
//!    sky doesn't glow.
//! 3. The last pass, when there is a scene image: the image onto the view
//!    (`finish.wgsl`), with bloom's glow added. With screen effects, into
//!    an image the view's size instead (made even at low: `effects`).
//! 4. The camera's screen effects, in order, each over what the one before
//!    made, the last onto the view (effect.wgsl).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3, Vec4};

use crate::ao::{self, AoImages};
use crate::assets::{self, Assets, Kit, PipelineMaker, SharedAssets};
use crate::bloom::BloomChain;
use crate::clusters::{Clusters, Grid, Reach, TILES_X, TILES_Y};
use crate::effects::EffectImages;
use crate::frame_ring::{FrameRing, Parts};
use crate::gpu::Gpu;
use crate::gpu_time::{GpuTimer, GpuTimes, Part};
use crate::scene::{
    AmbientOcclusion, Blend, Bloom, ColorSpace, LightKind, MaterialTextures, MeshKey, Quality, RenderScene, ShaderKey,
    ShadowFilter, SkyKind, TextureKey,
};
use crate::shader::CustomShader;
use crate::shadows::{self, Caster, MAX_CASCADES, MAX_MAPS, MAX_SPOT_MAPS, ShadowMaps};
use crate::sky::{self, Environment, SPECULAR_MIPS};
use crate::texture::OwnTextures;
use fynkel_mesh::{MeshData, Vertex};

mod encode;
mod pipelines;
mod prepare;

use pipelines::*;
pub(crate) use pipelines::{SKIN_ATTRIBUTES, skin_buffer};
use prepare::*;

/// What one frame did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Objects drawn.
    pub draws: u32,
    /// GPU draw calls: one per mesh, since draws sharing a mesh are
    /// instanced together.
    pub draw_calls: u32,
    /// Draws skipped because their mesh key wasn't uploaded.
    pub missing_meshes: u32,
    /// Draws skipped because they're wholly outside the camera's view.
    pub culled: u32,
    /// Debug lines drawn.
    pub lines: u32,
    /// Particles drawn: every particle system's in view.
    pub particles: u32,
    /// Point and spot lights in the frame's list: in view, up to
    /// [`Renderer::MAX_LIGHTS`].
    pub lights: u32,
    /// Lights left out: suns past [`Renderer::MAX_SUNS`], and point and spot
    /// lights in view past [`Renderer::MAX_LIGHTS`] (the furthest from the
    /// camera).
    pub lights_left_out: u32,
    /// Shadow maps drawn: the sun's cascades and the spot lights'.
    pub shadow_maps: u32,
    /// Draws into shadow maps: one per caster per map it falls in.
    pub shadow_casters: u32,
    /// Joints bending skinned meshes: every skeleton's in the scene.
    pub joints: u32,
    /// GPU memory of the shadow maps the view holds, shared with the kit's
    /// other views or not.
    pub shadow_bytes: u64,
    /// GPU memory the view holds for the sky's light, and an image sky's
    /// pixels.
    pub sky_bytes: u64,
    /// Whether the frame works out the sky's light: a new gradient or
    /// image. One turned or brightened isn't new.
    pub sky_light_made: bool,
    /// Whether a see-through surface in view reads what's behind it
    /// (`scene_depth`, `scene_color`), so the frame keeps the solid
    /// scene's depth and colour for it.
    pub behind: bool,
    /// GPU memory the view holds for that: kept a second after the last
    /// frame that read it, 0 when no frame has for longer.
    pub behind_bytes: u64,
    /// Screen effects run.
    pub effects: u32,
    /// GPU memory the view holds for the pictures they pass between, kept
    /// as `behind_bytes` is.
    pub effect_bytes: u64,
    /// Pipelines for materials' own shaders still being made on threads
    /// of their own, for a renderer that doesn't wait for them
    /// ([`Renderer::set_wait_for_shaders`]): their draws are left out
    /// till they're made. As of the frame before.
    pub shaders_waiting: u32,
}

/// `Frame` in forward.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    sky: SkyUniform,
    camera_position: [f32; 4],
    ambient: [f32; 4],
    /// x: how many of `suns` are used. y: the one that casts shadows, or
    /// [`NO_SHADOWS`]. z: 1 if lamps give highlights
    /// ([`Quality::lamp_highlights`]).
    counts: [u32; 4],
    suns: [SunUniform; MAX_SUNS],
    lights: [LightUniform; MAX_LIGHTS],
    shadows: ShadowUniforms,
    /// x, y: cells across and down per pixel. z, w: a point's slice is its
    /// depth along the view (its logarithm, for a perspective camera)
    /// times z plus w.
    cells: [f32; 4],
    /// xyz: where the camera is. w: 1 for a perspective camera, 0 for an
    /// orthographic one.
    cells_eye: [f32; 4],
    /// xyz: the way the camera looks.
    cells_forward: [f32; 4],
    /// rgb: what fog fades into, as seen. w: 1 if there's fog.
    fog_color: [f32; 4],
    /// x: where it starts. y: 1 / how far it takes to be whole. z: 1 / the
    /// height it halves over, or 0.
    fog: [f32; 4],
    /// x: multiplies the sky's light. y: the specular cube's last mip.
    sky_light: [f32; 4],
    /// x, y: 1 / the view's width and height, in pixels.
    view_size: [f32; 4],
    /// x: the game's time ([`RenderScene::time`]), y: its step this frame.
    time: [f32; 4],
    /// What the sky's cubes' red, green and blue are worth: a gradient's
    /// zenith, horizon and ground colours; an image's intensity, alone.
    sky_colors: [[f32; 4]; 3],
}

/// `Sky` in forward.wgsl and sky.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SkyUniform {
    inverse_view_proj: [[f32; 4]; 4],
    /// rgb: the colours. zenith's w: 1 gradient, 2 image. horizon's: the
    /// image's intensity. ground's: its turn.
    zenith: [f32; 4],
    horizon: [f32; 4],
    ground: [f32; 4],
}

/// `Shadows` in forward.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShadowUniforms {
    /// World to each cascade's map.
    cascades: [[[f32; 4]; 4]; MAX_CASCADES],
    /// World to each spot map.
    spots: [[[f32; 4]; 4]; MAX_SPOT_MAPS],
    /// xyz: how far along the view each cascade ends. w: where the shadows
    /// start fading out.
    splits: [f32; 4],
    /// x: cascades, y: the filter (0 hard, 1 soft, 2 softer), z: 1 / the
    /// cascades' size, w: 1 / the spot maps'.
    params: [f32; 4],
    /// Each cascade's texel, in metres.
    texels: [f32; 4],
    /// xyz: the way the camera looks. w: how far the shadows reach.
    camera_forward: [f32; 4],
    /// xyz: where the camera is (an orthographic one's too).
    camera_at: [f32; 4],
}

/// `FrameUniforms::counts.y` when no sun casts shadows.
const NO_SHADOWS: u32 = u32::MAX;

/// `Sun` in forward.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SunUniform {
    /// The unit vector towards the sun.
    to_sun: [f32; 4],
    color: [f32; 4],
}

/// `Lamp` in forward.wgsl: a point or spot light.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightUniform {
    /// w: 1 / range².
    position: [f32; 4],
    color: [f32; 4],
    /// The unit vector it shines along (a spot light's).
    direction: [f32; 4],
    /// x, y: the cone's fade as a scale and an offset on the cosine of the
    /// angle from the axis. A point light's (0, 1) lights every way.
    /// z: its spot map, or -1. w: a texel of that map, per metre away.
    cone: [f32; 4],
}

const MAX_SUNS: usize = 4;
pub(crate) const MAX_LIGHTS: usize = 64;

/// The per-draw vertex data (instance step), locations 4–13.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    /// The model matrix's rows, but its last (0, 0, 0, 1). The shader works
    /// out the normals' matrix and whether it mirrors.
    model: [[f32; 4]; 3],
    base_color: [f32; 4],
    /// Metallic, roughness, alpha cutoff, and 0.
    params: [f32; 4],
    /// Emission; w: 1 if shadows fall on it, 0 if not.
    emissive: [f32; 4],
    /// UV scale (xy), then offset (zw).
    uv: [f32; 4],
    /// The object's own numbers ([`Draw::own`]), for a material's shader.
    own: [f32; 4],
    /// Where the material's values start among the joints: a shader's.
    values: u32,
    /// Its outline's linear colour, and how wide it is, in metres.
    outline: [f32; 4],
}

/// A particle, as particles.wgsl reads it (`FyParticleIn`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleInstance {
    /// Its centre, and its size across.
    center_size: [f32; 4],
    /// Linear, times the material's base colour.
    color: [f32; 4],
    /// How it moves, and its turn.
    velocity_turn: [f32; 4],
    /// UV scale and offset: the material's and its frame's.
    uv: [f32; 4],
    /// Metallic, roughness, softness, and how it's drawn.
    params: [f32; 4],
    /// The material's emission, and 1: shadows fall on it.
    emissive: [f32; 4],
    /// How far through its life, its own number, and a stretched one's
    /// length and speed scales.
    particle: [f32; 4],
    /// Where the material's values start, and room to the next.
    values: [u32; 4],
}

/// `FyParticleIn`'s attributes, after a mesh particle's vertex.
const PARTICLE_ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
    4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
    8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Uint32,
];

/// A trail's next point, as particles.wgsl reads it (`FyTrailEnd`): the
/// same instances, one on.
const TRAIL_END_ATTRIBUTES: [wgpu::VertexAttribute; 4] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 96, shader_location: 3 },
];

/// A particle system's batch: drawn as meshes, squares or a trail's
/// strip, lit or not, and soft or not; or flat text's letters, over
/// everything or not.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ParticleLook {
    mesh: bool,
    trail: bool,
    lit: bool,
    soft: bool,
    glyphs: bool,
    on_top: bool,
    sprites: bool,
}

/// A particle pipeline's key: its shader's hash (`None`, the built-in
/// one), how it's drawn, and the frame's lighting it's made for.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ParticleKey {
    shader: Option<u64>,
    look: ParticleLook,
    additive: bool,
    textured: bool,
    shadowed: bool,
    sky_lit: bool,
}

/// particles.wgsl's `params.w` for a quad from its own axes: a letter of
/// flat text, or a sprite's.
const GLYPH_MODE: f32 = 6.0;

/// Marks a particle draw's index among the blended draws'.
const PARTICLES: u32 = 1 << 31;

/// A debug line's end, as lines.wgsl reads it.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineVertex {
    position: [f32; 3],
    /// Linear RGBA.
    color: [f32; 4],
}

pub(crate) const FRAME_UNIFORMS_SIZE: u64 = std::mem::size_of::<FrameUniforms>() as u64;

/// The multisampled colour and depth targets, and the scene image the
/// colour resolves into above low quality. Remade when the size, the
/// samples or the image's format ([`Quality::image_format`]) changes.
struct Targets {
    width: u32,
    height: u32,
    /// Samples a pixel: [`Quality::samples`], as the device draws it.
    samples: u32,
    /// The multisampled colour, or `None` with one sample, which draws
    /// straight into what it would resolve into.
    color: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    image: Option<SceneImage>,
    /// Ambient occlusion's images, made the first frame a camera has it.
    ao: Option<AoImages>,
    /// What's behind, made the first frame a shader reads it.
    behind: Option<BehindImages>,
    /// The solid scene's depth for soft particles, when nothing reads
    /// what's behind.
    soft: Option<SoftDepth>,
}

/// Which images a bind group reads, for knowing when it must be made again.
static NEXT_IMAGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The solid scene's depth at one sample, drawn in a pass of its own before
/// the scene, for soft particles to read. Getting it as what's behind does
/// (the scene in two passes, its samples kept and put back) moves whole
/// images through memory, which costs a tiled GPU most: 14 ms a frame on an
/// A10 at medium (docs/stage-14-findings.md, RN11). Cut-outs aren't in it,
/// as in ambient occlusion's.
struct SoftDepth {
    id: u64,
    depth: wgpu::TextureView,
}

impl SoftDepth {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("soft particles' depth"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self { id: NEXT_IMAGES.fetch_add(1, std::sync::atomic::Ordering::Relaxed), depth }
    }

    /// Its GPU memory, `width` × `height`.
    fn bytes(width: u32, height: u32) -> u64 {
        u64::from(width) * u64::from(height) * 4
    }
}

/// What's behind see-through surfaces, for shaders that read it: the solid
/// scene's depth, multisampled and kept past its pass, and one sample of
/// it; its colour, resolved; and the bind groups the passes read them
/// through.
struct BehindImages {
    /// Which images these are, for the bind group that reads them.
    id: u64,
    msaa_depth: wgpu::TextureView,
    depth: wgpu::TextureView,
    color: wgpu::TextureView,
    /// The depth copy's input, and the colour put back's.
    copy: wgpu::BindGroup,
    restore: wgpu::BindGroup,
    /// The depth copy, as screen effects read it.
    effect_depth: wgpu::BindGroup,
}

impl BehindImages {
    fn new(kit: &Kit, width: u32, height: u32, color_format: wgpu::TextureFormat, samples: u32) -> Self {
        let device = &kit.gpu.device;
        let make = |label, format, sample_count| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let msaa_depth = make("msaa depth, kept", DEPTH_FORMAT, samples);
        let depth = make("scene depth", DEPTH_FORMAT, 1);
        let color = make("scene colour", color_format, 1);
        let one = |label, layout, view| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }],
            })
        };
        Self {
            id: NEXT_IMAGES.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            copy: one("scene depth", if samples > 1 { &kit.msaa_depth_layout } else { &kit.image_layout }, &msaa_depth),
            restore: one("what's behind", &kit.image_layout, &color),
            effect_depth: one("scene depth, for effects", &kit.effects.depth_layout, &depth),
            msaa_depth,
            depth,
            color,
        }
    }

    /// Their GPU memory, `width` × `height` with colour in `format`.
    fn bytes(width: u32, height: u32, format: wgpu::TextureFormat, samples: u32) -> u64 {
        let color = format.block_copy_size(None).unwrap_or(4);
        u64::from(width) * u64::from(height) * u64::from(4 * samples + 4 + color)
    }
}

/// The image the scene is drawn into, its format, and the bind group the
/// last pass reads it through.
struct SceneImage {
    format: wgpu::TextureFormat,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    /// Bloom's images, made the first frame a camera has bloom.
    bloom: Option<BloomChain>,
}

/// Which faces a pipeline draws, and which way round.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Faces {
    /// Fronts only: counter-clockwise on screen.
    Front,
    /// Fronts only, for a mirrored object: mirroring turns its triangles
    /// clockwise on screen, so clockwise is the front.
    Mirrored,
    /// Both sides (a double-sided material), the back lit as the front.
    Both,
}

impl Faces {
    const ALL: [Self; 3] = [Self::Front, Self::Mirrored, Self::Both];

    fn of(material: &crate::PbrMaterial, mirrored: bool) -> Self {
        match (material.double_sided, mirrored) {
            (true, _) => Self::Both,
            (false, false) => Self::Front,
            (false, true) => Self::Mirrored,
        }
    }
}

/// How a draw covers what's behind it, in the order they're drawn: solid
/// first, then blended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Cover {
    Opaque,
    /// Opaque with an alpha cutoff.
    Cutout,
    Transparent,
    Additive,
}

impl Cover {
    const COUNT: usize = 4;

    fn of(material: &crate::PbrMaterial) -> Self {
        match material.blend {
            Blend::Opaque if material.alpha_cutoff > 0.0 => Self::Cutout,
            Blend::Opaque => Self::Opaque,
            Blend::Transparent => Self::Transparent,
            Blend::Additive => Self::Additive,
        }
    }

    /// Drawn over what's behind, back to front, after the sky.
    fn blended(self) -> bool {
        self >= Self::Transparent
    }
}

/// What a forward pipeline is made for: the shader's work left out where a
/// frame or a material doesn't use it (hardening R11).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Variant {
    cover: Cover,
    faces: Faces,
    /// Some light casts shadows this frame.
    shadowed: bool,
    /// The material has a texture.
    textured: bool,
    /// The sky lights the scene from its cubes (a gradient or an image).
    sky_lit: bool,
    /// Ambient occlusion darkens it.
    ao: bool,
    /// A skeleton bends the mesh.
    skinned: bool,
}

impl Variant {
    const COUNT: usize = Cover::COUNT * Faces::ALL.len() * 2 * 2 * 2 * 2 * 2;

    /// Its index in `Pipelines::forward`.
    fn index(self) -> usize {
        (((((self.cover as usize * Faces::ALL.len() + self.faces as usize) * 2 + usize::from(self.shadowed)) * 2
            + usize::from(self.textured))
            * 2
            + usize::from(self.sky_lit))
            * 2
            + usize::from(self.ao))
            * 2
            + usize::from(self.skinned)
    }
}

/// One instanced draw call, as `prepare` left it for `encode`: `count`
/// instances of `mesh` starting at instance `first`, with one set of
/// textures, covering what's behind one way, facing one way, bent by
/// skeletons or not.
#[derive(Clone, Copy)]
struct Batch {
    cover: Cover,
    faces: Faces,
    skinned: bool,
    /// Its draws have outlines, drawn after the solid scene.
    outlined: bool,
    /// The material's own shader, or `None` for the built-in one.
    shader: Option<ShaderKey>,
    textures: OwnTextures,
    mesh: MeshKey,
    /// Which of `mesh`'s submeshes.
    submesh: u32,
    /// A particle system's particles, from `first` in the frame's
    /// particles, rather than draws of `mesh` (a mesh particle's mesh).
    particle: Option<ParticleLook>,
    first: u32,
    count: u32,
}

/// Draws into one shadow map: `count` casters of `mesh` from caster
/// `first`, opaque or cut out with `textures`. `map` is a cascade's index,
/// or [`MAX_CASCADES`] plus a spot map's.
#[derive(Clone, Copy)]
struct CasterBatch {
    map: u8,
    /// A cut-out's own shader and its textures, which cut it.
    own: Option<Look>,
    cutout: bool,
    skinned: bool,
    textures: MaterialTextures,
    mesh: MeshKey,
    submesh: u32,
    first: u32,
    count: u32,
}

/// A caster's sort key: its map, a cut-out's own shader, cut out or not,
/// skinned or not, its textures (a cut-out's only) and its mesh. Within a
/// map, those with own shaders come last.
type CasterKey = (u8, Option<Look>, bool, bool, MaterialTextures, MeshPart);

/// A mesh's key and one of its submeshes.
type MeshPart = (u64, u32);

/// A draw's own shader, if it has one, and its textures.
type Look = (Option<ShaderKey>, OwnTextures);

/// An outline pipeline's key: its shader's hash (`None`, the built-in
/// one), mirrored, skinned, and with normals of its own.
type OutlineKey = (Option<u64>, bool, bool, bool);

/// A pipeline of a material's own shader being made on a thread of its
/// own: its key in its map.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Building {
    Own(u64, usize),
    Shadow(u64, bool, bool, bool),
    Depth(u64, bool),
    Outline(OutlineKey),
    Effect(u64),
    Particles(ParticleKey),
}

/// The pipeline under `key` in `map`, if it's made.
fn made<K: Eq + std::hash::Hash>(
    map: &Mutex<HashMap<K, wgpu::RenderPipeline>>,
    key: &K,
) -> Option<wgpu::RenderPipeline> {
    map.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
}

/// An error scope's error: on native backends it's ready at once.
fn now(pop: impl std::future::Future<Output = Option<wgpu::Error>>) -> Option<wgpu::Error> {
    let mut pop = std::pin::pin!(pop);
    match pop.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(error) => error,
        std::task::Poll::Pending => None,
    }
}

/// The pipelines that draw into one target format, shared by every
/// renderer on a device that draws into it ([`Kit`]).
///
/// Each is made the first time a frame draws with it: a game uses one or
/// two of the forward pipelines, and the view's format draws the scene only
/// at low quality.
pub(crate) struct Pipelines {
    /// The format they draw into.
    format: wgpu::TextureFormat,
    /// The samples a pixel of the scene they draw into has.
    samples: u32,
    maker: PipelineMaker,
    device: wgpu::Device,
    frame_layout: wgpu::BindGroupLayout,
    textures_layout: wgpu::BindGroupLayout,
    own_textures_layouts: [wgpu::BindGroupLayout; crate::MAX_TEXTURES],
    maps_layout: wgpu::BindGroupLayout,
    /// For shaders that read what's behind: the maps' and its own.
    behind_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    msaa_depth_layout: wgpu::BindGroupLayout,
    sky_light_layout: wgpu::BindGroupLayout,
    sky_image_layout: wgpu::BindGroupLayout,
    bloom_layout: wgpu::BindGroupLayout,
    forward_shader: OnceLock<(wgpu::ShaderModule, wgpu::PipelineLayout)>,
    /// One for each [`Variant`], at its index.
    forward: [OnceLock<wgpu::RenderPipeline>; Variant::COUNT],
    /// Materials' own shaders, by their code's hash: each one's module
    /// and layout, and its pipelines by variant, made as they're drawn.
    own_shaders: Mutex<HashMap<u64, (wgpu::ShaderModule, wgpu::PipelineLayout)>>,
    own: Mutex<HashMap<(u64, usize), wgpu::RenderPipeline>>,
    /// Their shadows' pipelines, by hash, spot or cascade, skinned, and
    /// cut out.
    own_shadows: Mutex<HashMap<(u64, bool, bool, bool), wgpu::RenderPipeline>>,
    /// Ambient occlusion's depth of those that move their vertices, by
    /// hash and skinned.
    own_depths: Mutex<HashMap<(u64, bool), wgpu::RenderPipeline>>,
    /// Outlines' pipelines, by their shader's hash (`None`, the built-in
    /// one), mirrored, skinned, and with normals of their own.
    outlines: Mutex<HashMap<OutlineKey, wgpu::RenderPipeline>>,
    own_view_layout: wgpu::BindGroupLayout,
    lines: OnceLock<wgpu::RenderPipeline>,
    /// Particles' modules, the built-in material's (`None`) and materials'
    /// own shaders' by hash, and their pipelines.
    particle_modules: Mutex<HashMap<Option<u64>, wgpu::ShaderModule>>,
    particles: Mutex<HashMap<ParticleKey, wgpu::RenderPipeline>>,
    /// The sky, behind everything.
    sky: OnceLock<wgpu::RenderPipeline>,
    /// What's behind (behind.wgsl): one sample of the solid scene's depth
    /// copied out, and its colour put back.
    behind_shader: OnceLock<wgpu::ShaderModule>,
    copy_depth: OnceLock<wgpu::RenderPipeline>,
    restore: OnceLock<wgpu::RenderPipeline>,
    /// The last pass, onto a view: for sRGB formats only (a view's).
    finish: OnceLock<wgpu::RenderPipeline>,
    /// The same, adding bloom's glow.
    finish_bloom: OnceLock<wgpu::RenderPipeline>,
    /// Screen effects', by their code's hash.
    effect_layout: wgpu::BindGroupLayout,
    effect_depth_layout: wgpu::BindGroupLayout,
    effects: Mutex<HashMap<u64, wgpu::RenderPipeline>>,
    /// Own shaders' pipelines being made on threads of their own.
    building: Mutex<HashSet<Building>>,
    /// Those the GPU refused when made on a thread of their own: never
    /// drawn, nor made again.
    failed: Mutex<HashSet<Building>>,
}

/// Draws [`RenderScene`]s into textures of one format: a view's. What it
/// draws with, it shares: the pipelines with every renderer on its device,
/// the meshes and textures with every renderer of its world.
pub struct Renderer {
    kit: Arc<Kit>,
    format: wgpu::TextureFormat,
    pipelines: Arc<Pipelines>,
    /// The pipelines that draw into the scene image, got when its format
    /// is first used.
    image_pipelines: Option<Arc<Pipelines>>,
    /// The samples the scene is drawn with this frame: its quality's, as
    /// far as the device draws them.
    samples: u32,
    /// Each pass's time on the GPU, when asked for.
    timer: GpuTimer,
    /// Whether a frame waits for its own shaders' pipelines to be made,
    /// or goes without their draws till they are.
    wait_for_shaders: bool,
    assets: SharedAssets,
    frame_data: FrameRing,
    targets: Option<Targets>,

    // Built by `prepare`, consumed by `encode`. Reused every frame.
    uniforms: FrameUniforms,
    /// The scene's joints, as the shaders read them (`skin.wgsl`).
    joints: Vec<[[f32; 4]; 3]>,
    /// Whether each draw is drawn bent: its mesh has joints and its skin
    /// is in the scene's.
    skinned: Vec<bool>,
    /// Each draw's shader and textures, or `None` when its shader isn't
    /// there (not uploaded yet, or freed), which draws nothing.
    looks: Vec<Option<Look>>,
    /// Whether each draw's shader moves its vertices, so its shadow is
    /// drawn with it.
    moving: Vec<bool>,
    /// Whether a see-through draw in view reads what's behind it; whether
    /// the images for it are kept, and for how many frames none has.
    behind: bool,
    behind_kept: bool,
    behind_idle: u32,
    /// Whether a soft particle in view reads the solid scene's depth;
    /// whether its image is kept, and for how many frames none has.
    soft: bool,
    soft_kept: bool,
    soft_idle: u32,
    /// Where the materials' values start among the joints.
    values_at: u32,
    instances: Vec<Instance>,
    /// The solid draws' batches, then the blended ones' from `blended_from`.
    batches: Vec<Batch>,
    blended_from: usize,
    line_vertices: Vec<LineVertex>,
    /// The particle systems' particles, as their batches draw them.
    particle_instances: Vec<ParticleInstance>,
    /// A transparent system's particles in the order they're drawn: each
    /// one's depth and index packed to sort as one number.
    particle_order: Vec<u64>,
    /// The scene's solid draws that are seen, each with its sort key,
    /// sorted.
    order: Vec<(SortKey, u32)>,
    /// Its blended ones, each with how far it is ahead of the camera,
    /// sorted furthest first.
    blended: Vec<([i32; 2], f32, SortKey, u32)>,
    /// Point and spot lights in view: how far each is from the camera
    /// (squared) and its index in the scene.
    in_view_lights: Vec<(f32, u32)>,
    /// The frame's point and spot lights, in the order of
    /// [`FrameUniforms::lights`].
    frame_lights: Vec<Reach>,
    /// Which of them light which cells of the view.
    clusters: Clusters,
    /// Draws that cast shadows: their bounding sphere and their index.
    shadow_candidates: Vec<(Vec3, f32, u32)>,
    /// Each caster in each map, sorted as they're drawn.
    caster_order: Vec<(CasterKey, u32)>,
    casters: Vec<Caster>,
    caster_batches: Vec<CasterBatch>,
    /// Each map's view matrix, at its index times the view stride.
    shadow_views: Vec<u8>,
    /// The cascades drawn this frame, and the spot maps.
    shadow_maps_drawn: (u8, u8),
    /// The maps this frame needs, by size and layers: the cascades, then
    /// the spot maps; `None` where it needs none.
    shadow_shape: [Option<(u32, u32)>; 2],
    shadow_maps: Option<ShadowMaps>,
    /// The maps' shape the next `encode` draws with, and for how many
    /// frames each kind has been using fewer than it holds.
    shadow_next: [(u32, u32); 2],
    shadow_idle: [u32; 2],
    clear: wgpu::Color,
    /// The sky this frame lights the scene with from cubes, and whether
    /// it's drawn behind everything.
    sky_next: Option<SkyKind>,
    sky_drawn: bool,
    /// This frame's bloom: the camera's, where the level draws it and it
    /// glows at all.
    bloom: Option<Bloom>,
    /// This frame's ambient occlusion, as `bloom`, and what its passes
    /// read.
    ao: Option<AmbientOcclusion>,
    ao_params: ao::Params,
    /// The cubes (and image) of the last such sky, kept until another.
    environment: Option<Environment>,
    /// What the scene is drawn at: the view's size, or with
    /// [`CameraView::pixels`], smaller.
    size: (u32, u32),
    /// The scene image's format this frame: the level's, or with the
    /// pixel look the view's even at low; `None`, drawn straight to the
    /// view.
    image_format: Option<wgpu::TextureFormat>,
    /// This frame's level: the scene's, or the device's.
    quality: Quality,
    /// The view's size.
    view_size: (u32, u32),
    /// This frame's screen effects' shaders, in order, and their uniforms,
    /// each at its stride ([`crate::effects::EffectKit::stride`]).
    effect_chain: Vec<ShaderKey>,
    effect_uniforms: Vec<u8>,
    /// Their own textures, as a material shader's are.
    effect_textures: Vec<OwnTextures>,
    /// Whether one reads the solid scene's depth, which the frame then
    /// keeps (in what's behind's images).
    effect_depth: bool,
    /// The pictures they pass between, and for how many frames none has
    /// run.
    effect_images: Option<EffectImages>,
    effects_idle: u32,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer").field("format", &self.format).field("size", &self.size).finish_non_exhaustive()
    }
}

pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// How many frames shadow maps a view stops using are kept: about a
/// second.
const KEEP_UNUSED_MAPS: u32 = 60;
pub(crate) const INSTANCE_SIZE: u64 = std::mem::size_of::<Instance>() as u64;

impl Renderer {
    /// The most suns that light a frame: the first ones in the scene.
    pub const MAX_SUNS: usize = MAX_SUNS;
    /// The most point and spot lights in a frame: the ones in view nearest
    /// the camera.
    pub const MAX_LIGHTS: usize = MAX_LIGHTS;

    /// A renderer for target views of `format`, which must be an sRGB
    /// format.
    ///
    /// sRGB is required so the MSAA resolve averages in linear light. A
    /// resolve into a plain format averages encoded values and darkens
    /// every edge. A plain-format texture or surface still works: create it
    /// with the sRGB format in `view_formats`, and pass that view
    /// (`format.add_srgb_suffix()`).
    ///
    /// # Panics
    ///
    /// If `format` isn't sRGB.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        Self::sharing(Assets::shared(&Kit::new(gpu)), format)
    }

    /// A renderer for target views of `format` (sRGB, as [`Renderer::new`]
    /// says) that draws `assets`, a world's, and shares them with the other
    /// renderers given them. Its pipelines come from their kit.
    pub fn sharing(assets: SharedAssets, format: wgpu::TextureFormat) -> Self {
        assert!(format.is_srgb(), "the renderer draws into sRGB views, got {format:?}");
        let kit = assets::lock(&assets).kit.clone();
        Self {
            pipelines: kit.pipelines(format, Quality::LOW.samples),
            frame_data: FrameRing::new(
                &kit.gpu.device,
                (kit.frame_layout.clone(), FRAME_UNIFORMS_SIZE),
                (
                    kit.shadows.view_layout.clone(),
                    kit.shadows.own_view_layout.clone(),
                    kit.shadows.view_stride,
                    MAX_MAPS as u64 * kit.shadows.view_stride,
                ),
                64 * INSTANCE_SIZE,
            ),
            quality: kit.device_quality().quality(),
            format,
            image_pipelines: None,
            samples: Quality::LOW.samples,
            wait_for_shaders: true,
            assets,
            targets: None,
            uniforms: FrameUniforms::zeroed(),
            joints: Vec::new(),
            skinned: Vec::new(),
            looks: Vec::new(),
            moving: Vec::new(),
            behind: false,
            behind_kept: false,
            behind_idle: 0,
            soft: false,
            soft_kept: false,
            soft_idle: 0,
            values_at: 0,
            instances: Vec::new(),
            batches: Vec::new(),
            blended_from: 0,
            line_vertices: Vec::new(),
            particle_instances: Vec::new(),
            particle_order: Vec::new(),
            order: Vec::new(),
            blended: Vec::new(),
            in_view_lights: Vec::new(),
            frame_lights: Vec::new(),
            clusters: Clusters::new(),
            shadow_candidates: Vec::new(),
            caster_order: Vec::new(),
            casters: Vec::new(),
            caster_batches: Vec::new(),
            shadow_views: vec![0; MAX_MAPS * kit.shadows.view_stride as usize],
            shadow_maps_drawn: (0, 0),
            shadow_shape: [None; 2],
            shadow_maps: None,
            shadow_next: [(1, 0); 2],
            shadow_idle: [0; 2],
            clear: wgpu::Color::BLACK,
            sky_next: None,
            sky_drawn: false,
            bloom: None,
            ao: None,
            ao_params: bytemuck::Zeroable::zeroed(),
            environment: None,
            size: (0, 0),
            image_format: None,
            view_size: (1, 1),
            effect_chain: Vec::new(),
            effect_uniforms: Vec::new(),
            effect_images: None,
            effects_idle: 0,
            effect_textures: Vec::new(),
            effect_depth: false,
            timer: GpuTimer::new(&kit.gpu.device, &kit.gpu.queue),
            kit,
        }
    }

    /// Whether a frame waits for the pipelines of materials' own shaders
    /// it hasn't drawn with before (the default), or draws without those
    /// objects while they're made on threads of their own, so a shader the
    /// driver hasn't seen doesn't hold the frame up: for a view on screen.
    /// [`FrameStats::shaders_waiting`] says how many are being made.
    pub fn set_wait_for_shaders(&mut self, wait: bool) {
        self.wait_for_shaders = wait;
    }

    /// Whether each pass's time on the GPU is measured ([`Renderer::gpu_times`]),
    /// for finding what a frame's GPU time goes on. Off by default: the
    /// times are copied back every frame, which costs a little and, on
    /// Apple GPUs, keeps some driver memory in use. Stays off where the
    /// device can't time passes.
    pub fn set_gpu_timing(&mut self, on: bool) {
        self.timer.set(on);
    }

    /// The GPU's time for each part of the frames timed so far, added up
    /// ([`Renderer::set_gpu_timing`]). A frame's times come back a frame or
    /// two after it's sent, once the GPU is done with it.
    pub fn gpu_times(&mut self) -> GpuTimes {
        self.timer.times()
    }

    /// The meshes and textures it draws, to share with another renderer
    /// ([`Renderer::sharing`]).
    pub fn assets(&self) -> &SharedAssets {
        &self.assets
    }

    /// The device's pipelines and shared GPU state it draws with.
    pub fn kit(&self) -> &Arc<Kit> {
        &self.kit
    }

    /// The target format this renderer draws into.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The quality the last frame was prepared at: its scene's, or the
    /// device's level's when the scene didn't choose.
    pub fn quality(&self) -> Quality {
        self.quality
    }

    /// Uploads `data` under `key` to its assets ([`Assets::upload_mesh`]).
    pub fn upload_mesh(&mut self, key: MeshKey, data: &MeshData) {
        let made = self.kit.make_mesh(data);
        assets::lock(&self.assets).insert_mesh(key, made);
    }

    /// Frees the mesh under `key` ([`Assets::remove_mesh`]).
    pub fn remove_mesh(&mut self, key: MeshKey) -> bool {
        assets::lock(&self.assets).remove_mesh(key)
    }

    /// Uploads an image under `key` to its assets ([`Assets::upload_texture`]).
    pub fn upload_texture(&mut self, key: TextureKey, space: ColorSpace, width: u32, height: u32, rgba: &[u8]) {
        let made = self.kit.make_texture(space, width, height, rgba);
        assets::lock(&self.assets).insert_texture(key, made);
    }

    /// Puts a font's atlas of distances under `key`
    /// ([`Assets::make_distances`]), replacing any there.
    pub fn upload_distances(&mut self, key: TextureKey, width: u32, height: u32, pixels: &[u8]) {
        let made = self.kit.make_distances(width, height, pixels);
        assets::lock(&self.assets).insert_texture(key, made);
    }

    /// Frees the textures under `key` ([`Assets::remove_texture`]).
    pub fn remove_texture(&mut self, key: TextureKey) -> bool {
        assets::lock(&self.assets).remove_texture(key)
    }

    /// Puts a material's own shader under `key` ([`Assets::upload_shader`]).
    pub fn upload_shader(&mut self, key: ShaderKey, shader: Arc<CustomShader>) {
        assets::lock(&self.assets).upload_shader(key, shader);
    }

    /// GPU memory held by its assets' textures ([`Assets::texture_bytes`]).
    pub fn texture_bytes(&self) -> u64 {
        assets::lock(&self.assets).texture_bytes()
    }

    /// GPU memory held by its assets' meshes ([`Assets::mesh_bytes`]).
    pub fn mesh_bytes(&self) -> u64 {
        assets::lock(&self.assets).mesh_bytes()
    }
}

impl Renderer {
    /// `prepare` + `encode` + submit, for callers that don't need the split.
    pub fn render(&mut self, scene: &RenderScene, target: &wgpu::TextureView, width: u32, height: u32) -> FrameStats {
        let stats = self.prepare(scene, width, height);
        let mut encoder =
            self.kit.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        self.encode(&mut encoder, target);
        self.kit.gpu.queue.submit([encoder.finish()]);
        stats
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::*;

    /// Stage 14's BR2: a pipeline the GPU refuses on the shader thread
    /// fails there, in scopes of its own, instead of reaching the device's
    /// handler (which loses the device for every view); it's never drawn,
    /// nor made again.
    #[test]
    fn a_pipeline_refused_on_the_shader_thread_is_dropped_and_the_device_lives() {
        let Ok(gpu) = Gpu::headless() else { return };
        let unscoped = Arc::new(AtomicUsize::new(0));
        gpu.device.on_uncaptured_error(Arc::new({
            let unscoped = Arc::clone(&unscoped);
            move |_| {
                unscoped.fetch_add(1, Ordering::SeqCst);
            }
        }));
        let pipelines = Arc::new(Pipelines::new(&Kit::new(&gpu), wgpu::TextureFormat::Rgba8UnormSrgb, 1));
        let calls = Arc::new(AtomicUsize::new(0));
        let make = {
            let calls = Arc::clone(&calls);
            move |p: &Pipelines| {
                calls.fetch_add(1, Ordering::SeqCst);
                let module = p.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("refused"),
                    source: wgpu::ShaderSource::Wgsl("not wgsl".into()),
                });
                let pipeline = p.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("refused"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: None,
                    multiview_mask: None,
                    cache: None,
                });
                p.effects.lock().unwrap().insert(7, pipeline.clone());
                pipeline
            }
        };
        assert!(pipelines.ready(false, Building::Effect(7), None, make.clone()).is_none());
        let start = Instant::now();
        while pipelines.waiting() > 0 {
            assert!(start.elapsed() < Duration::from_secs(10), "the shader thread finishes");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(unscoped.load(Ordering::SeqCst), 0, "no error reached the device's handler");
        assert!(made(&pipelines.effects, &7).is_none(), "the refused pipeline isn't kept");
        assert!(pipelines.ready(false, Building::Effect(7), None, make).is_none());
        assert_eq!(pipelines.waiting(), 0, "nor made again");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
