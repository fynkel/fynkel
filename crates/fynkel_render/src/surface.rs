//! Shader assets in the renderer (ADR-0031 § 6).
//!
//! A `.shader` file's code is compiled into two modules: the scene module
//! — `frame.wgsl`, the mesh stage, the light and `surface.wgsl`'s entry
//! points around the shader's `surface()` and `vertex()` — and the shadow
//! module (`surface_shadow.wgsl`). Each shader gets its own material bind
//! group layout (group 1: the property block, then each texture and its
//! sampler) and one pipeline per variant: skinned or not, single- or
//! double-sided, and the pass — scene, transparent, depth prepass,
//! selection mask, shadow. Groups 0 and 2 are the ones every mesh uses,
//! so a shader's draws batch with the rest.

use fynkel_shader::Shader;

use super::{
    hdr_blend_target, mask_target, vertex_buffers, DEPTH_FORMAT, HDR_FORMAT, LIGHT_LIBRARY,
    MESH_STAGE, SHADOW_FORMAT,
};

const FRAME: &str = include_str!("shaders/frame.wgsl");
const MESH_INPUT: &str = include_str!("shaders/mesh_input.wgsl");
const ENTRIES: &str = include_str!("shaders/surface.wgsl");
const SHADOW: &str = include_str!("shaders/surface_shadow.wgsl");

/// What the WGSL written for one shader adds after its code: its
/// `vertex()` wrapped for the engine's stages (or nothing), and whether
/// it is lit.
fn hooks(shader: &Shader) -> String {
    let hook = if shader.has_vertex() {
        "fn fynkel_hook(in: VsIn) -> VsIn {\n    var v = in;\n    \
         let o = vertex(VertexInput(in.position, in.normal, in.tangent, in.uv0, in.uv1, in.color));\n    \
         v.position = o.position;\n    v.normal = o.normal;\n    v.tangent = o.tangent;\n    \
         v.uv0 = o.uv0;\n    v.uv1 = o.uv1;\n    v.color = o.color;\n    return v;\n}\n"
    } else {
        "fn fynkel_hook(in: VsIn) -> VsIn {\n    return in;\n}\n"
    };
    let lit = shader.interface.lighting == fynkel_shader::Lighting::Lit;
    format!("\n{hook}\nconst FYNKEL_LIT: bool = {lit};\n")
}

/// The scene module's source for `shader`.
pub(crate) fn scene_source(shader: &Shader) -> String {
    let tail = format!("{MESH_STAGE}{LIGHT_LIBRARY}{ENTRIES}{}", hooks(shader));
    format!("{FRAME}{}", fynkel_shader::assemble(shader, &tail).source)
}

/// The shadow module's source for `shader`.
pub(crate) fn shadow_source(shader: &Shader) -> String {
    let tail = format!("{MESH_INPUT}{SHADOW}{}", hooks(shader));
    fynkel_shader::assemble(shader, &tail).source
}

/// The layouts every shader's pipelines share with the built-in's.
pub(crate) struct SurfaceContext<'a> {
    pub frame: &'a wgpu::BindGroupLayout,
    pub shadow: &'a wgpu::BindGroupLayout,
    pub skin: &'a wgpu::BindGroupLayout,
    pub sample_count: u32,
}

/// Which pipeline of a shader a draw needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pass {
    Opaque,
    Transparent,
    Depth,
    Mask,
    Shadow,
}

/// One shader asset, compiled: its interface, its material layout and its
/// pipelines at the current sample count.
pub(crate) struct SurfaceShader {
    pub shader: Shader,
    pub layout: wgpu::BindGroupLayout,
    scene: wgpu::ShaderModule,
    shadow: wgpu::ShaderModule,
    pipelines: Pipelines,
}

/// Indexed by `skinned * 2 + double_sided`; the shadow pass culls
/// nothing, so it has the skinned axis only.
struct Pipelines {
    sample_count: u32,
    opaque: [wgpu::RenderPipeline; 4],
    transparent: [wgpu::RenderPipeline; 4],
    depth: [wgpu::RenderPipeline; 4],
    mask: [wgpu::RenderPipeline; 4],
    shadow: [wgpu::RenderPipeline; 2],
}

