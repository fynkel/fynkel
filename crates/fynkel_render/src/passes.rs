//! [`ExtractRenderer::draw`]: the pass chain `shadow → depth → AO → opaque
//! → transparent → bloom → post`, and the per-draw helpers.

use super::*;

impl ExtractRenderer {
    /// Records the frame's pass chain into `encoder`:
    ///
    /// | Pass | Reads | Writes |
    /// | --- | --- | --- |
    /// | shadow | caster instances, the sun fit | the shadow map (depth) |
    /// | depth prepass | opaque/mask instances | the scene depth |
    /// | ambient occlusion | the scene depth | the AO target |
    /// | AO blur | the AO target, the scene depth | the AO the mesh pass samples |
    /// | scene: opaque | opaque/mask instances, materials, lights, the shadow map | HDR colour (× samples) + depth |
    /// | scene: sky | the environment texture | HDR colour where depth is untouched |
    /// | scene: transparent | blend instances back-to-front, debug lines; depth (read) | HDR colour |
    /// | resolve | the multisampled HDR | the single-sample HDR (skipped at 1 sample) |
    /// | post | the HDR, exposure | the target — or the LDR at render scale |
    /// | presentation | the LDR | the target (skipped at scale 1) |
    ///
    /// The three scene stages are one GPU render pass (the attachments
    /// stay on-tile), resolved at its end.
    pub fn draw(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let encode_start = std::time::Instant::now();
        let present =
            self.attachments.ldr_view.is_some() && self.attachments.present_bind_group.is_some();
        let shadow = self.shadow_active;
        let ao = self.ao_active;
        if let Some(timer) = self.gpu_timer.as_mut() {
            let depth_resolve = ao
                && self
                    .attachments
                    .ao
                    .as_ref()
                    .is_some_and(|a| a.resolved_depth.is_some());
            timer.begin_frame(encoder, shadow, ao, present, depth_resolve);
        }
        if self.shadow_active {
            self.shadow_pass(encoder);
        }
        if ao {
            self.depth_prepass(encoder);
            self.ao_passes(encoder);
        }
        self.scene_pass(encoder);
        // The post pair spans the bloom stage when it runs: its bright-pass
        // writes the beginning, the post pass the end.
        let bloom = self.bloom_active;
        if bloom {
            self.bloom_passes(encoder);
        }
        let a = &self.attachments;
        let post_target = a.ldr_view.as_ref().unwrap_or(target);
        self.fullscreen_pass(
            encoder,
            "fynkel post",
            post_target,
            &self.pipelines.post,
            &a.post_bind_group,
            self.timestamps_part(2, !bloom, true),
        );
        if let (Some(_), Some(present)) = (&a.ldr_view, &a.present_bind_group) {
            self.fullscreen_pass(
                encoder,
                "fynkel present",
                target,
                &self.pipelines.present,
                present,
                self.timestamps(3),
            );
        }
        // The selection outline, then gizmos and handles, over the
        // finished frame at the target's resolution (ADR-0019; not timed):
        // the handles stay on top of the outline.
        if self.outline_pass.active() {
            self.outline_mask_pass(encoder);
            self.outline_pass.composite(encoder, target);
        }
        self.overlay_pass.draw(encoder, target);
        self.stats.encode_us = encode_start.elapsed().as_micros() as u32;
    }

