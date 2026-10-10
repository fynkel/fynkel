//! The forward renderer: one pass, 4× MSAA, reversed-Z.
//!
//! A frame has two halves. [`Renderer::prepare`] is plain CPU work: it
//! turns the scene into per-draw instance data and the frame uniforms, in
//! buffers it reuses, so a steady frame allocates nothing.
//! [`Renderer::encode`] writes that where the GPU reads it (see
//! `frame_ring`) and records the pass. Debug lines come last, on top.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};

use crate::frame_ring::FrameRing;
use crate::gpu::Gpu;
use crate::scene::{ColorSpace, MaterialTextures, MeshKey, RenderScene, TextureKey};
use crate::texture::Textures;
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
    /// Debug lines drawn.
    pub lines: u32,
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

/// The per-draw vertex data (instance step), locations 4–14.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    normal: [[f32; 3]; 3],
    base_color: [f32; 4],
    /// Metallic, roughness, alpha cutoff, and -1 when the model mirrors.
    params: [f32; 4],
    /// Emission (w unused).
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

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl GpuMesh {
    fn bytes(&self) -> u64 {
        self.vertices.size() + self.indices.size()
    }
}

/// The multisampled colour and depth targets, remade when the size changes.
struct Targets {
    width: u32,
    height: u32,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
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

/// A pipeline's index in `Renderer::pipelines`.
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

/// Draws [`RenderScene`]s into textures of one format.
pub struct Renderer {
    gpu: Gpu,
    format: wgpu::TextureFormat,
    /// Opaque then cut-out, each for every [`Faces`] (see
    /// [`pipeline_index`]).
    pipelines: Vec<wgpu::RenderPipeline>,
    lines: wgpu::RenderPipeline,
    frame_data: FrameRing,
    meshes: HashMap<MeshKey, GpuMesh>,
    textures: Textures,
    targets: Option<Targets>,

