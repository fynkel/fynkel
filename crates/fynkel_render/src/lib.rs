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
//! Allocation policy: the instance buffer grows geometrically and is
//! reused; a frame with no growth and no new assets allocates nothing on
//! the CPU side beyond the batch table. Pipelines are a fixed set created
//! up front (rebuilt only when a quality switch changes the sample
//! count); bind groups are cached per material and per texture.

use std::collections::HashMap;

use fynkel_contracts::AssetRef;
use fynkel_cooked::payload::{
    SkinWeight, Vertex, TEXTURE_FORMAT_RGBA16F_LINEAR, TEXTURE_FORMAT_RGBA8_SRGB,
};
use fynkel_core::material::AlphaMode;
use fynkel_core::volume::{self, PostSettings, VolumeProfile};
use fynkel_core::{Environment, Extract, Projection};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

pub mod culling;
pub mod debug;
pub mod exposure;
mod outline;
pub mod overlay;
pub mod quality;
pub mod shadow;

pub use culling::{Frustum, Sphere};
pub use debug::{DebugDraw, DebugVertex};
pub use fynkel_assets::{Assets, EnvironmentPayload, PbrMaterial, QualityProfile};
pub use overlay::{Grid, OutlineStyle, Overlay, OverlayLine, OverlayTriangle, MAX_OUTLINE_WIDTH};
pub use quality::{Quality, QualityStatus};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The linear HDR intermediate every frame is shaded into.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The sun's shadow map format.
pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The ambient-occlusion targets (R2): one byte a pixel, 1 unoccluded.
pub const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// Bloom's mip chain (R3b): exposed radiance, half the internal size and
/// down.
pub const BLOOM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Levels of the bloom chain at most: at 1280×720 the last is 20×11.
pub const BLOOM_LEVELS: u32 = 6;

/// Point + spot lights one object can be lit by — the blueprint's Tier-1
/// budget. The strongest at the object's bounds win (E1 light culling).
pub const MAX_LIGHTS: usize = 8;

/// Point + spot lights one frame can carry. Lights whose range misses the
/// camera frustum never enter it (that cannot change the picture: only
/// what is inside the frustum is shaded); when more than this reach the
/// view, the nearest to the camera win and the rest are counted dropped.
pub const LIGHT_POOL: usize = 64;

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

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    /// Base colour factor.
    tint: [f32; 4],
    /// Emissive × strength (relative to the exposure's white); w is the
    /// skinned draw's first joint in the palette buffer (M3-19).
    emissive: [f32; 4],
    /// metallic, roughness, normal_scale, occlusion_strength
    params: [f32; 4],
    /// alpha cutoff (or `NO_CUTOFF` / `BLEND`), unlit, the object's light
    /// list in the frame's list buffer (E1 — `z` was the double-sided
    /// flag, which only the CPU ever read: it picks the pipeline), and
    /// whether it receives shadows
    flags: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuLight {
    position: [f32; 4],
    color: [f32; 4],
    direction: [f32; 4],
    params: [f32; 4],
}

const LIGHT_POINT: f32 = 0.0;
const LIGHT_SPOT: f32 = 1.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    cascades: [GpuCascade; shadow::CASCADES],
    camera_pos: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
    sky: [f32; 4],
    shadow: [f32; 4],
    ao: [f32; 4],
    bloom: [f32; 4],
    bloom_tint: [f32; 4],
    counts: [u32; 4],
    lights: [GpuLight; LIGHT_POOL],
}

/// One cascade as the mesh shader reads it (R1): where it looks from,
/// which corner of the atlas it lives in, and what a receiver must be
/// pushed along its normal to stay out of its own shadow.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuCascade {
    /// World → this cascade's light clip.
    view_proj: [[f32; 4]; 4],
    /// The cascade's rectangle in atlas uv: origin, then size.
    rect: [f32; 4],
    /// x: normal offset in world units; yzw unused.
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ShadowUniforms {
    view_proj: [[f32; 4]; 4],
}

/// One cascade's slot in the shadow pass's uniform buffer: the pass
/// binds it by dynamic offset, so the four fits ride one buffer.
const SHADOW_UNIFORM_STRIDE: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostUniforms {
    clear_color: [f32; 4],
}

/// Right-handed, Y-up perspective camera (blueprint §4.5). Used when the
/// extract has no `Camera` component; exposure is then automatic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y_radians: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: Vec3::new(0.0, 1.5, 4.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_y_radians: 60f32.to_radians(),
            near: 0.1,
            far: 100.0,
        }
    }
}

impl Camera {
    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.eye, self.target, self.up)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(
            self.fov_y_radians,
            aspect,
            self.near,
            self.far,
        )
    }

    pub fn view_proj(&self, width: u32, height: u32) -> Mat4 {
        let aspect = width.max(1) as f32 / height.max(1) as f32;
        self.projection(aspect) * self.view()
    }
}

/// A mesh on the GPU, in the cooked vertex layout.
struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    /// A skinned mesh's joints and weights (`SkinWeight` per vertex),
    /// vertex slot 2 of the skinned pipelines (M3-19).
    skin_buffer: Option<wgpu::Buffer>,
    /// The cooked bounds as a sphere, in mesh space (culling).
    bounds: Sphere,
}

/// A texture ready for sampling.
struct GpuTexture {
    view: wgpu::TextureView,
    /// Bytes uploaded (every mip), for the GPU memory estimate.
    bytes: u64,
}

/// A material's five maps bound together, with the factors the instance
/// carries.
struct GpuMaterial {
    material: PbrMaterial,
    bind_group: wgpu::BindGroup,
    /// The texture refs the bind group holds views of.
    textures: Vec<AssetRef>,
}

/// One draw: a mesh, a material, a contiguous range of instances.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BatchKey {
    /// `None`: the built-in cube.
    mesh: Option<AssetRef>,
    /// `None`: the default material (every map at its fallback).
    material: Option<AssetRef>,
    /// Drawn through the skinned pipelines: the mesh carries skin weights
    /// and every instance a joint palette (M3-19).
    skinned: bool,
}

/// Where an opaque instance is drawn; the order is the batch layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Visibility {
    /// A caster outside the camera frustum but inside the sun's box.
    ShadowOnly = 0,
    /// A caster in view.
    Both = 1,
    /// In view, not casting.
    SceneOnly = 2,
}

/// An opaque (or mask) batch: one contiguous instance range for the
/// scene draw and one for the shadow draw.
struct Batch {
    key: BatchKey,
    double_sided: bool,
    /// The scene draw: `count` instances from `first` (those inside the
    /// camera frustum).
    first: u32,
    count: u32,
    /// The shadow draw: `shadow_count` instances from `shadow_first`
    /// (casters inside the light's box). The two ranges overlap on the
    /// casters that are in both; see `prepare` for the layout.
    shadow_first: u32,
    shadow_count: u32,
}

/// One outlined instance's draw into the selection outline's mask: its
/// batch's mesh and material, and its slot in the outline's own instance
/// buffer (the scene's batches are left as they are).
struct OutlineDraw {
    key: BatchKey,
    double_sided: bool,
    index: u32,
}

/// One `blend` instance, drawn on its own in back-to-front order.
struct TransparentDraw {
    key: BatchKey,
    double_sided: bool,
    index: u32,
}

/// Counts from the last `prepare`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Instances drawn in the scene passes (inside the camera frustum).
    pub instances: u32,
    /// Instances outside the camera frustum, not drawn in the scene (they
    /// may still cast into the shadow map).
    pub culled: u32,
    /// Records the scene camera's `culling_mask` leaves out (ADR-0020):
    /// neither drawn nor casting a shadow.
    pub layer_hidden: u32,
    /// Cooked meshes among the records (before culling).
    pub meshes: u32,
    pub placeholders: u32,
    pub missing: u32,
    /// Distinct opaque `(mesh, material)` batches.
    pub batches: u32,
    /// Mesh draw calls in the scene pass: opaque batches with something
    /// in view, plus one per transparent instance.
    pub draws: u32,
    /// Instances drawn skinned (a joint palette applied per vertex), and
    /// the joint matrices uploaded for them this frame.
    pub skinned: u32,
    pub joints: u32,
    /// `blend` instances drawn back-to-front (one draw each).
    pub transparent: u32,
    /// Instances drawn into the shadow map (0 when no shadow was drawn).
    pub shadow_casters: u32,
    /// Draw calls in the shadow pass.
    pub shadow_draws: u32,
    /// The shadow map side drawn this frame (0: no shadow — the profile
    /// has shadows off, or no sun casts).
    pub shadow_map: u32,
    /// Cascades drawn into that map's 2×2 atlas (0 when no shadow was
    /// drawn); each one is `shadow_map / 2` on a side.
    pub shadow_cascades: u32,
    /// Lines the overlay drew: the [`Overlay`] queue's and the
    /// [`DebugDraw`] queue's (ADR-0019).
    pub debug_lines: u32,
    /// Triangles the overlay drew.
    pub overlay_triangles: u32,
    /// Whether the overlay drew the ground grid.
    pub grid: bool,
    /// Instances drawn into the selection outline's mask (0: the frame
    /// has no outline, and no outline passes).
    pub outlined: u32,
    /// [`ExtraWorld`]s drawn beside the main extract this frame.
    pub extra_worlds: u32,
    /// Whether the frame ran the ambient-occlusion stage (R2).
    pub ambient_occlusion: bool,
    /// Whether the frame ran the bloom stage (R3b).
    pub bloom: bool,
    /// Volumes (R3a) whose profile reached the camera this frame.
    pub volumes: u32,
    /// Samples per pixel of the scene passes (1 or 4).
    pub msaa: u32,
    /// The internal render size (target × render scale).
    pub render_width: u32,
    pub render_height: u32,
    /// Point + spot lights uploaded to the frame's pool (≤ [`LIGHT_POOL`]):
    /// those whose range reaches the view.
    pub lights: u32,
    /// Point + spot lights that reached the view but did not fit the pool.
    pub lights_dropped: u32,
    /// The longest per-object light list this frame (≤ [`MAX_LIGHTS`]).
    pub lights_max_per_object: u32,
    /// Instances that had more lights on them than [`MAX_LIGHTS`]: the
    /// weakest at their bounds were left out.
    pub lights_overflow: u32,
    pub scene_camera: bool,
    pub scene_light: bool,
    /// The `Environment` sky is drawn behind the meshes.
    pub sky: bool,
    /// The sky is an HDR environment and lights the scene (E1): the
    /// prefiltered cubes are bound and the constant ambient is off.
    pub ibl: bool,
    /// Instrumentation (blueprint §15.4, M3-16). Wall time of `prepare`
    /// (upload + cull + the timestamp read-back) and of the last `draw`
    /// (pass recording; the submit is the host's), in microseconds.
    pub prepare_us: u32,
    pub encode_us: u32,
    /// Estimated GPU memory held: resident meshes and textures, the
    /// attachments at the internal size, the shadow map, the instance,
    /// joint and debug buffers.
    pub gpu_bytes: u64,
    /// The device samples timestamps at pass boundaries
    /// (`wgpu::Features::TIMESTAMP_QUERY`; Metal on Apple GPUs). Without
    /// it the four GPU times stay 0.
    pub gpu_timestamps: bool,
    /// GPU time of the most recently read-back frame's passes (the
    /// read-back trails the draw by three or four frames): the shadow
    /// pass, the ambient-occlusion stage (the depth prepass, SSAO and its
    /// blur, timed as one span), the scene pass (opaque + sky +
    /// transparent, resolved), post (+ present), and the frame's span
    /// from the first pass's begin to the last pass's end — less than the
    /// sum on a tile-based GPU, whose passes overlap.
    pub gpu_shadow_us: u32,
    pub gpu_ao_us: u32,
    pub gpu_scene_us: u32,
    pub gpu_post_us: u32,
    pub gpu_frame_us: u32,
    /// Frames drawn whose timestamps could not be taken because every
    /// read-back slot was still in flight (the GPU more than three frames
    /// behind); cumulative.
    pub gpu_untimed: u32,
    /// Frames whose timestamps have been read back; cumulative, so a host
    /// sampling per frame knows when the four times are new.
    pub gpu_timed: u32,
}

/// The sun's shadow, the scene, post (+ present) and the ambient
/// occlusion stage: begin/end query pairs (`GpuTimer`). The AO pair is
/// split across three passes — its begin is the depth prepass's, its end
/// the blur's — so the stage is one span.
const TIMESTAMP_QUERIES: u32 = 10;
/// The query pair the ambient-occlusion stage writes (after the four the
/// original pass chain uses: shadow 0, scene 1, post 2, presentation 3).
const AO_PAIR: u32 = 4;
const TIMESTAMP_SLOTS: usize = 3;

/// A read-back slot of the [`GpuTimer`]: `state` 0 free, 1 copied into by
/// a `draw` (the host submits it), 2 mapping requested, 3 mapped.
struct TimestampSlot {
    buffer: wgpu::Buffer,
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    /// Which pass pairs the frame wrote (a skipped shadow, AO or present
    /// pass leaves stale values in its queries).
    shadow: bool,
    ao: bool,
    present: bool,
}

/// Pass timestamps (`wgpu::Features::TIMESTAMP_QUERY`, M3-16): a frame's
/// passes write begin/end timestamps into one of two query sets; the next
/// frame's `draw` resolves the previous set before its own passes (on a
/// tile-based GPU a resolve in the same command buffer can run before the
/// last pass's fragment stage has written its end stamp) and copies it
/// into a free read-back slot; `prepare` maps the slots the host has
/// submitted and reads the ones that came back — never blocking, so the
/// numbers describe a frame three or four behind the one being prepared.
struct GpuTimer {
    query_sets: [wgpu::QuerySet; 2],
    /// The set this frame's passes write; the other holds the previous
    /// frame's stamps until `draw` resolves it.
    writing: usize,
    /// The previous frame's pass flags, resolved with its set.
    pending: Option<(bool, bool, bool)>,
    resolve: wgpu::Buffer,
    slots: Vec<TimestampSlot>,
    next: usize,
    untimed: u32,
}

