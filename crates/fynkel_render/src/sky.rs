//! The sky: what's seen behind everything, and the light it gives.
//!
//! A plain sky is a colour: the clear colour, and that much ambient light.
//! A gradient or an image lights the scene from cubes worked out on the GPU
//! once, when it's set (`env.wgsl`), not each frame. An image's are
//! unturned at intensity 1, the forward pass turning and scaling what it
//! reads; a gradient's hold each of its colours' share of the light in a
//! channel (zenith red, horizon green, ground blue), so they're made once
//! for every gradient, the forward pass colouring what it reads:
//!
//! - the radiance cube, [`RADIANCE`] texels a side: the sky itself, with
//!   mips, each the average of the one above;
//! - the specular cube, [`SPECULAR`] a side, a mip for each roughness from
//!   a mirror (0) to fully rough (1): what a surface of that roughness
//!   reflects;
//! - the irradiance cube, [`IRRADIANCE`] a side: what lights a rough
//!   surface facing each way.
//!
//! The forward pass reads the last two (group 3); the radiance cube is
//! dropped once they're made. An image is kept too, packed as
//! `Rgb9e5Ufloat` (4 bytes a pixel), for drawing behind everything.

use std::sync::OnceLock;

use glam::Vec3;

use crate::assets::PipelineMaker;
use crate::gpu_time::{GpuTimer, Part};
use crate::scene::SkyKind;

/// The radiance cube's side, in texels: 6 × 128² half-float texels, about
/// 1 MB with its mips, while the others are made.
pub(crate) const RADIANCE: u32 = 128;
const RADIANCE_MIPS: u32 = 8;
/// The specular cube's side, and its mips: roughness 0, 0.2, ... 1.
pub(crate) const SPECULAR: u32 = 64;
pub(crate) const SPECULAR_MIPS: u32 = 6;
/// The irradiance cube's side.
pub(crate) const IRRADIANCE: u32 = 16;
/// The radiance mip the irradiance is read from: 8 texels a side.
const IRRADIANCE_SOURCE_MIP: f32 = 4.0;

const CUBE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgb9e5Ufloat;

/// `Pass` in env.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassUniform {
    face: u32,
    size: f32,
    roughness: f32,
    source_size: f32,
    zenith: [f32; 4],
    horizon: [f32; 4],
    ground: [f32; 4],
}

/// What every renderer on a device shares for skies: layouts, samplers,
/// the cubes' pipelines, and the stand-ins bound without a sky.
pub(crate) struct SkyKit {
    device: wgpu::Device,
    queue: wgpu::Queue,
    maker: PipelineMaker,
    /// env.wgsl's group 0.
    env_layout: wgpu::BindGroupLayout,
    env_pipelines: OnceLock<EnvPipelines>,
    /// The forward pass's group 3: the specular and irradiance cubes and
    /// their sampler.
    pub(crate) light_layout: wgpu::BindGroupLayout,
    /// sky.wgsl's group 1: the image and its sampler.
    pub(crate) image_layout: wgpu::BindGroupLayout,
    cube_sampler: wgpu::Sampler,
    image_sampler: wgpu::Sampler,
    /// Black cubes and a black image, 1 × 1, for no sky.
    pub(crate) no_light: wgpu::BindGroup,
    pub(crate) no_image: wgpu::BindGroup,
}

struct EnvPipelines {
    source: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    specular: wgpu::RenderPipeline,
    irradiance: wgpu::RenderPipeline,
}

impl SkyKit {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, maker: &PipelineMaker) -> Self {
        let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let env_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky light passes"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<PassUniform>() as u64),
                    },
                    count: None,
                },
                texture(1, wgpu::TextureViewDimension::D2),
                texture(2, wgpu::TextureViewDimension::Cube),
                sampler(3),
            ],
        });
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky light"),
            entries: &[
                texture(0, wgpu::TextureViewDimension::Cube),
                texture(1, wgpu::TextureViewDimension::Cube),
                sampler(2),
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky image"),
            entries: &[texture(0, wgpu::TextureViewDimension::D2), sampler(1)],
        });
        let cube_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky light"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        // Round the way round, clamped top and bottom.
        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky image"),
            address_mode_u: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let black_cube = cube(device, "no sky light", 1, 1, wgpu::TextureUsages::TEXTURE_BINDING);
        let black_view = cube_view(&black_cube);
        let no_light = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("no sky light"),
            layout: &light_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&black_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&black_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&cube_sampler) },
            ],
        });
        let black_image = image_texture(device, queue, 1, 1, &[0]);
        let no_image = image_bind_group(device, &image_layout, &black_image, &image_sampler);
        Self {
            device: device.clone(),
            queue: queue.clone(),
            maker: maker.clone(),
            env_layout,
            env_pipelines: OnceLock::new(),
            light_layout,
            image_layout,
            cube_sampler,
            image_sampler,
            no_light,
            no_image,
        }
    }

    fn env_pipelines(&self) -> &EnvPipelines {
        self.env_pipelines.get_or_init(|| {
            let device = &self.device;
            let shader = device.create_shader_module(wgpu::include_wgsl!("env.wgsl"));
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky light"),
                bind_group_layouts: &[Some(&self.env_layout)],
                immediate_size: 0,
            });
            let make = |entry: &'static str| {
                self.maker.make(wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
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
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: CUBE_FORMAT,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            EnvPipelines {
                source: make("fs_source"),
                downsample: make("fs_downsample"),
                specular: make("fs_specular"),
                irradiance: make("fs_irradiance"),
            }
        })
    }
}

