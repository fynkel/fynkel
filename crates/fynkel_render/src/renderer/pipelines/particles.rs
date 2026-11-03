//! Particles' pipelines: squares, trails and meshes, of the built-in
//! material or a material's own shader, blended into the scene.

use super::*;

impl Pipelines {
    /// The module particles of the built-in material (`None`) or a
    /// material's own `shader` are drawn with: its forward module with
    /// particles.wgsl after it.
    fn particle_module(&self, shader: Option<&CustomShader>) -> wgpu::ShaderModule {
        self.particle_modules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(shader.map(|s| s.hash))
            .or_insert_with(|| {
                let source = match shader {
                    None => concat!(
                        include_str!("../../skin.wgsl"),
                        include_str!("../../forward.wgsl"),
                        include_str!("../../library.wgsl"),
                        include_str!("../../helpers.wgsl"),
                        include_str!("../../standard.wgsl"),
                        include_str!("../../particles.wgsl")
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
    pub(in crate::renderer) fn particles_ready(
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
    pub(in crate::renderer) fn particles(
        &self,
        shader: Option<&CustomShader>,
        key: ParticleKey,
    ) -> wgpu::RenderPipeline {
        self.kept(&self.particles, key, || {
            let module = self.particle_module(shader);
            let textures = shader.map_or(&self.textures_layout, |s| self.own_textures_layout(s));
            let behind = key.look.soft || shader.is_some_and(|s| s.reads_behind);
            let maps = if behind { &self.behind_layout } else { &self.maps_layout };
            let label = match shader {
                Some(shader) => format!("particles ({})", shader.name),
                None => "particles".to_owned(),
            };
            let groups = [Some(&self.frame_layout), Some(textures), Some(maps), Some(&self.sky_light_layout)];
            let layout = self.layout(&label, &groups);
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
                ("gather_shadows", on(self.gather_shadows)),
                ("sky_lit", on(key.sky_lit)),
                ("ao", 0.0),
                ("double_sided", 0.0),
            ];
            // As the blended surfaces' (`make_forward_with`): premultiplied,
            // tested against the depth but not writing it, and into the
            // scene image their alpha as light.
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
            let compare = if key.look.on_top { wgpu::CompareFunction::Always } else { wgpu::CompareFunction::Greater };
            self.maker.make(wgpu::RenderPipelineDescriptor {
                label: Some(&label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &constants,
                        ..Default::default()
                    },
                    buffers,
                },
                // Squares face the camera; a mesh's backs are hidden by its
                // fronts, as an opaque mesh's would be, mostly.
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: key.look.mesh.then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(depth(DEPTH_FORMAT, false, compare)),
                multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry_point),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &constants,
                        ..Default::default()
                    },
                    targets: &[target(self.format, Some(wgpu::BlendState { color, alpha: color }), self.light_mask())],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }
}