impl GpuTimer {
    fn new(device: &wgpu::Device) -> Self {
        let query_sets = std::array::from_fn(|i| {
            device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some(&format!("fynkel pass timestamps {i}")),
                ty: wgpu::QueryType::Timestamp,
                count: TIMESTAMP_QUERIES,
            })
        });
        let size = u64::from(TIMESTAMP_QUERIES) * 8;
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fynkel timestamp resolve"),
            size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..TIMESTAMP_SLOTS)
            .map(|i| TimestampSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("fynkel timestamp read-back {i}")),
                    size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Default::default(),
                shadow: false,
                ao: false,
                present: false,
            })
            .collect();
        Self {
            query_sets,
            writing: 0,
            pending: None,
            resolve,
            slots,
            next: 0,
            untimed: 0,
        }
    }

    /// The pass's timestamp writes for query pair `pair`.
    fn writes(&self, pair: u32) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.writes_part(pair, true, true)
    }

    /// Half a pair: the AO stage's three passes share pair 4, the first
    /// writing its beginning and the last its end.
    fn writes_part(
        &self,
        pair: u32,
        begin: bool,
        end: bool,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.query_sets[self.writing],
            beginning_of_pass_write_index: begin.then_some(pair * 2),
            end_of_pass_write_index: end.then_some(pair * 2 + 1),
        })
    }

    /// Before a frame's passes: resolves the previous frame's set into the
    /// next free slot (the frame goes untimed when every slot is still in
    /// flight) and swaps the sets; `shadow`/`ao`/`present` are this
    /// frame's pass flags, kept for its own resolve next time.
    fn begin_frame(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        shadow: bool,
        ao: bool,
        present: bool,
    ) {
        use std::sync::atomic::Ordering;
        if let Some((shadow, ao, present)) = self.pending.take() {
            let previous = 1 - self.writing;
            let i = self.next;
            let slot = &mut self.slots[i];
            if slot.state.load(Ordering::Acquire) != 0 {
                self.untimed += 1;
            } else {
                encoder.resolve_query_set(
                    &self.query_sets[previous],
                    0..TIMESTAMP_QUERIES,
                    &self.resolve,
                    0,
                );
                encoder.copy_buffer_to_buffer(
                    &self.resolve,
                    0,
                    &slot.buffer,
                    0,
                    u64::from(TIMESTAMP_QUERIES) * 8,
                );
                slot.shadow = shadow;
                slot.ao = ao;
                slot.present = present;
                slot.state.store(1, Ordering::Release);
                self.next = (i + 1) % TIMESTAMP_SLOTS;
            }
        }
        self.writing = 1 - self.writing;
        self.pending = Some((shadow, ao, present));
    }

    /// Reads back what has arrived and asks for what has been submitted:
    /// the pass times of the most recently mapped slot, in microseconds
    /// (shadow, ambient occlusion, scene, post, whole frame).
    fn collect(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<(u32, u32, u32, u32, u32)> {
        use std::sync::atomic::Ordering;
        // Map callbacks fire from the poll.
        let _ = device.poll(wgpu::PollType::Poll);
        let period = queue.get_timestamp_period();
        let mut latest = None;
        // Oldest first, so the newest mapped slot wins.
        for k in 0..TIMESTAMP_SLOTS {
            let slot = &self.slots[(self.next + k) % TIMESTAMP_SLOTS];
            match slot.state.load(Ordering::Acquire) {
                3 => {
                    let ticks: Vec<u64> = match slot.buffer.slice(..).get_mapped_range() {
                        Ok(view) => bytemuck::cast_slice::<u8, u64>(&view).to_vec(),
                        Err(_) => Vec::new(),
                    };
                    slot.buffer.unmap();
                    slot.state.store(0, Ordering::Release);
                    if ticks.len() < TIMESTAMP_QUERIES as usize {
                        continue;
                    }
                    let us = |ticks: u64| -> u32 {
                        (ticks as f64 * f64::from(period) / 1000.0).round() as u32
                    };
                    let span = |b: usize, e: usize| us(ticks[e].saturating_sub(ticks[b]));
                    let shadow = if slot.shadow { span(0, 1) } else { 0 };
                    let ao_span = 2 * AO_PAIR as usize;
                    let ao = if slot.ao {
                        span(ao_span, ao_span + 1)
                    } else {
                        0
                    };
                    let scene = span(2, 3);
                    let post = span(4, 5) + if slot.present { span(6, 7) } else { 0 };
                    // A tile-based GPU starts a pass's vertex stage before
                    // the previous pass's fragment stage ends, so the
                    // passes overlap: the frame is the span from the first
                    // begin to the last end, not their sum.
                    let mut pairs = vec![1, 2];
                    if slot.shadow {
                        pairs.push(0);
                    }
                    if slot.ao {
                        pairs.push(AO_PAIR as usize);
                    }
                    if slot.present {
                        pairs.push(3);
                    }
                    let first = pairs.iter().map(|p| ticks[p * 2]).min().unwrap_or(0);
                    let last = pairs.iter().map(|p| ticks[p * 2 + 1]).max().unwrap_or(0);
                    latest = Some((shadow, ao, scene, post, us(last.saturating_sub(first))));
                }
                1 => {
                    let state = slot.state.clone();
                    slot.state.store(2, Ordering::Release);
                    slot.buffer
                        .slice(..)
                        .map_async(wgpu::MapMode::Read, move |r| {
                            // A failed map (device lost) frees the slot; the
                            // frame stays untimed.
                            state.store(if r.is_ok() { 3 } else { 0 }, Ordering::Release);
                        });
                }
                _ => {}
            }
        }
        latest
    }
}

/// The fixed pipeline set for one sample count (`docs/renderer.md` lists
/// every variant).
struct Pipelines {
    /// Opaque meshes, back faces culled.
    mesh: wgpu::RenderPipeline,
    /// Opaque meshes, double-sided (no culling, back faces flip the normal).
    mesh_double_sided: wgpu::RenderPipeline,
    /// `blend` meshes: alpha blended, depth read only.
    transparent: wgpu::RenderPipeline,
    transparent_double_sided: wgpu::RenderPipeline,
    /// The four above with the vertex skinned by its joint palette
    /// (`vs_skinned`; M3-19).
    mesh_skinned: wgpu::RenderPipeline,
    mesh_skinned_double_sided: wgpu::RenderPipeline,
    transparent_skinned: wgpu::RenderPipeline,
    transparent_skinned_double_sided: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    /// Depth only, from the sun.
    shadow: wgpu::RenderPipeline,
    shadow_skinned: wgpu::RenderPipeline,
    /// Depth only, from the camera, for the opaque instances the scene
    /// pass will draw: the ambient-occlusion stage's input (R2). The
    /// vertex stage and the culling are the mesh pass's, so the depths
    /// match exactly and its `LessEqual` compare keeps every fragment.
    depth: wgpu::RenderPipeline,
    depth_double_sided: wgpu::RenderPipeline,
    depth_skinned: wgpu::RenderPipeline,
    depth_skinned_double_sided: wgpu::RenderPipeline,
    /// The selection outline's mask (`outline.rs`): the mesh vertex
    /// stages, one sample, no depth, `fs_mask` into `R8Unorm` blended
    /// with `Max`. Culling as the scene pass's twins.
    mask: wgpu::RenderPipeline,
    mask_double_sided: wgpu::RenderPipeline,
    mask_skinned: wgpu::RenderPipeline,
    mask_skinned_double_sided: wgpu::RenderPipeline,
    /// Screen-space ambient occlusion and its blur.
    ao: wgpu::RenderPipeline,
    ao_blur: wgpu::RenderPipeline,
    /// Bloom (R3b): the bright-pass into the chain's first level, the
    /// downsample between levels, and the upsample blended back over the
    /// finer level by `scatter` (the blend constant).
    bloom_prefilter: wgpu::RenderPipeline,
    bloom_down: wgpu::RenderPipeline,
    bloom_up: wgpu::RenderPipeline,
    post: wgpu::RenderPipeline,
    /// LDR at render scale → the target.
    present: wgpu::RenderPipeline,
    /// The scene passes' samples per pixel.
    sample_count: u32,
    /// The target is a Unorm format: the post pass encodes sRGB itself.
    encode_srgb: bool,
}

/// Fallback maps and the default material bind group (need a queue, so
/// they exist from the first `prepare`).
struct Fallbacks {
    white: GpuTexture,
    flat_normal: GpuTexture,
    default_material: wgpu::BindGroup,
}

/// The frame's render targets at the internal size.
struct Attachments {
    width: u32,
    height: u32,
    sample_count: u32,
    /// Depth for the scene passes (multisampled with the colour). The
    /// depth prepass fills and stores it; the AO passes read it.
    depth_view: wgpu::TextureView,
    /// Ambient occlusion at this size (R2); `None` while the profile has
    /// it off, and then the pass chain has no AO stage at all.
    ao: Option<AoTargets>,
    /// Bloom's chain (R3b); `None` while the profile has it off, and then
    /// the post pass reads a black placeholder.
    bloom: Option<BloomTargets>,
    /// The multisampled HDR colour, resolved into `hdr_view`; `None` at
    /// one sample (the scene passes write `hdr_view` directly).
    hdr_msaa_view: Option<wgpu::TextureView>,
    /// The single-sample HDR the post pass reads.
    hdr_view: wgpu::TextureView,
    /// The tone-mapped frame at render scale, presented to the target;
    /// `None` at scale 1 (the post pass writes the target directly).
    ldr_view: Option<wgpu::TextureView>,
    post_bind_group: wgpu::BindGroup,
    present_bind_group: Option<wgpu::BindGroup>,
    /// Texture memory of the attachments (the GPU memory estimate).
    bytes: u64,
}

/// Ambient occlusion (R2) at the internal size: the SSAO estimate and the
/// blur that removes its per-pixel rotation, R8 each. Each pass reads the
/// target the other writes, so neither is ever bound and attached at once.
struct AoTargets {
    view: wgpu::TextureView,
    blur_view: wgpu::TextureView,
    /// Group 1 of the SSAO pass: the scene depth, and `blur_view` (which
    /// the pass does not read — the layout is shared with the blur).
    ao_bind_group: wgpu::BindGroup,
    /// Group 1 of the blur: the scene depth and `view`.
    blur_bind_group: wgpu::BindGroup,
}

/// Bloom's mip chain (R3b) at half the internal size: one view per level,
/// and the group 1 each pass reads — the HDR frame for the bright-pass,
/// the finer level for a downsample, the coarser one for an upsample.
struct BloomTargets {
    levels: Vec<wgpu::TextureView>,
    prefilter_bind_group: wgpu::BindGroup,
    /// `down[i]` reads level `i` (drawn into level `i + 1`).
    down: Vec<wgpu::BindGroup>,
    /// `up[i]` reads level `i + 1` (blended into level `i`).
    up: Vec<wgpu::BindGroup>,
}

/// The sun's shadow map: `resolution` 1 is the placeholder bound while the
/// profile has shadows off.
struct ShadowMap {
    view: wgpu::TextureView,
    resolution: u32,
}

impl ShadowMap {
    fn bytes(&self) -> u64 {
        u64::from(self.resolution) * u64::from(self.resolution) * 4
    }

    /// The side of one cascade's quadrant: the map holds the four of them
    /// in a 2×2 atlas, so the sun costs the same memory it always did (R1).
    fn cascade_side(&self) -> u32 {
        (self.resolution / 2).max(1)
    }

