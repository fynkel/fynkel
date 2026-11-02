//! The renderer's pipelines, made once each and kept by what they draw.

use super::*;

impl Pipelines {
    pub(crate) fn new(kit: &Kit, format: wgpu::TextureFormat, samples: u32) -> Self {
        Self {
            format,
            samples,
            maker: kit.maker.clone(),
            device: kit.gpu.device.clone(),
            frame_layout: kit.frame_layout.clone(),
            textures_layout: kit.textures.layout.clone(),
            own_textures_layouts: kit.textures.own_layouts.clone(),
            maps_layout: kit.shadows.maps_layout.clone(),
            behind_layout: kit.shadows.behind_layout.clone(),
            image_layout: kit.image_layout.clone(),
            msaa_depth_layout: kit.msaa_depth_layout.clone(),
            sky_light_layout: kit.sky.light_layout.clone(),
            sky_image_layout: kit.sky.image_layout.clone(),
            bloom_layout: kit.bloom.layout.clone(),
            forward_shader: OnceLock::new(),
            forward: std::array::from_fn(|_| OnceLock::new()),
            own_shaders: Mutex::default(),
            own: Mutex::default(),
            own_shadows: Mutex::default(),
            own_depths: Mutex::default(),
            outlines: Mutex::default(),
            own_view_layout: kit.shadows.own_view_layout.clone(),
            lines: OnceLock::new(),
            particle_modules: Mutex::default(),
            particles: Mutex::default(),
            sky: OnceLock::new(),
            behind_shader: OnceLock::new(),
            copy_depth: OnceLock::new(),
            restore: OnceLock::new(),
            finish: OnceLock::new(),
            finish_bloom: OnceLock::new(),
            effect_layout: kit.effects.layout.clone(),
            effect_depth_layout: kit.effects.depth_layout.clone(),
            effects: Mutex::new(HashMap::new()),
            building: Mutex::default(),
            failed: Mutex::default(),
        }
    }

