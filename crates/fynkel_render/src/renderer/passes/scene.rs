//! The scene (1), into the scene image or straight into the view. When a
//! see-through surface reads what's behind it, in two: the solid scene and
//! the sky, its depth kept and its colour resolved for them to read; then
//! that put back and the see-through drawn over it.

use super::*;

impl Scene<'_> {
    /// Every draw of `batches` (those from `blended_from` on blended),
    /// the `sky` image's sky between the solid and the blended, and the
    /// lines on top. `behind` is what's behind's images when a shader
    /// reads them; `kept_depth` the same images when the solid scene's
    /// depth is kept for them or a screen effect. `resolve` is where the
    /// scene ends up, cleared to `clear`.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn draw_scene(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        (behind, kept_depth): (Option<&BehindImages>, Option<&BehindImages>),
        resolve: &wgpu::TextureView,
        clear: wgpu::Color,
        sky: Option<&wgpu::BindGroup>,
        (batches, blended_from): (&[Batch], usize),
    ) {
        let timer = self.rec.timer;
        let lines = self.line_count > 0;
        // Where the solid scene ends up: what's behind's colour, when a
        // shader reads it, or the scene image or the view.
        let scene_resolve = behind.map_or(resolve, |b| &b.color);
        let (solid, blended) = batches.split_at(blended_from);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: targets.color.as_ref().unwrap_or(scene_resolve),
                    depth_slice: None,
                    resolve_target: targets.color.as_ref().map(|_| scene_resolve),
                    // The multisampled colour is only needed until it's
                    // resolved, so it's never stored. On tile GPUs it never
                    // leaves the chip. With one sample it's drawn straight
                    // where it would resolve.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: if targets.color.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                // Kept only for what's behind, or an effect.
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: kept_depth.map_or(&targets.depth, |b| &b.msaa_depth),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: if kept_depth.is_some() { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: timer.pass(Part::Scene),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let (drawn, line_drawn) = match behind {
                None => (batches, lines),
                Some(_) => (solid, false),
            };
            self.bind(&mut pass, drawn, sky.is_some(), line_drawn);
            self.draw(&mut pass, solid, false);
            self.draw(&mut pass, solid, true);
            // After the solid scene, so it's drawn only where nothing else
            // is; before the blended, which show it through them.
            if let Some(sky) = sky {
                pass.set_pipeline(self.pipelines.sky());
                pass.set_bind_group(1, sky, &[]);
                pass.draw(0..3, 0..1);
            }
            if behind.is_none() {
                self.draw(&mut pass, blended, false);
                self.lines(&mut pass);
            }
        }
        // One sample of the solid scene's depth, for what's behind or the
        // effects to read: blended surfaces don't write it, so it's the
        // same before them as after.
        if let Some(kept) = kept_depth {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &kept.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: timer.pass(Part::Scene),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(self.pipelines.copy_depth());
            pass.set_bind_group(0, &kept.copy, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(behind) = behind {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene, see-through"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: targets.color.as_ref().unwrap_or(resolve),
                    depth_slice: None,
                    resolve_target: targets.color.as_ref().map(|_| resolve),
                    // Every sample is written, by what's behind put back.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: if targets.color.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &behind.msaa_depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: timer.pass(Part::Scene),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(self.pipelines.restore());
            pass.set_bind_group(0, &behind.restore, &[]);
            pass.draw(0..3, 0..1);
            self.bind(&mut pass, blended, false, lines);
            self.draw(&mut pass, blended, false);
            self.lines(&mut pass);
        }
    }

    /// What the draws read: the frame, the maps and the sky's light.
    fn bind(&self, pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], sky: bool, lines: bool) {
        let frame = self.rec.frame;
        if !batches.is_empty() || lines || sky {
            pass.set_bind_group(0, frame.bind_group, &[]);
        }
        if !batches.is_empty() {
            pass.set_bind_group(2, &self.rec.shadow_maps.bind_group, &[]);
            pass.set_bind_group(3, self.sky_light, &[]);
            // Particles alone have no instances.
            if frame.lines > frame.instances {
                pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
            }
        }
    }

    /// The debug lines.
    fn lines(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.line_count > 0 {
            pass.set_pipeline(self.pipelines.lines());
            pass.set_vertex_buffer(0, self.rec.frame.buffer.slice(self.rec.frame.lines..));
            pass.draw(0..self.line_count, 0..1);
        }
    }

    /// `batches`' surfaces, or with `outlines`, the outlines of those that
    /// have them.
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], outlines: bool) {
        let Recording { meshes, textures, shaders, shadow_maps, wait, .. } = self.rec;
        let wait = *wait;
        let mut last: Option<&Batch> = None;
        // The maps bound in group 2, or what's behind as well.
        let mut reading = false;
        for batch in batches.iter().filter(|b| b.outlined || !outlines) {
            if let Some(look) = batch.particle {
                // What follows sets its own pipeline and textures.
                last = None;
                self.particles(pass, batch, look, &mut reading);
                continue;
            }
            // Freed since prepare, by another view's render worker.
            let (Some(mesh), Some(bind_group)) = (meshes.get(&batch.mesh), textures.own_bind_group(&batch.textures))
            else {
                continue;
            };
            // Its mesh put back without joints since prepare.
            if batch.skinned && mesh.skin.is_none() {
                continue;
            }
            // Its shader freed or changed since prepare.
            let shader = match batch.shader {
                None => None,
                Some(key) => match shaders.get(&key) {
                    Some(shader) if shader.textures.len() == batch.textures.count as usize => Some(shader),
                    _ => continue,
                },
            };
            let reads = shader.is_some_and(|s| s.reads_behind);
            // Changed since prepare: what it reads isn't there (soft
            // particles' own depth may be bound, with no colour).
            if reads && self.behind_group.is_none() {
                continue;
            }
            if reads != reading {
                let group = if reads { self.behind_group } else { Some(&shadow_maps.bind_group) };
                let Some(group) = group else {
                    continue;
                };
                pass.set_bind_group(2, group, &[]);
                reading = reads;
            }
            let textured = |b: &Batch| b.textures.base != MaterialTextures::NONE;
            let pipeline = |b: &Batch| (b.cover, b.faces, textured(b), b.skinned, b.shader);
            if outlines {
                // Its mesh's outline normals or not choose it too.
                if last.is_none_or(|l| pipeline(l) != pipeline(batch) || l.mesh != batch.mesh) {
                    let mirrored = batch.faces == Faces::Mirrored;
                    let normals = mesh.outline_normals.is_some();
                    let parts = (mirrored, batch.skinned, normals);
                    let Some(pipeline) = self.pipelines.outline_ready(shader, parts, wait) else { continue };
                    pass.set_pipeline(&pipeline);
                }
                if let Some(normals) = &mesh.outline_normals {
                    pass.set_vertex_buffer(3, normals.slice(..));
                }
            } else if last.is_none_or(|l| pipeline(l) != pipeline(batch)) {
                let (cover, faces) = (batch.cover, batch.faces);
                let variant = Variant {
                    cover,
                    faces,
                    shadowed: self.shadowed,
                    textured: textured(batch),
                    sky_lit: self.sky_lit,
                    // Blended surfaces aren't in its depth.
                    ao: self.ao && !cover.blended(),
                    skinned: batch.skinned,
                };
                match shader {
                    None => pass.set_pipeline(self.pipelines.forward(variant)),
                    Some(shader) => {
                        let Some(pipeline) = self.pipelines.own_ready(shader, variant, wait) else { continue };
                        pass.set_pipeline(&pipeline);
                    }
                }
            }
            if last.is_none_or(|l| l.textures != batch.textures) {
                pass.set_bind_group(1, bind_group, &[]);
            }
            last = Some(batch);
            let skin = mesh.skin.as_ref().filter(|_| batch.skinned);
            draw_mesh(pass, mesh, skin, (batch.submesh, batch.first, batch.count));
        }
    }

    /// A batch of particles, `look` choosing their pipeline. `reading` is
    /// whether what's behind (or soft particles' depth) is bound in group
    /// 2, and changes with it.
    fn particles(&self, pass: &mut wgpu::RenderPass<'_>, batch: &Batch, look: ParticleLook, reading: &mut bool) {
        let Recording { meshes, textures, shaders, shadow_maps, frame, wait, .. } = self.rec;
        let Some(bind_group) = textures.own_bind_group(&batch.textures) else { return };
        let shader = match batch.shader {
            None => None,
            Some(key) => match shaders.get(&key) {
                Some(shader) if shader.textures.len() == batch.textures.count as usize => Some(shader),
                _ => return,
            },
        };
        let mesh = match look.mesh {
            false => None,
            true => match meshes.get(&batch.mesh) {
                Some(mesh) => Some(mesh),
                None => return,
            },
        };
        // Soft only where the depth was kept; a shader that reads what's
        // behind isn't drawn without it. Where one does, soft particles
        // read its depth too, so one group serves both.
        let reads = shader.is_some_and(|s| s.reads_behind);
        if reads && self.behind_group.is_none() {
            return;
        }
        let look = ParticleLook { soft: look.soft && self.soft_group.is_some(), ..look };
        let wants = reads || look.soft;
        if wants != *reading {
            let group = match (wants, reads) {
                (true, true) => self.behind_group,
                (true, false) => self.soft_group,
                (false, _) => Some(&shadow_maps.bind_group),
            };
            let Some(group) = group else { return };
            pass.set_bind_group(2, group, &[]);
            *reading = wants;
        }
        let key = ParticleKey {
            shader: shader.map(|s| s.hash),
            look,
            additive: batch.cover == Cover::Additive,
            textured: batch.textures.base != MaterialTextures::NONE,
            shadowed: self.shadowed,
            sky_lit: self.sky_lit,
        };
        let Some(pipeline) = self.pipelines.particles_ready(shader, key, *wait) else { return };
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(1, bind_group, &[]);
        let instances = batch.first..batch.first + batch.count;
        match mesh {
            // A strip from each point to the next: the next the same
            // instances, one on.
            None if look.trail => {
                let next = frame.particles + std::mem::size_of::<ParticleInstance>() as u64;
                pass.set_vertex_buffer(0, frame.buffer.slice(frame.particles..));
                pass.set_vertex_buffer(1, frame.buffer.slice(next..));
                pass.draw(0..6, batch.first..batch.first + batch.count.saturating_sub(1));
                // The draws' instances back where they were.
                if frame.lines > frame.instances {
                    pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
                }
            }
            None => {
                pass.set_vertex_buffer(0, frame.buffer.slice(frame.particles..));
                pass.draw(0..6, instances);
            }
            Some(mesh) => {
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, frame.buffer.slice(frame.particles..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, instances);
                // The draws' instances back where they were.
                if frame.lines > frame.instances {
                    pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
                }
            }
        }
    }
}
