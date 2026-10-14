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
//! [`Renderer::MAX_LIGHTS`]; each object then gets the ones of those that
//! reach it, brightest at it first, up to [`Quality::lights_per_object`],
//! and its shader loops over just those.
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

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3, Vec4};

use crate::assets::{self, Assets, Kit, SharedAssets};
use crate::frame_ring::FrameRing;
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
}

/// `Frame` in forward.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    camera_position: [f32; 4],
    ambient: [f32; 4],
    /// x: how many of `suns` are used. y: the one that casts shadows, or
    /// [`NO_SHADOWS`].
    counts: [u32; 4],
    suns: [SunUniform; MAX_SUNS],
    lights: [LightUniform; MAX_LIGHTS],
    shadows: ShadowUniforms,
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
const MAX_LIGHTS: usize = 64;
/// The most lights an object's instance has room for.
const LIGHT_SLOTS: usize = 8;
/// An empty slot in an instance's light list.
const NO_LIGHT: u8 = 0xFF;

/// The per-draw vertex data (instance step), locations 4–14.
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
    /// The point and spot lights that light it: indices into the frame's
    /// list, a byte each, nearest first, [`NO_LIGHT`] after the last.
    lights: [u32; 2],
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

/// A pipeline's index in `Pipelines::forward`.
fn pipeline_index(cutout: bool, faces: Faces) -> usize {
    usize::from(cutout) * Faces::ALL.len() + faces as usize
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
pub(crate) struct Pipelines {
    /// The format they draw into.
    format: wgpu::TextureFormat,
    /// Opaque then cut-out, each for every [`Faces`] (see
    /// [`pipeline_index`]).
    forward: Vec<wgpu::RenderPipeline>,
    lines: wgpu::RenderPipeline,
    /// The last pass, onto a view: for sRGB formats only (a view's).
    finish: Option<wgpu::RenderPipeline>,
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
    /// Each seen draw's lights (as [`Instance::lights`]), by its index in
    /// the scene.
    seen: Vec<[u32; 2]>,
    /// Point and spot lights in view: how far each is from the camera
    /// (squared) and its index in the scene.
    in_view_lights: Vec<(f32, u32)>,
    /// The frame's point and spot lights, in the order of
    /// [`FrameUniforms::lights`].
    frame_lights: Vec<Reach>,
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
            seen: Vec::new(),
            in_view_lights: Vec::new(),
            frame_lights: Vec::new(),
            shadow_candidates: Vec::new(),
            caster_order: Vec::new(),
            casters: Vec::new(),
            caster_batches: Vec::new(),
            shadow_views: vec![0; MAX_MAPS * kit.shadows.view_stride as usize],
            shadow_maps_drawn: (0, 0),
            shadow_shape: [None; 2],
            shadow_maps: None,
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
        assets::lock(&self.assets).upload_mesh(key, data);
    }

    /// Frees the mesh under `key` ([`Assets::remove_mesh`]).
    pub fn remove_mesh(&mut self, key: MeshKey) -> bool {
        assets::lock(&self.assets).remove_mesh(key)
    }

    /// Uploads an image under `key` to its assets ([`Assets::upload_texture`]).
    pub fn upload_texture(&mut self, key: TextureKey, space: ColorSpace, width: u32, height: u32, rgba: &[u8]) {
        assets::lock(&self.assets).upload_texture(key, space, width, height, rgba);
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
        u.counts = [suns as u32, shadow_sun.map_or(NO_SHADOWS, |(i, _)| i as u32), 0, 0];

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
        let per_object = self.quality.lights_per_object();

        // Leave out what the camera can't see, then group the rest by
        // pipeline, textures and mesh, so each run of the same three is one
        // instanced draw call and the state changes least between calls.
        // Each draw's key, normal matrix and lights are worked out once.
        // The sort is in place and the buffers reused, so a steady frame
        // allocates nothing.
        self.order.clear();
        self.seen.resize(scene.draws.len(), [0; 2]);
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
            self.seen[i] = brightest_lights(&self.frame_lights, at, radius, per_object);
            let m = &draw.material;
            let key = (m.alpha_cutoff > 0.0, Faces::of(m, mirrored), m.textures, draw.mesh.0);
            self.order.push((key, i as u32));
        }
        drop(assets);
        self.order.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        for &((cutout, faces, textures, mesh), i) in &self.order {
            let draw = &scene.draws[i as usize];
            let m = &draw.material;
            let lights = self.seen[i as usize];
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
                lights,
            });
        }
        for line in &scene.lines {
            for end in [line.a, line.b] {
                self.line_vertices.push(LineVertex { position: end.into(), color: line.color });
            }
        }
        self.prepare_shadows(scene, &camera, aspect, shadow_sun.map(|(_, direction)| direction), &mut stats);
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
            self.shadow_shape[1] = Some((level.spot_map_size, level.spot_maps.min(MAX_SPOT_MAPS) as u32));
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
        // The shadow maps, remade when a level needs others. A kind of map
        // the frame doesn't use keeps what it had, so a spot light coming
        // into view doesn't make new ones.
        let had = self.shadow_maps.as_ref().map_or([(1, 0); 2], |m| m.shape);
        let shape = [0, 1].map(|k| self.shadow_shape[k].unwrap_or(had[k]));
        if self.shadow_maps.is_none() || shape != had {
            self.shadow_maps = Some(ShadowMaps::new(&self.kit.gpu.device, &self.kit.shadows, shape));
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
            bytemuck::bytes_of(&self.uniforms),
            &self.shadow_views,
            bytemuck::cast_slice(&self.instances),
            bytemuck::cast_slice(&self.line_vertices),
            bytemuck::cast_slice(&self.casters),
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
            let mut last: Option<&Batch> = None;
            for batch in &self.batches {
                // Freed since prepare, by another view's render worker.
                let (Some(mesh), Some(bind_group)) = (meshes.get(&batch.mesh), textures.bind_group(&batch.textures))
                else {
                    continue;
                };
                if last.is_none_or(|l| (l.cutout, l.faces) != (batch.cutout, batch.faces)) {
                    pass.set_pipeline(&scene_pipelines.forward[pipeline_index(batch.cutout, batch.faces)]);
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
                pass.set_pipeline(&scene_pipelines.lines);
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
            pass.set_pipeline(pipelines.finish.as_ref().expect("a view's format is sRGB"));
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
        let device = &kit.gpu.device;
        let frame_layout = &kit.frame_layout;
        let shader = device.create_shader_module(wgpu::include_wgsl!("forward.wgsl"));

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("forward"),
            bind_group_layouts: &[Some(frame_layout), Some(&kit.textures.layout), Some(&kit.shadows.maps_layout)],
            immediate_size: 0,
        });
        // Position, normal, tangent and UV, as in `fynkel_mesh::Vertex`.
        let vertex_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2];
        let instance_attributes = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
            7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
            11 => Uint32x2,
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
        let pipeline = |cutout: bool, faces: Faces| {
            let (front_face, cull_mode) = match faces {
                Faces::Front => (wgpu::FrontFace::Ccw, Some(wgpu::Face::Back)),
                Faces::Mirrored => (wgpu::FrontFace::Cw, Some(wgpu::Face::Back)),
                Faces::Both => (wgpu::FrontFace::Ccw, None),
            };
            let constants = [("double_sided", if faces == Faces::Both { 1.0 } else { 0.0 })];
            let compilation_options = wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if cutout { "forward cut-out" } else { "forward" }),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
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
                    module: &shader,
                    entry_point: Some(if cutout { "fs_cutout" } else { "fs_main" }),
                    compilation_options,
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: if cutout { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL },
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        // Lines: over everything (depth always passes, and isn't written),
        // blended by their alpha.
        let line_shader = device.create_shader_module(wgpu::include_wgsl!("lines.wgsl"));
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lines"),
            bind_group_layouts: &[Some(frame_layout)],
            immediate_size: 0,
        });
        let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let lines = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    // Colour only: the target stays opaque.
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // The last pass, onto a view.
        let finish = format.is_srgb().then(|| {
            let shader = device.create_shader_module(wgpu::include_wgsl!("finish.wgsl"));
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("finish"),
                bind_group_layouts: &[Some(&kit.image_layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });

        Self {
            format,
            finish,
            forward: [false, true]
                .into_iter()
                .flat_map(|cutout| Faces::ALL.map(|faces| (cutout, faces)))
                .map(|(cutout, faces)| pipeline(cutout, faces))
                .collect(),
            lines,
        }
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

/// What a point or spot light reaches, for choosing an object's lights.
#[derive(Clone, Copy)]
struct Reach {
    position: Vec3,
    range: f32,
    /// Its colour's brightest channel.
    peak: f32,
    /// A spot light's axis, and the sine and cosine of half its angle.
    cone: Option<(Vec3, f32, f32)>,
}

impl Reach {
    fn of(light: &crate::Light) -> Self {
        let (position, range, cone) = match light.kind {
            LightKind::Sun { .. } => unreachable!("suns aren't in the frame's list"),
            LightKind::Point { position, range } => (position, range, None),
            LightKind::Spot { position, direction, range, angle, .. } => {
                let half = (angle * 0.5).clamp(0.0, std::f32::consts::FRAC_PI_2);
                (position, range, Some((direction.normalize_or_zero(), half.sin(), half.cos())))
            }
        };
        Self { position, range, peak: light.color.max_element(), cone }
    }

    /// How brightly it lights a sphere at `at` with radius `r`: its
    /// irradiance at the sphere's nearest point (no closer than 10 cm), or
    /// `None` when it doesn't reach it at all.
    fn at(&self, at: Vec3, r: f32) -> Option<f32> {
        let to = at - self.position;
        let gap = to.length() - r;
        if gap >= self.range {
            return None;
        }
        if let Some((axis, sin, cos)) = self.cone {
            // Outside the cone: the sphere's centre is further from the
            // cone's side than its radius (Wronski, "Cull that cone!").
            let along = to.dot(axis);
            let across = (to.length_squared() - along * along).max(0.0).sqrt();
            if cos * across - sin * along > r || along < -r {
                return None;
            }
        }
        Some(self.peak / gap.max(0.1).powi(2))
    }
}

/// The lights of `lights` that reach a sphere at `at` with radius `r`, the
/// brightest at it first, at most `cap`: their indices, a byte each, packed
/// four to a word, [`NO_LIGHT`] after the last. Among lights alike, the
/// brightest are the nearest.
fn brightest_lights(lights: &[Reach], at: Vec3, r: f32, cap: usize) -> [u32; 2] {
    let cap = cap.min(LIGHT_SLOTS);
    let mut best = [(0.0, NO_LIGHT); LIGHT_SLOTS];
    let mut count = 0;
    for (index, light) in lights.iter().enumerate() {
        let Some(bright) = light.at(at, r) else { continue };
        if count == cap && bright <= best[cap - 1].0 {
            continue;
        }
        // Into its place, the dimmest dropping off the end when full.
        let mut k = count.min(cap - 1);
        count = (count + 1).min(cap);
        while k > 0 && best[k - 1].0 < bright {
            best[k] = best[k - 1];
            k -= 1;
        }
        best[k] = (bright, index as u8);
    }
    let byte = |k: usize| u32::from(best[k].1) << (8 * (k % 4));
    [(0..4).map(byte).sum(), (4..8).map(byte).sum()]
}
