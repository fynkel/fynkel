//! The stand-in without the `rapier` feature (ADR-0029 `modules.strip`):
//! the adapter's API over nothing. It keeps no bodies — a `RigidBody` or
//! a `Collider` stays a component — answers no queries and reports no
//! events; gravity, collision meshes and physics materials are only
//! remembered, so the loader's registrations still succeed. The world
//! never steps it: a runtime built without Rapier runs with the physics
//! module off (`Modules::COMPILED`).

use std::collections::{BTreeSet, HashMap};

use fynkel_contracts::{AssetRef, Entity};
use fynkel_ecs::World as Ecs;

use crate::{
    BodyCommand, BodyState, CharacterState, Hit, PhysicsEvent, PhysicsMaterial, PhysicsStats,
    QuerySpec, Rejection, Vec3, MAX_MESH_COLLIDER_TRIANGLES,
};

pub struct Physics {
    gravity: Vec3,
    meshes: HashMap<AssetRef, usize>,
    materials: HashMap<AssetRef, PhysicsMaterial>,
}

impl Default for Physics {
    fn default() -> Self {
        Self::new()
    }
}

impl Physics {
    pub fn new() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            meshes: HashMap::new(),
            materials: HashMap::new(),
        }
    }

    pub fn gravity(&self) -> Vec3 {
        self.gravity
    }

    pub fn set_gravity(&mut self, gravity: Vec3) {
        if gravity.is_finite() {
            self.gravity = gravity;
        }
    }

    /// Remembers the mesh's triangle count (refused above the limit, as
    /// the adapter refuses it).
    pub fn register_mesh(
        &mut self,
        key: AssetRef,
        positions: &[[f32; 3]],
        indices: &[u32],
    ) -> Result<usize, String> {
        let triangles = indices.len() / 3;
        if triangles == 0 {
            return Err("mesh has no triangles".into());
        }
        if triangles > MAX_MESH_COLLIDER_TRIANGLES {
            return Err(format!(
                "mesh has {triangles} triangles, more than the {MAX_MESH_COLLIDER_TRIANGLES} a mesh collider allows"
            ));
        }
        let n = positions.len() as u32;
        if indices.iter().any(|i| *i >= n) {
            return Err("mesh index out of range".into());
        }
        self.meshes.insert(key, triangles);
        Ok(triangles)
    }

    pub fn has_mesh(&self, key: &AssetRef) -> bool {
        self.meshes.contains_key(key)
    }

    /// No collider is built, so none uses a mesh.
    pub fn uses_mesh(&self, _key: &AssetRef) -> bool {
        false
    }

    pub fn replace_mesh(
        &mut self,
        key: &AssetRef,
        mesh: Option<(&[[f32; 3]], &[u32])>,
    ) -> Result<(), String> {
        // As the adapter: a refused replacement leaves the mesh forgotten.
        self.meshes.remove(key);
        match mesh {
            Some((positions, indices)) => self
                .register_mesh(key.clone(), positions, indices)
                .map(|_| ()),
            None => Ok(()),
        }
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    pub fn register_material(
        &mut self,
        key: AssetRef,
        material: PhysicsMaterial,
    ) -> Result<(), String> {
        material.validate()?;
        self.materials.insert(key, material);
        Ok(())
    }

    pub fn has_material(&self, key: &AssetRef) -> bool {
        self.materials.contains_key(key)
    }

    pub fn material(&self, key: &AssetRef) -> Option<PhysicsMaterial> {
        self.materials.get(key).copied()
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    pub fn queue(&mut self, _index: u32, _entity: Entity, _command: BodyCommand) {}

    pub fn add_root_motion(&mut self, _entity: Entity, _velocity: Vec3) {}

    pub fn has_character(&self, _entity: Entity) -> bool {
        false
    }

    pub fn stats(&self) -> PhysicsStats {
        PhysicsStats::default()
    }

    pub fn body_count(&self) -> usize {
        0
    }

    pub fn has_work(&self, _ecs: &Ecs) -> bool {
        false
    }

    pub fn events(&self) -> &[PhysicsEvent] {
        &[]
    }

    pub fn take_rejections(&mut self) -> Vec<Rejection> {
        Vec::new()
    }

    pub fn body_state(&self, _entity: Entity) -> Option<BodyState> {
        None
    }

    pub fn character_state(&self, _entity: Entity) -> Option<CharacterState> {
        None
    }

    pub fn character_count(&self) -> usize {
        0
    }

    pub fn owns(&self, _entity: Entity) -> bool {
        false
    }

    pub fn step(&mut self, _ecs: &mut Ecs, _dt: f32, _watched: &BTreeSet<Entity>) {}

    pub fn record(&mut self, _ecs: &Ecs) {}

    pub fn refresh(&mut self, _ecs: &mut Ecs) {}

    pub fn query(&self, _q: &QuerySpec, _out: &mut Vec<Hit>) {}

    pub fn debug_lines(&self, _out: &mut Vec<(Vec3, Vec3, bool)>) {}
}
