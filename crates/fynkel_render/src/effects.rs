//! Screen effects (stage 10): a camera's shaders run over the finished
//! picture, in order, each over what the one before it made, the last onto
//! the view (effect.wgsl). Pictures pass between them in images the view's
//! size and format, made the first frame a camera has effects and let go
//! a while after it has none.

/// An effect's uniform: the view's size and the time, the camera (its
/// inverse view and projection, where it is and the way it looks), then
/// its values.
pub(crate) const UNIFORM_BYTES: u64 = 16 + 64 + 32 + crate::MAX_VALUES_BYTES as u64;

/// Where an effect's values start in its uniform.
pub(crate) const VALUES_AT: usize = 16 + 64 + 32;

/// The layout and sampler every effect reads through.
pub(crate) struct EffectKit {
    /// The picture, its sampler and the uniform, at a dynamic offset.
    pub(crate) layout: wgpu::BindGroupLayout,
    /// The solid scene's depth, for effects that read it (group 2; their
    /// own textures are group 1, as a material's shader's).
    pub(crate) depth_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Each effect's uniform's room in the buffer.
    pub(crate) stride: u64,
}

impl EffectKit {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen effect"),
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
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                    },
                    count: None,
                },
            ],
        });
        let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen effect depth"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen effect"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        Self { layout, depth_layout, sampler, stride: UNIFORM_BYTES.div_ceil(align) * align }
    }
}

/// A renderer's pictures between effects, one or two, and their uniforms.
pub(crate) struct EffectImages {
    pub(crate) size: (u32, u32),
    format: wgpu::TextureFormat,
    /// Each image's view, and the bind group an effect reads it through.
    pub(crate) images: Vec<(wgpu::TextureView, wgpu::BindGroup)>,
    pub(crate) buffer: wgpu::Buffer,
    /// How many effects' uniforms the buffer holds.
    room: usize,
}

impl EffectImages {
    /// Images for `count` effects (one, or two to pass between), of `size`
    /// and `format`.
    pub(crate) fn new(
        kit: &EffectKit,
        device: &wgpu::Device,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        count: usize,
    ) -> Self {
        let room = count.next_power_of_two();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen effects"),
            size: room as u64 * kit.stride,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let images = (0..count.min(2))
            .map(|_| {
                let view = device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("screen effect picture"),
                        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default());
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("screen effect picture"),
                    layout: &kit.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&kit.sampler) },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &buffer,
                                offset: 0,
                                size: wgpu::BufferSize::new(UNIFORM_BYTES),
                            }),
                        },
                    ],
                });
                (view, bind_group)
            })
            .collect();
        Self { size, format, images, buffer, room }
    }

    /// Whether these do for `count` effects of `size` and `format`.
    pub(crate) fn fit(&self, size: (u32, u32), format: wgpu::TextureFormat, count: usize) -> bool {
        self.size == size && self.format == format && self.images.len() >= count.min(2) && self.room >= count
    }

    /// The GPU memory `count` effects' images take at `size` and `format`.
    pub(crate) fn bytes(size: (u32, u32), format: wgpu::TextureFormat, count: usize) -> u64 {
        let texel = u64::from(format.block_copy_size(None).unwrap_or(4));
        count.min(2) as u64 * u64::from(size.0) * u64::from(size.1) * texel
    }
}
