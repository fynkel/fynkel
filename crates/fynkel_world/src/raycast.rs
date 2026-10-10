//! Raycasts against the meshes the world draws (stage 2 plan §3.4).
//!
//! No physics: a ray is tested against each drawn object's box first, then
//! against its triangles, on the CPU. What a ray can hit is what a view
//! draws, the way the view draws it:
//! - An object is hit when it has a `Transform` and a `MeshRenderer` with
//!   both a mesh and a material, and it and its parents are active: the
//!   extract's rule for drawing it.
//! - Only objects on the layers in the cast's mask are hit.
//! - A triangle is hit only from its front, unless the material is
//!   double-sided, as back faces are culled. A mirrored object (negative
//!   scale) keeps its fronts where they were, as the renderer's mirrored
//!   pipelines do.
//! - Cut-outs (`alpha_cutoff`) are hit as if solid: the texture isn't read.
//!
//! The test runs in each object's own space, so its matrix is inverted once
//! and no triangle is moved. A ray's distance is the same number in both
//! spaces when its direction is carried over unnormalised.

use glam::{Mat3, Mat4, Vec3};

use crate::components::{MeshRenderer, Transform};
use crate::hierarchy::WorldMatrices;
use crate::{ObjectId, World};

/// A half-line: from `origin`, along `direction` (unit length).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    origin: Vec3,
    direction: Vec3,
}

impl Ray {
    /// `None` when `origin` isn't finite or `direction` has no length.
    pub fn new(origin: Vec3, direction: Vec3) -> Option<Self> {
        let direction = direction.try_normalize()?;
        origin.is_finite().then_some(Self { origin, direction })
    }

    pub fn origin(&self) -> Vec3 {
        self.origin
    }

    /// Unit length.
    pub fn direction(&self) -> Vec3 {
        self.direction
    }

    /// The point `distance` along the ray.
    pub fn at(&self, distance: f32) -> Vec3 {
        self.origin + self.direction * distance
    }
}

/// Where a ray hit first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub object: ObjectId,
    /// In the world.
    pub point: Vec3,
    /// The triangle's face normal in the world, unit length, on the side
    /// the ray came from.
    pub normal: Vec3,
    /// From the ray's origin to [`point`](Self::point).
    pub distance: f32,
}

/// Casts rays at a world. It keeps the world matrices between casts, so a
/// steady world is cast at without allocating. One of these belongs to one
/// world.
#[derive(Debug, Default)]
pub struct Raycaster {
    matrices: WorldMatrices,
}

impl Raycaster {
    /// The closest hit along `ray` no further than `max_distance`, on the
    /// layers in `mask` (bit `n` for layer `n`), in the world as it is now.
    pub fn cast(&mut self, world: &World, ray: Ray, max_distance: f32, mask: u32) -> Option<RayHit> {
        self.matrices.update(world);
        let mut best: Option<RayHit> = None;
        world.for_each2::<MeshRenderer, Transform>(|id, r, t| {
            let (Some(mesh), Some(material)) = (r.mesh, r.material) else { return };
            if !world.in_mask(id, mask) {
                return;
            }
            let Ok(mesh) = world.mesh(mesh) else { return };
            let reach = best.map_or(max_distance, |b| b.distance);
            let Some(model) = self.matrices.world(world, id, t) else { return };
            // The box around the object's box, in the world: far cheaper
            // than the inverse, which only objects the ray may hit need.
            let (lo, hi) = mesh.bounds();
            let centre = model.transform_point3((lo + hi) * 0.5);
            let half = Mat3::from_mat4(model).abs() * ((hi - lo) * 0.5);
            if !hits_box(ray.origin, ray.direction, centre - half, centre + half, reach) {
                return;
            }
            let inverse = model.inverse();
            // A flattened object (a zero scale) has no inverse and covers
            // no area.
            if model.determinant() == 0.0 || !inverse.is_finite() {
                return;
            }
            let Ok(material) = world.material(material) else { return };
            let origin = inverse.transform_point3(ray.origin);
            let direction = inverse.transform_vector3(ray.direction);
            let positions = mesh.vertices();
            let mut closest: Option<(f32, Vec3)> = None;
            for triangle in mesh.indices().chunks_exact(3) {
                let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(positions[triangle[i] as usize].position));
                let limit = closest.map_or(reach, |(d, _)| d);
                if let Some(hit) = hit_triangle(origin, direction, a, b, c, material.double_sided, limit) {
                    closest = Some(hit);
                }
            }
            if let Some((distance, face)) = closest {
                best = Some(RayHit {
                    object: id,
                    point: ray.at(distance),
                    normal: world_normal(inverse, face, ray.direction),
                    distance,
                });
            }
        });
        best
    }
}

impl World {
    /// The closest drawn object along `ray`, no further than
    /// `max_distance`, on the layers in `mask`. See the [module
    /// docs](crate::raycast) for what can be hit. A [`Raycaster`] does the
    /// same without allocating each time.
    pub fn raycast(&self, ray: Ray, max_distance: f32, mask: u32) -> Option<RayHit> {
        Raycaster::default().cast(self, ray, max_distance, mask)
    }
}

/// Whether the ray enters the box before `reach` (the slab test). A
/// direction component of 0 gives infinities, which compare the right way.
fn hits_box(origin: Vec3, direction: Vec3, lo: Vec3, hi: Vec3, reach: f32) -> bool {
    let inv = direction.recip();
    let a = (lo - origin) * inv;
    let b = (hi - origin) * inv;
    // NaN (0 × ∞, a ray along a face) falls out of min and max on the side
    // that keeps the box.
    let near = a.min(b).max_element().max(0.0);
    let far = a.max(b).min_element().min(reach);
    near <= far
}

/// Möller–Trumbore. The distance along the ray and the triangle's
/// unnormalised face normal (front side, counter-clockwise), when the ray
/// meets it from a side it can be hit from, closer than `limit`.
fn hit_triangle(
    origin: Vec3,
    direction: Vec3,
    a: Vec3,
    b: Vec3,
    c: Vec3,
    double_sided: bool,
    limit: f32,
) -> Option<(f32, Vec3)> {
    let (ab, ac) = (b - a, c - a);
    let p = direction.cross(ac);
    let det = ab.dot(p);
    // det > 0: the ray meets the front. Near 0: it runs along the plane.
    if det.abs() < 1e-12 || (det < 0.0 && !double_sided) {
        return None;
    }
    let inv = det.recip();
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(ab);
    let v = direction.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = ac.dot(q) * inv;
    (t >= 0.0 && t < limit).then(|| (t, ab.cross(ac)))
}

/// A face normal from an object's space in the world (by the inverse
/// transpose, which keeps it at right angles to the face under any scale),
/// turned to face the ray.
fn world_normal(inverse: Mat4, face: Vec3, direction: Vec3) -> Vec3 {
    let n = inverse.transpose().transform_vector3(face).normalize();
    if n.dot(direction) > 0.0 { -n } else { n }
}