    /// Cascade `i`'s quadrant in atlas uv — origin then size — inset by one
    /// texel so a PCF tap at the edge cannot reach its neighbour.
    fn cascade_rect(&self, i: usize) -> [f32; 4] {
        let side = self.cascade_side() as f32;
        let atlas = self.resolution as f32;
        let (col, row) = ((i % 2) as f32, (i / 2) as f32);
        [
            (col * side + 1.0) / atlas,
            (row * side + 1.0) / atlas,
            (side - 2.0).max(1.0) / atlas,
            (side - 2.0).max(1.0) / atlas,
        ]
    }
}

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
    batch_index: HashMap<BatchKey, usize>,
    /// Reused by `assign_lights`: (illuminance at the bounds, pool index).
    light_scratch: Vec<(f32, u8)>,
    /// One light list per instance, in the instance buffer's order — the
    /// storage array the mesh stages index with `Instance.flags[2]`.
    light_lists: Vec<[u32; 4]>,
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
    /// Whether this frame's pass chain has the ambient-occlusion stage
    /// (R2): the profile asks for it and the targets exist.
    ao_active: bool,
    /// Whether this frame's pass chain has the bloom stage (R3b): the
    /// profile allows it, the chain exists and the volumes give it an
    /// intensity.
    bloom_active: bool,
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
    /// `Assets::evictions` when the GPU caches were last reconciled.
    evictions_seen: u64,
    live_edits_seen: u64,
    mesh_edits_seen: u64,
    meshes: HashMap<AssetRef, Option<GpuMesh>>,
    textures: HashMap<AssetRef, Option<GpuTexture>>,
    /// The cooked environments read for an HDR sky (E1; `None`: the
    /// texture has no `ENVM`, or it could not be read).
    environments: HashMap<AssetRef, Option<std::sync::Arc<EnvironmentPayload>>>,
    materials: HashMap<AssetRef, Option<GpuMaterial>>,
    /// Volume profiles (R3a; `None`: it could not be read).
    volume_profiles: HashMap<AssetRef, Option<VolumeProfile>>,
    asset_errors: Vec<String>,
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
                visibility: wgpu::ShaderStages::VERTEX,
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
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let skin_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel skin bgl"),
            entries: &[storage_entry(0), storage_entry(1)],
        });
        let joint_capacity = 64;
        let joint_buffer = create_joint_buffer(device, joint_capacity);
        let light_list_capacity = 256;
        let light_list_buffer = create_light_list_buffer(device, light_list_capacity);
        let joint_bind_group =
            create_joint_bind_group(device, &skin_layout, &joint_buffer, &light_list_buffer);
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
            batch_index: HashMap::new(),
            light_scratch: Vec::with_capacity(LIGHT_POOL),
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
            evictions_seen: 0,
            live_edits_seen: 0,
            mesh_edits_seen: 0,
            meshes: HashMap::new(),
            textures: HashMap::new(),
            environments: HashMap::new(),
            materials: HashMap::new(),
            volume_profiles: HashMap::new(),
            asset_errors: Vec::new(),
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
            frustum_culling: true,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
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
        }
        let internal = wanted.internal_size(self.width, self.height);
        if internal != (self.attachments.width, self.attachments.height)
            || wanted.msaa != self.attachments.sample_count
            || (wanted.render_scale != 1.0) != self.attachments.ldr_view.is_some()
            || wanted.ambient_occlusion != self.attachments.ao.is_some()
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

    /// Where meshes, textures and materials come from. Replacing the
    /// store drops every loaded asset (they are reloaded on demand);
    /// setting a clone of the current store is a no-op.
    pub fn set_assets(&mut self, assets: Option<Assets>) {
        if let (Some(a), Some(b)) = (&self.assets, &assets) {
            if a.same_store(b) {
                return;
            }
        }
        self.evictions_seen = assets.as_ref().map(Assets::evictions).unwrap_or(0);
        self.live_edits_seen = assets.as_ref().map(Assets::live_edits).unwrap_or(0);
        self.mesh_edits_seen = assets.as_ref().map(Assets::mesh_edits).unwrap_or(0);
        self.assets = assets;
        self.clear_assets();
    }

    pub fn assets(&self) -> Option<&Assets> {
        self.assets.as_ref()
    }

    /// Drops the GPU copies of assets the store evicted since the last
    /// frame (a scene unload released them); nothing to do otherwise.
    fn reconcile_evictions(&mut self) {
        let Some(assets) = &self.assets else { return };
        // A material or volume profile edited in place (the editor's
        // Inspector): every bind group is rebuilt and every profile re-read
        // from the store on its next use.
        let edits = assets.live_edits();
        if edits != self.live_edits_seen {
            self.live_edits_seen = edits;
            self.materials.clear();
            self.volume_profiles.clear();
        }
        // A mesh the game made, changed or removed (`world.createMesh`):
        // those GPU copies go, and the next use uploads what the store has.
        let mesh_edits = assets.mesh_edits();
        if mesh_edits != self.mesh_edits_seen {
            for r in assets.meshes_changed_since(self.mesh_edits_seen) {
                self.meshes.remove(&r);
            }
            self.mesh_edits_seen = mesh_edits;
        }
        let now = assets.evictions();
        if now == self.evictions_seen {
            return;
        }
        self.evictions_seen = now;
        let a = assets.clone();
        self.meshes
            .retain(|r, _| a.is_resident(r.asset) || a.is_runtime_mesh(r));
        self.textures.retain(|r, _| a.is_resident(r.asset));
        self.environments.retain(|r, _| a.is_resident(r.asset));
        self.volume_profiles.retain(|r, _| a.is_resident(r.asset));
        let textures = &self.textures;
        self.materials.retain(|r, m| {
            a.is_resident(r.asset)
                && m.as_ref()
                    .is_none_or(|m| m.textures.iter().all(|t| textures.contains_key(t)))
        });
        if let Some((r, _)) = &self.sky_bind_group {
            if !self.textures.contains_key(r) {
                self.sky_bind_group = None;
            }
        }
        if let Some((r, _)) = &self.ibl {
            if !a.is_resident(r.asset) {
                self.ibl = None;
                self.ibl_dirty = true;
            }
        }
        self.asset_errors.clear();
    }

    /// Forgets every loaded asset and error; the next `prepare` reloads
    /// what the extract references.
    pub fn clear_assets(&mut self) {
        self.meshes.clear();
        self.textures.clear();
        self.environments.clear();
        self.materials.clear();
        self.volume_profiles.clear();
        self.sky_bind_group = None;
        self.ibl_dirty |= self.ibl.take().is_some();
        self.asset_errors.clear();
    }

    /// One line per asset that failed to load since `clear_assets`
    /// (each asset is tried once).
    pub fn asset_errors(&self) -> &[String] {
        &self.asset_errors
    }

    // ----------------------------------------------------------- prepare

    /// Applies a pending quality change, then uploads the camera, the
    /// lights, the sun's shadow fit, the environment, the extract's
    /// instances (culled against the camera frustum and the sun's box,
    /// grouped into one range per opaque `(mesh, material)` batch, then
    /// the transparent ones back-to-front) — the queued [`ExtraWorld`]s'
    /// with them, which it consumes — and the debug lines. Returns the
    /// number of instances the scene passes will draw.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extract: &Extract,
    ) -> u32 {
        let prepare_start = std::time::Instant::now();
        // Consumed by this frame; handed back empty at the end to keep
        // the queue's capacity.
        let mut extras = std::mem::take(&mut self.extra_worlds);
        self.reconcile_evictions();
        self.resolve_quality(device);
        self.ensure_fallbacks(device, queue);
        if let Some(timer) = self.gpu_timer.as_mut() {
            if let Some(times) = timer.collect(device, queue) {
                self.gpu_times = times;
                self.gpu_timed += 1;
            }
        }
        let mut stats = FrameStats {
            msaa: self.quality.msaa,
            render_width: self.attachments.width,
            render_height: self.attachments.height,
            encode_us: self.stats.encode_us,
            gpu_timestamps: self.gpu_timer.is_some(),
            gpu_shadow_us: self.gpu_times.0,
            gpu_ao_us: self.gpu_times.1,
            gpu_scene_us: self.gpu_times.2,
            gpu_post_us: self.gpu_times.3,
            gpu_frame_us: self.gpu_times.4,
            gpu_untimed: self.gpu_timer.as_ref().map_or(0, |t| t.untimed),
            gpu_timed: self.gpu_timed,
            ..FrameStats::default()
        };

        // Camera.
        let aspect = self.width as f32 / self.height as f32;
        let (view_proj, camera_pos, component, shape) = match &extract.camera {
            Some(c) => {
                let (_, _, translation) = c.world.to_scale_rotation_translation();
                let view = fynkel_core::view_matrix(&c.world);
                let proj = fynkel_core::projection_matrix(&c.camera, aspect);
                let shape = match c.camera.projection {
                    Projection::Perspective => shadow::Shape::Perspective {
                        fov_y: c.camera.fov_y,
                    },
                    Projection::Orthographic => shadow::Shape::Orthographic {
                        height: c.camera.ortho_height,
                    },
                };
                stats.scene_camera = true;
                (
                    proj * view,
                    translation,
                    Some(&c.camera),
                    shadow::CameraShape {
                        view,
                        near: c.camera.near,
                        far: c.camera.far,
                        aspect,
                        shape,
                    },
                )
            }
            None => (
                self.camera.view_proj(self.width, self.height),
                self.camera.eye,
                None,
                shadow::CameraShape {
                    view: self.camera.view(),
                    near: self.camera.near,
                    far: self.camera.far,
                    aspect,
                    shape: shadow::Shape::Perspective {
                        fov_y: self.camera.fov_y_radians,
                    },
                },
            ),
        };

        // Sun, ambient, sky. A scene that authors no lighting at all (no
        // light, no Environment) gets the host's fallback sun.
        // An extra world's lights count as authored, and its sun is the
        // frame's when the main extract has none.
        let authored_lighting = !extract.directional_lights.is_empty()
            || !extract.point_lights.is_empty()
            || !extract.spot_lights.is_empty()
            || extract.environment.is_some()
            || extras.iter().any(|x| {
                !x.extract.directional_lights.is_empty()
                    || !x.extract.point_lights.is_empty()
                    || !x.extract.spot_lights.is_empty()
            });
        let sun = match extract.directional_lights.first() {
            Some(l) => Some((l, l.direction)),
            None => extras.iter().find_map(|x| {
                x.extract
                    .directional_lights
                    .first()
                    .map(|l| (l, x.transform.transform_vector3(l.direction)))
            }),
        };
        let (sun_dir, sun_color, sun_lux, sun_present, sun_casts) = match sun {
            Some((l, direction)) => {
                stats.scene_light = true;
                let lux = l.light.intensity.max(0.0);
                (
                    (-direction).normalize_or(Vec3::Y),
                    l.light.color.max(Vec3::ZERO) * lux,
                    lux,
                    true,
                    l.light.cast_shadows,
                )
            }
            None if !authored_lighting => (
                self.light_dir,
                Vec3::splat(exposure::FALLBACK_SUN_LUX),
                exposure::FALLBACK_SUN_LUX,
                true,
                true,
            ),
            None => (Vec3::Y, Vec3::ZERO, 0.0, false, false),
        };
        let default_env = Environment::default();
        let env = extract
            .environment
            .as_ref()
            .map_or(&default_env, |e| &e.environment);
        let ambient_lux = env.ambient_intensity.max(0.0);
        let ambient = env.ambient_color.max(Vec3::ZERO) * ambient_lux;
        let sky = env
            .sky
            .as_ref()
            .filter(|r| self.texture(device, queue, r))
            .cloned();
        if let Some(r) = &sky {
            if self
                .sky_bind_group
                .as_ref()
                .is_none_or(|(have, _)| have != r)
            {
                let view = &self.textures[r].as_ref().expect("loaded above").view;
                let bg = create_single_bind_group(
                    device,
                    &self.single_layout,
                    &self.sampler,
                    view,
                    &self.post_buffer,
                    "fynkel sky bg",
                );
                self.sky_bind_group = Some((r.clone(), bg));
            }
        } else {
            self.sky_bind_group = None;
        }
        stats.sky = sky.is_some();

        // Image-based lighting (E1): the environment prefiltered next to
        // an HDR sky. An 8-bit sky has no `ENVM` and lights nothing — the
        // constant ambient stays.
        let want_ibl = sky.as_ref().filter(|r| self.environment(r));
        if self.ibl.as_ref().map(|(r, _)| r) != want_ibl {
            self.ibl = want_ibl.cloned().map(|r| {
                let env = self.environments[&r].clone().expect("loaded above");
                let ibl = Ibl::upload(device, queue, &env, &r);
                (r, ibl)
            });
            self.ibl_dirty = true;
        }
        if self.ibl_dirty {
            self.rebuild_frame_bind_group(device);
            self.ibl_dirty = false;
        }
        let ibl_mips = self.ibl.as_ref().map_or(1, |(_, i)| i.specular_mips);
        // Metering: an IBL scene's ambient illuminance is the
        // environment's mean irradiance at the sky's intensity.
        let ibl_lux = self.ibl.as_ref().map_or(0.0, |(r, _)| {
            let e = self.environments[r].as_ref().expect("bound above");
            let mean = (e.mean_irradiance[0] + e.mean_irradiance[1] + e.mean_irradiance[2]) / 3.0;
            mean * env.sky_intensity.max(0.0)
        });
        stats.ibl = self.ibl.is_some();
        let ev100 = exposure::ev100_for(component, sun_lux, ambient_lux.max(ibl_lux));
        self.ev100 = ev100;
        let white = exposure::white_nits(ev100);

        // Point and spot lights: every one of them, culled to the view
        // and assigned per object below.
        // Each with its `culling_mask` (ADR-0020): the layers it lights.
        // An extra world's are placed by its transform first.
        let light_sources = std::iter::once((extract, None))
            .chain(extras.iter().map(|x| (&x.extract, Some(&x.transform))));
        let mut lights: Vec<(f32, GpuLight, u32)> = Vec::with_capacity(
            light_sources
                .clone()
                .map(|(e, _)| e.point_lights.len() + e.spot_lights.len())
                .sum(),
        );
        for (source, transform) in light_sources {
            for l in &source.point_lights {
                let (position, _, range) =
                    place_light(transform, l.position, Vec3::NEG_Z, l.light.range);
                lights.push((
                    position.distance_squared(camera_pos),
                    GpuLight {
                        position: position.extend(range.max(1e-3)).to_array(),
                        color: (l.light.color.max(Vec3::ZERO) * l.light.intensity.max(0.0))
                            .extend(LIGHT_POINT)
                            .to_array(),
                        ..GpuLight::default()
                    },
                    l.light.culling_mask,
                ));
            }
            for l in &source.spot_lights {
                let (position, direction, range) =
                    place_light(transform, l.position, l.direction, l.light.range);
                let outer = l.light.outer_angle.clamp(1e-3, std::f32::consts::PI - 1e-3);
                let inner = l.light.inner_angle.clamp(0.0, outer);
                let cos_outer = outer.cos();
                // `smoothstep` needs distinct edges.
                let cos_inner = inner.cos().max(cos_outer + 1e-4);
                lights.push((
                    position.distance_squared(camera_pos),
                    GpuLight {
                        position: position.extend(range.max(1e-3)).to_array(),
                        color: (l.light.color.max(Vec3::ZERO) * l.light.intensity.max(0.0))
                            .extend(LIGHT_SPOT)
                            .to_array(),
                        direction: direction
                            .normalize_or(Vec3::NEG_Z)
                            .extend(cos_outer)
                            .to_array(),
                        params: [cos_inner, 0.0, 0.0, 0.0],
                    },
                    l.light.culling_mask,
                ));
            }
        }
        // The sun's shadow fit depends only on the camera, so it comes
        // before the instances: casters are culled against its box.
        let fit = (self.quality.shadows && sun_present && sun_casts)
            .then(|| shadow::fit(sun_dir, &shape.shadow_corners(), self.shadow_map.resolution));
        let camera_frustum = Frustum::from_view_proj(&view_proj);
        let light_frustum = fit.as_ref().map(|f| Frustum::from_view_proj(&f.view_proj));

        // The frame's light pool (E1). A light whose range sphere misses
        // the camera frustum reaches nothing that is shaded, so it never
        // enters the pool — exact, not an approximation, and it is why
        // `frustum_culling` does not gate this. The nearest to the camera
        // fill the pool when more than `LIGHT_POOL` still reach the view.
        lights.retain(|(_, l, _)| {
            camera_frustum.intersects(&Sphere {
                center: Vec3::new(l.position[0], l.position[1], l.position[2]),
                radius: l.position[3],
            })
        });
        lights.sort_by(|a, b| a.0.total_cmp(&b.0));
        let n_lights = lights.len().min(LIGHT_POOL);
        let mut packed = [GpuLight::default(); LIGHT_POOL];
        let mut light_masks = [u32::MAX; LIGHT_POOL];
        for (i, (_, l, mask)) in lights.iter().take(LIGHT_POOL).enumerate() {
            packed[i] = *l;
            light_masks[i] = *mask;
        }
        // The layers the frame's camera draws; the host's own camera (no
        // scene camera) and the editor's draw every layer.
        let camera_mask = extract
            .camera
            .as_ref()
            .map_or(u32::MAX, |c| c.camera.culling_mask);
        stats.lights = n_lights as u32;
        stats.lights_dropped = (lights.len() - n_lights) as u32;

        // Instances: opaque and mask by (mesh, material) in order of
        // first appearance; blend on their own, sorted back-to-front.
        // Each instance is tagged with where it is drawn — see the batch
        // layout below.
        let mut grouped: Vec<(bool, Vec<Grouped>)> = Vec::new();
        let mut blended: Vec<(f32, BatchKey, bool, Instance, [u32; 4])> = Vec::new();
        self.batch_index.clear();
        let mut mesh_of_record: HashMap<u32, usize> = HashMap::with_capacity(extract.meshes.len());
        let cube_key = || BatchKey {
            mesh: None,
            material: None,
            skinned: false,
        };
        let cube_bounds = self.cube.bounds;
        let mut caster_bounds: Option<Sphere> = None;
        // The selection outline (ADR-0019, `outline.rs`): the mask value
        // of each outlined entity — a child 0.5, selected 1, selected
        // winning when an entity is both.
        let outline = self.overlay.take_outline();
        let mut outline_marks: HashMap<fynkel_contracts::Entity, f32> = HashMap::new();
        if let Some(o) = &outline {
            for e in &o.children {
                outline_marks.insert(*e, 0.5);
            }
            for e in &o.selected {
                outline_marks.insert(*e, 1.0);
            }
        }
        self.outline_draws.clear();
        self.outline_instances.clear();
        // The main extract, then each extra world: its records placed by
        // its transform, its palettes after those before it, its
        // instances marked grey when it is.
        let sources = std::iter::once((extract, None, false)).chain(
            extras
                .iter()
                .map(|x| (&x.extract, Some(x.transform), x.gray)),
        );
        let mut joint_base = 0u32;
        for (si, (source, transform, gray)) in sources.enumerate() {
            let main = si == 0;
            mesh_of_record.clear();
            for (i, m) in source.meshes.iter().enumerate() {
                mesh_of_record.insert(m.record, i);
            }
            for (ri, r) in source.records.iter().enumerate() {
                let world = match transform {
                    Some(t) => t * r.world,
                    None => r.world,
                };
                if r.layer_bit & camera_mask == 0 {
                    // Not this camera's layers: no draw, no shadow, no light list.
                    stats.layer_hidden += 1;
                    continue;
                }
                let (key, double_sided, mut instance, casts, blend, bounds) = match mesh_of_record
                    .get(&(ri as u32))
                {
                    Some(&mi) => {
                        let m = &source.meshes[mi];
                        let have_mesh = self.mesh(device, &m.mesh);
                        let (material_key, material) = self.material(device, queue, &m.material);
                        if have_mesh {
                            stats.meshes += 1;
                            let blend = material.alpha_mode == AlphaMode::Blend;
                            let gpu = self.meshes.get(&m.mesh).and_then(Option::as_ref);
                            let bounds = gpu.map_or(cube_bounds, |g| g.bounds);
                            // Skinned when the mesh carries weights and
                            // the extract a palette for this instance; a
                            // skinned mesh without a `Skin` draws as
                            // cooked (its bind pose).
                            let palette = m
                                .skin
                                .filter(|_| gpu.is_some_and(|g| g.skin_buffer.is_some()));
                            let mut instance = instance_for(&material, world, m.receive_shadows);
                            if let Some(p) = palette {
                                instance.emissive[3] = (joint_base + p.first_joint) as f32;
                            }
                            (
                                BatchKey {
                                    mesh: Some(m.mesh.clone()),
                                    material: material_key,
                                    skinned: palette.is_some(),
                                },
                                material.double_sided,
                                instance,
                                m.cast_shadows && !blend,
                                blend,
                                bounds,
                            )
                        } else {
                            stats.missing += 1;
                            (
                                cube_key(),
                                false,
                                Instance {
                                    model: world.to_cols_array_2d(),
                                    tint: MISSING_TINT,
                                    emissive: [0.0; 4],
                                    params: [0.0, 1.0, 1.0, 1.0],
                                    flags: [NO_CUTOFF, 1.0, 0.0, 0.0],
                                },
                                m.cast_shadows,
                                false,
                                cube_bounds,
                            )
                        }
                    }
                    None => {
                        if !self.placeholders {
                            continue;
                        }
                        stats.placeholders += 1;
                        (
                            cube_key(),
                            false,
                            Instance {
                                model: world.to_cols_array_2d(),
                                tint: tint_for(r.entity.slot()),
                                emissive: [0.0; 4],
                                params: [0.0, 1.0, 1.0, 1.0],
                                flags: [NO_CUTOFF, 0.0, 0.0, 1.0],
                            },
                            true,
                            false,
                            cube_bounds,
                        )
                    }
                };
                if gray {
                    instance.flags[3] += GRAY_FLAG;
                }
                // Culling: the mesh's bounding sphere in world space against
                // the camera frustum and, for casters, the sun's box. A
                // caster outside the view still shadows what is in it.
                let sphere = bounds.transformed(&world);
                // Whether this caster reaches the shadowed range at all,
                // tested against the loose fit whether or not culling is on:
                // what casts decides how tight the cascades are (R1), and
                // culling must not change the picture.
                let reaches_shadow = casts
                    && light_frustum
                        .as_ref()
                        .is_some_and(|f| f.intersects(&sphere));
                if reaches_shadow {
                    // A subset of the loose fit, so the cascades can only
                    // shrink against it, never grow.
                    caster_bounds = Some(match caster_bounds {
                        Some(u) => u.union(&sphere),
                        None => sphere,
                    });
                }
                let (in_view, casts) = if self.frustum_culling {
                    (camera_frustum.intersects(&sphere), reaches_shadow)
                } else {
                    (true, casts && fit.is_some())
                };
                // Outlined: a copy of the instance for the mask, whatever
                // pass draws the original (a blended one included). What is
                // out of view has no silhouette on screen to ring.
                if in_view && main {
                    if let Some(&mark) = outline_marks.get(&r.entity) {
                        let mut copy = instance;
                        copy.params[0] = mark;
                        // Any valid light list: the mask shades nothing.
                        copy.flags[2] = 0.0;
                        self.outline_draws.push(OutlineDraw {
                            key: key.clone(),
                            double_sided,
                            index: self.outline_instances.len() as u32,
                        });
                        self.outline_instances.push(copy);
                    }
                }
                if !in_view {
                    stats.culled += 1;
                    if !casts {
                        continue;
                    }
                }
                // The object's light list (E1): the lights of the pool that
                // reach its bounds, the strongest first. Only what is shaded
                // needs one — a shadow-only instance is never lit, and an
                // unlit material ignores every light.
                let mut light_list = [0u32; 4];
                if in_view && n_lights > 0 && instance.flags[1] < 0.5 {
                    let (list, count, overflow) = assign_lights(
                        &packed[..n_lights],
                        &light_masks[..n_lights],
                        r.layer_bit,
                        &sphere,
                        &mut self.light_scratch,
                    );
                    light_list = list;
                    stats.lights_max_per_object = stats.lights_max_per_object.max(count as u32);
                    stats.lights_overflow += overflow as u32;
                }
                if blend {
                    let origin = world.w_axis.truncate();
                    blended.push((
                        origin.distance_squared(camera_pos),
                        key,
                        double_sided,
                        instance,
                        light_list,
                    ));
                    continue;
                }
                let slot = match self.batch_index.get(&key) {
                    Some(&s) => s,
                    None => {
                        grouped.push((double_sided, Vec::new()));
                        self.batch_index.insert(key, grouped.len() - 1);
                        grouped.len() - 1
                    }
                };
                grouped[slot].1.push((
                    instance,
                    match (casts, in_view) {
                        (true, false) => Visibility::ShadowOnly,
                        (true, true) => Visibility::Both,
                        (false, true) => Visibility::SceneOnly,
                        (false, false) => unreachable!("skipped above"),
                    },
                    light_list,
                ));
            }
            joint_base += source.joints.len() as u32;
        }

        // Batch layout: [shadow only][both][scene only], so the shadow
        // draw is the first two and the scene draw the last two — one
        // contiguous range each, one upload.
        // Either storage buffer of group 2 outgrowing its capacity means
        // one new bind group for both.
        let mut rebind_group_2 = false;
        self.instances.clear();
        self.light_lists.clear();
        self.batches.clear();
        self.transparent.clear();
        let mut keys: Vec<(BatchKey, usize)> = self.batch_index.drain().collect();
        keys.sort_by_key(|(_, slot)| *slot);
        let mut casters_total = 0;
        for (key, slot) in keys {
            let (double_sided, list) = &mut grouped[slot];
            // Stable: instances keep their order within a category.
            list.sort_by_key(|(_, v, _)| *v as u8);
            let count_of = |v: Visibility| list.iter().filter(|(_, x, _)| *x == v).count() as u32;
            let (shadow_only, both) =
                (count_of(Visibility::ShadowOnly), count_of(Visibility::Both));
            let start = self.instances.len() as u32;
            let shadow_count = shadow_only + both;
            let count = list.len() as u32 - shadow_only;
            casters_total += shadow_count;
            if count > 0 {
                stats.draws += 1;
            }
            if shadow_count > 0 {
                stats.shadow_draws += 1;
            }
            stats.instances += count;
            self.batches.push(Batch {
                key,
                double_sided: *double_sided,
                first: start + shadow_only,
                count,
                shadow_first: start,
                shadow_count,
            });
            // Every instance says where its light list is: the two live
            // in the same slot of their buffers.
            for (instance, _, lights) in list.iter() {
                let mut instance = *instance;
                instance.flags[2] = self.instances.len() as f32;
                self.instances.push(instance);
                self.light_lists.push(*lights);
            }
        }
        // Farthest first.
        blended.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, key, double_sided, mut instance, lights) in blended {
            self.transparent.push(TransparentDraw {
                key,
                double_sided,
                index: self.instances.len() as u32,
            });
            instance.flags[2] = self.instances.len() as f32;
            self.instances.push(instance);
            self.light_lists.push(lights);
        }
        let n = self.instances.len() as u32;
        if n > self.instance_capacity {
            self.instance_capacity = n.next_power_of_two().max(self.instance_capacity * 2);
            self.instance_buffer = create_instance_buffer(device, self.instance_capacity);
        }
        // The light lists sit in the same slots as the instances, so the
        // two buffers grow together.
        if n > self.light_list_capacity {
            self.light_list_capacity = n.next_power_of_two().max(self.light_list_capacity * 2);
            self.light_list_buffer = create_light_list_buffer(device, self.light_list_capacity);
            rebind_group_2 = true;
        }
        if n > 0 {
            queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&self.instances),
            );
            queue.write_buffer(
                &self.light_list_buffer,
                0,
                bytemuck::cast_slice(&self.light_lists),
            );
        }
        stats.batches = self.batches.len() as u32;
        stats.transparent = self.transparent.len() as u32;
        stats.instances += stats.transparent;
        stats.draws += stats.transparent;
        stats.skinned = self
            .batches
            .iter()
            .filter(|b| b.key.skinned)
            .map(|b| b.count)
            .sum::<u32>()
            + self.transparent.iter().filter(|t| t.key.skinned).count() as u32;

        // The joint palettes, as the extract laid them out (an instance's
        // `emissive.w` is its first joint). The buffer never shrinks and
        // is never empty: the skinned stages bind it unconditionally.
        // With extra worlds, their palettes follow the main extract's
        // (`joint_base` above).
        let n_joints = joint_base;
        if n_joints > self.joint_capacity {
            self.joint_capacity = n_joints.next_power_of_two().max(self.joint_capacity * 2);
            self.joint_buffer = create_joint_buffer(device, self.joint_capacity);
            rebind_group_2 = true;
        }
        if rebind_group_2 {
            self.joint_bind_group = create_joint_bind_group(
                device,
                &self.skin_layout,
                &self.joint_buffer,
                &self.light_list_buffer,
            );
        }
        if stats.skinned > 0 && n_joints > 0 {
            if extras.is_empty() {
                queue.write_buffer(&self.joint_buffer, 0, bytemuck::cast_slice(&extract.joints));
            } else {
                self.joint_scratch.clear();
                self.joint_scratch.extend_from_slice(&extract.joints);
                for x in &extras {
                    self.joint_scratch.extend_from_slice(&x.extract.joints);
                }
                queue.write_buffer(
                    &self.joint_buffer,
                    0,
                    bytemuck::cast_slice(&self.joint_scratch),
                );
            }
            stats.joints = n_joints;
        }

        // The outline's instances, in their own buffer. The draws are
        // grouped by pipeline (static before skinned, culled before
        // double-sided) so the mask pass switches at most three times;
        // each keeps its slot. Every outlined instance is in view, so a
        // skinned one's palette went up above with the scene's.
        let n_outlined = self.outline_draws.len() as u32;
        if n_outlined > 0 {
            self.outline_draws
                .sort_by_key(|d| (d.key.skinned, d.double_sided));
            if n_outlined > self.outline_capacity {
                self.outline_capacity = n_outlined.next_power_of_two();
                self.outline_buffer = create_instance_buffer(device, self.outline_capacity);
            }
            queue.write_buffer(
                &self.outline_buffer,
                0,
                bytemuck::cast_slice(&self.outline_instances),
            );
        }
        self.outline_pass.prepare(
            device,
            queue,
            outline.map(|o| o.style).filter(|_| n_outlined > 0),
            (self.width, self.height),
            self.pipelines.encode_srgb,
        );
        stats.outlined = if self.outline_pass.active() {
            n_outlined
        } else {
            0
        };

        // The sun's shadow: drawn when the profile draws shadows, the sun
        // casts and something is there to cast.
        self.shadow_active = casters_total > 0;
        let mut shadow = [0.0f32; 4];
        let mut cascades = [GpuCascade::default(); shadow::CASCADES];
        if fit.is_some() && self.shadow_active {
            // The cascades are fitted now, not before the loop: the
            // casters' bounds are what tightens them, and they are only
            // known once every record has been through (R1).
            let fitted = shadow::fit_cascades(
                sun_dir,
                &shape,
                self.shadow_map.cascade_side(),
                caster_bounds,
            );
            shadow = [
                1.0,
                1.0 / self.shadow_map.resolution as f32,
                shadow::CASCADE_BLEND,
                shadow::CASCADES as f32,
            ];
            for (i, f) in fitted.fits.iter().enumerate() {
                cascades[i] = GpuCascade {
                    view_proj: f.view_proj.to_cols_array_2d(),
                    rect: self.shadow_map.cascade_rect(i),
                    params: [f.texel_world * SHADOW_NORMAL_OFFSET_TEXELS, 0.0, 0.0, 0.0],
                };
                queue.write_buffer(
                    &self.shadow_uniform_buffer,
                    i as u64 * SHADOW_UNIFORM_STRIDE,
                    bytemuck::bytes_of(&ShadowUniforms {
                        view_proj: f.view_proj.to_cols_array_2d(),
                    }),
                );
            }
            stats.shadow_casters = casters_total;
            stats.shadow_map = self.shadow_map.resolution;
            stats.shadow_cascades = shadow::CASCADES as u32;
            // Every batch with casters is drawn once per cascade.
            stats.shadow_draws *= shadow::CASCADES as u32;
        } else {
            self.shadow_active = false;
            stats.shadow_draws = 0;
        }

        // Volumes (R3a): the frame's look, blended at the camera.
        let (post, volumes) = self.resolve_volumes(extract, camera_pos);
        self.post = post;
        stats.volumes = volumes;

        // Ambient occlusion (R2): the stage runs when the quality profile
        // asks for it (a volume cannot turn it on), the volumes leave it
        // an intensity and there is opaque geometry for the prepass.
        self.ao_active = self.attachments.ao.is_some()
            && post.ao_intensity > 0.0
            && stats.draws > stats.transparent;
        stats.ambient_occlusion = self.ao_active;
        // Bloom (R3b): the profile allows it and a volume gives it an
        // intensity; otherwise the post pass adds nothing, bit for bit.
        self.bloom_active = self.attachments.bloom.is_some() && post.bloom_intensity > 0.0;
        stats.bloom = self.bloom_active;

        let u = FrameUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            cascades,
            camera_pos: camera_pos
                .extend(exposure::exposure_factor(ev100))
                .to_array(),
            sun_dir: sun_dir.extend(sun_present as u32 as f32).to_array(),
            sun_color: sun_color.extend(0.0).to_array(),
            ambient: ambient.extend(white).to_array(),
            sky: [
                env.sky_intensity.max(0.0),
                env.sky_rotation,
                stats.ibl as u32 as f32,
                (ibl_mips - 1) as f32,
            ],
            shadow,
            ao: [post.ao_radius, post.ao_intensity, 0.0, 0.0],
            bloom: [
                post.bloom_threshold.max(0.0),
                post.bloom_threshold.max(0.0) * 0.5,
                // Unity's mapping: neither end leaves a level out.
                0.05 + 0.9 * post.bloom_scatter.clamp(0.0, 1.0),
                if self.bloom_active {
                    post.bloom_intensity
                } else {
                    0.0
                },
            ],
            bloom_tint: post.bloom_tint.max(Vec3::ZERO).extend(0.0).to_array(),
            counts: [
                n_lights as u32,
                stats.sky as u32,
                self.pipelines.encode_srgb as u32,
                self.ao_active as u32,
            ],
            lights: packed,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&u));

        // The overlay (ADR-0019): its queue and the debug lines, uploaded
        // and cleared for the next frame.
        self.overlay_pass.prepare(
            device,
            queue,
            &mut self.overlay,
            &mut self.debug,
            overlay::OverlayFrame {
                view_proj,
                camera_pos,
                target: (self.width, self.height),
                depth_view: &self.attachments.depth_view,
                depth: (self.attachments.width, self.attachments.height),
                encode_srgb: self.pipelines.encode_srgb,
            },
        );
        let counts = self.overlay_pass.counts();
        stats.debug_lines = counts.lines;
        stats.overlay_triangles = counts.triangles;
        stats.grid = counts.grid;

        stats.extra_worlds = extras.len() as u32;
        extras.clear();
        self.extra_worlds = extras;
        stats.gpu_bytes = self.gpu_bytes();
        stats.prepare_us = prepare_start.elapsed().as_micros() as u32;
        self.stats = stats;
        stats.instances
    }

    /// The GPU memory estimate (`FrameStats::gpu_bytes`): what this
    /// renderer created and still holds. Driver-side copies, pipeline
    /// state and the swapchain are not counted.
    fn gpu_bytes(&self) -> u64 {
        let meshes: u64 = self
            .meshes
            .values()
            .flatten()
            .map(|m| {
                m.vertex_buffer.size()
                    + m.index_buffer.size()
                    + m.skin_buffer.as_ref().map_or(0, wgpu::Buffer::size)
            })
            .sum();
        let textures: u64 = self.textures.values().flatten().map(|t| t.bytes).sum();
        meshes
            + textures
            + self.ibl.as_ref().map_or(0, |(_, i)| i.bytes())
            + self.ibl_none.bytes()
            + self.brdf_lut.bytes
            + self.cube.vertex_buffer.size()
            + self.cube.index_buffer.size()
            + self.attachments.bytes
            + self.shadow_map.bytes()
            + self.instance_buffer.size()
            + self.joint_buffer.size()
            + self.overlay_pass.bytes()
            + self.outline_pass.bytes()
            + self.outline_buffer.size()
            + self.uniform_buffer.size()
            + self.shadow_uniform_buffer.size()
            + self.post_buffer.size()
    }

    // -------------------------------------------------------------- draw

    /// Records the frame's pass chain into `encoder`:
    ///
    /// | Pass | Reads | Writes |
    /// | --- | --- | --- |
    /// | shadow | caster instances, the sun fit | the shadow map (depth) |
    /// | depth prepass | opaque/mask instances | the scene depth |
    /// | ambient occlusion | the scene depth | the AO target |
    /// | AO blur | the AO target, the scene depth | the AO the mesh pass samples |
    /// | scene: opaque | opaque/mask instances, materials, lights, the shadow map | HDR colour (× samples) + depth |
    /// | scene: sky | the environment texture | HDR colour where depth is untouched |
    /// | scene: transparent | blend instances back-to-front, debug lines; depth (read) | HDR colour |
    /// | resolve | the multisampled HDR | the single-sample HDR (skipped at 1 sample) |
    /// | post | the HDR, exposure | the target — or the LDR at render scale |
    /// | presentation | the LDR | the target (skipped at scale 1) |
    ///
    /// The three scene stages are one GPU render pass (the attachments
    /// stay on-tile), resolved at its end.
    pub fn draw(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let encode_start = std::time::Instant::now();
        let present =
            self.attachments.ldr_view.is_some() && self.attachments.present_bind_group.is_some();
        let shadow = self.shadow_active;
        let ao = self.ao_active;
        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.begin_frame(encoder, shadow, ao, present);
        }
        if self.shadow_active {
            self.shadow_pass(encoder);
        }
        if ao {
            self.depth_prepass(encoder);
            self.ao_passes(encoder);
        }
        self.scene_pass(encoder);
        // The post pair spans the bloom stage when it runs: its bright-pass
        // writes the beginning, the post pass the end.
        let bloom = self.bloom_active;
        if bloom {
            self.bloom_passes(encoder);
        }
        let a = &self.attachments;
        let post_target = a.ldr_view.as_ref().unwrap_or(target);
        self.fullscreen_pass(
            encoder,
            "fynkel post",
            post_target,
            &self.pipelines.post,
            &a.post_bind_group,
            self.timestamps_part(2, !bloom, true),
        );
        if let (Some(_), Some(present)) = (&a.ldr_view, &a.present_bind_group) {
            self.fullscreen_pass(
                encoder,
                "fynkel present",
                target,
                &self.pipelines.present,
                present,
                self.timestamps(3),
            );
        }
        // The selection outline, then gizmos and handles, over the
        // finished frame at the target's resolution (ADR-0019; not timed):
        // the handles stay on top of the outline.
        if self.outline_pass.active() {
            self.outline_mask_pass(encoder);
            self.outline_pass.composite(encoder, target);
        }
        self.overlay_pass.draw(encoder, target);
        self.stats.encode_us = encode_start.elapsed().as_micros() as u32;
    }

    /// The pass's timestamp writes (`GpuTimer`), when the device samples
    /// them.
    fn timestamps(&self, pair: u32) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.gpu_timer.as_ref().and_then(|t| t.writes(pair))
    }

    /// One end of a query pair: the AO stage's three passes share pair
    /// `AO_PAIR`, so the whole stage reads as one span.
    fn timestamps_part(
        &self,
        pair: u32,
        begin: bool,
        end: bool,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.gpu_timer
            .as_ref()
            .and_then(|t| t.writes_part(pair, begin, end))
    }

    /// Depth only, from the sun, every caster inside its box (each
    /// batch's shadow range).
    fn shadow_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel shadow"),
            timestamp_writes: self.timestamps(0),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadow_map.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        // One quadrant of the atlas per cascade: the viewport moves, the
        // dynamic offset picks that cascade's matrix, and the same casters
        // are drawn into each — what a cascade does not cover its
        // projection clips away (R1). The viewport is the quadrant inset
        // by a texel, matching `ShadowMap::cascade_rect`, so the border
        // stays at the cleared far depth and a PCF tap at the edge of one
        // cascade reads "lit" instead of its neighbour.
        let side = self.shadow_map.cascade_side() as f32;
        for cascade in 0..shadow::CASCADES {
            let (col, row) = ((cascade % 2) as f32, (cascade / 2) as f32);
            pass.set_viewport(
                col * side + 1.0,
                row * side + 1.0,
                (side - 2.0).max(1.0),
                (side - 2.0).max(1.0),
                0.0,
                1.0,
            );
            pass.set_bind_group(
                0,
                &self.shadow_bind_group,
                &[(cascade as u64 * SHADOW_UNIFORM_STRIDE) as u32],
            );
            // Static casters, then the skinned ones: one pipeline switch.
            for skinned in [false, true] {
                let mut bound = false;
                for b in self
                    .batches
                    .iter()
                    .filter(|b| b.key.skinned == skinned && b.shadow_count > 0)
                {
                    let Some(mesh) = self.mesh_for(&b.key) else {
                        continue;
                    };
                    if !bound {
                        pass.set_pipeline(if skinned {
                            &self.pipelines.shadow_skinned
                        } else {
                            &self.pipelines.shadow
                        });
                        bound = true;
                    }
                    pass.set_bind_group(1, self.material_for(&b.key, fallbacks), &[]);
                    bind_mesh(&mut pass, mesh, skinned);
                    pass.draw_indexed(
                        0..mesh.index_count,
                        0,
                        b.shadow_first..b.shadow_first + b.shadow_count,
                    );
                }
            }
        }
    }

    /// The camera depth prepass (R2): every opaque and mask instance the
    /// scene pass will draw, depth only, with the scene pass's own vertex
    /// stage. The scene pass then loads this depth instead of clearing —
    /// its compare is `LessEqual`, and early-z drops the overdraw the
    /// prepass has already resolved.
    fn depth_prepass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel depth prepass"),
            timestamp_writes: self.timestamps_part(AO_PAIR, true, false),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.attachments.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        if self.instances.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        // The scene pass's opaque loop exactly, with the colour dropped.
        for (skinned, double_sided) in [(false, false), (false, true), (true, false), (true, true)]
        {
            let mut bound = false;
            for b in self.batches.iter().filter(|b| {
                b.key.skinned == skinned && b.double_sided == double_sided && b.count > 0
            }) {
                let Some(mesh) = self.mesh_for(&b.key) else {
                    continue;
                };
                if !bound {
                    pass.set_pipeline(self.pipelines.depth(skinned, double_sided));
                    bound = true;
                }
                pass.set_bind_group(1, self.material_for(&b.key, fallbacks), &[]);
                bind_mesh(&mut pass, mesh, skinned);
                pass.draw_indexed(0..mesh.index_count, 0, b.first..b.first + b.count);
            }
        }
    }

    /// The selection outline's mask: every outlined instance's silhouette
    /// at the target's size, cleared to 0 first, no depth — what other
    /// geometry hides is in it too, as Unity outlines it.
    fn outline_mask_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(mask) = self.outline_pass.mask_view() else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel outline mask"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: mask,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        pass.set_vertex_buffer(1, self.outline_buffer.slice(..));
        let mut bound: Option<(bool, bool)> = None;
        for d in &self.outline_draws {
            let Some(mesh) = self.mesh_for(&d.key) else {
                continue;
            };
            let variant = (d.key.skinned, d.double_sided);
            if bound != Some(variant) {
                pass.set_pipeline(self.pipelines.mask(d.key.skinned, d.double_sided));
                bound = Some(variant);
            }
            pass.set_bind_group(1, self.material_for(&d.key, fallbacks), &[]);
            bind_mesh(&mut pass, mesh, d.key.skinned);
            pass.draw_indexed(0..mesh.index_count, 0, d.index..d.index + 1);
        }
    }

    /// Screen-space ambient occlusion from the prepass's depth, then the
    /// blur that averages its per-pixel rotation away (R2). Group 0 is
    /// the frame uniform alone: the mesh pass's group 0 holds the target
    /// the blur writes.
    fn ao_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(ao) = self.attachments.ao.as_ref() else {
            return;
        };
        for (label, target, pipeline, bind_group, timestamps) in [
            (
                "fynkel ao",
                &ao.view,
                &self.pipelines.ao,
                &ao.ao_bind_group,
                None,
            ),
            (
                "fynkel ao blur",
                &ao.blur_view,
                &self.pipelines.ao_blur,
                &ao.blur_bind_group,
                self.timestamps_part(AO_PAIR, false, true),
            ),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                timestamp_writes: timestamps,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.ao_frame_bind_group, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Bloom (R3b): the bright-pass from the HDR frame into level 0, a
    /// downsample into each coarser level, then back up — each level
    /// replaced by itself mixed with the tent of the coarser one by
    /// `scatter` (the blend constant). Level 0 is what the post pass adds.
    fn bloom_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(bloom) = self.attachments.bloom.as_ref() else {
            return;
        };
        let scatter = f64::from(0.05 + 0.9 * self.post.bloom_scatter.clamp(0.0, 1.0));
        let n = bloom.levels.len();
        let mut steps: Vec<(&str, usize, &wgpu::RenderPipeline, &wgpu::BindGroup, bool)> =
            Vec::with_capacity(2 * n - 1);
        steps.push((
            "fynkel bloom prefilter",
            0,
            &self.pipelines.bloom_prefilter,
            &bloom.prefilter_bind_group,
            false,
        ));
        for i in 1..n {
            steps.push((
                "fynkel bloom down",
                i,
                &self.pipelines.bloom_down,
                &bloom.down[i - 1],
                false,
            ));
        }
        for i in (0..n - 1).rev() {
            steps.push((
                "fynkel bloom up",
                i,
                &self.pipelines.bloom_up,
                &bloom.up[i],
                true,
            ));
        }
        for (k, (label, level, pipeline, bind_group, blend)) in steps.into_iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                timestamp_writes: if k == 0 {
                    self.timestamps_part(2, true, false)
                } else {
                    None
                },
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &bloom.levels[level],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // An upsample mixes over what the downsample left.
                        load: if blend {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            if blend {
                pass.set_blend_constant(wgpu::Color {
                    r: scatter,
                    g: scatter,
                    b: scatter,
                    a: scatter,
                });
            }
            pass.set_bind_group(0, &self.ao_frame_bind_group, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Opaque batches (culled, then double-sided), the sky behind them,
    /// then the transparent instances back-to-front and the debug lines.
    fn scene_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let a = &self.attachments;
        let (view, resolve_target) = match &a.hdr_msaa_view {
            Some(msaa) => (msaa, Some(&a.hdr_view)),
            None => (&a.hdr_view, None),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel scene"),
            timestamp_writes: self.timestamps(1),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: if resolve_target.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &a.depth_view,
                depth_ops: Some(wgpu::Operations {
                    // The depth prepass has already written this frame's
                    // opaque depth when the AO stage ran (R2).
                    load: if self.ao_active {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(1.0)
                    },
                    // The overlay reads it after post (ADR-0019).
                    store: if self.overlay_pass.active() {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        if !self.instances.is_empty() {
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            // Culled batches first, then the double-sided ones, static
            // before skinned: at most one pipeline switch per variant.
            for (skinned, double_sided) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                let mut bound = false;
                for b in self.batches.iter().filter(|b| {
                    b.key.skinned == skinned && b.double_sided == double_sided && b.count > 0
                }) {
                    let Some(mesh) = self.mesh_for(&b.key) else {
                        continue;
                    };
                    if !bound {
                        pass.set_pipeline(self.pipelines.opaque(skinned, double_sided));
                        bound = true;
                    }
                    pass.set_bind_group(1, self.material_for(&b.key, fallbacks), &[]);
                    bind_mesh(&mut pass, mesh, skinned);
                    pass.draw_indexed(0..mesh.index_count, 0, b.first..b.first + b.count);
                }
            }
        }
        if let Some((_, sky)) = &self.sky_bind_group {
            pass.set_pipeline(&self.pipelines.sky);
            pass.set_bind_group(1, sky, &[]);
            pass.draw(0..3, 0..1);
        }
        if !self.transparent.is_empty() {
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            let mut bound: Option<(bool, bool)> = None;
            for t in &self.transparent {
                let Some(mesh) = self.mesh_for(&t.key) else {
                    continue;
                };
                let variant = (t.key.skinned, t.double_sided);
                if bound != Some(variant) {
                    pass.set_pipeline(self.pipelines.transparent(t.key.skinned, t.double_sided));
                    bound = Some(variant);
                }
                pass.set_bind_group(1, self.material_for(&t.key, fallbacks), &[]);
                bind_mesh(&mut pass, mesh, t.key.skinned);
                pass.draw_indexed(0..mesh.index_count, 0, t.index..t.index + 1);
            }
        }
    }

    /// One fullscreen triangle with `pipeline` and `bind_group` (group 1)
    /// into `target`, cleared to `CLEAR_COLOR` first.
    fn fullscreen_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            timestamp_writes,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn mesh_for(&self, key: &BatchKey) -> Option<&GpuMesh> {
        match &key.mesh {
            Some(r) => self.meshes.get(r).and_then(Option::as_ref),
            None => Some(&self.cube),
        }
    }

    fn material_for<'a>(&'a self, key: &BatchKey, fallbacks: &'a Fallbacks) -> &'a wgpu::BindGroup {
        match &key.material {
            Some(r) => match self.materials.get(r).and_then(Option::as_ref) {
                Some(m) => &m.bind_group,
                None => &fallbacks.default_material,
            },
            None => &fallbacks.default_material,
        }
    }

    // ------------------------------------------------------------ assets

    fn ensure_fallbacks(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.fallbacks.is_some() {
            return;
        }
        let white = upload_rgba8(
            device,
            queue,
            Rgba8Image::flat(wgpu::TextureFormat::Rgba8Unorm, &[255, 255, 255, 255]),
            "fynkel white",
        );
        let flat_normal = upload_rgba8(
            device,
            queue,
            Rgba8Image::flat(wgpu::TextureFormat::Rgba8Unorm, &[128, 128, 255, 255]),
            "fynkel flat normal",
        );
        let default_material = create_material_bind_group(
            device,
            &self.material_layout,
            &self.sampler,
            [&white, &flat_normal, &white, &white, &white].map(|t| &t.view),
            "fynkel default material",
        );
        write_brdf_lut(queue, &self.brdf_lut_texture);
        self.fallbacks = Some(Fallbacks {
            white,
            flat_normal,
            default_material,
        });
    }

    /// Loads the mesh on first use; `true` when it is on the GPU.
    fn mesh(&mut self, device: &wgpu::Device, r: &AssetRef) -> bool {
        if let Some(m) = self.meshes.get(r) {
            return m.is_some();
        }
        let loaded = match self.assets.as_ref().map(|a| a.mesh(r)) {
            Some(Ok(payload)) => Some(upload_mesh(
                device,
                &format!("fynkel mesh {r}"),
                &payload.vertices,
                &payload.indices,
                &payload.skin,
                Sphere::from_bounds(payload.bounds_min, payload.bounds_max),
            )),
            Some(Err(e)) => {
                self.asset_errors.push(format!("mesh {r}: {}", e.message));
                None
            }
            None => {
                self.asset_errors
                    .push(format!("mesh {r}: no asset source (set_assets)"));
                None
            }
        };
        let ok = loaded.is_some();
        self.meshes.insert(r.clone(), loaded);
        ok
    }

    fn texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, r: &AssetRef) -> bool {
        if let Some(t) = self.textures.get(r) {
            return t.is_some();
        }
        let loaded = match self.assets.as_ref().map(|a| a.texture(r)) {
            Some(Ok(payload)) => {
                let format = match payload.format {
                    TEXTURE_FORMAT_RGBA8_SRGB => wgpu::TextureFormat::Rgba8UnormSrgb,
                    TEXTURE_FORMAT_RGBA16F_LINEAR => wgpu::TextureFormat::Rgba16Float,
                    _ => wgpu::TextureFormat::Rgba8Unorm,
                };
                Some(upload_rgba8(
                    device,
                    queue,
                    Rgba8Image {
                        width: payload.width,
                        height: payload.height,
                        format,
                        mip_levels: payload.mip_levels,
                        rgba8: &payload.texels,
                    },
                    &format!("fynkel texture {r}"),
                ))
            }
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("texture {r}: {}", e.message));
                None
            }
            None => None,
        };
        let ok = loaded.is_some();
        self.textures.insert(r.clone(), loaded);
        ok
    }

    /// The frame's look (R3a): every volume whose profile reads, by its
    /// weight × how far inside its shape `eye` is, blended over the
    /// engine defaults (`fynkel_core::volume::resolve`); and how many
    /// contributed.
    fn resolve_volumes(&mut self, extract: &Extract, eye: Vec3) -> (PostSettings, u32) {
        if extract.volumes.is_empty() {
            return (PostSettings::default(), 0);
        }
        let mut amounts = Vec::with_capacity(extract.volumes.len());
        for v in &extract.volumes {
            let amount = v.volume.weight * volume::factor(&v.volume, &v.world, eye);
            if amount > 0.0 && self.volume_profile(&v.volume.profile).is_some() {
                amounts.push((v.volume.profile.clone(), amount, v.volume.priority));
            }
        }
        let mut contributions: Vec<volume::Contribution<'_>> = amounts
            .iter()
            .filter_map(|(r, amount, priority)| {
                let profile = self.volume_profiles.get(r)?.as_ref()?;
                Some(volume::Contribution {
                    profile,
                    amount: *amount,
                    priority: *priority,
                })
            })
            .collect();
        volume::resolve(PostSettings::default(), &mut contributions)
    }

    /// A volume profile, read once per reference (R3a); `None` when it
    /// could not be (reported once in `asset_errors`) or there is no
    /// asset store. A nil reference is "no profile", not an error.
    fn volume_profile(&mut self, r: &AssetRef) -> Option<VolumeProfile> {
        if r.is_nil() {
            return None;
        }
        if let Some(p) = self.volume_profiles.get(r) {
            return *p;
        }
        let loaded = match self.assets.as_ref().map(|a| a.volume_profile(r)) {
            Some(Ok(p)) => Some(p),
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("volume profile {r}: {}", e.message));
                None
            }
            None => None,
        };
        self.volume_profiles.insert(r.clone(), loaded);
        loaded
    }

    /// Reads the environment cooked next to an HDR sky (E1), once per
    /// reference; `false` when the texture has none (an 8-bit sky) or it
    /// could not be read.
    fn environment(&mut self, r: &AssetRef) -> bool {
        if let Some(e) = self.environments.get(r) {
            return e.is_some();
        }

        let loaded = match self.assets.as_ref().map(|a| a.environment(r)) {
            Some(Ok(e)) => e,
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("environment {r}: {}", e.message));
                None
            }
            None => None,
        };
        let ok = loaded.is_some();
        self.environments.insert(r.clone(), loaded);
        ok
    }

    /// The cooked material with its maps bound, loaded on first use; the
    /// model's defaults (and the default bind group: key `None`) when it
    /// cannot be read. A nil reference is not an error: "no material".
    fn material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        r: &AssetRef,
    ) -> (Option<AssetRef>, PbrMaterial) {
        if r.is_nil() {
            return (None, PbrMaterial::default());
        }
        if let Some(m) = self.materials.get(r) {
            return match m {
                Some(m) => (Some(r.clone()), m.material.clone()),
                None => (None, PbrMaterial::default()),
            };
        }
        let loaded = match self.assets.as_ref().map(|a| a.material(r)) {
            Some(Ok(m)) => Some(m),
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("material {r}: {}", e.message));
                None
            }
            None => None,
        };
        let gpu = loaded.map(|material| {
            let slots = [
                &material.base_color_texture,
                &material.normal_texture,
                &material.metallic_roughness_texture,
                &material.occlusion_texture,
                &material.emissive_texture,
            ];
            let mut textures = Vec::new();
            let mut resolved: Vec<Option<AssetRef>> = Vec::with_capacity(5);
            for slot in slots {
                let have = slot.as_ref().filter(|t| self.texture(device, queue, t));
                if let Some(t) = have {
                    textures.push(t.clone());
                }
                resolved.push(have.cloned());
            }
            let fallbacks = self.fallbacks.as_ref().expect("ensure_fallbacks ran");
            let views: [&wgpu::TextureView; 5] = std::array::from_fn(|i| match &resolved[i] {
                Some(t) => &self.textures[t].as_ref().expect("resolved").view,
                None if i == 1 => &fallbacks.flat_normal.view,
                None => &fallbacks.white.view,
            });
            let bind_group = create_material_bind_group(
                device,
                &self.material_layout,
                &self.sampler,
                views,
                &format!("fynkel material {r}"),
            );
            GpuMaterial {
                material,
                bind_group,
                textures,
            }
        });
        let out = match &gpu {
            Some(g) => (Some(r.clone()), g.material.clone()),
            None => (None, PbrMaterial::default()),
        };
        self.materials.insert(r.clone(), gpu);
        out
    }
}

