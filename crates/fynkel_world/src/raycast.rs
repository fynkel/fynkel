//! Raycasts against the meshes the world draws (stage 2 plan §3.4).
//!
//! No physics: a ray is tested against each drawn object's box first, then
//! against its triangles, on the CPU. What a ray can hit is what a view
//! draws, the way the view draws it:
//! - An object is hit when it has a `Transform` and a `MeshRenderer` with
//!   both a mesh and a material, and it and its parents are active: the
//!   extract's rule for drawing it.
//! - Only objects on the layers in the cast's mask are hit.
//! - A triangle is hit only from its front, unless its submesh's material
//!   is double-sided, as back faces are culled. A mirrored object
//!   (negative scale) keeps its fronts where they were, as the renderer's
//!   mirrored pipelines do.
//! - A submesh after the first is hit only when it's drawn: when the
//!   object has a material for it.
//! - A skinned mesh is hit where its joints have bent it to, as it's
//!   drawn; with a joint destroyed, where it was bound.
//! - Cut-outs (`alpha_cutoff`) are hit as if solid: the texture isn't read.
//!
//! The test runs in each object's own space, so its matrix is inverted once
//! and no triangle is moved. A still mesh of many triangles has a box tree
//! over them ([`crate::bvh`]), made the first time a ray reaches it and
//! again when the mesh changes, and the world matrices are worked out
//! again only when something moved (WO4). A ray's distance is the same number in both
//! spaces when its direction is carried over unnormalised. A skinned mesh
//! is bent into the world first, as the renderer bends it, and tested
//! there.

use std::ops::Range;

use glam::{Mat3, Mat4, Vec3};

use crate::MeshData;
use crate::bvh::{Bvh, WORTH_A_TREE};
use crate::components::{MeshRenderer, Transform};
use crate::hierarchy::WorldMatrices;
use crate::id::IdMap;
use crate::{MeshId, ObjectId, World};

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

/// Rays that reach one version of a mesh before its tree is made.
const CASTS_BEFORE_A_TREE: u32 = 16;

/// Casts rays at a world. It keeps the world matrices between casts, and
/// the trees over its meshes' triangles, so a steady world is cast at
/// without allocating. One of these belongs to one world.
#[derive(Debug, Default)]
pub struct Raycaster {
    matrices: WorldMatrices,
    /// A skin's joints as they bend its mesh, and its vertices bent.
    joints: Vec<Mat4>,
    bent: Vec<Vec3>,
    /// Each mesh's tree, with the version it was made for and the rays
    /// that reached that version: none until [`CASTS_BEFORE_A_TREE`] have,
    /// as making it costs as much as that many casts without (a mesh
    /// changed every frame and cast at a few times never gets one).
    trees: IdMap<MeshId, (u64, u32, Option<Bvh>)>,
    /// For each submesh of the mesh being tested: drawn, and if so
    /// whether double-sided.
    sides: Vec<Option<bool>>,
}

impl Raycaster {
    /// The closest hit along `ray` no further than `max_distance`, on the
    /// layers in `mask` (bit `n` for layer `n`), in the world as it is now.
    pub fn cast(&mut self, world: &World, ray: Ray, max_distance: f32, mask: u32) -> Option<RayHit> {
        let Self { matrices, joints, bent, trees, sides } = self;
        matrices.update(world);
        // Trees of meshes no longer in the world go, now and then.
        if trees.len() > 16 && trees.len() > 2 * world.mesh_count() {
            trees.retain(|&id, _| world.has_mesh(id));
        }
        let (has_skins, has_extra) = (world.has_skins(), world.has_submesh_materials());
        let mut best: Option<RayHit> = None;
        world.for_each2::<MeshRenderer, Transform>(|id, r, t| {
            let (Some(mesh_id), Some(material)) = (r.mesh, r.material) else { return };
            if !world.in_mask(id, mask) {
                return;
            }
            let Ok(mesh) = world.mesh(mesh_id) else { return };
            let reach = best.map_or(max_distance, |b| b.distance);
            let Some(model) = matrices.world(world, id, t) else { return };
            // A skinned mesh, all its joints there: bent as it's drawn.
            joints.clear();
            let skin = if has_skins && mesh.skin().is_some() { world.skin(id) } else { None };
            let skinned = skin.is_some_and(|skin| {
                skin.joints().iter().zip(skin.inverse_binds()).all(|(&joint, &inverse_bind)| {
                    matrices.matrix(world, joint).map(|m| joints.push(m * inverse_bind)).is_some()
                })
            });
            // A still mesh's box first: most are missed there.
            let into = if skinned { None } else { into_object(mesh, model, ray, reach) };
            if !skinned && into.is_none() {
                return;
            }
            // Each submesh drawn, and whether it's seen from behind.
            let extra = if has_extra { world.submesh_materials(id) } else { &[] };
            sides.clear();
            sides.extend((0..mesh.submeshes().len()).map(|k| {
                let material = if k == 0 { Some(material) } else { extra.get(k - 1).copied() };
                material.and_then(|m| world.material(m).ok()).map(|m| m.double_sided)
            }));
            let parts =
                mesh.submeshes().iter().zip(sides.iter()).filter_map(|(range, side)| Some((range.clone(), (*side)?)));
            let hit = match into {
                None => hit_skinned(mesh, joints, bent, parts, ray, reach),
                Some((inverse, origin, direction)) => {
                    let positions = mesh.vertices();
                    let at = |i: u32| Vec3::from(positions[i as usize].position);
                    let tree = if mesh.indices().len() / 3 < WORTH_A_TREE {
                        None
                    } else {
                        let version = world.mesh_version(mesh_id);
                        match trees.get_mut(&mesh_id) {
                            Some((seen, casts, tree)) if *seen == version => {
                                *casts += 1;
                                if tree.is_none() && *casts >= CASTS_BEFORE_A_TREE {
                                    *tree = Some(Bvh::new(mesh));
                                }
                                tree.as_ref()
                            }
                            _ => {
                                trees.insert(mesh_id, (version, 1, None));
                                None
                            }
                        }
                    };
                    let hit = match tree {
                        Some(tree) => tree.closest(mesh, at, sides, origin, direction, reach),
                        None => closest(mesh, parts, at, origin, direction, reach),
                    };
                    hit.map(|(distance, face)| (distance, world_normal(inverse, face, ray.direction)))
                }
            };
            if let Some((distance, normal)) = hit {
                best = Some(RayHit { object: id, point: ray.at(distance), normal, distance });
            }
        });
        best
    }
}

