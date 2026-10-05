//! `fynkel_render` — draws `fynkel_core`'s render extract.
//!
//! Blueprint §8.5, renderer V1 (a) and (b) (§19.2d items 4 and 5;
//! `docs/renderer.md` is the reference). The renderer is pure Rust and
//! host-agnostic: hosts hand it a device, a target format and an
//! [`Extract`]; where the target comes from (a winit swapchain, an
//! IOSurface ring, an offscreen texture) is the host's business.
//!
//! What it draws, in the pass chain `shadow → opaque → transparent → post
//! → presentation` ([`ExtractRenderer::draw`] documents each pass's inputs
//! and outputs):
//! * every `MeshRenderer` in the extract with its cooked mesh and its
//!   `fynkel.pbr@1` material — Cook-Torrance (GGX, Smith, Schlick) with
//!   every map (base colour, normal, metallic-roughness, occlusion,
//!   emissive), `alphaMode` mask by discard, `blend` sorted back-to-front
//!   in the transparent pass, double-sided materials without culling —
//!   read from the cooked cache through the [`Assets`] residency store on
//!   first use (the store a world loaded its scene from; GPU copies go
//!   when the store evicts);
//! * the extract's camera (`Camera` component, highest priority;
//!   perspective or orthographic), its first `DirectionalLight` as the
//!   sun — shadowed by one PCF-filtered shadow map when it casts — the
//!   eight nearest `PointLight`/`SpotLight`s, and its `Environment`
//!   (constant ambient, optional equirectangular sky) — or the host-set
//!   [`Camera`] and light when the scene has none;
//! * in physical units into a linear HDR intermediate (multisampled and
//!   scaled by the active [`Quality`] — the project's `quality` profile,
//!   switchable at runtime), exposed by the camera's EV100
//!   (`Camera.exposure`, or metered from the sun), ACES tone mapped and
//!   sRGB encoded into the host's target;
//! * optionally ([`ExtractRenderer::placeholders`]) a lit unit cube for
//!   every spatial entity without a `MeshRenderer` — the M1 look;
//! * a magenta cube where a mesh is missing from the cache, a plain
//!   surface where a texture or material is;
//! * the [`DebugDraw`] lines a host queued (AABBs, axes) on top.
//!
//! Allocation policy: instance, light and joint GPU buffers grow geometrically;
//! extraction and lighting scratch capacities are reused. Batch assembly and
//! asset-reference copies may still allocate on the CPU. Pipelines are a fixed set created
//! up front (rebuilt only when a quality switch changes the sample
//! count); bind groups are cached per material and per texture.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fynkel_contracts::{AssetRef, Uuid};
use fynkel_cooked::payload::{
    CustomMaterialPayload, MaterialSlot, SkinWeight, Vertex, ALPHA_BLEND, ALPHA_MASK,
    FALLBACK_BLACK, FALLBACK_GRAY, FALLBACK_NORMAL, TEXTURE_FORMAT_RGBA16F_LINEAR,
    TEXTURE_FORMAT_RGBA8_SRGB,
};
use fynkel_core::material::AlphaMode;
use fynkel_core::volume::{self, PostSettings, VolumeProfile};
use fynkel_core::{Environment, Extract, Projection};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

mod camera;
pub mod culling;
pub mod debug;
pub mod exposure;
mod gpu_timer;
mod gpu_types;
mod headless;
mod ibl;
mod lights;
mod outline;
pub mod overlay;
mod passes;
mod pipelines;
mod prepare;
pub mod quality;
mod residency;
pub mod shadow;
mod spatial;
mod stats;
mod streaming;
mod visibility;
pub use streaming::UploadBudget;
mod surface;
mod targets;
mod upload;

use gpu_timer::*;
use gpu_types::*;
use ibl::*;
pub use lights::LightBudget;
use lights::*;
use passes::*;
use pipelines::*;
use surface::{Pass, SurfaceContext, SurfaceShader};
use targets::*;
use upload::*;

pub use camera::Camera;
pub use culling::{Frustum, Sphere};
pub use debug::{DebugDraw, DebugVertex};
pub use fynkel_assets::{Assets, EnvironmentPayload, PbrMaterial, QualityProfile, Warm};
pub use headless::{
    block_on, clear_rgba8, device_features, device_limits, headless_device, render_offscreen,
};
pub use overlay::{Grid, OutlineStyle, Overlay, OverlayLine, OverlayTriangle, MAX_OUTLINE_WIDTH};
pub use quality::{Quality, QualityStatus};
pub use stats::FrameStats;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The linear HDR intermediate every frame is shaded into.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The sun's shadow map format.
pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The screen-space stage's targets: red the ambient occlusion (R2),
/// green the contact shadows (R3d); a byte each, 1 unoccluded.
pub const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg8Unorm;
/// Bloom's mip chain (R3b): exposed radiance, half the internal size and
/// down.
pub const BLOOM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Levels of the bloom chain at most: at 1280×720 the last is 20×11.
pub const BLOOM_LEVELS: u32 = 6;

