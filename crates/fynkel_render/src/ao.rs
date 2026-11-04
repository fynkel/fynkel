//! Light ambient occlusion: darkening where surfaces meet
//! ([`CameraView::ambient_occlusion`](crate::CameraView)).
//!
//! Worked out before the scene is drawn, at half the view's size, from
//! depth alone: the solid scene's depth (`ao_depth.wgsl`), then how hidden
//! each pixel is from 8 points near it, then that blurred (`ao.wgsl`). The
//! forward pass darkens the ambient and sky light by it, as a material's
//! own occlusion; lights and shadows are left as they are. A soft hint of
//! contact, cheap enough for high quality on a phone; a camera without it
//! runs none of it, and its forward pipelines don't read it.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::Mat4;

use crate::assets::PipelineMaker;
use crate::gpu_time::{GpuTimer, Part};
use crate::scene::AmbientOcclusion;
use fynkel_mesh::Vertex;

/// The depth's format, and the occlusion's: one channel, 0 to 1.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// Each pixel's distance along the view, as fs_ao works it out, for the
/// blur: as exact as the sum it was in the shader.
const DISTANCE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

/// `Params` in ao.wgsl.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Params {
    inverse_projection: [[f32; 4]; 4],
    shape: [f32; 4],
}

impl Params {
    /// For `ao` seen through `projection` (reversed-Z, perspective or not)
    /// into a view `height` pixels tall.
    pub(crate) fn new(ao: AmbientOcclusion, projection: Mat4, perspective: bool, height: u32) -> Self {
        let image_height = (height / 2).max(1) as f32;
        Self {
            inverse_projection: projection.inverse().to_cols_array_2d(),
            shape: [
                projection.y_axis.y * image_height / 2.0,
                ao.radius,
                ao.intensity,
                if perspective { 1.0 } else { 0.0 },
            ],
        }
    }
}

/// What every renderer on a device shares for ambient occlusion: layouts,
/// pipelines, and the white stand-in the forward pass reads without it.
pub(crate) struct AoKit {
    device: wgpu::Device,
    maker: PipelineMaker,
    frame_layout: wgpu::BindGroupLayout,
    instance_stride: u64,
    /// ao.wgsl's group 0: the depth, the unblurred occlusion, the params,
    /// the distances.
    layout: wgpu::BindGroupLayout,
    /// How the forward pass reads the occlusion.
    pub(crate) sampler: wgpu::Sampler,
    /// 1 × 1 white: nothing hidden.
    pub(crate) none: wgpu::TextureView,
    /// 1 × 1 of distances: fs_ao's stand-in for the ones it writes.
    no_distance: wgpu::TextureView,
    pipelines: OnceLock<Pipelines>,
}

struct Pipelines {
    /// The depth's, unbent then skinned.
    depth: [wgpu::RenderPipeline; 2],
    ao: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
}

impl AoKit {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        maker: &PipelineMaker,
        frame_layout: &wgpu::BindGroupLayout,
        instance_stride: u64,
    ) -> Self {
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ambient occlusion"),
            entries: &[
                texture(0, wgpu::TextureSampleType::Depth),
                texture(1, wgpu::TextureSampleType::Float { filterable: false }),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Params>() as u64),
                    },
                    count: None,
                },
                texture(3, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ambient occlusion"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("no ambient occlusion"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            white.as_image_copy(),
            &[255],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: None, rows_per_image: None },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let no_distance = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("no ambient occlusion distances"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DISTANCE_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            device: device.clone(),
            maker: maker.clone(),
            frame_layout: frame_layout.clone(),
            instance_stride,
            layout,
            sampler,
            none: white.create_view(&Default::default()),
            no_distance,
            pipelines: OnceLock::new(),
        }
    }

    /// The depth pass's pipeline for unbent or skinned draws.
    pub(crate) fn depth_pipeline(&self, skinned: bool) -> &wgpu::RenderPipeline {
        &self.pipelines().depth[usize::from(skinned)]
    }

    fn pipelines(&self) -> &Pipelines {
        self.pipelines.get_or_init(|| {
            let device = &self.device;
            let depth_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ambient occlusion depth"),
                source: wgpu::ShaderSource::Wgsl(
                    concat!(include_str!("skin.wgsl"), include_str!("ao_depth.wgsl")).into(),
                ),
            });
            let depth_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ambient occlusion depth"),
                bind_group_layouts: &[Some(&self.frame_layout)],
                immediate_size: 0,
            });
            // The forward pass's buffers; only the position, the model
            // matrix and where a skinned draw's joints start are read.
            let vertex_attributes =
                wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Uint32x3];
            // The model rows, then (after the base colour) the params.
            let instance_attributes = [(4, 0), (5, 16), (6, 32), (8, 64)].map(|(shader_location, offset)| {
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset, shader_location }
            });
            let skin_attributes = crate::renderer::SKIN_ATTRIBUTES;
            let buffers = [
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attributes,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: self.instance_stride,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attributes,
                }),
                Some(crate::renderer::skin_buffer(&skin_attributes)),
            ];
            let depth = [false, true].map(|skinned| {
                self.maker.make(wgpu::RenderPipelineDescriptor {
                    label: Some("ambient occlusion depth"),
                    layout: Some(&depth_layout),
                    vertex: wgpu::VertexState {
                        module: &depth_shader,
                        entry_point: Some(if skinned { "vs_depth_skinned" } else { "vs_depth" }),
                        compilation_options: Default::default(),
                        buffers: if skinned { &buffers } else { &buffers[..2] },
                    },
                    // Both sides: a mirrored object's front is clockwise, and
                    // the nearer surface wins either way.
                    primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Greater),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: None,
                    multiview_mask: None,
                    cache: None,
                })
            });

            let shader = device.create_shader_module(wgpu::include_wgsl!("ao.wgsl"));
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ambient occlusion"),
                bind_group_layouts: &[Some(&self.layout)],
                immediate_size: 0,
            });
            let target =
                |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
            let make = |label, entry_point, targets: &[Option<wgpu::ColorTargetState>]| {
                self.maker.make(wgpu::RenderPipelineDescriptor {
                    label: Some(label),
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
                        entry_point: Some(entry_point),
                        compilation_options: Default::default(),
                        targets,
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            Pipelines {
                depth,
                ao: make("ambient occlusion", "fs_ao", &[target(FORMAT), target(DISTANCE_FORMAT)]),
                blur: make("ambient occlusion blur", "fs_blur", &[target(FORMAT)]),
            }
        })
    }
}

