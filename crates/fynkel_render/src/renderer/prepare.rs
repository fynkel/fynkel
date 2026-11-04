//! The CPU half of a frame: the scene turned into draws, instances and
//! the frame's uniforms ([`Renderer::prepare`]).

use super::*;

impl Renderer {
    /// The CPU half of a frame: builds per-draw data and the frame uniforms
    /// for a `width` × `height` target. Makes no GPU calls, and allocates
    /// nothing once its buffers have grown to the scene's size.
    pub fn prepare(&mut self, scene: &RenderScene, width: u32, height: u32) -> FrameStats {
        let view_size = (width.max(1), height.max(1));
        self.size = view_size;
        self.view_size = view_size;
        self.effect_chain.clear();
        self.effect_uniforms.clear();
        self.effect_textures.clear();
        self.effect_depth = false;
        self.quality = scene.quality.unwrap_or(self.kit.device_quality().quality());
        self.image_format = self.quality.image_format();
        self.joints.clear();
        self.skinned.clear();
        self.looks.clear();
        self.moving.clear();
        self.instances.clear();
        self.batches.clear();
        self.blended_from = 0;
        self.line_vertices.clear();
        self.particle_instances.clear();
        self.shadow_candidates.clear();
        self.caster_order.clear();
        self.casters.clear();
        self.caster_batches.clear();
        self.shadow_maps_drawn = (0, 0);
        self.shadow_shape = [None; 2];
        self.sky_next = None;
        self.sky_drawn = false;
        self.bloom = None;
        self.ao = None;
        self.behind = false;
        self.soft = false;
        let mut stats = FrameStats {
            shaders_waiting: (self.pipelines.waiting() + self.image_pipelines.as_ref().map_or(0, |p| p.waiting()))
                as u32,
            ..FrameStats::default()
        };

        let Some(mut camera) = scene.camera else {
            self.clear = wgpu::Color::BLACK;
            return stats;
        };
        // The view's shape, whatever size the scene is drawn at.
        let aspect = view_size.0 as f32 / view_size.1 as f32;
        // The pixel look: drawn small, into an image even at low, and the
        // camera on the picture's pixels.
        if let Some(pixels) = camera.pixels.map(|p| p.clamp(1, view_size.1)) {
            self.size = (((pixels as f32 * aspect).round() as u32).max(1), pixels);
            self.image_format.get_or_insert(self.format);
            snap_to_pixels(&mut camera, pixels);
        }
        self.bloom = camera.bloom.filter(|b| self.quality.bloom && b.strength > 0.0);
        let [r, g, b] = camera.clear_color.map(f64::from);
        // Linear: the sRGB target encodes it like everything else.
        self.clear = wgpu::Color { r, g, b, a: 1.0 };

        // Near and far swapped: reversed-Z.
        let projection = match camera.orthographic {
            Some(half) => glam::camera::rh::proj::directx::orthographic(
                -half * aspect,
                half * aspect,
                -half,
                half,
                camera.far,
                camera.near,
            ),
            None => glam::camera::rh::proj::directx::perspective(camera.fov_y, aspect, camera.far, camera.near),
        };
        // Orthographic: every surface is seen from the same direction, so
        // the shader gets that direction (w = 0) rather than a position.
        let to_viewer = match camera.orthographic {
            Some(_) => camera.view.inverse().transform_vector3(Vec3::Z).normalize().extend(0.0),
            None => camera.position.extend(1.0),
        };
        self.ao =
            camera.ambient_occlusion.filter(|a| self.quality.ambient_occlusion && a.intensity > 0.0 && a.radius > 0.0);
        if let Some(ao) = self.ao {
            self.ao_params = ao::Params::new(ao, projection, camera.orthographic.is_none(), self.size.1);
        }
        let planes = frustum_planes(projection * camera.view);
        let u = &mut self.uniforms;
        u.view_proj = (projection * camera.view).to_cols_array_2d();
        u.camera_position = to_viewer.into();
        u.ambient = scene.ambient.extend(0.0).into();
        u.view_size = [1.0 / self.size.0 as f32, 1.0 / self.size.1 as f32, 0.0, 0.0];
        u.time = [scene.time[0], scene.time[1], 0.0, 0.0];

        // The sky: a plain one is the clear colour and more ambient light;
        // a gradient or an image is drawn after the scene and lights it
        // from its cubes.
        u.sky = SkyUniform {
            inverse_view_proj: (projection * camera.view).inverse().to_cols_array_2d(),
            ..SkyUniform::zeroed()
        };
        u.sky_light = [0.0, (SPECULAR_MIPS - 1) as f32, 1.0, 0.0];
        let mut horizon = Vec3::from(camera.clear_color);
        if let Some(sky) = &scene.sky {
            match &sky.kind {
                SkyKind::Color(c) => {
                    u.ambient = (scene.ambient + *c * sky.lighting).extend(0.0).into();
                    if camera.sky {
                        let [r, g, b] = c.to_array().map(f64::from);
                        self.clear = wgpu::Color { r, g, b, a: 1.0 };
                    }
                    horizon = *c;
                }
                SkyKind::Gradient { zenith, horizon: level, ground } => {
                    u.sky.zenith = zenith.extend(1.0).into();
                    u.sky.horizon = level.extend(0.0).into();
                    u.sky.ground = ground.extend(0.0).into();
                    horizon = *level;
                }
                SkyKind::Image { image, intensity, rotation } => {
                    u.sky.zenith = [0.0, 0.0, 0.0, 2.0];
                    u.sky.horizon = [0.0, 0.0, 0.0, *intensity];
                    u.sky.ground = [0.0, 0.0, 0.0, *rotation];
                    horizon = sky::tonemap_neutral(image.horizon() * *intensity);
                }
            }
            stats.sky_bytes = sky::bytes(&sky.kind);
            if !matches!(sky.kind, SkyKind::Color(_)) {
                // An image's light is worked out unturned at intensity 1, a
                // gradient's for unit colours.
                let (colors, rotation) = match sky.kind {
                    SkyKind::Image { intensity, rotation, .. } => {
                        (Mat3::from_diagonal(Vec3::splat(intensity)), rotation)
                    }
                    SkyKind::Gradient { zenith, horizon, ground } => (Mat3::from_cols(zenith, horizon, ground), 0.0),
                    SkyKind::Color(_) => unreachable!("tested above"),
                };
                let (sin, cos) = rotation.sin_cos();
                u.sky_light = [sky.lighting, (SPECULAR_MIPS - 1) as f32, cos, sin];
                u.sky_colors = [colors.x_axis, colors.y_axis, colors.z_axis].map(|c| c.extend(0.0).into());
                stats.sky_light_made = self.environment.as_ref().is_none_or(|e| !e.kind.same_light(&sky.kind));
                self.sky_next = Some(sky.kind.clone());
                self.sky_drawn = camera.sky;
            }
        }
        u.fog_color = [0.0; 4];
        if let Some(fog) = scene.fog {
            u.fog_color = fog.color.unwrap_or(horizon).extend(1.0).into();
            let height = fog.height.filter(|h| *h > 0.0).map_or(0.0, |h| 1.0 / h);
            u.fog = [fog.start, 1.0 / (fog.end - fog.start).max(1e-4), height, 0.0];
        }

        // The suns, in the scene's order; one with no direction lights
        // nothing.
        let mut suns = 0;
        let mut shadow_sun = None;
        for light in &scene.lights {
            let LightKind::Sun { direction } = light.kind else { continue };
            let to_sun = (-direction).normalize_or_zero();
            if to_sun == Vec3::ZERO {
                continue;
            }
            if suns == MAX_SUNS {
                stats.lights_left_out += 1;
                continue;
            }
            u.suns[suns] = SunUniform { to_sun: to_sun.extend(0.0).into(), color: light.color.extend(0.0).into() };
            if light.shadows && shadow_sun.is_none() {
                shadow_sun = Some((suns, -to_sun));
            }
            suns += 1;
        }
        let highlights = u32::from(self.quality.lamp_highlights);
        u.counts = [suns as u32, shadow_sun.map_or(NO_SHADOWS, |(i, _)| i as u32), highlights, 0];

        // The point and spot lights that can light what's seen: their reach
        // is in view, and they give some light. Past the cap, the ones
        // nearest the camera.
        self.in_view_lights.clear();
        for (i, light) in scene.lights.iter().enumerate() {
            let Some((position, range)) = light.reach() else { continue };
            if range > 0.0 && light.color.max_element() > 0.0 && in_view(&planes, position, range) {
                self.in_view_lights.push((position.distance_squared(camera.position), i as u32));
            }
        }
        if self.in_view_lights.len() > MAX_LIGHTS {
            stats.lights_left_out += (self.in_view_lights.len() - MAX_LIGHTS) as u32;
            self.in_view_lights.select_nth_unstable_by(MAX_LIGHTS, |a, b| a.0.total_cmp(&b.0));
            self.in_view_lights.truncate(MAX_LIGHTS);
        }
        self.frame_lights.clear();
        for (slot, &(_, i)) in self.in_view_lights.iter().enumerate() {
            let light = &scene.lights[i as usize];
            u.lights[slot] = light_uniform(light);
            self.frame_lights.push(Reach::of(light));
        }
        stats.lights = self.frame_lights.len() as u32;
        // Particles lit once each go through every lamp (particles.wgsl).
        u.counts[3] = stats.lights;
        // The cells: their slices end where the furthest lamp's reach does,
        // rounded up a quarter octave at a time so a moving camera seldom
        // changes the grid's shape (`Clusters` keeps it while it holds).
        let forward = camera.view.inverse().transform_vector3(Vec3::NEG_Z).normalize();
        let reach = self
            .frame_lights
            .iter()
            .map(|l| (l.position - camera.position).dot(forward) + l.range)
            .fold(camera.near * 2.0, f32::max);
        let quarters = ((reach / camera.near).log2() * 4.0).ceil();
        let far = (camera.near * (quarters * 0.25).exp2()).min(camera.far.max(camera.near * 2.0));
        let half = match camera.orthographic {
            Some(h) => (h * aspect, h),
            None => {
                let t = (camera.fov_y * 0.5).tan();
                (t * aspect, t)
            }
        };
        let grid =
            Grid { view: camera.view, half, orthographic: camera.orthographic.is_some(), near: camera.near, far };
        self.clusters.build(&grid, &self.frame_lights);
        let (scale, offset) = grid.slicing();
        u.cells = [TILES_X as f32 / self.size.0 as f32, TILES_Y as f32 / self.size.1 as f32, scale, offset];
        u.cells_eye = camera.position.extend(if grid.orthographic { 0.0 } else { 1.0 }).into();
        u.cells_forward = forward.extend(0.0).into();

        // Leave out what the camera can't see, then group the solid rest by
        // pipeline, textures and mesh, so each run of the same three is one
        // instanced draw call and the state changes least between calls.
        // Blended draws go furthest first, by their centres, each over what
        // it's in front of; a run of the same three is one call there too.
        // Each draw's key, normal matrix and lights are worked out once.
        // The sorts are in place and the buffers reused, so a steady frame
        // allocates nothing.
        self.order.clear();
        self.blended.clear();
        self.joints.extend(scene.joints.iter().map(|m| {
            let rows = m.transpose();
            [rows.x_axis.into(), rows.y_axis.into(), rows.z_axis.into()]
        }));
        // Materials' values go after the joints, where their shaders read
        // them.
        let values = self.joints.len() as u32;
        self.values_at = values;
        self.joints.extend_from_slice(&scene.shader_values);
        let assets = assets::lock(&self.assets);
        for (i, draw) in scene.draws.iter().enumerate() {
            let Some(mesh) = assets.meshes.get(&draw.mesh) else {
                stats.missing_meshes += 1;
                self.skinned.push(false);
                self.looks.push(None);
                self.moving.push(false);
                continue;
            };
            // Bent only by a skin the scene has all of: a joint number past
            // its end would read another skeleton's, or past the buffer.
            let skin = match (draw.skin, &mesh.skin) {
                (Some(first), Some(skin)) if first as usize + skin.joints.len() <= scene.joints.len() => {
                    Some((first as usize, skin))
                }
                _ => None,
            };
            self.skinned.push(skin.is_some());
            let m = &draw.material;
            let mut moving = false;
            let mut reads_behind = false;
            let look = match &m.shader {
                None => Some((None, OwnTextures::of(m.textures))),
                // A screen effect's shader draws no object.
                Some(used) => assets.shaders.get(&used.shader).filter(|s| !s.effect).map(|shader| {
                    let srgb = srgb_bits(shader);
                    let own =
                        OwnTextures { base: m.textures, own: used.textures, count: shader.textures.len() as u8, srgb };
                    moving = shader.moves;
                    reads_behind = shader.reads_behind;
                    (Some(used.shader), own)
                }),
            };
            self.looks.push(look);
            self.moving.push(moving);
            // Its shader not there: not drawn, and casts no shadow.
            let Some((shader, textures)) = look else { continue };
            let (at, radius) = match skin {
                Some((first, skin)) => skinned_bounds(&scene.joints[first..], &skin.joints),
                None => bounds(draw.model, mesh.centre, mesh.radius),
            };
            // As far as its shader can move it, besides.
            let radius = radius + m.shader.map_or(0.0, |used| used.reach.max(0.0));
            // Out of view, it can still shadow what's in it. Blended
            // surfaces cast none. One that reads what's behind is drawn
            // over it, so it's see-through whatever its blending.
            let cover = match Cover::of(&draw.material) {
                cover if reads_behind && !cover.blended() => Cover::Transparent,
                cover => cover,
            };
            if draw.cast_shadows && !cover.blended() {
                self.shadow_candidates.push((at, radius, i as u32));
            }
            if !in_view(&planes, at, radius) {
                stats.culled += 1;
                continue;
            }
            let mirrored = Mat3::from_mat4(draw.model).determinant() < 0.0;
            let outlined = cover == Cover::Opaque && m.outline.is_some_and(|o| o.width > 0.0);
            let key = (
                cover,
                Faces::of(m, mirrored),
                skin.is_some(),
                shader,
                textures,
                outlined,
                (draw.mesh.0, draw.submesh),
            );
            if cover.blended() {
                self.behind |= reads_behind;
                self.blended.push(([0, 0], (at - camera.position).dot(forward), key, i as u32));
            } else {
                self.order.push((key, i as u32));
            }
        }
        // Particle systems: among the blended, by their bounds. One whose
        // material doesn't blend is drawn as transparent. Soft ones, and
        // those whose shader reads what's behind, keep it.
        for (i, particles) in scene.particle_draws.iter().enumerate() {
            if particles.count == 0 {
                continue;
            }
            let m = &particles.material;
            let (shader, textures, reads_behind) = match &m.shader {
                None => (None, OwnTextures::of(m.textures), false),
                Some(used) => match assets.shaders.get(&used.shader).filter(|s| !s.effect) {
                    Some(shader) => {
                        let count = shader.textures.len() as u8;
                        let own = OwnTextures { base: m.textures, own: used.textures, count, srgb: srgb_bits(shader) };
                        (Some(used.shader), own, shader.reads_behind)
                    }
                    None => continue,
                },
            };
            let mut radius = particles.radius;
            let mesh = match particles.mode {
                crate::ParticleMode::Mesh(key) => {
                    let Some(mesh) = assets.meshes.get(&key) else {
                        stats.missing_meshes += 1;
                        continue;
                    };
                    radius += (mesh.centre.length() + mesh.radius) * particles.largest;
                    (key.0, 0)
                }
                _ => (u64::MAX, 0),
            };
            if !in_view(&planes, particles.center, radius) {
                stats.culled += 1;
                continue;
            }
            self.behind |= reads_behind;
            self.soft |= particles.softness > 0.0 && self.quality.soft_particles;
            let cover = match Cover::of(m) {
                Cover::Additive => Cover::Additive,
                _ => Cover::Transparent,
            };
            let key = (cover, Faces::Both, false, shader, textures, false, mesh);
            // Text over everything goes after everything.
            let (sorting, distance) = match particles.mode {
                crate::ParticleMode::Glyphs { on_top: true } => ([i32::MAX; 2], f32::NEG_INFINITY),
                _ => (particles.sorting, (particles.center - camera.position).dot(forward)),
            };
            self.blended.push((sorting, distance, key, i as u32 | PARTICLES));
        }
        // The camera's screen effects, each one's uniform laid out as
        // effect.wgsl reads it. Any makes the scene image, even at low, for
        // the last pass to draw into theirs.
        let stride = self.kit.effects.stride as usize;
        for effect in &scene.effects {
            let Some(shader) = assets.shaders.get(&effect.shader).filter(|s| s.effect) else { continue };
            self.effect_chain.push(effect.shader);
            self.effect_textures.push(OwnTextures {
                base: MaterialTextures::NONE,
                own: effect.textures,
                count: shader.textures.len() as u8,
                srgb: srgb_bits(shader),
            });
            self.effect_depth |= shader.reads_behind;
            let at = self.effect_uniforms.len();
            self.effect_uniforms.resize(at + stride, 0);
            let u = &self.uniforms;
            let frame = [view_size.0 as f32, view_size.1 as f32, scene.time[0], 0.0];
            self.effect_uniforms[at..at + 16].copy_from_slice(bytemuck::bytes_of(&frame));
            self.effect_uniforms[at + 16..at + 80].copy_from_slice(bytemuck::bytes_of(&u.sky.inverse_view_proj));
            self.effect_uniforms[at + 80..at + 96].copy_from_slice(bytemuck::bytes_of(&u.cells_eye));
            self.effect_uniforms[at + 96..at + 112].copy_from_slice(bytemuck::bytes_of(&u.cells_forward));
            let first = effect.values as usize;
            let blocks = (shader.values_size as usize).div_ceil(48);
            if let Some(values) = scene.shader_values.get(first..first + blocks) {
                let bytes: &[u8] = bytemuck::cast_slice(values);
                let (size, from) = (shader.values_size as usize, at + crate::effects::VALUES_AT);
                self.effect_uniforms[from..from + size].copy_from_slice(&bytes[..size]);
            }
        }
        drop(assets);
        if !self.effect_chain.is_empty() {
            self.image_format.get_or_insert(self.format);
        }
        stats.effects = self.effect_chain.len() as u32;
        self.order.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        // By sorting layer and order (sprites'), then furthest first; the
        // same distance keeps the scene's order, so they don't flicker.
        self.blended.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)).then(a.3.cmp(&b.3)));

        for &(key, i) in &self.order {
            batch(&mut self.batches, &mut self.instances, key, &scene.draws[i as usize], values);
        }
        self.blended_from = self.batches.len();
        for &(_, _, key, i) in &self.blended {
            match i & PARTICLES {
                0 => batch(&mut self.batches, &mut self.instances, key, &scene.draws[i as usize], values),
                _ => particle_batch(
                    &mut self.batches,
                    (&mut self.particle_instances, &mut self.particle_order),
                    key,
                    scene,
                    &scene.particle_draws[(i & !PARTICLES) as usize],
                    (forward, camera.position, values),
                    self.quality.soft_particles,
                ),
            }
        }
        stats.particles = self.particle_instances.len() as u32;
        stats.joints = scene.joints.len() as u32;
        for line in &scene.lines {
            for end in [line.a, line.b] {
                self.line_vertices.push(LineVertex { position: end.into(), color: line.color });
            }
        }
        // Cut-outs with their own shaders add their shadows' instances
        // after these.
        stats.draws = self.instances.len() as u32;
        self.prepare_shadows(scene, &camera, aspect, shadow_sun.map(|(_, direction)| direction), &mut stats);
        // The shadow maps `encode` draws with: as many as the frame uses.
        // A kind it uses fewer of keeps what it has a while, so shadows that
        // come and go don't remake them, then lets them go.
        let had = self.shadow_maps.as_ref().map_or([(1, 0); 2], |m| m.shape);
        for (k, had) in had.into_iter().enumerate() {
            let want = self.shadow_shape[k].unwrap_or((had.0, 0));
            let fewer = want.0 == had.0 && want.1 < had.1;
            self.shadow_idle[k] = if fewer { self.shadow_idle[k] + 1 } else { 0 };
            self.shadow_next[k] = if fewer && self.shadow_idle[k] <= KEEP_UNUSED_MAPS { had } else { want };
        }
        stats.shadow_bytes = ShadowMaps::bytes(self.shadow_next);
        // What's behind, or the depth an effect reads: kept a while too,
        // then let go.
        let wanted = self.behind || self.effect_depth;
        self.behind_idle = if wanted { 0 } else { self.behind_idle.saturating_add(1) };
        let held = self.targets.as_ref().is_some_and(|t| t.behind.is_some() && (t.width, t.height) == self.size);
        self.behind_kept = wanted || held && self.behind_idle <= KEEP_UNUSED_MAPS;
        stats.behind = self.behind;
        if self.behind_kept {
            let format = self.image_format.unwrap_or(self.format);
            stats.behind_bytes = BehindImages::bytes(self.size.0, self.size.1, format, self.samples);
        }
        // Soft particles' depth, when what's behind doesn't give it: kept a
        // while too.
        let soft = self.soft && !self.behind;
        self.soft_idle = if soft { 0 } else { self.soft_idle.saturating_add(1) };
        let held = self.targets.as_ref().is_some_and(|t| t.soft.is_some() && (t.width, t.height) == self.size);
        self.soft_kept = soft || held && self.soft_idle <= KEEP_UNUSED_MAPS;
        if self.soft_kept {
            stats.behind_bytes += SoftDepth::bytes(self.size.0, self.size.1);
        }
        // The scene's image, and ambient occlusion's, which stay once made.
        let (width, height) = (u64::from(self.size.0), u64::from(self.size.1));
        if let Some(format) = self.image_format {
            stats.target_bytes = width * height * u64::from(format.block_copy_size(None).unwrap_or(4));
        }
        if self.ao.is_some() || self.targets.as_ref().is_some_and(|t| t.ao.is_some()) {
            stats.target_bytes += AoImages::bytes(self.size.0, self.size.1);
        }
        // Bloom's images: kept a while too.
        self.bloom_idle = if self.bloom.is_some() { 0 } else { self.bloom_idle.saturating_add(1) };
        let image = self.targets.as_ref().filter(|t| (t.width, t.height) == self.size).and_then(|t| t.image.as_ref());
        if self.bloom.is_some() || image.is_some_and(|i| i.bloom.is_some()) && self.bloom_idle <= KEEP_UNUSED_MAPS {
            stats.bloom_bytes = crate::bloom::BloomChain::bytes(&self.kit.bloom, self.size.0, self.size.1);
        }
        // The effects' pictures: kept a while too.
        let count = self.effect_chain.len();
        self.effects_idle = if count > 0 { 0 } else { self.effects_idle.saturating_add(1) };
        if count > 0 {
            stats.effect_bytes = EffectImages::bytes(view_size, self.format, count);
        } else if let Some(images) = self.effect_images.as_ref().filter(|_| self.effects_idle <= KEEP_UNUSED_MAPS) {
            stats.effect_bytes = EffectImages::bytes(images.size, self.format, images.images.len());
        }
        stats.lines = scene.lines.len() as u32;
        stats.draw_calls = self.batches.len() as u32;
        stats
    }

    /// The shadow maps of a frame `prepare` has lit and culled: the sun's
    /// cascades (`sun` is the way its light travels), and the nearest spot
    /// lights' maps, each with the casters that fall in it, sorted and
    /// batched as they're drawn.
    pub(super) fn prepare_shadows(
        &mut self,
        scene: &RenderScene,
        camera: &crate::CameraView,
        aspect: f32,
        sun: Option<Vec3>,
        stats: &mut FrameStats,
    ) {
        // No shadows: no sun's maps and no spot maps, whatever lights ask.
        let on = self.quality.shadows.is_some();
        let level = self.quality.shadows.or(Quality::LOW.shadows).expect("low has shadows");
        let sun = sun.filter(|_| on);
        let stride = self.kit.shadows.view_stride as usize;
        let u = &mut self.uniforms;
        u.shadows = ShadowUniforms::zeroed();
        let sh = &mut u.shadows;
        let filter = match level.filter {
            ShadowFilter::Hard => 0.0,
            ShadowFilter::Soft => 1.0,
            ShadowFilter::Softer => 2.0,
        };
        sh.params = [0.0, filter, 1.0 / level.map_size as f32, 1.0 / level.spot_map_size as f32];
        let view = |map: usize, matrix: Mat4, views: &mut Vec<u8>| {
            views[map * stride..map * stride + 64].copy_from_slice(bytemuck::bytes_of(&matrix.to_cols_array()));
        };

        // The sun's cascades. Each takes the casters that overlap it, which
        // also stretch it back towards the sun to hold them.
        if let Some(direction) = sun {
            let count = level.cascades.clamp(1, MAX_CASCADES);
            let basis = shadows::light_basis(direction);
            let (mut cascades, splits) = shadows::cascades(camera, aspect, &level, &basis);
            for &(at, r, i) in &self.shadow_candidates {
                for (c, cascade) in cascades[..count].iter_mut().enumerate() {
                    if cascade.reach(&basis, at, r) {
                        let skinned = self.skinned[i as usize];
                        let (look, moving) = (self.looks[i as usize], self.moving[i as usize]);
                        self.caster_order
                            .push((caster_key(c as u8, &scene.draws[i as usize], skinned, look, moving), i));
                    }
                }
            }
            for (c, cascade) in cascades[..count].iter().enumerate() {
                let matrix = cascade.matrix(&basis);
                sh.cascades[c] = matrix.to_cols_array_2d();
                sh.texels[c] = cascade.texel(level.map_size);
                view(c, matrix, &mut self.shadow_views);
            }
            let distance = shadows::shadow_distance(camera, &level);
            sh.splits = [splits[0], splits[1], splits[2], distance * 0.9];
            sh.params[0] = count as f32;
            sh.camera_forward = shadows::camera_forward(camera).extend(distance).into();
            sh.camera_at = camera.position.extend(1.0).into();
            self.shadow_maps_drawn.0 = count as u8;
            self.shadow_shape[0] = Some((level.map_size, count as u32));
        }

        // The spot lights that cast shadows, a map each, the nearest the
        // camera first: their slots in the frame's list.
        let cap = if on { level.spot_maps.min(MAX_SPOT_MAPS) } else { 0 };
        let mut nearest = [(f32::INFINITY, 0usize); MAX_SPOT_MAPS];
        for (slot, &(distance, i)) in self.in_view_lights.iter().enumerate() {
            let light = &scene.lights[i as usize];
            if light.shadows && matches!(light.kind, LightKind::Spot { .. }) && cap > 0 && distance < nearest[cap - 1].0
            {
                let mut k = cap - 1;
                while k > 0 && nearest[k - 1].0 > distance {
                    nearest[k] = nearest[k - 1];
                    k -= 1;
                }
                nearest[k] = (distance, slot);
            }
        }
        let mut spots = 0;
        for &(distance, slot) in &nearest[..cap] {
            if distance == f32::INFINITY {
                break;
            }
            let light = &scene.lights[self.in_view_lights[slot].1 as usize];
            let LightKind::Spot { position, direction, range, angle, .. } = light.kind else { continue };
            let (matrix, texel) = shadows::spot_view(position, direction, angle, range, level.spot_map_size);
            u.lights[slot].cone[2] = spots as f32;
            u.lights[slot].cone[3] = texel;
            u.shadows.spots[spots] = matrix.to_cols_array_2d();
            let map = MAX_CASCADES + spots;
            view(map, matrix, &mut self.shadow_views);
            let planes = frustum_planes(matrix);
            for &(at, r, d) in &self.shadow_candidates {
                if in_view(&planes, at, r) {
                    let skinned = self.skinned[d as usize];
                    let (look, moving) = (self.looks[d as usize], self.moving[d as usize]);
                    self.caster_order.push((caster_key(map as u8, &scene.draws[d as usize], skinned, look, moving), d));
                }
            }
            spots += 1;
        }
        if spots > 0 {
            self.shadow_maps_drawn.1 = spots as u8;
            self.shadow_shape[1] = Some((level.spot_map_size, spots as u32));
        }

        self.caster_order.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for &(key @ (map, own, cutout, skinned, textures, _), i) in &self.caster_order {
            let draw = &scene.draws[i as usize];
            let m = &draw.material;
            // A cut-out with its own shader is cut by it, and one whose
            // shader moves its vertices is moved by it: drawn as the
            // forward pass draws it, from the instances after its own.
            let first = if own.is_some() { self.instances.len() } else { self.casters.len() } as u32;
            match self.caster_batches.last_mut() {
                Some(b) if (b.map, b.own, b.cutout, b.skinned, b.textures, (b.mesh.0, b.submesh)) == key => {
                    b.count += 1
                }
                _ => self.caster_batches.push(CasterBatch {
                    map,
                    own,
                    cutout,
                    skinned,
                    textures,
                    mesh: draw.mesh,
                    submesh: draw.submesh,
                    first,
                    count: 1,
                }),
            }
            if own.is_some() {
                self.instances.push(instance(draw, skinned, self.values_at));
                continue;
            }
            let ([su, sv], [ou, ov]) = (m.uv_scale, m.uv_offset);
            let joints = if skinned { draw.skin.unwrap_or(0) as f32 } else { 0.0 };
            self.casters.push(Caster {
                model: draw.model.to_cols_array_2d(),
                uv: [su, sv, ou, ov],
                alpha: [m.base_color[3], m.alpha_cutoff, joints, 0.0],
            });
        }
        stats.shadow_maps = u32::from(self.shadow_maps_drawn.0) + u32::from(self.shadow_maps_drawn.1);
        stats.shadow_casters = self.casters.len() as u32 + (self.instances.len() as u32 - stats.draws);
    }
}

