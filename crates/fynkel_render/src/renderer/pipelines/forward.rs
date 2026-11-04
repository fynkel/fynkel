//! The scene pass's pipelines: surfaces of the built-in material and of
//! materials' own shaders, their outlines, the sky and the debug lines.

use super::*;

impl Pipelines {
    /// The forward pipeline for `variant`.
    pub(in crate::renderer) fn forward(&self, variant: Variant) -> &wgpu::RenderPipeline {
        self.forward[variant.index()].get_or_init(|| {
            let (shader, layout) = self.forward_shader();
            self.make_forward_with(variant, shader, layout, "")
        })
    }

    /// The built-in material's module and its layout, made the first time.
    fn forward_shader(&self) -> &(wgpu::ShaderModule, wgpu::PipelineLayout) {
        self.forward_shader.get_or_init(|| {
            let source = concat!(
                include_str!("../../skin.wgsl"),
                include_str!("../../forward.wgsl"),
                include_str!("../../standard.wgsl")
            );
            self.forward_module("forward", source, &self.textures_layout, &self.maps_layout)
        })
    }

    /// The textures layout of `shader`'s group 1.
    pub(super) fn own_textures_layout(&self, shader: &CustomShader) -> &wgpu::BindGroupLayout {
        match shader.textures.len() {
            0 => &self.textures_layout,
            n => &self.own_textures_layouts[n - 1],
        }
    }

    /// `shader`'s module and its forward pass's layout, made the first
    /// time.
    pub(super) fn own_module(&self, shader: &CustomShader) -> (wgpu::ShaderModule, wgpu::PipelineLayout) {
        self.own_shaders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(shader.hash)
            .or_insert_with(|| {
                let maps = if shader.reads_behind { &self.behind_layout } else { &self.maps_layout };
                self.forward_module(&shader.name, &shader.wgsl, self.own_textures_layout(shader), maps)
            })
            .clone()
    }

    /// [`Pipelines::own`] when it's ready ([`Pipelines::ready`]).
    pub(in crate::renderer) fn own_ready(
        self: &Arc<Self>,
        shader: &Arc<CustomShader>,
        variant: Variant,
        wait: bool,
    ) -> Option<wgpu::RenderPipeline> {
        let key = (shader.hash, variant.index());
        let shader = Arc::clone(shader);
        self.ready(wait, Building::Own(key.0, key.1), made(&self.own, &key), move |p| p.own(&shader, variant))
    }

    /// The pipeline for `variant` of a material's own shader.
    pub(in crate::renderer) fn own(&self, shader: &CustomShader, variant: Variant) -> wgpu::RenderPipeline {
        self.kept(&self.own, (shader.hash, variant.index()), || {
            let (module, layout) = self.own_module(shader);
            self.make_forward_with(variant, &module, &layout, &shader.name)
        })
    }

    /// [`Pipelines::outline`] when it's ready. The built-in material's are
    /// made at once, as its forward pipelines are.
    pub(in crate::renderer) fn outline_ready(
        self: &Arc<Self>,
        shader: Option<&Arc<CustomShader>>,
        (mirrored, skinned, normals): (bool, bool, bool),
        wait: bool,
    ) -> Option<wgpu::RenderPipeline> {
        let key = (shader.map(|s| s.hash), mirrored, skinned, normals);
        let shader = shader.cloned();
        let wait = wait || shader.is_none();
        self.ready(wait, Building::Outline(key), made(&self.outlines, &key), move |p| {
            p.outline(shader.as_deref(), mirrored, skinned, normals)
        })
    }

