//! The fixed set of render pipelines, created up front (again only when a
//! quality switch changes the sample count).

use super::*;

/// The fixed pipeline set for one sample count (`docs/renderer.md` lists
/// every variant).
pub(crate) struct Pipelines {
    /// Opaque meshes, back faces culled.
    pub(crate) mesh: wgpu::RenderPipeline,
    /// Opaque meshes, double-sided (no culling, back faces flip the normal).
    pub(crate) mesh_double_sided: wgpu::RenderPipeline,
    /// `blend` meshes: alpha blended, depth read only.
    pub(crate) transparent: wgpu::RenderPipeline,
    pub(crate) transparent_double_sided: wgpu::RenderPipeline,
    /// The four above with the vertex skinned by its joint palette
    /// (`vs_skinned`; M3-19).
    pub(crate) mesh_skinned: wgpu::RenderPipeline,
    pub(crate) mesh_skinned_double_sided: wgpu::RenderPipeline,
    pub(crate) transparent_skinned: wgpu::RenderPipeline,
    pub(crate) transparent_skinned_double_sided: wgpu::RenderPipeline,
    pub(crate) sky: wgpu::RenderPipeline,
    /// Depth only, from the sun.
    pub(crate) shadow: wgpu::RenderPipeline,
    pub(crate) shadow_skinned: wgpu::RenderPipeline,
    /// Depth only, from the camera, for the opaque instances the scene
    /// pass will draw: the ambient-occlusion stage's input (R2). The
    /// vertex stage and the culling are the mesh pass's, so the depths
    /// match exactly and its `LessEqual` compare keeps every fragment.
    pub(crate) depth: wgpu::RenderPipeline,
    pub(crate) depth_double_sided: wgpu::RenderPipeline,
    pub(crate) depth_skinned: wgpu::RenderPipeline,
    pub(crate) depth_skinned_double_sided: wgpu::RenderPipeline,
    /// The selection outline's mask (`outline.rs`): the mesh vertex
    /// stages, one sample, no depth, `fs_mask` into `R8Unorm` blended
    /// with `Max`. Culling as the scene pass's twins.
    pub(crate) mask: wgpu::RenderPipeline,
    pub(crate) mask_double_sided: wgpu::RenderPipeline,
    pub(crate) mask_skinned: wgpu::RenderPipeline,
    pub(crate) mask_skinned_double_sided: wgpu::RenderPipeline,
    /// Screen-space ambient occlusion and its blur.
    pub(crate) ao: wgpu::RenderPipeline,
    pub(crate) ao_blur: wgpu::ComputePipeline,
    pub(crate) ao_depth_resolve: wgpu::RenderPipeline,
    /// Bloom (R3b): the bright-pass into the chain's first level, the
    /// downsample between levels, and the upsample blended back over the
    /// finer level by `scatter` (the blend constant).
    pub(crate) bloom_prefilter: wgpu::RenderPipeline,
    pub(crate) bloom_down: wgpu::RenderPipeline,
    pub(crate) bloom_up: wgpu::RenderPipeline,
    pub(crate) post: wgpu::RenderPipeline,
    /// LDR at render scale → the target.
    pub(crate) present: wgpu::RenderPipeline,
    /// The scene passes' samples per pixel.
    pub(crate) sample_count: u32,
    /// The target is a Unorm format: the post pass encodes sRGB itself.
    pub(crate) encode_srgb: bool,
}

impl Pipelines {
    pub(crate) fn opaque(&self, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        match (skinned, double_sided) {
            (false, false) => &self.mesh,
            (false, true) => &self.mesh_double_sided,
            (true, false) => &self.mesh_skinned,
            (true, true) => &self.mesh_skinned_double_sided,
        }
    }

    /// The depth prepass twin of [`opaque`](Self::opaque): same vertex
    /// stage, same culling, no colour (R2).
    pub(crate) fn depth(&self, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        match (skinned, double_sided) {
            (false, false) => &self.depth,
            (false, true) => &self.depth_double_sided,
            (true, false) => &self.depth_skinned,
            (true, true) => &self.depth_skinned_double_sided,
        }
    }

