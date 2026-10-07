//! Copies what the renderer needs out of the world into a [`RenderScene`]
//! (stage 1 plan §3.5): resolved, linear values the render worker can draw
//! without looking anything up.
//!
//! The rules:
//! - The camera is the one on the object with the lowest id that has both a
//!   `Camera` and a `Transform`. Its transform's scale is ignored.
//! - The sun is the directional light with the lowest id that has a
//!   `Transform`; it shines along the transform's forward (-Z). A
//!   directional light without a transform has no direction and is
//!   ignored.
//! - Every ambient light adds to the ambient term.
//! - An object is drawn when it has a `Transform` and a `MeshRenderer`
//!   with both a mesh and a material.

use std::collections::HashSet;

use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::{CameraView, Draw, MeshKey, PbrMaterial, RenderScene, Sun};
use fynkel_world::{Camera, Color, Light, LightKind, MeshId, MeshRenderer, MeshSource, ObjectId, Transform, World};

/// A change to the meshes the renderer holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MeshOp {
    Upload(MeshKey, MeshSource),
    Remove(MeshKey),
}

/// One view's extract. It remembers which meshes the view's renderer has,
/// so each mesh is uploaded once and freed when the world removes it.
#[derive(Debug, Default)]
pub(crate) struct Extract {
    uploaded: HashSet<MeshId>,
}

pub(crate) fn mesh_key(id: MeshId) -> MeshKey {
    MeshKey(id.raw())
}

impl Extract {
    /// Fills `scene` from `world` (emptying it first) and adds the mesh
    /// changes since the last run to `ops`. Allocates nothing once `scene`
    /// and `ops` have grown to size and the meshes are unchanged.
    pub(crate) fn run(&mut self, world: &World, scene: &mut RenderScene, ops: &mut Vec<MeshOp>) {
        scene.clear();

        self.uploaded.retain(|&id| {
            let kept = world.mesh(id).is_ok();
            if !kept {
                ops.push(MeshOp::Remove(mesh_key(id)));
            }
            kept
        });
        for (id, source) in world.meshes() {
            if self.uploaded.insert(id) {
                ops.push(MeshOp::Upload(mesh_key(id), source));
            }
        }

        let mut camera: Option<(ObjectId, Camera, Transform)> = None;
        world.for_each2::<Camera, Transform>(|id, c, t| {
            if camera.is_none_or(|(best, ..)| id < best) {
                camera = Some((id, *c, *t));
            }
        });
        scene.camera = camera.map(|(_, c, t)| CameraView {
            view: Mat4::from_rotation_translation(t.rotation, t.position).inverse(),
            position: t.position,
            fov_y: c.fov_y.to_radians(),
            near: c.near,
            far: c.far,
            clear_color: rgb(c.clear_color),
        });

        let mut sun: Option<(ObjectId, Sun)> = None;
        world.for_each2::<Light, Transform>(|id, light, t| {
            if light.kind == LightKind::Directional && sun.is_none_or(|(best, _)| id < best) {
                sun = Some((id, Sun { direction: t.forward(), color: Vec3::from(rgb(light.color)) * light.intensity }));
            }
        });
        scene.sun = sun.map(|(_, s)| s);
        world.for_each::<Light>(|_, light| {
            if light.kind == LightKind::Ambient {
                scene.ambient += Vec3::from(rgb(light.color)) * light.intensity;
            }
        });

        world.for_each2::<MeshRenderer, Transform>(|_, r, t| {
            let (Some(mesh), Some(material)) = (r.mesh, r.material) else { return };
            // The world refuses a renderer pointing at a missing material,
            // so this always succeeds.
            let Ok(m) = world.material(material) else { return };
            scene.draws.push(Draw {
                mesh: mesh_key(mesh),
                model: t.matrix(),
                material: PbrMaterial {
                    base_color: m.base_color.to_linear(),
                    metallic: m.metallic,
                    roughness: m.roughness,
                },
            });
        });
    }
}

/// Linear RGB, alpha dropped.
fn rgb(color: Color) -> [f32; 3] {
    let [r, g, b, _] = color.to_linear();
    [r, g, b]
}

#[cfg(test)]
mod tests;
