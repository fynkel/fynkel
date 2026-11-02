//! The GPU half of a frame: the passes recorded ([`Renderer::encode`]).

use super::*;

impl Renderer {
    /// The GPU half: writes what [`Renderer::prepare`] built where the GPU
    /// reads it and records the passes into `encoder`, ending in `target`.
    /// The target must be a view in this renderer's format, of the size
    /// given to `prepare`. `encoder` must be submitted before the next
    /// `encode`.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        self.timer.begin();
        let (width, height) = self.size;
        let image = self.image_format;
        self.samples = self.kit.samples(self.quality.samples, &[image.unwrap_or(self.format)]);
        if self.pipelines.samples != self.samples {
            self.pipelines = self.kit.pipelines(self.format, self.samples);
        }
        if self.targets.as_ref().is_none_or(|t| {
            (t.width, t.height, t.samples, t.image.as_ref().map(|i| i.format)) != (width, height, self.samples, image)
        }) {
            self.targets = Some(self.create_targets(width, height, image));
        }
        if let Some(format) = image
            && self.image_pipelines.as_ref().is_none_or(|p| (p.format, p.samples) != (format, self.samples))
        {
            self.image_pipelines = Some(self.kit.pipelines(format, self.samples));
        }
        let effects = self.effect_chain.len();
        if effects > 0 {
            if self.effect_images.as_ref().is_none_or(|e| !e.fit(self.view_size, self.format, effects)) {
                let device = &self.kit.gpu.device;
                self.effect_images =
                    Some(EffectImages::new(&self.kit.effects, device, self.view_size, self.format, effects));
            }
        } else if self.effects_idle > KEEP_UNUSED_MAPS {
            self.effect_images = None;
        }
        if self.shadow_maps.as_ref().is_none_or(|m| m.shape != self.shadow_next) {
            let none = (0, &self.kit.ao.none, &self.kit.ao.sampler);
            self.shadow_maps = Some(ShadowMaps::new(&self.kit.gpu.device, &self.kit.shadows, self.shadow_next, none));
        }
        // Ambient occlusion's images, and the forward pass reading them, or
        // the stand-in.
        if let Some(targets) = &mut self.targets {
            if self.ao.is_some() && targets.ao.is_none() {
                targets.ao = Some(AoImages::new(&self.kit.ao, width, height));
            }
            let ao = match (&self.ao, &mut targets.ao) {
                (Some(_), Some(images)) => {
                    images.set(&self.kit.gpu.queue, self.ao_params);
                    (images.id, &images.blurred, &self.kit.ao.sampler)
                }
                _ => (0, &self.kit.ao.none, &self.kit.ao.sampler),
            };
            // What's behind, for the shaders that read it, and the bind
            // group they read it through.
            if !self.behind_kept {
                targets.behind = None;
            } else if targets.behind.is_none() {
                let format = targets.image.as_ref().map_or(self.format, |i| i.format);
                targets.behind = Some(BehindImages::new(&self.kit, width, height, format, targets.samples));
            }
            if !self.soft_kept {
                targets.soft = None;
            } else if targets.soft.is_none() {
                targets.soft = Some(SoftDepth::new(&self.kit.gpu.device, width, height));
            }
            if let Some(maps) = &mut self.shadow_maps {
                maps.read_ao(&self.kit.gpu.device, &self.kit.shadows, ao);
                if let (true, Some(behind)) = (self.behind, &targets.behind) {
                    maps.read_behind(
                        &self.kit.gpu.device,
                        &self.kit.shadows,
                        ao,
                        (behind.id, &behind.depth, &behind.color),
                    );
                } else if let (true, Some(soft)) = (self.soft, &targets.soft) {
                    // No colour is read: a stand-in.
                    maps.read_behind(
                        &self.kit.gpu.device,
                        &self.kit.shadows,
                        ao,
                        (soft.id, &soft.depth, &self.kit.ao.none),
                    );
                }
            }
        }
        // A new sky's light, worked out before anything reads it.
        if let Some(kind) = &self.sky_next
            && self.environment.as_ref().is_none_or(|e| !e.kind.same_light(kind))
        {
            self.environment = Some(Environment::new(&self.kit.sky, encoder, kind, &self.timer));
        }
        if let (Some(_), Some(image)) = (self.bloom, self.targets.as_mut().and_then(|t| t.image.as_mut()))
            && image.bloom.is_none()
        {
            image.bloom = Some(BloomChain::new(&self.kit.bloom, &image.view, width, height));
        }
        let Self { kit, pipelines, image_pipelines, assets, frame_data, targets, shadow_maps, environment, .. } = self;
        let wait = self.wait_for_shaders;
        let sky_light = match (&self.sky_next, &*environment) {
            (Some(_), Some(environment)) => &environment.light,
            _ => &kit.sky.no_light,
        };
        let sky_image = environment.as_ref().and_then(|e| e.image.as_ref()).unwrap_or(&kit.sky.no_image);
        let targets = targets.as_mut().expect("made above");
        let shadow_maps = shadow_maps.as_ref().expect("made above");

