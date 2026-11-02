//! The depth-only pipelines of materials whose own shaders move their
//! vertices or cut them out: their shadows, and ambient occlusion's depth.
//! The built-in material's are shadows.rs's and ao.rs's own.

use super::*;

impl Pipelines {
    /// [`Pipelines::own_shadow`] when it's ready.
    pub(in crate::renderer) fn own_shadow_ready(
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

    /// The pipeline that draws the shadow of a material with its own
    /// `shader`, cut out or moving its vertices, into a spot map or a
    /// cascade, bent by skeletons or not: the forward pass's vertices and
    /// instances, moved by its `vertex`, and a cut-out's alpha cutting it
    /// (own_shadow.wgsl).
    pub(in crate::renderer) fn own_shadow(
        &self,
        shader: &CustomShader,
        spot: bool,
        skinned: bool,
        cutout: bool,
    ) -> wgpu::RenderPipeline {
        self.kept(&self.own_shadows, (shader.hash, spot, skinned, cutout), || {
            let (module, _) = self.own_module(shader);
            let label = format!("shadow ({})", shader.name);
            let groups =
                [Some(&self.frame_layout), Some(self.own_textures_layout(shader)), Some(&self.own_view_layout)];
            let layout = self.layout(&label, &groups);
            let buffers = standard_buffers();
            let format = if spot { shadows::SPOT_FORMAT } else { shadows::CASCADE_FORMAT };
            self.maker.make(wgpu::RenderPipelineDescriptor {
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
                // A shadow map's depth isn't reversed: nearer the light is
                // less.
                depth_stencil: Some(wgpu::DepthStencilState {
                    bias: shadows::bias(format),
                    ..depth(format, true, wgpu::CompareFunction::Less)
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
            })
        })
    }

    /// [`Pipelines::own_depth`] when it's ready.
    pub(in crate::renderer) fn own_depth_ready(
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

    /// The pipeline that draws ambient occlusion's depth (ao.rs) of a
    /// material whose own `shader` moves its vertices.
    pub(in crate::renderer) fn own_depth(&self, shader: &CustomShader, skinned: bool) -> wgpu::RenderPipeline {
        self.kept(&self.own_depths, (shader.hash, skinned), || {
            let (module, _) = self.own_module(shader);
            let label = format!("ambient occlusion depth ({})", shader.name);
            // Its `vertex` reads group 0 alone (shader.rs checks).
            let layout = self.layout(&label, &[Some(&self.frame_layout)]);
            let buffers = standard_buffers();
            self.maker.make(wgpu::RenderPipelineDescriptor {
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
                depth_stencil: Some(depth(crate::ao::DEPTH_FORMAT, true, wgpu::CompareFunction::Greater)),
                multisample: Default::default(),
                fragment: None,
                multiview_mask: None,
                cache: None,
            })
        })
    }
}