/// The ray in the space of `mesh` placed by `model`, when it reaches the
/// mesh's box no further than `reach`: the model's inverse, and the ray's
/// origin and unnormalised direction there.
fn into_object(mesh: &MeshData, model: Mat4, ray: Ray, reach: f32) -> Option<(Mat4, Vec3, Vec3)> {
    // The box around the object's box, in the world: far cheaper than the
    // inverse, which only objects the ray may hit need.
    let (lo, hi) = mesh.bounds();
    let centre = model.transform_point3((lo + hi) * 0.5);
    let half = Mat3::from_mat4(model).abs() * ((hi - lo) * 0.5);
    if !hits_box(ray.origin, ray.direction, centre - half, centre + half, reach) {
        return None;
    }
    let inverse = model.inverse();
    // A flattened object (a zero scale) has no inverse and covers no area.
    if model.determinant() == 0.0 || !inverse.is_finite() {
        return None;
    }
    Some((inverse, inverse.transform_point3(ray.origin), inverse.transform_vector3(ray.direction)))
}

/// As [`hit_still`] for a skinned mesh, its vertices bent by `joints`
/// (each joint's world matrix times its inverse bind) into `bent`.
fn hit_skinned(
    mesh: &MeshData,
    joints: &[Mat4],
    bent: &mut Vec<Vec3>,
    parts: impl Iterator<Item = (Range<u32>, bool)>,
    ray: Ray,
    reach: f32,
) -> Option<(f32, Vec3)> {
    // Each vertex is a weighted mean of itself moved by some of the
    // joints, so it's inside the box around the mesh's box moved by each.
    let (lo, hi) = mesh.bounds();
    let (middle, size) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
    let (mut min, mut max) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for joint in joints {
        let (centre, half) = (joint.transform_point3(middle), Mat3::from_mat4(*joint).abs() * size);
        (min, max) = (min.min(centre - half), max.max(centre + half));
    }
    if !hits_box(ray.origin, ray.direction, min, max, reach) {
        return None;
    }
    let skin = mesh.skin()?;
    bent.clear();
    bent.extend(mesh.vertices().iter().zip(skin).map(|(v, s)| {
        let p = Vec3::from(v.position);
        s.joints.iter().zip(&s.weights).fold(Vec3::ZERO, |at, (&j, &w)| match joints.get(usize::from(j)) {
            Some(m) => at + m.transform_point3(p) * (f32::from(w) / 65535.0),
            None => at,
        })
    }));
    let (distance, face) = closest(mesh, parts, |i| bent[i as usize], ray.origin, ray.direction, reach)?;
    let normal = face.normalize();
    Some((distance, if normal.dot(ray.direction) > 0.0 { -normal } else { normal }))
}

/// The closest of `mesh`'s triangles in `parts` along origin, direction,
/// their corners where `at` puts them: the distance and the unnormalised
/// face normal.
fn closest(
    mesh: &MeshData,
    parts: impl Iterator<Item = (Range<u32>, bool)>,
    at: impl Fn(u32) -> Vec3,
    origin: Vec3,
    direction: Vec3,
    reach: f32,
) -> Option<(f32, Vec3)> {
    let mut closest: Option<(f32, Vec3)> = None;
    for (range, double_sided) in parts {
        let Some(indices) = mesh.indices().get(range.start as usize..range.end as usize) else { continue };
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|i| at(triangle[i]));
            let limit = closest.map_or(reach, |(d, _)| d);
            if let Some(hit) = hit_triangle(origin, direction, a, b, c, double_sided, limit) {
                closest = Some(hit);
            }
        }
    }
    closest
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
pub(crate) fn hit_triangle(
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
