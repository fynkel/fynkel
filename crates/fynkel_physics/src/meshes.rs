//! Meshes for mesh colliders (docs/stage-5.md, step 6), each given once
//! and shared by every collider made of it: triangles for a static or
//! kinematic one (Unity's non-convex `MeshCollider`), or their convex hull
//! (a convex one, or one on a dynamic body). The hull is worked out the
//! first time it's needed; the unscaled shapes are made once and shared.

use std::cell::OnceCell;
use std::collections::HashMap;

use rapier3d::parry::transformation::try_convex_hull;
use rapier3d::prelude::*;

use crate::{ColliderShape, Physics, PhysicsError, Vector};

/// Points, and three indices into them per triangle.
type Triangles = (Vec<Vector>, Vec<[u32; 3]>);

/// A mesh's points and triangles, and what's been made of them.
#[derive(Debug)]
struct Mesh {
    points: Vec<Vector>,
    triangles: Vec<[u32; 3]>,
    // The hull's points and triangles, or None when there's none.
    hull: OnceCell<Option<Triangles>>,
    // The unscaled shapes.
    whole: OnceCell<Option<SharedShape>>,
    whole_hull: OnceCell<Option<SharedShape>>,
}

#[derive(Debug, Default)]
pub(crate) struct Meshes {
    meshes: HashMap<u64, Mesh>,
    last: u64,
}

impl Mesh {
    fn hull(&self) -> Option<&Triangles> {
        self.hull.get_or_init(|| try_convex_hull(&self.points).ok()).as_ref()
    }

    /// Its triangles scaled by `scale`, or None if they can't be made.
    fn triangles(&self, scale: Vector) -> Option<SharedShape> {
        let make = |scale: Vector| {
            let points = self.points.iter().map(|p| *p * scale).collect();
            SharedShape::trimesh_with_flags(points, self.triangles.clone(), TriMeshFlags::FIX_INTERNAL_EDGES).ok()
        };
        if scale == Vector::ONE { self.whole.get_or_init(|| make(scale)).clone() } else { make(scale) }
    }

    /// Its hull scaled by `scale`, or None if it can't be made.
    fn convex(&self, scale: Vector) -> Option<SharedShape> {
        let (points, triangles) = self.hull()?;
        let make = |scale: Vector| {
            let points = points.iter().map(|p| *p * scale).collect();
            SharedShape::convex_mesh(points, triangles)
        };
        if scale == Vector::ONE { self.whole_hull.get_or_init(|| make(scale)).clone() } else { make(scale) }
    }
}

impl Meshes {
    /// The shape of mesh `id` scaled by `scale`: its hull when `convex`.
    /// When that can't be made (squashed to nothing, say), the triangles
    /// instead of a hull, and a point instead of triangles.
    pub(crate) fn shape(&self, id: u64, convex: bool, scale: Vector, smallest: f32) -> SharedShape {
        let Some(mesh) = self.meshes.get(&id) else { return SharedShape::ball(smallest) };
        let scale = scale.max(Vector::splat(smallest));
        let shape = if convex { mesh.convex(scale).or_else(|| mesh.triangles(scale)) } else { mesh.triangles(scale) };
        shape.unwrap_or_else(|| SharedShape::ball(smallest))
    }

    /// Refuses a mesh that isn't there, or makes no shape: a convex one
    /// whose points are all on a line, say.
    pub(crate) fn check(&self, id: u64, convex: bool) -> Result<(), PhysicsError> {
        let mesh = self.meshes.get(&id).ok_or_else(|| PhysicsError::Value(format!("no mesh {id}")))?;
        if convex && mesh.convex(Vector::ONE).is_none() {
            return Err(PhysicsError::Value(format!("mesh {id} has no convex hull: are its points all on a line?")));
        }
        if !convex && mesh.triangles(Vector::ONE).is_none() {
            return Err(PhysicsError::Value(format!("mesh {id}'s triangles make no shape")));
        }
        Ok(())
    }

    pub(crate) fn contains(&self, id: u64) -> bool {
        self.meshes.contains_key(&id)
    }
}

impl Physics {
    /// Adds a mesh for mesh colliders: `positions`, and three indices into
    /// them per triangle. Returns its id, never 0 and never reused.
    pub fn add_mesh(&mut self, positions: &[Vector], indices: &[[u32; 3]]) -> Result<u64, PhysicsError> {
        if indices.is_empty() {
            return Err(PhysicsError::Value("a mesh needs at least one triangle".into()));
        }
        if let Some(p) = positions.iter().find(|p| !p.is_finite()) {
            return Err(PhysicsError::Value(format!("a mesh's positions must be finite, not {p}")));
        }
        if let Some(&i) = indices.iter().flatten().find(|&&i| i as usize >= positions.len()) {
            return Err(PhysicsError::Value(format!("a mesh's index {i} is past its {} positions", positions.len())));
        }
        let meshes = &mut self.meshes;
        meshes.last += 1;
        meshes.meshes.insert(
            meshes.last,
            Mesh {
                points: positions.to_vec(),
                triangles: indices.to_vec(),
                hull: OnceCell::new(),
                whole: OnceCell::new(),
                whole_hull: OnceCell::new(),
            },
        );
        Ok(meshes.last)
    }

    /// Removes a mesh no collider is made of.
    pub fn remove_mesh(&mut self, id: u64) -> Result<(), PhysicsError> {
        if !self.meshes.contains(id) {
            return Err(PhysicsError::Value(format!("no mesh {id}")));
        }
        let used = self
            .followed
            .iter()
            .flatten()
            .any(|f| matches!(f.spec.shape, ColliderShape::Mesh { mesh, .. } if mesh == id));
        if used {
            return Err(PhysicsError::Value(format!("mesh {id} is used by a collider")));
        }
        self.meshes.meshes.remove(&id);
        Ok(())
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.meshes.len()
    }
}
