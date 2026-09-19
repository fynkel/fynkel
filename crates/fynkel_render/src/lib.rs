//! `fynkel_render` — draws `fynkel_core`'s render extract.
//!
//! Blueprint §8.5, renderer V1 (a) (§19.2d item 4; `docs/renderer.md` is
//! the reference). The renderer is pure Rust and host-agnostic: hosts hand
//! it a device, a target format and an [`Extract`]; where the target comes
//! from (a winit swapchain, an IOSurface ring, an offscreen texture) is
//! the host's business.
//!
//! What it draws:
//! * every `MeshRenderer` in the extract with its cooked mesh and its
//!   `fynkel.pbr@1` material — Cook-Torrance (GGX, Smith, Schlick) with
//!   every map (base colour, normal, metallic-roughness, occlusion,
//!   emissive), `alphaMode` mask by discard, double-sided materials
//!   without culling — read from the cooked cache through the [`Assets`]
//!   residency store on first use (the store a world loaded its scene
//!   from; GPU copies go when the store evicts);
//! * the extract's camera (`Camera` component, highest priority;
//!   perspective or orthographic), its first `DirectionalLight` as the
//!   sun, the eight nearest `PointLight`/`SpotLight`s, and its
//!   `Environment` (constant ambient, optional equirectangular sky) —
//!   or the host-set [`Camera`] and light when the scene has none;
//! * in physical units into a linear HDR intermediate, exposed by the
//!   camera's EV100 (`Camera.exposure`, or metered from the sun), ACES
//!   tone mapped and sRGB encoded into the host's target;
//! * optionally ([`ExtractRenderer::placeholders`]) a lit unit cube for
//!   every spatial entity without a `MeshRenderer` — the M1 look;
//! * a magenta cube where a mesh is missing from the cache, a plain
//!   surface where a texture or material is.
//!
//! Allocation policy: the instance buffer grows geometrically and is
//! reused; a frame with no growth and no new assets allocates nothing on
//! the CPU side beyond the batch table. Pipelines are four fixed variants
//! created up front; bind groups are cached per material and per texture.

use std::collections::HashMap;

use fynkel_contracts::AssetRef;
use fynkel_cooked::payload::{Vertex, TEXTURE_FORMAT_RGBA8_SRGB};
use fynkel_core::material::AlphaMode;
use fynkel_core::{Environment, Extract, Projection};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

pub mod exposure;

pub use fynkel_assets::{Assets, PbrMaterial};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The linear HDR intermediate every frame is shaded into.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Point + spot lights one draw can see (the nearest to the camera win).
pub const MAX_LIGHTS: usize = 8;

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

/// Alpha cutoff value meaning "no mask" in [`Instance::flags`].
const NO_CUTOFF: f32 = -1.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    /// Base colour factor.
    tint: [f32; 4],
    /// Emissive × strength (relative to the exposure's white), w unused.
    emissive: [f32; 4],
    /// metallic, roughness, normal_scale, occlusion_strength
    params: [f32; 4],
    /// alpha cutoff (or `NO_CUTOFF`), unlit, double-sided, unused
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
    camera_pos: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
    sky: [f32; 4],
    counts: [u32; 4],
    lights: [GpuLight; MAX_LIGHTS],
}

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
}

/// A texture ready for sampling.
struct GpuTexture {
    view: wgpu::TextureView,
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
}

struct Batch {
    key: BatchKey,
    double_sided: bool,
    first: u32,
    count: u32,
}

/// Counts from the last `prepare`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub instances: u32,
    pub meshes: u32,
    pub placeholders: u32,
    pub missing: u32,
    pub batches: u32,
    /// Point + spot lights uploaded (≤ [`MAX_LIGHTS`]).
    pub lights: u32,
    /// Point + spot lights beyond the budget, left out this frame.
    pub lights_dropped: u32,
    pub scene_camera: bool,
    pub scene_light: bool,
    /// The `Environment` sky is drawn behind the meshes.
    pub sky: bool,
}