/// Tells one view's images apart from another's, and from none (0).
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A view's images, made for its size.
pub(crate) struct AoImages {
    /// Which these are, for knowing when the forward pass reads others.
    pub(crate) id: u64,
    depth: wgpu::TextureView,
    raw: wgpu::TextureView,
    distance: wgpu::TextureView,
    /// What the forward pass reads.
    pub(crate) blurred: wgpu::TextureView,
    ao_group: wgpu::BindGroup,
    blur_group: wgpu::BindGroup,
    params: wgpu::Buffer,
    written: Params,
}

impl AoImages {
    /// Their GPU memory, for a `width` × `height` view: half its size each
    /// way, in the four formats [`AoImages::new`] makes them in.
    pub(crate) fn bytes(width: u32, height: u32) -> u64 {
        let pixels = u64::from((width / 2).max(1)) * u64::from((height / 2).max(1));
        let size = |format: wgpu::TextureFormat| u64::from(format.block_copy_size(None).unwrap_or(4));
        // The depth is 4 bytes a pixel; a depth format has no copy size.
        pixels * (4 + size(FORMAT) * 2 + size(DISTANCE_FORMAT))
    }

    /// Images for a `width` × `height` view.
    pub(crate) fn new(kit: &AoKit, width: u32, height: u32) -> Self {
        let device = &kit.device;
        let size = wgpu::Extent3d { width: (width / 2).max(1), height: (height / 2).max(1), depth_or_array_layers: 1 };
        let image = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let depth = image("ambient occlusion depth", DEPTH_FORMAT);
        let raw = image("ambient occlusion unblurred", FORMAT);
        let distance = image("ambient occlusion distances", DISTANCE_FORMAT);
        let blurred = image("ambient occlusion", FORMAT);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ambient occlusion"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = |second: &wgpu::TextureView, fourth: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ambient occlusion"),
                layout: &kit.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&depth) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(second) },
                    wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(fourth) },
                ],
            })
        };
        // fs_ao doesn't read bindings 1 and 3; the stand-ins fill them.
        let ao_group = group(&kit.none, &kit.no_distance);
        let blur_group = group(&raw, &distance);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            depth,
            raw,
            distance,
            blurred,
            ao_group,
            blur_group,
            params,
            written: bytemuck::Zeroable::zeroed(),
        }
    }

    /// Writes `params` if they've changed.
    pub(crate) fn set(&mut self, queue: &wgpu::Queue, params: Params) {
        if params != self.written {
            queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
            self.written = params;
        }
    }

    /// Begins the depth pass: the caller draws the solid scene into it
    /// with [`AoKit::depth_pipeline`].
    pub(crate) fn depth_pass<'e>(
        &self,
        kit: &AoKit,
        encoder: &'e mut wgpu::CommandEncoder,
        timer: &GpuTimer,
    ) -> wgpu::RenderPass<'e> {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ambient occlusion depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: timer.pass(Part::AmbientOcclusion),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(kit.depth_pipeline(false));
        pass
    }

    /// Records the occlusion and its blur, after the depth pass.
    pub(crate) fn encode(&self, kit: &AoKit, encoder: &mut wgpu::CommandEncoder, timer: &GpuTimer) {
        let pipelines = kit.pipelines();
        let attach = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
            })
        };
        let ao = [attach(&self.raw), attach(&self.distance)];
        let blur = [attach(&self.blurred)];
        for (label, into, pipeline, group) in [
            ("ambient occlusion", &ao[..], &pipelines.ao, &self.ao_group),
            ("ambient occlusion blur", &blur[..], &pipelines.blur, &self.blur_group),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: into,
                depth_stencil_attachment: None,
                timestamp_writes: timer.pass(Part::AmbientOcclusion),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The blur reads each neighbour's distance as fs_ao wrote it, rather
    /// than working it out from the depth again: 16 matrix products a
    /// pixel cost the A10 most of the blur's 5.6 ms (stage 16).
    #[test]
    fn the_blur_reads_distances_fs_ao_wrote() {
        let shader = include_str!("ao.wgsl");
        let blur = &shader[shader.find("fn fs_blur").expect("fs_blur")..];
        assert_eq!(blur.matches("position(").count(), 1, "only its own pixel's: {blur}");
        assert!(blur.contains("textureLoad(distance"), "{blur}");
    }
}