    pub(crate) fn transparent(&self, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        match (skinned, double_sided) {
            (false, false) => &self.transparent,
            (false, true) => &self.transparent_double_sided,
            (true, false) => &self.transparent_skinned,
            (true, true) => &self.transparent_skinned_double_sided,
        }
    }

    /// The selection outline's mask twin of [`opaque`](Self::opaque).
    pub(crate) fn mask(&self, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        match (skinned, double_sided) {
            (false, false) => &self.mask,
            (false, true) => &self.mask_double_sided,
            (true, false) => &self.mask_skinned,
            (true, true) => &self.mask_skinned_double_sided,
        }
    }

    pub(crate) const fn count() -> usize {
        // The twenty-seven below plus the overlay's grid, lines and
        // triangles (`overlay::OverlayPass`, ADR-0019) and the selection
        // outline's composite (`outline::OutlinePass`).
        31
    }
}

/// The bind group layouts the pipelines are built over.
pub(crate) struct Layouts<'a> {
    pub(crate) frame: &'a wgpu::BindGroupLayout,
    pub(crate) material: &'a wgpu::BindGroupLayout,
    pub(crate) single: &'a wgpu::BindGroupLayout,
    pub(crate) shadow: &'a wgpu::BindGroupLayout,
    pub(crate) skin: &'a wgpu::BindGroupLayout,
    /// The AO passes' group 0: the frame uniform alone (the mesh pass's
    /// group 0 holds the texture the blur writes).
    pub(crate) ao_frame: &'a wgpu::BindGroupLayout,
    pub(crate) ao: &'a wgpu::BindGroupLayout,
    /// The post pass's group 1 (the HDR, its parameters, bloom's level 0).
    pub(crate) post: &'a wgpu::BindGroupLayout,
    /// A bloom pass's group 1 (R3b).
    pub(crate) bloom: &'a wgpu::BindGroupLayout,
}

/// The bind group layouts the attachments' bind groups are made over.
pub(crate) struct AttachmentLayouts<'a> {
    pub(crate) present: &'a wgpu::BindGroupLayout,
    pub(crate) post: &'a wgpu::BindGroupLayout,
    pub(crate) ao: &'a wgpu::BindGroupLayout,
    pub(crate) bloom: &'a wgpu::BindGroupLayout,
    pub(crate) ao_depth_resolve: &'a wgpu::RenderPipeline,
    pub(crate) ao_blur: &'a wgpu::ComputePipeline,
}

/// Twenty-seven of the pipeline variants (`docs/renderer.md`) for one sample
/// count — the overlay's three (grid, lines, triangles) live with its pass
/// in `overlay.rs`, the outline's composite in `outline.rs`: mesh (cull
/// back), mesh double-sided (cull none), transparent (blend, cull back),
/// transparent double-sided, the same four skinned (`vs_skinned`, a
/// third vertex buffer of joint weights and the palette storage buffer at
/// group 2), sky, shadow, shadow skinned, the four camera depth prepass
/// twins, the four outline mask twins (one sample whatever the scene's),
/// SSAO, the AO blur, the sample-zero depth copy, bloom's three, post, present. Nothing else is ever created; a quality switch to
/// another sample count rebuilds this set.
/// The mesh stages' vertex buffers: slot 0 the cooked 72-byte vertex,
/// slot 1 the [`Instance`], and for the skinned stages slot 2 the cooked
/// `SkinWeight` (four u16 joints, four f32 weights).
pub(crate) fn vertex_buffers(skinned: bool) -> Vec<Option<wgpu::VertexBufferLayout<'static>>> {
    const VERTEX: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x3, 2 => Float32x4,
        3 => Float32x2, 4 => Float32x2, 5 => Float32x4
    ];
    const INSTANCE: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
        10 => Float32x4, 11 => Float32x4, 12 => Float32x4, 13 => Float32x4
    ];
    const SKIN: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![14 => Uint16x4, 15 => Float32x4];
    let mut buffers = vec![
        Some(wgpu::VertexBufferLayout {
            array_stride: Vertex::STRIDE as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE,
        }),
    ];
    if skinned {
        buffers.push(Some(wgpu::VertexBufferLayout {
            array_stride: SkinWeight::STRIDE as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SKIN,
        }));
    }
    buffers
}

