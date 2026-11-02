//! The solid scene's depth at one sample: for ambient occlusion, and soft
//! particles. Cut-outs and blended draws aren't in it. Those whose shaders
//! move their vertices are drawn with them.

use super::*;

impl Recording<'_> {
    /// The opaque draws of `solid` (the batches before the blended ones)
    /// into `pass`.
    pub(in crate::renderer) fn solid_depth(&self, pass: &mut wgpu::RenderPass<'_>, solid: &[Batch]) {
        let Self { kit, meshes, shaders, frame, .. } = self;
        let mut solid = solid.iter().filter(|b| b.cover == Cover::Opaque).peekable();
        if solid.peek().is_none() {
            return;
        }
        pass.set_bind_group(0, frame.bind_group, &[]);
        pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
        let mut last = None;
        for batch in solid {
            let Some(mesh) = meshes.get(&batch.mesh) else { continue };
            let skin = mesh.skin.as_ref().filter(|_| batch.skinned);
            if batch.skinned && skin.is_none() {
                continue;
            }
            let moving = batch.shader.and_then(|key| shaders.get(&key)).filter(|s| s.moves);
            let pipeline = (batch.skinned, moving.map(|s| s.hash));
            if last != Some(pipeline) {
                last = Some(pipeline);
                match moving {
                    None => pass.set_pipeline(kit.ao.depth_pipeline(batch.skinned)),
                    Some(shader) => match self.pipelines.own_depth_ready(shader, batch.skinned, self.wait) {
                        Some(made) => pass.set_pipeline(&made),
                        None => {
                            last = None;
                            continue;
                        }
                    },
                }
            }
            draw_mesh(pass, mesh, skin, (batch.submesh, batch.first, batch.count));
        }
    }

    /// Soft particles' own depth pass, when what's behind doesn't give it.
    pub(in crate::renderer) fn soft_depth(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        soft: &SoftDepth,
        solid: &[Batch],
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("soft particles' depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &soft.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: self.timer.pass(Part::Scene),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.solid_depth(&mut pass, solid);
    }
}
