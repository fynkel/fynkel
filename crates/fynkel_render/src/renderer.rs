//! The forward renderer: one pass, 4× MSAA, reversed-Z.
//!
//! A frame has two halves. [`Renderer::prepare`] is plain CPU work: it
//! turns the scene into per-draw instance data and the frame uniforms, in
//! buffers it reuses, so a steady frame allocates nothing.
//! [`Renderer::encode`] writes that where the GPU reads it (see
//! `frame_ring`) and records the pass.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};

use crate::frame_ring::FrameRing;
use crate::gpu::Gpu;
use crate::scene::{MeshKey, RenderScene};
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
}

/// `Frame` in forward.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    camera_position: [f32; 4],
    to_sun: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
}

/// The per-draw vertex data (instance step), locations 2–10.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    normal: [[f32; 3]; 3],
    base_color: [f32; 4],
    params: [f32; 4],
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

/// The multisampled colour and depth targets, remade when the size changes.
struct Targets {
    width: u32,
    height: u32,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
}

/// One instanced draw call, as `prepare` left it for `encode`: `count`
/// instances of `mesh` starting at instance `first`.
#[derive(Clone, Copy)]
struct Batch {
    mesh: MeshKey,
    first: u32,
    count: u32,
}

/// Draws [`RenderScene`]s into textures of one format.
pub struct Renderer {
    gpu: Gpu,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    frame_data: FrameRing,
    meshes: HashMap<MeshKey, GpuMesh>,
    targets: Option<Targets>,

    // Built by `prepare`, consumed by `encode`. Reused every frame.
    uniforms: FrameUniforms,
    instances: Vec<Instance>,
    batches: Vec<Batch>,
    /// Indices into the scene's draws, sorted by mesh.
    order: Vec<u32>,
    clear: wgpu::Color,
    size: (u32, u32),
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("format", &self.format)
            .field("meshes", &self.meshes.len())
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const INSTANCE_SIZE: u64 = std::mem::size_of::<Instance>() as u64;

impl Renderer {
    /// MSAA sample count.
    pub const SAMPLES: u32 = 4;

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
        assert!(format.is_srgb(), "the renderer draws into sRGB views, got {format:?}");
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("forward.wgsl"));

        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<FrameUniforms>() as u64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("forward"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        // Position, normal, tangent and UV, as in `fynkel_mesh::Vertex`.
        let vertex_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2];
        let instance_attributes = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x3, 9 => Float32x3, 10 => Float32x3,
            11 => Float32x4, 12 => Float32x4,
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("forward"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
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
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
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
            multisample: wgpu::MultisampleState { count: Self::SAMPLES, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            gpu: gpu.clone(),
            format,
            pipeline,
            frame_data: FrameRing::new(
                device,
                frame_layout,
                std::mem::size_of::<FrameUniforms>() as u64,
                64 * INSTANCE_SIZE,
            ),
            meshes: HashMap::new(),
            targets: None,
            uniforms: FrameUniforms::zeroed(),
            instances: Vec::new(),
            batches: Vec::new(),
            order: Vec::new(),
            clear: wgpu::Color::BLACK,
            size: (0, 0),
        }
    }

    /// The target format this renderer draws into.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Uploads `data` under `key`, replacing any mesh already there.
    pub fn upload_mesh(&mut self, key: MeshKey, data: &MeshData) {
        let device = &self.gpu.device;
        let vertices =
            filled_buffer(device, "mesh vertices", wgpu::BufferUsages::VERTEX, bytemuck::cast_slice(data.vertices()));
        let indices =
            filled_buffer(device, "mesh indices", wgpu::BufferUsages::INDEX, bytemuck::cast_slice(data.indices()));
        self.meshes.insert(key, GpuMesh { vertices, indices, index_count: data.indices().len() as u32 });
    }

    /// Frees the mesh under `key`. Returns whether there was one.
    pub fn remove_mesh(&mut self, key: MeshKey) -> bool {
        self.meshes.remove(&key).is_some()
    }

