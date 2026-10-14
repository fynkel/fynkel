//! Copies what the renderer needs out of the world into a [`RenderScene`]
//! (stage 1 plan §3.5): resolved, linear values the render worker can draw
//! without looking anything up.
//!
//! The rules:
//! - The camera is the view's own when it has one ([`Extract::camera`]):
//!   drawn from while it's shown and has a `Camera` and a `Transform`, and
//!   nothing is drawn otherwise. Else it's the one on the object with the
//!   lowest id that has both. Its transform's scale is ignored. A shake
//!   ([`Extract::shake`]) moves it when it's drawn, not in the world. One
//!   whose near clip isn't below its far draws nothing, so the frame is
//!   refused with a message instead.
//! - Every directional, point and spot light with a `Transform` goes to the
//!   renderer, in the order of their ids. A directional light is a sun,
//!   shining along its world forward (-Z); a point light shines from its
//!   world position; a spot light from there along its forward. Without a
//!   transform, a light has no place and is ignored. The renderer chooses
//!   which light what (its `RenderScene::lights`).
//! - Every ambient light adds to the ambient term.
//! - An object is drawn when it has a `Transform` and a `MeshRenderer`
//!   with both a mesh and a material, and its layer is in the camera's
//!   culling mask (stage 4 plan §3.9). Lights light every layer.
//! - The world's debug lines are drawn over the scene as they are.
//! - A texture is uploaded once for each colour space a material reads it
//!   in, the first frame a material uses it that way, and freed when the
//!   world removes it. Once for the world, however many views it has: what
//!   the GPU has is the world's [`Ledger`].
//! - Only shown objects count, for all of the above: an inactive object and
//!   everything below it are left out (stage 3 plan §3.3).
//! - Everything is placed by its world transform: its own after its
//!   parents' (stage 2 plan §3.1). The camera and the lights take only the
//!   position and rotation from it; a parent's scale doesn't stretch them
//!   (a light's range is its own).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::{
    CameraView, ColorSpace, Draw, Light as RenderLight, Line, MaterialTextures, MeshKey, PbrMaterial, Quality,
    RenderScene, TextureKey,
};
use fynkel_world::{
    Camera, Color, ImageData, Light, LightKind, Material, MaterialId, MeshData, MeshId, MeshRenderer, ObjectId, Quat,
    TextureId, Transform, World, WorldMatrices,
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

/// Which meshes and textures the GPU has, or is about to: those uploaded,
/// so each goes up once and is freed when the world removes it.
#[derive(Debug, Default)]
pub(crate) struct Uploaded {
    meshes: HashSet<MeshId>,
    textures: HashSet<(TextureId, ColorSpace)>,
}

/// A world's [`Uploaded`], shared by its views, and the changes still to
/// be made on the GPU, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    pub(crate) uploaded: Uploaded,
    pub(crate) ops: Vec<ResourceOp>,
}

/// One view's extract.
#[derive(Debug, Default)]
pub(crate) struct Extract {
    /// What its own renderer has, when it shares nothing ([`Extract::run`]).
    #[cfg(test)]
    own: Uploaded,
    matrices: WorldMatrices,
    /// The view's own camera, or `None` for the world's main one.
    pub(crate) camera: Option<ObjectId>,
    /// The world's camera shake: how far its camera is moved, in the
    /// world, when this view draws from it.
    pub(crate) shake: Option<(ObjectId, Vec3)>,
    /// The world's quality level, or `None` for the device's.
    pub(crate) quality: Option<Quality>,
    /// The lights found, by id, before they're put in order. Kept so a
    /// frame allocates nothing.
    lights: Vec<(ObjectId, RenderLight)>,
    /// Each material as the renderer takes it, worked out once a frame:
    /// objects are many, materials few.
    materials: HashMap<MaterialId, PbrMaterial>,
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
    /// changes since the last run to `ops`, for a renderer of its own.
    /// Allocates nothing once `scene` and `ops` have grown to size and the
    /// meshes are unchanged.
    #[cfg(test)]
    pub(crate) fn run(&mut self, world: &World, scene: &mut RenderScene, ops: &mut Vec<ResourceOp>) {
        let mut own = std::mem::take(&mut self.own);
        let filled = self.fill(world, scene, &mut own, ops);
        self.own = own;
        filled.expect("a camera that can draw");
    }

    /// [`Extract::run`] for a view that shares its world's meshes and
    /// textures: the changes go to the world's `ledger`, once for all its
    /// views. Fails, saying why, when the camera can't draw (near ≥ far):
    /// `scene` is then not to be drawn.
    pub(crate) fn run_shared(
        &mut self,
        world: &World,
        scene: &mut RenderScene,
        ledger: &mut Ledger,
    ) -> Result<(), String> {
        let Ledger { uploaded, ops } = ledger;
        self.fill(world, scene, uploaded, ops)
    }

