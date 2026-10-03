//! [`ExtractRenderer::prepare`]: the extract into this frame's batches,
//! lights, uniforms and GPU buffers.

use super::*;

impl ExtractRenderer {
    /// Applies a pending quality change, then uploads the camera, the
    /// lights, the sun's shadow fit, the environment, the extract's
    /// instances (culled against the camera frustum and the sun's box,
    /// grouped into one range per opaque `(mesh, material)` batch, then
    /// the transparent ones back-to-front) — the queued [`ExtraWorld`]s'
    /// with them, which it consumes — and the debug lines. Returns the
    /// number of instances the scene passes will draw.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extract: &Extract,
    ) -> u32 {
        self.upload_usage = streaming::UploadUsage::default();
        let prepare_start = std::time::Instant::now();
        // Consumed by this frame; handed back empty at the end to keep
        // the queue's capacity.
        let mut extras = std::mem::take(&mut self.extra_worlds);
        self.retain_extract_assets(extract, &extras);
        self.reconcile_evictions(queue);
        self.resolve_quality(device);
        self.ensure_fallbacks(device, queue);
        if let Some(timer) = self.gpu_timer.as_mut() {
            if let Some(times) = timer.collect(device, queue) {
                self.gpu_times = times;
                self.gpu_timed += 1;
            }
        }
        let mut stats = FrameStats {
            msaa: self.quality.msaa,
            render_width: self.attachments.width,
            render_height: self.attachments.height,
            encode_us: self.stats.encode_us,
            gpu_timestamps: self.gpu_timer.is_some(),
            gpu_shadow_us: self.gpu_times.0,
            gpu_ao_us: self.gpu_times.1,
            gpu_scene_us: self.gpu_times.2,
            gpu_post_us: self.gpu_times.3,
            gpu_frame_us: self.gpu_times.4,
            gpu_untimed: self.gpu_timer.as_ref().map_or(0, |t| t.untimed),
            gpu_timed: self.gpu_timed,
            ..FrameStats::default()
        };
        stats.warmed = self.warm_up(device, queue);
        // Readiness describes the instances assembled below, not merely the
        // cache by the time a later object has finished preparing.
        self.frame_assets_deferred = false;

        // Camera.
        let aspect = self.width as f32 / self.height as f32;
        let (view_proj, camera_pos, component, shape) = match &extract.camera {
            Some(c) => {
                let (_, _, translation) = c.world.to_scale_rotation_translation();
                let view = fynkel_core::view_matrix(&c.world);
                let proj = fynkel_core::projection_matrix(&c.camera, aspect);
                let shape = match c.camera.projection {
                    Projection::Perspective => shadow::Shape::Perspective {
                        fov_y: c.camera.fov_y,
                    },
                    Projection::Orthographic => shadow::Shape::Orthographic {
                        height: c.camera.ortho_height,
                    },
                };
                stats.scene_camera = true;
                (
                    proj * view,
                    translation,
                    Some(&c.camera),
                    shadow::CameraShape {
                        view,
                        near: c.camera.near,
                        far: c.camera.far,
                        aspect,
                        shape,
                    },
                )
            }
            None => (
                self.camera.view_proj(self.width, self.height),
                self.camera.eye,
                None,
                shadow::CameraShape {
                    view: self.camera.view(),
                    near: self.camera.near,
                    far: self.camera.far,
                    aspect,
                    shape: shadow::Shape::Perspective {
                        fov_y: self.camera.fov_y_radians,
                    },
                },
            ),
        };

        // The world's time for the shaders (ADR-0031 `globals`): x the
        // time, y the seconds since the previous frame, zw the internal
        // target's size.
        let frame_time = [
            extract.time,
            self.last_time.map_or(0.0, |t| (extract.time - t).max(0.0)),
            self.attachments.width as f32,
            self.attachments.height as f32,
        ];

        // Sun, ambient, sky. A scene that authors no lighting at all (no
        // light, no Environment) gets the host's fallback sun.
        // An extra world's lights count as authored, and its sun is the
        // frame's when the main extract has none.
        let authored_lighting = !extract.directional_lights.is_empty()
            || !extract.point_lights.is_empty()
            || !extract.spot_lights.is_empty()
            || extract.environment.is_some()
            || extras.iter().any(|x| {
                !x.extract.directional_lights.is_empty()
                    || !x.extract.point_lights.is_empty()
                    || !x.extract.spot_lights.is_empty()
            });
        let sun = match extract.directional_lights.first() {
            Some(l) => Some((l, l.direction)),
            None => extras.iter().find_map(|x| {
                x.extract
                    .directional_lights
                    .first()
                    .map(|l| (l, x.transform.transform_vector3(l.direction)))
            }),
        };
        let (sun_dir, sun_color, sun_lux, sun_present, sun_casts) = match sun {
            Some((l, direction)) => {
                stats.scene_light = true;
                let lux = l.light.intensity.max(0.0);
                (
                    (-direction).normalize_or(Vec3::Y),
                    l.light.color.max(Vec3::ZERO) * lux,
                    lux,
                    true,
                    l.light.cast_shadows,
                )
            }
            None if !authored_lighting => (
                self.light_dir,
                Vec3::splat(exposure::FALLBACK_SUN_LUX),
                exposure::FALLBACK_SUN_LUX,
                true,
                true,
            ),
            None => (Vec3::Y, Vec3::ZERO, 0.0, false, false),
        };
        let default_env = Environment::default();
        let env = extract
            .environment
            .as_ref()
            .map_or(&default_env, |e| &e.environment);
        let ambient_lux = env.ambient_intensity.max(0.0);
        let ambient = env.ambient_color.max(Vec3::ZERO) * ambient_lux;
        let sky = env
            .sky
            .as_ref()
            .filter(|r| self.texture(device, queue, r))
            .cloned();
        if let Some(r) = &sky {
            if self
                .sky_bind_group
                .as_ref()
                .is_none_or(|(have, _)| have != r)
            {
                let view = &self.textures[r].as_ref().expect("loaded above").view;
                let bg = create_single_bind_group(
                    device,
                    &self.single_layout,
                    &self.sampler,
                    view,
                    &self.post_buffer,
                    "fynkel sky bg",
                );
                self.sky_bind_group = Some((r.clone(), bg));
            }
        } else {
            self.sky_bind_group = None;
        }
        stats.sky = sky.is_some();

        // Image-based lighting (E1): the environment prefiltered next to
        // an HDR sky. An 8-bit sky has no `ENVM` and lights nothing — the
        // constant ambient stays.
        let want_ibl = sky.as_ref().filter(|r| self.environment(r));
        if self.ibl.as_ref().map(|(r, _)| r) != want_ibl {
            // Retire the old environment before charging its replacement.
            self.ibl = None;
            self.rebuild_frame_bind_group(device);
            self.ibl = want_ibl.cloned().and_then(|r| {
                let env = self.environments[&r].clone().expect("loaded above");
                let bytes = ((env.irradiance.len() + env.specular.len()) * 2) as u64;
                match self.admit_asset_bytes(&r, bytes) {
                    Ok(true) => Some((r.clone(), Ibl::upload(device, queue, &env, &r))),
                    Ok(false) => None,
                    Err(()) => {
                        self.environments.insert(r, None);
                        None
                    }
                }
            });
            self.ibl_dirty = true;
        }
        if self.ibl_dirty {
            self.rebuild_frame_bind_group(device);
            self.ibl_dirty = false;
        }
        let ibl_mips = self.ibl.as_ref().map_or(1, |(_, i)| i.specular_mips);
        // Metering: an IBL scene's ambient illuminance is the
        // environment's mean irradiance at the sky's intensity.
        let ibl_lux = self.ibl.as_ref().map_or(0.0, |(r, _)| {
            let e = self.environments[r].as_ref().expect("bound above");
            let mean = (e.mean_irradiance[0] + e.mean_irradiance[1] + e.mean_irradiance[2]) / 3.0;
            mean * env.sky_intensity.max(0.0)
        });
        stats.ibl = self.ibl.is_some();
        let ev100 = exposure::ev100_for(component, sun_lux, ambient_lux.max(ibl_lux));
        self.ev100 = ev100;
        let white = exposure::white_nits(ev100);

        // Point and spot lights: every one of them, culled to the view
        // and assigned per object below.
        // Each with its `culling_mask` (ADR-0020): the layers it lights.
        // An extra world's are placed by its transform first.
        let light_sources = std::iter::once((extract, None))
            .chain(extras.iter().map(|x| (&x.extract, Some(&x.transform))));
        let LightFrameScratch {
            mut lights,
            mut packed,
            mut masks,
            mut bounds,
        } = std::mem::take(&mut self.light_frame);
        lights.clear();
        packed.clear();
        masks.clear();
        bounds.clear();
        for (source, transform) in light_sources {
            for l in &source.point_lights {
                let (position, _, range) =
                    place_light(transform, l.position, Vec3::NEG_Z, l.light.range);
                lights.push((
                    position.distance_squared(camera_pos),
                    GpuLight {
                        position: position.extend(range.max(1e-3)).to_array(),
                        color: (l.light.color.max(Vec3::ZERO) * l.light.intensity.max(0.0))
                            .extend(LIGHT_POINT)
                            .to_array(),
                        ..GpuLight::default()
                    },
                    l.light.culling_mask,
                ));
            }
            for l in &source.spot_lights {
                let (position, direction, range) =
                    place_light(transform, l.position, l.direction, l.light.range);
                let outer = l.light.outer_angle.clamp(1e-3, std::f32::consts::PI - 1e-3);
                let inner = l.light.inner_angle.clamp(0.0, outer);
                let cos_outer = outer.cos();
                // `smoothstep` needs distinct edges.
                let cos_inner = inner.cos().max(cos_outer + 1e-4);
                lights.push((
                    position.distance_squared(camera_pos),
                    GpuLight {
                        position: position.extend(range.max(1e-3)).to_array(),
                        color: (l.light.color.max(Vec3::ZERO) * l.light.intensity.max(0.0))
                            .extend(LIGHT_SPOT)
                            .to_array(),
                        direction: direction
                            .normalize_or(Vec3::NEG_Z)
                            .extend(cos_outer)
                            .to_array(),
                        params: [cos_inner, 0.0, 0.0, 0.0],
                    },
                    l.light.culling_mask,
                ));
            }
        }
        // The sun's shadow fit depends only on the camera, so it comes
        // before the instances: casters are culled against its box.
        let fit = (self.quality.shadows && sun_present && sun_casts)
            .then(|| shadow::fit(sun_dir, &shape.shadow_corners(), self.shadow_map.resolution));
        let camera_frustum = Frustum::from_view_proj(&view_proj);
        let light_frustum = fit.as_ref().map(|f| Frustum::from_view_proj(&f.view_proj));
        self.prepare_visibility(
            extract,
            &extras,
            &camera_frustum,
            light_frustum.as_ref(),
            &mut stats,
        );

        // The frame's light pool (E1). A light whose range sphere misses
        // the camera frustum reaches nothing that is shaded, so it never
        // enters the pool — exact, not an approximation, and it is why
        // `frustum_culling` does not gate this. The nearest to the camera
        // fill the pool when more than `LIGHT_POOL` still reach the view.
        // The layers the frame's camera draws; the host's own camera (no
        // scene camera) and the editor's draw every layer.
        let camera_mask = extract
            .camera
            .as_ref()
            .map_or(u32::MAX, |c| c.camera.culling_mask);
        lights.retain(|(_, l, mask)| {
            if mask & camera_mask == 0 {
                return false;
            }
            camera_frustum.intersects(&Sphere {
                center: Vec3::new(l.position[0], l.position[1], l.position[2]),
                radius: l.position[3],
            })
        });
        lights.sort_by(|a, b| a.0.total_cmp(&b.0));
        let n_lights = lights.len().min(self.light_budget.pool);
        packed.resize(n_lights, GpuLight::default());
        masks.resize(n_lights, u32::MAX);
        for (i, (_, l, mask)) in lights.iter().take(self.light_budget.pool).enumerate() {
            packed[i] = *l;
            masks[i] = *mask;
        }
        bounds.extend(packed.iter().map(|l| Sphere {
            center: Vec3::new(l.position[0], l.position[1], l.position[2]),
            radius: l.position[3],
        }));
        self.light_index.update(&bounds);
        stats.lights = n_lights as u32;
        stats.lights_dropped = (lights.len() - n_lights) as u32;

        // Instances: opaque and mask by (mesh, material) in order of
        // first appearance; blend on their own, sorted back-to-front.
        // Each instance is tagged with where it is drawn — see the batch
        // layout below.
        let mut grouped: Vec<(bool, Vec<Grouped>)> = Vec::new();
        let mut blended: Vec<(f32, BatchKey, bool, Instance, [u32; 12])> = Vec::new();
        self.batch_index.clear();
        let mut mesh_of_record: HashMap<u32, usize> = HashMap::with_capacity(extract.meshes.len());
        let cube_key = || BatchKey {
            mesh: None,
            material: None,
            skinned: false,
        };
        let cube_bounds = self.cube.bounds;
        let mut caster_bounds: Option<Sphere> = None;
        let mut unbounded_caster = false;
        // The selection outline (ADR-0019, `outline.rs`): the mask value
        // of each outlined entity — a child 0.5, selected 1, selected
        // winning when an entity is both.
        let outline = self.overlay.take_outline();
        let mut outline_marks: HashMap<fynkel_contracts::Entity, f32> = HashMap::new();
        if let Some(o) = &outline {
            for e in &o.children {
                outline_marks.insert(*e, 0.5);
            }
            for e in &o.selected {
                outline_marks.insert(*e, 1.0);
            }
        }
        self.outline_draws.clear();
        self.outline_instances.clear();
        // The main extract, then each extra world: its records placed by
        // its transform, its palettes after those before it, its
        // instances marked grey when it is.
        let sources = std::iter::once((extract, None, false)).chain(
            extras
                .iter()
                .map(|x| (&x.extract, Some(x.transform), x.gray)),
        );
        let mut joint_base = 0u32;
        let mut visibility_base = 0usize;
        for (si, (source, transform, gray)) in sources.enumerate() {
            let main = si == 0;
            mesh_of_record.clear();
            for (i, m) in source.meshes.iter().enumerate() {
                mesh_of_record.insert(m.record, i);
            }
            for (ri, r) in source.records.iter().enumerate() {
                let world = match transform {
                    Some(t) => t * r.world,
                    None => r.world,
                };
                if r.layer_bit & camera_mask == 0 {
                    // Not this camera's layers: no draw, no shadow, no light list.
                    stats.layer_hidden += 1;
                    continue;
                }
                if self.frustum_culling
                    && !self.camera_visible[visibility_base + ri]
                    && !self.shadow_visible[visibility_base + ri]
                {
                    if let Some(&mi) = mesh_of_record.get(&(ri as u32)) {
                        if self
                            .meshes
                            .get(&source.meshes[mi].mesh)
                            .is_some_and(Option::is_some)
                        {
                            stats.meshes += 1;
                        }
                    } else if !self.placeholders {
                        continue;
                    } else {
                        stats.placeholders += 1;
                    }
                    stats.culled += 1;
                    continue;
                }
                let (key, double_sided, mut instance, casts, blend, sphere) = match mesh_of_record
                    .get(&(ri as u32))
                {
                    Some(&mi) => {
                        let m = &source.meshes[mi];
                        let have_mesh = self.mesh(device, &m.mesh);
                        let loaded = self.material(device, queue, &m.material);
                        let material = &loaded.material;
                        if have_mesh {
                            stats.meshes += 1;
                            let blend = material.alpha_mode == AlphaMode::Blend;
                            let gpu = self.meshes.get(&m.mesh).and_then(Option::as_ref);
                            let bounds = gpu.map_or(cube_bounds, |g| g.bounds);
                            // Skinned when the mesh carries weights and
                            // the extract a palette for this instance; a
                            // skinned mesh without a `Skin` draws as
                            // cooked (its bind pose).
                            let palette = m
                                .skin
                                .filter(|_| gpu.is_some_and(|g| g.skin_buffer.is_some()));
                            let cached_bound = self.visibility_bounds[visibility_base + ri];
                            let sphere = if cached_bound.is_finite() {
                                cached_bound
                            } else if loaded.deforms {
                                Sphere::UNBOUNDED
                            } else {
                                match palette {
                                    Some(p) => {
                                        let start = p.first_joint as usize;
                                        let end = start.checked_add(p.joint_count as usize);
                                        let joints =
                                            end.and_then(|end| source.joints.get(start..end));
                                        match (gpu.and_then(|g| g.skin_bounds.as_ref()), joints) {
                                            (Some(bounds), Some(joints)) => {
                                                stats.bounds_joints += bounds.joint_count();
                                                bounds.posed(&world, joints)
                                            }
                                            _ => Sphere::UNBOUNDED,
                                        }
                                    }
                                    None => bounds.transformed(&world),
                                }
                            };
                            let mut instance = instance_for(material, world, m.receive_shadows);
                            if loaded.broken {
                                instance.tint = MISSING_TINT;
                                instance.flags[1] = 1.0;
                            }
                            if let Some(p) = palette {
                                instance.emissive[3] = (joint_base + p.first_joint) as f32;
                            }
                            (
                                BatchKey {
                                    mesh: Some(m.mesh.clone()),
                                    material: loaded.key.clone(),
                                    skinned: palette.is_some(),
                                },
                                material.double_sided,
                                instance,
                                m.cast_shadows && loaded.casts && !blend,
                                blend,
                                sphere,
                            )
                        } else {
                            stats.missing += 1;
                            (
                                cube_key(),
                                false,
                                Instance {
                                    model: world.to_cols_array_2d(),
                                    tint: MISSING_TINT,
                                    emissive: [0.0; 4],
                                    params: [0.0, 1.0, 1.0, 1.0],
                                    flags: [NO_CUTOFF, 1.0, 0.0, 0.0],
                                },
                                m.cast_shadows,
                                false,
                                cube_bounds.transformed(&world),
                            )
                        }
                    }
                    None => {
                        if !self.placeholders {
                            continue;
                        }
                        stats.placeholders += 1;
                        (
                            cube_key(),
                            false,
                            Instance {
                                model: world.to_cols_array_2d(),
                                tint: tint_for(r.entity.slot()),
                                emissive: [0.0; 4],
                                params: [0.0, 1.0, 1.0, 1.0],
                                flags: [NO_CUTOFF, 0.0, 0.0, 1.0],
                            },
                            true,
                            false,
                            cube_bounds.transformed(&world),
                        )
                    }
                };
                if gray {
                    instance.flags[3] += GRAY_FLAG;
                }
                stats.bounds_unbounded += u32::from(!sphere.is_finite());
                // Culling: the posed bounding sphere in world space against
                // the camera frustum and, for casters, the sun's box. A
                // caster outside the view still shadows what is in it.

                // Whether this caster reaches the shadowed range at all,
                // tested against the loose fit whether or not culling is on:
                // what casts decides how tight the cascades are (R1), and
                // culling must not change the picture.
                let known_bound = self.visibility_bounds[visibility_base + ri].is_finite();
                let reaches_shadow = casts
                    && if known_bound {
                        self.shadow_visible[visibility_base + ri]
                    } else {
                        light_frustum
                            .as_ref()
                            .is_some_and(|f| f.intersects(&sphere))
                    };
                if reaches_shadow {
                    unbounded_caster |= !sphere.is_finite();
                    // A subset of the loose fit, so the cascades can only
                    // shrink against it, never grow.
                    caster_bounds = Some(match caster_bounds {
                        Some(u) => u.union(&sphere),
                        None => sphere,
                    });
                }
                let (in_view, casts) = if self.frustum_culling {
                    (
                        if known_bound {
                            self.camera_visible[visibility_base + ri]
                        } else {
                            camera_frustum.intersects(&sphere)
                        },
                        reaches_shadow,
                    )
                } else {
                    (true, casts && fit.is_some())
                };
                // Outlined: a copy of the instance for the mask, whatever
                // pass draws the original (a blended one included). What is
                // out of view has no silhouette on screen to ring.
                if in_view && main {
                    if let Some(&mark) = outline_marks.get(&r.entity) {
                        let mut copy = instance;
                        copy.params[0] = mark;
                        // Any valid light list: the mask shades nothing.
                        copy.flags[2] = 0.0;
                        self.outline_draws.push(OutlineDraw {
                            key: key.clone(),
                            double_sided,
                            index: self.outline_instances.len() as u32,
                        });
                        self.outline_instances.push(copy);
                    }
                }
                if !in_view {
                    stats.culled += 1;
                    if !casts {
                        continue;
                    }
                }
                // The object's light list (E1): the lights of the pool that
                // reach its bounds, the strongest first. Only what is shaded
                // needs one — a shadow-only instance is never lit, and an
                // unlit material ignores every light.
                let mut light_list = [0u32; 12];
                if in_view && n_lights > 0 && instance.flags[1] < 0.5 {
                    self.light_index
                        .overlaps(&sphere, &mut self.light_candidates);
                    stats.light_candidates += self.light_candidates.len() as u32;
                    let (list, count, overflow) = assign_lights(
                        &packed[..n_lights],
                        &masks[..n_lights],
                        r.layer_bit,
                        &sphere,
                        &mut self.light_scratch,
                        self.light_budget.per_object,
                        &self.light_candidates,
                    );
                    light_list = list;
                    stats.lights_max_per_object = stats.lights_max_per_object.max(count as u32);
                    stats.lights_overflow += overflow as u32;
                }
                if blend {
                    let origin = world.w_axis.truncate();
                    blended.push((
                        origin.distance_squared(camera_pos),
                        key,
                        double_sided,
                        instance,
                        light_list,
                    ));
                    continue;
                }
                let slot = match self.batch_index.get(&key) {
                    Some(&s) => s,
                    None => {
                        grouped.push((double_sided, Vec::new()));
                        self.batch_index.insert(key, grouped.len() - 1);
                        grouped.len() - 1
                    }
                };
                grouped[slot].1.push((
                    instance,
                    match (casts, in_view) {
                        (true, false) => Visibility::ShadowOnly,
                        (true, true) => Visibility::Both,
                        (false, true) => Visibility::SceneOnly,
                        (false, false) => unreachable!("skipped above"),
                    },
                    light_list,
                ));
            }
            joint_base += source.joints.len() as u32;
            visibility_base += source.records.len();
        }

        // Batch layout: [shadow only][both][scene only], so the shadow
        // draw is the first two and the scene draw the last two — one
        // contiguous range each, one upload.
        // Either storage buffer of group 2 outgrowing its capacity means
        // one new bind group for both.
        let mut rebind_group_2 = false;
        if n_lights > self.local_light_capacity {
            self.local_light_capacity = n_lights.next_power_of_two();
            self.local_light_buffer = create_local_light_buffer(device, self.local_light_capacity);
            rebind_group_2 = true;
        }
        if n_lights > 0 {
            queue.write_buffer(&self.local_light_buffer, 0, bytemuck::cast_slice(&packed));
        }
        self.instances.clear();
        self.light_lists.clear();
        self.batches.clear();
        self.transparent.clear();
        let mut keys: Vec<(BatchKey, usize)> = self.batch_index.drain().collect();
        keys.sort_by_key(|(_, slot)| *slot);
        let mut casters_total = 0;
        for (key, slot) in keys {
            let (double_sided, list) = &mut grouped[slot];
            // Stable: instances keep their order within a category.
            list.sort_by_key(|(_, v, _)| *v as u8);
            let count_of = |v: Visibility| list.iter().filter(|(_, x, _)| *x == v).count() as u32;
            let (shadow_only, both) =
                (count_of(Visibility::ShadowOnly), count_of(Visibility::Both));
            let start = self.instances.len() as u32;
            let shadow_count = shadow_only + both;
            let count = list.len() as u32 - shadow_only;
            casters_total += shadow_count;
            if count > 0 {
                stats.draws += 1;
            }
            if shadow_count > 0 {
                stats.shadow_draws += 1;
            }
            stats.instances += count;
            self.batches.push(Batch {
                key,
                double_sided: *double_sided,
                first: start + shadow_only,
                count,
                shadow_first: start,
                shadow_count,
            });
            // Every instance says where its light list is: the two live
            // in the same slot of their buffers.
            for (instance, _, lights) in list.iter() {
                let mut instance = *instance;
                instance.flags[2] = self.instances.len() as f32;
                self.instances.push(instance);
                self.light_lists.push(*lights);
            }
        }
        // Farthest first.
        blended.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, key, double_sided, mut instance, lights) in blended {
            self.transparent.push(TransparentDraw {
                key,
                double_sided,
                index: self.instances.len() as u32,
            });
            instance.flags[2] = self.instances.len() as f32;
            self.instances.push(instance);
            self.light_lists.push(lights);
        }
        let n = self.instances.len() as u32;
        if n > self.instance_capacity {
            self.instance_capacity = n.next_power_of_two().max(self.instance_capacity * 2);
            self.instance_buffer = create_instance_buffer(device, self.instance_capacity);
        }
        // The light lists sit in the same slots as the instances, so the
        // two buffers grow together.
        if n > self.light_list_capacity {
            self.light_list_capacity = n.next_power_of_two().max(self.light_list_capacity * 2);
            self.light_list_buffer = create_light_list_buffer(device, self.light_list_capacity);
            rebind_group_2 = true;
        }
        if n > 0 {
            queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&self.instances),
            );
            queue.write_buffer(
                &self.light_list_buffer,
                0,
                bytemuck::cast_slice(&self.light_lists),
            );
        }
        stats.batches = self.batches.len() as u32;
        stats.transparent = self.transparent.len() as u32;
        stats.instances += stats.transparent;
        stats.draws += stats.transparent;
        stats.skinned = self
            .batches
            .iter()
            .filter(|b| b.key.skinned)
            .map(|b| b.count)
            .sum::<u32>()
            + self.transparent.iter().filter(|t| t.key.skinned).count() as u32;

        // The joint palettes, as the extract laid them out (an instance's
        // `emissive.w` is its first joint). The buffer never shrinks and
        // is never empty: the skinned stages bind it unconditionally.
        // With extra worlds, their palettes follow the main extract's
        // (`joint_base` above).
        let n_joints = joint_base;
        if n_joints > self.joint_capacity {
            self.joint_capacity = n_joints.next_power_of_two().max(self.joint_capacity * 2);
            self.joint_buffer = create_joint_buffer(device, self.joint_capacity);
            rebind_group_2 = true;
        }
        if rebind_group_2 {
            self.joint_bind_group = create_joint_bind_group(
                device,
                &self.skin_layout,
                &self.joint_buffer,
                &self.light_list_buffer,
                &self.local_light_buffer,
            );
        }
        if stats.skinned > 0 && n_joints > 0 {
            if extras.is_empty() {
                queue.write_buffer(&self.joint_buffer, 0, bytemuck::cast_slice(&extract.joints));
            } else {
                self.joint_scratch.clear();
                self.joint_scratch.extend_from_slice(&extract.joints);
                for x in &extras {
                    self.joint_scratch.extend_from_slice(&x.extract.joints);
                }
                queue.write_buffer(
                    &self.joint_buffer,
                    0,
                    bytemuck::cast_slice(&self.joint_scratch),
                );
            }
            stats.joints = n_joints;
        }

        // The outline's instances, in their own buffer. The draws are
        // grouped by pipeline (static before skinned, culled before
        // double-sided) so the mask pass switches at most three times;
        // each keeps its slot. Every outlined instance is in view, so a
        // skinned one's palette went up above with the scene's.
        let n_outlined = self.outline_draws.len() as u32;
        if n_outlined > 0 {
            self.outline_draws
                .sort_by_key(|d| (d.key.skinned, d.double_sided));
            if n_outlined > self.outline_capacity {
                self.outline_capacity = n_outlined.next_power_of_two();
                self.outline_buffer = create_instance_buffer(device, self.outline_capacity);
            }
            queue.write_buffer(
                &self.outline_buffer,
                0,
                bytemuck::cast_slice(&self.outline_instances),
            );
        }
        self.outline_pass.prepare(
            device,
            queue,
            outline.map(|o| o.style).filter(|_| n_outlined > 0),
            (self.width, self.height),
            self.pipelines.encode_srgb,
        );
        stats.outlined = if self.outline_pass.active() {
            n_outlined
        } else {
            0
        };

        // The sun's shadow: drawn when the profile draws shadows, the sun
        // casts and something is there to cast.
        self.shadow_active = casters_total > 0;
        let mut shadow = [0.0f32; 4];
        let mut cascades = [GpuCascade::default(); shadow::CASCADES];
        if fit.is_some() && self.shadow_active {
            // The cascades are fitted now, not before the loop: the
            // casters' bounds are what tightens them, and they are only
            // known once every record has been through (R1).
            let fitted = shadow::fit_cascades(
                sun_dir,
                &shape,
                self.shadow_map.cascade_side(),
                if unbounded_caster {
                    None
                } else {
                    caster_bounds
                },
            );
            shadow = [
                1.0,
                1.0 / self.shadow_map.resolution as f32,
                shadow::CASCADE_BLEND,
                shadow::CASCADES as f32,
            ];
            for (i, f) in fitted.fits.iter().enumerate() {
                cascades[i] = GpuCascade {
                    view_proj: f.view_proj.to_cols_array_2d(),
                    rect: self.shadow_map.cascade_rect(i),
                    params: [f.texel_world * SHADOW_NORMAL_OFFSET_TEXELS, 0.0, 0.0, 0.0],
                };
                queue.write_buffer(
                    &self.shadow_uniform_buffer,
                    i as u64 * SHADOW_UNIFORM_STRIDE,
                    bytemuck::bytes_of(&ShadowUniforms {
                        view_proj: f.view_proj.to_cols_array_2d(),
                        time: frame_time,
                        camera_pos: camera_pos.extend(1.0).to_array(),
                    }),
                );
            }
            stats.shadow_casters = casters_total;
            stats.shadow_map = self.shadow_map.resolution;
            stats.shadow_cascades = shadow::CASCADES as u32;
            // Every batch with casters is drawn once per cascade.
            stats.shadow_draws *= shadow::CASCADES as u32;
        } else {
            self.shadow_active = false;
            stats.shadow_draws = 0;
        }

        // Volumes (R3a): the frame's look, blended at the camera.
        let (post, volumes) = self.resolve_volumes(extract, camera_pos);
        self.post = post;
        stats.volumes = volumes;

        // Ambient occlusion (R2) and contact shadows (R3d): each is on when
        // the quality profile allows it (a volume cannot turn it on) and
        // the volumes give it an intensity — contact shadows also need a
        // sun. The screen-space stage runs for either when there is
        // opaque geometry for the prepass; the other one's channel stays 1.
        let opaque = self.attachments.ao.is_some() && stats.draws > stats.transparent;
        let ao_on = opaque && self.quality.ambient_occlusion && post.ao_intensity > 0.0;
        let contact_on = opaque
            && self.quality.contact_shadows
            && sun_present
            && post.contact_intensity > 0.0
            && post.contact_length > 0.0
            && post.contact_distance > 0.0;
        self.ao_active = ao_on || contact_on;
        stats.ambient_occlusion = ao_on;
        stats.contact_shadows = contact_on;
        // Bloom (R3b): the profile allows it and a volume gives it an
        // intensity; otherwise the post pass adds nothing, bit for bit.
        self.bloom_active = self.attachments.bloom.is_some() && post.bloom_intensity > 0.0;
        stats.bloom = self.bloom_active;
        // Colour grading (R3c): arithmetic in the post pass, no stage of
        // its own; neutral skips it.
        stats.grading = post.grading.is_active();

        let u = FrameUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            cascades,
            camera_pos: camera_pos
                .extend(exposure::exposure_factor(ev100))
                .to_array(),
            sun_dir: sun_dir.extend(sun_present as u32 as f32).to_array(),
            sun_color: sun_color.extend(0.0).to_array(),
            ambient: ambient.extend(white).to_array(),
            sky: [
                env.sky_intensity.max(0.0),
                env.sky_rotation,
                stats.ibl as u32 as f32,
                (ibl_mips - 1) as f32,
            ],
            shadow,
            ao: [
                post.ao_radius,
                if ao_on { post.ao_intensity } else { 0.0 },
                0.0,
                0.0,
            ],
            contact: if contact_on {
                [
                    post.contact_intensity.clamp(0.0, 1.0),
                    post.contact_length,
                    post.contact_thickness.max(1e-4),
                    post.contact_distance,
                ]
            } else {
                [0.0; 4]
            },
            bloom: [
                post.bloom_threshold.max(0.0),
                post.bloom_threshold.max(0.0) * 0.5,
                // Unity's mapping: neither end leaves a level out.
                0.05 + 0.9 * post.bloom_scatter.clamp(0.0, 1.0),
                if self.bloom_active {
                    post.bloom_intensity
                } else {
                    0.0
                },
            ],
            bloom_tint: post.bloom_tint.max(Vec3::ZERO).extend(0.0).to_array(),
            grade: GpuGrade::new(&post.grading),
            counts: [
                n_lights as u32,
                stats.sky as u32,
                self.pipelines.encode_srgb as u32,
                self.ao_active as u32,
            ],
            time: frame_time,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&u));
        self.last_time = Some(extract.time);

        // The overlay (ADR-0019): its queue and the debug lines, uploaded
        // and cleared for the next frame.
        self.overlay_pass.prepare(
            device,
            queue,
            &mut self.overlay,
            &mut self.debug,
            overlay::OverlayFrame {
                view_proj,
                camera_pos,
                target: (self.width, self.height),
                depth_view: &self.attachments.depth_view,
                depth: (self.attachments.width, self.attachments.height),
                encode_srgb: self.pipelines.encode_srgb,
            },
        );
        let counts = self.overlay_pass.counts();
        stats.debug_lines = counts.lines;
        stats.overlay_triangles = counts.triangles;
        stats.grid = counts.grid;

        stats.extra_worlds = extras.len() as u32;
        self.assets_pending = self.frame_assets_deferred || !self.assets_settled(extract, &extras);
        extras.clear();
        self.extra_worlds = extras;
        self.light_frame = LightFrameScratch {
            lights,
            packed,
            masks,
            bounds,
        };
        stats.gpu_bytes = self.gpu_bytes();
        stats.prepare_us = prepare_start.elapsed().as_micros() as u32;
        stats.asset_upload_bytes = self.upload_usage.bytes;
        stats.asset_upload_operations = self.upload_usage.operations;
        stats.asset_upload_deferred = self.upload_usage.deferred;
        self.stats = stats;
        stats.instances
    }
}
