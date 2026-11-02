//! The GPU half of a frame ([`Renderer::encode`]): the images it needs
//! made, then its passes recorded in order, each in `passes`.

use super::passes::{Recording, Scene};
use super::*;

impl Renderer {
    /// The GPU half: writes what [`Renderer::prepare`] built where the GPU
    /// reads it and records the passes into `encoder`, ending in `target`.
    /// The target must be a view in this renderer's format, of the size
    /// given to `prepare`. `encoder` must be submitted before the next
    /// `encode`.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        self.timer.begin();
        self.make_images(encoder);
        let effects = self.effect_chain.len();
        let Self { kit, pipelines, image_pipelines, assets, frame_data, targets, shadow_maps, environment, .. } = self;
        let sky_light = match (&self.sky_next, &*environment) {
            (Some(_), Some(environment)) => &environment.light,
            _ => &kit.sky.no_light,
        };
        let sky_image = environment.as_ref().and_then(|e| e.image.as_ref()).unwrap_or(&kit.sky.no_image);
        let targets = targets.as_mut().expect("made in make_images");
        let shadow_maps = shadow_maps.as_ref().expect("made in make_images");

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
        let rec = Recording {
            kit,
            pipelines,
            meshes,
            textures,
            shaders,
            frame: &frame,
            shadow_maps,
            timer: &self.timer,
            wait: self.wait_for_shaders,
        };

        // 0. The shadow maps.
        rec.shadow_maps(encoder, &self.caster_batches, self.shadow_maps_drawn);

        // Ambient occlusion: the solid scene's depth, then how hidden each
        // pixel is, blurred.
        let solid = &self.batches[..self.blended_from];
        if let (Some(_), Some(images)) = (self.ao, &targets.ao) {
            rec.solid_depth(&mut images.depth_pass(&kit.ao, encoder, &self.timer), solid);
            images.encode(&kit.ao, encoder, &self.timer);
        }
        // Soft particles' depth, when what's behind doesn't give it.
        let soft_depth = targets.soft.as_ref().filter(|_| self.soft && !self.behind);
        if let Some(soft) = soft_depth {
            rec.soft_depth(encoder, soft, solid);
        }

        // 1. The scene, into the scene image or straight into the view.
        let (scene_pipelines, resolve) = match &targets.image {
            Some(image) => (image_pipelines.as_ref().expect("made in make_images"), &image.view),
            None => (&*pipelines, target),
        };
        let behind = targets.behind.as_ref().filter(|_| self.behind);
        let behind_group = behind.and(shadow_maps.behind_bind_group());
        let scene = Scene {
            rec: &rec,
            pipelines: scene_pipelines,
            behind_group,
            // What soft particles read their depth through: what's behind,
            // or their own depth.
            soft_group: behind_group.or(soft_depth.and(shadow_maps.behind_bind_group())),
            sky_light,
            sky_lit: self.sky_next.is_some(),
            ao: self.ao.is_some(),
            shadowed: self.shadow_maps_drawn != (0, 0),
            line_count: self.line_vertices.len() as u32,
        };
        // The solid scene's depth, for what's behind or a screen effect.
        let kept_depth = targets.behind.as_ref().filter(|_| self.behind || self.effect_depth);
        let clear = match targets.image {
            // Shown as picked: not light (`Renderer` above).
            Some(_) => wgpu::Color { a: 0.0, ..self.clear },
            None => self.clear,
        };
        let sky = self.sky_drawn.then_some(sky_image);
        scene.draw_scene(
            encoder,
            targets,
            (behind, kept_depth),
            resolve,
            clear,
            sky,
            (&self.batches, self.blended_from),
        );

        // 2. Bloom's glow, from the scene image.
        if let (Some(bloom), Some(SceneImage { bloom: Some(chain), .. })) = (self.bloom, &mut targets.image) {
            chain.encode(&kit.bloom, &kit.gpu.queue, encoder, bloom, &self.timer);
        }

        // 3. The last pass: the scene image onto the view, or with screen
        // effects into the first's picture.
        let effect_images = self.effect_images.as_ref().filter(|_| effects > 0);
        if let Some(image) = &targets.image {
            let glow = image.bloom.as_ref().filter(|_| self.bloom.is_some());
            rec.finish(encoder, effect_images.map_or(target, |e| &e.images[0].0), image, glow);
        }

        // 4. The screen effects, passing the picture between their two
        // images, the last onto the view.
        if let Some(images) = effect_images {
            let depth = kept_depth.map(|d| &d.effect_depth);
            rec.effects(
                encoder,
                target,
                images,
                (&self.effect_chain, &self.effect_textures, &self.effect_uniforms),
                depth,
            );
        }
        self.timer.end(encoder);
    }

    /// What this frame draws into, made or made again for its size, level
    /// and what it draws: the targets, the scene image's pipelines, the
    /// effects', shadow maps', ambient occlusion's, what's behind's and soft
    /// particles' images, the sky's light and bloom's chain.
    fn make_images(&mut self, encoder: &mut wgpu::CommandEncoder) {
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
