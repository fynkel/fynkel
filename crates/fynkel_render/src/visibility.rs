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
        self.visibility_bounds.clear();
        for (source, placement) in std::iter::once((extract, Mat4::IDENTITY))
            .chain(extras.iter().map(|e| (&e.extract, e.transform)))
        {
            let base = self.visibility_bounds.len();
            self.visibility_bounds.extend(
                source
                    .records
                    .iter()
                    .map(|r| self.cube.bounds.transformed(&(placement * r.world))),
            );
            for m in &source.meshes {
                let Some(record) = source.records.get(m.record as usize) else {
                    continue;
                };
                let world = placement * record.world;
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
        self.visibility_index.update(&self.visibility_bounds);
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
    }
}
