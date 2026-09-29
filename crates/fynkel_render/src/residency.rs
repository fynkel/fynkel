//! The renderer's side of the asset store: meshes, textures, materials,
//! surfaces and volumes loaded on first use, dropped when the store evicts.

use super::*;

impl ExtractRenderer {
    /// Where meshes, textures and materials come from. Replacing the
    /// store drops every loaded asset (they are reloaded on demand);
    /// setting a clone of the current store is a no-op.
    pub fn set_assets(&mut self, assets: Option<Assets>) {
        if let (Some(a), Some(b)) = (&self.assets, &assets) {
            if a.same_store(b) {
                return;
            }
        }
        self.evictions_seen = assets.as_ref().map(Assets::evictions).unwrap_or(0);
        self.live_edits_seen = assets.as_ref().map(Assets::live_edits).unwrap_or(0);
        self.mesh_edits_seen = assets.as_ref().map(Assets::mesh_edits).unwrap_or(0);
        self.material_edits_seen = assets.as_ref().map(Assets::material_edits).unwrap_or(0);
        self.assets = assets;
        self.clear_assets();
    }

    pub fn assets(&self) -> Option<&Assets> {
        self.assets.as_ref()
    }

    /// Drops the GPU copies of assets the store evicted since the last
    /// frame (a scene unload released them); nothing to do otherwise.
    pub(crate) fn reconcile_evictions(&mut self, queue: &wgpu::Queue) {
        let Some(assets) = &self.assets else { return };
        // A material or volume profile edited in place (the editor's
        // Inspector): every bind group is rebuilt and every profile re-read
        // from the store on its next use.
        let edits = assets.live_edits();
        if edits != self.live_edits_seen {
            self.live_edits_seen = edits;
            self.materials.clear();
            self.broken_materials.clear();
            self.volume_profiles.clear();
        }
        // A mesh the game made, changed or removed (`world.createMesh`):
        // those GPU copies go, and the next use uploads what the store has.
        let mesh_edits = assets.mesh_edits();
        if mesh_edits != self.mesh_edits_seen {
            for r in assets.meshes_changed_since(self.mesh_edits_seen) {
                self.meshes.remove(&r);
            }
            self.mesh_edits_seen = mesh_edits;
        }
        // A shader material the game made or changed
        // (`world.createShaderMaterial`): new values alone rewrite its
        // uniform; anything else rebuilds it on its next use.
        let material_edits = assets.material_edits();
        if material_edits != self.material_edits_seen {
            for r in assets.materials_changed_since(self.material_edits_seen) {
                let now = assets.custom_material(&r).ok().flatten();
                let rewritten = match (self.materials.get(&r), &now) {
                    (Some(Some(gpu)), Some(now)) => match (&gpu.custom, &gpu.block) {
                        (Some(was), Some(block)) if same_but_values(was, now) => {
                            queue.write_buffer(block, 0, &now.block);
                            true
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if rewritten {
                    if let Some(Some(gpu)) = self.materials.get_mut(&r) {
                        gpu.custom = now;
                    }
                } else {
                    self.materials.remove(&r);
                    self.broken_materials.remove(&r);
                }
            }
            self.material_edits_seen = material_edits;
        }
        let now = assets.evictions();
        if now == self.evictions_seen {
            return;
        }
        self.evictions_seen = now;
        let a = assets.clone();
        self.meshes
            .retain(|r, _| a.is_resident(r.asset) || a.is_runtime_mesh(r));
        self.textures.retain(|r, _| a.is_resident(r.asset));
        self.environments.retain(|r, _| a.is_resident(r.asset));
        self.volume_profiles.retain(|r, _| a.is_resident(r.asset));
        let textures = &self.textures;
        self.materials.retain(|r, m| {
            a.is_resident(r.asset)
                && m.as_ref()
                    .is_none_or(|m| m.textures.iter().all(|t| textures.contains_key(t)))
        });
        self.broken_materials.retain(|r| a.is_resident(r.asset));
        self.retain_used_surfaces();
        if let Some((r, _)) = &self.sky_bind_group {
            if !self.textures.contains_key(r) {
                self.sky_bind_group = None;
            }
        }
        if let Some((r, _)) = &self.ibl {
            if !a.is_resident(r.asset) {
                self.ibl = None;
                self.ibl_dirty = true;
            }
        }
        self.asset_errors.clear();
    }

    /// Forgets every loaded asset and error; the next `prepare` reloads
    /// what the extract references.
    pub fn clear_assets(&mut self) {
        self.meshes.clear();
        self.textures.clear();
        self.environments.clear();
        self.materials.clear();
        self.broken_materials.clear();
        self.surfaces.clear();
        self.volume_profiles.clear();
        self.sky_bind_group = None;
        self.ibl_dirty |= self.ibl.take().is_some();
        self.asset_errors.clear();
    }

    /// One line per asset that failed to load since `clear_assets`
    /// (each asset is tried once).
    pub fn asset_errors(&self) -> &[String] {
        &self.asset_errors
    }

    // ----------------------------------------------------------- prepare

    /// The GPU memory estimate (`FrameStats::gpu_bytes`): what this
    /// renderer created and still holds. Driver-side copies, pipeline
    /// state and the swapchain are not counted.
    pub(crate) fn gpu_bytes(&self) -> u64 {
        let meshes: u64 = self
            .meshes
            .values()
            .flatten()
            .map(|m| {
                m.vertex_buffer.size()
                    + m.index_buffer.size()
                    + m.skin_buffer.as_ref().map_or(0, wgpu::Buffer::size)
            })
            .sum();
        let textures: u64 = self.textures.values().flatten().map(|t| t.bytes).sum();
        meshes
            + textures
            + self.ibl.as_ref().map_or(0, |(_, i)| i.bytes())
            + self.ibl_none.bytes()
            + self.brdf_lut.bytes
            + self.cube.vertex_buffer.size()
            + self.cube.index_buffer.size()
            + self.attachments.bytes
            + self.shadow_map.bytes()
            + self.instance_buffer.size()
            + self.joint_buffer.size()
            + self.overlay_pass.bytes()
            + self.outline_pass.bytes()
            + self.outline_buffer.size()
            + self.uniform_buffer.size()
            + self.shadow_uniform_buffer.size()
            + self.post_buffer.size()
    }

    // -------------------------------------------------------------- draw

    pub(crate) fn mesh_for(&self, key: &BatchKey) -> Option<&GpuMesh> {
        match &key.mesh {
            Some(r) => self.meshes.get(r).and_then(Option::as_ref),
            None => Some(&self.cube),
        }
    }

    pub(crate) fn material_for<'a>(
        &'a self,
        key: &BatchKey,
        fallbacks: &'a Fallbacks,
    ) -> &'a wgpu::BindGroup {
        match &key.material {
            Some(r) => match self.materials.get(r).and_then(Option::as_ref) {
                Some(m) => &m.bind_group,
                None => &fallbacks.default_material,
            },
            None => &fallbacks.default_material,
        }
    }

    // ------------------------------------------------------------ assets

    pub(crate) fn ensure_fallbacks(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.fallbacks.is_some() {
            return;
        }
        let white = upload_rgba8(
            device,
            queue,
            Rgba8Image::flat(wgpu::TextureFormat::Rgba8Unorm, &[255, 255, 255, 255]),
            "fynkel white",
        );
        let flat_normal = upload_rgba8(
            device,
            queue,
            Rgba8Image::flat(wgpu::TextureFormat::Rgba8Unorm, &[128, 128, 255, 255]),
            "fynkel flat normal",
        );
        let black = upload_rgba8(
            device,
            queue,
            Rgba8Image::flat(wgpu::TextureFormat::Rgba8Unorm, &[0, 0, 0, 255]),
            "fynkel black",
        );
        let gray = upload_rgba8(
            device,
            queue,
            Rgba8Image::flat(wgpu::TextureFormat::Rgba8Unorm, &[128, 128, 128, 255]),
            "fynkel gray",
        );
        let default_material = create_material_bind_group(
            device,
            &self.material_layout,
            &self.sampler,
            [&white, &flat_normal, &white, &white, &white].map(|t| &t.view),
            "fynkel default material",
        );
        write_brdf_lut(queue, &self.brdf_lut_texture);
        self.fallbacks = Some(Fallbacks {
            white,
            flat_normal,
            black,
            gray,
            default_material,
        });
    }

    /// Uploads what a background scene load made resident
    /// ([`Assets::next_warm`]) until the frame's warm budget is spent, so
    /// the scene's first frames after its activation upload nothing
    /// (ADR-0038 §5). How many it uploaded.
    pub(crate) fn warm_up(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> u32 {
        if self.warm_budget_us == 0 {
            return 0;
        }
        let budget = std::time::Duration::from_micros(self.warm_budget_us as u64);
        let started = std::time::Instant::now();
        let mut warmed = 0;
        while started.elapsed() < budget {
            let Some(next) = self.assets.as_ref().and_then(|a| a.next_warm()) else {
                break;
            };
            match next {
                Warm::Mesh(r) => {
                    self.mesh(device, &r);
                }
                Warm::Texture(r) => {
                    self.texture(device, queue, &r);
                }
                Warm::Material(r) => {
                    self.material(device, queue, &r);
                }
            }
            warmed += 1;
        }
        warmed
    }

    /// Loads the mesh on first use; `true` when it is on the GPU.
    pub(crate) fn mesh(&mut self, device: &wgpu::Device, r: &AssetRef) -> bool {
        if let Some(m) = self.meshes.get(r) {
            return m.is_some();
        }
        let loaded = match self.assets.as_ref().map(|a| a.mesh(r)) {
            Some(Ok(payload)) => Some(upload_mesh(
                device,
                &format!("fynkel mesh {r}"),
                &payload.vertices,
                &payload.indices,
                &payload.skin,
                Sphere::from_bounds(payload.bounds_min, payload.bounds_max),
            )),
            Some(Err(e)) => {
                self.asset_errors.push(format!("mesh {r}: {}", e.message));
                None
            }
            None => {
                self.asset_errors
                    .push(format!("mesh {r}: no asset source (set_assets)"));
                None
            }
        };
        let ok = loaded.is_some();
        self.meshes.insert(r.clone(), loaded);
        ok
    }

    pub(crate) fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        r: &AssetRef,
    ) -> bool {
        if let Some(t) = self.textures.get(r) {
            return t.is_some();
        }
        let loaded = match self.assets.as_ref().map(|a| a.texture(r)) {
            Some(Ok(payload)) => {
                let format = match payload.format {
                    TEXTURE_FORMAT_RGBA8_SRGB => wgpu::TextureFormat::Rgba8UnormSrgb,
                    TEXTURE_FORMAT_RGBA16F_LINEAR => wgpu::TextureFormat::Rgba16Float,
                    _ => wgpu::TextureFormat::Rgba8Unorm,
                };
                Some(upload_rgba8(
                    device,
                    queue,
                    Rgba8Image {
                        width: payload.width,
                        height: payload.height,
                        format,
                        mip_levels: payload.mip_levels,
                        rgba8: &payload.texels,
                    },
                    &format!("fynkel texture {r}"),
                ))
            }
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("texture {r}: {}", e.message));
                None
            }
            None => None,
        };
        let ok = loaded.is_some();
        self.textures.insert(r.clone(), loaded);
        ok
    }

    /// The frame's look (R3a): every volume whose profile reads, by its
    /// weight × how far inside its shape `eye` is, blended over the
    /// engine defaults (`fynkel_core::volume::resolve`); and how many
    /// contributed.
    pub(crate) fn resolve_volumes(&mut self, extract: &Extract, eye: Vec3) -> (PostSettings, u32) {
        if extract.volumes.is_empty() {
            return (PostSettings::default(), 0);
        }
        let mut amounts = Vec::with_capacity(extract.volumes.len());
        for v in &extract.volumes {
            let amount = v.volume.weight * volume::factor(&v.volume, &v.world, eye);
            if amount > 0.0 && self.volume_profile(&v.volume.profile).is_some() {
                amounts.push((v.volume.profile.clone(), amount, v.volume.priority));
            }
        }
        let mut contributions: Vec<volume::Contribution<'_>> = amounts
            .iter()
            .filter_map(|(r, amount, priority)| {
                let profile = self.volume_profiles.get(r)?.as_ref()?;
                Some(volume::Contribution {
                    profile,
                    amount: *amount,
                    priority: *priority,
                })
            })
            .collect();
        volume::resolve(PostSettings::default(), &mut contributions)
    }

    /// A volume profile, read once per reference (R3a); `None` when it
    /// could not be (reported once in `asset_errors`) or there is no
    /// asset store. A nil reference is "no profile", not an error.
    pub(crate) fn volume_profile(&mut self, r: &AssetRef) -> Option<VolumeProfile> {
        if r.is_nil() {
            return None;
        }
        if let Some(p) = self.volume_profiles.get(r) {
            return *p;
        }
        let loaded = match self.assets.as_ref().map(|a| a.volume_profile(r)) {
            Some(Ok(p)) => Some(p),
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("volume profile {r}: {}", e.message));
                None
            }
            None => None,
        };
        self.volume_profiles.insert(r.clone(), loaded);
        loaded
    }

    /// Reads the environment cooked next to an HDR sky (E1), once per
    /// reference; `false` when the texture has none (an 8-bit sky) or it
    /// could not be read.
    pub(crate) fn environment(&mut self, r: &AssetRef) -> bool {
        if let Some(e) = self.environments.get(r) {
            return e.is_some();
        }

        let loaded = match self.assets.as_ref().map(|a| a.environment(r)) {
            Some(Ok(e)) => e,
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("environment {r}: {}", e.message));
                None
            }
            None => None,
        };
        let ok = loaded.is_some();
        self.environments.insert(r.clone(), loaded);
        ok
    }

    /// The cooked material with its maps bound, loaded on first use; the
    /// model's defaults (and the default bind group: key `None`) when it
    /// cannot be read. A nil reference is not an error: "no material".
    /// A shader asset's material compiles its shader on first use
    /// (ADR-0031) and is the error material when that fails.
    pub(crate) fn material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        r: &AssetRef,
    ) -> LoadedMaterial {
        let plain = LoadedMaterial {
            key: None,
            material: PbrMaterial::default(),
            casts: true,
            broken: false,
        };
        if r.is_nil() {
            return plain;
        }
        if self.broken_materials.contains(r) {
            return LoadedMaterial {
                broken: true,
                ..plain
            };
        }
        if let Some(m) = self.materials.get(r) {
            return match m {
                Some(m) => LoadedMaterial {
                    key: Some(r.clone()),
                    material: m.material.clone(),
                    casts: self.casts(m.shader),
                    broken: false,
                },
                None => plain,
            };
        }
        let custom = match self.assets.as_ref().map(|a| a.custom_material(r)) {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("material {r}: {}", e.message));
                self.materials.insert(r.clone(), None);
                return plain;
            }
            None => None,
        };
        if let Some(c) = custom {
            return match self.custom_material(device, queue, r, &c) {
                Some(gpu) => {
                    let out = LoadedMaterial {
                        key: Some(r.clone()),
                        material: gpu.material.clone(),
                        casts: self.casts(gpu.shader),
                        broken: false,
                    };
                    self.materials.insert(r.clone(), Some(gpu));
                    out
                }
                None => {
                    self.broken_materials.insert(r.clone());
                    LoadedMaterial {
                        broken: true,
                        ..plain
                    }
                }
            };
        }
        let loaded = match self.assets.as_ref().map(|a| a.material(r)) {
            Some(Ok(m)) => Some(m),
            Some(Err(e)) => {
                self.asset_errors
                    .push(format!("material {r}: {}", e.message));
                None
            }
            None => None,
        };
        let gpu = loaded.map(|material| {
            let slots = [
                &material.base_color_texture,
                &material.normal_texture,
                &material.metallic_roughness_texture,
                &material.occlusion_texture,
                &material.emissive_texture,
            ];
            let mut textures = Vec::new();
            let mut resolved: Vec<Option<AssetRef>> = Vec::with_capacity(5);
            for slot in slots {
                let have = slot.as_ref().filter(|t| self.texture(device, queue, t));
                if let Some(t) = have {
                    textures.push(t.clone());
                }
                resolved.push(have.cloned());
            }
            let fallbacks = self.fallbacks.as_ref().expect("ensure_fallbacks ran");
            let views: [&wgpu::TextureView; 5] = std::array::from_fn(|i| match &resolved[i] {
                Some(t) => &self.textures[t].as_ref().expect("resolved").view,
                None if i == 1 => &fallbacks.flat_normal.view,
                None => &fallbacks.white.view,
            });
            let bind_group = create_material_bind_group(
                device,
                &self.material_layout,
                &self.sampler,
                views,
                &format!("fynkel material {r}"),
            );
            GpuMaterial {
                material,
                bind_group,
                textures,
                shader: None,
                custom: None,
                block: None,
            }
        });
        let out = match &gpu {
            Some(g) => LoadedMaterial {
                key: Some(r.clone()),
                material: g.material.clone(),
                casts: true,
                broken: false,
            },
            None => plain,
        };
        self.materials.insert(r.clone(), gpu);
        out
    }

    /// Whether a material drawn with `shader` casts the sun's shadow.
    pub(crate) fn casts(&self, shader: Option<Uuid>) -> bool {
        shader.is_none_or(|id| self.surface(id).is_some_and(SurfaceShader::casts_shadows))
    }

    pub(crate) fn surface(&self, id: Uuid) -> Option<&SurfaceShader> {
        self.surfaces.get(&id).and_then(Option::as_ref)
    }

    /// The shader asset a batch draws with; `None`: the built-in.
    pub(crate) fn surface_for(&self, key: &BatchKey) -> Option<&SurfaceShader> {
        let m = self.materials.get(key.material.as_ref()?)?.as_ref()?;
        self.surface(m.shader?)
    }

    /// Drops the compiled shaders no material uses any more.
    pub(crate) fn retain_used_surfaces(&mut self) {
        let used: HashSet<Uuid> = self
            .materials
            .values()
            .flatten()
            .filter_map(|m| m.shader)
            .collect();
        self.surfaces.retain(|id, _| used.contains(id));
    }

    /// A shader asset's material (ADR-0031): its shader compiled on first
    /// use, its block in a uniform buffer, each texture slot bound (or its
    /// fallback texel) with the slot's sampler. `None` — reported once —
    /// when the shader is missing or does not compile, or the material
    /// was cooked for another version of it.
    pub(crate) fn custom_material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        r: &AssetRef,
        c: &Arc<CustomMaterialPayload>,
    ) -> Option<GpuMaterial> {
        let id = Uuid::from_bytes(c.shader);
        if !self.surfaces.contains_key(&id) {
            let built = self.surface_shader(device, id);
            self.surfaces.insert(id, built);
        }
        let (size, count) = {
            let s = self.surface(id)?;
            (
                s.shader.layout.size as usize,
                s.shader.interface.textures().count(),
            )
        };
        if c.block.len() != size || c.slots.len() != count {
            self.asset_errors.push(format!(
                "material {r}: cooked for another version of its shader (recook)"
            ));
            return None;
        }
        let mut textures = Vec::new();
        let mut resolved: Vec<Option<AssetRef>> = Vec::with_capacity(count);
        for slot in &c.slots {
            let t = slot.texture.as_ref().map(|(asset, sub)| AssetRef {
                asset: Uuid::from_bytes(*asset),
                sub: sub.clone(),
            });
            let have = t.filter(|t| self.texture(device, queue, t));
            if let Some(t) = &have {
                textures.push(t.clone());
            }
            resolved.push(have);
        }
        for slot in &c.slots {
            let key = self.slot_sampler_key(slot);
            self.surface_samplers
                .entry(key)
                .or_insert_with(|| create_slot_sampler(device, key));
        }
        let surface = self.surface(id).expect("compiled above");
        let fallbacks = self.fallbacks.as_ref().expect("ensure_fallbacks ran");
        let block = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("fynkel material {r} block")),
            contents: &c.block,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: block.as_entire_binding(),
        }];
        for (i, (slot, t)) in c.slots.iter().zip(&resolved).enumerate() {
            let view = match t {
                Some(t) => &self.textures[t].as_ref().expect("resolved").view,
                None => match slot.fallback {
                    FALLBACK_BLACK => &fallbacks.black.view,
                    FALLBACK_NORMAL => &fallbacks.flat_normal.view,
                    FALLBACK_GRAY => &fallbacks.gray.view,
                    _ => &fallbacks.white.view,
                },
            };
            let sampler = &self.surface_samplers[&self.slot_sampler_key(slot)];
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + 2 * i as u32,
                resource: wgpu::BindingResource::TextureView(view),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 2 + 2 * i as u32,
                resource: wgpu::BindingResource::Sampler(sampler),
            });
        }
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("fynkel material {r}")),
            layout: &surface.layout,
            entries: &entries,
        });
        // The instance carries no factors for a shader asset (its values
        // are in the block): white, and the material's alpha and sides.
        let material = PbrMaterial {
            base_color: glam::Vec4::ONE,
            emissive: Vec3::ZERO,
            alpha_mode: match c.alpha_mode {
                ALPHA_MASK => AlphaMode::Mask,
                ALPHA_BLEND => AlphaMode::Blend,
                _ => AlphaMode::Opaque,
            },
            alpha_cutoff: c.alpha_cutoff,
            double_sided: c.double_sided,
            ..PbrMaterial::default()
        };
        Some(GpuMaterial {
            material,
            bind_group,
            textures,
            shader: Some(id),
            custom: Some(c.clone()),
            block: Some(block),
        })
    }

    /// Reads and compiles shader asset `id`; `None` (reported) when it
    /// cannot be.
    pub(crate) fn surface_shader(
        &mut self,
        device: &wgpu::Device,
        id: Uuid,
    ) -> Option<SurfaceShader> {
        let text = match self.assets.as_ref()?.shader_source(id) {
            Ok(t) => t,
            Err(e) => {
                self.asset_errors
                    .push(format!("shader {id}: {}", e.message));
                return None;
            }
        };
        let shader = match fynkel_shader::parse(None, &text) {
            Ok(s) => s,
            Err(d) => {
                let why = d.first().map_or("does not read", |d| d.message.as_str());
                self.asset_errors.push(format!("shader {id}: {why}"));
                return None;
            }
        };
        let ctx = SurfaceContext {
            frame: &self.frame_layout,
            shadow: &self.shadow_layout,
            skin: &self.skin_layout,
            sample_count: self.pipelines.sample_count,
        };
        match SurfaceShader::new(device, &ctx, shader) {
            Ok(s) => Some(s),
            Err(e) => {
                self.asset_errors.push(format!("shader {id}: {e}"));
                None
            }
        }
    }

    /// A slot's sampler settings: filter, mipmaps, wrap and anisotropy —
    /// at least the quality's, as the built-in's maps, where the filter
    /// allows it.
    pub(crate) fn slot_sampler_key(&self, slot: &MaterialSlot) -> [u32; 4] {
        let anisotropy = if slot.filter == 0 && slot.mipmaps {
            slot.anisotropy.max(self.quality.anisotropy).clamp(1, 16)
        } else {
            1
        };
        [slot.filter, slot.mipmaps as u32, slot.wrap, anisotropy]
    }
}