        // Bind groups for texture sets not seen before. A steady frame only
        // looks them up. The world's other views wait while this one
        // records; a mesh or texture they free meanwhile is gone from the
        // next frame on.
        let mut assets = assets::lock(assets);
        let Assets { kit: _, meshes, textures, shaders } = &mut *assets;
        for set in self.batches.iter().map(|b| &b.textures).chain(&self.effect_textures) {
            textures.prepare_own(&kit.gpu.device, &kit.textures, *set);
        }
        for batch in self.caster_batches.iter().filter(|b| b.cutout || b.own.is_some()) {
            match batch.own {
                None => textures.prepare(&kit.gpu.device, &kit.textures, batch.textures),
                Some((_, own)) => textures.prepare_own(&kit.gpu.device, &kit.textures, own),
            }
        }
        let frame = frame_data.write(
            &kit.gpu.device,
            Parts {
                uniforms: bytemuck::bytes_of(&self.uniforms),
                views: &self.shadow_views,
                joints: bytemuck::cast_slice(&self.joints),
                instances: bytemuck::cast_slice(&self.instances),
                lines: bytemuck::cast_slice(&self.line_vertices),
                casters: bytemuck::cast_slice(&self.casters),
                particles: bytemuck::cast_slice(&self.particle_instances),
                cells: self.clusters.bytes(),
            },
        );

