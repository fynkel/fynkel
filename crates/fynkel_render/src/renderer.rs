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
//! 1. The scene: every draw, then the debug lines on top. Above
//!    [`Quality::Low`] it is drawn into a half-float image (the scene
//!    image); at low, straight into the view.
//! 2. The last pass, when there is a scene image: the image onto the view
//!    (`finish.wgsl`). Effects that need the whole picture (bloom) go here.

use std::sync::{Arc, OnceLock};

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3, Vec4};

use crate::assets::{self, Assets, Kit, PipelineMaker, SharedAssets};
use crate::clusters::{Clusters, Grid, Reach, TILES_X, TILES_Y};
use crate::frame_ring::{FrameRing, Parts};
use crate::gpu::Gpu;
use crate::scene::{ColorSpace, LightKind, MaterialTextures, MeshKey, Quality, RenderScene, ShadowFilter, TextureKey};
use crate::shadows::{self, Caster, MAX_CASCADES, MAX_MAPS, MAX_SPOT_MAPS, ShadowMaps};
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
    /// GPU memory of the shadow maps the view holds, shared with the kit's
    /// other views or not.
    pub shadow_bytes: u64,
}

/// `Frame` in forward.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
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

/// The per-draw vertex data (instance step), locations 4–10.
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
}

/// The image the scene is drawn into, its format, and the bind group the
/// last pass reads it through.
struct SceneImage {
    format: wgpu::TextureFormat,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
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

/// What a forward pipeline is made for: the shader's work left out where a
/// frame or a material doesn't use it (hardening R11).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Variant {
    cutout: bool,
    faces: Faces,
    /// Some light casts shadows this frame.
    shadowed: bool,
    /// The material has a texture.
    textured: bool,
}

impl Variant {
    const COUNT: usize = 2 * Faces::ALL.len() * 2 * 2;

    /// Its index in `Pipelines::forward`.
    fn index(self) -> usize {
        ((usize::from(self.cutout) * Faces::ALL.len() + self.faces as usize) * 2 + usize::from(self.shadowed)) * 2
            + usize::from(self.textured)
    }
}

/// One instanced draw call, as `prepare` left it for `encode`: `count`
/// instances of `mesh` starting at instance `first`, with one set of
/// textures, opaque or cut out, facing one way.
#[derive(Clone, Copy)]
struct Batch {
    cutout: bool,
    faces: Faces,
    textures: MaterialTextures,
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
    cutout: bool,
    textures: MaterialTextures,
    mesh: MeshKey,
    first: u32,
    count: u32,
}