    fn fill(
        &mut self,
        world: &World,
        scene: &mut RenderScene,
        uploaded: &mut Uploaded,
        ops: &mut Vec<ResourceOp>,
    ) -> Result<(), String> {
        scene.clear();

        uploaded.meshes.retain(|&id| {
            let kept = world.mesh(id).is_ok();
            if !kept {
                ops.push(ResourceOp::RemoveMesh(mesh_key(id)));
            }
            kept
        });
        for (id, mesh) in world.meshes() {
            if uploaded.meshes.insert(id) {
                ops.push(ResourceOp::UploadMesh(mesh_key(id), Arc::clone(mesh)));
            }
        }

        // Removals first, so a key is never freed after its new upload.
        uploaded.textures.retain(|&(id, _)| {
            let kept = world.texture(id).is_ok();
            if !kept {
                ops.push(ResourceOp::RemoveTexture(texture_key(id)));
            }
            kept
        });
        // Materials are few next to objects, so looking at each one's
        // slots, and resolving it, every frame costs little.
        self.materials.clear();
        for (id, material) in world.materials() {
            self.materials.insert(id, pbr(material));
            for (key, space) in texture_keys(material).slots() {
                let Some(key) = key else { continue };
                let id = TextureId::from_raw(key.0).expect("made from a TextureId");
                if uploaded.textures.insert((id, space)) {
                    let image = world.texture(id).expect("the world refuses a material with a missing texture");
                    ops.push(ResourceOp::UploadTexture(key, space, Arc::clone(image)));
                }
            }
        }

        self.matrices.update(world);
        let matrices = &self.matrices;
        // Where a shown object is and which way it faces, scale left out:
        // the rotation as Dart's, which a parent scaled to 0 doesn't spoil.
        let placed = |id: ObjectId, t: &Transform| -> Option<(Quat, Vec3)> {
            let position = matrices.world(world, id, t)?.w_axis.truncate();
            Some((world.world_transform(id).ok()?.rotation, position))
        };

        let mut camera: Option<(ObjectId, Camera, Quat, Vec3)> = None;
        let own = self.camera;
        world.for_each2::<Camera, Transform>(|id, c, t| {
            if own.is_none_or(|own| own == id)
                && camera.is_none_or(|(best, ..)| id < best)
                && let Some((rotation, position)) = placed(id, t)
            {
                camera = Some((id, *c, rotation, position));
            }
        });
        if let Some((id, c, ..)) = camera
            && c.near >= c.far
        {
            let name = world.name(id).ok().flatten().map_or_else(|| format!("{id:?}"), |n| format!("'{n}'"));
            return Err(format!(
                "the camera on {name} has near {} and far {}: near must be below far, or it draws nothing",
                c.near, c.far
            ));
        }
        let shake = self.shake;
        let camera = camera.map(|(id, c, rotation, position)| match shake {
            Some((shaken, by)) if shaken == id => (id, c, rotation, position + by),
            _ => (id, c, rotation, position),
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

        scene.quality = self.quality;
        let lights = &mut self.lights;
        lights.clear();
        world.for_each2::<Light, Transform>(|id, light, t| {
            if light.kind == LightKind::Ambient {
                return;
            }
            let Some((rotation, position)) = placed(id, t) else { return };
            let color = Vec3::from(rgb(light.color)) * light.intensity;
            let forward = rotation * Vec3::NEG_Z;
            lights.push((
                id,
                match light.kind {
                    LightKind::Point => RenderLight::point(position, light.range, color),
                    LightKind::Spot => RenderLight::spot(
                        position,
                        forward,
                        light.range,
                        light.spot_angle.to_radians(),
                        light.softness,
                        color,
                    ),
                    _ => RenderLight::sun(forward, color),
                },
            ));
        });
        lights.sort_unstable_by_key(|&(id, _)| id);
        scene.lights.extend(lights.iter().map(|&(_, light)| light));
        world.for_each::<Light>(|id, light| {
            if light.kind == LightKind::Ambient && matrices.shown(world, id) {
                scene.ambient += Vec3::from(rgb(light.color)) * light.intensity;
            }
        });

        let materials = &self.materials;
        // Objects next to each other often share a material: the last one
        // found is kept.
        let mut last: Option<(MaterialId, PbrMaterial)> = None;
        world.for_each2::<MeshRenderer, Transform>(|id, r, t| {
            let (Some(mesh), Some(material)) = (r.mesh, r.material) else { return };
            if !world.in_mask(id, mask) {
                return;
            }
            let Some(model) = matrices.world(world, id, t) else { return };
            let material = match last {
                Some((at, m)) if at == material => m,
                _ => {
                    // The world refuses a renderer pointing at a missing
                    // material, so this always succeeds.
                    let Some(&m) = materials.get(&material) else { return };
                    last = Some((material, m));
                    m
                }
            };
            let mut material = material;
            if r.own_color {
                material.base_color = r.color.to_linear();
            }
            scene.draws.push(Draw { mesh: mesh_key(mesh), model, material, cast_shadows: true, receive_shadows: true });
        });

        scene.lines.extend(world.debug_lines().iter().map(|l| Line { a: l.a, b: l.b, color: l.color.to_linear() }));
        Ok(())
    }
}

/// A material as the renderer takes it: linear colours, emission times its
/// intensity.
fn pbr(m: &Material) -> PbrMaterial {
    PbrMaterial {
        base_color: m.base_color.to_linear(),
        metallic: m.metallic,
        roughness: m.roughness,
        emissive: (Vec3::from(rgb(m.emissive)) * m.emissive_intensity).into(),
        uv_scale: m.uv_scale.into(),
        uv_offset: m.uv_offset.into(),
        alpha_cutoff: m.alpha_cutoff,
        double_sided: m.double_sided,
        textures: texture_keys(m),
    }
}

/// Linear RGB, alpha dropped.
fn rgb(color: Color) -> [f32; 3] {
    let [r, g, b, _] = color.to_linear();
    [r, g, b]
}

#[cfg(test)]
mod tests;
