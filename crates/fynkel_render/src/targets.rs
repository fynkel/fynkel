//! The frame's render targets (HDR, depth, AO, bloom, shadow map), the
//! fallbacks, samplers, buffers and bind groups.

use super::*;

/// Fallback maps and the default material bind group (need a queue, so
/// they exist from the first `prepare`).
pub(crate) struct Fallbacks {
    pub(crate) white: GpuTexture,
    pub(crate) flat_normal: GpuTexture,
    /// A shader asset's `black` and `gray` texture defaults.
    pub(crate) black: GpuTexture,
    pub(crate) gray: GpuTexture,
    pub(crate) default_material: wgpu::BindGroup,
}

/// The frame's render targets at the internal size.
pub(crate) struct Attachments {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) sample_count: u32,
    /// Depth for the scene passes (multisampled with the colour). The
    /// depth prepass fills and stores it; the AO passes read it.
    pub(crate) depth_view: wgpu::TextureView,
    /// The screen-space stage's targets at this size, ambient occlusion
    /// (R2) and contact shadows (R3d); `None` while the profile has both
    /// off, and then the pass chain has no such stage at all.
    pub(crate) ao: Option<AoTargets>,
    /// Bloom's chain (R3b); `None` while the profile has it off, and then
    /// the post pass reads a black placeholder.
    pub(crate) bloom: Option<BloomTargets>,
    /// The multisampled HDR colour, resolved into `hdr_view`; `None` at
    /// one sample (the scene passes write `hdr_view` directly).
    pub(crate) hdr_msaa_view: Option<wgpu::TextureView>,
    /// The single-sample HDR the post pass reads.
    pub(crate) hdr_view: wgpu::TextureView,
    /// The tone-mapped frame at render scale, presented to the target;
    /// `None` at scale 1 (the post pass writes the target directly).
    pub(crate) ldr_view: Option<wgpu::TextureView>,
    pub(crate) post_bind_group: wgpu::BindGroup,
    pub(crate) present_bind_group: Option<wgpu::BindGroup>,
    /// Texture memory of the attachments (the GPU memory estimate).
    pub(crate) bytes: u64,
}

/// Ambient occlusion (R2) at the internal size: the SSAO estimate and the
/// blur that removes its per-pixel rotation, RG8 each. Each pass reads the
/// target the other writes, so neither is ever bound and attached at once.
pub(crate) struct AoTargets {
    /// Sample zero copied from MSAA depth; absent for single-sample scenes.
    pub(crate) resolved_depth: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    pub(crate) view: wgpu::TextureView,
    pub(crate) blur_view: wgpu::TextureView,
    /// Group 1 of the SSAO pass: the scene depth, and `blur_view` (which
    /// the pass does not read — the layout is shared with the blur).
    pub(crate) ao_bind_group: wgpu::BindGroup,
    /// Group 1 of the blur: the scene depth and `view`.
    pub(crate) blur_bind_group: wgpu::BindGroup,
}

/// Bloom's mip chain (R3b) at half the internal size: one view per level,
/// and the group 1 each pass reads — the HDR frame for the bright-pass,
/// the finer level for a downsample, the coarser one for an upsample.
pub(crate) struct BloomTargets {
    pub(crate) levels: Vec<wgpu::TextureView>,
    pub(crate) prefilter_bind_group: wgpu::BindGroup,
    /// `down[i]` reads level `i` (drawn into level `i + 1`).
    pub(crate) down: Vec<wgpu::BindGroup>,
    /// `up[i]` reads level `i + 1` (blended into level `i`).
    pub(crate) up: Vec<wgpu::BindGroup>,
}

/// The sun's shadow map: `resolution` 1 is the placeholder bound while the
/// profile has shadows off.
pub(crate) struct ShadowMap {
    pub(crate) view: wgpu::TextureView,
    pub(crate) resolution: u32,
}

impl ShadowMap {
    pub(crate) fn bytes(&self) -> u64 {
        u64::from(self.resolution) * u64::from(self.resolution) * 4
    }

    /// The side of one cascade's quadrant: the map holds the four of them
    /// in a 2×2 atlas, so the sun costs the same memory it always did (R1).
    pub(crate) fn cascade_side(&self) -> u32 {
        (self.resolution / 2).max(1)
    }

    /// Cascade `i`'s quadrant in atlas uv — origin then size — inset by one
    /// texel so a PCF tap at the edge cannot reach its neighbour.
    pub(crate) fn cascade_rect(&self, i: usize) -> [f32; 4] {
        let side = self.cascade_side() as f32;
        let atlas = self.resolution as f32;
        let (col, row) = ((i % 2) as f32, (i / 2) as f32);
        [
            (col * side + 1.0) / atlas,
            (row * side + 1.0) / atlas,
            (side - 2.0).max(1.0) / atlas,
            (side - 2.0).max(1.0) / atlas,
        ]
    }
}