/// A view's sky: what lights its scene and what's drawn behind, for the
/// sky it was made for.
pub(crate) struct Environment {
    /// What it was made from.
    pub(crate) kind: SkyKind,
    /// The forward pass's group 3.
    pub(crate) light: wgpu::BindGroup,
    /// sky.wgsl's group 1: an image's, or none.
    pub(crate) image: Option<wgpu::BindGroup>,
}

/// The GPU memory a view holds for a sky of `kind`: its cubes, and an
/// image's pixels.
pub(crate) fn bytes(kind: &SkyKind) -> u64 {
    let texels = |side: u32, mips: u32| (0..mips).map(|m| 6 * u64::from((side >> m).max(1)).pow(2)).sum::<u64>();
    match kind {
        SkyKind::Color(_) => 0,
        SkyKind::Gradient { .. } => 8 * (texels(SPECULAR, SPECULAR_MIPS) + texels(IRRADIANCE, 1)),
        SkyKind::Image { image, .. } => 8 * (texels(SPECULAR, SPECULAR_MIPS) + texels(IRRADIANCE, 1)) + image.bytes(),
    }
}

impl Environment {
    /// Works out `kind`'s light (a gradient's or an image's) in passes
    /// recorded into `encoder`.
    pub(crate) fn new(kit: &SkyKit, encoder: &mut wgpu::CommandEncoder, kind: &SkyKind, timer: &GpuTimer) -> Self {
        let device = &kit.device;
        let pipelines = kit.env_pipelines();
        let mut base = PassUniform {
            face: 0,
            size: 0.0,
            roughness: 0.0,
            source_size: RADIANCE as f32,
            zenith: [0.0; 4],
            horizon: [0.0; 4],
            ground: [0.0; 4],
        };
        let image = match kind {
            // Unturned at intensity 1: the forward pass turns and scales
            // what it reads, so a turning or brightening sky is never
            // worked out again.
            SkyKind::Image { image, .. } => {
                base.ground[3] = 1.0;
                Some(image_texture(device, &kit.queue, image.width(), image.height(), image.packed()))
            }
            // Each colour's share of the light in a channel of its own:
            // blending is linear in the colours, so the forward pass gets
            // any gradient's light from these.
            SkyKind::Gradient { .. } => {
                base.zenith = [1.0, 0.0, 0.0, 0.0];
                base.horizon = [0.0, 1.0, 0.0, 0.0];
                base.ground = [0.0, 0.0, 1.0, 0.0];
                None
            }
            // A plain sky needs no cubes; nothing makes one for it.
            SkyKind::Color(_) => None,
        };
        let source_view = image.as_ref().map_or_else(
            || image_texture(device, &kit.queue, 1, 1, &[0]).create_view(&Default::default()),
            |t| t.create_view(&Default::default()),
        );

        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let radiance = cube(device, "sky radiance", RADIANCE, RADIANCE_MIPS, usage);
        let specular = cube(device, "sky specular", SPECULAR, SPECULAR_MIPS, usage);
        let irradiance = cube(device, "sky irradiance", IRRADIANCE, 1, usage);

        // Every pass's uniforms, one buffer at the device's offset
        // alignment, and what each draws: (pipeline, target, its uniforms,
        // the radiance view it reads).
        let stride = u64::from(device.limits().min_uniform_buffer_offset_alignment)
            .max(std::mem::size_of::<PassUniform>() as u64);
        let mut uniforms: Vec<u8> = Vec::new();
        let mut passes: Vec<(&wgpu::RenderPipeline, wgpu::TextureView, u64, Option<u32>)> = Vec::new();
        let mut add =
            |pipeline, texture: &wgpu::Texture, mip: u32, face: u32, uniform: PassUniform, reads: Option<u32>| {
                let offset = uniforms.len() as u64;
                uniforms.extend_from_slice(bytemuck::bytes_of(&uniform));
                uniforms.resize((offset + stride) as usize, 0);
                let target = texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    base_array_layer: face,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                passes.push((pipeline, target, offset, reads));
            };
        for face in 0..6 {
            let size = RADIANCE as f32;
            add(&pipelines.source, &radiance, 0, face, PassUniform { face, size, ..base }, None);
        }
        for mip in 1..RADIANCE_MIPS {
            for face in 0..6 {
                let size = (RADIANCE >> mip) as f32;
                // Reads mip - 1 alone, through a view of it, at its lod 0.
                add(&pipelines.downsample, &radiance, mip, face, PassUniform { face, size, ..base }, Some(mip - 1));
            }
        }
        for mip in 0..SPECULAR_MIPS {
            for face in 0..6 {
                let size = (SPECULAR >> mip) as f32;
                let roughness = mip as f32 / (SPECULAR_MIPS - 1) as f32;
                add(&pipelines.specular, &specular, mip, face, PassUniform { face, size, roughness, ..base }, None);
            }
        }
        for face in 0..6 {
            let size = IRRADIANCE as f32;
            let roughness = IRRADIANCE_SOURCE_MIP;
            add(&pipelines.irradiance, &irradiance, 0, face, PassUniform { face, size, roughness, ..base }, None);
        }
        let buffer = filled(device, &uniforms);

        let whole = cube_view(&radiance);
        let mips: Vec<wgpu::TextureView> = (0..RADIANCE_MIPS)
            .map(|mip| {
                radiance.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::Cube),
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let bind_group = |reads: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky light pass"),
                layout: &kit.env_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &buffer,
                            offset: 0,
                            size: wgpu::BufferSize::new(std::mem::size_of::<PassUniform>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&source_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(reads) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&kit.cube_sampler) },
                ],
            })
        };
        // The source passes read no cube, but one must be bound: a stand-in.
        let reading_whole = bind_group(&whole);
        let reading_mip: Vec<wgpu::BindGroup> = mips.iter().map(&bind_group).collect();
        let reading_nothing = {
            let stand_in = cube(device, "sky light stand-in", 1, 1, wgpu::TextureUsages::TEXTURE_BINDING);
            bind_group(&cube_view(&stand_in))
        };
        for (pipeline, target, offset, reads) in &passes {
            let group = match reads {
                Some(mip) => &reading_mip[*mip as usize],
                None if std::ptr::eq(*pipeline, &pipelines.source) => &reading_nothing,
                None => &reading_whole,
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky light"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timer.pass(Part::SkyLight),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[*offset as u32]);
            pass.draw(0..3, 0..1);
        }

        let light = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky light"),
            layout: &kit.light_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&cube_view(&specular)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&cube_view(&irradiance)),
                },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&kit.cube_sampler) },
            ],
        });
        let image = image.map(|t| image_bind_group(device, &kit.image_layout, &t, &kit.image_sampler));
        Self { kind: kind.clone(), light, image }
    }
}

/// Khronos PBR Neutral, as forward.wgsl's: for colours the CPU works out
/// to match what the GPU shows.
pub(crate) fn tonemap_neutral(color: Vec3) -> Vec3 {
    let start = 0.8 - 0.04;
    let x = color.min_element();
    let offset = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };
    let color = color - Vec3::splat(offset);
    let peak = color.max_element();
    if peak < start {
        return color;
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    let color = color * (new_peak / peak);
    let g = 1.0 - 1.0 / (0.15 * (peak - new_peak) + 1.0);
    color.lerp(Vec3::splat(new_peak), g)
}

fn cube(device: &wgpu::Device, label: &str, side: u32, mips: u32, usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 6 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CUBE_FORMAT,
        usage,
        view_formats: &[],
    })
}

fn cube_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    })
}

fn image_texture(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, packed: &[u32]) -> wgpu::Texture {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sky image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: IMAGE_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(packed),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
        size,
    );
    texture
}

fn image_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sky image"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

fn filled(device: &wgpu::Device, bytes: &[u8]) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sky light passes"),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: true,
    });
    buffer.get_mapped_range_mut(..).expect("mapped at creation").copy_from_slice(bytes);
    buffer.unmap();
    buffer
}