        // 0. The shadow maps: each cleared, then its casters' depth.
        let (cascades, spots) = self.shadow_maps_drawn;
        let maps = (0..cascades).map(|c| (c, false, c)).chain((0..spots).map(|k| (MAX_CASCADES as u8 + k, true, k)));
        let mut batches = self.caster_batches.iter().peekable();
        for (map, spot, layer) in maps {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if spot { "spot shadow map" } else { "sun shadow map" }),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: shadow_maps.layer(spot, layer.into()),
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
                if let Some(skin) = skin {
                    pass.set_vertex_buffer(2, skin.buffer.slice(..));
                }
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(mesh.submesh(batch.submesh), 0, batch.first..batch.first + batch.count);
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
                    let Some(pipeline) = pipelines.own_shadow_ready(shader, parts, wait) else { continue };
                    pass.set_pipeline(&pipeline);
                }
                if last.is_none_or(|l| l.own != batch.own) {
                    pass.set_bind_group(1, bind_group, &[]);
                }
                last = Some(batch);
                if let Some(skin) = skin {
                    pass.set_vertex_buffer(2, skin.buffer.slice(..));
                }
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(mesh.submesh(batch.submesh), 0, batch.first..batch.first + batch.count);
            }
        }

        // The solid scene's depth at one sample, into `pass`: for ambient
        // occlusion, and soft particles. Cut-outs and blended draws aren't
        // in it. Those whose shaders move their vertices are drawn with
        // them.
        let solid_depth = |pass: &mut wgpu::RenderPass<'_>| {
            let mut solid = self.batches[..self.blended_from].iter().filter(|b| b.cover == Cover::Opaque).peekable();
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
                        Some(shader) => match pipelines.own_depth_ready(shader, batch.skinned, wait) {
                            Some(made) => pass.set_pipeline(&made),
                            None => {
                                last = None;
                                continue;
                            }
                        },
                    }
                }
                if let Some(skin) = skin {
                    pass.set_vertex_buffer(2, skin.buffer.slice(..));
                }
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(mesh.submesh(batch.submesh), 0, batch.first..batch.first + batch.count);
            }
        };

        // Ambient occlusion: the solid scene's depth, then how hidden each
        // pixel is, blurred.
        if let (Some(_), Some(images)) = (self.ao, &targets.ao) {
            solid_depth(&mut images.depth_pass(&kit.ao, encoder, &self.timer));
            images.encode(&kit.ao, encoder, &self.timer);
        }

        // Soft particles' depth, when what's behind doesn't give it.
        let soft_depth = targets.soft.as_ref().filter(|_| self.soft && !self.behind);
        if let Some(soft) = soft_depth {
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
            solid_depth(&mut pass);
        }

        // 1. The scene, into the scene image or straight into the view.
        // When a see-through surface reads what's behind it, in two: the
        // solid scene and the sky, its depth kept and its colour resolved
        // for them to read; then that put back and the see-through drawn
        // over it.
        let (scene_pipelines, resolve) = match &targets.image {
            Some(image) => (image_pipelines.as_ref().expect("got above"), &image.view),
            None => (&*pipelines, target),
        };
        let behind = targets.behind.as_ref().filter(|_| self.behind);
        let behind_group = behind.and(shadow_maps.behind_bind_group());
        // What soft particles read their depth through: what's behind, or
        // their own depth.
        let soft_group = behind_group.or(soft_depth.and(shadow_maps.behind_bind_group()));
        // The solid scene's depth, for what's behind or a screen effect.
        let kept_depth = targets.behind.as_ref().filter(|_| self.behind || self.effect_depth);
        // Where the solid scene ends up: what's behind's colour, when a
        // shader reads it, or the scene image or the view.
        let scene_resolve = behind.map_or(resolve, |b| &b.color);
        let sky_lit = self.sky_next.is_some();
        let ao = self.ao.is_some();
        let shadowed = self.shadow_maps_drawn != (0, 0);
        let (solid, blended) = self.batches.split_at(self.blended_from);
        // Their surfaces, or with `outlines`, the outlines of those that
        // have them.
        let draw = |pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], outlines: bool| {
            let mut last: Option<&Batch> = None;
            // The maps bound in group 2, or what's behind as well.
            let mut reading = false;
            for batch in batches.iter().filter(|b| b.outlined || !outlines) {
                if let Some(look) = batch.particle {
                    // What follows sets its own pipeline and textures.
                    last = None;
                    let Some(bind_group) = textures.own_bind_group(&batch.textures) else { continue };
                    let shader = match batch.shader {
                        None => None,
                        Some(key) => match shaders.get(&key) {
                            Some(shader) if shader.textures.len() == batch.textures.count as usize => Some(shader),
                            _ => continue,
                        },
                    };
                    let mesh = match look.mesh {
                        false => None,
                        true => match meshes.get(&batch.mesh) {
                            Some(mesh) => Some(mesh),
                            None => continue,
                        },
                    };
                    // Soft only where the depth was kept; a shader that
                    // reads what's behind isn't drawn without it. Where
                    // one does, soft particles read its depth too, so one
                    // group serves both.
                    let reads = shader.is_some_and(|s| s.reads_behind);
                    if reads && behind_group.is_none() {
                        continue;
                    }
                    let look = ParticleLook { soft: look.soft && soft_group.is_some(), ..look };
                    let wants = reads || look.soft;
                    if wants != reading {
                        let group = match (wants, reads) {
                            (true, true) => behind_group,
                            (true, false) => soft_group,
                            (false, _) => Some(&shadow_maps.bind_group),
                        };
                        let Some(group) = group else { continue };
                        pass.set_bind_group(2, group, &[]);
                        reading = wants;
                    }
                    let key = ParticleKey {
                        shader: shader.map(|s| s.hash),
                        look,
                        additive: batch.cover == Cover::Additive,
                        textured: batch.textures.base != MaterialTextures::NONE,
                        shadowed,
                        sky_lit,
                    };
                    let Some(pipeline) = scene_pipelines.particles_ready(shader, key, wait) else { continue };
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(1, bind_group, &[]);
                    let instances = batch.first..batch.first + batch.count;
                    match mesh {
                        // A strip from each point to the next: the next
                        // the same instances, one on.
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
                    continue;
                }
                // Freed since prepare, by another view's render worker.
                let (Some(mesh), Some(bind_group)) =
                    (meshes.get(&batch.mesh), textures.own_bind_group(&batch.textures))
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
                if reads && behind_group.is_none() {
                    continue;
                }
                if reads != reading {
                    let group = if reads { behind_group } else { Some(&shadow_maps.bind_group) };
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
                        let Some(pipeline) = scene_pipelines.outline_ready(shader, parts, wait) else { continue };
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
                        shadowed,
                        textured: textured(batch),
                        sky_lit,
                        // Blended surfaces aren't in its depth.
                        ao: ao && !cover.blended(),
                        skinned: batch.skinned,
                    };
                    match shader {
                        None => pass.set_pipeline(scene_pipelines.forward(variant)),
                        Some(shader) => {
                            let Some(pipeline) = scene_pipelines.own_ready(shader, variant, wait) else { continue };
                            pass.set_pipeline(&pipeline);
                        }
                    }
                }
                if batch.skinned
                    && let Some(skin) = &mesh.skin
                {
                    pass.set_vertex_buffer(2, skin.buffer.slice(..));
                }
                if last.is_none_or(|l| l.textures != batch.textures) {
                    pass.set_bind_group(1, bind_group, &[]);
                }
                last = Some(batch);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(mesh.submesh(batch.submesh), 0, batch.first..batch.first + batch.count);
            }
        };
        // What the draws read: the frame, the maps and the sky's light.
        let bind = |pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], sky: bool, lines: bool| {
            if !batches.is_empty() || lines || sky {
                pass.set_bind_group(0, frame.bind_group, &[]);
            }
            if !batches.is_empty() {
                pass.set_bind_group(2, &shadow_maps.bind_group, &[]);
                pass.set_bind_group(3, sky_light, &[]);
                // Particles alone have no instances.
                if frame.lines > frame.instances {
                    pass.set_vertex_buffer(1, frame.buffer.slice(frame.instances..frame.lines));
                }
            }
        };
        let lines = |pass: &mut wgpu::RenderPass<'_>| {
            if !self.line_vertices.is_empty() {
                pass.set_pipeline(scene_pipelines.lines());
                pass.set_vertex_buffer(0, frame.buffer.slice(frame.lines..));
                pass.draw(0..self.line_vertices.len() as u32, 0..1);
            }
        };
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
                        load: wgpu::LoadOp::Clear(match targets.image {
                            // Shown as picked: not light (above).
                            Some(_) => wgpu::Color { a: 0.0, ..self.clear },
                            None => self.clear,
                        }),
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
                timestamp_writes: self.timer.pass(Part::Scene),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let (drawn, line_drawn) = match behind {
                None => (&self.batches[..], !self.line_vertices.is_empty()),
                Some(_) => (solid, false),
            };
            bind(&mut pass, drawn, self.sky_drawn, line_drawn);
            draw(&mut pass, solid, false);
            draw(&mut pass, solid, true);
            // After the solid scene, so it's drawn only where nothing else
            // is; before the blended, which show it through them.
            if self.sky_drawn {
                pass.set_pipeline(scene_pipelines.sky());
                pass.set_bind_group(1, sky_image, &[]);
                pass.draw(0..3, 0..1);
            }
            if behind.is_none() {
                draw(&mut pass, blended, false);
                lines(&mut pass);
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
                timestamp_writes: self.timer.pass(Part::Scene),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(scene_pipelines.copy_depth());
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
                timestamp_writes: self.timer.pass(Part::Scene),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(scene_pipelines.restore());
            pass.set_bind_group(0, &behind.restore, &[]);
            pass.draw(0..3, 0..1);
            bind(&mut pass, blended, false, !self.line_vertices.is_empty());
            draw(&mut pass, blended, false);
            lines(&mut pass);
        }

        // 2. Bloom's glow, from the scene image.
        if let (Some(bloom), Some(SceneImage { bloom: Some(chain), .. })) = (self.bloom, &mut targets.image) {
            chain.encode(&kit.bloom, &kit.gpu.queue, encoder, bloom, &self.timer);
        }

        // 3. The last pass: the scene image onto the view, or with screen
        // effects into the first's picture.
        let effect_images = self.effect_images.as_ref().filter(|_| effects > 0);
        if let Some(image) = &targets.image {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("finish"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: effect_images.map_or(target, |e| &e.images[0].0),
                    depth_slice: None,
                    resolve_target: None,
                    // Every pixel is written, so what was there is never read.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.timer.pass(Part::Finish),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &image.bind_group, &[]);
            match image.bloom.as_ref().filter(|_| self.bloom.is_some()) {
                Some(chain) => {
                    pass.set_pipeline(pipelines.finish(true));
                    pass.set_bind_group(1, chain.glow(), &[]);
                }
                None => pass.set_pipeline(pipelines.finish(false)),
            }
            pass.draw(0..3, 0..1);
        }

        // 4. The screen effects, passing the picture between their two
        // images, the last onto the view.
        if let Some(images) = effect_images {
            kit.gpu.queue.write_buffer(&images.buffer, 0, &self.effect_uniforms);
            for (i, key) in self.effect_chain.iter().enumerate() {
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
                // Freed or changed since prepare, or not made yet: the
                // picture goes on as it was.
                let shader = shaders.get(key).filter(|s| s.textures.len() == self.effect_textures[i].count as usize);
                let groups = shader.and_then(|s| {
                    let own = textures.own_bind_group(&self.effect_textures[i]).filter(|_| !s.textures.is_empty());
                    let depth = kept_depth.map(|d| &d.effect_depth).filter(|_| s.reads_behind);
                    let whole = s.reads_behind == depth.is_some() && s.textures.is_empty() != own.is_some();
                    whole.then_some((s, own, depth))
                });
                let made = groups.and_then(|(s, own, depth)| Some((pipelines.effect_ready(s, wait)?, own, depth)));
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
                    None => pass.set_pipeline(&pipelines.effect_copy()),
                }
                pass.draw(0..3, 0..1);
            }
        }
        self.timer.end(encoder);
    }

    pub(super) fn create_targets(&self, width: u32, height: u32, image: Option<wgpu::TextureFormat>) -> Targets {
        let make = |label, format| {
            self.kit
                .gpu
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: self.samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    // Cleared and thrown away in the one pass: on tiled
                    // GPUs (phones, Apple) they need no memory at all.
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TRANSIENT_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let device = &self.kit.gpu.device;
        let image = image.map(|format| {
            // Resolved into, then read by the last pass: stored, so it
            // costs memory and bandwidth where the MSAA targets don't.
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("scene image"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene image"),
                layout: &self.kit.image_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
            });
            SceneImage { format, view, bind_group, bloom: None }
        });
        let color_format = image.as_ref().map_or(self.format, |i| i.format);
        Targets {
            width,
            height,
            samples: self.samples,
            color: (self.samples > 1).then(|| make("msaa colour", color_format)),
            depth: make("msaa depth", DEPTH_FORMAT),
            image,
            ao: None,
            behind: None,
            soft: None,
        }
    }
}