    // Built by `prepare`, consumed by `encode`. Reused every frame.
    uniforms: FrameUniforms,
    instances: Vec<Instance>,
    batches: Vec<Batch>,
    line_vertices: Vec<LineVertex>,
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
            .field("texture_bytes", &self.textures.bytes())
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
        let textures = Textures::new(gpu);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("forward"),
            bind_group_layouts: &[Some(&frame_layout), Some(&textures.layout)],
            immediate_size: 0,
        });
        // Position, normal, tangent and UV, as in `fynkel_mesh::Vertex`.
        let vertex_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2];
        let instance_attributes = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x3, 9 => Float32x3, 10 => Float32x3,
            11 => Float32x4, 12 => Float32x4, 13 => Float32x4, 14 => Float32x4,
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
                    count: Self::SAMPLES,
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
            bind_group_layouts: &[Some(&frame_layout)],
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
            multisample: wgpu::MultisampleState { count: Self::SAMPLES, ..Default::default() },
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

        Self {
            gpu: gpu.clone(),
            lines,
            format,
            pipelines: [false, true]
                .into_iter()
                .flat_map(|cutout| Faces::ALL.map(|faces| (cutout, faces)))
                .map(|(cutout, faces)| pipeline(cutout, faces))
                .collect(),
            frame_data: FrameRing::new(
                device,
                frame_layout,
                std::mem::size_of::<FrameUniforms>() as u64,
                64 * INSTANCE_SIZE,
            ),
            meshes: HashMap::new(),
            textures,
            targets: None,
            uniforms: FrameUniforms::zeroed(),
            instances: Vec::new(),
            batches: Vec::new(),
            line_vertices: Vec::new(),
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

    /// Uploads an image under `key`, to be read in `space`, replacing any
    /// texture there in that space. RGBA8, rows from the top. The full mip
    /// chain is drawn on the GPU, averaging in `space`. Draws whose
    /// material names a slot of that space use it from the next frame.
    ///
    /// The image becomes one texture per space it's uploaded in; a draw
    /// whose texture isn't uploaded in its slot's space uses a texture that
    /// changes nothing (white, or a flat normal).
    ///
    /// # Panics
    ///
    /// If `rgba` isn't `width × height × 4` bytes, or a side is 0 or more
    /// than 8192.
    pub fn upload_texture(&mut self, key: TextureKey, space: ColorSpace, width: u32, height: u32, rgba: &[u8]) {
        self.textures.upload(key, space, width, height, rgba);
    }

    /// Frees the textures under `key`, in both spaces. Returns whether
    /// there were any.
    pub fn remove_texture(&mut self, key: TextureKey) -> bool {
        self.textures.remove(key)
    }

    /// GPU memory held by textures, in bytes: every mip level, as stored.
    pub fn texture_bytes(&self) -> u64 {
        self.textures.bytes()
    }

    /// GPU memory held by meshes' vertex and index buffers, in bytes.
    pub fn mesh_bytes(&self) -> u64 {
        self.meshes.values().map(GpuMesh::bytes).sum()
    }

    /// The CPU half of a frame: builds per-draw data and the frame uniforms
    /// for a `width` × `height` target. Makes no GPU calls, and allocates
    /// nothing once its buffers have grown to the scene's size.
    pub fn prepare(&mut self, scene: &RenderScene, width: u32, height: u32) -> FrameStats {
        self.size = (width.max(1), height.max(1));
        self.instances.clear();
        self.batches.clear();
        self.line_vertices.clear();
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
        let to_sun = scene.sun.map(|s| (-s.direction).normalize_or_zero()).unwrap_or(Vec3::ZERO);
        self.uniforms = FrameUniforms {
            view_proj: (projection * camera.view).to_cols_array_2d(),
            camera_position: to_viewer.into(),
            to_sun: to_sun.extend(if to_sun == Vec3::ZERO { 0.0 } else { 1.0 }).into(),
            sun_color: scene.sun.map_or(Vec3::ZERO, |s| s.color).extend(0.0).into(),
            ambient: scene.ambient.extend(0.0).into(),
        };

        // Group draws by pipeline, then textures, then mesh, so each run
        // of the same three is one instanced draw call and the state
        // changes least between calls. The sort is in place and the order
        // is reused, so this allocates nothing in a steady frame.
        self.order.clear();
        self.order.extend(0..scene.draws.len() as u32);
        self.order.sort_unstable_by_key(|&i| {
            let draw = &scene.draws[i as usize];
            let faces = Faces::of(&draw.material, mirrors(draw.model));
            (draw.material.alpha_cutoff > 0.0, faces, draw.material.textures, draw.mesh.0)
        });

        for &i in &self.order {
            let draw = &scene.draws[i as usize];
            if !self.meshes.contains_key(&draw.mesh) {
                stats.missing_meshes += 1;
                continue;
            }
            let m = &draw.material;
            let cutout = m.alpha_cutoff > 0.0;
            let (normal, mirrored) = normal_matrix(draw.model);
            let faces = Faces::of(m, mirrored);
            let instance = self.instances.len() as u32;
            match self.batches.last_mut() {
                Some(b) if (b.cutout, b.faces, b.textures, b.mesh) == (cutout, faces, m.textures, draw.mesh) => {
                    b.count += 1
                }
                _ => self.batches.push(Batch {
                    cutout,
                    faces,
                    textures: m.textures,
                    mesh: draw.mesh,
                    first: instance,
                    count: 1,
                }),
            }
            let [er, eg, eb] = m.emissive;
            let ([su, sv], [ou, ov]) = (m.uv_scale, m.uv_offset);
            self.instances.push(Instance {
                model: draw.model.to_cols_array_2d(),
                normal: normal.to_cols_array_2d(),
                base_color: m.base_color,
                params: [m.metallic, m.roughness, m.alpha_cutoff, if mirrored { -1.0 } else { 1.0 }],
                emissive: [er, eg, eb, 0.0],
                uv: [su, sv, ou, ov],
            });
        }
        for line in &scene.lines {
            for end in [line.a, line.b] {
                self.line_vertices.push(LineVertex { position: end.into(), color: line.color });
            }
        }
        stats.lines = scene.lines.len() as u32;
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
        // Bind groups for texture sets not seen before. A steady frame only
        // looks them up.
        for batch in &self.batches {
            self.textures.prepare(batch.textures);
        }
        let frame = self.frame_data.write(
            &self.gpu.device,
            bytemuck::bytes_of(&self.uniforms),
            bytemuck::cast_slice(&self.instances),
            bytemuck::cast_slice(&self.line_vertices),
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
        if self.batches.is_empty() && self.line_vertices.is_empty() {
            return;
        }
        pass.set_bind_group(0, frame.bind_group, &[]);
        if !self.batches.is_empty() {
            pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
        }
        let mut last: Option<&Batch> = None;
        for batch in &self.batches {
            if last.is_none_or(|l| (l.cutout, l.faces) != (batch.cutout, batch.faces)) {
                pass.set_pipeline(&self.pipelines[pipeline_index(batch.cutout, batch.faces)]);
            }
            if last.is_none_or(|l| l.textures != batch.textures) {
                pass.set_bind_group(1, self.textures.bind_group(&batch.textures), &[]);
            }
            last = Some(batch);
            let mesh = &self.meshes[&batch.mesh];
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, batch.first..batch.first + batch.count);
        }
        if !self.line_vertices.is_empty() {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, frame.buffer.slice(frame.lines..));
            pass.draw(0..self.line_vertices.len() as u32, 0..1);
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
/// object draws unlit instead of NaN. Also whether the matrix mirrors (a
/// negative determinant), which flips the bitangent.
fn normal_matrix(model: Mat4) -> (Mat3, bool) {
    let m = Mat3::from_mat4(model);
    let det = m.determinant();
    (if det.abs() < 1e-12 { Mat3::ZERO } else { m.inverse().transpose() }, det < 0.0)
}

/// Whether `model` mirrors: a negative determinant, which also turns its
/// triangles' winding around.
fn mirrors(model: Mat4) -> bool {
    Mat3::from_mat4(model).determinant() < 0.0
}