/// The fixed pipeline set (`docs/renderer.md` lists every variant).
struct Pipelines {
    /// Opaque meshes, back faces culled.
    mesh: wgpu::RenderPipeline,
    /// Opaque meshes, double-sided (no culling, back faces flip the normal).
    mesh_double_sided: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    post: wgpu::RenderPipeline,
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

/// What a frame draws: filled from an [`Extract`] by [`ExtractRenderer::prepare`].
pub struct ExtractRenderer {
    pipelines: Pipelines,
    material_layout: wgpu::BindGroupLayout,
    single_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    cube: GpuMesh,
    fallbacks: Option<Fallbacks>,
    uniform_buffer: wgpu::Buffer,
    post_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u32,
    instances: Vec<Instance>,
    batches: Vec<Batch>,
    batch_index: HashMap<BatchKey, usize>,
    depth_view: wgpu::TextureView,
    hdr_view: wgpu::TextureView,
    post_bind_group: wgpu::BindGroup,
    sky_bind_group: Option<(AssetRef, wgpu::BindGroup)>,
    width: u32,
    height: u32,
    assets: Option<Assets>,
    /// `Assets::evictions` when the GPU caches were last reconciled.
    evictions_seen: u64,
    meshes: HashMap<AssetRef, Option<GpuMesh>>,
    textures: HashMap<AssetRef, Option<GpuTexture>>,
    materials: HashMap<AssetRef, Option<GpuMaterial>>,
    asset_errors: Vec<String>,
    stats: FrameStats,
    ev100: f32,
    /// Fallback camera when the extract has none.
    pub camera: Camera,
    /// Fallback direction *toward* the light when the extract authors no
    /// lighting at all (a white [`exposure::FALLBACK_SUN_LUX`] sun).
    pub light_dir: Vec3,
    /// Draw a lit cube for spatial entities without a `MeshRenderer`
    /// (the M1 behaviour; on by default).
    pub placeholders: bool,
}

impl ExtractRenderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fynkel frame uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
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
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel frame bgl"),
            entries: &[uniform_entry(
                0,
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            )],
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fynkel frame bg"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
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
        // Sky (texture) and post (HDR + params) share one layout.
        let single_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel single-texture bgl"),
            entries: &[
                sampler_entry(0),
                texture_entry(1),
                uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fynkel material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let pipelines = create_pipelines(
            device,
            format,
            &frame_layout,
            &material_layout,
            &single_layout,
        );

        let (vertices, indices) = cube_mesh();
        let cube = upload_mesh(device, "fynkel cube", &vertices, &indices);
        let (width, height) = (width.max(1), height.max(1));
        let depth_view = create_depth(device, width, height);
        let hdr_view = create_hdr(device, width, height);
        let post_bind_group =
            create_post_bind_group(device, &single_layout, &sampler, &hdr_view, &post_buffer);

        Self {
            pipelines,
            material_layout,
            single_layout,
            sampler,
            cube,
            fallbacks: None,
            uniform_buffer,
            post_buffer,
            frame_bind_group,
            instance_buffer,
            instance_capacity,
            instances: Vec::with_capacity(instance_capacity as usize),
            batches: Vec::new(),
            batch_index: HashMap::new(),
            depth_view,
            hdr_view,
            post_bind_group,
            sky_bind_group: None,
            width,
            height,
            assets: None,
            evictions_seen: 0,
            meshes: HashMap::new(),
            textures: HashMap::new(),
            materials: HashMap::new(),
            asset_errors: Vec::new(),
            stats: FrameStats::default(),
            ev100: exposure::ev100_from_lux(exposure::FALLBACK_SUN_LUX),
            camera: Camera::default(),
            light_dir: Vec3::new(0.4, 1.0, 0.6).normalize(),
            placeholders: true,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.depth_view = create_depth(device, self.width, self.height);
        self.hdr_view = create_hdr(device, self.width, self.height);
        self.post_bind_group = create_post_bind_group(
            device,
            &self.single_layout,
            &self.sampler,
            &self.hdr_view,
            &self.post_buffer,
        );
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

    /// The EV100 the last `prepare` exposed the frame at (the camera's
    /// manual value, or metered from the sun and ambient).
    pub fn ev100(&self) -> f32 {
        self.ev100
    }

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
        let now = assets.evictions();
        if now == self.evictions_seen {
            return;
        }
        self.evictions_seen = now;
        let a = assets.clone();
        self.meshes.retain(|r, _| a.is_resident(r.asset));
        self.textures.retain(|r, _| a.is_resident(r.asset));
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
        self.asset_errors.clear();
    }

    /// Forgets every loaded asset and error; the next `prepare` reloads
    /// what the extract references.
    pub fn clear_assets(&mut self) {
        self.meshes.clear();
        self.textures.clear();
        self.materials.clear();
        self.sky_bind_group = None;
        self.asset_errors.clear();
    }

    /// One line per asset that failed to load since `clear_assets`
    /// (each asset is tried once).
    pub fn asset_errors(&self) -> &[String] {
        &self.asset_errors
    }

    /// Uploads the camera, the lights, the environment and the extract's
    /// instances. Returns the number of instances queued for
    /// [`draw`](Self::draw).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extract: &Extract,
    ) -> u32 {
        self.reconcile_evictions();
        self.ensure_fallbacks(device, queue);
        let mut stats = FrameStats::default();

        // Camera.
        let aspect = self.width as f32 / self.height as f32;
        let (view_proj, camera_pos, component) = match &extract.camera {
            Some(c) => {
                let (_, rotation, translation) = c.world.to_scale_rotation_translation();
                let view = Mat4::from_rotation_translation(rotation, translation).inverse();
                let proj = match c.camera.projection {
                    Projection::Perspective => glam::camera::rh::proj::directx::perspective(
                        c.camera.fov_y,
                        aspect,
                        c.camera.near,
                        c.camera.far,
                    ),
                    Projection::Orthographic => {
                        let h = c.camera.ortho_height * 0.5;
                        let w = h * aspect;
                        glam::camera::rh::proj::directx::orthographic(
                            -w,
                            w,
                            -h,
                            h,
                            c.camera.near,
                            c.camera.far,
                        )
                    }
                };
                stats.scene_camera = true;
                (proj * view, translation, Some(&c.camera))
            }
            None => (
                self.camera.view_proj(self.width, self.height),
                self.camera.eye,
                None,
            ),
        };

        // Sun, ambient, sky. A scene that authors no lighting at all (no
        // light, no Environment) gets the host's fallback sun.
        let authored_lighting = !extract.directional_lights.is_empty()
            || !extract.point_lights.is_empty()
            || !extract.spot_lights.is_empty()
            || extract.environment.is_some();
        let (sun_dir, sun_color, sun_lux, sun_present) = match extract.directional_lights.first() {
            Some(l) => {
                stats.scene_light = true;
                let lux = l.light.intensity.max(0.0);
                (
                    (-l.direction).normalize_or(Vec3::Y),
                    l.light.color.max(Vec3::ZERO) * lux,
                    lux,
                    true,
                )
            }
            None if !authored_lighting => (
                self.light_dir,
                Vec3::splat(exposure::FALLBACK_SUN_LUX),
                exposure::FALLBACK_SUN_LUX,
                true,
            ),
            None => (Vec3::Y, Vec3::ZERO, 0.0, false),
        };
        let default_env = Environment::default();
        let env = extract
            .environment
            .as_ref()
            .map_or(&default_env, |e| &e.environment);
        let ambient_lux = env.ambient_intensity.max(0.0);
        let ambient = env.ambient_color.max(Vec3::ZERO) * ambient_lux;
        let ev100 = exposure::ev100_for(component, sun_lux, ambient_lux);
        self.ev100 = ev100;
        let white = exposure::white_nits(ev100);
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

        // Point and spot lights: the nearest MAX_LIGHTS to the camera.
        let mut lights: Vec<(f32, GpuLight)> =
            Vec::with_capacity(extract.point_lights.len() + extract.spot_lights.len());
        for l in &extract.point_lights {
            lights.push((
                l.position.distance_squared(camera_pos),
                GpuLight {
                    position: l.position.extend(l.light.range.max(1e-3)).to_array(),
                    color: (l.light.color.max(Vec3::ZERO) * l.light.intensity.max(0.0))
                        .extend(LIGHT_POINT)
                        .to_array(),
                    ..GpuLight::default()
                },
            ));
        }
        for l in &extract.spot_lights {
            let outer = l.light.outer_angle.clamp(1e-3, std::f32::consts::PI - 1e-3);
            let inner = l.light.inner_angle.clamp(0.0, outer);
            let cos_outer = outer.cos();
            // `smoothstep` needs distinct edges.
            let cos_inner = inner.cos().max(cos_outer + 1e-4);
            lights.push((
                l.position.distance_squared(camera_pos),
                GpuLight {
                    position: l.position.extend(l.light.range.max(1e-3)).to_array(),
                    color: (l.light.color.max(Vec3::ZERO) * l.light.intensity.max(0.0))
                        .extend(LIGHT_SPOT)
                        .to_array(),
                    direction: l
                        .direction
                        .normalize_or(Vec3::NEG_Z)
                        .extend(cos_outer)
                        .to_array(),
                    params: [cos_inner, 0.0, 0.0, 0.0],
                },
            ));
        }
        lights.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut packed = [GpuLight::default(); MAX_LIGHTS];
        let n_lights = lights.len().min(MAX_LIGHTS);
        for (slot, (_, l)) in packed.iter_mut().zip(lights.iter()) {
            *slot = *l;
        }
        stats.lights = n_lights as u32;
        stats.lights_dropped = (lights.len() - n_lights) as u32;

        let u = FrameUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera_pos: camera_pos
                .extend(exposure::exposure_factor(ev100))
                .to_array(),
            sun_dir: sun_dir.extend(sun_present as u32 as f32).to_array(),
            sun_color: sun_color.extend(0.0).to_array(),
            ambient: ambient.extend(white).to_array(),
            sky: [env.sky_intensity.max(0.0), env.sky_rotation, 0.0, 0.0],
            counts: [
                n_lights as u32,
                stats.sky as u32,
                self.pipelines.encode_srgb as u32,
                0,
            ],
            lights: packed,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&u));

        // Group instances by (mesh, material) in order of first appearance.
        let mut grouped: Vec<(bool, Vec<Instance>)> = Vec::new();
        self.batch_index.clear();
        let mut mesh_of_record: HashMap<u32, usize> = HashMap::with_capacity(extract.meshes.len());
        for (i, m) in extract.meshes.iter().enumerate() {
            mesh_of_record.insert(m.record, i);
        }
        for (ri, r) in extract.records.iter().enumerate() {
            let (key, double_sided, instance) = match mesh_of_record.get(&(ri as u32)) {
                Some(&mi) => {
                    let m = &extract.meshes[mi];
                    let have_mesh = self.mesh(device, &m.mesh);
                    let (material_key, material) = self.material(device, queue, &m.material);
                    if have_mesh {
                        stats.meshes += 1;
                        (
                            BatchKey {
                                mesh: Some(m.mesh.clone()),
                                material: material_key,
                            },
                            material.double_sided,
                            instance_for(&material, r.world),
                        )
                    } else {
                        stats.missing += 1;
                        (
                            BatchKey {
                                mesh: None,
                                material: None,
                            },
                            false,
                            Instance {
                                model: r.world.to_cols_array_2d(),
                                tint: MISSING_TINT,
                                emissive: [0.0; 4],
                                params: [0.0, 1.0, 1.0, 1.0],
                                flags: [NO_CUTOFF, 1.0, 0.0, 0.0],
                            },
                        )
                    }
                }
                None => {
                    if !self.placeholders {
                        continue;
                    }
                    stats.placeholders += 1;
                    (
                        BatchKey {
                            mesh: None,
                            material: None,
                        },
                        false,
                        Instance {
                            model: r.world.to_cols_array_2d(),
                            tint: tint_for(r.entity.slot()),
                            emissive: [0.0; 4],
                            params: [0.0, 1.0, 1.0, 1.0],
                            flags: [NO_CUTOFF, 0.0, 0.0, 0.0],
                        },
                    )
                }
            };
            let slot = match self.batch_index.get(&key) {
                Some(&s) => s,
                None => {
                    grouped.push((double_sided, Vec::new()));
                    self.batch_index.insert(key, grouped.len() - 1);
                    grouped.len() - 1
                }
            };
            grouped[slot].1.push(instance);
        }

        self.instances.clear();
        self.batches.clear();
        let mut keys: Vec<(BatchKey, usize)> = self.batch_index.drain().collect();
        keys.sort_by_key(|(_, slot)| *slot);
        for (key, slot) in keys {
            let (double_sided, list) = &grouped[slot];
            self.batches.push(Batch {
                key,
                double_sided: *double_sided,
                first: self.instances.len() as u32,
                count: list.len() as u32,
            });
            self.instances.extend_from_slice(list);
        }
        let n = self.instances.len() as u32;
        if n > self.instance_capacity {
            self.instance_capacity = n.next_power_of_two().max(self.instance_capacity * 2);
            self.instance_buffer = create_instance_buffer(device, self.instance_capacity);
        }
        if n > 0 {
            queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&self.instances),
            );
        }
        stats.instances = n;
        stats.batches = self.batches.len() as u32;
        self.stats = stats;
        n
    }

    /// Records the frame: the HDR pass (meshes, then the sky behind them)
    /// and the post pass into `target`.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fynkel hdr"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.hdr_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            self.draw_meshes(&mut pass);
            if let Some((_, sky)) = &self.sky_bind_group {
                pass.set_pipeline(&self.pipelines.sky);
                pass.set_bind_group(1, sky, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel post"),
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
        pass.set_pipeline(&self.pipelines.post);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, &self.post_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn draw_meshes(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.instances.is_empty() {
            return;
        }
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        // Culled batches first, then the double-sided ones: one pipeline
        // switch per frame.
        for double_sided in [false, true] {
            let mut bound = false;
            for b in self
                .batches
                .iter()
                .filter(|b| b.double_sided == double_sided)
            {
                if !bound {
                    pass.set_pipeline(if double_sided {
                        &self.pipelines.mesh_double_sided
                    } else {
                        &self.pipelines.mesh
                    });
                    bound = true;
                }
                let mesh = match &b.key.mesh {
                    Some(r) => match self.meshes.get(r).and_then(Option::as_ref) {
                        Some(m) => m,
                        None => continue,
                    },
                    None => &self.cube,
                };
                let material = match &b.key.material {
                    Some(r) => match self.materials.get(r).and_then(Option::as_ref) {
                        Some(m) => &m.bind_group,
                        None => &fallbacks.default_material,
                    },
                    None => &fallbacks.default_material,
                };
                pass.set_bind_group(1, material, &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, b.first..b.first + b.count);
            }
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
            1,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
            &[255, 255, 255, 255],
            "fynkel white",
        );
        let flat_normal = upload_rgba8(
            device,
            queue,
            1,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
            &[128, 128, 255, 255],
            "fynkel flat normal",
        );
        let default_material = create_material_bind_group(
            device,
            &self.material_layout,
            &self.sampler,
            [&white, &flat_normal, &white, &white, &white].map(|t| &t.view),
            "fynkel default material",
        );
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
                let format = if payload.format == TEXTURE_FORMAT_RGBA8_SRGB {
                    wgpu::TextureFormat::Rgba8UnormSrgb
                } else {
                    wgpu::TextureFormat::Rgba8Unorm
                };
                Some(upload_rgba8(
                    device,
                    queue,
                    payload.width,
                    payload.height,
                    format,
                    &payload.rgba8,
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

/// The per-instance data for a material at `world`.
fn instance_for(m: &PbrMaterial, world: Mat4) -> Instance {
    let cutoff = match m.alpha_mode {
        AlphaMode::Mask => m.alpha_cutoff.clamp(0.0, 1.0),
        // Blend is drawn opaque until renderer V1 (b) (§19.2d item 5).
        AlphaMode::Opaque | AlphaMode::Blend => NO_CUTOFF,
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
        flags: [cutoff, 0.0, if m.double_sided { 1.0 } else { 0.0 }, 0.0],
    }
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
    const fn count() -> usize {
        4
    }
}

/// The four pipeline variants (`docs/renderer.md`): mesh (cull back),
/// mesh double-sided (cull none), sky, post. Nothing else is ever created.
fn create_pipelines(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    frame_layout: &wgpu::BindGroupLayout,
    material_layout: &wgpu::BindGroupLayout,
    single_layout: &wgpu::BindGroupLayout,
) -> Pipelines {
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

    let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel mesh pl"),
        bind_group_layouts: &[Some(frame_layout), Some(material_layout)],
        ..Default::default()
    });
    let single_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel single-texture pl"),
        bind_group_layouts: &[Some(frame_layout), Some(single_layout)],
        ..Default::default()
    });
    let hdr_target = [Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let mesh = |label, cull_mode| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
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
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &hdr_target,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let fullscreen =
        |label, shader: &wgpu::ShaderModule, targets: &[Option<wgpu::ColorTargetState>], depth| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&single_pl),
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
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
    let sky = fullscreen(
        "fynkel sky pipeline",
        &sky_shader,
        &hdr_target,
        Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
    );
    let post = fullscreen(
        "fynkel post pipeline",
        &post_shader,
        &[Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALL,
        })],
        None,
    );
    Pipelines {
        mesh: mesh("fynkel mesh pipeline", Some(wgpu::Face::Back)),
        mesh_double_sided: mesh("fynkel mesh pipeline (double-sided)", None),
        sky,
        post,
        encode_srgb: !format.is_srgb(),
    }
}

fn create_instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel instances"),
        size: capacity as u64 * std::mem::size_of::<Instance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_attachment(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
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
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    tex.create_view(&Default::default())
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    create_attachment(
        device,
        "fynkel depth",
        width,
        height,
        DEPTH_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    )
}

fn create_hdr(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    create_attachment(
        device,
        "fynkel hdr",
        width,
        height,
        HDR_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    )
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

fn create_post_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    hdr: &wgpu::TextureView,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    create_single_bind_group(device, layout, sampler, hdr, params, "fynkel post bg")
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
    GpuMesh {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
    }
}

/// Uploads a top-row-first RGBA8 image.
fn upload_rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    rgba8: &[u8],
    label: &str,
) -> GpuTexture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    GpuTexture {
        view: texture.create_view(&Default::default()),
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
        required_limits: device_limits(&adapter),
        ..Default::default()
    }))
    .ok()
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