/// An instance on its way into the buffer: its data, where it is drawn,
/// and the light list that goes into the same slot.
type Grouped = (Instance, Visibility, [u32; 4]);

/// The lights of `pool` that reach `sphere`, packed for an [`Instance`]:
/// up to [`MAX_LIGHTS`] pool indices one per byte (the first four in `x`,
/// the next four in `y`) and how many in `z`. Returns that list, its
/// length and whether lights were left out. When more than the budget
/// reach one object the strongest at its bounds win — the weakest are the
/// ones a viewer is least likely to miss, and the choice is stable while
/// nothing moves.
fn assign_lights(
    pool: &[GpuLight],
    masks: &[u32],
    layer_bit: u32,
    sphere: &Sphere,
    scratch: &mut Vec<(f32, u8)>,
) -> ([u32; 4], usize, bool) {
    scratch.clear();
    for (i, l) in pool.iter().enumerate() {
        // A light whose `culling_mask` leaves the object's layer out
        // (ADR-0020) does not light it.
        if masks[i] & layer_bit == 0 {
            continue;
        }
        let center = Vec3::new(l.position[0], l.position[1], l.position[2]);
        let range = l.position[3];
        // How far the light is from the nearest point of the bounds.
        let d = (center.distance(sphere.center) - sphere.radius).max(0.0);
        if d >= range {
            continue;
        }
        // The shader's own windowed inverse square there, on the light's
        // brightest channel: the most this object can get from it.
        let ratio = d / range;
        let window = 1.0 - ratio * ratio * ratio * ratio;
        let peak = l.color[0].max(l.color[1]).max(l.color[2]);
        scratch.push((peak * window * window / d.max(1e-2).powi(2), i as u8));
    }
    let overflow = scratch.len() > MAX_LIGHTS;
    if overflow {
        scratch.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        scratch.truncate(MAX_LIGHTS);
    }
    let mut list = [0u32; 4];
    for (n, (_, i)) in scratch.iter().enumerate() {
        list[n / 4] |= u32::from(*i) << ((n % 4) * 8);
    }
    list[2] = scratch.len() as u32;
    (list, scratch.len(), overflow)
}