    /// The pass's timestamp writes (`GpuTimer`), when the device samples
    /// them.
    pub(crate) fn timestamps(&self, pair: u32) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.gpu_timer.as_ref().and_then(|t| t.writes(pair))
    }

    /// One end of a query pair: the AO stage's three passes share pair
    /// `AO_PAIR`, so the whole stage reads as one span.
    pub(crate) fn timestamps_part(
        &self,
        pair: u32,
        begin: bool,
        end: bool,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.gpu_timer
            .as_ref()
            .and_then(|t| t.writes_part(pair, begin, end))
    }

    /// Individual screen-space passes share the existing stage endpoints
    /// (8/9), with additional samples for depth/estimate/blur attribution.
    fn screen_space_timestamps(
        &self,
        begin: u32,
        end: u32,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.gpu_timer
            .as_ref()
            .map(|timer| wgpu::RenderPassTimestampWrites {
                query_set: &timer.query_sets[timer.writing],
                beginning_of_pass_write_index: Some(begin),
                end_of_pass_write_index: Some(end),
            })
    }

    /// Depth only, from the sun, every caster inside its box (each
    /// batch's shadow range).
    pub(crate) fn shadow_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel shadow"),
            timestamp_writes: self.timestamps(0),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadow_map.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        // One quadrant of the atlas per cascade: the viewport moves, the
        // dynamic offset picks that cascade's matrix, and the same casters
        // are drawn into each — what a cascade does not cover its
        // projection clips away (R1). The viewport is the quadrant inset
        // by a texel, matching `ShadowMap::cascade_rect`, so the border
        // stays at the cleared far depth and a PCF tap at the edge of one
        // cascade reads "lit" instead of its neighbour.
        let side = self.shadow_map.cascade_side() as f32;
        for cascade in 0..shadow::CASCADES {
            let (col, row) = ((cascade % 2) as f32, (cascade / 2) as f32);
            pass.set_viewport(
                col * side + 1.0,
                row * side + 1.0,
                (side - 2.0).max(1.0),
                (side - 2.0).max(1.0),
                0.0,
                1.0,
            );
            pass.set_bind_group(
                0,
                &self.shadow_bind_group,
                &[(cascade as u64 * SHADOW_UNIFORM_STRIDE) as u32],
            );
            // Static casters, then the skinned ones: one pipeline switch.
            for skinned in [false, true] {
                let mut bound = None;
                for b in self
                    .batches
                    .iter()
                    .filter(|b| b.key.skinned == skinned && b.shadow_count > 0)
                {
                    let Some(mesh) = self.mesh_for(&b.key) else {
                        continue;
                    };
                    self.bind_pipeline(&mut pass, &mut bound, &b.key, Pass::Shadow, false);
                    pass.set_bind_group(1, self.material_for(&b.key, fallbacks), &[]);
                    bind_mesh(&mut pass, mesh, skinned);
                    pass.draw_indexed(
                        0..mesh.index_count,
                        0,
                        b.shadow_first..b.shadow_first + b.shadow_count,
                    );
                }
            }
        }
    }

    /// The camera depth prepass (R2): every opaque and mask instance the
    /// scene pass will draw, depth only, with the scene pass's own vertex
    /// stage. The scene pass then loads this depth instead of clearing —
    /// its compare is `LessEqual`, and early-z drops the overdraw the
    /// prepass has already resolved.
    pub(crate) fn depth_prepass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel depth prepass"),
            timestamp_writes: self.screen_space_timestamps(8, 10),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.attachments.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        if self.instances.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        // The scene pass's opaque loop exactly, with the colour dropped.
        for (skinned, double_sided) in [(false, false), (false, true), (true, false), (true, true)]
        {
            let mut bound = None;
            for b in self.batches.iter().filter(|b| {
                b.key.skinned == skinned && b.double_sided == double_sided && b.count > 0
            }) {
                let Some(mesh) = self.mesh_for(&b.key) else {
                    continue;
                };
                self.bind_pipeline(&mut pass, &mut bound, &b.key, Pass::Depth, double_sided);
                pass.set_bind_group(1, self.material_for(&b.key, fallbacks), &[]);
                bind_mesh(&mut pass, mesh, skinned);
                pass.draw_indexed(0..mesh.index_count, 0, b.first..b.first + b.count);
            }
        }
    }

    /// The selection outline's mask: every outlined instance's silhouette
    /// at the target's size, cleared to 0 first, no depth — what other
    /// geometry hides is in it too, as Unity outlines it.
    pub(crate) fn outline_mask_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(mask) = self.outline_pass.mask_view() else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel outline mask"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: mask,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        pass.set_vertex_buffer(1, self.outline_buffer.slice(..));
        let mut bound = None;
        for d in &self.outline_draws {
            let Some(mesh) = self.mesh_for(&d.key) else {
                continue;
            };
            self.bind_pipeline(&mut pass, &mut bound, &d.key, Pass::Mask, d.double_sided);
            pass.set_bind_group(1, self.material_for(&d.key, fallbacks), &[]);
            bind_mesh(&mut pass, mesh, d.key.skinned);
            pass.draw_indexed(0..mesh.index_count, 0, d.index..d.index + 1);
        }
    }

    /// Screen-space ambient occlusion from the prepass's depth, then the
    /// blur that averages its per-pixel rotation away (R2). Group 0 is
    /// the frame uniform alone: the mesh pass's group 0 holds the target
    /// the blur writes.
    pub(crate) fn ao_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(ao) = self.attachments.ao.as_ref() else {
            return;
        };
        if let Some((view, group)) = &ao.resolved_depth {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fynkel AO depth resolve"),
                timestamp_writes: self.screen_space_timestamps(14, 15),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipelines.ao_depth_resolve);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let label = "fynkel ao";
            let target = &ao.view;
            let pipeline = &self.pipelines.ao;
            let bind_group = &ao.ao_bind_group;
            let timestamps = self.screen_space_timestamps(11, 12);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                timestamp_writes: timestamps,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.ao_frame_bind_group, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let timestamps = self
            .gpu_timer
            .as_ref()
            .map(|timer| wgpu::ComputePassTimestampWrites {
                query_set: &timer.query_sets[timer.writing],
                beginning_of_pass_write_index: Some(13),
                end_of_pass_write_index: Some(9),
            });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fynkel tiled ao blur"),
            timestamp_writes: timestamps,
        });
        pass.set_pipeline(&self.pipelines.ao_blur);
        pass.set_bind_group(0, &self.ao_frame_bind_group, &[]);
        pass.set_bind_group(1, &ao.blur_bind_group, &[]);
        pass.dispatch_workgroups(
            self.attachments.width.div_ceil(8),
            self.attachments.height.div_ceil(8),
            1,
        );
    }

    /// Bloom (R3b): the bright-pass from the HDR frame into level 0, a
    /// downsample into each coarser level, then back up — each level
    /// replaced by itself mixed with the tent of the coarser one by
    /// `scatter` (the blend constant). Level 0 is what the post pass adds.
    pub(crate) fn bloom_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(bloom) = self.attachments.bloom.as_ref() else {
            return;
        };
        let scatter = f64::from(0.05 + 0.9 * self.post.bloom_scatter.clamp(0.0, 1.0));
        let n = bloom.levels.len();
        let mut steps: Vec<(&str, usize, &wgpu::RenderPipeline, &wgpu::BindGroup, bool)> =
            Vec::with_capacity(2 * n - 1);
        steps.push((
            "fynkel bloom prefilter",
            0,
            &self.pipelines.bloom_prefilter,
            &bloom.prefilter_bind_group,
            false,
        ));
        for i in 1..n {
            steps.push((
                "fynkel bloom down",
                i,
                &self.pipelines.bloom_down,
                &bloom.down[i - 1],
                false,
            ));
        }
        for i in (0..n - 1).rev() {
            steps.push((
                "fynkel bloom up",
                i,
                &self.pipelines.bloom_up,
                &bloom.up[i],
                true,
            ));
        }
        for (k, (label, level, pipeline, bind_group, blend)) in steps.into_iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                timestamp_writes: if k == 0 {
                    self.timestamps_part(2, true, false)
                } else {
                    None
                },
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &bloom.levels[level],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // An upsample mixes over what the downsample left.
                        load: if blend {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            if blend {
                pass.set_blend_constant(wgpu::Color {
                    r: scatter,
                    g: scatter,
                    b: scatter,
                    a: scatter,
                });
            }
            pass.set_bind_group(0, &self.ao_frame_bind_group, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Opaque batches (culled, then double-sided), the sky behind them,
    /// then the transparent instances back-to-front and the debug lines.
    pub(crate) fn scene_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let a = &self.attachments;
        let (view, resolve_target) = match &a.hdr_msaa_view {
            Some(msaa) => (msaa, Some(&a.hdr_view)),
            None => (&a.hdr_view, None),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel scene"),
            timestamp_writes: self.timestamps(1),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: if resolve_target.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &a.depth_view,
                depth_ops: Some(wgpu::Operations {
                    // The depth prepass has already written this frame's
                    // opaque depth when the AO stage ran (R2).
                    load: if self.ao_active {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(1.0)
                    },
                    // The overlay reads it after post (ADR-0019).
                    store: if self.overlay_pass.active() {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.joint_bind_group, &[]);
        let Some(fallbacks) = self.fallbacks.as_ref() else {
            return;
        };
        if !self.instances.is_empty() {
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            // Culled batches first, then the double-sided ones, static
            // before skinned: at most one pipeline switch per variant.
            for (skinned, double_sided) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                let mut bound = None;
                for b in self.batches.iter().filter(|b| {
                    b.key.skinned == skinned && b.double_sided == double_sided && b.count > 0
                }) {
                    let Some(mesh) = self.mesh_for(&b.key) else {
                        continue;
                    };
                    self.bind_pipeline(&mut pass, &mut bound, &b.key, Pass::Opaque, double_sided);
                    pass.set_bind_group(1, self.material_for(&b.key, fallbacks), &[]);
                    bind_mesh(&mut pass, mesh, skinned);
                    pass.draw_indexed(0..mesh.index_count, 0, b.first..b.first + b.count);
                }
            }
        }
        if let Some((_, sky)) = &self.sky_bind_group {
            pass.set_pipeline(&self.pipelines.sky);
            pass.set_bind_group(1, sky, &[]);
            pass.draw(0..3, 0..1);
        }
        if !self.transparent.is_empty() {
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            let mut bound = None;
            for t in &self.transparent {
                let Some(mesh) = self.mesh_for(&t.key) else {
                    continue;
                };
                self.bind_pipeline(
                    &mut pass,
                    &mut bound,
                    &t.key,
                    Pass::Transparent,
                    t.double_sided,
                );
                pass.set_bind_group(1, self.material_for(&t.key, fallbacks), &[]);
                bind_mesh(&mut pass, mesh, t.key.skinned);
                pass.draw_indexed(0..mesh.index_count, 0, t.index..t.index + 1);
            }
        }
    }

    /// One fullscreen triangle with `pipeline` and `bind_group` (group 1)
    /// into `target`, cleared to `CLEAR_COLOR` first.
    pub(crate) fn fullscreen_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            timestamp_writes,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Sets the pipeline `key` draws with in `pass` — its shader asset's
    /// (ADR-0031) or the built-in's — unless `bound` is it already.
    pub(crate) fn bind_pipeline<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'_>,
        bound: &mut Option<&'a wgpu::RenderPipeline>,
        key: &BatchKey,
        which: Pass,
        double_sided: bool,
    ) {
        let skinned = key.skinned;
        let pipeline = match self.surface_for(key) {
            Some(s) => s.pipeline(which, skinned, double_sided),
            None => match which {
                Pass::Opaque => self.pipelines.opaque(skinned, double_sided),
                Pass::Transparent => self.pipelines.transparent(skinned, double_sided),
                Pass::Depth => self.pipelines.depth(skinned, double_sided),
                Pass::Mask => self.pipelines.mask(skinned, double_sided),
                Pass::Shadow if skinned => &self.pipelines.shadow_skinned,
                Pass::Shadow => &self.pipelines.shadow,
            },
        };
        if !bound.is_some_and(|b| std::ptr::eq(b, pipeline)) {
            pass.set_pipeline(pipeline);
            *bound = Some(pipeline);
        }
    }
}

pub(crate) fn instance_for(m: &PbrMaterial, world: Mat4, receive_shadows: bool) -> Instance {
    let cutoff = match m.alpha_mode {
        AlphaMode::Mask => m.alpha_cutoff.clamp(0.0, 1.0),
        AlphaMode::Opaque => NO_CUTOFF,
        AlphaMode::Blend => BLEND,
    };
    Instance {
        model: world.to_cols_array_2d(),
        tint: m.base_color.to_array(),
        emissive: (m.emissive * m.emissive_strength).extend(0.0).to_array(),
        params: [
            m.metallic,
            m.roughness,
            m.normal_scale,
            m.occlusion_strength,
        ],
        flags: [
            cutoff,
            0.0,
            // Filled in when the instance takes its place in the buffer.
            0.0,
            if receive_shadows { 1.0 } else { 0.0 },
        ],
    }
}

/// Binds `mesh`'s vertex and index buffers — and, for a skinned draw,
/// its weights on slot 2.
pub(crate) fn bind_mesh(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh, skinned: bool) {
    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
    if skinned {
        if let Some(skin) = &mesh.skin_buffer {
            pass.set_vertex_buffer(2, skin.slice(..));
        }
    }
    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
}

/// Deterministic per-entity tint so neighbouring placeholder cubes are
/// distinguishable.
pub(crate) fn tint_for(slot: u32) -> [f32; 4] {
    const PALETTE: [[f32; 3]; 6] = [
        [0.90, 0.30, 0.25],
        [0.25, 0.60, 0.90],
        [0.30, 0.80, 0.40],
        [0.95, 0.75, 0.20],
        [0.85, 0.85, 0.90],
        [0.55, 0.35, 0.75],
    ];
    let [r, g, b] = PALETTE[(slot as usize) % PALETTE.len()];
    [r, g, b, 1.0]
}
