//! Copies what the renderer needs out of the world into a [`RenderScene`]
//! (stage 1 plan §3.5): resolved, linear values the render worker can draw
//! without looking anything up.
//!
//! The rules:
//! - The camera is the one on the object with the lowest id that has both a
//!   `Camera` and a `Transform`. Its transform's scale is ignored.
//! - The sun is the directional light with the lowest id that has a
//!   `Transform`; it shines along its world forward (-Z). A
//!   directional light without a transform has no direction and is
//!   ignored.
//! - Every ambient light adds to the ambient term.
//! - An object is drawn when it has a `Transform` and a `MeshRenderer`
//!   with both a mesh and a material, and its layer is in the camera's
//!   culling mask (stage 4 plan §3.9). Lights light every layer.
//! - The world's debug lines are drawn over the scene as they are.
//! - A texture is uploaded once for each colour space a material reads it
//!   in, the first frame a material uses it that way, and freed when the
//!   world removes it.
//! - Only shown objects count, for all of the above: an inactive object and
//!   everything below it are left out (stage 3 plan §3.3).
//! - Everything is placed by its world transform: its own after its
//!   parents' (stage 2 plan §3.1). The camera and the sun take only the
//!   position and rotation from it; a parent's scale doesn't stretch them.

use std::collections::HashSet;
use std::sync::Arc;

use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::{
    CameraView, ColorSpace, Draw, Line, MaterialTextures, MeshKey, PbrMaterial, RenderScene, Sun, TextureKey,
};
use fynkel_world::{
    Camera, Color, ImageData, Light, LightKind, Material, MeshData, MeshId, MeshRenderer, ObjectId, Quat, TextureId,
    Transform, World, WorldMatrices,
};

/// A change to the meshes and textures the renderer holds. An upload
/// shares the world's data; nothing is copied until the GPU upload.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResourceOp {
    UploadMesh(MeshKey, Arc<MeshData>),
    RemoveMesh(MeshKey),
    UploadTexture(TextureKey, ColorSpace, Arc<ImageData>),
    /// In every colour space. Sent once for each space the texture was
    /// uploaded in; removing again does nothing.
    RemoveTexture(TextureKey),
}

/// One view's extract. It remembers which meshes and textures the view's
/// renderer has, so each is uploaded once and freed when the world removes
/// it.
#[derive(Debug, Default)]
pub(crate) struct Extract {
    uploaded: HashSet<MeshId>,
    uploaded_textures: HashSet<(TextureId, ColorSpace)>,
    matrices: WorldMatrices,
}

pub(crate) fn mesh_key(id: MeshId) -> MeshKey {
    MeshKey(id.raw())
}

pub(crate) fn texture_key(id: TextureId) -> TextureKey {
    TextureKey(id.raw())
}

/// A material's textures as the renderer names them.
fn texture_keys(m: &Material) -> MaterialTextures {
    let key = |id: Option<TextureId>| id.map(texture_key);
    MaterialTextures {
        base_color: key(m.base_color_texture),
        normal: key(m.normal_texture),
        metallic_roughness: key(m.metallic_roughness_texture),
        occlusion: key(m.occlusion_texture),
        emissive: key(m.emissive_texture),
    }
}