    /// How many of its pipelines are being made on threads of their own.
    pub(crate) fn waiting(&self) -> usize {
        self.building.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// `made`, a pipeline of a material's own shader, if it's made. If
    /// not: with `wait`, made now by `make`; without, `make` runs on a
    /// thread of its own (once, however many frames ask), and the draw
    /// goes without it till then. A driver compiling a shader it hasn't
    /// seen can take a third of a second, which a frame on screen can't.
    pub(super) fn ready(
        self: &Arc<Self>,
        wait: bool,
        key: Building,
        made: Option<wgpu::RenderPipeline>,
        make: impl FnOnce(&Self) -> wgpu::RenderPipeline + Send + 'static,
    ) -> Option<wgpu::RenderPipeline> {
        if made.is_some() {
            return made;
        }
        if wait {
            return Some(make(self));
        }
        if self.failed.lock().unwrap_or_else(PoisonError::into_inner).contains(&key) {
            return None;
        }
        if self.building.lock().unwrap_or_else(PoisonError::into_inner).insert(key) {
            let pipelines = Arc::clone(self);
            let spawned = std::thread::Builder::new().name("fynkel shader".into()).spawn(move || {
                // Error scopes are a thread's own: without these, an error
                // here would reach the device's handler and lose it, for
                // every view (the render thread's scopes don't cover it).
                let memory = pipelines.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
                let validation = pipelines.device.push_error_scope(wgpu::ErrorFilter::Validation);
                make(&pipelines);
                let refused = now(validation.pop()).is_some() | now(memory.pop()).is_some();
                if refused {
                    pipelines.failed.lock().unwrap_or_else(PoisonError::into_inner).insert(key);
                    pipelines.forget(key);
                }
                pipelines.building.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
            });
            if spawned.is_err() {
                // No thread to be had: it's tried again next frame.
                self.building.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
            }
        }
        None
    }

    /// Takes the pipeline under `key` out of its map: one the GPU refused.
    /// A frame that took it in the moment before fails in the render
    /// thread's scopes, rather than losing the device.
    pub(super) fn forget(&self, key: Building) {
        fn out<K: Eq + std::hash::Hash>(map: &Mutex<HashMap<K, wgpu::RenderPipeline>>, key: &K) {
            map.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
        }
        match key {
            Building::Own(hash, variant) => out(&self.own, &(hash, variant)),
            Building::Shadow(hash, spot, skinned, cutout) => out(&self.own_shadows, &(hash, spot, skinned, cutout)),
            Building::Depth(hash, skinned) => out(&self.own_depths, &(hash, skinned)),
            Building::Outline(key) => out(&self.outlines, &key),
            Building::Effect(hash) => out(&self.effects, &hash),
            Building::Particles(key) => out(&self.particles, &key),
        }
    }

    /// [`Pipelines::own`] when it's ready ([`Pipelines::ready`]).
    pub(super) fn own_ready(
        self: &Arc<Self>,
        shader: &Arc<CustomShader>,
        variant: Variant,
        wait: bool,
    ) -> Option<wgpu::RenderPipeline> {
        let key = (shader.hash, variant.index());
        let shader = Arc::clone(shader);
        self.ready(wait, Building::Own(key.0, key.1), made(&self.own, &key), move |p| p.own(&shader, variant))
    }

    /// [`Pipelines::own_shadow`] when it's ready.
    pub(super) fn own_shadow_ready(
        self: &Arc<Self>,
        shader: &Arc<CustomShader>,
        (spot, skinned, cutout): (bool, bool, bool),
        wait: bool,
    ) -> Option<wgpu::RenderPipeline> {
        let key = (shader.hash, spot, skinned, cutout);
        let shader = Arc::clone(shader);
        self.ready(wait, Building::Shadow(key.0, spot, skinned, cutout), made(&self.own_shadows, &key), move |p| {
            p.own_shadow(&shader, spot, skinned, cutout)
        })
    }

    /// [`Pipelines::own_depth`] when it's ready.
    pub(super) fn own_depth_ready(
        self: &Arc<Self>,
        shader: &Arc<CustomShader>,
        skinned: bool,
        wait: bool,
    ) -> Option<wgpu::RenderPipeline> {
        let key = (shader.hash, skinned);
        let shader = Arc::clone(shader);
        self.ready(wait, Building::Depth(key.0, skinned), made(&self.own_depths, &key), move |p| {
            p.own_depth(&shader, skinned)
        })
    }

    /// [`Pipelines::outline`] when it's ready. The built-in material's are
    /// made at once, as its forward pipelines are.
    pub(super) fn outline_ready(
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

    /// The forward pipeline for `variant`.
    pub(super) fn forward(&self, variant: Variant) -> &wgpu::RenderPipeline {
        self.forward[variant.index()].get_or_init(|| self.make_forward(variant))
    }

    pub(super) fn make_forward(&self, variant: Variant) -> wgpu::RenderPipeline {
        let (shader, layout) = self.forward_shader();
        self.make_forward_with(variant, shader, layout, "")
    }

    /// The built-in material's module and its layout, made the first time.
    pub(super) fn forward_shader(&self) -> &(wgpu::ShaderModule, wgpu::PipelineLayout) {
        self.forward_shader.get_or_init(|| {
            let source = concat!(
                include_str!("../skin.wgsl"),
                include_str!("../forward.wgsl"),
                include_str!("../standard.wgsl")
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

    /// The pipeline for `variant` of a material's own shader.
    pub(super) fn own(&self, shader: &CustomShader, variant: Variant) -> wgpu::RenderPipeline {
        let key = (shader.hash, variant.index());
        if let Some(pipeline) = self.own.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let (module, layout) = self.own_module(shader);
        let pipeline = self.make_forward_with(variant, &module, &layout, &shader.name);
        self.own.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// The pipeline that draws the shadow of a material with its own
    /// `shader`, cut out or moving its vertices, into a spot map or a
    /// cascade, bent by skeletons or not: the forward pass's vertices and
    /// instances, moved by its `vertex`, and a cut-out's alpha cutting it
    /// (own_shadow.wgsl).
    pub(super) fn own_shadow(
        &self,
        shader: &CustomShader,
        spot: bool,
        skinned: bool,
        cutout: bool,
    ) -> wgpu::RenderPipeline {
        let key = (shader.hash, spot, skinned, cutout);
        if let Some(pipeline) = self.own_shadows.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let (module, _) = self.own_module(shader);
        let label = format!("shadow ({})", shader.name);
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&label),
            bind_group_layouts: &[
                Some(&self.frame_layout),
                Some(self.own_textures_layout(shader)),
                Some(&self.own_view_layout),
            ],
            immediate_size: 0,
        });
        let vertex_attributes = VERTEX_ATTRIBUTES;
        let instance_attributes = INSTANCE_ATTRIBUTES;
        let skin_attributes = SKIN_ATTRIBUTES;
        let buffers = forward_buffers(&vertex_attributes, &instance_attributes, &skin_attributes);
        let format = if spot { shadows::SPOT_FORMAT } else { shadows::CASCADE_FORMAT };
        let pipeline = self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(match (spot, skinned) {
                    (false, false) => "vs_fy_shadow_cascade",
                    (false, true) => "vs_fy_shadow_cascade_skinned",
                    (true, false) => "vs_fy_shadow",
                    (true, true) => "vs_fy_shadow_skinned",
                }),
                compilation_options: Default::default(),
                buffers: if skinned { &buffers } else { &buffers[..2] },
            },
            // Both sides, as every caster (shadows.rs).
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: shadows::bias(format),
            }),
            multisample: Default::default(),
            // Depth only, unless it's cut out.
            fragment: cutout.then(|| wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_fy_shadow"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.own_shadows.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// The pipeline that draws outlines of draws with `shader` (`None`,
    /// the built-in one), mirrored or not, bent by skeletons or not, with
    /// their mesh's outline normals in vertex buffer 3 or not: their back
    /// faces, pushed out (forward.wgsl, `vs_outline`).
    pub(super) fn outline(
        &self,
        shader: Option<&CustomShader>,
        mirrored: bool,
        skinned: bool,
        normals: bool,
    ) -> wgpu::RenderPipeline {
        let key = (shader.map(|s| s.hash), mirrored, skinned, normals);
        if let Some(pipeline) = self.outlines.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let (module, layout) = match shader {
            Some(shader) => self.own_module(shader),
            None => self.forward_shader().clone(),
        };
        // With normals of its own, they're read in place of the tangents.
        let [position, normal, tangent, uv] = VERTEX_ATTRIBUTES;
        let vertex_attributes: &[wgpu::VertexAttribute] =
            if normals { &[position, normal, uv] } else { &[position, normal, tangent, uv] };
        let instance_attributes = INSTANCE_ATTRIBUTES;
        let skin_attributes = SKIN_ATTRIBUTES;
        let normal_attributes = wgpu::vertex_attr_array![2 => Float32x3];
        let [vertex, instance, skin] = forward_buffers(vertex_attributes, &instance_attributes, &skin_attributes);
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
        let pipeline = self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(if skinned { "vs_outline_skinned" } else { "vs_outline" }),
                compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
                buffers: &buffers[..used],
            },
            // Inside out: its front faces are left out.
            primitive: wgpu::PrimitiveState {
                front_face: if mirrored { wgpu::FrontFace::Cw } else { wgpu::FrontFace::Ccw },
                cull_mode: Some(wgpu::Face::Front),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_outline"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    // Its alpha 0 tells bloom it isn't light; a view (low)
                    // keeps its own.
                    write_mask: if self.format.is_srgb() { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL },
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.outlines.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// The pipeline that draws ambient occlusion's depth (ao.rs) of a
    /// material whose own `shader` moves its vertices.
    pub(super) fn own_depth(&self, shader: &CustomShader, skinned: bool) -> wgpu::RenderPipeline {
        let key = (shader.hash, skinned);
        if let Some(pipeline) = self.own_depths.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let (module, _) = self.own_module(shader);
        let label = format!("ambient occlusion depth ({})", shader.name);
        // Its `vertex` reads group 0 alone (shader.rs checks).
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&label),
            bind_group_layouts: &[Some(&self.frame_layout)],
            immediate_size: 0,
        });
        let vertex_attributes = VERTEX_ATTRIBUTES;
        let instance_attributes = INSTANCE_ATTRIBUTES;
        let skin_attributes = SKIN_ATTRIBUTES;
        let buffers = forward_buffers(&vertex_attributes, &instance_attributes, &skin_attributes);
        let pipeline = self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(if skinned { "vs_fy_depth_skinned" } else { "vs_fy_depth" }),
                compilation_options: Default::default(),
                buffers: if skinned { &buffers } else { &buffers[..2] },
            },
            // As ao.rs's own.
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::ao::DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        self.own_depths.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// A forward pass's module from `source`, and its layout with
    /// `textures` for group 1 and `maps` for group 2.
    pub(super) fn forward_module(
        &self,
        label: &str,
        source: &str,
        textures: &wgpu::BindGroupLayout,
        maps: &wgpu::BindGroupLayout,
    ) -> (wgpu::ShaderModule, wgpu::PipelineLayout) {
        let device = &self.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&self.frame_layout), Some(textures), Some(maps), Some(&self.sky_light_layout)],
            immediate_size: 0,
        });
        (shader, layout)
    }

    pub(super) fn make_forward_with(
        &self,
        variant: Variant,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        name: &str,
    ) -> wgpu::RenderPipeline {
        let Variant { cover, faces, shadowed, textured, sky_lit, ao, skinned } = variant;
        let vertex_attributes = VERTEX_ATTRIBUTES;
        let instance_attributes = INSTANCE_ATTRIBUTES;
        let skin_attributes = SKIN_ATTRIBUTES;
        let buffers = forward_buffers(&vertex_attributes, &instance_attributes, &skin_attributes);
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
            ("textured", on(textured)),
            ("ao", on(ao)),
            ("sky_lit", on(sky_lit)),
            ("dither_step", self.dither_step()),
        ];
        let compilation_options = wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() };
        // Into the scene image, alpha says how much is light (bloom): all
        // of what's drawn, over what's behind as the colour is. Into a
        // view, the target stays opaque.
        let alpha = if self.format.is_srgb() { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL };
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
            // Reversed-Z: near is 1, far is 0, so nearer means greater. Float
            // depth has most precision near 0, which is where reversed-Z puts
            // the distance, where perspective has least.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(!cover.blended()),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: self.samples,
                alpha_to_coverage_enabled: cover == Cover::Cutout,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(entry_point),
                compilation_options,
                targets: &[Some(wgpu::ColorTargetState { format: self.format, blend, write_mask })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// behind.wgsl, reading the kept depth as it's drawn: multisampled,
    /// or with one sample, plain.
    pub(super) fn behind_shader(&self) -> &wgpu::ShaderModule {
        self.behind_shader.get_or_init(|| {
            let source = include_str!("../behind.wgsl");
            let source = if self.samples > 1 {
                source.into()
            } else {
                source.replace("texture_multisampled_2d<f32>", "texture_2d<f32>")
            };
            self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("behind.wgsl"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        })
    }

    /// Copies one sample of the solid scene's multisampled depth into the
    /// depth shaders that read what's behind see.
    pub(super) fn copy_depth(&self) -> &wgpu::RenderPipeline {
        self.copy_depth.get_or_init(|| {
            let shader = self.behind_shader();
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("scene depth"),
                bind_group_layouts: &[Some(if self.samples > 1 {
                    &self.msaa_depth_layout
                } else {
                    &self.image_layout
                })],
                immediate_size: 0,
            });
            self.maker.make(wgpu::RenderPipelineDescriptor {
                label: Some("scene depth"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_depth"),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// Puts the solid scene's resolved colour back into the multisampled
    /// target, every sample, for the see-through draws to go over. The
    /// depth is kept, and left as it is.
    pub(super) fn restore(&self) -> &wgpu::RenderPipeline {
        self.restore.get_or_init(|| {
            let shader = self.behind_shader();
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("what's behind"),
                bind_group_layouts: &[Some(&self.image_layout)],
                immediate_size: 0,
            });
            self.maker.make(wgpu::RenderPipelineDescriptor {
                label: Some("what's behind"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_restore"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// The module particles of the built-in material (`None`) or a
    /// material's own `shader` are drawn with: its forward module with
    /// particles.wgsl after it.
    pub(super) fn particle_module(&self, shader: Option<&CustomShader>) -> wgpu::ShaderModule {
        self.particle_modules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(shader.map(|s| s.hash))
            .or_insert_with(|| {
                let source = match shader {
                    None => concat!(
                        include_str!("../skin.wgsl"),
                        include_str!("../forward.wgsl"),
                        include_str!("../library.wgsl"),
                        include_str!("../helpers.wgsl"),
                        include_str!("../standard.wgsl"),
                        include_str!("../particles.wgsl")
                    )
                    .to_owned(),
                    Some(shader) => format!("{}\n{}", shader.wgsl, crate::shader::PARTICLES),
                };
                let label = shader.map_or_else(|| "particles".to_owned(), |s| format!("particles ({})", s.name));
                self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(&label),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                })
            })
            .clone()
    }

    /// [`Pipelines::particles`] when it's ready. The built-in material's
    /// are made at once.
    pub(super) fn particles_ready(
        self: &Arc<Self>,
        shader: Option<&Arc<CustomShader>>,
        key: ParticleKey,
        wait: bool,
    ) -> Option<wgpu::RenderPipeline> {
        let shader = shader.cloned();
        let wait = wait || shader.is_none();
        self.ready(wait, Building::Particles(key), made(&self.particles, &key), move |p| {
            p.particles(shader.as_deref(), key)
        })
    }

    /// Particles' pipeline for `key`, with the built-in material or a
    /// material's own `shader`.
    pub(super) fn particles(&self, shader: Option<&CustomShader>, key: ParticleKey) -> wgpu::RenderPipeline {
        if let Some(pipeline) = self.particles.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return pipeline.clone();
        }
        let module = self.particle_module(shader);
        let textures = shader.map_or(&self.textures_layout, |s| self.own_textures_layout(s));
        let behind = key.look.soft || shader.is_some_and(|s| s.reads_behind);
        let maps = if behind { &self.behind_layout } else { &self.maps_layout };
        let label = match shader {
            Some(shader) => format!("particles ({})", shader.name),
            None => "particles".to_owned(),
        };
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&label),
            bind_group_layouts: &[Some(&self.frame_layout), Some(textures), Some(maps), Some(&self.sky_light_layout)],
            immediate_size: 0,
        });
        let on = |yes: bool| if yes { 1.0 } else { 0.0 };
        // The built-in material's squares and trails are lit once a
        // particle, in their vertices; meshes, and a material's own
        // shader, pixel by pixel.
        let once = key.look.lit && shader.is_none() && !key.look.mesh;
        // Blended, so not in ambient occlusion's depth.
        let constants = [
            ("textured", on(key.textured)),
            ("particle_lit", on(key.look.lit || shader.is_some())),
            ("particle_lit_once", on(once)),
            ("shadowed", on(key.shadowed)),
            ("sky_lit", on(key.sky_lit)),
            ("ao", 0.0),
            ("double_sided", 0.0),
        ];
        // As the blended surfaces' (`make_forward_with`): premultiplied,
        // tested against the depth but not writing it, and into the scene
        // image their alpha as light.
        let (entry_point, dst_factor) = match (key.additive, key.look.soft) {
            _ if key.look.glyphs => ("fs_fy_glyph", wgpu::BlendFactor::OneMinusSrcAlpha),
            _ if key.look.sprites => ("fs_fy_sprite", wgpu::BlendFactor::OneMinusSrcAlpha),
            (false, false) => ("fs_fy_particle_transparent", wgpu::BlendFactor::OneMinusSrcAlpha),
            (true, false) => ("fs_fy_particle_additive", wgpu::BlendFactor::One),
            (false, true) => ("fs_fy_particle_soft_transparent", wgpu::BlendFactor::OneMinusSrcAlpha),
            (true, true) => ("fs_fy_particle_soft_additive", wgpu::BlendFactor::One),
        };
        let color = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor,
            operation: wgpu::BlendOperation::Add,
        };
        let write_mask = if self.format.is_srgb() { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL };
        let instance = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &PARTICLE_ATTRIBUTES,
        };
        let vertex = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRIBUTES,
        };
        let end = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &TRAIL_END_ATTRIBUTES,
        };
        let mesh_buffers = [Some(vertex), Some(instance.clone())];
        let trail_buffers = [Some(instance.clone()), Some(end)];
        let square_buffers = [Some(instance)];
        let (entry, buffers): (_, &[Option<wgpu::VertexBufferLayout<'_>>]) = match (key.look.mesh, key.look.trail) {
            (true, _) => ("vs_fy_particle_mesh", &mesh_buffers),
            (false, true) => ("vs_fy_trail", &trail_buffers),
            (false, false) => ("vs_fy_particle", &square_buffers),
        };
        let pipeline = self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
                buffers,
            },
            // Squares face the camera; a mesh's backs are hidden by its
            // fronts, as an opaque mesh's would be, mostly.
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: key.look.mesh.then_some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(match key.look.on_top {
                    true => wgpu::CompareFunction::Always,
                    false => wgpu::CompareFunction::Greater,
                }),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: Some(wgpu::BlendState { color, alpha: color }),
                    write_mask,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.particles.lock().unwrap_or_else(PoisonError::into_inner).insert(key, pipeline.clone());
        pipeline
    }

    /// Debug lines' pipeline.
    pub(super) fn lines(&self) -> &wgpu::RenderPipeline {
        self.lines.get_or_init(|| self.make_lines())
    }

    pub(super) fn make_lines(&self) -> wgpu::RenderPipeline {
        // Lines: over everything (depth always passes, and isn't written),
        // blended by their alpha.
        let line_shader = self.device.create_shader_module(wgpu::include_wgsl!("../lines.wgsl"));
        let line_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lines"),
            bind_group_layouts: &[Some(&self.frame_layout)],
            immediate_size: 0,
        });
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    // Colour only: the target stays opaque.
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// One step of the target, for the shaders' `dither_step`: a 10-bit
    /// scene image's, else 0 (the screen's 8 sRGB bits are finest in the
    /// darks already).
    pub(super) fn dither_step(&self) -> f64 {
        if self.format == wgpu::TextureFormat::Rgb10a2Unorm { 1.0 / 1023.0 } else { 0.0 }
    }

    /// The sky's pipeline: one triangle at the far plane, drawn where
    /// nothing is (reversed-Z: the depth is still its clear value, 0).
    pub(super) fn sky(&self) -> &wgpu::RenderPipeline {
        self.sky.get_or_init(|| {
            let shader = self.device.create_shader_module(wgpu::include_wgsl!("../sky.wgsl"));
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky"),
                bind_group_layouts: &[Some(&self.frame_layout), Some(&self.sky_image_layout)],
                immediate_size: 0,
            });
            self.maker.make(wgpu::RenderPipelineDescriptor {
                label: Some("sky"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("dither_step", self.dither_step())],
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.format,
                        blend: None,
                        // Its alpha into the scene image: an image is light,
                        // a gradient isn't (as the forward pass's).
                        write_mask: if self.format.is_srgb() {
                            wgpu::ColorWrites::COLOR
                        } else {
                            wgpu::ColorWrites::ALL
                        },
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// The last pass's pipeline, onto a view (of an sRGB format), adding
    /// bloom's glow or not.
    pub(super) fn finish(&self, bloom: bool) -> &wgpu::RenderPipeline {
        assert!(self.format.is_srgb(), "a view's format is sRGB");
        match bloom {
            false => self.finish.get_or_init(|| self.make_finish(false)),
            true => self.finish_bloom.get_or_init(|| self.make_finish(true)),
        }
    }

    /// A screen effect's pipeline, onto a view's format, made the first
    /// time.
    pub(super) fn effect(&self, shader: &CustomShader) -> wgpu::RenderPipeline {
        if let Some(made) = made(&self.effects, &shader.hash) {
            return made;
        }
        let made = self.make_effect(&shader.name, &shader.wgsl, shader.textures.len(), shader.reads_behind);
        self.effects.lock().unwrap_or_else(PoisonError::into_inner).entry(shader.hash).or_insert(made).clone()
    }

    /// [`Pipelines::effect`] when it's ready ([`Pipelines::ready`]).
    pub(super) fn effect_ready(
        self: &Arc<Self>,
        shader: &Arc<CustomShader>,
        wait: bool,
    ) -> Option<wgpu::RenderPipeline> {
        let key = shader.hash;
        let shader = Arc::clone(shader);
        self.ready(wait, Building::Effect(key), made(&self.effects, &key), move |p| p.effect(&shader))
    }

    /// An effect that changes nothing, for one freed since its frame was
    /// prepared.
    pub(super) fn effect_copy(&self) -> wgpu::RenderPipeline {
        const COPY: &str = concat!(
            include_str!("../effect.wgsl"),
            "@fragment\nfn fs_fy_effect(in: FyOut) -> @location(0) vec4<f32> {\n",
            "    return vec4<f32>(picture_at(vec2<i32>(in.clip.xy)).rgb, 1.0);\n}\n"
        );
        self.effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(0)
            .or_insert_with(|| self.make_effect("copy", COPY, 0, false))
            .clone()
    }

    /// An effect's pipeline: the picture in group 0; its `textures` of its
    /// own in group 1, as a material shader's; the solid scene's depth in
    /// group 2 if it reads it (`depth`).
    pub(super) fn make_effect(&self, name: &str, source: &str, textures: usize, depth: bool) -> wgpu::RenderPipeline {
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let own = textures.checked_sub(1).map(|i| &self.own_textures_layouts[i]);
        let groups = [Some(&self.effect_layout), own, depth.then_some(&self.effect_depth_layout)];
        let used = if depth {
            3
        } else if own.is_some() {
            2
        } else {
            1
        };
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screen effect"),
            bind_group_layouts: &groups[..used],
            immediate_size: 0,
        });
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(name),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fy_effect"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_fy_effect"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    pub(super) fn make_finish(&self, bloom: bool) -> wgpu::RenderPipeline {
        // The last pass, onto a view.
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("finish"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("../neutral.wgsl"), include_str!("../finish.wgsl")).into(),
            ),
        });
        let groups = [Some(&self.image_layout), Some(&self.bloom_layout)];
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("finish"),
            bind_group_layouts: if bloom { &groups } else { &groups[..1] },
            immediate_size: 0,
        });
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some("finish"),
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
                entry_point: Some(if bloom { "fs_bloom" } else { "fs_main" }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}