/// The per-instance data for a material at `world`.
/// Where an extra world's light stands in the frame (`transform`: none for
/// the main extract's, which is left bit for bit as it is): its position,
/// its direction and its range, scaled by the transform's largest axis.
fn place_light(
    transform: Option<&Mat4>,
    position: Vec3,
    direction: Vec3,
    range: f32,
) -> (Vec3, Vec3, f32) {
    match transform {
        None => (position, direction, range),
        Some(t) => {
            let scale = t
                .x_axis
                .truncate()
                .length()
                .max(t.y_axis.truncate().length())
                .max(t.z_axis.truncate().length());
            (
                t.transform_point3(position),
                t.transform_vector3(direction),
                range * scale,
            )
        }
    }
}

fn instance_for(m: &PbrMaterial, world: Mat4, receive_shadows: bool) -> Instance {
    let cutoff = match m.alpha_mode {
        AlphaMode::Mask => m.alpha_cutoff.clamp(0.0, 1.0),
        AlphaMode::Opaque => NO_CUTOFF,
        AlphaMode::Blend => BLEND,
    };
    Instance {
        model: world.to_cols_array_2d(),
        tint: m.base_color.to_array(),
        emissive: (m.emissive * m.emissive_strength).extend(0.0).to_array(),
        params: [
            m.metallic,
            m.roughness,
            m.normal_scale,
            m.occlusion_strength,
        ],
        flags: [
            cutoff,
            0.0,
            // Filled in when the instance takes its place in the buffer.
            0.0,
            if receive_shadows { 1.0 } else { 0.0 },
        ],
    }
}