/// Point + spot lights one object can be lit by — the blueprint's Tier-1
/// budget. The strongest at the object's bounds win (E1 light culling).
pub const MAX_LIGHTS: usize = 8;

/// Default frame light budget (configurable through `set_light_budget`).
/// Lights whose range misses the
/// camera frustum never enter it (that cannot change the picture: only
/// what is inside the frustum is shaded); when more than this reach the
/// view, the nearest to the camera win and the rest are counted dropped.
pub const LIGHT_POOL: usize = 1024;
/// Maximum explicit frame budget (one MiB of local-light storage).
pub const MAX_LIGHT_POOL: usize = 16_384;

/// Background colour: the bytes written to the target where nothing was
/// drawn (no tone mapping or encoding applies). Shared with the
/// pixel-readback tests.
pub const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.08,
    g: 0.09,
    b: 0.11,
    a: 1.0,
};

/// The tint of a placeholder cube standing in for a mesh the cache does
/// not have.
pub const MISSING_TINT: [f32; 4] = [1.0, 0.0, 1.0, 1.0];

/// Alpha cutoff value meaning "opaque, no mask" in [`Instance::flags`].
const NO_CUTOFF: f32 = -1.0;
/// Alpha cutoff value meaning "blend" (the transparent pass).
const BLEND: f32 = -2.0;

/// Receiver normal offset, in shadow texels.
const SHADOW_NORMAL_OFFSET_TEXELS: f32 = 1.5;

/// Added to [`Instance::flags`]`[3]` (receives shadows, 0 or 1) for an
/// instance of a grey [`ExtraWorld`]: the mesh stage reads bit 0 as
/// "receives shadows" and bit 1 as "grey".
const GRAY_FLAG: f32 = 2.0;

/// Another world drawn into the next frame beside the extract `prepare` is
/// given (the editor's Prefab Mode in context, multi-scene editing):
/// [`ExtractRenderer::push_extra_world`].
///
/// Its instances share the frame's depth buffer, light pool and shadow
/// maps, their model matrices pre-multiplied by `transform`; its point and
/// spot lights (placed by `transform`, their range scaled by its largest
/// axis) join the frame's pool and its casters the sun's shadow; its
/// skinned meshes are posed by its own joint palettes. Its first
/// directional light is the sun only when the main extract has none. Its
/// camera, `Environment` and volumes are ignored: the main extract's are
/// the frame's. Meshes and materials come from the renderer's asset store
/// (the main world's), so an extra world must reference the same cache.
/// The selection outline only rings the main extract's entities.
#[derive(Clone, Debug)]
pub struct ExtraWorld {
    pub extract: Extract,
    /// World of the extract → world of the frame (column-major).
    pub transform: Mat4,
    /// Drawn greyed — Unity's Prefab Mode context in "Gray": the shaded
    /// colour (lit, emissive and unlit alike) desaturated to its Rec. 709
    /// luminance and halved, `rgb' = 0.5 · dot(rgb, (0.2126, 0.7152,
    /// 0.0722))`, alpha kept; still depth-tested, shadowed and casting.
    pub gray: bool,
}

const LIGHT_POINT: f32 = 0.0;
const LIGHT_SPOT: f32 = 1.0;

/// The mesh vertex stage every surface is drawn over, and the engine's
/// light every lit surface ends in (ADR-0031): the built-in `pbr.wgsl`
/// and each shader asset are assembled after `frame.wgsl` from these.
const MESH_STAGE: &str = concat!(
    include_str!("shaders/mesh_input.wgsl"),
    include_str!("shaders/mesh.wgsl")
);
const LIGHT_LIBRARY: &str = include_str!("shaders/light.wgsl");

/// One cascade's slot in the shadow pass's uniform buffer: the pass
/// binds it by dynamic offset, so the four fits ride one buffer.
const SHADOW_UNIFORM_STRIDE: u64 = 256;

/// The sun's shadow, the scene, post (+ present) and the ambient
/// occlusion stage: begin/end query pairs (`GpuTimer`). The AO pair is
/// split across three passes — its begin is the depth prepass's, its end
/// the blur's — so the stage is one span. Six additional queries
/// identify depth/resolve/estimate/blur boundaries for opt-in tracing.
const TIMESTAMP_QUERIES: u32 = 16;
/// The query pair the ambient-occlusion stage writes (after the four the
/// original pass chain uses: shadow 0, scene 1, post 2, presentation 3).
const AO_PAIR: u32 = 4;
const TIMESTAMP_SLOTS: usize = 3;