impl SurfaceShader {
    /// Compiles `shader` for this device. `Err` (the reason) when the
    /// assembled module does not validate — the cook checked the shader,
    /// so that is a cache from another build or an engine bug; its
    /// materials then draw as the error material.
    pub fn new(
        device: &wgpu::Device,
        ctx: &SurfaceContext<'_>,
        shader: Shader,
    ) -> Result<SurfaceShader, String> {
        let scene_src = scene_source(&shader);
        let shadow_src = shadow_source(&shader);
        fynkel_shader::validate(&scene_src)?;
        fynkel_shader::validate(&shadow_src)?;
        let name = &shader.interface.name;
        let scene = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("fynkel shader {name}")),
            source: wgpu::ShaderSource::Wgsl(scene_src.into()),
        });
        let shadow = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("fynkel shader {name} (shadow)")),
            source: wgpu::ShaderSource::Wgsl(shadow_src.into()),
        });
        let layout = material_layout(device, &shader);
        let pipelines = Pipelines::new(device, ctx, &layout, &scene, &shadow, name);
        Ok(SurfaceShader {
            shader,
            layout,
            scene,
            shadow,
            pipelines,
        })
    }

    /// Rebuilds the pipelines for a new sample count (the layout, and so
    /// every material's bind group, stays).
    pub fn rebuild(&mut self, device: &wgpu::Device, ctx: &SurfaceContext<'_>) {
        if self.pipelines.sample_count != ctx.sample_count {
            self.pipelines = Pipelines::new(
                device,
                ctx,
                &self.layout,
                &self.scene,
                &self.shadow,
                &self.shader.interface.name,
            );
        }
    }

    pub fn pipeline(&self, pass: Pass, skinned: bool, double_sided: bool) -> &wgpu::RenderPipeline {
        let i = skinned as usize * 2 + double_sided as usize;
        let p = &self.pipelines;
        match pass {
            Pass::Opaque => &p.opaque[i],
            Pass::Transparent => &p.transparent[i],
            Pass::Depth => &p.depth[i],
            Pass::Mask => &p.mask[i],
            Pass::Shadow => &p.shadow[skinned as usize],
        }
    }

    /// Whether its materials cast the sun's shadow (`shadows on`).
    pub fn casts_shadows(&self) -> bool {
        self.shader.interface.shadows
    }
}

/// Group 1 for `shader`: the property block at 0, texture property `i`
/// at `1 + 2i` and its sampler at `2 + 2i` — seen by both stages, since
/// `vertex()` may read them.
fn material_layout(device: &wgpu::Device, shader: &Shader) -> wgpu::BindGroupLayout {
    let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
    let mut entries = vec![wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: both,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(shader.layout.size as u64),
        },
        count: None,
    }];
    for i in 0..shader.interface.textures().count() as u32 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 1 + 2 * i,
            visibility: both,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2 + 2 * i,
            visibility: both,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("fynkel shader {} bgl", shader.interface.name)),
        entries: &entries,
    })
}