    /// The pipeline that draws outlines of draws with `shader` (`None`,
    /// the built-in one), mirrored or not, bent by skeletons or not, with
    /// their mesh's outline normals in vertex buffer 3 or not: their back
    /// faces, pushed out (forward.wgsl, `vs_outline`).
    pub(in crate::renderer) fn outline(
        &self,
        shader: Option<&CustomShader>,
        mirrored: bool,
        skinned: bool,
        normals: bool,
    ) -> wgpu::RenderPipeline {
        let key = (shader.map(|s| s.hash), mirrored, skinned, normals);
        self.kept(&self.outlines, key, || {
            let (module, layout) = match shader {
                Some(shader) => self.own_module(shader),
                None => self.forward_shader().clone(),
            };
            // With normals of its own, they're read in place of the tangents.
            let [position, normal, tangent, uv] = VERTEX_ATTRIBUTES;
            let vertex_attributes: &[wgpu::VertexAttribute] =
                if normals { &[position, normal, uv] } else { &[position, normal, tangent, uv] };
            let normal_attributes = wgpu::vertex_attr_array![2 => Float32x3];
            let [vertex, instance, skin] = forward_buffers(vertex_attributes, &INSTANCE_ATTRIBUTES, &SKIN_ATTRIBUTES);
            let normal_buffer = wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &normal_attributes,
            };
            let buffers = [vertex, instance, skin.filter(|_| skinned), normals.then_some(normal_buffer)];
            let used = if normals {
                4
            } else if skinned {
                3
            } else {
                2
            };
            let constants = [("outline_normals", if normals { 1.0 } else { 0.0 })];
            let label = match shader {
                Some(shader) => format!("outline ({})", shader.name),
                None => "outline".to_owned(),
            };
            self.maker.make(wgpu::RenderPipelineDescriptor {
                label: Some(&label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(if skinned { "vs_outline_skinned" } else { "vs_outline" }),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &constants,
                        ..Default::default()
                    },
                    buffers: &buffers[..used],
                },
                // Inside out: its front faces are left out.
                primitive: wgpu::PrimitiveState {
                    front_face: if mirrored { wgpu::FrontFace::Cw } else { wgpu::FrontFace::Ccw },
                    cull_mode: Some(wgpu::Face::Front),
                    ..Default::default()
                },
                // A touch further away (the depth is reversed), so it loses to
                // a surface in the same place: a double-walled mesh's back
                // faces lie exactly on its front ones.
                depth_stencil: Some(wgpu::DepthStencilState {
                    bias: wgpu::DepthBiasState { constant: -4, slope_scale: -1.0, clamp: 0.0 },
                    ..depth(DEPTH_FORMAT, true, wgpu::CompareFunction::Greater)
                }),
                multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_outline"),
                    compilation_options: Default::default(),
                    // Its alpha 0 tells bloom it isn't light.
                    targets: &[target(self.format, None, self.light_mask())],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// A forward pass's module from `source`, and its layout with
    /// `textures` for group 1 and `maps` for group 2.
    fn forward_module(
        &self,
        label: &str,
        source: &str,
        textures: &wgpu::BindGroupLayout,
        maps: &wgpu::BindGroupLayout,
    ) -> (wgpu::ShaderModule, wgpu::PipelineLayout) {
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let groups = [Some(&self.frame_layout), Some(textures), Some(maps), Some(&self.sky_light_layout)];
        (shader, self.layout(label, &groups))
    }

    /// The forward pipeline for `variant` from `shader`, a material's own
    /// (`name`) or the built-in one's (`""`).
    fn make_forward_with(
        &self,
        variant: Variant,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        name: &str,
    ) -> wgpu::RenderPipeline {
        let Variant { cover, faces, shadowed, textured, sky_lit, ao, skinned } = variant;
        let buffers = standard_buffers();
        // Cut-outs get their own pipelines: a shader that can drop pixels
        // costs tile GPUs their hidden-surface removal, so opaque surfaces
        // don't use it. Its alpha becomes MSAA coverage, and it writes
        // colour only, so the target stays opaque. Blended surfaces are
        // tested against the depth but don't write it, so whatever is
        // drawn after them shows through; their colour comes premultiplied
        // by their alpha, and their alpha leaves the target's as it is.
        let (front_face, cull_mode) = match faces {
            Faces::Front => (wgpu::FrontFace::Ccw, Some(wgpu::Face::Back)),
            Faces::Mirrored => (wgpu::FrontFace::Cw, Some(wgpu::Face::Back)),
            Faces::Both => (wgpu::FrontFace::Ccw, None),
        };
        let on = |yes: bool| if yes { 1.0 } else { 0.0 };
        let constants = [
            ("double_sided", on(faces == Faces::Both)),
            ("shadowed", on(shadowed)),
            ("gather_shadows", on(self.gather_shadows)),
            ("textured", on(textured)),
            ("ao", on(ao)),
            ("sky_lit", on(sky_lit)),
            ("dither_step", self.dither_step()),
        ];
        let compilation_options = wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() };
        // Into the scene image, alpha says how much is light (bloom): all
        // of what's drawn, over what's behind as the colour is.
        let alpha = self.light_mask();
        let (label, entry_point, blend, write_mask) = match cover {
            Cover::Opaque => ("forward", "fs_main", None, wgpu::ColorWrites::ALL),
            Cover::Cutout => ("forward cut-out", "fs_cutout", None, alpha),
            Cover::Transparent => (
                "forward transparent",
                "fs_transparent",
                Some(wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                }),
                alpha,
            ),
            Cover::Additive => (
                "forward additive",
                "fs_additive",
                Some(wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                }),
                alpha,
            ),
        };
        let blend = blend.map(|color| wgpu::BlendState { color, alpha: color });
        let label = if name.is_empty() { label.to_owned() } else { format!("{label} ({name})") };
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: if skinned { &buffers } else { &buffers[..2] },
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face,
                cull_mode,
                ..Default::default()
            },
            // Float depth has most precision near 0, which is where
            // reversed-Z puts the distance, where perspective has least.
            depth_stencil: Some(depth(DEPTH_FORMAT, !cover.blended(), wgpu::CompareFunction::Greater)),
            multisample: wgpu::MultisampleState {
                count: self.samples,
                alpha_to_coverage_enabled: cover == Cover::Cutout,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(entry_point),
                compilation_options,
                targets: &[target(self.format, blend, write_mask)],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// The sky's pipeline: one triangle at the far plane, drawn where
    /// nothing is (reversed-Z: the depth is still its clear value, 0).
    pub(in crate::renderer) fn sky(&self) -> &wgpu::RenderPipeline {
        self.sky.get_or_init(|| {
            let shader = self.device.create_shader_module(wgpu::include_wgsl!("../../sky.wgsl"));
            let layout = self.layout("sky", &[Some(&self.frame_layout), Some(&self.sky_image_layout)]);
            self.screen(
                "sky",
                &layout,
                &shader,
                ("vs_main", "fs_main"),
                &[("dither_step", self.dither_step())],
                Some(depth(DEPTH_FORMAT, false, wgpu::CompareFunction::GreaterEqual)),
                self.samples,
                // Its alpha into the scene image: an image is light, a
                // gradient isn't (as the forward pass's).
                target(self.format, None, self.light_mask()),
            )
        })
    }

    /// Debug lines' pipeline: over everything (depth always passes, and
    /// isn't written), blended by their alpha.
    pub(in crate::renderer) fn lines(&self) -> &wgpu::RenderPipeline {
        self.lines.get_or_init(|| {
            let line_shader = self.device.create_shader_module(wgpu::include_wgsl!("../../lines.wgsl"));
            let line_layout = self.layout("lines", &[Some(&self.frame_layout)]);
            let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
            self.maker.make(wgpu::RenderPipelineDescriptor {
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
                depth_stencil: Some(depth(DEPTH_FORMAT, false, wgpu::CompareFunction::Always)),
                multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &line_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    // Colour only: the target stays opaque.
                    targets: &[target(self.format, Some(wgpu::BlendState::ALPHA_BLENDING), wgpu::ColorWrites::COLOR)],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }
}