pub(crate) fn create_material_sampler(device: &wgpu::Device, anisotropy: u32) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("fynkel material sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: anisotropy.clamp(1, 16) as u16,
        ..Default::default()
    })
}

/// A shader asset slot's sampler for [`Renderer::slot_sampler_key`]'s
/// settings.
pub(crate) fn create_slot_sampler(
    device: &wgpu::Device,
    [filter, mipmaps, wrap, anisotropy]: [u32; 4],
) -> wgpu::Sampler {
    let address = match wrap {
        1 => wgpu::AddressMode::ClampToEdge,
        2 => wgpu::AddressMode::MirrorRepeat,
        _ => wgpu::AddressMode::Repeat,
    };
    let (filter, mipmap_filter) = if filter == 1 {
        (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest)
    } else {
        (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear)
    };
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("fynkel shader slot sampler"),
        address_mode_u: address,
        address_mode_v: address,
        address_mode_w: address,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter,
        // Without mipmaps: the top level only.
        lod_max_clamp: if mipmaps == 1 { 32.0 } else { 0.0 },
        anisotropy_clamp: anisotropy as u16,
        ..Default::default()
    })
}

/// The scene targets at `quality`'s internal size and sample count, the
/// LDR intermediate when the render scale is not 1, and the post/present
/// bind groups over them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_attachments(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    quality: &Quality,
    width: u32,
    height: u32,
    layouts: AttachmentLayouts<'_>,
    present_sampler: &wgpu::Sampler,
    post_buffer: &wgpu::Buffer,
) -> Attachments {
    let AttachmentLayouts {
        present: single_layout,
        post: post_layout,
        ao: ao_layout,
        bloom: bloom_layout,
        ao_depth_resolve,
    } = layouts;
    let (w, h) = quality.internal_size(width, height);
    let samples = quality.msaa;
    // The AO passes sample the depth the prepass stored (R2).
    let depth_view = create_attachment(
        device,
        "fynkel depth",
        w,
        h,
        samples,
        DEPTH_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let ao = (quality.ambient_occlusion || quality.contact_shadows).then(|| {
        let ao_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let view = create_attachment(device, "fynkel ao", w, h, 1, AO_FORMAT, ao_usage);
        let blur_view = create_attachment(device, "fynkel ao blur", w, h, 1, AO_FORMAT, ao_usage);
        let resolved_depth = (samples > 1).then(|| {
            let view = create_attachment(
                device,
                "fynkel AO resolved depth",
                w,
                h,
                1,
                wgpu::TextureFormat::R32Float,
                ao_usage,
            );
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fynkel AO depth resolve"),
                layout: &ao_depth_resolve.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&depth_view),
                }],
            });
            (view, group)
        });
        let sampled_depth = resolved_depth
            .as_ref()
            .map_or(&depth_view, |(view, _)| view);
        AoTargets {
            ao_bind_group: create_ao_bind_group(
                device,
                ao_layout,
                sampled_depth,
                &blur_view,
                "fynkel ao bg",
            ),
            blur_bind_group: create_ao_bind_group(
                device,
                ao_layout,
                sampled_depth,
                &view,
                "fynkel ao blur bg",
            ),
            resolved_depth,
            view,
            blur_view,
        }
    });
    let hdr_view = create_attachment(
        device,
        "fynkel hdr",
        w,
        h,
        1,
        HDR_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let hdr_msaa_view = (samples > 1).then(|| {
        create_attachment(
            device,
            "fynkel hdr (multisampled)",
            w,
            h,
            samples,
            HDR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
    });
    let ldr_view = (quality.render_scale != 1.0).then(|| {
        create_attachment(
            device,
            "fynkel ldr",
            w,
            h,
            1,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        )
    });
    // Bloom (R3b): half the internal size, as many levels as fit (down
    // to 2 texels on the short side), each one a view the passes attach.
    let (bw, bh) = ((w / 2).max(1), (h / 2).max(1));
    let mut bloom_texels = 0u64;
    let bloom = quality.bloom.then(|| {
        let mut count = 1;
        while count < BLOOM_LEVELS && (bw >> count) >= 2 && (bh >> count) >= 2 {
            count += 1;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fynkel bloom"),
            size: wgpu::Extent3d {
                width: bw,
                height: bh,
                depth_or_array_layers: 1,
            },
            mip_level_count: count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: BLOOM_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels: Vec<wgpu::TextureView> = (0..count)
            .map(|level| {
                bloom_texels += u64::from((bw >> level).max(1)) * u64::from((bh >> level).max(1));
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("fynkel bloom level"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let reads = |view: &wgpu::TextureView, label| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: bloom_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(present_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                ],
            })
        };
        let n = levels.len();
        BloomTargets {
            prefilter_bind_group: reads(&hdr_view, "fynkel bloom prefilter bg"),
            down: levels[..n - 1]
                .iter()
                .map(|v| reads(v, "fynkel bloom down bg"))
                .collect(),
            up: levels[1..]
                .iter()
                .map(|v| reads(v, "fynkel bloom up bg"))
                .collect(),
            levels,
        }
    });
    // Without the chain the post pass reads a black texel (textures start
    // zeroed).
    let bloom_placeholder = bloom.is_none().then(|| {
        create_attachment(
            device,
            "fynkel bloom placeholder",
            1,
            1,
            1,
            BLOOM_FORMAT,
            wgpu::TextureUsages::TEXTURE_BINDING,
        )
    });
    let bloom_view = match (&bloom, &bloom_placeholder) {
        (Some(b), _) => &b.levels[0],
        (None, Some(p)) => p,
        (None, None) => unreachable!("one of the two exists"),
    };
    let post_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fynkel post bg"),
        layout: post_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(present_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&hdr_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: post_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(bloom_view),
            },
        ],
    });
    let present_bind_group = ldr_view.as_ref().map(|ldr| {
        create_single_bind_group(
            device,
            single_layout,
            present_sampler,
            ldr,
            post_buffer,
            "fynkel present bg",
        )
    });
    // Depth32 × samples, HDR rgba16f × samples when resolved, the HDR
    // resolve, the LDR at render scale (4 bytes a texel), the two AO
    // targets (two bytes each), optional R32Float depth copy, bloom's chain (rgba16f).
    let texels = u64::from(w.max(1)) * u64::from(h.max(1));
    let bytes = texels * 4 * u64::from(samples)
        + if samples > 1 {
            texels * 8 * u64::from(samples)
        } else {
            0
        }
        + texels * 8
        + if ldr_view.is_some() { texels * 4 } else { 0 }
        + if ao.is_some() {
            texels * (if samples > 1 { 8 } else { 4 })
        } else {
            0
        }
        + bloom_texels * 8;
    Attachments {
        width: w,
        height: h,
        sample_count: samples,
        depth_view,
        ao,
        bloom,
        hdr_msaa_view,
        hdr_view,
        ldr_view,
        post_bind_group,
        present_bind_group,
        bytes,
    }
}

