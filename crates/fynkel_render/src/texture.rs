//! Textures on the GPU: uploading with mipmaps, the material sampler, and
//! one bind group per set of material textures.
//!
//! An image becomes a texture per colour space it's used in: sRGB for
//! colour slots, linear for data slots ([`ColorSpace`]). The slot decides,
//! so a normal map can't be read as colour by mistake. Its mipmaps are
//! drawn on the GPU in that space, so a texture doesn't darken as it
//! shrinks (`mipmap.wgsl`).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::assets::PipelineMaker;
use crate::gpu::Gpu;
use crate::scene::{ColorSpace, MaterialTextures, TextureKey};

struct GpuTexture {
    view: wgpu::TextureView,
    /// All mip levels, as stored (4 bytes a pixel).
    bytes: u64,
}

/// Draws mip levels, one pipeline per texture format, made when a texture
/// of that format is first uploaded.
struct Mipmapper {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    maker: PipelineMaker,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    srgb: OnceLock<wgpu::RenderPipeline>,
    linear: OnceLock<wgpu::RenderPipeline>,
}

impl Mipmapper {
    fn new(device: &wgpu::Device, maker: &PipelineMaker) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("mipmap.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mipmap"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mipmap"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mipmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            sampler,
            maker: maker.clone(),
            shader,
            pipeline_layout,
            srgb: OnceLock::new(),
            linear: OnceLock::new(),
        }
    }

    fn make(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some("mipmap"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Records drawing levels 1 and up of `texture` from level 0.
    fn record(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture) {
        let pipeline = if texture.format().is_srgb() {
            self.srgb.get_or_init(|| self.make(wgpu::TextureFormat::Rgba8UnormSrgb))
        } else {
            self.linear.get_or_init(|| self.make(wgpu::TextureFormat::Rgba8Unorm))
        };
        let level = |mip| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        for mip in 1..texture.mip_level_count() {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mipmap"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&level(mip - 1)) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mipmap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &level(mip),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// The 1×1 textures used for empty slots.
struct Defaults {
    /// sRGB white, for colour slots.
    white_srgb: wgpu::TextureView,
    /// Linear white, for metallic-roughness and occlusion: the factors
    /// alone decide.
    white_linear: wgpu::TextureView,
    /// (0.5, 0.5, 1) exactly, in a float format, so an unmapped surface
    /// keeps its normal to the bit.
    flat_normal: wgpu::TextureView,
}

/// What every world's textures share on one device: the bind group
/// layout, the sampler, the empty-slot textures and the mipmapper.
pub(crate) struct TextureKit {
    pub(crate) layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    defaults: Defaults,
    mipmapper: Mipmapper,
}

/// Every texture a world has on the GPU, and what draws need to use them.
#[derive(Default)]
pub(crate) struct Textures {
    textures: HashMap<(TextureKey, ColorSpace), GpuTexture>,
    bind_groups: HashMap<MaterialTextures, wgpu::BindGroup>,
}

impl TextureKit {
    pub(crate) fn new(gpu: &Gpu, maker: &PipelineMaker) -> Self {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material textures"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                sampler_entry(5),
            ],
        });
        // Trilinear and repeating. Anisotropic filtering keeps a texture
        // sharp on a surface seen at a slant (the ground, far away); 8× is
        // where most of its gain is. It needs every filter linear.
        let anisotropic =
            gpu.adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::ANISOTROPIC_FILTERING);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: if anisotropic { 8 } else { 1 },
            ..Default::default()
        });
        let pixel = |label, format, bytes: &[u8]| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            gpu.queue.write_texture(
                texture.as_image_copy(),
                bytes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: None, rows_per_image: None },
                texture.size(),
            );
            texture.create_view(&Default::default())
        };
        // Half floats: 0.5 = 0x3800, 1.0 = 0x3C00.
        let flat: Vec<u8> = [0x3800u16, 0x3800, 0x3C00, 0x3C00].iter().flat_map(|h| h.to_le_bytes()).collect();
        let defaults = Defaults {
            white_srgb: pixel("white (sRGB)", wgpu::TextureFormat::Rgba8UnormSrgb, &[255; 4]),
            white_linear: pixel("white", wgpu::TextureFormat::Rgba8Unorm, &[255; 4]),
            flat_normal: pixel("flat normal", wgpu::TextureFormat::Rgba16Float, &flat),
        };
        Self { layout, sampler, defaults, mipmapper: Mipmapper::new(device, maker) }
    }
}

