//! Meshes for mesh colliders (docs/stage-5.md, step 6), each given once
//! and shared by every collider made of it: triangles for a static or
//! kinematic one (Unity's non-convex `MeshCollider`), or their convex hull
//! (a convex one, or one on a dynamic body). The hull is worked out the
//! first time it's needed; the unscaled shapes are made once and shared,
//! and the last few scaled ones too, so a level's copies of a mesh at one
//! scale share one shape.

use std::cell::{Cell, OnceCell, RefCell};
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
    // The last scaled shapes made or used, the latest first.
    scaled: RefCell<Vec<Scaled>>,
}

/// How many scaled shapes a mesh keeps.
const SCALED: usize = 8;

#[derive(Debug)]
struct Scaled {
    scale: Vector,
    convex: bool,
    shape: Option<SharedShape>,
}

#[derive(Debug, Default)]
pub(crate) struct Meshes {
    meshes: HashMap<u64, Mesh>,
    last: u64,
    // How many shapes were made of them.
    made: Cell<usize>,
}

impl Mesh {
    fn new(positions: &[Vector], indices: &[[u32; 3]]) -> Self {
        Mesh {
            points: positions.to_vec(),
            triangles: indices.to_vec(),
            hull: OnceCell::new(),
            whole: OnceCell::new(),
            whole_hull: OnceCell::new(),
            scaled: RefCell::new(Vec::with_capacity(SCALED)),
        }
    }

    fn hull(&self) -> Option<&Triangles> {
        self.hull.get_or_init(|| try_convex_hull(&self.points).ok()).as_ref()
    }

    /// Its triangles scaled by `scale`, or None if they can't be made.
    fn triangles(&self, scale: Vector, made: &Cell<usize>) -> Option<SharedShape> {
        let make = |scale: Vector| {
            made.set(made.get() + 1);
            let points = self.points.iter().map(|p| *p * scale).collect();
            SharedShape::trimesh_with_flags(points, self.triangles.clone(), TriMeshFlags::FIX_INTERNAL_EDGES).ok()
        };
        if scale == Vector::ONE {
            self.whole.get_or_init(|| make(scale)).clone()
        } else {
            self.scaled(scale, false, make)
        }
    }

    /// Its hull scaled by `scale`, or None if it can't be made.
    fn convex(&self, scale: Vector, made: &Cell<usize>) -> Option<SharedShape> {
        let (points, triangles) = self.hull()?;
        let make = |scale: Vector| {
            made.set(made.get() + 1);
            let points = points.iter().map(|p| *p * scale).collect();
            SharedShape::convex_mesh(points, triangles)
        };
        if scale == Vector::ONE {
            self.whole_hull.get_or_init(|| make(scale)).clone()
        } else {
            self.scaled(scale, true, make)
        }
    }

    /// The shape at `scale` from the last few, or made by `make` and kept.
    fn scaled(
        &self,
        scale: Vector,
        convex: bool,
        make: impl FnOnce(Vector) -> Option<SharedShape>,
    ) -> Option<SharedShape> {
        let mut scaled = self.scaled.borrow_mut();
        if let Some(i) = scaled.iter().position(|s| s.scale == scale && s.convex == convex) {
            scaled[..=i].rotate_right(1);
            return scaled[0].shape.clone();
        }
        let shape = make(scale);
        if scaled.len() == SCALED {
            scaled.pop();
        }
        scaled.insert(0, Scaled { scale, convex, shape: shape.clone() });
        shape
    }
}

impl Meshes {
    /// The shape of mesh `id` scaled by `scale`: its hull when `convex`.
    /// When that can't be made (squashed to nothing, say), the triangles
    /// instead of a hull, and a point instead of triangles.
    pub(crate) fn shape(&self, id: u64, convex: bool, scale: Vector, smallest: f32) -> SharedShape {
        let Some(mesh) = self.meshes.get(&id) else { return SharedShape::ball(smallest) };
        let scale = scale.max(Vector::splat(smallest));
        let made = &self.made;
        let shape = if convex {
            mesh.convex(scale, made).or_else(|| mesh.triangles(scale, made))
        } else {
            mesh.triangles(scale, made)
        };
        shape.unwrap_or_else(|| SharedShape::ball(smallest))
    }

    /// Refuses a mesh that isn't there, or makes no shape: a convex one
    /// whose points are all on a line, say.
    pub(crate) fn check(&self, id: u64, convex: bool) -> Result<(), PhysicsError> {
        let mesh = self.meshes.get(&id).ok_or_else(|| PhysicsError::Value(format!("no mesh {id}")))?;
        if convex && mesh.convex(Vector::ONE, &self.made).is_none() {
            return Err(PhysicsError::Value(format!("mesh {id} has no convex hull: are its points all on a line?")));
        }
        if !convex && mesh.triangles(Vector::ONE, &self.made).is_none() {
            return Err(PhysicsError::Value(format!("mesh {id}'s triangles make no shape")));
        }
        Ok(())
    }

    pub(crate) fn contains(&self, id: u64) -> bool {
        self.meshes.contains_key(&id)
    }
}

fn check_mesh(positions: &[Vector], indices: &[[u32; 3]]) -> Result<(), PhysicsError> {
    if indices.is_empty() {
        return Err(PhysicsError::Value("a mesh needs at least one triangle".into()));
    }
    if let Some(p) = positions.iter().find(|p| !p.is_finite()) {
        return Err(PhysicsError::Value(format!("a mesh's positions must be finite, not {p}")));
    }
    if let Some(&i) = indices.iter().flatten().find(|&&i| i as usize >= positions.len()) {
        return Err(PhysicsError::Value(format!("a mesh's index {i} is past its {} positions", positions.len())));
    }
    Ok(())
}

impl Physics {
    /// Adds a mesh for mesh colliders: `positions`, and three indices into
    /// them per triangle. Returns its id, never 0 and never reused.
    pub fn add_mesh(&mut self, positions: &[Vector], indices: &[[u32; 3]]) -> Result<u64, PhysicsError> {
        check_mesh(positions, indices)?;
        let meshes = &mut self.meshes;
        meshes.last += 1;
        meshes.meshes.insert(meshes.last, Mesh::new(positions, indices));
        Ok(meshes.last)
    }

    /// Gives mesh `id` new points and triangles, and each collider made of
    /// it its new shape (Unity's `MeshCollider.sharedMesh` set again after
    /// the mesh changed). Refused as [`Physics::add_mesh`] refuses, and then
    /// nothing changes.
    pub fn set_mesh(&mut self, id: u64, positions: &[Vector], indices: &[[u32; 3]]) -> Result<(), PhysicsError> {
        if !self.meshes.contains(id) {
            return Err(PhysicsError::Value(format!("no mesh {id}")));
        }
        check_mesh(positions, indices)?;
        self.meshes.meshes.insert(id, Mesh::new(positions, indices));
        let using: Vec<_> = self
            .world
            .colliders
            .iter()
            .filter_map(|(handle, _)| {
                let followed = self.followed.get(handle.into_raw_parts().0 as usize)?.as_ref()?;
                matches!(followed.spec.shape, ColliderShape::Mesh { mesh, .. } if mesh == id).then_some((
                    handle,
                    followed.spec,
                    followed.object,
                ))
            })
            .collect();
        for (handle, spec, object) in using {
            self.replace(handle, spec, object);
        }
        Ok(())
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

    /// How many shapes were made of meshes so far: each mesh's own, and one
    /// for each scale it's used at, shared by the colliders at that scale.
    pub fn mesh_shapes_made(&self) -> usize {
        self.meshes.made.get()
    }
}