    /// The CPU half of a frame: builds per-draw data and the frame uniforms
    /// for a `width` × `height` target. Makes no GPU calls, and allocates
    /// nothing once its buffers have grown to the scene's size.
    pub fn prepare(&mut self, scene: &RenderScene, width: u32, height: u32) -> FrameStats {
        self.size = (width.max(1), height.max(1));
        self.instances.clear();
        self.batches.clear();
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
        let projection = glam::camera::rh::proj::directx::perspective(camera.fov_y, aspect, camera.far, camera.near);
        let to_sun = scene.sun.map(|s| (-s.direction).normalize_or_zero()).unwrap_or(Vec3::ZERO);
        self.uniforms = FrameUniforms {
            view_proj: (projection * camera.view).to_cols_array_2d(),
            camera_position: camera.position.extend(1.0).into(),
            to_sun: to_sun.extend(if to_sun == Vec3::ZERO { 0.0 } else { 1.0 }).into(),
            sun_color: scene.sun.map_or(Vec3::ZERO, |s| s.color).extend(0.0).into(),
            ambient: scene.ambient.extend(0.0).into(),
        };

        // Group draws by mesh, so each mesh is one instanced draw call. The
        // sort is in place and the order is reused, so this allocates
        // nothing in a steady frame.
        self.order.clear();
        self.order.extend(0..scene.draws.len() as u32);
        self.order.sort_unstable_by_key(|&i| scene.draws[i as usize].mesh.0);

        for &i in &self.order {
            let draw = &scene.draws[i as usize];
            if !self.meshes.contains_key(&draw.mesh) {
                stats.missing_meshes += 1;
                continue;
            }
            let instance = self.instances.len() as u32;
            match self.batches.last_mut() {
                Some(batch) if batch.mesh == draw.mesh => batch.count += 1,
                _ => self.batches.push(Batch { mesh: draw.mesh, first: instance, count: 1 }),
            }
            self.instances.push(Instance {
                model: draw.model.to_cols_array_2d(),
                normal: normal_matrix(draw.model).to_cols_array_2d(),
                base_color: draw.material.base_color,
                params: [draw.material.metallic, draw.material.roughness, 0.0, 0.0],
            });
        }
        stats.draws = self.instances.len() as u32;
        stats.draw_calls = self.batches.len() as u32;
        stats
    }

    /// The GPU half: writes what [`Renderer::prepare`] built where the GPU
    /// reads it and records the pass into `encoder`, resolving into
    /// `target`. The target must be a view in this renderer's format, of
    /// the size given to `prepare`. `encoder` must be submitted before the
    /// next `encode`.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let (width, height) = self.size;
        if self.targets.as_ref().is_none_or(|t| (t.width, t.height) != (width, height)) {
            self.targets = Some(self.create_targets(width, height));
        }
        let frame = self.frame_data.write(
            &self.gpu.device,
            bytemuck::bytes_of(&self.uniforms),
            bytemuck::cast_slice(&self.instances),
        );

        let targets = self.targets.as_ref().expect("made above");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("forward"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.color,
                depth_slice: None,
                resolve_target: Some(target),
                // The multisampled colour is only needed until it's resolved,
                // so it's never stored. On tile GPUs it never leaves the chip.
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
        if self.batches.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame.bind_group, &[]);
        pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..));
        for batch in &self.batches {
            let mesh = &self.meshes[&batch.mesh];
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, batch.first..batch.first + batch.count);
        }
    }

    fn create_targets(&self, width: u32, height: u32) -> Targets {
        let make = |label, format| {
            self.gpu
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: Self::SAMPLES,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Targets { width, height, color: make("msaa colour", self.format), depth: make("msaa depth", DEPTH_FORMAT) }
    }

    /// `prepare` + `encode` + submit, for callers that don't need the split.
    pub fn render(&mut self, scene: &RenderScene, target: &wgpu::TextureView, width: u32, height: u32) -> FrameStats {
        let stats = self.prepare(scene, width, height);
        let mut encoder =
            self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        self.encode(&mut encoder, target);
        self.gpu.queue.submit([encoder.finish()]);
        stats
    }
}

/// A buffer holding `contents`, written by the CPU where the GPU reads it:
/// no staging copy, so no blit (see `frame_ring`).
fn filled_buffer(device: &wgpu::Device, label: &str, usage: wgpu::BufferUsages, contents: &[u8]) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (contents.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(wgpu::COPY_BUFFER_ALIGNMENT),
        usage: usage | wgpu::BufferUsages::MAP_WRITE,
        mapped_at_creation: true,
    });
    buffer.get_mapped_range_mut(..).expect("mapped at creation").slice(..contents.len()).copy_from_slice(contents);
    buffer.unmap();
    buffer
}

/// The inverse-transpose of the upper 3×3: keeps normals perpendicular to
/// their surface under non-uniform scale. Rotation-only matrices give
/// themselves back. A degenerate (zero-scale) matrix gives zero, so the
/// object draws unlit instead of NaN.
fn normal_matrix(model: Mat4) -> Mat3 {
    let m = Mat3::from_mat4(model);
    let det = m.determinant();
    if det.abs() < 1e-12 { Mat3::ZERO } else { m.inverse().transpose() }
}
