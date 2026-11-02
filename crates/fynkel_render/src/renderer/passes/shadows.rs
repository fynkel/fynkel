//! The shadow maps (0): each cleared, then its casters' depth, as its
//! light sees them.

use super::*;

impl Recording<'_> {
    /// The `drawn` maps (sun cascades, spot maps), each with its casters
    /// from `batches`, sorted by map: the plain ones first, then those
    /// with their own shaders.
    pub(in crate::renderer) fn shadow_maps(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        batches: &[CasterBatch],
        drawn: (u8, u8),
    ) {
        let Self { kit, meshes, textures, shaders, frame, .. } = self;
        let (cascades, spots) = drawn;
        let maps = (0..cascades).map(|c| (c, false, c)).chain((0..spots).map(|k| (MAX_CASCADES as u8 + k, true, k)));
        let mut batches = batches.iter().peekable();
        for (map, spot, layer) in maps {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if spot { "spot shadow map" } else { "sun shadow map" }),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.shadow_maps.layer(spot, layer.into()),
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: self.timer.pass(Part::Shadows),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let view = (u64::from(map) * kit.shadows.view_stride) as u32;
            pass.set_bind_group(0, frame.views, &[view]);
            pass.set_vertex_buffer(1, frame.buffer.slice(frame.casters..));
            let mut last: Option<&CasterBatch> = None;
            while let Some(batch) = batches.next_if(|b| b.map == map && b.own.is_none()) {
                let Some(mesh) = meshes.get(&batch.mesh) else { continue };
                if batch.cutout {
                    let Some(bind_group) = textures.bind_group(&batch.textures) else { continue };
                    if last.is_none_or(|l| l.textures != batch.textures || !l.cutout) {
                        pass.set_bind_group(1, bind_group, &[]);
                    }
                }
                let skin = mesh.skin.as_ref().filter(|_| batch.skinned);
                if batch.skinned && skin.is_none() {
                    continue;
                }
                if last.is_none_or(|l| (l.cutout, l.skinned) != (batch.cutout, batch.skinned)) {
                    pass.set_pipeline(kit.shadows.pipeline(spot, batch.cutout, batch.skinned));
                }
                last = Some(batch);
                draw_mesh(&mut pass, mesh, skin, (batch.submesh, batch.first, batch.count));
            }
            // Then those with their own shaders, cut out or moving, which
            // read what the forward pass does, and the map's view in group 2.
            if batches.peek().is_some_and(|b| b.map == map) {
                pass.set_bind_group(0, frame.bind_group, &[]);
                pass.set_bind_group(2, frame.own_views, &[view]);
                pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
            }
            let mut last: Option<&CasterBatch> = None;
            while let Some(batch) = batches.next_if(|b| b.map == map) {
                let Some((Some(key), looks)) = batch.own else { continue };
                // Freed or changed since prepare.
                let (Some(mesh), Some(bind_group), Some(shader)) =
                    (meshes.get(&batch.mesh), textures.own_bind_group(&looks), shaders.get(&key))
                else {
                    continue;
                };
                if shader.textures.len() != looks.count as usize {
                    continue;
                }
                let skin = mesh.skin.as_ref().filter(|_| batch.skinned);
                if batch.skinned && skin.is_none() {
                    continue;
                }
                if last.is_none_or(|l| (l.own, l.skinned, l.cutout) != (batch.own, batch.skinned, batch.cutout)) {
                    let parts = (spot, batch.skinned, batch.cutout);
                    let Some(pipeline) = self.pipelines.own_shadow_ready(shader, parts, self.wait) else { continue };
                    pass.set_pipeline(&pipeline);
                }
                if last.is_none_or(|l| l.own != batch.own) {
                    pass.set_bind_group(1, bind_group, &[]);
                }
                last = Some(batch);
                draw_mesh(&mut pass, mesh, skin, (batch.submesh, batch.first, batch.count));
            }
        }
    }
}
