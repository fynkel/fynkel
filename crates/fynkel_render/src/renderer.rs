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

use std::collections::HashMap;
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
use crate::scene::{
    AmbientOcclusion, Blend, Bloom, ColorSpace, LightKind, MaterialTextures, MeshKey, Quality, RenderScene, ShaderKey,
    ShadowFilter, SkyKind, TextureKey,
};
use crate::shader::CustomShader;
use crate::shadows::{self, Caster, MAX_CASCADES, MAX_MAPS, MAX_SPOT_MAPS, ShadowMaps};
use crate::sky::{self, Environment, SPECULAR_MIPS};
use crate::texture::OwnTextures;
use fynkel_mesh::{MeshData, Vertex};

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
/// colour resolves into above low quality. Remade when the size or the
/// image's format ([`Quality::image_format`]) changes.
struct Targets {
    width: u32,
    height: u32,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    image: Option<SceneImage>,
    /// Ambient occlusion's images, made the first frame a camera has it.
    ao: Option<AoImages>,
    /// What's behind, made the first frame a shader reads it.
    behind: Option<BehindImages>,
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
}

impl BehindImages {
    fn new(kit: &Kit, width: u32, height: u32, color_format: wgpu::TextureFormat) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
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
        let msaa_depth = make("msaa depth, kept", DEPTH_FORMAT, Renderer::SAMPLES);
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
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            copy: one("scene depth", &kit.msaa_depth_layout, &msaa_depth),
            restore: one("what's behind", &kit.image_layout, &color),
            msaa_depth,
            depth,
            color,
        }
    }

    /// Their GPU memory, `width` × `height` with colour in `format`.
    fn bytes(width: u32, height: u32, format: wgpu::TextureFormat) -> u64 {
        let color = format.block_copy_size(None).unwrap_or(4);
        u64::from(width) * u64::from(height) * u64::from(4 * Renderer::SAMPLES + 4 + color)
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
    first: u32,
    count: u32,
}

/// A caster's sort key: its map, a cut-out's own shader, cut out or not,
/// skinned or not, its textures (a cut-out's only) and its mesh. Within a
/// map, those with own shaders come last.
type CasterKey = (u8, Option<Look>, bool, bool, MaterialTextures, u64);

/// A draw's own shader, if it has one, and its textures.
type Look = (Option<ShaderKey>, OwnTextures);

/// The pipelines that draw into one target format, shared by every
/// renderer on a device that draws into it ([`Kit`]).
///
/// Each is made the first time a frame draws with it: a game uses one or
/// two of the forward pipelines, and the view's format draws the scene only
/// at low quality.
pub(crate) struct Pipelines {
    /// The format they draw into.
    format: wgpu::TextureFormat,
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
    outlines: Mutex<HashMap<(Option<u64>, bool, bool, bool), wgpu::RenderPipeline>>,
    own_view_layout: wgpu::BindGroupLayout,
    lines: OnceLock<wgpu::RenderPipeline>,
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
    effects: Mutex<HashMap<u64, wgpu::RenderPipeline>>,
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
    /// Where the materials' values start among the joints.
    values_at: u32,
    instances: Vec<Instance>,
    /// The solid draws' batches, then the blended ones' from `blended_from`.
    batches: Vec<Batch>,
    blended_from: usize,
    line_vertices: Vec<LineVertex>,
    /// The scene's solid draws that are seen, each with its sort key,
    /// sorted.
    order: Vec<(SortKey, u32)>,
    /// Its blended ones, each with how far it is ahead of the camera,
    /// sorted furthest first.
    blended: Vec<(f32, SortKey, u32)>,
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

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// How many frames shadow maps a view stops using are kept: about a
/// second.
const KEEP_UNUSED_MAPS: u32 = 60;
pub(crate) const INSTANCE_SIZE: u64 = std::mem::size_of::<Instance>() as u64;

impl Renderer {
    /// MSAA sample count.
    pub const SAMPLES: u32 = 4;
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
            pipelines: kit.pipelines(format),
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
            values_at: 0,
            instances: Vec::new(),
            batches: Vec::new(),
            blended_from: 0,
            line_vertices: Vec::new(),
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
            kit,
        }
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

