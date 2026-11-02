//! Skins (stage 9 phase 2): which objects bend a model copy's skinned
//! meshes. A skin's joints are the copy's own objects, one per glTF joint
//! node, so a clip that moves them bends the mesh, and a sword made a
//! child of a hand follows it.

use std::sync::Arc;

use glam::Mat4;

use crate::ObjectId;
use crate::world::World;

/// The skeleton that bends one object's mesh: its joints, and where each
/// was when the mesh was bound to it. Shared by every mesh one skin bends.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    joints: Arc<[ObjectId]>,
    inverse_binds: Arc<[Mat4]>,
}

impl Skin {
    pub(crate) fn new(joints: Arc<[ObjectId]>, inverse_binds: Arc<[Mat4]>) -> Self {
        debug_assert_eq!(joints.len(), inverse_binds.len());
        Self { joints, inverse_binds }
    }

    /// The joint objects, in the order the mesh's joint numbers count.
    pub fn joints(&self) -> &[ObjectId] {
        &self.joints
    }

    /// Each joint's world matrix when the mesh was bound, inverted: a
    /// vertex moves by the joint's world matrix times this.
    pub fn inverse_binds(&self) -> &[Mat4] {
        &self.inverse_binds
    }

    /// Which skeleton it is, the same for every mesh that shares it while
    /// it lives: their joints need working out once.
    pub fn key(&self) -> usize {
        self.joints.as_ptr() as *const () as usize
    }
}

impl World {
    pub(crate) fn add_skin(&mut self, object: ObjectId, skin: Skin) {
        self.skins.insert(object, skin);
        self.renderers_version += 1;
    }

    /// The skin that bends `object`'s mesh, if a model copy's.
    pub fn skin(&self, object: ObjectId) -> Option<&Skin> {
        self.skins.get(&object)
    }

    /// Whether any object has a skin.
    pub fn has_skins(&self) -> bool {
        !self.skins.is_empty()
    }
}
