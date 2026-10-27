//! A box tree over a mesh's triangles (stage 14's WO4), so a ray tests the
//! few triangles near it instead of every one: a 50,000-triangle ground
//! took a cast 0.2 ms, triangle by triangle.
//!
//! Built for a mesh's version, in its own space, once a few rays have
//! reached its box (a mesh changed every frame and cast at a few times is
//! cheaper without): each node's box holds its triangles, split in two at
//! the middle of the longest side of their centres' box, down to a few a
//! leaf.
//! A ray goes into the nearer child first and skips any box that starts
//! past the closest hit so far.

use glam::Vec3;

use crate::MeshData;
use crate::raycast::hit_triangle;

/// Triangles a leaf holds at most.
const LEAF: usize = 4;

/// Meshes with fewer triangles are tested one by one: a tree wouldn't pay.
pub(crate) const WORTH_A_TREE: usize = 64;

/// A triangle while the tree is made: its box, its centre, and its first
/// index and submesh.
#[derive(Clone, Copy)]
struct Item {
    lo: Vec3,
    hi: Vec3,
    centre: Vec3,
    triangle: (u32, u32),
}

#[derive(Clone, Copy, Debug)]
struct Node {
    lo: Vec3,
    hi: Vec3,
    /// A leaf's first triangle in [`Bvh::triangles`]; an inner node's
    /// second child (its first is the next node).
    start: u32,
    /// A leaf's triangle count; 0 for an inner node.
    count: u32,
}

/// The tree over one mesh's triangles.
#[derive(Debug, Default)]
pub(crate) struct Bvh {
    nodes: Vec<Node>,
    /// Each triangle's first index in the mesh's indices, and its submesh,
    /// in the leaves' order.
    triangles: Vec<(u32, u32)>,
}

impl Bvh {
    /// The tree over every triangle in `mesh`'s submeshes, its vertices at
    /// their positions.
    pub(crate) fn new(mesh: &MeshData) -> Self {
        let positions = mesh.vertices();
        let indices = mesh.indices();
        let corner = |i: usize| Vec3::from(positions[indices[i] as usize].position);
        // Each triangle with its box and centre, sorted in place as the
        // tree is made.
        let mut items: Vec<Item> = Vec::new();
        for (k, range) in mesh.submeshes().iter().enumerate() {
            let end = (range.end as usize).min(indices.len());
            let mut first = range.start as usize;
            while first + 3 <= end {
                let [a, b, c] = [0, 1, 2].map(|i| corner(first + i));
                let (lo, hi) = (a.min(b).min(c), a.max(b).max(c));
                items.push(Item { lo, hi, centre: (lo + hi) * 0.5, triangle: (first as u32, k as u32) });
                first += 3;
            }
        }
        let mut bvh = Self { nodes: Vec::with_capacity(2 * items.len() / LEAF + 1), triangles: Vec::new() };
        if !items.is_empty() {
            let count = items.len();
            bvh.build(&mut items, 0, count);
        }
        bvh.triangles = items.iter().map(|item| item.triangle).collect();
        bvh
    }

    /// Makes the node for `items[from..to]`, and those below it.
    fn build(&mut self, items: &mut [Item], from: usize, to: usize) {
        let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        let (mut centres_lo, mut centres_hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        for item in &items[from..to] {
            (lo, hi) = (lo.min(item.lo), hi.max(item.hi));
            (centres_lo, centres_hi) = (centres_lo.min(item.centre), centres_hi.max(item.centre));
        }
        let at = self.nodes.len();
        self.nodes.push(Node { lo, hi, start: from as u32, count: (to - from) as u32 });
        let size = centres_hi - centres_lo;
        // All the centres at one point can't be split: a leaf, however many.
        if to - from <= LEAF || size.max_element() <= 0.0 {
            return;
        }
        let axis = if size.x >= size.y && size.x >= size.z {
            0
        } else if size.y >= size.z {
            1
        } else {
            2
        };
        // The halves either side of the middle of the centres on that axis,
        // in one pass. The centres spread over it, neither half is empty.
        let split = (centres_lo[axis] + centres_hi[axis]) * 0.5;
        let (mut left, mut right) = (from, to);
        while left < right {
            if items[left].centre[axis] < split {
                left += 1;
            } else {
                right -= 1;
                items.swap(left, right);
            }
        }
        // All on one side by rounding: the middle by count instead.
        let middle = if left == from || left == to { from + (to - from) / 2 } else { left };
        self.build(items, from, middle);
        let second = self.nodes.len() as u32;
        self.nodes[at].count = 0;
        self.nodes[at].start = second;
        self.build(items, middle, to);
    }

    /// The closest triangle along origin, direction no further than
    /// `reach`, the corners where `at` puts them: its distance and its
    /// unnormalised face normal. `sides` says for each submesh whether it's
    /// drawn (and so hit) and double-sided.
    pub(crate) fn closest(
        &self,
        mesh: &MeshData,
        at: impl Fn(u32) -> Vec3,
        sides: &[Option<bool>],
        origin: Vec3,
        direction: Vec3,
        reach: f32,
    ) -> Option<(f32, Vec3)> {
        if self.nodes.is_empty() {
            return None;
        }
        let indices = mesh.indices();
        let inverse = direction.recip();
        let mut closest: Option<(f32, Vec3)> = None;
        // Nodes to visit, with the distance their boxes start at. A tree
        // over 2^32 triangles is under 64 deep.
        let mut stack = [(0_u32, 0.0_f32); 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let (index, enter) = stack[top];
            let limit = closest.map_or(reach, |(d, _)| d);
            if enter >= limit {
                continue;
            }
            let node = self.nodes[index as usize];
            if node.count > 0 {
                let leaf = &self.triangles[node.start as usize..(node.start + node.count) as usize];
                for &(first, submesh) in leaf {
                    let Some(Some(double_sided)) = sides.get(submesh as usize) else { continue };
                    let i = first as usize;
                    let [a, b, c] = [0, 1, 2].map(|k| at(indices[i + k]));
                    let limit = closest.map_or(reach, |(d, _)| d);
                    if let Some(hit) = hit_triangle(origin, direction, a, b, c, *double_sided, limit) {
                        closest = Some(hit);
                    }
                }
                continue;
            }
            let (first, second) = (index + 1, node.start);
            let near = |n: u32| {
                let n = self.nodes[n as usize];
                enters(origin, inverse, n.lo, n.hi, limit).map(|t| (n, t))
            };
            match (near(first), near(second)) {
                (Some((_, a)), Some((_, b))) => {
                    // The farther pushed first, so the nearer is taken next.
                    let (far, near) = if a <= b { ((second, b), (first, a)) } else { ((first, a), (second, b)) };
                    stack[top] = far;
                    stack[top + 1] = near;
                    top += 2;
                }
                (Some((_, a)), None) => {
                    stack[top] = (first, a);
                    top += 1;
                }
                (None, Some((_, b))) => {
                    stack[top] = (second, b);
                    top += 1;
                }
                (None, None) => {}
            }
        }
        closest
    }
}

/// Where the ray enters the box, if before `reach` (the slab test, with
/// the direction's reciprocal). A direction component of 0 gives
/// infinities, which compare the right way.
fn enters(origin: Vec3, inverse: Vec3, lo: Vec3, hi: Vec3, reach: f32) -> Option<f32> {
    let a = (lo - origin) * inverse;
    let b = (hi - origin) * inverse;
    let near = a.min(b).max_element().max(0.0);
    let far = a.max(b).min_element().min(reach);
    (near <= far).then_some(near)
}