/// The transparent stage's target: premultiplied accumulation — colour by
/// the fragment's alpha, the HDR alpha (the post pass's coverage) by
/// `1 - alpha`.
pub(crate) fn hdr_blend_target() -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        }),
        write_mask: wgpu::ColorWrites::ALL,
    }
}

/// The selection outline's mask target, blended by `keep`.
pub(crate) fn mask_target(keep: wgpu::BlendComponent) -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format: outline::MASK_FORMAT,
        blend: Some(wgpu::BlendState {
            color: keep,
            alpha: keep,
        }),
        write_mask: wgpu::ColorWrites::ALL,
    }
}

pub(crate) fn create_pipelines(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    sample_count: u32,
    layouts: Layouts<'_>,
) -> Pipelines {
    let Layouts {
        frame: frame_layout,
        material: material_layout,
        single: single_layout,
        shadow: shadow_layout,
        skin: skin_layout,
        ao_frame: ao_frame_layout,
        ao: ao_layout,
        post: post_layout,
        bloom: bloom_layout,
    } = layouts;
    let module = |label, body: &str| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(
                format!("{}{body}", include_str!("shaders/frame.wgsl")).into(),
            ),
        })
    };
    let mesh_shader = module(
        "fynkel mesh shader",
        &format!(
            "{MESH_STAGE}{LIGHT_LIBRARY}{}",
            include_str!("shaders/pbr.wgsl")
        ),
    );
    let sky_shader = module("fynkel sky shader", include_str!("shaders/sky.wgsl"));
    let post_shader = module("fynkel post shader", include_str!("shaders/post.wgsl"));
    let bloom_shader = module("fynkel bloom shader", include_str!("shaders/bloom.wgsl"));
    let present_shader = module(
        "fynkel present shader",
        include_str!("shaders/present.wgsl"),
    );
    let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fynkel shadow shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shadow.wgsl").into()),
    });
    // AO reads sample zero copied from MSAA depth into R32Float once,
    // or the original single-sample depth texture directly.
    let ao_shader = module(
        "fynkel ao shader",
        &include_str!("shaders/ao.wgsl")
            .replace(
                "DEPTH_TEXTURE",
                if sample_count > 1 {
                    "texture_2d<f32>"
                } else {
                    "texture_depth_2d"
                },
            )
            .replace(
                "DEPTH_LOAD",
                if sample_count > 1 {
                    "textureLoad(depth_tex, c, 0).x"
                } else {
                    "textureLoad(depth_tex, c, 0)"
                },
            ),
    );

    let blur_shader = module(
        "fynkel tiled ao blur",
        &include_str!("shaders/ao_blur.wgsl")
            .replace(
                "DEPTH_TEXTURE",
                if sample_count > 1 {
                    "texture_2d<f32>"
                } else {
                    "texture_depth_2d"
                },
            )
            .replace(
                "DEPTH_LOAD",
                if sample_count > 1 {
                    "textureLoad(depth_tex, c, 0).x"
                } else {
                    "textureLoad(depth_tex, c, 0)"
                },
            ),
    );
    let blur_layout = create_ao_blur_layout(device, sample_count);
    let blur_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel tiled ao blur layout"),
        bind_group_layouts: &[Some(ao_frame_layout), Some(&blur_layout)],
        ..Default::default()
    });
    let ao_blur = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("fynkel tiled ao blur"),
        layout: Some(&blur_pl),
        module: &blur_shader,
        entry_point: Some("cs_main"),
        compilation_options: Default::default(),
        cache: None,
    });

    // Group 2 (the joint palettes) is part of every mesh layout so the
    // passes bind it once; the static stages simply never read it.
    let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel mesh pl"),
        bind_group_layouts: &[Some(frame_layout), Some(material_layout), Some(skin_layout)],
        ..Default::default()
    });
    let shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel shadow pl"),
        bind_group_layouts: &[
            Some(shadow_layout),
            Some(material_layout),
            Some(skin_layout),
        ],
        ..Default::default()
    });
    let single_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel single-texture pl"),
        bind_group_layouts: &[Some(frame_layout), Some(single_layout)],
        ..Default::default()
    });
    let post_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel post pl"),
        bind_group_layouts: &[Some(frame_layout), Some(post_layout)],
        ..Default::default()
    });
    // The bloom passes share the AO passes' group 0: the frame uniform
    // alone.
    let bloom_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel bloom pl"),
        bind_group_layouts: &[Some(ao_frame_layout), Some(bloom_layout)],
        ..Default::default()
    });
    let ao_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel ao pl"),
        bind_group_layouts: &[Some(ao_frame_layout), Some(ao_layout)],
        ..Default::default()
    });
    let hdr_target = [Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let hdr_blend_target = [Some(hdr_blend_target())];
    let multisample = wgpu::MultisampleState {
        count: sample_count,
        ..Default::default()
    };
    let mesh_buffers = vertex_buffers(false);
    let skinned_buffers = vertex_buffers(true);
    // `fs_depth` (no colour target) is the camera depth prepass; the rest
    // shade. The compare is `LessEqual` because a prepass has already
    // written exactly the depth the scene pass computes — the vertex
    // stage is the same function, so the two agree to the bit.
    let mesh = |label,
                skinned: bool,
                cull_mode,
                targets: &[Option<wgpu::ColorTargetState>],
                depth_write,
                entry: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: if skinned {
                    &skinned_buffers
                } else {
                    &mesh_buffers
                },
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(depth_write),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample,
            multiview_mask: None,
            cache: None,
        })
    };
    let shadow_pipeline = |label, skinned: bool| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&shadow_pl),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: if skinned {
                    &skinned_buffers
                } else {
                    &mesh_buffers
                },
            },
            fragment: Some(wgpu::FragmentState {
                module: &shadow_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SHADOW_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    // The outline's mask (`outline.rs`): the scene's vertex stages and
    // culling at the target's size, one sample and no depth — the whole
    // silhouette, hidden parts included. `Max` keeps a selected texel's 1
    // over a child's 0.5 in whatever order they land (WebGPU allows the
    // min/max operations with factors of one only).
    let keep_max = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    };
    let mask_target = [Some(mask_target(keep_max))];
    let mask = |label, skinned: bool, cull_mode| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: if skinned {
                    &skinned_buffers
                } else {
                    &mesh_buffers
                },
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_mask"),
                compilation_options: Default::default(),
                targets: &mask_target,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let shadow = shadow_pipeline("fynkel shadow pipeline", false);
    let shadow_skinned = shadow_pipeline("fynkel shadow skinned pipeline", true);
    let ao_target = [Some(wgpu::ColorTargetState {
        format: AO_FORMAT,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let ao_pipeline = |label, entry: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&ao_pl),
            vertex: wgpu::VertexState {
                module: &ao_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ao_shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &ao_target,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            // The AO targets are single-sample whatever the scene is.
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let bloom_pipeline = |label, entry: &str, blend| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&bloom_pl),
            vertex: wgpu::VertexState {
                module: &bloom_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &bloom_shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: BLOOM_FORMAT,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    // `lerp(finer, coarser, scatter)`: the upsample is the source, the
    // level it lands on the destination.
    let scatter = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Constant,
        dst_factor: wgpu::BlendFactor::OneMinusConstant,
        operation: wgpu::BlendOperation::Add,
    };
    let bloom_scatter = wgpu::BlendState {
        color: scatter,
        alpha: scatter,
    };
    let fullscreen = |label,
                      layout: &wgpu::PipelineLayout,
                      shader: &wgpu::ShaderModule,
                      targets: &[Option<wgpu::ColorTargetState>],
                      depth,
                      multisample| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: depth,
            multisample,
            multiview_mask: None,
            cache: None,
        })
    };
    let sky = fullscreen(
        "fynkel sky pipeline",
        &single_pl,
        &sky_shader,
        &hdr_target,
        Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample,
    );
    let host_target = [Some(wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let post = fullscreen(
        "fynkel post pipeline",
        &post_pl,
        &post_shader,
        &host_target,
        None,
        Default::default(),
    );
    let present = fullscreen(
        "fynkel present pipeline",
        &single_pl,
        &present_shader,
        &host_target,
        None,
        Default::default(),
    );
    let back = Some(wgpu::Face::Back);
    Pipelines {
        mesh: mesh(
            "fynkel mesh pipeline",
            false,
            back,
            &hdr_target,
            true,
            "fs_main",
        ),
        mesh_double_sided: mesh(
            "fynkel mesh pipeline (double-sided)",
            false,
            None,
            &hdr_target,
            true,
            "fs_main",
        ),
        transparent: mesh(
            "fynkel transparent pipeline",
            false,
            back,
            &hdr_blend_target,
            false,
            "fs_main",
        ),
        transparent_double_sided: mesh(
            "fynkel transparent pipeline (double-sided)",
            false,
            None,
            &hdr_blend_target,
            false,
            "fs_main",
        ),
        mesh_skinned: mesh(
            "fynkel mesh skinned pipeline",
            true,
            back,
            &hdr_target,
            true,
            "fs_main",
        ),
        mesh_skinned_double_sided: mesh(
            "fynkel mesh skinned pipeline (double-sided)",
            true,
            None,
            &hdr_target,
            true,
            "fs_main",
        ),
        transparent_skinned: mesh(
            "fynkel transparent skinned pipeline",
            true,
            back,
            &hdr_blend_target,
            false,
            "fs_main",
        ),
        transparent_skinned_double_sided: mesh(
            "fynkel transparent skinned pipeline (double-sided)",
            true,
            None,
            &hdr_blend_target,
            false,
            "fs_main",
        ),
        sky,
        shadow,
        shadow_skinned,
        depth: mesh("fynkel depth pipeline", false, back, &[], true, "fs_depth"),
        depth_double_sided: mesh(
            "fynkel depth pipeline (double-sided)",
            false,
            None,
            &[],
            true,
            "fs_depth",
        ),
        depth_skinned: mesh(
            "fynkel depth skinned pipeline",
            true,
            back,
            &[],
            true,
            "fs_depth",
        ),
        depth_skinned_double_sided: mesh(
            "fynkel depth skinned pipeline (double-sided)",
            true,
            None,
            &[],
            true,
            "fs_depth",
        ),
        mask: mask("fynkel outline mask pipeline", false, back),
        mask_double_sided: mask("fynkel outline mask pipeline (double-sided)", false, None),
        mask_skinned: mask("fynkel outline mask skinned pipeline", true, back),
        mask_skinned_double_sided: mask(
            "fynkel outline mask skinned pipeline (double-sided)",
            true,
            None,
        ),
        ao: ao_pipeline("fynkel ao pipeline", "fs_main"),
        ao_blur,
        ao_depth_resolve: create_ao_depth_resolve(device),
        bloom_prefilter: bloom_pipeline(
            "fynkel bloom prefilter pipeline",
            "fs_prefilter",
            wgpu::BlendState::REPLACE,
        ),
        bloom_down: bloom_pipeline(
            "fynkel bloom down pipeline",
            "fs_down",
            wgpu::BlendState::REPLACE,
        ),
        bloom_up: bloom_pipeline("fynkel bloom up pipeline", "fs_up", bloom_scatter),
        post,
        present,
        sample_count,
        encode_srgb: !format.is_srgb(),
    }
}

/// Copy sample zero once, preserving the existing AO depth-sample policy.
/// Repeated screen-space reads then use compact R32Float rather than repeatedly reading MSAA depth.
fn create_ao_depth_resolve(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fynkel AO depth resolve"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ao_depth.wgsl").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fynkel AO depth resolve"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::R32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod screen_space_tests {
    use super::*;

    #[test]
    fn ao_kernel_preserves_reference_hemisphere_across_dither_rotations() {
        let source = include_str!("shaders/ao.wgsl");
        let table = source
            .split("const AO_KERNEL =")
            .nth(1)
            .unwrap()
            .split(");")
            .next()
            .unwrap();
        let points: Vec<Vec<f32>> = table
            .lines()
            .filter_map(|line| {
                let coordinates = line.trim().strip_prefix("vec3<f32>(")?.strip_suffix("),")?;
                Some(
                    coordinates
                        .split(',')
                        .map(|v| v.trim().parse().unwrap())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(points.len(), 12);
        for (i, k) in points.iter().enumerate() {
            assert_eq!(k.len(), 3);
            let a = (i as f32 + 0.5) / 12.0;
            let radius = 0.2 + 0.8 * a * a;
            for angle in 0..1024 {
                let rotation = angle as f32 * std::f32::consts::TAU / 1024.0;
                let phi = rotation + i as f32 * 2.399_963_1;
                let reference =
                    glam::Vec3::new(phi.cos() * a.sqrt(), phi.sin() * a.sqrt(), (1.0 - a).sqrt())
                        * radius;
                let actual = glam::Vec3::new(
                    rotation.cos() * k[0] - rotation.sin() * k[1],
                    rotation.sin() * k[0] + rotation.cos() * k[1],
                    k[2],
                );
                assert!(
                    (reference - actual).abs().max_element() < 3e-6,
                    "sample {i}, rotation {rotation}"
                );
            }
        }
    }

    #[test]
    fn ao_depth_copy_preserves_sample_zero_at_odd_and_single_pixel_sizes() {
        let Some((device, queue)) = headless_device() else {
            return;
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("distinct depth per sample"),
            source: wgpu::ShaderSource::Wgsl(r#"
                @vertex fn vs_main(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
                    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
                    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
                }
                @fragment fn fs_main(@builtin(position) p: vec4<f32>, @builtin(sample_index) sample: u32) -> @builtin(frag_depth) f32 {
                    return 0.1 + 0.1 * f32(sample) + floor(p.x) * 0.001 + floor(p.y) * 0.01;
                }
            "#.into()),
        });
        let write = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 4,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let resolve = create_ao_depth_resolve(&device);
        for (width, height) in [(1, 1), (5, 3)] {
            let extent = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let texture = |format, sample_count, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: extent,
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let depth = texture(
                DEPTH_FORMAT,
                4,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let output = texture(
                wgpu::TextureFormat::R32Float,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let depth_view = depth.create_view(&Default::default());
            let output_view = output.create_view(&Default::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &resolve.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&depth_view),
                }],
            });
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(height) * 256,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&write);
                pass.draw(0..3, 0..1);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &output_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&resolve);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &output,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(height),
                    },
                },
                extent,
            );
            queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    tx.send(result).unwrap();
                });
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            rx.recv().unwrap().unwrap();
            let data = buffer.slice(..).get_mapped_range().unwrap();
            for y in 0..height {
                for x in 0..width {
                    let offset = (y * 256 + x * 4) as usize;
                    let actual = f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
                    let expected = 0.1 + x as f32 * 0.001 + y as f32 * 0.01;
                    assert!(
                        (actual - expected).abs() < 1e-6,
                        "{width}x{height} ({x},{y}): {actual} vs {expected}"
                    );
                }
            }
            drop(data);
            buffer.unmap();
        }
    }
}