impl Textures {
    /// Uploads RGBA8 pixels under `key` for use in `space`, with a full
    /// mip chain, replacing any texture already there. Submits its own GPU
    /// work.
    ///
    /// # Panics
    ///
    /// If `rgba` isn't `width × height × 4` bytes, or a side is 0 or over
    /// the device's limit. `fynkel_load::ImageData` checks all of these.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upload(
        &mut self,
        gpu: &Gpu,
        kit: &TextureKit,
        key: TextureKey,
        space: ColorSpace,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) {
        assert_eq!(rgba.len() as u64, width as u64 * height as u64 * 4, "{width} × {height} RGBA8 pixels");
        // The loader allows 8192 a side, but a device may offer less: over
        // its limit, the image is halved until it fits, not refused by the
        // GPU (which would stop the view).
        let limit = gpu.device.limits().max_texture_dimension_2d;
        let shrunk;
        let (width, height, rgba) = if width.max(height) > limit {
            shrunk = shrink_to(limit, width, height, rgba);
            (shrunk.0, shrunk.1, shrunk.2.as_slice())
        } else {
            (width, height, rgba)
        };
        let device = &gpu.device;
        let mip_level_count = width.max(height).ilog2() + 1;
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("material texture"),
            size,
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: space.format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: None },
            size,
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mipmaps") });
        kit.mipmapper.record(device, &mut encoder, &texture);
        gpu.queue.submit([encoder.finish()]);

        let bytes = (0..mip_level_count)
            .map(|mip| texture.size().mip_level_size(mip, texture.dimension()))
            .map(|level| level.width as u64 * level.height as u64 * 4);
        let gpu_texture = GpuTexture { view: texture.create_view(&Default::default()), bytes: bytes.sum() };
        self.textures.insert((key, space), gpu_texture);
        self.forget_bind_groups(key);
    }

    /// Frees `key`'s textures in every colour space. Returns whether there
    /// was one.
    pub(crate) fn remove(&mut self, key: TextureKey) -> bool {
        let before = self.textures.len();
        self.textures.retain(|&(k, _), _| k != key);
        self.forget_bind_groups(key);
        self.textures.len() < before
    }

    /// Bind groups name texture views, so one that uses `key` is remade
    /// when `key` changes or goes.
    fn forget_bind_groups(&mut self, key: TextureKey) {
        self.bind_groups.retain(|set, _| set.slots().iter().all(|&(k, _)| k != Some(key)));
    }

    /// GPU memory held by textures, in bytes: every mip level of every
    /// texture as stored, without the driver's padding.
    pub(crate) fn bytes(&self) -> u64 {
        self.textures.values().map(|t| t.bytes).sum()
    }

    /// Makes the bind group for `set` unless there is one. Allocates only
    /// when it makes one.
    pub(crate) fn prepare(&mut self, device: &wgpu::Device, kit: &TextureKit, set: MaterialTextures) {
        if self.bind_groups.contains_key(&set) {
            return;
        }
        let slots = set.slots();
        let view = |slot: usize| {
            let (key, space) = slots[slot];
            match key.and_then(|k| self.textures.get(&(k, space))) {
                Some(texture) => &texture.view,
                None if slot == MaterialTextures::NORMAL => &kit.defaults.flat_normal,
                None if space == ColorSpace::Srgb => &kit.defaults.white_srgb,
                None => &kit.defaults.white_linear,
            }
        };
        let texture = |slot: usize| wgpu::BindGroupEntry {
            binding: slot as u32,
            resource: wgpu::BindingResource::TextureView(view(slot)),
        };
        let entries = [
            texture(0),
            texture(1),
            texture(2),
            texture(3),
            texture(4),
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&kit.sampler) },
        ];
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material textures"),
            layout: &kit.layout,
            entries: &entries,
        });
        self.bind_groups.insert(set, bind_group);
    }

    /// The bind group [`Textures::prepare`] made for `set`, unless a
    /// texture it used changed since.
    pub(crate) fn bind_group(&self, set: &MaterialTextures) -> Option<&wgpu::BindGroup> {
        self.bind_groups.get(set)
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// `rgba` halved (each pixel the mean of the 2 × 2 it covers) until no side
/// is over `limit`. An odd side's last row or column is averaged with
/// itself.
fn shrink_to(limit: u32, mut width: u32, mut height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut pixels = rgba.to_vec();
    while width.max(height) > limit {
        let (w, h) = ((width / 2).max(1), (height / 2).max(1));
        let mut half = vec![0; w as usize * h as usize * 4];
        for y in 0..h {
            for x in 0..w {
                let ys = [(2 * y).min(height - 1), (2 * y + 1).min(height - 1)];
                let xs = [(2 * x).min(width - 1), (2 * x + 1).min(width - 1)];
                for c in 0..4 {
                    let mut sum = 0u32;
                    for sy in ys {
                        for sx in xs {
                            sum += u32::from(pixels[((sy * width + sx) * 4 + c) as usize]);
                        }
                    }
                    half[((y * w + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
                }
            }
        }
        (width, height, pixels) = (w, h, half);
    }
    (width, height, pixels)
}

#[cfg(test)]
mod shrink_tests {
    use super::shrink_to;

    #[test]
    fn an_image_over_the_limit_is_halved_until_it_fits() {
        // 8 × 2: left half black, right half white.
        let rgba: Vec<u8> = (0..16).flat_map(|i| if i % 8 < 4 { [0, 0, 0, 255] } else { [255; 4] }).collect();
        let (width, height, pixels) = shrink_to(2, 8, 2, &rgba);
        assert_eq!((width, height), (2, 1));
        assert_eq!(pixels, [0, 0, 0, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn an_odd_side_keeps_its_last_row() {
        // 3 × 1: 0, 100, 200 in red.
        let rgba = [0, 0, 0, 0, 100, 0, 0, 0, 200, 0, 0, 0];
        let (width, height, pixels) = shrink_to(1, 3, 1, &rgba);
        assert_eq!((width, height), (1, 1));
        assert_eq!(pixels[0], 50);
    }
}