impl Pipelines {
    fn new(
        device: &wgpu::Device,
        ctx: &SurfaceContext<'_>,
        material: &wgpu::BindGroupLayout,
        scene: &wgpu::ShaderModule,
        shadow: &wgpu::ShaderModule,
        name: &str,
    ) -> Pipelines {
        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fynkel shader pl"),
            bind_group_layouts: &[Some(ctx.frame), Some(material), Some(ctx.skin)],
            ..Default::default()
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fynkel shader shadow pl"),
            bind_group_layouts: &[Some(ctx.shadow), Some(material), Some(ctx.skin)],
            ..Default::default()
        });
        let opaque_target = [Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let blend_target = [Some(hdr_blend_target())];
        let keep_max = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Max,
        };
        let outline_target = [Some(mask_target(keep_max))];
        let label = |what: &str, i: usize| format!("fynkel shader {name} {what} {i}");
        // The scene's four passes over the scene module, as the built-in's:
        // back faces culled unless double-sided, `LessEqual` against the
        // prepass's depth, the transparent pass not writing depth.
        let variant = |i: usize,
                       what: &str,
                       entry: &str,
                       targets: &[Option<wgpu::ColorTargetState>],
                       depth: Option<bool>,
                       samples: u32| {
            let skinned = i >= 2;
            let buffers = vertex_buffers(skinned);
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&label(what, i)),
                layout: Some(&scene_layout),
                vertex: wgpu::VertexState {
                    module: scene,
                    entry_point: Some(if skinned {
                        "fynkel_vs_skinned"
                    } else {
                        "fynkel_vs"
                    }),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: scene,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets,
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: if i % 2 == 1 {
                        None
                    } else {
                        Some(wgpu::Face::Back)
                    },
                    ..Default::default()
                },
                depth_stencil: depth.map(|write| wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let n = ctx.sample_count;
        let shadow_pipeline = |skinned: bool| {
            let buffers = vertex_buffers(skinned);
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&label("shadow", skinned as usize)),
                layout: Some(&shadow_layout),
                vertex: wgpu::VertexState {
                    module: shadow,
                    entry_point: Some(if skinned {
                        "fynkel_shadow_vs_skinned"
                    } else {
                        "fynkel_shadow_vs"
                    }),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: shadow,
                    entry_point: Some("fynkel_shadow_fs"),
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
        Pipelines {
            sample_count: n,
            opaque: std::array::from_fn(|i| {
                variant(i, "opaque", "fynkel_fs", &opaque_target, Some(true), n)
            }),
            transparent: std::array::from_fn(|i| {
                variant(i, "transparent", "fynkel_fs", &blend_target, Some(false), n)
            }),
            depth: std::array::from_fn(|i| {
                variant(i, "depth", "fynkel_fs_depth", &[], Some(true), n)
            }),
            mask: std::array::from_fn(|i| {
                variant(i, "mask", "fynkel_fs_mask", &outline_target, None, 1)
            }),
            shadow: [shadow_pipeline(false), shadow_pipeline(true)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shader(lighting: &str, vertex: bool) -> Shader {
        let mut text = format!(
            "shader \"Test/Surface\" {{\n    lighting {lighting}\n    alpha opaque, mask, blend\n    properties {{\n        tint color = (1, 0.5, 0.25, 1)\n        speed float = 1\n        ripples texture = normal\n    }}\n}}\n\n\
             fn surface(in: SurfaceInput) -> Surface {{\n    var s = default_surface();\n    s.base_color = material.tint.rgb * sample_ripples(in.uv0 + globals.time * material.speed).rgb;\n    s.alpha = material.tint.a;\n    return s;\n}}\n"
        );
        if vertex {
            text.push_str(
                "\nfn vertex(in: VertexInput) -> VertexInput {\n    var v = in;\n    v.position.y += sin(globals.time) * material.speed;\n    return v;\n}\n",
            );
        }
        fynkel_shader::parse(Some("test.shader"), &text).expect("the test shader parses")
    }

    #[test]
    fn every_variant_of_a_shader_validates() {
        for lighting in ["lit", "unlit"] {
            for vertex in [false, true] {
                let s = shader(lighting, vertex);
                assert_eq!(s.has_vertex(), vertex);
                fynkel_shader::validate(&scene_source(&s))
                    .unwrap_or_else(|e| panic!("scene {lighting} {vertex}: {e}"));
                fynkel_shader::validate(&shadow_source(&s))
                    .unwrap_or_else(|e| panic!("shadow {lighting} {vertex}: {e}"));
            }
        }
    }

    /// A shader's own names cannot clash with the engine's because the
    /// cook refuses `ENGINE_NAMES`: that list must hold every top-level
    /// name the renderer writes around a shader (other than `fynkel…`).
    #[test]
    fn the_reserved_names_cover_the_engines_wgsl() {
        let s = shader("lit", true);
        let own = fynkel_shader::top_level_names(&fynkel_shader::assemble(&s, "").source).unwrap();
        for source in [scene_source(&s), shadow_source(&s)] {
            for name in fynkel_shader::top_level_names(&source).unwrap() {
                if own.contains(&name) {
                    continue;
                }
                assert!(
                    fynkel_shader::is_engine_name(&name),
                    "`{name}` is the engine's but not in fynkel_shader::ENGINE_NAMES"
                );
            }
        }
    }
}
