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

use wgpu::util::DeviceExt;

use crate::assets::PipelineMaker;
use crate::gpu::Gpu;
use crate::scene::{ColorSpace, MaterialTextures, TextureKey};
use crate::shader::{FIRST_TEXTURE, MAX_TEXTURES};

pub(crate) struct GpuTexture {
    view: wgpu::TextureView,
    /// All mip levels, as stored (4 bytes a pixel).
    pub(crate) bytes: u64,
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
            entries: &[
                texture_entry(0),
                sampler_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
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

    /// Records drawing levels 1 and up of `texture` from level 0, each
    /// level's alpha multiplied by its entry in `alpha_scales` (1 where
    /// there's none).
    fn record(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        alpha_scales: &[f32],
    ) {
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
            let scale = alpha_scales.get(mip as usize).copied().unwrap_or(1.0);
            let scale = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mipmap alpha scale"),
                contents: bytemuck::bytes_of(&[scale, 0.0, 0.0, 0.0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mipmap"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&level(mip - 1)) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: scale.as_entire_binding() },
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
    /// For materials with their own shaders: `layout`'s and then 1 to
    /// [`MAX_TEXTURES`] of the shader's own, at `own_layouts[n - 1]`.
    pub(crate) own_layouts: [wgpu::BindGroupLayout; MAX_TEXTURES],
    sampler: wgpu::Sampler,
    defaults: Defaults,
    mipmapper: Mipmapper,
}

/// Every texture a world has on the GPU, and what draws need to use them.
#[derive(Default)]
pub(crate) struct Textures {
    textures: HashMap<(TextureKey, ColorSpace), GpuTexture>,
    bind_groups: HashMap<MaterialTextures, wgpu::BindGroup>,
    own_bind_groups: HashMap<OwnTextures, wgpu::BindGroup>,
}

/// A material's textures and its shader's own: how many it has, which,
/// and which of them are colours (a bit each).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct OwnTextures {
    pub(crate) base: MaterialTextures,
    pub(crate) own: [Option<TextureKey>; MAX_TEXTURES],
    pub(crate) count: u8,
    pub(crate) srgb: u8,
}

impl OwnTextures {
    /// A material's textures, with no shader's.
    pub(crate) fn of(base: MaterialTextures) -> Self {
        Self { base, own: [None; MAX_TEXTURES], count: 0, srgb: 0 }
    }

    fn uses(&self, key: TextureKey) -> bool {
        self.base.slots().iter().any(|&(k, _)| k == Some(key)) || self.own.contains(&Some(key))
    }

    /// Own texture `i`'s colour space.
    pub(crate) fn space(&self, i: usize) -> ColorSpace {
        if self.srgb & (1 << i) != 0 { ColorSpace::Srgb } else { ColorSpace::Linear }
    }
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
        let own_layouts = std::array::from_fn(|n| {
            let mut entries = vec![
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                sampler_entry(5),
            ];
            entries.extend((0..=n as u32).map(|i| texture_entry(FIRST_TEXTURE + i)));
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("material and shader textures"),
                entries: &entries,
            })
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
        Self { layout, own_layouts, sampler, defaults, mipmapper: Mipmapper::new(device, maker) }
    }
}

impl GpuTexture {
    /// A texture of RGBA8 pixels for use in `space`, with a full mip chain.
    /// Submits its own GPU work.
    ///
    /// # Panics
    ///
    /// If `rgba` isn't `width × height × 4` bytes, or a side is 0.
    /// `fynkel_load::ImageData` checks both.
    pub(crate) fn new(gpu: &Gpu, kit: &TextureKit, space: ColorSpace, width: u32, height: u32, rgba: &[u8]) -> Self {
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
        // Colour's alpha may be a cut-out's; the other slots have none.
        let alpha_scales =
            if space == ColorSpace::Srgb { alpha_scales(width, height, rgba, mip_level_count) } else { Vec::new() };
        kit.mipmapper.record(device, &mut encoder, &texture, &alpha_scales);
        gpu.queue.submit([encoder.finish()]);

        let bytes = (0..mip_level_count)
            .map(|mip| texture.size().mip_level_size(mip, texture.dimension()))
            .map(|level| level.width as u64 * level.height as u64 * 4);
        Self { view: texture.create_view(&Default::default()), bytes: bytes.sum() }
    }
}

impl GpuTexture {
    /// A font's atlas of distances (stage 15): one byte a pixel, read as
    /// they are, one level. Submits its own GPU work.
    ///
    /// # Panics
    ///
    /// If `pixels` isn't `width × height` bytes, or a side is 0.
    pub(crate) fn distances(gpu: &Gpu, width: u32, height: u32, pixels: &[u8]) -> Self {
        assert_eq!(pixels.len() as u64, width as u64 * height as u64, "{width} × {height} bytes");
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            texture.as_image_copy(),
            pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: None },
            size,
        );
        Self { view: texture.create_view(&Default::default()), bytes: width as u64 * height as u64 }
    }
}

