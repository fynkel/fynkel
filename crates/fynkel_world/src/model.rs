//! Spawning a loaded model (stage 2 plan §3.3): one object per glTF node,
//! drawing the model's meshes with its materials.
//!
//! The model's meshes, textures and materials are added to a world once
//! ([`World::add_model`], or one by one from Dart), and every spawn uses
//! those ids, so ten copies of a model share one set of GPU data.

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::animation::Animator;
use crate::components::{MeshRenderer, Transform};
use crate::resources::{Blend, Material};
use crate::skin::Skin;
use crate::world::{Result, World, WorldError};
use crate::{Color, MaterialData, MaterialId, MeshId, ModelData, ObjectId, TextureId};

/// A model's resources in one world, index for index with
/// [`ModelData::meshes`], [`ModelData::images`] and
/// [`ModelData::materials`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelIds {
    pub meshes: Vec<MeshId>,
    pub textures: Vec<TextureId>,
    pub materials: Vec<MaterialId>,
}

impl Material {
    /// A model's material with its images as `textures` (index for index
    /// with [`ModelData::images`]). glTF's linear colours become sRGB, and
    /// its emissive strength the intensity.
    pub fn from_model(data: &MaterialData, textures: &[TextureId]) -> Self {
        let texture = |index: Option<usize>| index.map(|i| textures[i]);
        let [r, g, b] = data.emissive;
        Self {
            base_color: Color::from_linear(data.base_color),
            metallic: data.metallic,
            roughness: data.roughness,
            emissive: Color::from_linear([r, g, b, 1.0]),
            emissive_intensity: data.emissive_strength,
            uv_scale: Vec2::from_array(data.uv_scale),
            uv_offset: Vec2::from_array(data.uv_offset),
            alpha_cutoff: data.alpha_cutoff,
            double_sided: data.double_sided,
            blend: if data.transparent { Blend::Transparent } else { Blend::Opaque },
            base_color_texture: texture(data.base_color_texture),
            normal_texture: texture(data.normal_texture),
            metallic_roughness_texture: texture(data.metallic_roughness_texture),
            occlusion_texture: texture(data.occlusion_texture),
            emissive_texture: texture(data.emissive_texture),
        }
    }
}

impl World {
    /// Adds a model's meshes, images and materials, shared with the model
    /// (not copied), ready for [`World::spawn_model`]. On error nothing is
    /// added.
    pub fn add_model(&mut self, model: &ModelData) -> Result<ModelIds> {
        let meshes: Vec<MeshId> = model.meshes().iter().map(|m| self.add_mesh(m.clone())).collect();
        let textures: Vec<TextureId> = model.images().iter().map(|i| self.add_texture(i.clone())).collect();
        let mut materials = Vec::with_capacity(model.materials().len());
        for data in model.materials() {
            match self.add_material(Material::from_model(data, &textures)) {
                Ok(id) => materials.push(id),
                Err(e) => {
                    for id in materials {
                        self.remove_material(id).expect("added above, unused");
                    }
                    for id in textures {
                        self.remove_texture(id).expect("added above, unused");
                    }
                    for id in meshes {
                        self.remove_mesh(id).expect("added above, unused");
                    }
                    return Err(e);
                }
            }
        }
        Ok(ModelIds { meshes, textures, materials })
    }

    /// Spawns a model: a root object at `transform`, named after the
    /// model's scene, with one object per node below it, named and placed
    /// as in the file. A node that draws one primitive gets a
    /// [`MeshRenderer`]; one that draws several gets an unnamed child per
    /// primitive. Returns the root.
    ///
    /// `meshes` and `materials` are the model's resources in this world,
    /// index for index with [`ModelData::meshes`] and
    /// [`ModelData::materials`]. On error nothing is spawned.
    pub fn spawn_model(
        &mut self,
        model: &ModelData,
        meshes: &[MeshId],
        materials: &[MaterialId],
        transform: Transform,
    ) -> Result<ObjectId> {
        for (what, given, needed) in
            [("meshes", meshes.len(), model.meshes().len()), ("materials", materials.len(), model.materials().len())]
        {
            if given != needed {
                return Err(WorldError::ModelIds { what, given, needed });
            }
        }
        let root = self.spawn();
        match self.spawn_nodes(model, meshes, materials, root, transform) {
            Ok(()) => Ok(root),
            Err(e) => {
                self.despawn(root).expect("spawned above");
                Err(e)
            }
        }
    }