/// Where a draw goes among `map`'s casters: opaque ones need no textures,
/// so they group by mesh alone. One whose shader cuts it out or moves its
/// vertices (`moving`) is drawn with that shader.
pub(super) fn caster_key(map: u8, draw: &crate::Draw, skinned: bool, look: Option<Look>, moving: bool) -> CasterKey {
    let m = &draw.material;
    let cutout = m.alpha_cutoff > 0.0;
    let own = look.filter(|(shader, _)| (cutout || moving) && shader.is_some());
    let textures = if cutout && own.is_none() { m.textures } else { MaterialTextures::default() };
    (map, own, cutout, skinned, textures, (draw.mesh.0, draw.submesh))
}

/// A draw's sort key: how it covers what's behind, which faces, whether
/// it's bent, its shader, its textures, whether it's outlined and its mesh,
/// in the order draws are grouped.
pub(super) type SortKey = (Cover, Faces, bool, Option<ShaderKey>, OwnTextures, bool, MeshPart);

/// Adds `draw` to the last of `batches` if it has `key`, or starts another.
/// `values` is where the materials' values start among the joints.
pub(super) fn batch(
    batches: &mut Vec<Batch>,
    instances: &mut Vec<Instance>,
    key: SortKey,
    draw: &crate::Draw,
    values: u32,
) {
    let (cover, faces, skinned, shader, textures, outlined, _) = key;
    let first = instances.len() as u32;
    match batches.last_mut() {
        Some(b) if (b.cover, b.faces, b.skinned, b.shader, b.textures, b.outlined, (b.mesh.0, b.submesh)) == key => {
            b.count += 1
        }
        _ => {
            let (mesh, submesh) = (draw.mesh, draw.submesh);
            batches.push(Batch {
                cover,
                faces,
                skinned,
                outlined,
                shader,
                textures,
                mesh,
                submesh,
                particle: None,
                first,
                count: 1,
            })
        }
    }
    instances.push(instance(draw, skinned, values));
}