pub(crate) fn create_shadow_map(device: &wgpu::Device, resolution: u32) -> ShadowMap {
    ShadowMap {
        view: create_attachment(
            device,
            "fynkel shadow map",
            resolution,
            resolution,
            1,
            SHADOW_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        ),
        resolution,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_frame_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    shadow_map: &wgpu::TextureView,
    shadow_sampler: &wgpu::Sampler,
    cube_sampler: &wgpu::Sampler,
    ibl: &Ibl,
    brdf_lut: &GpuTexture,
    ao: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fynkel frame bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(shadow_map),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(cube_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&ibl.irradiance.view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&ibl.specular.view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&brdf_lut.view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(ao),
            },
        ],
    })
}

pub(crate) fn create_joint_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel joint palettes"),
        size: capacity as u64 * std::mem::size_of::<Mat4>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn create_joint_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    joints: &wgpu::Buffer,
    light_lists: &wgpu::Buffer,
    local_lights: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fynkel skin bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: joints.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: light_lists.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: local_lights.as_entire_binding(),
            },
        ],
    })
}

/// The per-instance light lists: twelve `u32` each, indexed by an
/// instance's `flags[2]`.
pub(crate) fn create_light_list_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel light lists"),
        size: capacity as u64 * std::mem::size_of::<[u32; 12]>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn create_instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel instances"),
        size: capacity as u64 * std::mem::size_of::<Instance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Group 1 of the AO passes: single-sample depth (native depth or the
/// R32Float sample-zero copy), and the other AO target.
pub(crate) fn create_ao_layout(device: &wgpu::Device, sample_count: u32) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fynkel ao bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: if sample_count > 1 {
                        wgpu::TextureSampleType::Float { filterable: false }
                    } else {
                        wgpu::TextureSampleType::Depth
                    },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

/// Group 0 of the AO passes: the frame uniform on its own.
pub(crate) fn create_ao_frame_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fynkel ao frame bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

pub(crate) fn create_ao_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    depth: &wgpu::TextureView,
    other: &wgpu::TextureView,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(depth),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(other),
            },
        ],
    })
}

pub(crate) fn create_attachment(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    sample_count: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    tex.create_view(&Default::default())
}

pub(crate) fn create_single_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
    params: &wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

pub(crate) fn create_material_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    views: [&wgpu::TextureView; 5],
    label: &str,
) -> wgpu::BindGroup {
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::Sampler(sampler),
    }];
    for (i, v) in views.iter().enumerate() {
        entries.push(wgpu::BindGroupEntry {
            binding: i as u32 + 1,
            resource: wgpu::BindingResource::TextureView(v),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &entries,
    })
}

pub(crate) fn create_local_light_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel local lights"),
        size: (capacity.max(1) * std::mem::size_of::<GpuLight>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