    /// The CPU half of a frame: builds per-draw data and the frame uniforms
    /// for a `width` × `height` target. Makes no GPU calls, and allocates
    /// nothing once its buffers have grown to the scene's size.
    pub fn prepare(&mut self, scene: &RenderScene, width: u32, height: u32) -> FrameStats {
        let view_size = (width.max(1), height.max(1));
        self.size = view_size;
        self.view_size = view_size;
        self.effect_chain.clear();
        self.effect_uniforms.clear();
        self.quality = scene.quality.unwrap_or(self.kit.device_quality().quality());
        self.image_format = self.quality.image_format();
        self.joints.clear();
        self.skinned.clear();
        self.looks.clear();
        self.moving.clear();
        self.instances.clear();
        self.batches.clear();
        self.blended_from = 0;
        self.line_vertices.clear();
        self.shadow_candidates.clear();
        self.caster_order.clear();
        self.casters.clear();
        self.caster_batches.clear();
        self.shadow_maps_drawn = (0, 0);
        self.shadow_shape = [None; 2];
        self.sky_next = None;
        self.sky_drawn = false;
        self.bloom = None;
        self.ao = None;
        self.behind = false;
        let mut stats = FrameStats::default();

        let Some(mut camera) = scene.camera else {
            self.clear = wgpu::Color::BLACK;
            return stats;
        };
        // The view's shape, whatever size the scene is drawn at.
        let aspect = view_size.0 as f32 / view_size.1 as f32;
        // The pixel look: drawn small, into an image even at low, and the
        // camera on the picture's pixels.
        if let Some(pixels) = camera.pixels.map(|p| p.clamp(1, view_size.1)) {
            self.size = (((pixels as f32 * aspect).round() as u32).max(1), pixels);
            self.image_format.get_or_insert(self.format);
            snap_to_pixels(&mut camera, pixels);
        }
        self.bloom = camera.bloom.filter(|b| self.quality.bloom && b.strength > 0.0);
        let [r, g, b] = camera.clear_color.map(f64::from);
        // Linear: the sRGB target encodes it like everything else.
        self.clear = wgpu::Color { r, g, b, a: 1.0 };

        // Near and far swapped: reversed-Z.
        let projection = match camera.orthographic {
            Some(half) => glam::camera::rh::proj::directx::orthographic(
                -half * aspect,
                half * aspect,
                -half,
                half,
                camera.far,
                camera.near,
            ),
            None => glam::camera::rh::proj::directx::perspective(camera.fov_y, aspect, camera.far, camera.near),
        };
        // Orthographic: every surface is seen from the same direction, so
        // the shader gets that direction (w = 0) rather than a position.
        let to_viewer = match camera.orthographic {
            Some(_) => camera.view.inverse().transform_vector3(Vec3::Z).normalize().extend(0.0),
            None => camera.position.extend(1.0),
        };
        self.ao =
            camera.ambient_occlusion.filter(|a| self.quality.ambient_occlusion && a.intensity > 0.0 && a.radius > 0.0);
        if let Some(ao) = self.ao {
            self.ao_params = ao::Params::new(ao, projection, camera.orthographic.is_none(), self.size.1);
        }
        let planes = frustum_planes(projection * camera.view);
        let u = &mut self.uniforms;
        u.view_proj = (projection * camera.view).to_cols_array_2d();
        u.camera_position = to_viewer.into();
        u.ambient = scene.ambient.extend(0.0).into();
        u.view_size = [1.0 / self.size.0 as f32, 1.0 / self.size.1 as f32, 0.0, 0.0];
        u.time = [scene.time[0], scene.time[1], 0.0, 0.0];

        // The sky: a plain one is the clear colour and more ambient light;
        // a gradient or an image is drawn after the scene and lights it
        // from its cubes.
        u.sky = SkyUniform {
            inverse_view_proj: (projection * camera.view).inverse().to_cols_array_2d(),
            ..SkyUniform::zeroed()
        };
        u.sky_light = [0.0, (SPECULAR_MIPS - 1) as f32, 0.0, 0.0];
        let mut horizon = Vec3::from(camera.clear_color);
        if let Some(sky) = &scene.sky {
            match &sky.kind {
                SkyKind::Color(c) => {
                    u.ambient = (scene.ambient + *c * sky.lighting).extend(0.0).into();
                    if camera.sky {
                        let [r, g, b] = c.to_array().map(f64::from);
                        self.clear = wgpu::Color { r, g, b, a: 1.0 };
                    }
                    horizon = *c;
                }
                SkyKind::Gradient { zenith, horizon: level, ground } => {
                    u.sky.zenith = zenith.extend(1.0).into();
                    u.sky.horizon = level.extend(0.0).into();
                    u.sky.ground = ground.extend(0.0).into();
                    horizon = *level;
                }
                SkyKind::Image { image, intensity, rotation } => {
                    u.sky.zenith = [0.0, 0.0, 0.0, 2.0];
                    u.sky.horizon = [0.0, 0.0, 0.0, *intensity];
                    u.sky.ground = [0.0, 0.0, 0.0, *rotation];
                    horizon = sky::tonemap_neutral(image.horizon() * *intensity);
                }
            }
            stats.sky_bytes = sky::bytes(&sky.kind);
            if !matches!(sky.kind, SkyKind::Color(_)) {
                u.sky_light[0] = sky.lighting;
                self.sky_next = Some(sky.kind.clone());
                self.sky_drawn = camera.sky;
            }
        }
        u.fog_color = [0.0; 4];
        if let Some(fog) = scene.fog {
            u.fog_color = fog.color.unwrap_or(horizon).extend(1.0).into();
            let height = fog.height.filter(|h| *h > 0.0).map_or(0.0, |h| 1.0 / h);
            u.fog = [fog.start, 1.0 / (fog.end - fog.start).max(1e-4), height, 0.0];
        }

        // The suns, in the scene's order; one with no direction lights
        // nothing.
        let mut suns = 0;
        let mut shadow_sun = None;
        for light in &scene.lights {
            let LightKind::Sun { direction } = light.kind else { continue };
            let to_sun = (-direction).normalize_or_zero();
            if to_sun == Vec3::ZERO {
                continue;
            }
            if suns == MAX_SUNS {
                stats.lights_left_out += 1;
                continue;
            }
            u.suns[suns] = SunUniform { to_sun: to_sun.extend(0.0).into(), color: light.color.extend(0.0).into() };
            if light.shadows && shadow_sun.is_none() {
                shadow_sun = Some((suns, -to_sun));
            }
            suns += 1;
        }
        let highlights = u32::from(self.quality.lamp_highlights);
        u.counts = [suns as u32, shadow_sun.map_or(NO_SHADOWS, |(i, _)| i as u32), highlights, 0];

        // The point and spot lights that can light what's seen: their reach
        // is in view, and they give some light. Past the cap, the ones
        // nearest the camera.
        self.in_view_lights.clear();
        for (i, light) in scene.lights.iter().enumerate() {
            let Some((position, range)) = light.reach() else { continue };
            if range > 0.0 && light.color.max_element() > 0.0 && in_view(&planes, position, range) {
                self.in_view_lights.push((position.distance_squared(camera.position), i as u32));
            }
        }
        if self.in_view_lights.len() > MAX_LIGHTS {
            stats.lights_left_out += (self.in_view_lights.len() - MAX_LIGHTS) as u32;
            self.in_view_lights.select_nth_unstable_by(MAX_LIGHTS, |a, b| a.0.total_cmp(&b.0));
            self.in_view_lights.truncate(MAX_LIGHTS);
        }
        self.frame_lights.clear();
        for (slot, &(_, i)) in self.in_view_lights.iter().enumerate() {
            let light = &scene.lights[i as usize];
            u.lights[slot] = light_uniform(light);
            self.frame_lights.push(Reach::of(light));
        }
        stats.lights = self.frame_lights.len() as u32;
        // The cells: their slices end where the furthest lamp's reach does,
        // rounded up a quarter octave at a time so a moving camera seldom
        // changes the grid's shape (`Clusters` keeps it while it holds).
        let forward = camera.view.inverse().transform_vector3(Vec3::NEG_Z).normalize();
        let reach = self
            .frame_lights
            .iter()
            .map(|l| (l.position - camera.position).dot(forward) + l.range)
            .fold(camera.near * 2.0, f32::max);
        let quarters = ((reach / camera.near).log2() * 4.0).ceil();
        let far = (camera.near * (quarters * 0.25).exp2()).min(camera.far.max(camera.near * 2.0));
        let half = match camera.orthographic {
            Some(h) => (h * aspect, h),
            None => {
                let t = (camera.fov_y * 0.5).tan();
                (t * aspect, t)
            }
        };
        let grid =
            Grid { view: camera.view, half, orthographic: camera.orthographic.is_some(), near: camera.near, far };
        self.clusters.build(&grid, &self.frame_lights);
        let (scale, offset) = grid.slicing();
        u.cells = [TILES_X as f32 / self.size.0 as f32, TILES_Y as f32 / self.size.1 as f32, scale, offset];
        u.cells_eye = camera.position.extend(if grid.orthographic { 0.0 } else { 1.0 }).into();
        u.cells_forward = forward.extend(0.0).into();

        // Leave out what the camera can't see, then group the solid rest by
        // pipeline, textures and mesh, so each run of the same three is one
        // instanced draw call and the state changes least between calls.
        // Blended draws go furthest first, by their centres, each over what
        // it's in front of; a run of the same three is one call there too.
        // Each draw's key, normal matrix and lights are worked out once.
        // The sorts are in place and the buffers reused, so a steady frame
        // allocates nothing.
        self.order.clear();
        self.blended.clear();
        self.joints.extend(scene.joints.iter().map(|m| {
            let rows = m.transpose();
            [rows.x_axis.into(), rows.y_axis.into(), rows.z_axis.into()]
        }));
        // Materials' values go after the joints, where their shaders read
        // them.
        let values = self.joints.len() as u32;
        self.values_at = values;
        self.joints.extend_from_slice(&scene.shader_values);
        let assets = assets::lock(&self.assets);
        for (i, draw) in scene.draws.iter().enumerate() {
            let Some(mesh) = assets.meshes.get(&draw.mesh) else {
                stats.missing_meshes += 1;
                self.skinned.push(false);
                self.looks.push(None);
                self.moving.push(false);
                continue;
            };
            // Bent only by a skin the scene has all of: a joint number past
            // its end would read another skeleton's, or past the buffer.
            let skin = match (draw.skin, &mesh.skin) {
                (Some(first), Some(skin)) if first as usize + skin.joints.len() <= scene.joints.len() => {
                    Some((first as usize, skin))
                }
                _ => None,
            };
            self.skinned.push(skin.is_some());
            let m = &draw.material;
            let mut moving = false;
            let mut reads_behind = false;
            let look = match &m.shader {
                None => Some((None, OwnTextures::of(m.textures))),
                // A screen effect's shader draws no object.
                Some(used) => assets.shaders.get(&used.shader).filter(|s| !s.effect).map(|shader| {
                    let srgb = shader
                        .textures
                        .iter()
                        .enumerate()
                        .fold(0, |bits, (i, t)| if t.space == ColorSpace::Srgb { bits | 1 << i } else { bits });
                    let own =
                        OwnTextures { base: m.textures, own: used.textures, count: shader.textures.len() as u8, srgb };
                    moving = shader.moves;
                    reads_behind = shader.reads_behind;
                    (Some(used.shader), own)
                }),
            };
            self.looks.push(look);
            self.moving.push(moving);
            // Its shader not there: not drawn, and casts no shadow.
            let Some((shader, textures)) = look else { continue };
            let (at, radius) = match skin {
                Some((first, skin)) => skinned_bounds(&scene.joints[first..], &skin.joints),
                None => bounds(draw.model, mesh.centre, mesh.radius),
            };
            // As far as its shader can move it, besides.
            let radius = radius + m.shader.map_or(0.0, |used| used.reach.max(0.0));
            // Out of view, it can still shadow what's in it. Blended
            // surfaces cast none. One that reads what's behind is drawn
            // over it, so it's see-through whatever its blending.
            let cover = match Cover::of(&draw.material) {
                cover if reads_behind && !cover.blended() => Cover::Transparent,
                cover => cover,
            };
            if draw.cast_shadows && !cover.blended() {
                self.shadow_candidates.push((at, radius, i as u32));
            }
            if !in_view(&planes, at, radius) {
                stats.culled += 1;
                continue;
            }
            let mirrored = Mat3::from_mat4(draw.model).determinant() < 0.0;
            let outlined = cover == Cover::Opaque && m.outline.is_some_and(|o| o.width > 0.0);
            let key = (cover, Faces::of(m, mirrored), skin.is_some(), shader, textures, outlined, draw.mesh.0);
            if cover.blended() {
                self.behind |= reads_behind;
                self.blended.push(((at - camera.position).dot(forward), key, i as u32));
            } else {
                self.order.push((key, i as u32));
            }
        }
        // The camera's screen effects, each one's uniform laid out as
        // effect.wgsl reads it. Any makes the scene image, even at low, for
        // the last pass to draw into theirs.
        let stride = self.kit.effects.stride as usize;
        for effect in &scene.effects {
            let Some(shader) = assets.shaders.get(&effect.shader).filter(|s| s.effect) else { continue };
            self.effect_chain.push(effect.shader);
            let at = self.effect_uniforms.len();
            self.effect_uniforms.resize(at + stride, 0);
            let frame = [view_size.0 as f32, view_size.1 as f32, scene.time[0], 0.0];
            self.effect_uniforms[at..at + 16].copy_from_slice(bytemuck::bytes_of(&frame));
            let first = effect.values as usize;
            let blocks = (shader.values_size as usize).div_ceil(48);
            if let Some(values) = scene.shader_values.get(first..first + blocks) {
                let bytes: &[u8] = bytemuck::cast_slice(values);
                let size = shader.values_size as usize;
                self.effect_uniforms[at + 16..at + 16 + size].copy_from_slice(&bytes[..size]);
            }
        }
        drop(assets);
        if !self.effect_chain.is_empty() {
            self.image_format.get_or_insert(self.format);
        }
        stats.effects = self.effect_chain.len() as u32;
        self.order.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        // The same distance keeps the scene's order, so they don't flicker.
        self.blended.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)));

        for &(key, i) in &self.order {
            batch(&mut self.batches, &mut self.instances, key, &scene.draws[i as usize], values);
        }
        self.blended_from = self.batches.len();
        for &(_, key, i) in &self.blended {
            batch(&mut self.batches, &mut self.instances, key, &scene.draws[i as usize], values);
        }
        stats.joints = scene.joints.len() as u32;
        for line in &scene.lines {
            for end in [line.a, line.b] {
                self.line_vertices.push(LineVertex { position: end.into(), color: line.color });
            }
        }
        // Cut-outs with their own shaders add their shadows' instances
        // after these.
        stats.draws = self.instances.len() as u32;
        self.prepare_shadows(scene, &camera, aspect, shadow_sun.map(|(_, direction)| direction), &mut stats);
        // The shadow maps `encode` draws with: as many as the frame uses.
        // A kind it uses fewer of keeps what it has a while, so shadows that
        // come and go don't remake them, then lets them go.
        let had = self.shadow_maps.as_ref().map_or([(1, 0); 2], |m| m.shape);
        for (k, had) in had.into_iter().enumerate() {
            let want = self.shadow_shape[k].unwrap_or((had.0, 0));
            let fewer = want.0 == had.0 && want.1 < had.1;
            self.shadow_idle[k] = if fewer { self.shadow_idle[k] + 1 } else { 0 };
            self.shadow_next[k] = if fewer && self.shadow_idle[k] <= KEEP_UNUSED_MAPS { had } else { want };
        }
        stats.shadow_bytes = ShadowMaps::bytes(self.shadow_next);
        // What's behind: kept a while too, then let go.
        self.behind_idle = if self.behind { 0 } else { self.behind_idle.saturating_add(1) };
        let held = self.targets.as_ref().is_some_and(|t| t.behind.is_some() && (t.width, t.height) == self.size);
        self.behind_kept = self.behind || held && self.behind_idle <= KEEP_UNUSED_MAPS;
        stats.behind = self.behind;
        if self.behind_kept {
            let format = self.image_format.unwrap_or(self.format);
            stats.behind_bytes = BehindImages::bytes(self.size.0, self.size.1, format);
        }
        // The effects' pictures: kept a while too.
        let count = self.effect_chain.len();
        self.effects_idle = if count > 0 { 0 } else { self.effects_idle.saturating_add(1) };
        if count > 0 {
            stats.effect_bytes = EffectImages::bytes(view_size, self.format, count);
        } else if let Some(images) = self.effect_images.as_ref().filter(|_| self.effects_idle <= KEEP_UNUSED_MAPS) {
            stats.effect_bytes = EffectImages::bytes(images.size, self.format, images.images.len());
        }
        stats.lines = scene.lines.len() as u32;
        stats.draw_calls = self.batches.len() as u32;
        stats
    }

    /// The shadow maps of a frame `prepare` has lit and culled: the sun's
    /// cascades (`sun` is the way its light travels), and the nearest spot
    /// lights' maps, each with the casters that fall in it, sorted and
    /// batched as they're drawn.
    fn prepare_shadows(
        &mut self,
        scene: &RenderScene,
        camera: &crate::CameraView,
        aspect: f32,
        sun: Option<Vec3>,
        stats: &mut FrameStats,
    ) {
        // No shadows: no sun's maps and no spot maps, whatever lights ask.
        let on = self.quality.shadows.is_some();
        let level = self.quality.shadows.or(Quality::LOW.shadows).expect("low has shadows");
        let sun = sun.filter(|_| on);
        let stride = self.kit.shadows.view_stride as usize;
        let u = &mut self.uniforms;
        u.shadows = ShadowUniforms::zeroed();
        let sh = &mut u.shadows;
        let filter = match level.filter {
            ShadowFilter::Hard => 0.0,
            ShadowFilter::Soft => 1.0,
            ShadowFilter::Softer => 2.0,
        };
        sh.params = [0.0, filter, 1.0 / level.map_size as f32, 1.0 / level.spot_map_size as f32];
        let view = |map: usize, matrix: Mat4, views: &mut Vec<u8>| {
            views[map * stride..map * stride + 64].copy_from_slice(bytemuck::bytes_of(&matrix.to_cols_array()));
        };

        // The sun's cascades. Each takes the casters that overlap it, which
        // also stretch it back towards the sun to hold them.
        if let Some(direction) = sun {
            let count = level.cascades.clamp(1, MAX_CASCADES);
            let basis = shadows::light_basis(direction);
            let (mut cascades, splits) = shadows::cascades(camera, aspect, &level, &basis);
            for &(at, r, i) in &self.shadow_candidates {
                for (c, cascade) in cascades[..count].iter_mut().enumerate() {
                    if cascade.reach(&basis, at, r) {
                        let skinned = self.skinned[i as usize];
                        let (look, moving) = (self.looks[i as usize], self.moving[i as usize]);
                        self.caster_order
                            .push((caster_key(c as u8, &scene.draws[i as usize], skinned, look, moving), i));
                    }
                }
            }
            for (c, cascade) in cascades[..count].iter().enumerate() {
                let matrix = cascade.matrix(&basis);
                sh.cascades[c] = matrix.to_cols_array_2d();
                sh.texels[c] = cascade.texel(level.map_size);
                view(c, matrix, &mut self.shadow_views);
            }
            let distance = shadows::shadow_distance(camera, &level);
            sh.splits = [splits[0], splits[1], splits[2], distance * 0.9];
            sh.params[0] = count as f32;
            sh.camera_forward = shadows::camera_forward(camera).extend(distance).into();
            sh.camera_at = camera.position.extend(1.0).into();
            self.shadow_maps_drawn.0 = count as u8;
            self.shadow_shape[0] = Some((level.map_size, count as u32));
        }

        // The spot lights that cast shadows, a map each, the nearest the
        // camera first: their slots in the frame's list.
        let cap = if on { level.spot_maps.min(MAX_SPOT_MAPS) } else { 0 };
        let mut nearest = [(f32::INFINITY, 0usize); MAX_SPOT_MAPS];
        for (slot, &(distance, i)) in self.in_view_lights.iter().enumerate() {
            let light = &scene.lights[i as usize];
            if light.shadows && matches!(light.kind, LightKind::Spot { .. }) && cap > 0 && distance < nearest[cap - 1].0
            {
                let mut k = cap - 1;
                while k > 0 && nearest[k - 1].0 > distance {
                    nearest[k] = nearest[k - 1];
                    k -= 1;
                }
                nearest[k] = (distance, slot);
            }
        }
        let mut spots = 0;
        for &(distance, slot) in &nearest[..cap] {
            if distance == f32::INFINITY {
                break;
            }
            let light = &scene.lights[self.in_view_lights[slot].1 as usize];
            let LightKind::Spot { position, direction, range, angle, .. } = light.kind else { continue };
            let (matrix, texel) = shadows::spot_view(position, direction, angle, range, level.spot_map_size);
            u.lights[slot].cone[2] = spots as f32;
            u.lights[slot].cone[3] = texel;
            u.shadows.spots[spots] = matrix.to_cols_array_2d();
            let map = MAX_CASCADES + spots;
            view(map, matrix, &mut self.shadow_views);
            let planes = frustum_planes(matrix);
            for &(at, r, d) in &self.shadow_candidates {
                if in_view(&planes, at, r) {
                    let skinned = self.skinned[d as usize];
                    let (look, moving) = (self.looks[d as usize], self.moving[d as usize]);
                    self.caster_order.push((caster_key(map as u8, &scene.draws[d as usize], skinned, look, moving), d));
                }
            }
            spots += 1;
        }
        if spots > 0 {
            self.shadow_maps_drawn.1 = spots as u8;
            self.shadow_shape[1] = Some((level.spot_map_size, spots as u32));
        }

        self.caster_order.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for &(key @ (map, own, cutout, skinned, textures, _), i) in &self.caster_order {
            let draw = &scene.draws[i as usize];
            let m = &draw.material;
            // A cut-out with its own shader is cut by it, and one whose
            // shader moves its vertices is moved by it: drawn as the
            // forward pass draws it, from the instances after its own.
            let first = if own.is_some() { self.instances.len() } else { self.casters.len() } as u32;
            match self.caster_batches.last_mut() {
                Some(b) if (b.map, b.own, b.cutout, b.skinned, b.textures, b.mesh.0) == key => b.count += 1,
                _ => self.caster_batches.push(CasterBatch {
                    map,
                    own,
                    cutout,
                    skinned,
                    textures,
                    mesh: draw.mesh,
                    first,
                    count: 1,
                }),
            }
            if own.is_some() {
                self.instances.push(instance(draw, skinned, self.values_at));
                continue;
            }
            let ([su, sv], [ou, ov]) = (m.uv_scale, m.uv_offset);
            let joints = if skinned { draw.skin.unwrap_or(0) as f32 } else { 0.0 };
            self.casters.push(Caster {
                model: draw.model.to_cols_array_2d(),
                uv: [su, sv, ou, ov],
                alpha: [m.base_color[3], m.alpha_cutoff, joints, 0.0],
            });
        }
        stats.shadow_maps = u32::from(self.shadow_maps_drawn.0) + u32::from(self.shadow_maps_drawn.1);
        stats.shadow_casters = self.casters.len() as u32 + (self.instances.len() as u32 - stats.draws);
    }

    /// The GPU half: writes what [`Renderer::prepare`] built where the GPU
    /// reads it and records the passes into `encoder`, ending in `target`.
    /// The target must be a view in this renderer's format, of the size
    /// given to `prepare`. `encoder` must be submitted before the next
    /// `encode`.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let (width, height) = self.size;
        let image = self.image_format;
        if self
            .targets
            .as_ref()
            .is_none_or(|t| (t.width, t.height, t.image.as_ref().map(|i| i.format)) != (width, height, image))
        {
            self.targets = Some(self.create_targets(width, height, image));
        }
        if let Some(format) = image
            && self.image_pipelines.as_ref().is_none_or(|p| p.format != format)
        {
            self.image_pipelines = Some(self.kit.pipelines(format));
        }
        let effects = self.effect_chain.len();
        if effects > 0 {
            if self.effect_images.as_ref().is_none_or(|e| !e.fit(self.view_size, self.format, effects)) {
                let device = &self.kit.gpu.device;
                self.effect_images =
                    Some(EffectImages::new(&self.kit.effects, device, self.view_size, self.format, effects));
            }
        } else if self.effects_idle > KEEP_UNUSED_MAPS {
            self.effect_images = None;
        }
        if self.shadow_maps.as_ref().is_none_or(|m| m.shape != self.shadow_next) {
            let none = (0, &self.kit.ao.none, &self.kit.ao.sampler);
            self.shadow_maps = Some(ShadowMaps::new(&self.kit.gpu.device, &self.kit.shadows, self.shadow_next, none));
        }
        // Ambient occlusion's images, and the forward pass reading them, or
        // the stand-in.
        if let Some(targets) = &mut self.targets {
            if self.ao.is_some() && targets.ao.is_none() {
                targets.ao = Some(AoImages::new(&self.kit.ao, width, height));
            }
            let ao = match (&self.ao, &mut targets.ao) {
                (Some(_), Some(images)) => {
                    images.set(&self.kit.gpu.queue, self.ao_params);
                    (images.id, &images.blurred, &self.kit.ao.sampler)
                }
                _ => (0, &self.kit.ao.none, &self.kit.ao.sampler),
            };
            // What's behind, for the shaders that read it, and the bind
            // group they read it through.
            if !self.behind_kept {
                targets.behind = None;
            } else if targets.behind.is_none() {
                let format = targets.image.as_ref().map_or(self.format, |i| i.format);
                targets.behind = Some(BehindImages::new(&self.kit, width, height, format));
            }
            if let Some(maps) = &mut self.shadow_maps {
                maps.read_ao(&self.kit.gpu.device, &self.kit.shadows, ao);
                if let (true, Some(behind)) = (self.behind, &targets.behind) {
                    maps.read_behind(
                        &self.kit.gpu.device,
                        &self.kit.shadows,
                        ao,
                        (behind.id, &behind.depth, &behind.color),
                    );
                }
            }
        }
        // A new sky's light, worked out before anything reads it.
        if let Some(kind) = &self.sky_next
            && self.environment.as_ref().is_none_or(|e| e.kind != *kind)
        {
            self.environment = Some(Environment::new(&self.kit.sky, encoder, kind));
        }
        if let (Some(_), Some(image)) = (self.bloom, self.targets.as_mut().and_then(|t| t.image.as_mut()))
            && image.bloom.is_none()
        {
            image.bloom = Some(BloomChain::new(&self.kit.bloom, &image.view, width, height));
        }
        let Self { kit, pipelines, image_pipelines, assets, frame_data, targets, shadow_maps, environment, .. } = self;
        let sky_light = match (&self.sky_next, &*environment) {
            (Some(_), Some(environment)) => &environment.light,
            _ => &kit.sky.no_light,
        };
        let sky_image = environment.as_ref().and_then(|e| e.image.as_ref()).unwrap_or(&kit.sky.no_image);
        let targets = targets.as_mut().expect("made above");
        let shadow_maps = shadow_maps.as_ref().expect("made above");

        // Bind groups for texture sets not seen before. A steady frame only
        // looks them up. The world's other views wait while this one
        // records; a mesh or texture they free meanwhile is gone from the
        // next frame on.
        let mut assets = assets::lock(assets);
        let Assets { kit: _, meshes, textures, shaders } = &mut *assets;
        for batch in &self.batches {
            textures.prepare_own(&kit.gpu.device, &kit.textures, batch.textures);
        }
        for batch in self.caster_batches.iter().filter(|b| b.cutout || b.own.is_some()) {
            match batch.own {
                None => textures.prepare(&kit.gpu.device, &kit.textures, batch.textures),
                Some((_, own)) => textures.prepare_own(&kit.gpu.device, &kit.textures, own),
            }
        }
        let frame = frame_data.write(
            &kit.gpu.device,
            Parts {
                uniforms: bytemuck::bytes_of(&self.uniforms),
                views: &self.shadow_views,
                joints: bytemuck::cast_slice(&self.joints),
                instances: bytemuck::cast_slice(&self.instances),
                lines: bytemuck::cast_slice(&self.line_vertices),
                casters: bytemuck::cast_slice(&self.casters),
                cells: self.clusters.bytes(),
            },
        );

        // 0. The shadow maps: each cleared, then its casters' depth.
        let (cascades, spots) = self.shadow_maps_drawn;
        let maps = (0..cascades).map(|c| (c, false, c)).chain((0..spots).map(|k| (MAX_CASCADES as u8 + k, true, k)));
        let mut batches = self.caster_batches.iter().peekable();
        for (map, spot, layer) in maps {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if spot { "spot shadow map" } else { "sun shadow map" }),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: shadow_maps.layer(spot, layer.into()),
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let view = (u64::from(map) * kit.shadows.view_stride) as u32;
            pass.set_bind_group(0, frame.views, &[view]);
            pass.set_vertex_buffer(1, frame.buffer.slice(frame.casters..));
            let mut last: Option<&CasterBatch> = None;
            while let Some(batch) = batches.next_if(|b| b.map == map && b.own.is_none()) {
                let Some(mesh) = meshes.get(&batch.mesh) else { continue };
                if batch.cutout {
                    let Some(bind_group) = textures.bind_group(&batch.textures) else { continue };
                    if last.is_none_or(|l| l.textures != batch.textures || !l.cutout) {
                        pass.set_bind_group(1, bind_group, &[]);
                    }
                }
                let skin = mesh.skin.as_ref().filter(|_| batch.skinned);
                if batch.skinned && skin.is_none() {
                    continue;
                }
                if last.is_none_or(|l| (l.cutout, l.skinned) != (batch.cutout, batch.skinned)) {
                    pass.set_pipeline(kit.shadows.pipeline(spot, batch.cutout, batch.skinned));
                }
                last = Some(batch);
                if let Some(skin) = skin {
                    pass.set_vertex_buffer(2, skin.buffer.slice(..));
                }
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, batch.first..batch.first + batch.count);
            }
            // Then those with their own shaders, cut out or moving, which
            // read what the forward pass does, and the map's view in group 2.
            if batches.peek().is_some_and(|b| b.map == map) {
                pass.set_bind_group(0, frame.bind_group, &[]);
                pass.set_bind_group(2, frame.own_views, &[view]);
                pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
            }
            let mut last: Option<&CasterBatch> = None;
            while let Some(batch) = batches.next_if(|b| b.map == map) {
                let Some((Some(key), looks)) = batch.own else { continue };
                // Freed or changed since prepare.
                let (Some(mesh), Some(bind_group), Some(shader)) =
                    (meshes.get(&batch.mesh), textures.own_bind_group(&looks), shaders.get(&key))
                else {
                    continue;
                };
                if shader.textures.len() != looks.count as usize {
                    continue;
                }
                let skin = mesh.skin.as_ref().filter(|_| batch.skinned);
                if batch.skinned && skin.is_none() {
                    continue;
                }
                if last.is_none_or(|l| (l.own, l.skinned, l.cutout) != (batch.own, batch.skinned, batch.cutout)) {
                    pass.set_pipeline(&pipelines.own_shadow(shader, spot, batch.skinned, batch.cutout));
                }
                if last.is_none_or(|l| l.own != batch.own) {
                    pass.set_bind_group(1, bind_group, &[]);
                }
                last = Some(batch);
                if let Some(skin) = skin {
                    pass.set_vertex_buffer(2, skin.buffer.slice(..));
                }
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, batch.first..batch.first + batch.count);
            }
        }

        // Ambient occlusion: the solid scene's depth, then how hidden each
        // pixel is, blurred. Cut-outs and blended draws aren't in it. Those
        // whose shaders move their vertices are drawn with them.
        if let (Some(_), Some(images)) = (self.ao, &targets.ao) {
            {
                let mut pass = images.depth_pass(&kit.ao, encoder);
                let mut solid =
                    self.batches[..self.blended_from].iter().filter(|b| b.cover == Cover::Opaque).peekable();
                if solid.peek().is_some() {
                    pass.set_bind_group(0, frame.bind_group, &[]);
                    pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
                }
                let mut last = None;
                for batch in solid {
                    let Some(mesh) = meshes.get(&batch.mesh) else { continue };
                    let skin = mesh.skin.as_ref().filter(|_| batch.skinned);
                    if batch.skinned && skin.is_none() {
                        continue;
                    }
                    let moving = batch.shader.and_then(|key| shaders.get(&key)).filter(|s| s.moves);
                    let pipeline = (batch.skinned, moving.map(|s| s.hash));
                    if last != Some(pipeline) {
                        last = Some(pipeline);
                        match moving {
                            None => pass.set_pipeline(kit.ao.depth_pipeline(batch.skinned)),
                            Some(shader) => pass.set_pipeline(&pipelines.own_depth(shader, batch.skinned)),
                        }
                    }
                    if let Some(skin) = skin {
                        pass.set_vertex_buffer(2, skin.buffer.slice(..));
                    }
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, batch.first..batch.first + batch.count);
                }
            }
            images.encode(&kit.ao, encoder);
        }

        // 1. The scene, into the scene image or straight into the view.
        // When a see-through surface reads what's behind it, in two: the
        // solid scene and the sky, its depth kept and its colour resolved
        // for them to read; then that put back and the see-through drawn
        // over it.
        let (scene_pipelines, resolve) = match &targets.image {
            Some(image) => (image_pipelines.as_deref().expect("got above"), &image.view),
            None => (&**pipelines, target),
        };
        let behind = targets.behind.as_ref().filter(|_| self.behind);
        let behind_group = behind.and(shadow_maps.behind_bind_group());
        let sky_lit = self.sky_next.is_some();
        let ao = self.ao.is_some();
        let shadowed = self.shadow_maps_drawn != (0, 0);
        let (solid, blended) = self.batches.split_at(self.blended_from);
        // Their surfaces, or with `outlines`, the outlines of those that
        // have them.
        let draw = |pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], outlines: bool| {
            let mut last: Option<&Batch> = None;
            // The maps bound in group 2, or what's behind as well.
            let mut reading = false;
            for batch in batches.iter().filter(|b| b.outlined || !outlines) {
                // Freed since prepare, by another view's render worker.
                let (Some(mesh), Some(bind_group)) =
                    (meshes.get(&batch.mesh), textures.own_bind_group(&batch.textures))
                else {
                    continue;
                };
                // Its mesh put back without joints since prepare.
                if batch.skinned && mesh.skin.is_none() {
                    continue;
                }
                // Its shader freed or changed since prepare.
                let shader = match batch.shader {
                    None => None,
                    Some(key) => match shaders.get(&key) {
                        Some(shader) if shader.textures.len() == batch.textures.count as usize => Some(shader),
                        _ => continue,
                    },
                };
                let reads = shader.is_some_and(|s| s.reads_behind);
                if reads != reading {
                    // Changed since prepare: what it reads isn't there.
                    let group = if reads { behind_group } else { Some(&shadow_maps.bind_group) };
                    let Some(group) = group else {
                        continue;
                    };
                    pass.set_bind_group(2, group, &[]);
                    reading = reads;
                }
                let textured = |b: &Batch| b.textures.base != MaterialTextures::NONE;
                let pipeline = |b: &Batch| (b.cover, b.faces, textured(b), b.skinned, b.shader);
                if outlines {
                    // Its mesh's outline normals or not choose it too.
                    if last.is_none_or(|l| pipeline(l) != pipeline(batch) || l.mesh != batch.mesh) {
                        let mirrored = batch.faces == Faces::Mirrored;
                        let normals = mesh.outline_normals.is_some();
                        let pipeline = scene_pipelines.outline(shader.map(|s| &**s), mirrored, batch.skinned, normals);
                        pass.set_pipeline(&pipeline);
                    }
                    if let Some(normals) = &mesh.outline_normals {
                        pass.set_vertex_buffer(3, normals.slice(..));
                    }
                } else if last.is_none_or(|l| pipeline(l) != pipeline(batch)) {
                    let (cover, faces) = (batch.cover, batch.faces);
                    let variant = Variant {
                        cover,
                        faces,
                        shadowed,
                        textured: textured(batch),
                        sky_lit,
                        // Blended surfaces aren't in its depth.
                        ao: ao && !cover.blended(),
                        skinned: batch.skinned,
                    };
                    match shader {
                        None => pass.set_pipeline(scene_pipelines.forward(variant)),
                        Some(shader) => pass.set_pipeline(&scene_pipelines.own(shader, variant)),
                    }
                }
                if batch.skinned
                    && let Some(skin) = &mesh.skin
                {
                    pass.set_vertex_buffer(2, skin.buffer.slice(..));
                }
                if last.is_none_or(|l| l.textures != batch.textures) {
                    pass.set_bind_group(1, bind_group, &[]);
                }
                last = Some(batch);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, batch.first..batch.first + batch.count);
            }
        };
        // What the draws read: the frame, the maps and the sky's light.
        let bind = |pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], sky: bool, lines: bool| {
            if !batches.is_empty() || lines || sky {
                pass.set_bind_group(0, frame.bind_group, &[]);
            }
            if !batches.is_empty() {
                pass.set_bind_group(2, &shadow_maps.bind_group, &[]);
                pass.set_bind_group(3, sky_light, &[]);
                pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
            }
        };
        let lines = |pass: &mut wgpu::RenderPass<'_>| {
            if !self.line_vertices.is_empty() {
                pass.set_pipeline(scene_pipelines.lines());
                pass.set_vertex_buffer(0, frame.buffer.slice(frame.lines..));
                pass.draw(0..self.line_vertices.len() as u32, 0..1);
            }
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color,
                    depth_slice: None,
                    resolve_target: Some(behind.map_or(resolve, |b| &b.color)),
                    // The multisampled colour is only needed until it's
                    // resolved, so it's never stored. On tile GPUs it never
                    // leaves the chip.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(match targets.image {
                            // Shown as picked: not light (above).
                            Some(_) => wgpu::Color { a: 0.0, ..self.clear },
                            None => self.clear,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                // Kept only for what's behind.
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: behind.map_or(&targets.depth, |b| &b.msaa_depth),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: if behind.is_some() { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let (drawn, line_drawn) = match behind {
                None => (&self.batches[..], !self.line_vertices.is_empty()),
                Some(_) => (solid, false),
            };
            bind(&mut pass, drawn, self.sky_drawn, line_drawn);
            draw(&mut pass, solid, false);
            draw(&mut pass, solid, true);
            // After the solid scene, so it's drawn only where nothing else
            // is; before the blended, which show it through them.
            if self.sky_drawn {
                pass.set_pipeline(scene_pipelines.sky());
                pass.set_bind_group(1, sky_image, &[]);
                pass.draw(0..3, 0..1);
            }
            if behind.is_none() {
                draw(&mut pass, blended, false);
                lines(&mut pass);
            }
        }
        if let Some(behind) = behind {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("scene depth"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &behind.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(scene_pipelines.copy_depth());
                pass.set_bind_group(0, &behind.copy, &[]);
                pass.draw(0..3, 0..1);
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene, see-through"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color,
                    depth_slice: None,
                    resolve_target: Some(resolve),
                    // Every sample is written, by what's behind put back.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &behind.msaa_depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(scene_pipelines.restore());
            pass.set_bind_group(0, &behind.restore, &[]);
            pass.draw(0..3, 0..1);
            bind(&mut pass, blended, false, !self.line_vertices.is_empty());
            draw(&mut pass, blended, false);
            lines(&mut pass);
        }

        // 2. Bloom's glow, from the scene image.
        if let (Some(bloom), Some(SceneImage { bloom: Some(chain), .. })) = (self.bloom, &mut targets.image) {
            chain.encode(&kit.bloom, &kit.gpu.queue, encoder, bloom);
        }

        // 3. The last pass: the scene image onto the view, or with screen
        // effects into the first's picture.
        let effect_images = self.effect_images.as_ref().filter(|_| effects > 0);
        if let Some(image) = &targets.image {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("finish"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: effect_images.map_or(target, |e| &e.images[0].0),
                    depth_slice: None,
                    resolve_target: None,
                    // Every pixel is written, so what was there is never read.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &image.bind_group, &[]);
            match image.bloom.as_ref().filter(|_| self.bloom.is_some()) {
                Some(chain) => {
                    pass.set_pipeline(pipelines.finish(true));
                    pass.set_bind_group(1, chain.glow(), &[]);
                }
                None => pass.set_pipeline(pipelines.finish(false)),
            }
            pass.draw(0..3, 0..1);
        }

        // 4. The screen effects, passing the picture between their two
        // images, the last onto the view.
        if let Some(images) = effect_images {
            kit.gpu.queue.write_buffer(&images.buffer, 0, &self.effect_uniforms);
            for (i, key) in self.effect_chain.iter().enumerate() {
                let last = i + 1 == effects;
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("screen effect"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: if last { target } else { &images.images[(i + 1) % 2].0 },
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                // Freed since prepare: the picture goes on as it was.
                let pipeline = shaders.get(key).map_or_else(|| pipelines.effect_copy(), |s| pipelines.effect(s));
                pass.set_pipeline(&pipeline);
                let offset = (i as u64 * kit.effects.stride) as u32;
                pass.set_bind_group(0, &images.images[i % 2].1, &[offset]);
                pass.draw(0..3, 0..1);
            }
        }
    }

    fn create_targets(&self, width: u32, height: u32, image: Option<wgpu::TextureFormat>) -> Targets {
        let make = |label, format| {
            self.kit
                .gpu
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: Self::SAMPLES,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    // Cleared and thrown away in the one pass: on tiled
                    // GPUs (phones, Apple) they need no memory at all.
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TRANSIENT_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let device = &self.kit.gpu.device;
        let image = image.map(|format| {
            // Resolved into, then read by the last pass: stored, so it
            // costs memory and bandwidth where the MSAA targets don't.
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("scene image"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene image"),
                layout: &self.kit.image_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
            });
            SceneImage { format, view, bind_group, bloom: None }
        });
        let color_format = image.as_ref().map_or(self.format, |i| i.format);
        Targets {
            width,
            height,
            color: make("msaa colour", color_format),
            depth: make("msaa depth", DEPTH_FORMAT),
            image,
            ao: None,
            behind: None,
        }
    }

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

impl Pipelines {
    pub(crate) fn new(kit: &Kit, format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            maker: kit.maker.clone(),
            device: kit.gpu.device.clone(),
            frame_layout: kit.frame_layout.clone(),
            textures_layout: kit.textures.layout.clone(),
            own_textures_layouts: kit.textures.own_layouts.clone(),
            maps_layout: kit.shadows.maps_layout.clone(),
            behind_layout: kit.shadows.behind_layout.clone(),
            image_layout: kit.image_layout.clone(),
            msaa_depth_layout: kit.msaa_depth_layout.clone(),
            sky_light_layout: kit.sky.light_layout.clone(),
            sky_image_layout: kit.sky.image_layout.clone(),
            bloom_layout: kit.bloom.layout.clone(),
            forward_shader: OnceLock::new(),
            forward: std::array::from_fn(|_| OnceLock::new()),
            own_shaders: Mutex::default(),
            own: Mutex::default(),
            own_shadows: Mutex::default(),
            own_depths: Mutex::default(),
            outlines: Mutex::default(),
            own_view_layout: kit.shadows.own_view_layout.clone(),
            lines: OnceLock::new(),
            sky: OnceLock::new(),
            behind_shader: OnceLock::new(),
            copy_depth: OnceLock::new(),
            restore: OnceLock::new(),
            finish: OnceLock::new(),
            finish_bloom: OnceLock::new(),
            effect_layout: kit.effects.layout.clone(),
            effects: Mutex::new(HashMap::new()),
        }
    }

    /// The forward pipeline for `variant`.
    fn forward(&self, variant: Variant) -> &wgpu::RenderPipeline {
        self.forward[variant.index()].get_or_init(|| self.make_forward(variant))
    }

    fn make_forward(&self, variant: Variant) -> wgpu::RenderPipeline {
        let (shader, layout) = self.forward_shader();
        self.make_forward_with(variant, shader, layout, "")
    }

    /// The built-in material's module and its layout, made the first time.
    fn forward_shader(&self) -> &(wgpu::ShaderModule, wgpu::PipelineLayout) {
        self.forward_shader.get_or_init(|| {
            let source =
                concat!(include_str!("skin.wgsl"), include_str!("forward.wgsl"), include_str!("standard.wgsl"));
            self.forward_module("forward", source, &self.textures_layout, &self.maps_layout)
        })
    }

    /// The textures layout of `shader`'s group 1.
    fn own_textures_layout(&self, shader: &CustomShader) -> &wgpu::BindGroupLayout {
        match shader.textures.len() {
            0 => &self.textures_layout,
            n => &self.own_textures_layouts[n - 1],
        }
    }

    /// `shader`'s module and its forward pass's layout, made the first
    /// time.
    fn own_module(&self, shader: &CustomShader) -> (wgpu::ShaderModule, wgpu::PipelineLayout) {
        self.own_shaders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(shader.hash)
            .or_insert_with(|| {
                let maps = if shader.reads_behind { &self.behind_layout } else { &self.maps_layout };
                self.forward_module(&shader.name, &shader.wgsl, self.own_textures_layout(shader), maps)
            })
            .clone()
    }

    /// The pipeline for `variant` of a material's own shader.
    fn own(&self, shader: &CustomShader, variant: Variant) -> wgpu::RenderPipeline {
        let key = (shader.hash, variant.index());
        if let Some(pipeline) = self.own.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let (module, layout) = self.own_module(shader);
        let pipeline = self.make_forward_with(variant, &module, &layout, &shader.name);
        self.own.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// The pipeline that draws the shadow of a material with its own
    /// `shader`, cut out or moving its vertices, into a spot map or a
    /// cascade, bent by skeletons or not: the forward pass's vertices and
    /// instances, moved by its `vertex`, and a cut-out's alpha cutting it
    /// (own_shadow.wgsl).
    fn own_shadow(&self, shader: &CustomShader, spot: bool, skinned: bool, cutout: bool) -> wgpu::RenderPipeline {
        let key = (shader.hash, spot, skinned, cutout);
        if let Some(pipeline) = self.own_shadows.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let (module, _) = self.own_module(shader);
        let label = format!("shadow ({})", shader.name);
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&label),
            bind_group_layouts: &[
                Some(&self.frame_layout),
                Some(self.own_textures_layout(shader)),
                Some(&self.own_view_layout),
            ],
            immediate_size: 0,
        });
        let vertex_attributes = VERTEX_ATTRIBUTES;
        let instance_attributes = INSTANCE_ATTRIBUTES;
        let skin_attributes = SKIN_ATTRIBUTES;
        let buffers = forward_buffers(&vertex_attributes, &instance_attributes, &skin_attributes);
        let format = if spot { shadows::SPOT_FORMAT } else { shadows::CASCADE_FORMAT };
        let pipeline = self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(match (spot, skinned) {
                    (false, false) => "vs_fy_shadow_cascade",
                    (false, true) => "vs_fy_shadow_cascade_skinned",
                    (true, false) => "vs_fy_shadow",
                    (true, true) => "vs_fy_shadow_skinned",
                }),
                compilation_options: Default::default(),
                buffers: if skinned { &buffers } else { &buffers[..2] },
            },
            // Both sides, as every caster (shadows.rs).
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: shadows::bias(format),
            }),
            multisample: Default::default(),
            // Depth only, unless it's cut out.
            fragment: cutout.then(|| wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_fy_shadow"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.own_shadows.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// The pipeline that draws outlines of draws with `shader` (`None`,
    /// the built-in one), mirrored or not, bent by skeletons or not, with
    /// their mesh's outline normals in vertex buffer 3 or not: their back
    /// faces, pushed out (forward.wgsl, `vs_outline`).
    fn outline(
        &self,
        shader: Option<&CustomShader>,
        mirrored: bool,
        skinned: bool,
        normals: bool,
    ) -> wgpu::RenderPipeline {
        let key = (shader.map(|s| s.hash), mirrored, skinned, normals);
        if let Some(pipeline) = self.outlines.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let (module, layout) = match shader {
            Some(shader) => self.own_module(shader),
            None => self.forward_shader().clone(),
        };
        // With normals of its own, they're read in place of the tangents.
        let [position, normal, tangent, uv] = VERTEX_ATTRIBUTES;
        let vertex_attributes: &[wgpu::VertexAttribute] =
            if normals { &[position, normal, uv] } else { &[position, normal, tangent, uv] };
        let instance_attributes = INSTANCE_ATTRIBUTES;
        let skin_attributes = SKIN_ATTRIBUTES;
        let normal_attributes = wgpu::vertex_attr_array![2 => Float32x3];
        let [vertex, instance, skin] = forward_buffers(vertex_attributes, &instance_attributes, &skin_attributes);
        let normal_buffer = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &normal_attributes,
        };
        let buffers = [vertex, instance, skin.filter(|_| skinned), normals.then_some(normal_buffer)];
        let used = if normals {
            4
        } else if skinned {
            3
        } else {
            2
        };
        let constants = [("outline_normals", if normals { 1.0 } else { 0.0 })];
        let label = match shader {
            Some(shader) => format!("outline ({})", shader.name),
            None => "outline".to_owned(),
        };
        let pipeline = self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(if skinned { "vs_outline_skinned" } else { "vs_outline" }),
                compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
                buffers: &buffers[..used],
            },
            // Inside out: its front faces are left out.
            primitive: wgpu::PrimitiveState {
                front_face: if mirrored { wgpu::FrontFace::Cw } else { wgpu::FrontFace::Ccw },
                cull_mode: Some(wgpu::Face::Front),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: Renderer::SAMPLES, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_outline"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.outlines.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// The pipeline that draws ambient occlusion's depth (ao.rs) of a
    /// material whose own `shader` moves its vertices.
    fn own_depth(&self, shader: &CustomShader, skinned: bool) -> wgpu::RenderPipeline {
        let key = (shader.hash, skinned);
        if let Some(pipeline) = self.own_depths.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let (module, _) = self.own_module(shader);
        let label = format!("ambient occlusion depth ({})", shader.name);
        // Its `vertex` reads group 0 alone (shader.rs checks).
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&label),
            bind_group_layouts: &[Some(&self.frame_layout)],
            immediate_size: 0,
        });
        let vertex_attributes = VERTEX_ATTRIBUTES;
        let instance_attributes = INSTANCE_ATTRIBUTES;
        let skin_attributes = SKIN_ATTRIBUTES;
        let buffers = forward_buffers(&vertex_attributes, &instance_attributes, &skin_attributes);
        let pipeline = self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(if skinned { "vs_fy_depth_skinned" } else { "vs_fy_depth" }),
                compilation_options: Default::default(),
                buffers: if skinned { &buffers } else { &buffers[..2] },
            },
            // As ao.rs's own.
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::ao::DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        self.own_depths.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// A forward pass's module from `source`, and its layout with
    /// `textures` for group 1 and `maps` for group 2.
    fn forward_module(
        &self,
        label: &str,
        source: &str,
        textures: &wgpu::BindGroupLayout,
        maps: &wgpu::BindGroupLayout,
    ) -> (wgpu::ShaderModule, wgpu::PipelineLayout) {
        let device = &self.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&self.frame_layout), Some(textures), Some(maps), Some(&self.sky_light_layout)],
            immediate_size: 0,
        });
        (shader, layout)
    }

    fn make_forward_with(
        &self,
        variant: Variant,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        name: &str,
    ) -> wgpu::RenderPipeline {
        let Variant { cover, faces, shadowed, textured, sky_lit, ao, skinned } = variant;
        let vertex_attributes = VERTEX_ATTRIBUTES;
        let instance_attributes = INSTANCE_ATTRIBUTES;
        let skin_attributes = SKIN_ATTRIBUTES;
        let buffers = forward_buffers(&vertex_attributes, &instance_attributes, &skin_attributes);
        // Cut-outs get their own pipelines: a shader that can drop pixels
        // costs tile GPUs their hidden-surface removal, so opaque surfaces
        // don't use it. Its alpha becomes MSAA coverage, and it writes
        // colour only, so the target stays opaque. Blended surfaces are
        // tested against the depth but don't write it, so whatever is
        // drawn after them shows through; their colour comes premultiplied
        // by their alpha, and their alpha leaves the target's as it is.
        let (front_face, cull_mode) = match faces {
            Faces::Front => (wgpu::FrontFace::Ccw, Some(wgpu::Face::Back)),
            Faces::Mirrored => (wgpu::FrontFace::Cw, Some(wgpu::Face::Back)),
            Faces::Both => (wgpu::FrontFace::Ccw, None),
        };
        let on = |yes: bool| if yes { 1.0 } else { 0.0 };
        let constants = [
            ("double_sided", on(faces == Faces::Both)),
            ("shadowed", on(shadowed)),
            ("textured", on(textured)),
            ("ao", on(ao)),
            ("sky_lit", on(sky_lit)),
        ];
        let compilation_options = wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() };
        // Into the scene image, alpha says how much is light (bloom): all
        // of what's drawn, over what's behind as the colour is. Into a
        // view, the target stays opaque.
        let alpha = if self.format.is_srgb() { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL };
        let (label, entry_point, blend, write_mask) = match cover {
            Cover::Opaque => ("forward", "fs_main", None, wgpu::ColorWrites::ALL),
            Cover::Cutout => ("forward cut-out", "fs_cutout", None, alpha),
            Cover::Transparent => (
                "forward transparent",
                "fs_transparent",
                Some(wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                }),
                alpha,
            ),
            Cover::Additive => (
                "forward additive",
                "fs_additive",
                Some(wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                }),
                alpha,
            ),
        };
        let blend = blend.map(|color| wgpu::BlendState { color, alpha: color });
        let label = if name.is_empty() { label.to_owned() } else { format!("{label} ({name})") };
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: if skinned { &buffers } else { &buffers[..2] },
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face,
                cull_mode,
                ..Default::default()
            },
            // Reversed-Z: near is 1, far is 0, so nearer means greater. Float
            // depth has most precision near 0, which is where reversed-Z puts
            // the distance, where perspective has least.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(!cover.blended()),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: Renderer::SAMPLES,
                alpha_to_coverage_enabled: cover == Cover::Cutout,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(entry_point),
                compilation_options,
                targets: &[Some(wgpu::ColorTargetState { format: self.format, blend, write_mask })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    fn behind_shader(&self) -> &wgpu::ShaderModule {
        self.behind_shader.get_or_init(|| self.device.create_shader_module(wgpu::include_wgsl!("behind.wgsl")))
    }

    /// Copies one sample of the solid scene's multisampled depth into the
    /// depth shaders that read what's behind see.
    fn copy_depth(&self) -> &wgpu::RenderPipeline {
        self.copy_depth.get_or_init(|| {
            let shader = self.behind_shader();
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("scene depth"),
                bind_group_layouts: &[Some(&self.msaa_depth_layout)],
                immediate_size: 0,
            });
            self.maker.make(wgpu::RenderPipelineDescriptor {
                label: Some("scene depth"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_depth"),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// Puts the solid scene's resolved colour back into the multisampled
    /// target, every sample, for the see-through draws to go over. The
    /// depth is kept, and left as it is.
    fn restore(&self) -> &wgpu::RenderPipeline {
        self.restore.get_or_init(|| {
            let shader = self.behind_shader();
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("what's behind"),
                bind_group_layouts: &[Some(&self.image_layout)],
                immediate_size: 0,
            });
            self.maker.make(wgpu::RenderPipelineDescriptor {
                label: Some("what's behind"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: Renderer::SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_restore"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// Debug lines' pipeline.
    fn lines(&self) -> &wgpu::RenderPipeline {
        self.lines.get_or_init(|| self.make_lines())
    }

    fn make_lines(&self) -> wgpu::RenderPipeline {
        // Lines: over everything (depth always passes, and isn't written),
        // blended by their alpha.
        let line_shader = self.device.create_shader_module(wgpu::include_wgsl!("lines.wgsl"));
        let line_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lines"),
            bind_group_layouts: &[Some(&self.frame_layout)],
            immediate_size: 0,
        });
        let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some("lines"),
            layout: Some(&line_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &line_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: Renderer::SAMPLES, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    // Colour only: the target stays opaque.
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// The sky's pipeline: one triangle at the far plane, drawn where
    /// nothing is (reversed-Z: the depth is still its clear value, 0).
    fn sky(&self) -> &wgpu::RenderPipeline {
        self.sky.get_or_init(|| {
            let shader = self.device.create_shader_module(wgpu::include_wgsl!("sky.wgsl"));
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky"),
                bind_group_layouts: &[Some(&self.frame_layout), Some(&self.sky_image_layout)],
                immediate_size: 0,
            });
            self.maker.make(wgpu::RenderPipelineDescriptor {
                label: Some("sky"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: Renderer::SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.format,
                        blend: None,
                        // Its alpha into the scene image: an image is light,
                        // a gradient isn't (as the forward pass's).
                        write_mask: if self.format.is_srgb() {
                            wgpu::ColorWrites::COLOR
                        } else {
                            wgpu::ColorWrites::ALL
                        },
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// The last pass's pipeline, onto a view (of an sRGB format), adding
    /// bloom's glow or not.
    fn finish(&self, bloom: bool) -> &wgpu::RenderPipeline {
        assert!(self.format.is_srgb(), "a view's format is sRGB");
        match bloom {
            false => self.finish.get_or_init(|| self.make_finish(false)),
            true => self.finish_bloom.get_or_init(|| self.make_finish(true)),
        }
    }

    /// A screen effect's pipeline, onto a view's format, made the first
    /// time.
    fn effect(&self, shader: &CustomShader) -> wgpu::RenderPipeline {
        self.effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(shader.hash)
            .or_insert_with(|| self.make_effect(&shader.name, &shader.wgsl))
            .clone()
    }

    /// An effect that changes nothing, for one freed since its frame was
    /// prepared.
    fn effect_copy(&self) -> wgpu::RenderPipeline {
        const COPY: &str = concat!(
            include_str!("effect.wgsl"),
            "@fragment\nfn fs_fy_effect(in: FyOut) -> @location(0) vec4<f32> {\n",
            "    return vec4<f32>(picture_at(vec2<i32>(in.clip.xy)).rgb, 1.0);\n}\n"
        );
        self.effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(0)
            .or_insert_with(|| self.make_effect("copy", COPY))
            .clone()
    }

    fn make_effect(&self, name: &str, source: &str) -> wgpu::RenderPipeline {
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screen effect"),
            bind_group_layouts: &[Some(&self.effect_layout)],
            immediate_size: 0,
        });
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(name),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fy_effect"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_fy_effect"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    fn make_finish(&self, bloom: bool) -> wgpu::RenderPipeline {
        // The last pass, onto a view.
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("finish"),
            source: wgpu::ShaderSource::Wgsl(concat!(include_str!("neutral.wgsl"), include_str!("finish.wgsl")).into()),
        });
        let groups = [Some(&self.image_layout), Some(&self.bloom_layout)];
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("finish"),
            bind_group_layouts: if bloom { &groups } else { &groups[..1] },
            immediate_size: 0,
        });
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some("finish"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if bloom { "fs_bloom" } else { "fs_main" }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}

/// Where a draw goes among `map`'s casters: opaque ones need no textures,
/// so they group by mesh alone. One whose shader cuts it out or moves its
/// vertices (`moving`) is drawn with that shader.
fn caster_key(map: u8, draw: &crate::Draw, skinned: bool, look: Option<Look>, moving: bool) -> CasterKey {
    let m = &draw.material;
    let cutout = m.alpha_cutoff > 0.0;
    let own = look.filter(|(shader, _)| (cutout || moving) && shader.is_some());
    let textures = if cutout && own.is_none() { m.textures } else { MaterialTextures::default() };
    (map, own, cutout, skinned, textures, draw.mesh.0)
}

/// A draw's sort key: how it covers what's behind, which faces, whether
/// it's bent, its shader, its textures, whether it's outlined and its mesh,
/// in the order draws are grouped.
type SortKey = (Cover, Faces, bool, Option<ShaderKey>, OwnTextures, bool, u64);

/// A skinned mesh's joints and weights (`fynkel_mesh::SkinVertex`), as
/// skin.wgsl reads them.
pub(crate) const SKIN_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![14 => Uint16x4, 15 => Unorm16x4];

/// A mesh's position, normal, tangent and UV, as in `fynkel_mesh::Vertex`.
const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2];

/// An [`Instance`], as forward.wgsl reads it.
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
    7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
    11 => Float32x4, 12 => Uint32, 13 => Float32x4,
];

/// The forward pass's vertex buffers: the mesh's, the instances and a
/// skinned mesh's joints.
fn forward_buffers<'a>(
    vertex: &'a [wgpu::VertexAttribute],
    instance: &'a [wgpu::VertexAttribute],
    skin: &'a [wgpu::VertexAttribute],
) -> [Option<wgpu::VertexBufferLayout<'a>>; 3] {
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: vertex,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: INSTANCE_SIZE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: instance,
        }),
        Some(skin_buffer(skin)),
    ]
}

/// The vertex buffer of a skinned mesh's joints and weights: slot 2.
pub(crate) fn skin_buffer(attributes: &[wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'_> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<fynkel_mesh::SkinVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes,
    }
}

/// Adds `draw` to the last of `batches` if it has `key`, or starts another.
/// `values` is where the materials' values start among the joints.
fn batch(batches: &mut Vec<Batch>, instances: &mut Vec<Instance>, key: SortKey, draw: &crate::Draw, values: u32) {
    let (cover, faces, skinned, shader, textures, outlined, _) = key;
    let first = instances.len() as u32;
    match batches.last_mut() {
        Some(b) if (b.cover, b.faces, b.skinned, b.shader, b.textures, b.outlined, b.mesh.0) == key => b.count += 1,
        _ => {
            batches.push(Batch { cover, faces, skinned, outlined, shader, textures, mesh: draw.mesh, first, count: 1 })
        }
    }
    instances.push(instance(draw, skinned, values));
}

/// `draw`'s instance, as forward.wgsl reads it.
fn instance(draw: &crate::Draw, skinned: bool, values: u32) -> Instance {
    let m = &draw.material;
    let [er, eg, eb] = m.emissive;
    let ([su, sv], [ou, ov]) = (m.uv_scale, m.uv_offset);
    Instance {
        model: {
            let rows = draw.model.transpose();
            [rows.x_axis.into(), rows.y_axis.into(), rows.z_axis.into()]
        },
        base_color: m.base_color,
        params: [m.metallic, m.roughness, m.alpha_cutoff, if skinned { draw.skin.unwrap_or(0) as f32 } else { 0.0 }],
        emissive: [er, eg, eb, if draw.receive_shadows { 1.0 } else { 0.0 }],
        uv: [su, sv, ou, ov],
        own: draw.own,
        values: m.shader.map_or(0, |used| values + used.values),
        outline: m.outline.map_or([0.0; 4], |o| [o.color[0], o.color[1], o.color[2], o.width]),
    }
}

/// Moves `camera` across its view to the nearest whole pixel of a picture
/// `pixels` high ([`CameraView::pixels`]): for a perspective camera, a
/// pixel where it looks at the ground, or 10 m away.
fn snap_to_pixels(camera: &mut crate::CameraView, pixels: u32) {
    let world = camera.view.inverse();
    let (right, up, forward) = (world.x_axis.truncate(), world.y_axis.truncate(), -world.z_axis.truncate());
    let height = match camera.orthographic {
        Some(half) => half * 2.0,
        None => {
            let ground = camera.position.y > 0.0 && forward.y < -1e-3;
            let distance = if ground { camera.position.y / -forward.y } else { 10.0 };
            2.0 * (camera.fov_y * 0.5).tan() * distance
        }
    };
    let texel = height / pixels as f32;
    if !(texel.is_finite() && texel > 0.0) {
        return;
    }
    let snap = |along: f32| (along / texel).round() * texel - along;
    let moved = right * snap(camera.position.dot(right)) + up * snap(camera.position.dot(up));
    camera.position += moved;
    camera.view *= Mat4::from_translation(-moved);
}

/// The six planes around what `view_proj` shows (left, right, bottom, top,
/// far and near, for reversed-Z with depth from 0 to 1), each as a normal
/// pointing inside and a distance, scaled so the normal is unit length.
fn frustum_planes(view_proj: Mat4) -> [Vec4; 6] {
    let row = |i: usize| view_proj.row(i);
    let (x, y, z, w) = (row(0), row(1), row(2), row(3));
    [w + x, w - x, w + y, w - y, z, w - z].map(|p| p / p.truncate().length().max(f32::MIN_POSITIVE))
}

/// The world-space centre and radius of a mesh's bounding sphere, at
/// `centre` with `radius` in its own space, when placed by `model`.
fn bounds(model: Mat4, centre: Vec3, radius: f32) -> (Vec3, f32) {
    // The longest axis's stretch: the sphere stays around the mesh under
    // any scale.
    let stretch =
        model.x_axis.truncate().length().max(model.y_axis.truncate().length()).max(model.z_axis.truncate().length());
    (model.transform_point3(centre), radius * stretch)
}

/// The world-space bounding sphere of a skinned mesh bent by `joints`
/// (from its skin's first): the box around each joint's sphere (`spheres`,
/// in the mesh's space as bound) where that joint takes it, and the sphere
/// around that box. So an arm swung out stays in it.
fn skinned_bounds(joints: &[Mat4], spheres: &[Option<(Vec3, f32)>]) -> (Vec3, f32) {
    let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for (joint, sphere) in joints.iter().zip(spheres) {
        let Some((centre, radius)) = *sphere else { continue };
        let (at, r) = bounds(*joint, centre, radius);
        lo = lo.min(at - r);
        hi = hi.max(at + r);
    }
    if lo.x > hi.x {
        return (Vec3::ZERO, 0.0);
    }
    ((lo + hi) * 0.5, (hi - lo).length() * 0.5)
}

/// Whether a sphere at `at` with radius `r` can be seen at all: false only
/// when it is wholly outside one of the `planes`.
fn in_view(planes: &[Vec4; 6], at: Vec3, r: f32) -> bool {
    planes.iter().all(|p| p.truncate().dot(at) + p.w >= -r)
}

/// A point or spot light as forward.wgsl reads it.
fn light_uniform(light: &crate::Light) -> LightUniform {
    let (position, range, direction, cone) = match light.kind {
        LightKind::Sun { .. } => unreachable!("suns aren't in the frame's list"),
        LightKind::Point { position, range } => (position, range, Vec3::ZERO, [0.0, 1.0]),
        LightKind::Spot { position, direction, range, angle, softness } => {
            // The fade runs from the cone's edge in to `softness` of the way
            // to its axis, linear in the cosine of the angle from the axis.
            let outer = (angle * 0.5).clamp(0.0, std::f32::consts::FRAC_PI_2);
            let inner = outer * (1.0 - softness.clamp(0.0, 1.0));
            let (cos_outer, cos_inner) = (outer.cos(), inner.cos());
            let scale = 1.0 / (cos_inner - cos_outer).max(1e-4);
            (position, range, direction.normalize_or_zero(), [scale, -cos_outer * scale])
        }
    };
    LightUniform {
        position: position.extend(1.0 / (range * range)).into(),
        color: light.color.extend(0.0).into(),
        direction: direction.extend(0.0).into(),
        cone: [cone[0], cone[1], -1.0, 0.0],
    }
}