/// A particle system's batch, and its particles' instances: a
/// transparent one's furthest first along the way the camera looks
/// (`forward`), an additive one's as they are, which adds the same; a
/// trail's points in order, each facing the `eye`. `values` is where the
/// materials' values start among the joints. Without `soft_particles`,
/// none are soft.
pub(super) fn particle_batch(
    batches: &mut Vec<Batch>,
    (instances, order): (&mut Vec<ParticleInstance>, &mut Vec<u64>),
    key: SortKey,
    scene: &RenderScene,
    draw: &crate::ParticleDraw,
    (forward, eye, values): (Vec3, Vec3, u32),
    soft_particles: bool,
) {
    let (cover, faces, _, shader, textures, _, mesh) = key;
    let first = instances.len() as u32;
    let m = &draw.material;
    if let crate::ParticleMode::Sprites { cutoff } = draw.mode {
        let from = draw.first as usize;
        let Some(sprites) = scene.sprites.get(from..from + draw.count as usize) else { return };
        let values = m.shader.map_or(0, |used| values + used.values);
        let base = m.base_color;
        // Furthest first, as a transparent system's particles.
        order.clear();
        order.extend(
            sprites.iter().enumerate().map(|(i, s)| (u64::from(!ordered(s.center.dot(forward))) << 32) | i as u64),
        );
        order.sort_unstable();
        instances.extend(order.iter().map(|&i| {
            let s = &sprites[(i & 0xFFFF_FFFF) as usize];
            let [r, g, b, a] = s.color;
            ParticleInstance {
                center_size: s.center.extend(1.0).into(),
                color: [r * base[0], g * base[1], b * base[2], a * base[3]],
                velocity_turn: s.right.extend(0.0).into(),
                uv: s.uv,
                params: [cutoff, 0.0, 0.0, GLYPH_MODE],
                emissive: [0.0; 4],
                particle: s.up.extend(1.0).into(),
                values: [values, 0, 0, 0],
            }
        }));
        let count = instances.len() as u32 - first;
        let (cover, faces, _, shader, textures, _, _) = key;
        let look = ParticleLook {
            mesh: false,
            trail: false,
            lit: draw.lit,
            soft: false,
            glyphs: false,
            on_top: false,
            sprites: true,
        };
        batches.push(Batch {
            cover,
            faces,
            skinned: false,
            outlined: false,
            shader,
            textures,
            mesh: MeshKey(u64::MAX),
            submesh: 0,
            particle: Some(look),
            first,
            count,
        });
        return;
    }
    if let crate::ParticleMode::Glyphs { on_top } = draw.mode {
        let from = draw.first as usize;
        let Some(glyphs) = scene.glyphs.get(from..from + draw.count as usize) else { return };
        let values = m.shader.map_or(0, |used| values + used.values);
        let base = m.base_color;
        // Furthest first, as a transparent system's particles.
        order.clear();
        order.extend(
            glyphs.iter().enumerate().map(|(i, g)| (u64::from(!ordered(g.center.dot(forward))) << 32) | i as u64),
        );
        order.sort_unstable();
        instances.extend(order.iter().map(|&i| {
            let g = &glyphs[(i & 0xFFFF_FFFF) as usize];
            let [r, gr, b, a] = g.color;
            let [or, og, ob, oa] = g.outline_color;
            ParticleInstance {
                center_size: g.center.extend(1.0).into(),
                color: [r * base[0], gr * base[1], b * base[2], a * base[3]],
                velocity_turn: g.right.extend(0.0).into(),
                uv: g.uv,
                params: [g.outline, g.softness, 0.0, GLYPH_MODE],
                emissive: [or, og, ob, 1.0],
                particle: g.up.extend(oa).into(),
                values: [values, 0, 0, 0],
            }
        }));
        let count = instances.len() as u32 - first;
        let (cover, faces, _, shader, textures, _, _) = key;
        let look = ParticleLook {
            mesh: false,
            trail: false,
            lit: draw.lit,
            soft: false,
            glyphs: true,
            on_top,
            sprites: false,
        };
        batches.push(Batch {
            cover,
            faces,
            skinned: false,
            outlined: false,
            shader,
            textures,
            mesh: MeshKey(u64::MAX),
            submesh: 0,
            particle: Some(look),
            first,
            count,
        });
        return;
    }
    let [er, eg, eb] = m.emissive;
    let base = m.base_color;
    let from = draw.first as usize;
    let Some(particles) = scene.particles.get(from..from + draw.count as usize) else { return };
    let (mode, length_scale, speed_scale) = match draw.mode {
        crate::ParticleMode::Billboard => (0.0, 0.0, 0.0),
        crate::ParticleMode::Stretched { length_scale, speed_scale } => (1.0, length_scale, speed_scale),
        crate::ParticleMode::Horizontal => (2.0, 0.0, 0.0),
        crate::ParticleMode::Vertical => (3.0, 0.0, 0.0),
        crate::ParticleMode::Mesh(_) => (4.0, 0.0, 0.0),
        crate::ParticleMode::Trail => (5.0, 0.0, 0.0),
        crate::ParticleMode::Glyphs { .. } | crate::ParticleMode::Sprites { .. } => (GLYPH_MODE, 0.0, 0.0),
    };
    let trail = draw.mode == crate::ParticleMode::Trail;
    let softness = if soft_particles { draw.softness } else { 0.0 };
    let [columns, rows] = draw.sheet.map(|n| n.max(1));
    // A frame's share of the texture, then where the material's scale and
    // offset put it.
    let ([su, sv], [ou, ov]) = (m.uv_scale, m.uv_offset);
    let (fu, fv) = (su / columns as f32, sv / rows as f32);
    let values = m.shader.map_or(0, |used| values + used.values);
    // A transparent system's furthest first: sorted by depth (then index,
    // so equal depths keep their order) as one number each, not as whole
    // instances with their depths worked out in every comparison.
    order.clear();
    if cover == Cover::Transparent && !trail {
        order.extend(
            particles.iter().enumerate().map(|(i, p)| (u64::from(!ordered(p.position.dot(forward))) << 32) | i as u64),
        );
        order.sort_unstable();
    } else {
        order.extend(0..particles.len() as u64);
    }
    instances.extend(order.iter().map(|&i| {
        let p = &particles[(i & 0xFFFF_FFFF) as usize];
        let [r, g, b, a] = p.color;
        let (column, row) = (p.frame % columns, (p.frame / columns) % rows);
        // A trail's point: half its width across the way it goes, as seen
        // from the eye, so the strip faces the camera and its joins meet.
        let velocity = match trail {
            false => p.velocity,
            true => p.velocity.cross(eye - p.position).normalize_or_zero() * (p.size * 0.5),
        };
        ParticleInstance {
            center_size: p.position.extend(p.size).into(),
            color: [r * base[0], g * base[1], b * base[2], a * base[3]],
            velocity_turn: velocity.extend(p.rotation).into(),
            uv: [fu, fv, column as f32 * fu + ou, row as f32 * fv + ov],
            params: [m.metallic, m.roughness, softness, mode],
            emissive: [er, eg, eb, 1.0],
            particle: [p.t, p.random, length_scale, speed_scale],
            values: [values, 0, 0, 0],
        }
    }));
    let count = instances.len() as u32 - first;
    batches.push(Batch {
        cover,
        faces,
        skinned: false,
        outlined: false,
        shader,
        textures,
        mesh: MeshKey(mesh.0),
        submesh: 0,
        particle: Some(ParticleLook {
            mesh: mesh.0 != u64::MAX,
            trail,
            lit: draw.lit,
            soft: softness > 0.0,
            glyphs: false,
            on_top: false,
            sprites: false,
        }),
        first,
        count,
    });
}