/// What a frame draws: filled from an [`Extract`] by [`ExtractRenderer::prepare`].
pub struct ExtractRenderer {
    format: wgpu::TextureFormat,
    pipelines: Pipelines,
    frame_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    single_layout: wgpu::BindGroupLayout,
    /// The AO passes' two layouts (R2); `ao_layout` follows the sample
    /// count, so a quality switch rebuilds it with the pipelines.
    ao_layout: wgpu::BindGroupLayout,
    ao_frame_layout: wgpu::BindGroupLayout,
    ao_frame_bind_group: wgpu::BindGroup,
    /// The post pass's group 1: the HDR, its parameters and bloom's first
    /// level (R3b).
    post_layout: wgpu::BindGroupLayout,
    /// A bloom pass's group 1: the level (or HDR frame) it reads.
    bloom_layout: wgpu::BindGroupLayout,
    shadow_layout: wgpu::BindGroupLayout,
    skin_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    shadow_sampler: wgpu::Sampler,
    /// Clamped trilinear, for the environment cubes and the BRDF table.
    cube_sampler: wgpu::Sampler,
    present_sampler: wgpu::Sampler,
    cube: GpuMesh,
    fallbacks: Option<Fallbacks>,
    uniform_buffer: wgpu::Buffer,
    shadow_uniform_buffer: wgpu::Buffer,
    post_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    shadow_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u32,
    instances: Vec<Instance>,
    /// The frame's joint palettes (`Extract::joints`), read by the skinned
    /// vertex stages through `joint_bind_group` (group 2).
    joint_buffer: wgpu::Buffer,
    joint_capacity: u32,
    joint_bind_group: wgpu::BindGroup,
    batches: Vec<Batch>,
    transparent: Vec<TransparentDraw>,
    batch_index: fynkel_contracts::FastMap<BatchKey, usize>,
    /// Reused by `assign_lights`: (illuminance at the bounds, pool index).
    light_scratch: Vec<(f32, u32)>,
    light_budget: LightBudget,
    light_frame: LightFrameScratch,
    local_light_buffer: wgpu::Buffer,
    local_light_capacity: usize,
    light_index: spatial::SpatialIndex,
    light_candidates: Vec<usize>,
    /// One light list per instance, in the instance buffer's order — the
    /// storage array the mesh stages index with `Instance.flags[2]`.
    light_lists: Vec<[u32; 12]>,
    light_list_buffer: wgpu::Buffer,
    light_list_capacity: u32,
    debug: DebugDraw,
    /// Gizmos and handles for the next frame, and the pass that draws
    /// them after post (ADR-0019).
    overlay: Overlay,
    overlay_pass: overlay::OverlayPass,
    /// The selection outline (`outline.rs`): the composite and the mask,
    /// and the outlined instances — copies of theirs carrying the mask
    /// value — in their own buffer.
    outline_pass: outline::OutlinePass,
    outline_draws: Vec<OutlineDraw>,
    outline_instances: Vec<Instance>,
    outline_buffer: wgpu::Buffer,
    outline_capacity: u32,
    /// The worlds the next `prepare` draws beside its extract, and the
    /// joint palettes of a frame that has any, back to back.
    extra_worlds: Vec<ExtraWorld>,
    joint_scratch: Vec<Mat4>,
    attachments: Attachments,
    shadow_map: ShadowMap,
    /// The shadow map holds this frame's sun: the shadow pass runs and the
    /// mesh pass samples it.
    shadow_active: bool,
    /// Whether this frame's pass chain has the screen-space stage — the
    /// depth prepass, the ambient occlusion (R2) and contact shadows
    /// (R3d) — : the targets exist and either effect is on.
    ao_active: bool,
    /// Whether this frame's pass chain has the bloom stage (R3b): the
    /// profile allows it, the chain exists and the volumes give it an
    /// intensity.
    bloom_active: bool,
    /// The previous frame's `Extract::time`, for `globals.delta_time`.
    last_time: Option<f32>,
    /// The look the last `prepare` resolved from the volumes (R3a).
    post: PostSettings,
    sky_bind_group: Option<(AssetRef, wgpu::BindGroup)>,
    /// The environment the frame bind group holds (E1), and the 1×1 black
    /// stand-in bound when a scene has none.
    ibl: Option<(AssetRef, Ibl)>,
    ibl_none: Ibl,
    /// The frame bind group holds an environment other than `ibl`'s.
    ibl_dirty: bool,
    brdf_lut: GpuTexture,
    brdf_lut_texture: wgpu::Texture,
    width: u32,
    height: u32,
    quality: Quality,
    quality_profile: String,
    requested_profile: Option<String>,
    quality_override: Option<Quality>,
    unknown_profile: Option<String>,
    assets: Option<Assets>,
    asset_leases: fynkel_contracts::FastMap<Uuid, fynkel_assets::Residency>,
    asset_roots: fynkel_contracts::FastSet<Uuid>,
    asset_roots_scratch: fynkel_contracts::FastSet<Uuid>,
    asset_dependency_revision: (u64, u64),
    /// `Assets::evictions` when the GPU caches were last reconciled.
    edit_epoch_seen: u64,
    evictions_seen: u64,
    live_edits_seen: u64,
    mesh_edits_seen: u64,
    material_edits_seen: u64,
    meshes: fynkel_contracts::FastMap<AssetRef, Option<GpuMesh>>,
    textures: fynkel_contracts::FastMap<AssetRef, Option<GpuTexture>>,
    /// The cooked environments read for an HDR sky (E1; `None`: the
    /// texture has no `ENVM`, or it could not be read).
    environments: fynkel_contracts::FastMap<AssetRef, Option<std::sync::Arc<EnvironmentPayload>>>,
    materials: fynkel_contracts::FastMap<AssetRef, Option<GpuMaterial>>,
    /// Shader asset materials that draw as the error material (their
    /// `materials` entry is `None`).
    broken_materials: fynkel_contracts::FastSet<AssetRef>,
    /// Shader assets compiled for their materials (ADR-0031; `None`: it
    /// could not be read or compiled). Kept while a material uses one.
    surfaces: fynkel_contracts::FastMap<Uuid, Option<SurfaceShader>>,
    /// Shader asset materials' samplers by (filter, mipmaps, wrap,
    /// anisotropy).
    surface_samplers: fynkel_contracts::FastMap<[u32; 4], wgpu::Sampler>,
    /// Volume profiles (R3a; `None`: it could not be read).
    volume_profiles: fynkel_contracts::FastMap<AssetRef, Option<VolumeProfile>>,
    asset_errors: Vec<String>,
    frame_assets_deferred: bool,
    visibility_index: spatial::SpatialIndex,
    visibility_bounds: Vec<Sphere>,
    camera_visible: Vec<bool>,
    shadow_visible: Vec<bool>,
    asset_failures: fynkel_contracts::FastMap<AssetRef, Vec<String>>,
    stats: FrameStats,
    /// Pass timestamps when the device supports them (M3-16).
    gpu_timer: Option<GpuTimer>,
    /// The last timestamps read back (shadow, scene, post, frame; µs)
    /// and how many frames have been read back.
    gpu_times: (u32, u32, u32, u32, u32),
    gpu_timed: u32,
    ev100: f32,
    /// Fallback camera when the extract has none.
    pub camera: Camera,
    /// Fallback direction *toward* the light when the extract authors no
    /// lighting at all (a white [`exposure::FALLBACK_SUN_LUX`] sun; it
    /// casts shadows).
    pub light_dir: Vec3,
    /// Draw a lit cube for spatial entities without a `MeshRenderer`
    /// (the M1 behaviour; on by default).
    pub placeholders: bool,
    /// How long each frame may spend uploading what a background scene
    /// load made resident (ADR-0038 §5), in microseconds; 0 turns the
    /// warm-up off. Default 2000.
    pub warm_budget_us: u32,
    pub upload_budget: UploadBudget,
    /// Asset buffers/textures only; render targets and driver pipeline memory
    /// are reported separately by `gpu_bytes`. Refused assets use fallbacks.
    gpu_asset_budget_bytes: u64,
    assets_pending: bool,
    upload_usage: streaming::UploadUsage,
    /// Test every instance's bounding sphere against the camera frustum
    /// (and the sun's box) before uploading it; off, everything is drawn.
    /// On by default; off only to prove the picture does not change.
    pub frustum_culling: bool,
}