    fn spawn_nodes(
        &mut self,
        model: &ModelData,
        meshes: &[MeshId],
        materials: &[MaterialId],
        root: ObjectId,
        transform: Transform,
    ) -> Result<()> {
        self.insert(root, transform)?;
        self.set_name(root, Some(model.name()))?;
        let mut stack: Vec<(usize, ObjectId)> = model.roots().iter().rev().map(|&n| (n, root)).collect();
        // Each node's object, for the animator; the root's entity until
        // the node's is made (every node is reached once).
        let animated = !model.clips().is_empty();
        let mut entities = if animated { vec![self.entity(root)?; model.nodes().len()] } else { Vec::new() };
        // Each node's object, for the skins' joints, and each skinned
        // part's object with its skin.
        let skinned = !model.skins().is_empty();
        let mut objects = if skinned { vec![root; model.nodes().len()] } else { Vec::new() };
        let mut bent: Vec<(ObjectId, usize)> = Vec::new();
        while let Some((index, parent)) = stack.pop() {
            let node = &model.nodes()[index];
            let object = self.spawn();
            if skinned {
                objects[index] = object;
            }
            if animated {
                entities[index] = self.entity(object)?;
            }
            self.set_parent(object, Some(parent))?;
            self.set_name(object, Some(&node.name))?;
            self.insert(
                object,
                Transform {
                    position: Vec3::from_array(node.translation),
                    rotation: Quat::from_array(node.rotation),
                    scale: Vec3::from_array(node.scale),
                },
            )?;
            let renderer = |part: &fynkel_load::Part| MeshRenderer {
                mesh: Some(meshes[part.mesh]),
                material: Some(materials[part.material]),
                ..Default::default()
            };
            match node.parts.as_slice() {
                [] => {}
                [part] => {
                    self.insert(object, renderer(part))?;
                    if let Some(skin) = node.skin {
                        bent.push((object, skin));
                    }
                }
                parts => {
                    for part in parts {
                        let child = self.spawn();
                        self.set_parent(child, Some(object))?;
                        self.insert(child, Transform::default())?;
                        self.insert(child, renderer(part))?;
                        if let Some(skin) = node.skin {
                            bent.push((child, skin));
                        }
                    }
                }
            }
            stack.extend(node.children.iter().rev().map(|&c| (c, object)));
        }
        if !bent.is_empty() {
            let skins: Vec<Skin> = model
                .skins()
                .iter()
                .map(|s| {
                    Skin::new(
                        s.joints.iter().map(|&j| objects[j]).collect(),
                        s.inverse_binds.iter().map(Mat4::from_cols_array).collect(),
                    )
                })
                .collect();
            for (object, skin) in bent {
                self.add_skin(object, skins[skin].clone());
            }
        }
        if animated {
            let rest = model
                .nodes()
                .iter()
                .map(|n| Transform {
                    position: Vec3::from_array(n.translation),
                    rotation: Quat::from_array(n.rotation),
                    scale: Vec3::from_array(n.scale),
                })
                .collect();
            let names = model.nodes().iter().map(|n| n.name.clone()).collect();
            let mut parents = vec![None; model.nodes().len()];
            for (i, node) in model.nodes().iter().enumerate() {
                for &child in &node.children {
                    parents[child] = Some(i);
                }
            }
            self.add_animator(root, Animator::new(model.clips().to_vec(), entities, names, parents, rest));
        }
        Ok(())
    }
}
