//! Bloom: light brighter than white glowing into what's around it
//! ([`CameraView::bloom`](crate::CameraView)).
//!
//! It works on the scene image, so only above low. A chain of images, each
//! half the size of the one before, the first half the view's: the scene
//! image's bright light goes into the first (the prefilter), then each
//! into the next, smaller (down), then each back into the larger, blurred
//! and added (up). The last pass adds the first image's glow to the scene
//! (`finish.wgsl`'s `fs_bloom`). Each pass reads a few texels of a small
//! image, so the whole chain costs about one more full-size pass. A camera
//! without bloom, or at strength 0, runs none of it.

use std::sync::OnceLock;

use crate::assets::PipelineMaker;
use crate::scene::Bloom;

/// The images' format: light, not yet tone mapped.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The most images in a chain: the smallest, of a 1080-pixel view, is 17
/// pixels across, so the glow reaches about a tenth of the view.
const MAX_LEVELS: u32 = 6;

/// `Params` in bloom.wgsl and finish.wgsl.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    strength: f32,
    threshold: f32,
    knee: f32,
    levels: f32,
}

/// What every renderer on a device shares for bloom: the layout each pass
/// (and the last) reads an image through, its sampler, and the pipelines.
pub(crate) struct BloomKit {
    device: wgpu::Device,
    maker: PipelineMaker,
    /// An image, its sampler and the [`Params`].
    pub(crate) layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: OnceLock<Pipelines>,
}

struct Pipelines {
    prefilter: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
}

impl BloomKit {
    pub(crate) fn new(device: &wgpu::Device, maker: &PipelineMaker) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom"),
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
            ],
        });
        // Clamped: light off the view's edge doesn't wrap round to the
        // other side.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { device: device.clone(), maker: maker.clone(), layout, sampler, pipelines: OnceLock::new() }
    }

    fn pipelines(&self) -> &Pipelines {
        self.pipelines.get_or_init(|| {
            let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("bloom"),
                source: wgpu::ShaderSource::Wgsl(
                    concat!(include_str!("neutral.wgsl"), include_str!("bloom.wgsl")).into(),
                ),
            });
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("bloom"),
                bind_group_layouts: &[Some(&self.layout)],
                immediate_size: 0,
            });
            let make = |label, entry_point, blend| {
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
                        targets: &[Some(wgpu::ColorTargetState {
                            format: FORMAT,
                            blend,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let add = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            };
            Pipelines {
                prefilter: make("bloom prefilter", "fs_prefilter", None),
                down: make("bloom down", "fs_down", None),
                up: make("bloom up", "fs_up", Some(wgpu::BlendState { color: add, alpha: add })),
            }
        })
    }
}

/// A view's chain of images, made for its size and scene image.
pub(crate) struct BloomChain {
    /// Each image's view, largest first, and the bind group a pass reads
    /// it through.
    levels: Vec<(wgpu::TextureView, wgpu::BindGroup)>,
    /// The scene image, read by the prefilter.
    scene: wgpu::BindGroup,
    params: wgpu::Buffer,
    /// What's in `params`.
    written: Params,
}

impl BloomChain {
    /// A chain for a `width` × `height` view whose scene image is `image`.
    pub(crate) fn new(kit: &BloomKit, image: &wgpu::TextureView, width: u32, height: u32) -> Self {
        let device = &kit.device;
        let (w, h) = ((width / 2).max(1), (height / 2).max(1));
        // Down to about 8 pixels across, at most.
        let levels = (w.min(h).max(1).ilog2().saturating_sub(2) + 1).min(MAX_LEVELS);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bloom"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let read = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom"),
                layout: &kit.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&kit.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                ],
            })
        };
        let levels = (0..levels)
            .map(|level| {
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                });
                let group = read(&view);
                (view, group)
            })
            .collect();
        Self { scene: read(image), levels, params, written: bytemuck::Zeroable::zeroed() }
    }

    /// What the last pass reads: the first image, its glow added up.
    pub(crate) fn glow(&self) -> &wgpu::BindGroup {
        &self.levels[0].1
    }

    /// Records the passes that work out the glow of `bloom` into the first
    /// image.
    pub(crate) fn encode(
        &mut self,
        kit: &BloomKit,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        bloom: Bloom,
    ) {
        let params = Params {
            strength: bloom.strength,
            threshold: bloom.threshold,
            knee: bloom.threshold * 0.5,
            levels: self.levels.len() as f32,
        };
        if params != self.written {
            queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
            self.written = params;
        }
        let pipelines = kit.pipelines();
        let pass = |encoder: &mut wgpu::CommandEncoder, label, into: &wgpu::TextureView, load, pipeline, from| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: into,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, from, &[]);
            pass.draw(0..3, 0..1);
        };
        // Every pixel is written, so what was there is never read.
        let fresh = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        pass(encoder, "bloom prefilter", &self.levels[0].0, fresh, &pipelines.prefilter, &self.scene);
        for level in 1..self.levels.len() {
            pass(encoder, "bloom down", &self.levels[level].0, fresh, &pipelines.down, &self.levels[level - 1].1);
        }
        for level in (0..self.levels.len() - 1).rev() {
            let load = wgpu::LoadOp::Load;
            pass(encoder, "bloom up", &self.levels[level].0, load, &pipelines.up, &self.levels[level + 1].1);
        }
    }
}