/// `depth`'s bits made to sort as it does: larger, larger.
pub(super) fn ordered(depth: f32) -> u32 {
    let bits = depth.to_bits();
    if bits >> 31 == 1 { !bits } else { bits | 1 << 31 }
}

/// `draw`'s instance, as forward.wgsl reads it.
pub(super) fn instance(draw: &crate::Draw, skinned: bool, values: u32) -> Instance {
    let m = &draw.material;
    let [er, eg, eb] = m.emissive;
    let ([su, sv], [ou, ov]) = (m.uv_scale, m.uv_offset);
    Instance {
        model: {
            let rows = draw.model.transpose();
            [rows.x_axis.into(), rows.y_axis.into(), rows.z_axis.into()]
        },
        base_color: m.base_color,
        params: [m.metallic, m.roughness, m.alpha_cutoff, if skinned { draw.skin.unwrap_or(0) as f32 } else { 0.0 }],
        emissive: [er, eg, eb, if draw.receive_shadows { 1.0 } else { 0.0 }],
        uv: [su, sv, ou, ov],
        own: draw.own,
        values: m.shader.map_or(0, |used| values + used.values),
        outline: m.outline.map_or([0.0; 4], |o| [o.color[0], o.color[1], o.color[2], o.width]),
    }
}

/// Moves `camera` across its view to the nearest whole pixel of a picture
/// `pixels` high ([`CameraView::pixels`]): for a perspective camera, a
/// pixel where it looks at the ground, or 10 m away.
pub(super) fn snap_to_pixels(camera: &mut crate::CameraView, pixels: u32) {
    let world = camera.view.inverse();
    let (right, up, forward) = (world.x_axis.truncate(), world.y_axis.truncate(), -world.z_axis.truncate());
    let height = match camera.orthographic {
        Some(half) => half * 2.0,
        None => {
            let ground = camera.position.y > 0.0 && forward.y < -1e-3;
            let distance = if ground { camera.position.y / -forward.y } else { 10.0 };
            2.0 * (camera.fov_y * 0.5).tan() * distance
        }
    };
    let texel = height / pixels as f32;
    if !(texel.is_finite() && texel > 0.0) {
        return;
    }
    let snap = |along: f32| (along / texel).round() * texel - along;
    let moved = right * snap(camera.position.dot(right)) + up * snap(camera.position.dot(up));
    camera.position += moved;
    camera.view *= Mat4::from_translation(-moved);
}