/// Binds `mesh`'s vertex and index buffers — and, for a skinned draw,
/// its weights on slot 2.
fn bind_mesh(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh, skinned: bool) {
    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
    if skinned {
        if let Some(skin) = &mesh.skin_buffer {
            pass.set_vertex_buffer(2, skin.slice(..));
        }
    }
    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
}

/// Deterministic per-entity tint so neighbouring placeholder cubes are
/// distinguishable.
fn tint_for(slot: u32) -> [f32; 4] {
    const PALETTE: [[f32; 3]; 6] = [
        [0.90, 0.30, 0.25],
        [0.25, 0.60, 0.90],
        [0.30, 0.80, 0.40],
        [0.95, 0.75, 0.20],
        [0.85, 0.85, 0.90],
        [0.55, 0.35, 0.75],
    ];
    let [r, g, b] = PALETTE[(slot as usize) % PALETTE.len()];
    [r, g, b, 1.0]
}

impl Pipelines {
    fn opaque(&self, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        match (skinned, double_sided) {
            (false, false) => &self.mesh,
            (false, true) => &self.mesh_double_sided,
            (true, false) => &self.mesh_skinned,
            (true, true) => &self.mesh_skinned_double_sided,
        }
    }

    /// The depth prepass twin of [`opaque`](Self::opaque): same vertex
    /// stage, same culling, no colour (R2).
    fn depth(&self, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        match (skinned, double_sided) {
            (false, false) => &self.depth,
            (false, true) => &self.depth_double_sided,
            (true, false) => &self.depth_skinned,
            (true, true) => &self.depth_skinned_double_sided,
        }
    }

    fn transparent(&self, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        match (skinned, double_sided) {
            (false, false) => &self.transparent,
            (false, true) => &self.transparent_double_sided,
            (true, false) => &self.transparent_skinned,
            (true, true) => &self.transparent_skinned_double_sided,
        }
    }

    /// The selection outline's mask twin of [`opaque`](Self::opaque).
    fn mask(&self, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        match (skinned, double_sided) {
            (false, false) => &self.mask,
            (false, true) => &self.mask_double_sided,
            (true, false) => &self.mask_skinned,
            (true, true) => &self.mask_skinned_double_sided,
        }
    }

    const fn count() -> usize {
        // The twenty-six below plus the overlay's grid, lines and
        // triangles (`overlay::OverlayPass`, ADR-0019) and the selection
        // outline's composite (`outline::OutlinePass`).
        30
    }
}

/// The bind group layouts the pipelines are built over.
struct Layouts<'a> {
    frame: &'a wgpu::BindGroupLayout,
    material: &'a wgpu::BindGroupLayout,
    single: &'a wgpu::BindGroupLayout,
    shadow: &'a wgpu::BindGroupLayout,
    skin: &'a wgpu::BindGroupLayout,
    /// The AO passes' group 0: the frame uniform alone (the mesh pass's
    /// group 0 holds the texture the blur writes).
    ao_frame: &'a wgpu::BindGroupLayout,
    ao: &'a wgpu::BindGroupLayout,
    /// The post pass's group 1 (the HDR, its parameters, bloom's level 0).
    post: &'a wgpu::BindGroupLayout,
    /// A bloom pass's group 1 (R3b).
    bloom: &'a wgpu::BindGroupLayout,
}

/// The bind group layouts the attachments' bind groups are made over.
struct AttachmentLayouts<'a> {
    present: &'a wgpu::BindGroupLayout,
    post: &'a wgpu::BindGroupLayout,
    ao: &'a wgpu::BindGroupLayout,
    bloom: &'a wgpu::BindGroupLayout,
}

