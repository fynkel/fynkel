//! Visibility uses the last published GPU geometry/material description.
//! Unprepared resources remain unbounded until available, so this never hides
//! a request or guesses bounds for a custom vertex shader.
use super::*;
impl ExtractRenderer {
    pub(crate) fn prepare_visibility(
        &mut self,
        extract: &Extract,
        extras: &[ExtraWorld],
        camera: &Frustum,
        shadow: Option<&Frustum>,
        stats: &mut FrameStats,
    ) {
        let start = std::time::Instant::now();
        self.visibility_bounds.clear();
        let mut authored = Vec::new();
        for (source, placement) in std::iter::once((extract, None))
            .chain(extras.iter().map(|e| (&e.extract, Some(e.transform))))
        {
            let base = self.visibility_bounds.len();
            // Mesh records get their conservative authored bound below. Do not
            // transform a fallback cube only to overwrite it immediately.
            authored.clear();
            authored.resize(source.records.len(), false);
            for mesh in &source.meshes {
                if let Some(mark) = authored.get_mut(mesh.record as usize) {
                    *mark = true;
                }
            }
            self.visibility_bounds
                .extend(source.records.iter().zip(&authored).map(|(r, has_mesh)| {
                    if *has_mesh {
                        Sphere::UNBOUNDED
                    } else {
                        let world = placement.map_or(r.world, |p| p * r.world);
                        self.cube.bounds.transformed(&world)
                    }
                }));
            for m in &source.meshes {
                let Some(record) = source.records.get(m.record as usize) else {
                    continue;
                };
                let world = placement.map_or(record.world, |p| p * record.world);
                let bound = match (
                    self.meshes.get(&m.mesh).and_then(Option::as_ref),
                    self.materials.get(&m.material),
                ) {
                    (Some(mesh), material) if m.material.is_nil() || material.is_some() => {
                        let deforms = material
                            .and_then(Option::as_ref)
                            .and_then(|m| m.shader)
                            .and_then(|s| self.surface(s))
                            .is_some_and(|s| s.shader.has_vertex());
                        if deforms {
                            Sphere::UNBOUNDED
                        } else if let Some(palette) = m.skin.filter(|_| mesh.skin_buffer.is_some())
                        {
                            let joints = (palette.first_joint as usize)
                                .checked_add(palette.joint_count as usize)
                                .and_then(|end| {
                                    source.joints.get(palette.first_joint as usize..end)
                                });
                            match (&mesh.skin_bounds, joints) {
                                (Some(bounds), Some(joints)) => {
                                    stats.bounds_joints += bounds.joint_count();
                                    bounds.posed(&world, joints)
                                }
                                _ => Sphere::UNBOUNDED,
                            }
                        } else {
                            mesh.bounds.transformed(&world)
                        }
                    }
                    _ => Sphere::UNBOUNDED,
                };
                self.visibility_bounds[base + m.record as usize] = bound;
            }
        }
        let bounds_us = start.elapsed().as_micros().min(u32::MAX as u128) as u32;
        stats.visibility_update_kind = self.visibility_index.update(&self.visibility_bounds) as u32;
        let update_us = start.elapsed().as_micros().min(u32::MAX as u128) as u32;
        stats.visibility_nodes += self
            .visibility_index
            .query(camera, &mut self.camera_visible) as u32;
        if let Some(shadow) = shadow {
            stats.visibility_nodes +=
                self.visibility_index
                    .query(shadow, &mut self.shadow_visible) as u32;
        } else {
            self.shadow_visible.clear();
            self.shadow_visible
                .resize(self.visibility_bounds.len(), false);
        }
        let total_us = start.elapsed().as_micros().min(u32::MAX as u128) as u32;
        stats.visibility_phases_us = [bounds_us, update_us - bounds_us, total_us - update_us];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_bounds_preserve_missing_meshes_holes_and_extra_world_placement() {
        let Some((device, _queue)) = headless_device() else {
            return;
        };
        let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 16, 16);
        let record = |slot, world| fynkel_core::ExtractRecord {
            entity: fynkel_contracts::Entity::new(fynkel_contracts::WorldId(1), slot, 1),
            world,
            layer_bit: 1,
        };
        let mesh = |record| fynkel_core::ExtractMesh {
            record,
            mesh: AssetRef::default(),
            material: AssetRef::default(),
            cast_shadows: true,
            receive_shadows: true,
            skin: None,
        };
        let mut main = Extract {
            records: vec![
                record(1, Mat4::IDENTITY),
                record(2, Mat4::from_translation(Vec3::X)),
            ],
            meshes: vec![mesh(1), mesh(1), mesh(u32::MAX)],
            ..Default::default()
        };
        let extra = Extract {
            records: vec![record(3, Mat4::from_scale(Vec3::splat(2.0)))],
            ..Default::default()
        };
        let placement = Mat4::from_translation(Vec3::new(3.0, 4.0, 5.0));
        let extras = [ExtraWorld {
            extract: extra,
            transform: placement,
            gray: false,
        }];
        let camera = Frustum::from_view_proj(&Mat4::IDENTITY);
        let mut stats = FrameStats::default();
        renderer.prepare_visibility(&main, &extras, &camera, None, &mut stats);
        assert_eq!(
            renderer.visibility_bounds,
            vec![
                renderer.cube.bounds.transformed(&Mat4::IDENTITY),
                Sphere::UNBOUNDED,
                renderer
                    .cube
                    .bounds
                    .transformed(&(placement * extras[0].extract.records[0].world))
            ]
        );
        assert!(renderer.shadow_visible.iter().all(|v| !v));
        // Reverse which main record is authored, then remove extras entirely.
        main.meshes = vec![mesh(0)];
        renderer.prepare_visibility(&main, &[], &camera, Some(&camera), &mut stats);
        assert_eq!(
            renderer.visibility_bounds,
            vec![
                Sphere::UNBOUNDED,
                renderer.cube.bounds.transformed(&main.records[1].world)
            ]
        );
        assert_eq!(renderer.camera_visible, renderer.shadow_visible);
        main.meshes.clear();
        main.records[0].world.x_axis.x = f32::NAN;
        renderer.prepare_visibility(&main, &[], &camera, None, &mut stats);
        assert_eq!(renderer.visibility_bounds[0], Sphere::UNBOUNDED);
        assert!(renderer.camera_visible[0]);
    }
}
