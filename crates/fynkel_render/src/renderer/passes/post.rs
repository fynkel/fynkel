//! The last pass (3): the scene image onto the view, or with screen
//! effects into the first's picture. Then the screen effects (4), passing
//! the picture between their two images, the last onto the view.

use super::*;

impl Recording<'_> {
    /// The scene `image` onto `target`, with bloom's `glow` added.
    pub(in crate::renderer) fn finish(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        image: &SceneImage,
        glow: Option<&BloomChain>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("finish"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                // Every pixel is written, so what was there is never read.
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: self.timer.pass(Part::Finish),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &image.bind_group, &[]);
        match glow {
            Some(chain) => {
                pass.set_pipeline(self.pipelines.finish(true));
                pass.set_bind_group(1, chain.glow(), &[]);
            }
            None => pass.set_pipeline(self.pipelines.finish(false)),
        }
        pass.draw(0..3, 0..1);
    }

    /// The `chain` of effects (each one's shader, own textures, and all
    /// their uniforms), from `images`' first picture, the last onto
    /// `target`. `depth` is the solid scene's, for those that read it.
    pub(in crate::renderer) fn effects(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        images: &EffectImages,
        (chain, own_textures, uniforms): (&[ShaderKey], &[OwnTextures], &[u8]),
        depth: Option<&wgpu::BindGroup>,
    ) {
        let Self { kit, textures, shaders, .. } = self;
        let effects = chain.len();
        kit.gpu.queue.write_buffer(&images.buffer, 0, uniforms);
        for (i, key) in chain.iter().enumerate() {
            let last = i + 1 == effects;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen effect"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: if last { target } else { &images.images[(i + 1) % 2].0 },
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.timer.pass(Part::Effects),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Freed or changed since prepare, or not made yet: the picture
            // goes on as it was.
            let shader = shaders.get(key).filter(|s| s.textures.len() == own_textures[i].count as usize);
            let groups = shader.and_then(|s| {
                let own = textures.own_bind_group(&own_textures[i]).filter(|_| !s.textures.is_empty());
                let depth = depth.filter(|_| s.reads_behind);
                let whole = s.reads_behind == depth.is_some() && s.textures.is_empty() != own.is_some();
                whole.then_some((s, own, depth))
            });
            let made =
                groups.and_then(|(s, own, depth)| Some((self.pipelines.effect_ready(s, self.wait)?, own, depth)));
            let offset = (i as u64 * kit.effects.stride) as u32;
            pass.set_bind_group(0, &images.images[i % 2].1, &[offset]);
            match made {
                Some((pipeline, own, depth)) => {
                    pass.set_pipeline(&pipeline);
                    if let Some(own) = own {
                        pass.set_bind_group(1, own, &[]);
                    }
                    if let Some(depth) = depth {
                        pass.set_bind_group(2, depth, &[]);
                    }
                }
                None => pass.set_pipeline(&self.pipelines.effect_copy()),
            }
            pass.draw(0..3, 0..1);
        }
    }
}