/// A caster's sort key: its map, cut out or not, its textures (a cut-out's
/// only) and its mesh.
type CasterKey = (u8, bool, MaterialTextures, u64);

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
    maps_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    forward_shader: OnceLock<(wgpu::ShaderModule, wgpu::PipelineLayout)>,
    /// One for each [`Variant`], at its index.
    forward: [OnceLock<wgpu::RenderPipeline>; Variant::COUNT],
    lines: OnceLock<wgpu::RenderPipeline>,
    /// The last pass, onto a view: for sRGB formats only (a view's).
    finish: OnceLock<wgpu::RenderPipeline>,
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
    instances: Vec<Instance>,
    batches: Vec<Batch>,
    line_vertices: Vec<LineVertex>,
    /// The scene's draws that are seen, each with its sort key, sorted.
    order: Vec<(SortKey, u32)>,
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
    size: (u32, u32),
    /// This frame's level: the scene's, or the device's.
    quality: Quality,
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
const INSTANCE_SIZE: u64 = std::mem::size_of::<Instance>() as u64;

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
                (kit.shadows.view_layout.clone(), kit.shadows.view_stride, MAX_MAPS as u64 * kit.shadows.view_stride),
                64 * INSTANCE_SIZE,
            ),
            quality: kit.quality(),
            format,
            image_pipelines: None,
            assets,
            targets: None,
            uniforms: FrameUniforms::zeroed(),
            instances: Vec::new(),
            batches: Vec::new(),
            line_vertices: Vec::new(),
            order: Vec::new(),
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
            size: (0, 0),
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

    /// The level the last frame was prepared at: its scene's, or the
    /// device's when the scene didn't choose.
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
        self.size = (width.max(1), height.max(1));
        self.quality = scene.quality.unwrap_or(self.kit.quality());
        self.instances.clear();
        self.batches.clear();
        self.line_vertices.clear();
        self.shadow_candidates.clear();
        self.caster_order.clear();
        self.casters.clear();
        self.caster_batches.clear();
        self.shadow_maps_drawn = (0, 0);
        self.shadow_shape = [None; 2];
        let mut stats = FrameStats::default();

        let Some(camera) = scene.camera else {
            self.clear = wgpu::Color::BLACK;
            return stats;
        };
        let [r, g, b] = camera.clear_color.map(f64::from);
        // Linear: the sRGB target encodes it like everything else.
        self.clear = wgpu::Color { r, g, b, a: 1.0 };

        let aspect = self.size.0 as f32 / self.size.1 as f32;
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
        let planes = frustum_planes(projection * camera.view);
        let u = &mut self.uniforms;
        u.view_proj = (projection * camera.view).to_cols_array_2d();
        u.camera_position = to_viewer.into();
        u.ambient = scene.ambient.extend(0.0).into();

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
        let highlights = u32::from(self.quality.lamp_highlights());
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

        // Leave out what the camera can't see, then group the rest by
        // pipeline, textures and mesh, so each run of the same three is one
        // instanced draw call and the state changes least between calls.
        // Each draw's key, normal matrix and lights are worked out once.
        // The sort is in place and the buffers reused, so a steady frame
        // allocates nothing.
        self.order.clear();
        let assets = assets::lock(&self.assets);
        for (i, draw) in scene.draws.iter().enumerate() {
            let Some(mesh) = assets.meshes.get(&draw.mesh) else {
                stats.missing_meshes += 1;
                continue;
            };
            let (at, radius) = bounds(draw.model, mesh.centre, mesh.radius);
            // Out of view, it can still shadow what's in it.
            if draw.cast_shadows {
                self.shadow_candidates.push((at, radius, i as u32));
            }
            if !in_view(&planes, at, radius) {
                stats.culled += 1;
                continue;
            }
            let mirrored = Mat3::from_mat4(draw.model).determinant() < 0.0;
            let m = &draw.material;
            let key = (m.alpha_cutoff > 0.0, Faces::of(m, mirrored), m.textures, draw.mesh.0);
            self.order.push((key, i as u32));
        }
        drop(assets);
        self.order.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        for &((cutout, faces, textures, mesh), i) in &self.order {
            let draw = &scene.draws[i as usize];
            let m = &draw.material;
            let instance = self.instances.len() as u32;
            match self.batches.last_mut() {
                Some(b) if (b.cutout, b.faces, b.textures, b.mesh.0) == (cutout, faces, textures, mesh) => b.count += 1,
                _ => self.batches.push(Batch { cutout, faces, textures, mesh: draw.mesh, first: instance, count: 1 }),
            }
            let [er, eg, eb] = m.emissive;
            let ([su, sv], [ou, ov]) = (m.uv_scale, m.uv_offset);
            self.instances.push(Instance {
                model: {
                    let rows = draw.model.transpose();
                    [rows.x_axis.into(), rows.y_axis.into(), rows.z_axis.into()]
                },
                base_color: m.base_color,
                params: [m.metallic, m.roughness, m.alpha_cutoff, 0.0],
                emissive: [er, eg, eb, if draw.receive_shadows { 1.0 } else { 0.0 }],
                uv: [su, sv, ou, ov],
            });
        }
        for line in &scene.lines {
            for end in [line.a, line.b] {
                self.line_vertices.push(LineVertex { position: end.into(), color: line.color });
            }
        }
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
        stats.lines = scene.lines.len() as u32;
        stats.draws = self.instances.len() as u32;
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
        let level = self.quality.shadows();
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
                        self.caster_order.push((caster_key(c as u8, &scene.draws[i as usize]), i));
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
        let cap = level.spot_maps.min(MAX_SPOT_MAPS);
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
                    self.caster_order.push((caster_key(map as u8, &scene.draws[d as usize]), d));
                }
            }
            spots += 1;
        }
        if spots > 0 {
            self.shadow_maps_drawn.1 = spots as u8;
            self.shadow_shape[1] = Some((level.spot_map_size, spots as u32));
        }

        self.caster_order.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for &(key @ (map, cutout, textures, _), i) in &self.caster_order {
            let draw = &scene.draws[i as usize];
            let m = &draw.material;
            let first = self.casters.len() as u32;
            match self.caster_batches.last_mut() {
                Some(b) if (b.map, b.cutout, b.textures, b.mesh.0) == key => b.count += 1,
                _ => self.caster_batches.push(CasterBatch { map, cutout, textures, mesh: draw.mesh, first, count: 1 }),
            }
            let ([su, sv], [ou, ov]) = (m.uv_scale, m.uv_offset);
            self.casters.push(Caster {
                model: draw.model.to_cols_array_2d(),
                uv: [su, sv, ou, ov],
                alpha: [m.base_color[3], m.alpha_cutoff, 0.0, 0.0],
            });
        }
        stats.shadow_maps = u32::from(self.shadow_maps_drawn.0) + u32::from(self.shadow_maps_drawn.1);
        stats.shadow_casters = self.casters.len() as u32;
    }

    /// The GPU half: writes what [`Renderer::prepare`] built where the GPU
    /// reads it and records the passes into `encoder`, ending in `target`.
    /// The target must be a view in this renderer's format, of the size
    /// given to `prepare`. `encoder` must be submitted before the next
    /// `encode`.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let (width, height) = self.size;
        let image = self.quality.image_format();
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
        if self.shadow_maps.as_ref().is_none_or(|m| m.shape != self.shadow_next) {
            self.shadow_maps = Some(ShadowMaps::new(&self.kit.gpu.device, &self.kit.shadows, self.shadow_next));
        }
        let Self { kit, pipelines, image_pipelines, assets, frame_data, targets, shadow_maps, .. } = self;
        let targets = targets.as_ref().expect("made above");
        let shadow_maps = shadow_maps.as_ref().expect("made above");

        // Bind groups for texture sets not seen before. A steady frame only
        // looks them up. The world's other views wait while this one
        // records; a mesh or texture they free meanwhile is gone from the
        // next frame on.
        let mut assets = assets::lock(assets);
        let Assets { kit: _, meshes, textures } = &mut *assets;
        for batch in &self.batches {
            textures.prepare(&kit.gpu.device, &kit.textures, batch.textures);
        }
        for batch in self.caster_batches.iter().filter(|b| b.cutout) {
            textures.prepare(&kit.gpu.device, &kit.textures, batch.textures);
        }
        let frame = frame_data.write(
            &kit.gpu.device,
            Parts {
                uniforms: bytemuck::bytes_of(&self.uniforms),
                views: &self.shadow_views,
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
            pass.set_bind_group(0, frame.views, &[(u64::from(map) * kit.shadows.view_stride) as u32]);
            pass.set_vertex_buffer(1, frame.buffer.slice(frame.casters..));
            let mut last: Option<&CasterBatch> = None;
            while let Some(batch) = batches.next_if(|b| b.map == map) {
                let Some(mesh) = meshes.get(&batch.mesh) else { continue };
                if batch.cutout {
                    let Some(bind_group) = textures.bind_group(&batch.textures) else { continue };
                    if last.is_none_or(|l| l.textures != batch.textures || !l.cutout) {
                        pass.set_bind_group(1, bind_group, &[]);
                    }
                }
                if last.is_none_or(|l| l.cutout != batch.cutout) {
                    pass.set_pipeline(kit.shadows.pipeline(spot, batch.cutout));
                }
                last = Some(batch);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, batch.first..batch.first + batch.count);
            }
        }

        // 1. The scene, into the scene image or straight into the view.
        let (scene_pipelines, resolve) = match &targets.image {
            Some(image) => (image_pipelines.as_deref().expect("got above"), &image.view),
            None => (&**pipelines, target),
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color,
                    depth_slice: None,
                    resolve_target: Some(resolve),
                    // The multisampled colour is only needed until it's
                    // resolved, so it's never stored. On tile GPUs it never
                    // leaves the chip.
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.clear), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !self.batches.is_empty() || !self.line_vertices.is_empty() {
                pass.set_bind_group(0, frame.bind_group, &[]);
            }
            if !self.batches.is_empty() {
                pass.set_bind_group(2, &shadow_maps.bind_group, &[]);
            }
            if !self.batches.is_empty() {
                pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
            }
            let shadowed = self.shadow_maps_drawn != (0, 0);
            let mut last: Option<&Batch> = None;
            for batch in &self.batches {
                // Freed since prepare, by another view's render worker.
                let (Some(mesh), Some(bind_group)) = (meshes.get(&batch.mesh), textures.bind_group(&batch.textures))
                else {
                    continue;
                };
                let textured = |b: &Batch| b.textures != MaterialTextures::NONE;
                if last.is_none_or(|l| (l.cutout, l.faces, textured(l)) != (batch.cutout, batch.faces, textured(batch)))
                {
                    let (cutout, faces) = (batch.cutout, batch.faces);
                    pass.set_pipeline(scene_pipelines.forward(Variant {
                        cutout,
                        faces,
                        shadowed,
                        textured: textured(batch),
                    }));
                }
                if last.is_none_or(|l| l.textures != batch.textures) {
                    pass.set_bind_group(1, bind_group, &[]);
                }
                last = Some(batch);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, batch.first..batch.first + batch.count);
            }
            if !self.line_vertices.is_empty() {
                pass.set_pipeline(scene_pipelines.lines());
                pass.set_vertex_buffer(0, frame.buffer.slice(frame.lines..));
                pass.draw(0..self.line_vertices.len() as u32, 0..1);
            }
        }

        // 2. The last pass: the scene image onto the view.
        if let Some(image) = &targets.image {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("finish"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            pass.set_pipeline(pipelines.finish());
            pass.set_bind_group(0, &image.bind_group, &[]);
            pass.draw(0..3, 0..1);
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
            SceneImage { format, view, bind_group }
        });
        let color_format = image.as_ref().map_or(self.format, |i| i.format);
        Targets {
            width,
            height,
            color: make("msaa colour", color_format),
            depth: make("msaa depth", DEPTH_FORMAT),
            image,
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
            maps_layout: kit.shadows.maps_layout.clone(),
            image_layout: kit.image_layout.clone(),
            forward_shader: OnceLock::new(),
            forward: Default::default(),
            lines: OnceLock::new(),
            finish: OnceLock::new(),
        }
    }

    /// The forward pipeline for `variant`.
    fn forward(&self, variant: Variant) -> &wgpu::RenderPipeline {
        self.forward[variant.index()].get_or_init(|| self.make_forward(variant))
    }

    fn make_forward(&self, variant: Variant) -> wgpu::RenderPipeline {
        let Variant { cutout, faces, shadowed, textured } = variant;
        let device = &self.device;
        let (shader, layout) = self.forward_shader.get_or_init(|| {
            let shader = device.create_shader_module(wgpu::include_wgsl!("forward.wgsl"));
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("forward"),
                bind_group_layouts: &[Some(&self.frame_layout), Some(&self.textures_layout), Some(&self.maps_layout)],
                immediate_size: 0,
            });
            (shader, layout)
        });
        // Position, normal, tangent and UV, as in `fynkel_mesh::Vertex`.
        let vertex_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2];
        let instance_attributes = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
            7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
        ];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vertex_attributes,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_SIZE,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance_attributes,
            }),
        ];
        // Cut-outs get their own pipelines: a shader that can drop pixels
        // costs tile GPUs their hidden-surface removal, so opaque surfaces
        // don't use it. Its alpha becomes MSAA coverage, and it writes
        // colour only, so the target stays opaque.
        let (front_face, cull_mode) = match faces {
            Faces::Front => (wgpu::FrontFace::Ccw, Some(wgpu::Face::Back)),
            Faces::Mirrored => (wgpu::FrontFace::Cw, Some(wgpu::Face::Back)),
            Faces::Both => (wgpu::FrontFace::Ccw, None),
        };
        let on = |yes: bool| if yes { 1.0 } else { 0.0 };
        let constants =
            [("double_sided", on(faces == Faces::Both)), ("shadowed", on(shadowed)), ("textured", on(textured))];
        let compilation_options = wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() };
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(if cutout { "forward cut-out" } else { "forward" }),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &buffers,
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
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: Renderer::SAMPLES,
                alpha_to_coverage_enabled: cutout,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(if cutout { "fs_cutout" } else { "fs_main" }),
                compilation_options,
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: if cutout { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL },
                })],
            }),
            multiview_mask: None,
            cache: None,
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

    /// The last pass's pipeline, onto a view (of an sRGB format).
    fn finish(&self) -> &wgpu::RenderPipeline {
        assert!(self.format.is_srgb(), "a view's format is sRGB");
        self.finish.get_or_init(|| self.make_finish())
    }

    fn make_finish(&self) -> wgpu::RenderPipeline {
        // The last pass, onto a view.
        let shader = self.device.create_shader_module(wgpu::include_wgsl!("finish.wgsl"));
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("finish"),
            bind_group_layouts: &[Some(&self.image_layout)],
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
                entry_point: Some("fs_main"),
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
/// so they group by mesh alone.
fn caster_key(map: u8, draw: &crate::Draw) -> CasterKey {
    let m = &draw.material;
    let cutout = m.alpha_cutoff > 0.0;
    (map, cutout, if cutout { m.textures } else { MaterialTextures::default() }, draw.mesh.0)
}

/// A draw's sort key: cut out or not, which faces, its textures and its
/// mesh, in the order draws are grouped.
type SortKey = (bool, Faces, MaterialTextures, u64);

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