/// Twenty-six of the pipeline variants (`docs/renderer.md`) for one sample
/// count — the overlay's three (grid, lines, triangles) live with its pass
/// in `overlay.rs`, the outline's composite in `outline.rs`: mesh (cull
/// back), mesh double-sided (cull none), transparent (blend, cull back),
/// transparent double-sided, the same four skinned (`vs_skinned`, a
/// third vertex buffer of joint weights and the palette storage buffer at
/// group 2), sky, shadow, shadow skinned, the four camera depth prepass
/// twins, the four outline mask twins (one sample whatever the scene's),
/// SSAO, the AO blur, bloom's three, post, present. Nothing else is ever created; a quality switch to
/// another sample count rebuilds this set.
fn create_pipelines(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    sample_count: u32,
    layouts: Layouts<'_>,
) -> Pipelines {
    let Layouts {
        frame: frame_layout,
        material: material_layout,
        single: single_layout,
        shadow: shadow_layout,
        skin: skin_layout,
        ao_frame: ao_frame_layout,
        ao: ao_layout,
        post: post_layout,
        bloom: bloom_layout,
    } = layouts;
    let module = |label, body: &str| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(
                format!("{}{body}", include_str!("shaders/frame.wgsl")).into(),
            ),
        })
    };
    let mesh_shader = module("fynkel mesh shader", include_str!("shaders/mesh.wgsl"));
    let sky_shader = module("fynkel sky shader", include_str!("shaders/sky.wgsl"));
    let post_shader = module("fynkel post shader", include_str!("shaders/post.wgsl"));
    let bloom_shader = module("fynkel bloom shader", include_str!("shaders/bloom.wgsl"));
    let present_shader = module(
        "fynkel present shader",
        include_str!("shaders/present.wgsl"),
    );
    let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fynkel shadow shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shadow.wgsl").into()),
    });
    // The AO passes read the scene depth as it is stored: multisampled
    // with the colour, or plain at one sample. `textureLoad`'s last
    // argument is the sample index or the mip level; 0 is right for both.
    let ao_shader = module(
        "fynkel ao shader",
        &include_str!("shaders/ao.wgsl").replace(
            "DEPTH_TEXTURE",
            if sample_count > 1 {
                "texture_depth_multisampled_2d"
            } else {
                "texture_depth_2d"
            },
        ),
    );

    // Group 2 (the joint palettes) is part of every mesh layout so the
    // passes bind it once; the static stages simply never read it.
    let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel mesh pl"),
        bind_group_layouts: &[Some(frame_layout), Some(material_layout), Some(skin_layout)],
        ..Default::default()
    });
    let shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel shadow pl"),
        bind_group_layouts: &[
            Some(shadow_layout),
            Some(material_layout),
            Some(skin_layout),
        ],
        ..Default::default()
    });
    let single_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel single-texture pl"),
        bind_group_layouts: &[Some(frame_layout), Some(single_layout)],
        ..Default::default()
    });
    let post_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel post pl"),
        bind_group_layouts: &[Some(frame_layout), Some(post_layout)],
        ..Default::default()
    });
    // The bloom passes share the AO passes' group 0: the frame uniform
    // alone.
    let bloom_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel bloom pl"),
        bind_group_layouts: &[Some(ao_frame_layout), Some(bloom_layout)],
        ..Default::default()
    });
    let ao_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel ao pl"),
        bind_group_layouts: &[Some(ao_frame_layout), Some(ao_layout)],
        ..Default::default()
    });
    let hdr_target = [Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    // Premultiplied accumulation: colour by the fragment's alpha, the
    // HDR alpha (the post pass's coverage) by `1 - alpha`.
    let hdr_blend_target = [Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        }),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let multisample = wgpu::MultisampleState {
        count: sample_count,
        ..Default::default()
    };
    let mesh_buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: Vertex::STRIDE as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3, 1 => Float32x3, 2 => Float32x4,
                3 => Float32x2, 4 => Float32x2, 5 => Float32x4
            ],
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
                10 => Float32x4, 11 => Float32x4, 12 => Float32x4, 13 => Float32x4
            ],
        }),
    ];
    // The skinned stages add slot 2: the cooked `SkinWeight` (four u16
    // joints, four f32 weights).
    let skinned_buffers = [
        mesh_buffers[0].clone(),
        mesh_buffers[1].clone(),
        Some(wgpu::VertexBufferLayout {
            array_stride: SkinWeight::STRIDE as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![14 => Uint16x4, 15 => Float32x4],
        }),
    ];
    // `fs_depth` (no colour target) is the camera depth prepass; the rest
    // shade. The compare is `LessEqual` because a prepass has already
    // written exactly the depth the scene pass computes — the vertex
    // stage is the same function, so the two agree to the bit.
    let mesh = |label,
                skinned: bool,
                cull_mode,
                targets: &[Option<wgpu::ColorTargetState>],
                depth_write,
                entry: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: if skinned {
                    &skinned_buffers
                } else {
                    &mesh_buffers
                },
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(depth_write),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample,
            multiview_mask: None,
            cache: None,
        })
    };
    let shadow_pipeline = |label, skinned: bool| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&shadow_pl),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: if skinned {
                    &skinned_buffers
                } else {
                    &mesh_buffers
                },
            },
            fragment: Some(wgpu::FragmentState {
                module: &shadow_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SHADOW_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    // The outline's mask (`outline.rs`): the scene's vertex stages and
    // culling at the target's size, one sample and no depth — the whole
    // silhouette, hidden parts included. `Max` keeps a selected texel's 1
    // over a child's 0.5 in whatever order they land (WebGPU allows the
    // min/max operations with factors of one only).
    let keep_max = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    };
    let mask_target = [Some(wgpu::ColorTargetState {
        format: outline::MASK_FORMAT,
        blend: Some(wgpu::BlendState {
            color: keep_max,
            alpha: keep_max,
        }),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let mask = |label, skinned: bool, cull_mode| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: if skinned {
                    &skinned_buffers
                } else {
                    &mesh_buffers
                },
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_mask"),
                compilation_options: Default::default(),
                targets: &mask_target,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let shadow = shadow_pipeline("fynkel shadow pipeline", false);
    let shadow_skinned = shadow_pipeline("fynkel shadow skinned pipeline", true);
    let ao_target = [Some(wgpu::ColorTargetState {
        format: AO_FORMAT,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let ao_pipeline = |label, entry: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&ao_pl),
            vertex: wgpu::VertexState {
                module: &ao_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ao_shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &ao_target,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            // The AO targets are single-sample whatever the scene is.
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let bloom_pipeline = |label, entry: &str, blend| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&bloom_pl),
            vertex: wgpu::VertexState {
                module: &bloom_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &bloom_shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: BLOOM_FORMAT,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    // `lerp(finer, coarser, scatter)`: the upsample is the source, the
    // level it lands on the destination.
    let scatter = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Constant,
        dst_factor: wgpu::BlendFactor::OneMinusConstant,
        operation: wgpu::BlendOperation::Add,
    };
    let bloom_scatter = wgpu::BlendState {
        color: scatter,
        alpha: scatter,
    };
    let fullscreen = |label,
                      layout: &wgpu::PipelineLayout,
                      shader: &wgpu::ShaderModule,
                      targets: &[Option<wgpu::ColorTargetState>],
                      depth,
                      multisample| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: depth,
            multisample,
            multiview_mask: None,
            cache: None,
        })
    };
    let sky = fullscreen(
        "fynkel sky pipeline",
        &single_pl,
        &sky_shader,
        &hdr_target,
        Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample,
    );
    let host_target = [Some(wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let post = fullscreen(
        "fynkel post pipeline",
        &post_pl,
        &post_shader,
        &host_target,
        None,
        Default::default(),
    );
    let present = fullscreen(
        "fynkel present pipeline",
        &single_pl,
        &present_shader,
        &host_target,
        None,
        Default::default(),
    );
    let back = Some(wgpu::Face::Back);
    Pipelines {
        mesh: mesh(
            "fynkel mesh pipeline",
            false,
            back,
            &hdr_target,
            true,
            "fs_main",
        ),
        mesh_double_sided: mesh(
            "fynkel mesh pipeline (double-sided)",
            false,
            None,
            &hdr_target,
            true,
            "fs_main",
        ),
        transparent: mesh(
            "fynkel transparent pipeline",
            false,
            back,
            &hdr_blend_target,
            false,
            "fs_main",
        ),
        transparent_double_sided: mesh(
            "fynkel transparent pipeline (double-sided)",
            false,
            None,
            &hdr_blend_target,
            false,
            "fs_main",
        ),
        mesh_skinned: mesh(
            "fynkel mesh skinned pipeline",
            true,
            back,
            &hdr_target,
            true,
            "fs_main",
        ),
        mesh_skinned_double_sided: mesh(
            "fynkel mesh skinned pipeline (double-sided)",
            true,
            None,
            &hdr_target,
            true,
            "fs_main",
        ),
        transparent_skinned: mesh(
            "fynkel transparent skinned pipeline",
            true,
            back,
            &hdr_blend_target,
            false,
            "fs_main",
        ),
        transparent_skinned_double_sided: mesh(
            "fynkel transparent skinned pipeline (double-sided)",
            true,
            None,
            &hdr_blend_target,
            false,
            "fs_main",
        ),
        sky,
        shadow,
        shadow_skinned,
        depth: mesh("fynkel depth pipeline", false, back, &[], true, "fs_depth"),
        depth_double_sided: mesh(
            "fynkel depth pipeline (double-sided)",
            false,
            None,
            &[],
            true,
            "fs_depth",
        ),
        depth_skinned: mesh(
            "fynkel depth skinned pipeline",
            true,
            back,
            &[],
            true,
            "fs_depth",
        ),
        depth_skinned_double_sided: mesh(
            "fynkel depth skinned pipeline (double-sided)",
            true,
            None,
            &[],
            true,
            "fs_depth",
        ),
        mask: mask("fynkel outline mask pipeline", false, back),
        mask_double_sided: mask("fynkel outline mask pipeline (double-sided)", false, None),
        mask_skinned: mask("fynkel outline mask skinned pipeline", true, back),
        mask_skinned_double_sided: mask(
            "fynkel outline mask skinned pipeline (double-sided)",
            true,
            None,
        ),
        ao: ao_pipeline("fynkel ao pipeline", "fs_main"),
        ao_blur: ao_pipeline("fynkel ao blur pipeline", "fs_blur"),
        bloom_prefilter: bloom_pipeline(
            "fynkel bloom prefilter pipeline",
            "fs_prefilter",
            wgpu::BlendState::REPLACE,
        ),
        bloom_down: bloom_pipeline(
            "fynkel bloom down pipeline",
            "fs_down",
            wgpu::BlendState::REPLACE,
        ),
        bloom_up: bloom_pipeline("fynkel bloom up pipeline", "fs_up", bloom_scatter),
        post,
        present,
        sample_count,
        encode_srgb: !format.is_srgb(),
    }
}

fn create_material_sampler(device: &wgpu::Device, anisotropy: u32) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("fynkel material sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: anisotropy.clamp(1, 16) as u16,
        ..Default::default()
    })
}

/// The scene targets at `quality`'s internal size and sample count, the
/// LDR intermediate when the render scale is not 1, and the post/present
/// bind groups over them.
#[allow(clippy::too_many_arguments)]
fn create_attachments(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    quality: &Quality,
    width: u32,
    height: u32,
    layouts: AttachmentLayouts<'_>,
    present_sampler: &wgpu::Sampler,
    post_buffer: &wgpu::Buffer,
) -> Attachments {
    let AttachmentLayouts {
        present: single_layout,
        post: post_layout,
        ao: ao_layout,
        bloom: bloom_layout,
    } = layouts;
    let (w, h) = quality.internal_size(width, height);
    let samples = quality.msaa;
    // The AO passes sample the depth the prepass stored (R2).
    let depth_view = create_attachment(
        device,
        "fynkel depth",
        w,
        h,
        samples,
        DEPTH_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let ao = quality.ambient_occlusion.then(|| {
        let ao_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let view = create_attachment(device, "fynkel ao", w, h, 1, AO_FORMAT, ao_usage);
        let blur_view = create_attachment(device, "fynkel ao blur", w, h, 1, AO_FORMAT, ao_usage);
        AoTargets {
            ao_bind_group: create_ao_bind_group(
                device,
                ao_layout,
                &depth_view,
                &blur_view,
                "fynkel ao bg",
            ),
            blur_bind_group: create_ao_bind_group(
                device,
                ao_layout,
                &depth_view,
                &view,
                "fynkel ao blur bg",
            ),
            view,
            blur_view,
        }
    });
    let hdr_view = create_attachment(
        device,
        "fynkel hdr",
        w,
        h,
        1,
        HDR_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let hdr_msaa_view = (samples > 1).then(|| {
        create_attachment(
            device,
            "fynkel hdr (multisampled)",
            w,
            h,
            samples,
            HDR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
    });
    let ldr_view = (quality.render_scale != 1.0).then(|| {
        create_attachment(
            device,
            "fynkel ldr",
            w,
            h,
            1,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        )
    });
    // Bloom (R3b): half the internal size, as many levels as fit (down
    // to 2 texels on the short side), each one a view the passes attach.
    let (bw, bh) = ((w / 2).max(1), (h / 2).max(1));
    let mut bloom_texels = 0u64;
    let bloom = quality.bloom.then(|| {
        let mut count = 1;
        while count < BLOOM_LEVELS && (bw >> count) >= 2 && (bh >> count) >= 2 {
            count += 1;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fynkel bloom"),
            size: wgpu::Extent3d {
                width: bw,
                height: bh,
                depth_or_array_layers: 1,
            },
            mip_level_count: count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: BLOOM_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels: Vec<wgpu::TextureView> = (0..count)
            .map(|level| {
                bloom_texels += u64::from((bw >> level).max(1)) * u64::from((bh >> level).max(1));
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("fynkel bloom level"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let reads = |view: &wgpu::TextureView, label| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: bloom_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(present_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                ],
            })
        };
        let n = levels.len();
        BloomTargets {
            prefilter_bind_group: reads(&hdr_view, "fynkel bloom prefilter bg"),
            down: levels[..n - 1]
                .iter()
                .map(|v| reads(v, "fynkel bloom down bg"))
                .collect(),
            up: levels[1..]
                .iter()
                .map(|v| reads(v, "fynkel bloom up bg"))
                .collect(),
            levels,
        }
    });
    // Without the chain the post pass reads a black texel (textures start
    // zeroed).
    let bloom_placeholder = bloom.is_none().then(|| {
        create_attachment(
            device,
            "fynkel bloom placeholder",
            1,
            1,
            1,
            BLOOM_FORMAT,
            wgpu::TextureUsages::TEXTURE_BINDING,
        )
    });
    let bloom_view = match (&bloom, &bloom_placeholder) {
        (Some(b), _) => &b.levels[0],
        (None, Some(p)) => p,
        (None, None) => unreachable!("one of the two exists"),
    };
    let post_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fynkel post bg"),
        layout: post_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(present_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&hdr_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: post_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(bloom_view),
            },
        ],
    });
    let present_bind_group = ldr_view.as_ref().map(|ldr| {
        create_single_bind_group(
            device,
            single_layout,
            present_sampler,
            ldr,
            post_buffer,
            "fynkel present bg",
        )
    });
    // Depth32 × samples, HDR rgba16f × samples when resolved, the HDR
    // resolve, the LDR at render scale (4 bytes a texel), the two AO
    // targets (one byte each), bloom's chain (rgba16f).
    let texels = u64::from(w.max(1)) * u64::from(h.max(1));
    let bytes = texels * 4 * u64::from(samples)
        + if samples > 1 {
            texels * 8 * u64::from(samples)
        } else {
            0
        }
        + texels * 8
        + if ldr_view.is_some() { texels * 4 } else { 0 }
        + if ao.is_some() { texels * 2 } else { 0 }
        + bloom_texels * 8;
    Attachments {
        width: w,
        height: h,
        sample_count: samples,
        depth_view,
        ao,
        bloom,
        hdr_msaa_view,
        hdr_view,
        ldr_view,
        post_bind_group,
        present_bind_group,
        bytes,
    }
}

fn create_shadow_map(device: &wgpu::Device, resolution: u32) -> ShadowMap {
    ShadowMap {
        view: create_attachment(
            device,
            "fynkel shadow map",
            resolution,
            resolution,
            1,
            SHADOW_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        ),
        resolution,
    }
}

#[allow(clippy::too_many_arguments)]
fn create_frame_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    shadow_map: &wgpu::TextureView,
    shadow_sampler: &wgpu::Sampler,
    cube_sampler: &wgpu::Sampler,
    ibl: &Ibl,
    brdf_lut: &GpuTexture,
    ao: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fynkel frame bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(shadow_map),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(cube_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&ibl.irradiance.view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&ibl.specular.view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&brdf_lut.view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(ao),
            },
        ],
    })
}

