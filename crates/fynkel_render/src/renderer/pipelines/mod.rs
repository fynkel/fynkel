//! The renderer's pipelines, made the first time a frame draws with them
//! and kept, by the pass they draw in: the scene's surfaces, outlines, the
//! sky and lines (`forward`); the depth-only ones of materials' own
//! shaders, for shadow maps and ambient occlusion (`casters`); particles
//! (`particles`); and the passes over the whole target (`screen`). What
//! they share is here: making one on a thread of its own, keeping it, and
//! the parts every pipeline's description is made of.

use super::*;

mod casters;
mod forward;
mod particles;
mod screen;

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
    pub(in crate::renderer) fn ready(
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
    pub(in crate::renderer) fn forget(&self, key: Building) {
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

    /// The pipeline kept in `map` under `key`, or made by `make` and kept.
    fn kept<K: Eq + std::hash::Hash>(
        &self,
        map: &Mutex<HashMap<K, wgpu::RenderPipeline>>,
        key: K,
        make: impl FnOnce() -> wgpu::RenderPipeline,
    ) -> wgpu::RenderPipeline {
        if let Some(pipeline) = made(map, &key) {
            return pipeline;
        }
        let pipeline = make();
        map.lock().unwrap_or_else(PoisonError::into_inner).entry(key).or_insert(pipeline).clone()
    }

    /// A pipeline layout of bind `groups`, in order.
    fn layout(&self, label: &str, groups: &[Option<&wgpu::BindGroupLayout>]) -> wgpu::PipelineLayout {
        self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: groups,
            immediate_size: 0,
        })
    }

    /// What a surface writes of its colour. Into the scene image its alpha
    /// says how much of it is light (bloom); into a view (low), the target
    /// keeps its own and stays opaque.
    fn light_mask(&self) -> wgpu::ColorWrites {
        if self.format.is_srgb() { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL }
    }

    /// One step of the target, for the shaders' `dither_step`: a 10-bit
    /// scene image's, else 0 (the screen's 8 sRGB bits are finest in the
    /// darks already).
    fn dither_step(&self) -> f64 {
        if self.format == wgpu::TextureFormat::Rgb10a2Unorm { 1.0 / 1023.0 } else { 0.0 }
    }

    /// A pass over the whole target: one triangle, no vertex buffers,
    /// `module`'s `vs_main`-like `vertex` and `fragment` entry points (the
    /// fragment's overridable `constants` set), `samples` a pixel, tested
    /// against the `depth` or not, and writing `target` or no colour.
    #[allow(clippy::too_many_arguments)]
    fn screen(
        &self,
        label: &str,
        layout: &wgpu::PipelineLayout,
        module: &wgpu::ShaderModule,
        (vertex, fragment): (&str, &str),
        constants: &[(&str, f64)],
        depth: Option<wgpu::DepthStencilState>,
        samples: u32,
        target: Option<wgpu::ColorTargetState>,
    ) -> wgpu::RenderPipeline {
        let targets = [target];
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some(vertex),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: depth,
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(fragment),
                compilation_options: wgpu::PipelineCompilationOptions { constants, ..Default::default() },
                targets: if targets[0].is_some() { &targets } else { &[] },
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}

/// A depth test into `format`: whether what passes writes its depth, and
/// how it's compared. Reversed-Z: near is 1, far is 0, so nearer means
/// greater.
fn depth(format: wgpu::TextureFormat, write: bool, compare: wgpu::CompareFunction) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

/// The colour target of `format`, blended by `blend` or not, with
/// `write_mask`.
fn target(
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    write_mask: wgpu::ColorWrites,
) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState { format, blend, write_mask })
}

/// A skinned mesh's joints and weights (`fynkel_mesh::SkinVertex`), as
/// skin.wgsl reads them.
pub(crate) const SKIN_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![14 => Uint16x4, 15 => Unorm16x4];

/// A mesh's position, normal, tangent, and its UV and colour together, as
/// in `fynkel_mesh::Vertex`: the UV's bits and the colour's four bytes as
/// three u32s (forward.wgsl's `uv_color`), as every location is used.
pub(in crate::renderer) const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Uint32x3];

/// An [`Instance`], as forward.wgsl reads it.
pub(in crate::renderer) const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
    7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
    11 => Float32x4, 12 => Uint32, 13 => Float32x4,
];

/// The forward pass's vertex buffers: the mesh's, the instances and a
/// skinned mesh's joints.
pub(in crate::renderer) fn forward_buffers<'a>(
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

/// [`forward_buffers`] with every attribute of the mesh, the instance and
/// the skin, as the forward pass and those drawing its draws read them.
/// A draw not bent by a skeleton uses the first two.
fn standard_buffers() -> [Option<wgpu::VertexBufferLayout<'static>>; 3] {
    forward_buffers(&VERTEX_ATTRIBUTES, &INSTANCE_ATTRIBUTES, &SKIN_ATTRIBUTES)
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
pub(in crate::renderer) fn srgb_bits(shader: &CustomShader) -> u8 {
    shader
        .textures
        .iter()
        .enumerate()
        .fold(0, |bits, (i, t)| if t.space == ColorSpace::Srgb { bits | 1 << i } else { bits })
}
