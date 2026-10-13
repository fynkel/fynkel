//! The forward renderer: one pass, 4× MSAA, reversed-Z.
//!
//! A frame has two halves. [`Renderer::prepare`] is plain CPU work: it
//! turns the scene into per-draw instance data and the frame uniforms, in
//! buffers it reuses, so a steady frame allocates nothing.
//! [`Renderer::encode`] writes that where the GPU reads it (see
//! `frame_ring`) and records the pass. Debug lines come last, on top.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3, Vec4};

use crate::assets::{self, Assets, Kit, SharedAssets};
use crate::frame_ring::FrameRing;
use crate::gpu::Gpu;
use crate::scene::{ColorSpace, MaterialTextures, MeshKey, RenderScene, TextureKey};
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

pub(crate) const FRAME_UNIFORMS_SIZE: u64 = std::mem::size_of::<FrameUniforms>() as u64;

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

/// The pipelines that draw into one target format, shared by every
/// renderer on a device that draws into it ([`Kit`]).
pub(crate) struct Pipelines {
    /// Opaque then cut-out, each for every [`Faces`] (see
    /// [`pipeline_index`]).
    forward: Vec<wgpu::RenderPipeline>,
    lines: wgpu::RenderPipeline,
}

/// Draws [`RenderScene`]s into textures of one format: a view's. What it
/// draws with, it shares: the pipelines with every renderer on its device,
/// the meshes and textures with every renderer of its world.
pub struct Renderer {
    kit: Arc<Kit>,
    format: wgpu::TextureFormat,
    pipelines: Arc<Pipelines>,
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
    /// Each seen draw's normal matrix and whether it mirrors, by its index
    /// in the scene.
    normals: Vec<(Mat3, bool)>,
    clear: wgpu::Color,
    size: (u32, u32),
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
                kit.frame_layout.clone(),
                FRAME_UNIFORMS_SIZE,
                64 * INSTANCE_SIZE,
            ),
            kit,
            format,
            assets,
            targets: None,
            uniforms: FrameUniforms::zeroed(),
            instances: Vec::new(),
            batches: Vec::new(),
            line_vertices: Vec::new(),
            order: Vec::new(),
            normals: Vec::new(),
            clear: wgpu::Color::BLACK,
            size: (0, 0),
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

        // Leave out what the camera can't see, then group the rest by
        // pipeline, textures and mesh, so each run of the same three is one
        // instanced draw call and the state changes least between calls.
        // Each draw's key and normal matrix are worked out once. The sort
        // is in place and the buffers reused, so a steady frame allocates
        // nothing.
        let planes = frustum_planes(projection * camera.view);
        self.order.clear();
        self.normals.resize(scene.draws.len(), (Mat3::ZERO, false));
        let assets = assets::lock(&self.assets);
        for (i, draw) in scene.draws.iter().enumerate() {
            let Some(mesh) = assets.meshes.get(&draw.mesh) else {
                stats.missing_meshes += 1;
                continue;
            };
            if !in_view(&planes, draw.model, mesh.centre, mesh.radius) {
                stats.culled += 1;
                continue;
            }
            let (normal, mirrored) = normal_matrix(draw.model);
            self.normals[i] = (normal, mirrored);
            let m = &draw.material;
            let key = (m.alpha_cutoff > 0.0, Faces::of(m, mirrored), m.textures, draw.mesh.0);
            self.order.push((key, i as u32));
        }
        drop(assets);
        self.order.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        for &((cutout, faces, textures, mesh), i) in &self.order {
            let draw = &scene.draws[i as usize];
            let m = &draw.material;
            let (normal, mirrored) = self.normals[i as usize];
            let instance = self.instances.len() as u32;
            match self.batches.last_mut() {
                Some(b) if (b.cutout, b.faces, b.textures, b.mesh.0) == (cutout, faces, textures, mesh) => b.count += 1,
                _ => self.batches.push(Batch { cutout, faces, textures, mesh: draw.mesh, first: instance, count: 1 }),
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
        // looks them up. The world's other views wait while this one
        // records; a mesh or texture they free meanwhile is gone from the
        // next frame on.
        let mut assets = assets::lock(&self.assets);
        let Assets { kit, meshes, textures } = &mut *assets;
        for batch in &self.batches {
            textures.prepare(&kit.gpu.device, &kit.textures, batch.textures);
        }
        let frame = self.frame_data.write(
            &self.kit.gpu.device,
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
            // Freed since prepare, by another view's render worker.
            let (Some(mesh), Some(bind_group)) = (meshes.get(&batch.mesh), textures.bind_group(&batch.textures)) else {
                continue;
            };
            if last.is_none_or(|l| (l.cutout, l.faces) != (batch.cutout, batch.faces)) {
                pass.set_pipeline(&self.pipelines.forward[pipeline_index(batch.cutout, batch.faces)]);
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
            pass.set_pipeline(&self.pipelines.lines);
            pass.set_vertex_buffer(0, frame.buffer.slice(frame.lines..));
            pass.draw(0..self.line_vertices.len() as u32, 0..1);
        }
    }

    fn create_targets(&self, width: u32, height: u32) -> Targets {
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
        Targets { width, height, color: make("msaa colour", self.format), depth: make("msaa depth", DEPTH_FORMAT) }
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
            bind_group_layouts: &[Some(frame_layout), Some(&kit.textures.layout)],
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

        Self {
            forward: [false, true]
                .into_iter()
                .flat_map(|cutout| Faces::ALL.map(|faces| (cutout, faces)))
                .map(|(cutout, faces)| pipeline(cutout, faces))
                .collect(),
            lines,
        }
    }
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

/// Whether a mesh whose bounding sphere is at `centre` with `radius`, in
/// its own space, can be seen at all when placed by `model`: false only
/// when the sphere is wholly outside one of the `planes`.
fn in_view(planes: &[Vec4; 6], model: Mat4, centre: Vec3, radius: f32) -> bool {
    let at = model.transform_point3(centre);
    // The longest axis's stretch: the sphere stays around the mesh under
    // any scale.
    let stretch =
        model.x_axis.truncate().length().max(model.y_axis.truncate().length()).max(model.z_axis.truncate().length());
    let r = radius * stretch;
    planes.iter().all(|p| p.truncate().dot(at) + p.w >= -r)
}