impl ExtractRenderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let quality = Quality::default();
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fynkel frame uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fynkel shadow uniforms"),
            size: SHADOW_UNIFORM_STRIDE * shadow::CASCADES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let post_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fynkel post uniforms"),
            contents: bytemuck::bytes_of(&PostUniforms {
                clear_color: [
                    CLEAR_COLOR.r as f32,
                    CLEAR_COLOR.g as f32,
                    CLEAR_COLOR.b as f32,
                    1.0,
                ],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let instance_capacity = 64;
        let instance_buffer = create_instance_buffer(device, instance_capacity);
        let overlay_pass = overlay::OverlayPass::new(device, format, quality.msaa);
        let outline_pass = outline::OutlinePass::new(device, format);
        let outline_capacity = 4;
        let outline_buffer = create_instance_buffer(device, outline_capacity);

        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let cube_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::Cube,
                multisampled: false,
            },
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel frame bgl"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                // Image-based lighting (E1): the irradiance and
                // prefiltered specular cubes, the split-sum BRDF table
                // and their sampler. Bound every frame — 1×1 black cubes
                // when the scene has no HDR environment.
                sampler_entry(3),
                cube_entry(4),
                cube_entry(5),
                texture_entry(6),
                // The frame's ambient occlusion (R2): a 1×1 white map
                // while the profile has it off.
                texture_entry(7),
            ],
        });
        // The shadow pass binds one cascade at a time out of the one
        // buffer, so its uniform takes a dynamic offset (R1).
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel shadow bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // A shader asset's shadow stage reads the time and the
                // camera in its fragment too (ADR-0031).
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<ShadowUniforms>() as u64
                    ),
                },
                count: None,
            }],
        });
        // Group 2 is what a mesh stage reads per instance: the frame's
        // joint palettes (M3-19) and its light lists (E1).
        let storage_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let skin_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel skin bgl"),
            entries: &[storage_entry(0), storage_entry(1), storage_entry(2)],
        });
        let joint_capacity = 64;
        let joint_buffer = create_joint_buffer(device, joint_capacity);
        let light_list_capacity = 256;
        let light_list_buffer = create_light_list_buffer(device, light_list_capacity);
        let local_light_capacity = LIGHT_POOL;
        let local_light_buffer = create_local_light_buffer(device, local_light_capacity);
        let joint_bind_group = create_joint_bind_group(
            device,
            &skin_layout,
            &joint_buffer,
            &light_list_buffer,
            &local_light_buffer,
        );
        let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fynkel shadow bg"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &shadow_uniform_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<ShadowUniforms>() as u64),
                }),
            }],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel material bgl"),
            entries: &[
                sampler_entry(0),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                texture_entry(5),
            ],
        });
        // Sky (texture), post (HDR + params) and present (LDR) share one
        // layout.
        let single_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel single-texture bgl"),
            entries: &[
                sampler_entry(0),
                texture_entry(1),
                uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel post bgl"),
            entries: &[
                sampler_entry(0),
                texture_entry(1),
                uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
                texture_entry(3),
            ],
        });
        let bloom_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel bloom bgl"),
            entries: &[sampler_entry(0), texture_entry(1)],
        });
        let ao_layout = create_ao_layout(device, quality.msaa);
        let ao_frame_layout = create_ao_frame_layout(device);
        let ao_frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fynkel ao frame bg"),
            layout: &ao_frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let sampler = create_material_sampler(device, quality.anisotropy);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fynkel shadow sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let cube_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fynkel environment sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let present_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fynkel present sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let pipelines = create_pipelines(
            device,
            format,
            quality.msaa,
            Layouts {
                frame: &frame_layout,
                material: &material_layout,
                single: &single_layout,
                shadow: &shadow_layout,
                skin: &skin_layout,
                ao_frame: &ao_frame_layout,
                ao: &ao_layout,
                post: &post_layout,
                bloom: &bloom_layout,
            },
        );

        let (vertices, indices) = cube_mesh();
        let cube = upload_mesh(
            device,
            "fynkel cube",
            &vertices,
            &indices,
            &[],
            Sphere::from_bounds([-0.5; 3], [0.5; 3]),
        );
        let (width, height) = (width.max(1), height.max(1));
        let attachments = create_attachments(
            device,
            format,
            &quality,
            width,
            height,
            AttachmentLayouts {
                present: &single_layout,
                post: &post_layout,
                ao: &ao_layout,
                bloom: &bloom_layout,
                ao_depth_resolve: &pipelines.ao_depth_resolve,
                ao_blur: &pipelines.ao_blur,
            },
            &present_sampler,
            &post_buffer,
        );
        let shadow_map = create_shadow_map(
            device,
            if quality.shadows {
                quality.shadow_resolution
            } else {
                1
            },
        );
        let ibl_none = Ibl::none(device);
        let (brdf_lut, brdf_lut_texture) = create_brdf_lut(device);
        let frame_bind_group = create_frame_bind_group(
            device,
            &frame_layout,
            &uniform_buffer,
            &shadow_map.view,
            &shadow_sampler,
            &cube_sampler,
            &ibl_none,
            &brdf_lut,
            // Never read before the first `prepare` (the uniform's AO
            // flag is 0 until then), and rebound with the real target or
            // the white fallback as soon as one runs.
            attachments
                .ao
                .as_ref()
                .map_or(&brdf_lut.view, |a| &a.blur_view),
        );

        Self {
            format,
            pipelines,
            frame_layout,
            material_layout,
            single_layout,
            ao_layout,
            ao_frame_layout,
            ao_frame_bind_group,
            post_layout,
            bloom_layout,
            shadow_layout,
            skin_layout,
            sampler,
            shadow_sampler,
            cube_sampler,
            present_sampler,
            cube,
            fallbacks: None,
            uniform_buffer,
            shadow_uniform_buffer,
            post_buffer,
            frame_bind_group,
            shadow_bind_group,
            instance_buffer,
            instance_capacity,
            instances: Vec::with_capacity(instance_capacity as usize),
            joint_buffer,
            joint_capacity,
            joint_bind_group,
            batches: Vec::new(),
            transparent: Vec::new(),
            batch_index: Default::default(),
            light_scratch: Vec::with_capacity(LIGHT_POOL),
            light_budget: LightBudget::default(),
            light_frame: Default::default(),
            local_light_buffer,
            local_light_capacity,
            light_index: Default::default(),
            light_candidates: Vec::new(),
            light_lists: Vec::new(),
            light_list_buffer,
            light_list_capacity,
            debug: DebugDraw::default(),
            overlay: Overlay::default(),
            overlay_pass,
            outline_pass,
            outline_draws: Vec::new(),
            outline_instances: Vec::new(),
            outline_buffer,
            outline_capacity,
            extra_worlds: Vec::new(),
            joint_scratch: Vec::new(),
            attachments,
            shadow_map,
            shadow_active: false,
            ao_active: false,
            bloom_active: false,
            last_time: None,
            post: PostSettings::default(),
            sky_bind_group: None,
            ibl: None,
            ibl_none,
            ibl_dirty: false,
            brdf_lut,
            brdf_lut_texture,
            width,
            height,
            quality,
            quality_profile: "default".to_string(),
            requested_profile: None,
            quality_override: None,
            unknown_profile: None,
            assets: None,
            asset_leases: Default::default(),
            asset_roots: Default::default(),
            asset_roots_scratch: Default::default(),
            asset_dependency_revision: (0, 0),
            edit_epoch_seen: 0,
            evictions_seen: 0,
            live_edits_seen: 0,
            mesh_edits_seen: 0,
            material_edits_seen: 0,
            meshes: Default::default(),
            textures: Default::default(),
            environments: Default::default(),
            materials: Default::default(),
            broken_materials: Default::default(),
            surfaces: Default::default(),
            surface_samplers: Default::default(),
            volume_profiles: Default::default(),
            asset_errors: Vec::new(),
            frame_assets_deferred: false,
            visibility_index: spatial::SpatialIndex::default(),
            visibility_bounds: Vec::new(),
            camera_visible: Vec::new(),
            shadow_visible: Vec::new(),
            asset_failures: Default::default(),
            stats: FrameStats::default(),
            gpu_timer: device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY)
                .then(|| GpuTimer::new(device)),
            gpu_times: (0, 0, 0, 0, 0),
            gpu_timed: 0,
            ev100: exposure::ev100_from_lux(exposure::FALLBACK_SUN_LUX),
            camera: Camera::default(),
            light_dir: Vec3::new(0.4, 1.0, 0.6).normalize(),
            placeholders: true,
            warm_budget_us: 2000,
            upload_budget: UploadBudget::default(),
            gpu_asset_budget_bytes: 512 * 1024 * 1024,
            assets_pending: false,
            upload_usage: streaming::UploadUsage::default(),
            frustum_culling: true,
        }
    }

    /// Rebuild every GPU-owned resource on a replacement device. Logical
    /// settings and asset ownership survive; no old device handle is reused.
    /// The host must also recreate its surface/targets and submit no more
    /// commands to the old device. Queued overlay/extra-world work is retained.
    pub fn recreate(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        let mut fresh = Self::new(device, format, self.width, self.height);
        fresh.camera = self.camera;
        fresh.placeholders = self.placeholders;
        fresh.frustum_culling = self.frustum_culling;
        fresh.warm_budget_us = self.warm_budget_us;
        fresh.upload_budget = self.upload_budget;
        fresh.light_budget = self.light_budget;
        fresh.gpu_asset_budget_bytes = self.gpu_asset_budget_bytes;
        fresh.requested_profile = self.requested_profile.clone();
        fresh.quality_override = self.quality_override;
        fresh.set_assets(self.assets.clone());
        fresh.overlay = std::mem::take(&mut self.overlay);
        fresh.debug = std::mem::take(&mut self.debug);
        fresh.extra_worlds = std::mem::take(&mut self.extra_worlds);
        *self = fresh;
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.size() == (width.max(1), height.max(1)) {
            return;
        }
        self.width = width.max(1);
        self.height = height.max(1);
        self.rebuild_attachments(device);
    }

    fn rebuild_attachments(&mut self, device: &wgpu::Device) {
        self.attachments = create_attachments(
            device,
            self.format,
            &self.quality,
            self.width,
            self.height,
            AttachmentLayouts {
                present: &self.single_layout,
                post: &self.post_layout,
                ao: &self.ao_layout,
                bloom: &self.bloom_layout,
                ao_depth_resolve: &self.pipelines.ao_depth_resolve,
                ao_blur: &self.pipelines.ao_blur,
            },
            &self.present_sampler,
            &self.post_buffer,
        );
        // Group 0 holds the AO target the mesh pass samples (R2).
        self.rebuild_frame_bind_group(device);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Number of instances the last `prepare` queued.
    pub fn instance_count(&self) -> u32 {
        self.stats.instances
    }

    pub fn set_light_budget(&mut self, budget: LightBudget) -> Result<(), String> {
        self.light_budget = budget.validate()?;
        Ok(())
    }
    pub fn light_budget(&self) -> LightBudget {
        self.light_budget
    }
    /// Actionable diagnostics when a scene exceeds the supported forward path.
    pub fn lighting_diagnostics(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.stats.lights_dropped > 0 {
            out.push(format!("{} visible local lights exceed the frame budget of {}; reduce simultaneous light overlap or split camera layers",self.stats.lights_dropped,self.light_budget.pool));
        }
        if self.stats.lights_overflow > 0 {
            out.push(format!(
                "{} objects exceed the per-object budget of {}; weakest contributions were omitted",
                self.stats.lights_overflow, self.light_budget.per_object
            ));
        }
        out
    }

    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    /// The look the last `prepare` resolved from the scene's volumes
    /// (R3a): the engine defaults when none reached the camera.
    pub fn post_settings(&self) -> PostSettings {
        self.post
    }

    /// The EV100 the last `prepare` exposed the frame at (the camera's
    /// manual value, or metered from the sun and ambient).
    pub fn ev100(&self) -> f32 {
        self.ev100
    }

    /// Lines to draw on the next frame (cleared by `prepare` once
    /// uploaded).
    pub fn debug(&mut self) -> &mut DebugDraw {
        &mut self.debug
    }

    /// Gizmos, handles and the ground grid to draw over the next frame
    /// (cleared by `prepare` once uploaded; ADR-0019).
    pub fn overlay(&mut self) -> &mut Overlay {
        &mut self.overlay
    }

    /// Queues `extra` to be drawn by the next `prepare` beside its extract
    /// (see [`ExtraWorld`]). The queue empties with every `prepare`: a
    /// host draws its extra worlds by queuing them before each frame.
    pub fn push_extra_world(&mut self, extra: ExtraWorld) {
        self.extra_worlds.push(extra);
    }

    /// The extra worlds queued for the next `prepare`.
    pub fn extra_worlds(&self) -> &[ExtraWorld] {
        &self.extra_worlds
    }

    /// Drops the extra worlds queued for the next `prepare`.
    pub fn clear_extra_worlds(&mut self) {
        self.extra_worlds.clear();
    }

    // ----------------------------------------------------------- quality

    /// Selects the project's quality profile by name (`None`: the
    /// project's default); applied by the next `prepare`, which reads the
    /// profiles from the asset store's project. Returns `false` when the
    /// current store's project has profiles and none is called `name` —
    /// the request is kept and the default applies until a store knows
    /// it. Clears a [`set_quality`](Self::set_quality) override.
    pub fn set_quality_profile(&mut self, name: Option<&str>) -> bool {
        self.requested_profile = name.map(str::to_string);
        self.quality_override = None;
        match (name, self.project_quality()) {
            (Some(n), Some(q)) => q.profile(n).is_some(),
            _ => true,
        }
    }

    /// Runs with explicit settings (clamped) whatever the project says,
    /// until a profile is selected.
    pub fn set_quality(&mut self, quality: Quality) {
        self.quality_override = Some(quality.clamped());
    }

    /// The quality in force as of the last `prepare`.
    pub fn quality(&self) -> QualityStatus {
        QualityStatus {
            profile: self.quality_profile.clone(),
            unknown_profile: self.unknown_profile.clone(),
            settings: self.quality,
            internal_size: (self.attachments.width, self.attachments.height),
        }
    }

    /// The profile names the current store's project declares.
    pub fn quality_profiles(&self) -> Vec<String> {
        self.project_quality()
            .map(|q| q.profiles.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default()
    }

    fn project_quality(&self) -> Option<fynkel_assets::Quality> {
        self.assets
            .as_ref()
            .and_then(Assets::project)
            .and_then(|p| p.quality)
    }

    /// Resolves the wanted settings (override → requested profile →
    /// project default → engine defaults) and applies a change.
    fn resolve_quality(&mut self, device: &wgpu::Device) {
        let (profile, wanted, unknown) = match (self.quality_override, self.project_quality()) {
            (Some(q), _) => ("custom".to_string(), q, None),
            (None, Some(project)) => {
                let default = || {
                    project
                        .default_profile()
                        .map(|p| (p.name.clone(), Quality::from(p)))
                        .unwrap_or_else(|| ("default".to_string(), Quality::default()))
                };
                match self.requested_profile.as_deref() {
                    Some(name) => match project.profile(name) {
                        Some(p) => (p.name.clone(), Quality::from(p), None),
                        None => {
                            let (n, q) = default();
                            (n, q, Some(name.to_string()))
                        }
                    },
                    None => {
                        let (n, q) = default();
                        (n, q, None)
                    }
                }
            }
            (None, None) => ("default".to_string(), Quality::default(), None),
        };
        self.quality_profile = profile;
        self.unknown_profile = unknown;
        if wanted != self.quality {
            self.apply_quality(device, wanted);
        }
    }

    /// Rebuilds exactly what `wanted` changes: the pipeline set for a new
    /// sample count, the attachments for a new internal size or sample
    /// count, the shadow map for a new resolution (or shadows toggled),
    /// the material sampler (and every bind group holding it) for a new
    /// anisotropy.
    fn apply_quality(&mut self, device: &wgpu::Device, wanted: Quality) {
        let old = self.quality;
        self.quality = wanted;
        if wanted.msaa != self.pipelines.sample_count {
            // The AO passes and the overlay read the depth as the sample
            // count stores it.
            self.ao_layout = create_ao_layout(device, wanted.msaa);
            self.overlay_pass.rebuild(device, self.format, wanted.msaa);
            self.pipelines = create_pipelines(
                device,
                self.format,
                wanted.msaa,
                Layouts {
                    frame: &self.frame_layout,
                    material: &self.material_layout,
                    single: &self.single_layout,
                    shadow: &self.shadow_layout,
                    skin: &self.skin_layout,
                    ao_frame: &self.ao_frame_layout,
                    ao: &self.ao_layout,
                    post: &self.post_layout,
                    bloom: &self.bloom_layout,
                },
            );
            let ctx = SurfaceContext {
                frame: &self.frame_layout,
                shadow: &self.shadow_layout,
                skin: &self.skin_layout,
                sample_count: wanted.msaa,
            };
            for s in self.surfaces.values_mut().flatten() {
                s.rebuild(device, &ctx);
            }
        }
        let internal = wanted.internal_size(self.width, self.height);
        if internal != (self.attachments.width, self.attachments.height)
            || wanted.msaa != self.attachments.sample_count
            || (wanted.render_scale != 1.0) != self.attachments.ldr_view.is_some()
            || (wanted.ambient_occlusion || wanted.contact_shadows) != self.attachments.ao.is_some()
            || wanted.bloom != self.attachments.bloom.is_some()
        {
            self.rebuild_attachments(device);
        }
        let resolution = if wanted.shadows {
            wanted.shadow_resolution
        } else {
            1
        };
        if resolution != self.shadow_map.resolution {
            self.shadow_map = create_shadow_map(device, resolution);
            self.rebuild_frame_bind_group(device);
        }
        if wanted.anisotropy != old.anisotropy {
            self.sampler = create_material_sampler(device, wanted.anisotropy);
            self.fallbacks = None;
            self.materials.clear();
            self.surface_samplers.clear();
            self.sky_bind_group = None;
        }
    }

    /// Rebinds group 0 after the shadow map or the environment changed.
    fn rebuild_frame_bind_group(&mut self, device: &wgpu::Device) {
        self.frame_bind_group = create_frame_bind_group(
            device,
            &self.frame_layout,
            &self.uniform_buffer,
            &self.shadow_map.view,
            &self.shadow_sampler,
            &self.cube_sampler,
            self.ibl.as_ref().map_or(&self.ibl_none, |(_, i)| i),
            &self.brdf_lut,
            // The frame's AO, or a white map: `counts.w` is 0 whenever
            // the fallback is bound, so the mesh pass never reads it.
            match (&self.attachments.ao, &self.fallbacks) {
                (Some(ao), _) => &ao.blur_view,
                (None, Some(f)) => &f.white.view,
                (None, None) => &self.brdf_lut.view,
            },
        );
    }

    // ------------------------------------------------------------ assets
}

/// The number of render pipelines a renderer ever creates.
pub const PIPELINE_VARIANTS: usize = Pipelines::count();

#[cfg(test)]
mod ao_blur_tests;