impl Extract {
    /// Fills `scene` from `world` (emptying it first) and adds the mesh
    /// changes since the last run to `ops`. Allocates nothing once `scene`
    /// and `ops` have grown to size and the meshes are unchanged.
    pub(crate) fn run(&mut self, world: &World, scene: &mut RenderScene, ops: &mut Vec<ResourceOp>) {
        scene.clear();

        self.uploaded.retain(|&id| {
            let kept = world.mesh(id).is_ok();
            if !kept {
                ops.push(ResourceOp::RemoveMesh(mesh_key(id)));
            }
            kept
        });
        for (id, mesh) in world.meshes() {
            if self.uploaded.insert(id) {
                ops.push(ResourceOp::UploadMesh(mesh_key(id), Arc::clone(mesh)));
            }
        }

        // Removals first, so a key is never freed after its new upload.
        self.uploaded_textures.retain(|&(id, _)| {
            let kept = world.texture(id).is_ok();
            if !kept {
                ops.push(ResourceOp::RemoveTexture(texture_key(id)));
            }
            kept
        });
        // Materials are few next to objects, so looking at each one's
        // slots every frame costs little.
        for (_, material) in world.materials() {
            for (key, space) in texture_keys(material).slots() {
                let Some(key) = key else { continue };
                let id = TextureId::from_raw(key.0).expect("made from a TextureId");
                if self.uploaded_textures.insert((id, space)) {
                    let image = world.texture(id).expect("the world refuses a material with a missing texture");
                    ops.push(ResourceOp::UploadTexture(key, space, Arc::clone(image)));
                }
            }
        }

        self.matrices.update(world);
        let matrices = &self.matrices;
        // Where a shown object is and which way it faces, scale left out.
        let placed = |id: ObjectId, t: &Transform| -> Option<(Quat, Vec3)> {
            let (_, rotation, position) = matrices.world(world, id, t)?.to_scale_rotation_translation();
            Some((rotation.normalize(), position))
        };

        let mut camera: Option<(ObjectId, Camera, Quat, Vec3)> = None;
        world.for_each2::<Camera, Transform>(|id, c, t| {
            if camera.is_none_or(|(best, ..)| id < best)
                && let Some((rotation, position)) = placed(id, t)
            {
                camera = Some((id, *c, rotation, position));
            }
        });
        // The layers drawn: all without a camera, which draws nothing anyway.
        let mask = camera.map_or(u32::MAX, |(_, c, ..)| c.culling_mask);
        scene.camera = camera.map(|(_, c, rotation, position)| CameraView {
            view: Mat4::from_rotation_translation(rotation, position).inverse(),
            position,
            fov_y: c.fov_y.to_radians(),
            orthographic: c.orthographic.then_some(c.size),
            near: c.near,
            far: c.far,
            clear_color: rgb(c.clear_color),
        });

        let mut sun: Option<(ObjectId, Sun)> = None;
        world.for_each2::<Light, Transform>(|id, light, t| {
            if light.kind == LightKind::Directional
                && sun.is_none_or(|(best, _)| id < best)
                && let Some((rotation, _)) = placed(id, t)
            {
                sun = Some((
                    id,
                    Sun { direction: rotation * Vec3::NEG_Z, color: Vec3::from(rgb(light.color)) * light.intensity },
                ));
            }
        });
        scene.sun = sun.map(|(_, s)| s);
        world.for_each::<Light>(|id, light| {
            if light.kind == LightKind::Ambient && matrices.shown(world, id) {
                scene.ambient += Vec3::from(rgb(light.color)) * light.intensity;
            }
        });

        world.for_each2::<MeshRenderer, Transform>(|id, r, t| {
            let (Some(mesh), Some(material)) = (r.mesh, r.material) else { return };
            if !world.in_mask(id, mask) {
                return;
            }
            let Some(model) = matrices.world(world, id, t) else { return };
            // The world refuses a renderer pointing at a missing material,
            // so this always succeeds.
            let Ok(m) = world.material(material) else { return };
            scene.draws.push(Draw {
                mesh: mesh_key(mesh),
                model,
                material: PbrMaterial {
                    base_color: m.base_color.to_linear(),
                    metallic: m.metallic,
                    roughness: m.roughness,
                    emissive: (Vec3::from(rgb(m.emissive)) * m.emissive_intensity).into(),
                    uv_scale: m.uv_scale.into(),
                    uv_offset: m.uv_offset.into(),
                    alpha_cutoff: m.alpha_cutoff,
                    double_sided: m.double_sided,
                    textures: texture_keys(&m),
                },
            });
        });

        scene.lines.extend(world.debug_lines().iter().map(|l| Line { a: l.a, b: l.b, color: l.color.to_linear() }));
    }
}

/// Linear RGB, alpha dropped.
fn rgb(color: Color) -> [f32; 3] {
    let [r, g, b, _] = color.to_linear();
    [r, g, b]
}

#[cfg(test)]
mod tests;