/// A skinned mesh's joints and weights (`fynkel_mesh::SkinVertex`), as
/// skin.wgsl reads them.
pub(crate) const SKIN_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![14 => Uint16x4, 15 => Unorm16x4];

/// A mesh's position, normal, tangent, and its UV and colour together, as
/// in `fynkel_mesh::Vertex`: the UV's bits and the colour's four bytes as
/// three u32s (forward.wgsl's `uv_color`), as every location is used.
pub(super) const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Uint32x3];

/// An [`Instance`], as forward.wgsl reads it.
pub(super) const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
    7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
    11 => Float32x4, 12 => Uint32, 13 => Float32x4,
];

/// The forward pass's vertex buffers: the mesh's, the instances and a
/// skinned mesh's joints.
pub(super) fn forward_buffers<'a>(
    vertex: &'a [wgpu::VertexAttribute],
    instance: &'a [wgpu::VertexAttribute],
    skin: &'a [wgpu::VertexAttribute],
) -> [Option<wgpu::VertexBufferLayout<'a>>; 3] {
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: vertex,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: INSTANCE_SIZE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: instance,
        }),
        Some(skin_buffer(skin)),
    ]
}

/// The vertex buffer of a skinned mesh's joints and weights: slot 2.
pub(crate) fn skin_buffer(attributes: &[wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'_> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<fynkel_mesh::SkinVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes,
    }
}

/// Which of `shader`'s own textures are sRGB, a bit each.
pub(super) fn srgb_bits(shader: &CustomShader) -> u8 {
    shader
        .textures
        .iter()
        .enumerate()
        .fold(0, |bits, (i, t)| if t.space == ColorSpace::Srgb { bits | 1 << i } else { bits })
}
