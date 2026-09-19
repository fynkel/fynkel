//! `fynkel_render` — draws `fynkel_core`'s render extract.
//!
//! The seed of blueprint §8.5. The renderer is pure Rust and host-agnostic:
//! hosts hand it a device, a target format and an [`Extract`]; where the
//! target comes from (a winit swapchain, an IOSurface ring, an offscreen
//! texture) is the host's business.
//!
//! What it draws (V1 slice):
//! * every `MeshRenderer` in the extract, with its cooked mesh and the
//!   `baseColor` factor + texture of its `fynkel.pbr@1` material, read from
//!   the cooked cache through [`CookedAssets`] on first use;
//! * the extract's camera (`Camera` component, highest priority) and its
//!   first directional light, or the host-set [`Camera`] and light when the
//!   scene has none;
//! * optionally ([`ExtractRenderer::placeholders`]) a lit unit cube for
//!   every spatial entity without a `MeshRenderer` — the M1 look, which the
//!   Dart demo scene still relies on;
//! * a magenta cube where a mesh is missing from the cache, a plain white
//!   surface where a texture or material is.
//!
//! Allocation policy: the instance buffer grows geometrically and is
//! reused; a frame with no growth and no new assets allocates nothing on
//! the CPU side beyond the batch table.

pub mod assets;

use std::collections::HashMap;

use fynkel_contracts::AssetRef;
use fynkel_cooked::payload::{Vertex, TEXTURE_FORMAT_RGBA8_SRGB};
use fynkel_core::{Extract, Projection};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

pub use assets::{CookedAssets, MaterialDesc};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Background colour (linear); shared with the pixel-readback tests.
pub const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.08,
    g: 0.09,
    b: 0.11,
    a: 1.0,
};

/// Illuminance that maps to "fully lit" (a `DirectionalLight` at its
/// default 10 000 lux renders a white surface white). No exposure control
/// yet; this is the one knob.
pub const EXPOSURE_LUX: f32 = 10_000.0;

/// Ambient fraction of the albedo added to every lit surface.
pub const AMBIENT: f32 = 0.18;

/// The tint of a placeholder cube standing in for a mesh the cache does
/// not have.
pub const MISSING_TINT: [f32; 4] = [1.0, 0.0, 1.0, 1.0];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    tint: [f32; 4],
    /// metallic, roughness, unlit, unused
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    light_color: [f32; 4],
    camera_pos: [f32; 4],
}

/// Right-handed, Y-up perspective camera (blueprint §4.5). Used when the
/// extract has no `Camera` component.
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

/// A base colour texture bound for sampling.
struct GpuTexture {
    bind_group: wgpu::BindGroup,
}

/// One draw: a mesh, a texture, a contiguous range of instances.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BatchKey {
    /// `None`: the built-in cube.
    mesh: Option<AssetRef>,
    /// `None`: the built-in white texture.
    texture: Option<AssetRef>,
}

struct Batch {
    key: BatchKey,
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
    pub scene_camera: bool,
    pub scene_light: bool,
}