/// The six planes around what `view_proj` shows (left, right, bottom, top,
/// far and near, for reversed-Z with depth from 0 to 1), each as a normal
/// pointing inside and a distance, scaled so the normal is unit length.
pub(super) fn frustum_planes(view_proj: Mat4) -> [Vec4; 6] {
    let row = |i: usize| view_proj.row(i);
    let (x, y, z, w) = (row(0), row(1), row(2), row(3));
    [w + x, w - x, w + y, w - y, z, w - z].map(|p| p / p.truncate().length().max(f32::MIN_POSITIVE))
}

/// The world-space centre and radius of a mesh's bounding sphere, at
/// `centre` with `radius` in its own space, when placed by `model`.
pub(super) fn bounds(model: Mat4, centre: Vec3, radius: f32) -> (Vec3, f32) {
    // The longest axis's stretch: the sphere stays around the mesh under
    // any scale.
    let stretch =
        model.x_axis.truncate().length().max(model.y_axis.truncate().length()).max(model.z_axis.truncate().length());
    (model.transform_point3(centre), radius * stretch)
}

/// The world-space bounding sphere of a skinned mesh bent by `joints`
/// (from its skin's first): the box around each joint's sphere (`spheres`,
/// in the mesh's space as bound) where that joint takes it, and the sphere
/// around that box. So an arm swung out stays in it.
pub(super) fn skinned_bounds(joints: &[Mat4], spheres: &[Option<(Vec3, f32)>]) -> (Vec3, f32) {
    let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for (joint, sphere) in joints.iter().zip(spheres) {
        let Some((centre, radius)) = *sphere else { continue };
        let (at, r) = bounds(*joint, centre, radius);
        lo = lo.min(at - r);
        hi = hi.max(at + r);
    }
    if lo.x > hi.x {
        return (Vec3::ZERO, 0.0);
    }
    ((lo + hi) * 0.5, (hi - lo).length() * 0.5)
}