impl Textures {
    /// Puts `texture` under `key` for use in `space`, replacing any there.
    pub(crate) fn insert(&mut self, key: TextureKey, space: ColorSpace, texture: GpuTexture) {
        self.textures.insert((key, space), texture);
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
        self.own_bind_groups.retain(|set, _| !set.uses(key));
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

    /// [`Textures::prepare`] for a material with its own shader. With none
    /// of the shader's own, it's the material's bind group.
    pub(crate) fn prepare_own(&mut self, device: &wgpu::Device, kit: &TextureKit, set: OwnTextures) {
        if set.count == 0 {
            return self.prepare(device, kit, set.base);
        }
        if self.own_bind_groups.contains_key(&set) {
            return;
        }
        let slots = set.base.slots();
        let view = |key: Option<TextureKey>, space: ColorSpace, normal: bool| match key
            .and_then(|k| self.textures.get(&(k, space)))
        {
            Some(texture) => &texture.view,
            None if normal => &kit.defaults.flat_normal,
            None if space == ColorSpace::Srgb => &kit.defaults.white_srgb,
            None => &kit.defaults.white_linear,
        };
        let mut entries: Vec<wgpu::BindGroupEntry<'_>> = (0..5)
            .map(|slot| wgpu::BindGroupEntry {
                binding: slot as u32,
                resource: wgpu::BindingResource::TextureView(view(
                    slots[slot].0,
                    slots[slot].1,
                    slot == MaterialTextures::NORMAL,
                )),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&kit.sampler) });
        for i in 0..set.count as usize {
            entries.push(wgpu::BindGroupEntry {
                binding: FIRST_TEXTURE + i as u32,
                resource: wgpu::BindingResource::TextureView(view(set.own[i], set.space(i), false)),
            });
        }
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material and shader textures"),
            layout: &kit.own_layouts[set.count as usize - 1],
            entries: &entries,
        });
        self.own_bind_groups.insert(set, bind_group);
    }

    /// The bind group [`Textures::prepare_own`] made for `set`.
    pub(crate) fn own_bind_group(&self, set: &OwnTextures) -> Option<&wgpu::BindGroup> {
        if set.count == 0 { self.bind_groups.get(&set.base) } else { self.own_bind_groups.get(set) }
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

/// The alpha a cut-out's texel needs to be there, as the scales are made
/// for: glTF's default cutoff, and most textures'.
const COVERAGE_CUTOFF: f32 = 0.5;

/// What each mip level's alpha is multiplied by, so as many of its texels
/// reach [`COVERAGE_CUTOFF`] as of the image's own (Castaño's
/// coverage-preserving mips; Unity's "Mip Maps Preserve Coverage").
/// Averaged, a fine cut-out's alpha sinks below the cutoff with distance,
/// and the surface and its shadow vanish.
///
/// The levels are followed as the GPU makes them: each the 2×2 average of
/// the one above as stored. A level is read filtered, so the share is
/// counted where it's sampled: at its texels and halfway between them. A
/// level too small to hold the share (all its texels alike) is covered
/// whole rather than not at all. Empty where no
/// texel is see-through, or none reaches the cutoff: nothing to keep.
fn alpha_scales(width: u32, height: u32, rgba: &[u8], levels: u32) -> Vec<f32> {
    let alpha = |i: usize| rgba[i * 4 + 3];
    let n = (width * height) as usize;
    if (0..n).all(|i| alpha(i) == 255) {
        return Vec::new();
    }
    let reaching = (0..n).filter(|&i| f32::from(alpha(i)) / 255.0 >= COVERAGE_CUTOFF).count();
    if reaching == 0 {
        return Vec::new();
    }
    let share = reaching as f64 / n as f64;
    let mut level: Vec<f32> = (0..n).map(|i| f32::from(alpha(i)) / 255.0).collect();
    let (mut w, mut h) = (width as usize, height as usize);
    let mut scales = vec![1.0];
    let mut sorted = Vec::new();
    for _ in 1..levels {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let at = |x: usize, y: usize| level[y.min(h - 1) * w + x.min(w - 1)];
        let raw: Vec<f32> = (0..nw * nh)
            .map(|i| {
                let (x, y) = (i % nw * 2, i / nw * 2);
                (at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1)) * 0.25
            })
            .collect();
        // Each texel, and the filtered alpha halfway to the next across,
        // down and diagonally.
        let near = |x: usize, y: usize| raw[y.min(nh - 1) * nw + x.min(nw - 1)];
        sorted.clear();
        for y in 0..nh {
            for x in 0..nw {
                let (a, right, down, both) = (near(x, y), near(x + 1, y), near(x, y + 1), near(x + 1, y + 1));
                sorted.extend([a, (a + right) * 0.5, (a + down) * 0.5, (a + right + down + both) * 0.25]);
            }
        }
        // The alpha the share's last sample has: scaled to just reach the
        // cutoff once stored as 8 bits.
        let k = ((share * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
        let (_, &mut threshold, _) = sorted.select_nth_unstable_by(k - 1, |a, b| b.total_cmp(a));
        let scale = if threshold > 0.0 { (COVERAGE_CUTOFF * 255.0).ceil() / 255.0 / threshold } else { 1.0 };
        scales.push(scale);
        level = raw.iter().map(|a| ((a * scale).min(1.0) * 255.0).round() / 255.0).collect();
        (w, h) = (nw, nh);
    }
    scales
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stripes, 3 texels opaque in each 8, and an opaque texture, which is
    /// left alone.
    #[test]
    fn mips_keep_a_cutouts_share() {
        let stripes: Vec<u8> = (0..64u32 * 64).flat_map(|i| [255, 255, 255, if i % 8 < 3 { 255 } else { 0 }]).collect();
        let scales = alpha_scales(64, 64, &stripes, 7);
        assert_eq!(scales.len(), 7);
        // None thins out; the level where the stripes alternate, 0.5 and 0,
        // is filtered to 0.25 between its texels, so it's raised.
        assert!(scales[1..].iter().all(|&s| s >= 1.0), "{scales:?}");
        assert!(scales[2] > 1.2, "{scales:?}");
        assert!(alpha_scales(4, 4, &[255; 64], 3).is_empty());
    }
}