/// What a frame draws: filled from an [`Extract`] by [`ExtractRenderer::prepare`].
pub struct ExtractRenderer {
    pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    cube: GpuMesh,
    /// The 1×1 white fallback; uploaded on the first `prepare` (needs a queue).
    white: Option<GpuTexture>,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u32,
    instances: Vec<Instance>,
    batches: Vec<Batch>,
    batch_index: HashMap<BatchKey, usize>,
    depth_view: wgpu::TextureView,
    width: u32,
    height: u32,
    assets: Option<CookedAssets>,
    meshes: HashMap<AssetRef, Option<GpuMesh>>,
    textures: HashMap<AssetRef, Option<GpuTexture>>,
    materials: HashMap<AssetRef, Option<MaterialDesc>>,
    asset_errors: Vec<String>,
    stats: FrameStats,
    /// Fallback camera when the extract has none.
    pub camera: Camera,
    /// Fallback direction *toward* the light when the extract has none.
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
        let instance_capacity = 64;
        let instance_buffer = create_instance_buffer(device, instance_capacity);

        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel frame bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fynkel frame bg"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel material bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fynkel mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fynkel mesh pl"),
            bind_group_layouts: &[Some(&frame_layout), Some(&texture_layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fynkel mesh pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
                            10 => Float32x4, 11 => Float32x4
                        ],
                    }),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
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
        });

        let (vertices, indices) = cube_mesh();
        let cube = upload_mesh(device, "fynkel cube", &vertices, &indices);
        let depth_view = create_depth(device, width, height);

        Self {
            pipeline,
            texture_layout,
            sampler,
            cube,
            white: None,
            uniform_buffer,
            bind_group,
            instance_buffer,
            instance_capacity,
            instances: Vec::with_capacity(instance_capacity as usize),
            batches: Vec::new(),
            batch_index: HashMap::new(),
            depth_view,
            width: width.max(1),
            height: height.max(1),
            assets: None,
            meshes: HashMap::new(),
            textures: HashMap::new(),
            materials: HashMap::new(),
            asset_errors: Vec::new(),
            stats: FrameStats::default(),
            camera: Camera::default(),
            light_dir: Vec3::new(0.4, 1.0, 0.6).normalize(),
            placeholders: true,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.depth_view = create_depth(device, self.width, self.height);
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

    /// Where meshes, textures and materials come from. Replacing the
    /// source drops every loaded asset (they are reloaded on demand).
    pub fn set_assets(&mut self, assets: Option<CookedAssets>) {
        self.assets = assets;
        self.clear_assets();
    }

    pub fn assets(&self) -> Option<&CookedAssets> {
        self.assets.as_ref()
    }

    /// Forgets every loaded asset and error; the next `prepare` reloads
    /// what the extract references.
    pub fn clear_assets(&mut self) {
        self.meshes.clear();
        self.textures.clear();
        self.materials.clear();
        self.asset_errors.clear();
    }

    /// One line per asset that failed to load since `clear_assets`
    /// (each asset is tried once).
    pub fn asset_errors(&self) -> &[String] {
        &self.asset_errors
    }

    /// Uploads the camera, the light and the extract's instances. Returns
    /// the number of instances queued for [`draw`](Self::draw).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extract: &Extract,
    ) -> u32 {
        if self.white.is_none() {
            self.white = Some(upload_rgba8(
                device,
                queue,
                1,
                1,
                wgpu::TextureFormat::Rgba8Unorm,
                &[255, 255, 255, 255],
                &self.texture_layout,
                &self.sampler,
                "fynkel white",
            ));
        }
        let aspect = self.width as f32 / self.height as f32;
        let (view_proj, camera_pos, scene_camera) = match &extract.camera {
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
                (proj * view, translation, true)
            }
            None => (
                self.camera.view_proj(self.width, self.height),
                self.camera.eye,
                false,
            ),
        };
        let (light_dir, light_color, scene_light) = match extract.directional_lights.first() {
            Some(l) => (
                (-l.direction).normalize_or(Vec3::Y),
                l.light.color * (l.light.intensity / EXPOSURE_LUX).max(0.0),
                true,
            ),
            None => (self.light_dir, Vec3::ONE, false),
        };
        let u = FrameUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            light_dir: light_dir.extend(0.0).to_array(),
            light_color: light_color.extend(AMBIENT).to_array(),
            camera_pos: camera_pos.extend(1.0).to_array(),
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&u));

        // Group instances by (mesh, texture) in order of first appearance.
        let mut grouped: Vec<Vec<Instance>> = Vec::new();
        self.batch_index.clear();
        let mut stats = FrameStats {
            scene_camera,
            scene_light,
            ..FrameStats::default()
        };
        let mut mesh_of_record: HashMap<u32, usize> = HashMap::with_capacity(extract.meshes.len());
        for (i, m) in extract.meshes.iter().enumerate() {
            mesh_of_record.insert(m.record, i);
        }
        for (ri, r) in extract.records.iter().enumerate() {
            let (key, instance) = match mesh_of_record.get(&(ri as u32)) {
                Some(&mi) => {
                    let m = &extract.meshes[mi];
                    let have_mesh = self.mesh(device, &m.mesh);
                    let material = self.material(&m.material);
                    let texture = material
                        .base_color_texture
                        .clone()
                        .filter(|t| self.texture(device, queue, t));
                    if have_mesh {
                        stats.meshes += 1;
                        (
                            BatchKey {
                                mesh: Some(m.mesh.clone()),
                                texture,
                            },
                            Instance {
                                model: r.world.to_cols_array_2d(),
                                tint: material.base_color,
                                params: [material.metallic, material.roughness, 0.0, 0.0],
                            },
                        )
                    } else {
                        stats.missing += 1;
                        (
                            BatchKey {
                                mesh: None,
                                texture: None,
                            },
                            Instance {
                                model: r.world.to_cols_array_2d(),
                                tint: MISSING_TINT,
                                params: [0.0, 1.0, 1.0, 0.0],
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
                            texture: None,
                        },
                        Instance {
                            model: r.world.to_cols_array_2d(),
                            tint: tint_for(r.entity.slot()),
                            params: [0.0, 1.0, 0.0, 0.0],
                        },
                    )
                }
            };
            let slot = match self.batch_index.get(&key) {
                Some(&s) => s,
                None => {
                    grouped.push(Vec::new());
                    self.batch_index.insert(key, grouped.len() - 1);
                    grouped.len() - 1
                }
            };
            grouped[slot].push(instance);
        }

        self.instances.clear();
        self.batches.clear();
        let mut keys: Vec<(BatchKey, usize)> = self.batch_index.drain().collect();
        keys.sort_by_key(|(_, slot)| *slot);
        for (key, slot) in keys {
            let list = &grouped[slot];
            self.batches.push(Batch {
                key,
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

    /// Records one pass that clears `target` and draws the prepared batches.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel opaque"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR_COLOR),
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
        if self.instances.is_empty() {
            return;
        }
        let Some(white) = self.white.as_ref() else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        for b in &self.batches {
            let mesh = match &b.key.mesh {
                Some(r) => match self.meshes.get(r).and_then(Option::as_ref) {
                    Some(m) => m,
                    None => continue,
                },
                None => &self.cube,
            };
            let texture = match &b.key.texture {
                Some(r) => match self.textures.get(r).and_then(Option::as_ref) {
                    Some(t) => t,
                    None => white,
                },
                None => white,
            };
            pass.set_bind_group(1, &texture.bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, b.first..b.first + b.count);
        }
    }

    // ------------------------------------------------------------ assets

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
                self.asset_errors.push(format!("mesh {e}"));
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
                    &self.texture_layout,
                    &self.sampler,
                    &format!("fynkel texture {r}"),
                ))
            }
            Some(Err(e)) => {
                self.asset_errors.push(format!("texture {e}"));
                None
            }
            None => None,
        };
        let ok = loaded.is_some();
        self.textures.insert(r.clone(), loaded);
        ok
    }

    /// The material's V1 fields, the manifest defaults when it cannot be
    /// read (a nil reference is not an error: "no material").
    fn material(&mut self, r: &AssetRef) -> MaterialDesc {
        if r.is_nil() {
            return MaterialDesc::default();
        }
        if let Some(m) = self.materials.get(r) {
            return m.clone().unwrap_or_default();
        }
        let loaded = match self.assets.as_ref().map(|a| a.material(r)) {
            Some(Ok(m)) => Some(m),
            Some(Err(e)) => {
                self.asset_errors.push(format!("material {e}"));
                None
            }
            None => None,
        };
        let out = loaded.clone().unwrap_or_default();
        self.materials.insert(r.clone(), loaded);
        out
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

fn create_instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel instances"),
        size: capacity as u64 * std::mem::size_of::<Instance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fynkel depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&Default::default())
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

fn bind_texture(
    device: &wgpu::Device,
    texture: &wgpu::Texture,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    label: &str,
) -> GpuTexture {
    let view = texture.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    GpuTexture { bind_group }
}

/// Uploads a top-row-first RGBA8 image and binds it for sampling.
#[allow(clippy::too_many_arguments)]
fn upload_rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    rgba8: &[u8],
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
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
    bind_texture(device, &texture, layout, sampler, label)
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

/// `CLEAR_COLOR` as RGBA8 (Unorm, no sRGB encoding).
pub fn clear_rgba8() -> [u8; 4] {
    [
        (CLEAR_COLOR.r * 255.0).round() as u8,
        (CLEAR_COLOR.g * 255.0).round() as u8,
        (CLEAR_COLOR.b * 255.0).round() as u8,
        255,
    ]
}