/// Whether a sphere at `at` with radius `r` can be seen at all: false only
/// when it is wholly outside one of the `planes`.
pub(super) fn in_view(planes: &[Vec4; 6], at: Vec3, r: f32) -> bool {
    planes.iter().all(|p| p.truncate().dot(at) + p.w >= -r)
}

/// A point or spot light as forward.wgsl reads it.
pub(super) fn light_uniform(light: &crate::Light) -> LightUniform {
    let (position, range, direction, cone) = match light.kind {
        LightKind::Sun { .. } => unreachable!("suns aren't in the frame's list"),
        LightKind::Point { position, range } => (position, range, Vec3::ZERO, [0.0, 1.0]),
        LightKind::Spot { position, direction, range, angle, softness } => {
            // The fade runs from the cone's edge in to `softness` of the way
            // to its axis, linear in the cosine of the angle from the axis.
            let outer = (angle * 0.5).clamp(0.0, std::f32::consts::FRAC_PI_2);
            let inner = outer * (1.0 - softness.clamp(0.0, 1.0));
            let (cos_outer, cos_inner) = (outer.cos(), inner.cos());
            let scale = 1.0 / (cos_inner - cos_outer).max(1e-4);
            (position, range, direction.normalize_or_zero(), [scale, -cos_outer * scale])
        }
    };
    LightUniform {
        position: position.extend(1.0 / (range * range)).into(),
        color: light.color.extend(0.0).into(),
        direction: direction.extend(0.0).into(),
        cone: [cone[0], cone[1], -1.0, 0.0],
    }
}