/// The cooked environment on the GPU (E1): the cosine-convolved
/// irradiance cube (E/π) and the GGX-prefiltered specular cube, whose mip
/// levels run roughness 0 → 1. Both are `Rgba16Float`, both sampled with
/// the renderer's clamped trilinear `cube_sampler`.
struct Ibl {
    irradiance: GpuTexture,
    specular: GpuTexture,
    specular_mips: u32,
}

impl Ibl {
    /// What a scene without an HDR environment binds: black 1×1 cubes, so
    /// group 0 is always complete and the shader needs no variant.
    /// wgpu zeroes a fresh texture, so there is nothing to write.
    fn none(device: &wgpu::Device) -> Ibl {
        Ibl {
            irradiance: create_cube(device, 1, 1, "fynkel ibl none").0,
            specular: create_cube(device, 1, 1, "fynkel ibl none").0,
            specular_mips: 1,
        }
    }

    /// Uploads a cooked `ENVM` payload.
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        env: &EnvironmentPayload,
        r: &AssetRef,
    ) -> Ibl {
        let halves = |v: &[u16]| -> Vec<u8> {
            let mut out = Vec::with_capacity(v.len() * 2);
            for h in v {
                out.extend_from_slice(&h.to_le_bytes());
            }
            out
        };
        let irradiance_bytes = halves(&env.irradiance);
        let face = (env.irradiance_size * env.irradiance_size) as usize * 8;
        let irradiance_faces: Vec<&[u8]> = (0..6)
            .map(|f| &irradiance_bytes[f * face..(f + 1) * face])
            .collect();
        let irradiance = upload_cube(
            device,
            queue,
            env.irradiance_size,
            1,
            &irradiance_faces,
            &format!("fynkel irradiance {r}"),
        );
        // Every specular level, face by face (the payload is mip-major).
        let mut specular_bytes = Vec::with_capacity(env.specular_mips as usize);
        for mip in 0..env.specular_mips {
            specular_bytes.push(halves(env.specular_level(mip).unwrap_or(&[])));
        }
        let mut specular_faces: Vec<&[u8]> = Vec::with_capacity(env.specular_mips as usize * 6);
        for (mip, bytes) in specular_bytes.iter().enumerate() {
            let size = (env.specular_size >> mip).max(1) as usize;
            let face = size * size * 8;
            for f in 0..6 {
                specular_faces.push(&bytes[f * face..(f + 1) * face]);
            }
        }
        let specular = upload_cube(
            device,
            queue,
            env.specular_size,
            env.specular_mips,
            &specular_faces,
            &format!("fynkel environment {r}"),
        );
        Ibl {
            irradiance,
            specular,
            specular_mips: env.specular_mips,
        }
    }

    fn bytes(&self) -> u64 {
        self.irradiance.bytes + self.specular.bytes
    }
}

/// Uploads a `Rgba16Float` cube map. `faces` is mip-major, face-minor
/// (the `ENVM` order): level 0's six faces, then level 1's, …
fn upload_cube(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: u32,
    mips: u32,
    faces: &[&[u8]],
    label: &str,
) -> GpuTexture {
    let (mut gpu, texture) = create_cube(device, size, mips, label);
    let mut bytes = 0u64;
    for (i, data) in faces.iter().enumerate() {
        let (mip, face) = (i as u32 / 6, i as u32 % 6);
        let side = (size >> mip).max(1);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: face,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * 8),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
        bytes += data.len() as u64;
    }
    gpu.bytes = bytes;
    gpu
}

/// An empty `Rgba16Float` cube (zeroed by wgpu) and its texture, so a
/// caller can write the levels later.
fn create_cube(
    device: &wgpu::Device,
    size: u32,
    mips: u32,
    label: &str,
) -> (GpuTexture, wgpu::Texture) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    });
    (GpuTexture { view, bytes: 0 }, texture)
}

/// The split-sum environment BRDF (E1): `∫ f(l, v) / F · n·l dl` over the
/// GGX lobe, integrated once on the CPU — `x` = `n·v`, `y` = roughness,
/// `r` = the scale on `F0`, `g` = the bias. The specular answer for a
/// prefiltered radiance `L` is `L · (F0 · r + g)`. 64×64 RGBA16F; the
/// table depends on nothing but the BRDF, so it is computed once per
/// process and uploaded per renderer.
fn create_brdf_lut(device: &wgpu::Device) -> (GpuTexture, wgpu::Texture) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fynkel brdf lut"),
        size: wgpu::Extent3d {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (
        GpuTexture {
            view,
            bytes: (BRDF_LUT_SIZE * BRDF_LUT_SIZE * 8) as u64,
        },
        texture,
    )
}

/// Fills the table on the first frame (`new` has no queue); the texels
/// are integrated once per process.
fn write_brdf_lut(queue: &wgpu::Queue, texture: &wgpu::Texture) {
    static LUT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        LUT.get_or_init(brdf_lut_texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(BRDF_LUT_SIZE * 8),
            rows_per_image: Some(BRDF_LUT_SIZE),
        },
        wgpu::Extent3d {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
    );
}

const BRDF_LUT_SIZE: u32 = 64;
const BRDF_LUT_SAMPLES: u32 = 512;

fn brdf_lut_texels() -> Vec<u8> {
    use fynkel_cooked::payload::f32_to_f16;
    let n = BRDF_LUT_SIZE as usize;
    let mut out = Vec::with_capacity(n * n * 8);
    for y in 0..n {
        let roughness = (y as f32 + 0.5) / n as f32;
        let a = roughness * roughness;
        let a2 = a * a;
        for x in 0..n {
            let ndotv = ((x as f32 + 0.5) / n as f32).max(1e-3);
            let v = [(1.0 - ndotv * ndotv).max(0.0).sqrt(), 0.0, ndotv];
            let (mut scale, mut bias) = (0.0f32, 0.0f32);
            for i in 0..BRDF_LUT_SAMPLES {
                let (x1, x2) = hammersley(i, BRDF_LUT_SAMPLES);
                let phi = 2.0 * std::f32::consts::PI * x1;
                let cos_theta = ((1.0 - x2) / (1.0 + (a2 - 1.0) * x2)).sqrt();
                let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                // The half vector in the tangent frame around +Z = N.
                let h = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                let vh = v[0] * h[0] + v[1] * h[1] + v[2] * h[2];
                let l = [
                    2.0 * vh * h[0] - v[0],
                    2.0 * vh * h[1] - v[1],
                    2.0 * vh * h[2] - v[2],
                ];
                if l[2] <= 0.0 {
                    continue;
                }
                let (ndotl, ndoth, vdoth) = (l[2], h[2].max(0.0), vh.max(0.0));
                // Height-correlated Smith visibility, the shader's.
                let gv = ndotl * (ndotv * ndotv * (1.0 - a2) + a2).sqrt();
                let gl = ndotv * (ndotl * ndotl * (1.0 - a2) + a2).sqrt();
                let vis = 0.5 / (gv + gl).max(1e-6);
                // pdf = D · n·h / (4 · v·h); the D cancels.
                let weight = 4.0 * vis * ndotl * vdoth / ndoth.max(1e-6);
                let fc = (1.0 - vdoth).powi(5);
                scale += (1.0 - fc) * weight;
                bias += fc * weight;
            }
            let (scale, bias) = (
                scale / BRDF_LUT_SAMPLES as f32,
                bias / BRDF_LUT_SAMPLES as f32,
            );
            for v in [scale, bias, 0.0, 1.0] {
                out.extend_from_slice(&f32_to_f16(v).to_le_bytes());
            }
        }
    }
    out
}

/// The Hammersley point set (the cook's, `fynkel_import::environment`).
fn hammersley(i: u32, n: u32) -> (f32, f32) {
    let mut bits = i;
    bits = bits.rotate_right(16);
    bits = ((bits & 0x5555_5555) << 1) | ((bits & 0xAAAA_AAAA) >> 1);
    bits = ((bits & 0x3333_3333) << 2) | ((bits & 0xCCCC_CCCC) >> 2);
    bits = ((bits & 0x0F0F_0F0F) << 4) | ((bits & 0xF0F0_F0F0) >> 4);
    bits = ((bits & 0x00FF_00FF) << 8) | ((bits & 0xFF00_FF00) >> 8);
    (i as f32 / n as f32, bits as f32 * 2.328_306_4e-10)
}

fn create_joint_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel joint palettes"),
        size: capacity as u64 * std::mem::size_of::<Mat4>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_joint_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    joints: &wgpu::Buffer,
    light_lists: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fynkel skin bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: joints.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: light_lists.as_entire_binding(),
            },
        ],
    })
}

/// The per-instance light lists (E1): four `u32` each, indexed by an
/// instance's `flags[2]`.
fn create_light_list_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel light lists"),
        size: capacity as u64 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel instances"),
        size: capacity as u64 * std::mem::size_of::<Instance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Group 1 of the AO passes: the scene depth as the sample count stores
/// it, and the other AO target.
fn create_ao_layout(device: &wgpu::Device, sample_count: u32) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fynkel ao bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: sample_count > 1,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

/// Group 0 of the AO passes: the frame uniform on its own.
fn create_ao_frame_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fynkel ao frame bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn create_ao_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    depth: &wgpu::TextureView,
    other: &wgpu::TextureView,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(depth),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(other),
            },
        ],
    })
}

fn create_attachment(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    sample_count: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    tex.create_view(&Default::default())
}

fn create_single_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
    params: &wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

fn create_material_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    views: [&wgpu::TextureView; 5],
    label: &str,
) -> wgpu::BindGroup {
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::Sampler(sampler),
    }];
    for (i, v) in views.iter().enumerate() {
        entries.push(wgpu::BindGroupEntry {
            binding: i as u32 + 1,
            resource: wgpu::BindingResource::TextureView(v),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &entries,
    })
}

fn upload_mesh(
    device: &wgpu::Device,
    label: &str,
    vertices: &[Vertex],
    indices: &[u32],
    skin: &[SkinWeight],
    bounds: Sphere,
) -> GpuMesh {
    // The cooked layout is the GPU layout: 18 little-endian f32 per vertex.
    let mut bytes = Vec::with_capacity(vertices.len() * Vertex::STRIDE);
    for v in vertices {
        v.write(&mut bytes);
    }
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &bytes,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    // A skinned mesh's weights as cooked: four u16 joints, four f32
    // weights per vertex (24 bytes).
    let skin_buffer = (!skin.is_empty()).then(|| {
        let mut bytes = Vec::with_capacity(skin.len() * SkinWeight::STRIDE);
        for w in skin {
            w.write(&mut bytes);
        }
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: &bytes,
            usage: wgpu::BufferUsages::VERTEX,
        })
    });
    GpuMesh {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
        skin_buffer,
        bounds,
    }
}

/// A top-row-first image with `mip_levels` levels concatenated in
/// `rgba8` (the cooked `TEXR` layout; 1 for a flat image) — RGBA8, or
/// RGBA16F when `format` says so (an HDR sky, E1).
struct Rgba8Image<'a> {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    mip_levels: u32,
    rgba8: &'a [u8],
}

impl<'a> Rgba8Image<'a> {
    fn flat(format: wgpu::TextureFormat, rgba8: &'a [u8; 4]) -> Self {
        Rgba8Image {
            width: 1,
            height: 1,
            format,
            mip_levels: 1,
            rgba8,
        }
    }
}

/// Uploads every level of `image`.
fn upload_rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: Rgba8Image,
    label: &str,
) -> GpuTexture {
    let Rgba8Image {
        width,
        height,
        format,
        mip_levels,
        rgba8,
    } = image;
    let mip_levels = mip_levels.max(1);
    let texel = format.block_copy_size(None).unwrap_or(4);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: mip_levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut at = 0usize;
    for level in 0..mip_levels {
        let (w, h) = fynkel_cooked::payload::mip_size(width, height, level);
        let len = w as usize * h as usize * texel as usize;
        let Some(bytes) = rgba8.get(at..at + len) else {
            break;
        };
        at += len;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * texel),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }
    GpuTexture {
        view: texture.create_view(&Default::default()),
        bytes: at as u64,
    }
}

fn cube_mesh() -> (Vec<Vertex>, Vec<u32>) {
    // Six faces of a unit cube centred at the origin; CCW winding from outside.
    // (normal, tangent u, tangent v)
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, u, v) in faces {
        let n = Vec3::from(normal);
        let u = Vec3::from(u);
        let v = Vec3::from(v);
        let base = vertices.len() as u32;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = (n + u * su + v * sv) * 0.5;
            vertices.push(Vertex {
                position: p.to_array(),
                normal,
                tangent: [u.x, u.y, u.z, 1.0],
                uv0: [(su + 1.0) * 0.5, (1.0 - sv) * 0.5],
                ..Vertex::default()
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

/// Creates a headless Metal/any-backend device for tests and offscreen hosts.
pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("fynkel headless device"),
        required_features: device_features(&adapter),
        required_limits: device_limits(&adapter),
        ..Default::default()
    }))
    .ok()
}

/// Features to request for [`ExtractRenderer`]: pass timestamps when the
/// adapter samples them (`FrameStats::gpu_*`, M3-16); nothing the renderer
/// cannot do without.
pub fn device_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & wgpu::Features::TIMESTAMP_QUERY
}

/// Limits to request for [`ExtractRenderer`]: WebGPU defaults, lowered to
/// what the adapter offers (the iOS simulator's Metal device sits below
/// them, e.g. 15 inter-stage variables). The renderer needs nothing beyond
/// the downlevel set, so every Apple adapter satisfies the result.
pub fn device_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    wgpu::Limits::default().or_worse_values_from(&adapter.limits())
}

/// Tiny inline executor so the renderer does not depend on an async runtime.
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY: the vtable functions do nothing with the (null) data pointer.
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_micros(50));
    }
}

/// Test/diagnostic helper: renders the extract into an offscreen RGBA8
/// texture and returns the pixels (row-major, tightly packed). This is the
/// readback the production paths never do per frame.
pub fn render_offscreen(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut ExtractRenderer,
    extract: &Extract,
) -> Vec<[u8; 4]> {
    let (width, height) = renderer.size();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fynkel offscreen"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // Rows must be 256-byte aligned for the copy.
    let bytes_per_row = (width * 4).div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel readback"),
        size: bytes_per_row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    renderer.prepare(device, queue, extract);
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &view);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().expect("map callback").expect("map readback");
    let data = slice.get_mapped_range().expect("mapped range");
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height as usize {
        let row = &data[y * bytes_per_row as usize..][..width as usize * 4];
        for px in row.chunks_exact(4) {
            pixels.push([px[0], px[1], px[2], px[3]]);
        }
    }
    drop(data);
    readback.unmap();
    pixels
}

/// `CLEAR_COLOR` as RGBA8 (the bytes the post pass writes where nothing
/// was drawn, whatever the target format).
pub fn clear_rgba8() -> [u8; 4] {
    [
        (CLEAR_COLOR.r * 255.0).round() as u8,
        (CLEAR_COLOR.g * 255.0).round() as u8,
        (CLEAR_COLOR.b * 255.0).round() as u8,
        255,
    ]
}

/// The number of render pipelines a renderer ever creates.
pub const PIPELINE_VARIANTS: usize = Pipelines::count();
