//! Flat outlines made solid (stage 13 §3.2): an outline with holes
//! extruded into a slab, its ends filled by ear clipping with each hole
//! joined to the outside, as fonts and maps do; and a profile turned
//! around Y, as on a lathe.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3};

use crate::ears;
use crate::shapes::Shape;
use crate::{MeshData, MeshError};

/// How sharp a corner of an outline or profile may be and still be shaded
/// smoothly across: a circle's corners are, a square's aren't.
const SMOOTH_DEGREES: f32 = 40.0;

/// Triangles that fill `outline` (counter-clockwise or not) less its
/// `holes`, three indices each into the points taken in order: the
/// outline's, then each hole's. Wound counter-clockwise as seen with X
/// right and Y up. Each hole must be inside the outline (its leftmost
/// point inside or on it); holes may touch it or each other at a point,
/// and the outline may touch itself, but neither may cross.
pub fn fill(outline: &[Vec2], holes: &[&[Vec2]]) -> Result<Vec<u32>, MeshError> {
    let mut rings = Vec::with_capacity(holes.len() + 1);
    let mut at = 0;
    for (k, ring) in std::iter::once(outline).chain(holes.iter().copied()).enumerate() {
        let name = || if k == 0 { "the outline".to_string() } else { format!("hole {k}") };
        if ring.len() < 3 {
            return Err(MeshError::Outline(format!("{} has {} points; it needs at least 3", name(), ring.len())));
        }
        if let Some(i) = ring.iter().position(|p| !p.is_finite()) {
            return Err(MeshError::NotFinite { what: "outline point", at: at + i });
        }
        if k > 0 {
            let leftmost = ring.iter().copied().min_by(|a, b| a.x.total_cmp(&b.x)).expect("at least three points");
            if !within(leftmost, outline) {
                return Err(MeshError::Outline(format!("{} isn't inside the outline", name())));
            }
        }
        rings.push(at..at + ring.len());
        at += ring.len();
    }
    let points: Vec<Vec2> = std::iter::once(outline).chain(holes.iter().copied()).flatten().copied().collect();
    Ok(ears::triangulate(&points, &rings))
}

/// Twice the signed area: more than 0 for counter-clockwise.
pub(crate) fn area(points: &[Vec2], ids: &[u32]) -> f32 {
    let mut sum = 0.0;
    for i in 0..ids.len() {
        let (a, b) = (points[ids[i] as usize], points[ids[(i + 1) % ids.len()] as usize]);
        sum += a.perp_dot(b);
    }
    sum
}

/// Whether `q` is inside the loop `ring` or on one of its edges.
fn within(q: Vec2, ring: &[Vec2]) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let (along, to) = (b - a, q - a);
        let on_line = along.perp_dot(to).abs() <= 1e-5 * along.length() * (1.0 + q.abs().max_element());
        if on_line && to.dot(along) >= 0.0 && to.dot(along) <= along.length_squared() {
            return true;
        }
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Each corner's normal on the edges either side of it: the two edges'
/// own for a sharp corner, one between them for a smooth one. Edge k runs
/// from point k to k + 1; `normal` gives an edge's outward normal.
pub(crate) fn edge_ends(edges: &[Vec2], closed: bool) -> Vec<(Vec2, Vec2)> {
    let mut ends = Vec::with_capacity(edges.len());
    edge_ends_into(edges, closed, &mut ends);
    ends
}

/// [`edge_ends`] into `ends`, in place of what it had.
pub(crate) fn edge_ends_into(edges: &[Vec2], closed: bool, ends: &mut Vec<(Vec2, Vec2)>) {
    let n = edges.len();
    let limit = SMOOTH_DEGREES.to_radians().cos();
    ends.clear();
    ends.extend((0..n).map(|k| {
        let own = edges[k];
        let start = if closed || k > 0 { Some(edges[(k + n - 1) % n]) } else { None };
        let end = if closed || k + 1 < n { Some(edges[(k + 1) % n]) } else { None };
        let blend = |other: Option<Vec2>| match other {
            Some(o) if o.dot(own) >= limit => (o + own).normalize_or(own),
            _ => own,
        };
        (blend(start), blend(end))
    }));
}

/// `tangent` made square to `normal`: at a smoothed corner the normal is
/// between the two edges', so the edge's own direction isn't quite.
pub(crate) fn square_to(tangent: Vec3, normal: Vec3) -> Vec3 {
    (tangent - normal * normal.dot(tangent)).normalize_or(tangent)
}

impl MeshData {
    /// `outline` less its `holes`, a slab `depth` thick along Z centred on
    /// the origin: its front faces +Z. The ends show the texture flat,
    /// `uv_per_metre` times a metre, upright from the front; the sides
    /// wrap it along the outline, from the front edge back. Corners
    /// sharper than 40° are sharp.
    pub fn extrude(outline: &[Vec2], holes: &[&[Vec2]], depth: f32, uv_per_metre: f32) -> Result<Self, MeshError> {
        let triangles = fill(outline, holes)?;
        let points: Vec<Vec2> = std::iter::once(outline).chain(holes.iter().copied()).flatten().copied().collect();
        let half = depth * 0.5;
        let k = uv_per_metre;
        let mut s = Shape::default();
        // The ends.
        for (z, normal, right) in [(half, Vec3::Z, Vec3::X), (-half, Vec3::NEG_Z, Vec3::NEG_X)] {
            let base = s.positions.len() as u32;
            for &p in &points {
                s.push(p.extend(z), normal, right, [p.x * right.x * k, -p.y * k]);
            }
            for tri in triangles.chunks_exact(3) {
                let [a, b, c] = [tri[0] + base, tri[1] + base, tri[2] + base];
                s.indices.extend(if z > 0.0 { [a, b, c] } else { [a, c, b] });
            }
        }
        // The sides, each loop walked with the outside on its right.
        for (index, ring) in std::iter::once(outline).chain(holes.iter().copied()).enumerate() {
            let mut ring: Vec<Vec2> = ring.to_vec();
            let ids: Vec<u32> = (0..ring.len() as u32).collect();
            if (area(&ring, &ids) > 0.0) != (index == 0) {
                ring.reverse();
            }
            let n = ring.len();
            let along: Vec<Vec2> = (0..n).map(|i| (ring[(i + 1) % n] - ring[i]).normalize_or_zero()).collect();
            let outward: Vec<Vec2> = along.iter().map(|d| Vec2::new(d.y, -d.x)).collect();
            let ends = edge_ends(&outward, true);
            let mut distance = 0.0;
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                let length = (b - a).length();
                if length == 0.0 {
                    continue;
                }
                let tangent = along[i].extend(0.0);
                let (na, nb) = (ends[i].0.extend(0.0), ends[i].1.extend(0.0));
                let (u0, u1) = (distance * k, (distance + length) * k);
                let base = s.positions.len() as u32;
                let (ta, tb) = (square_to(tangent, na), square_to(tangent, nb));
                s.push(a.extend(half), na, ta, [u0, 0.0]);
                s.push(b.extend(half), nb, tb, [u1, 0.0]);
                s.push(b.extend(-half), nb, tb, [u1, depth * k]);
                s.push(a.extend(-half), na, ta, [u0, depth * k]);
                // Front a, back a, back b; front a, back b, front b.
                s.indices.extend([base, base + 3, base + 2, base, base + 2, base + 1]);
                distance += length;
            }
        }
        Ok(s.build())
    }

    /// `profile` turned around Y with `sides` slices, as on a lathe: each
    /// point is (distance from Y, height). The outside is on the right of
    /// the way the profile goes, so a vase's goes from the middle of its
    /// base outwards, then up. The texture wraps around once from +Z, and
    /// runs from the profile's end (its top) to its start. Corners sharper
    /// than 40° are sharp.
    pub fn lathe(profile: &[Vec2], sides: u32) -> Result<Self, MeshError> {
        if profile.len() < 2 {
            return Err(MeshError::Outline(format!("a profile has {} points; it needs at least 2", profile.len())));
        }
        if let Some(at) = profile.iter().position(|p| !p.is_finite()) {
            return Err(MeshError::NotFinite { what: "profile point", at });
        }
        assert!(sides >= 3, "a lathe needs at least 3 sides");
        let edges: Vec<(Vec2, Vec2)> = profile.windows(2).map(|w| (w[0], w[1])).filter(|(a, b)| a != b).collect();
        if edges.is_empty() {
            return Err(MeshError::NoTriangles);
        }
        let outward: Vec<Vec2> = edges.iter().map(|(a, b)| (*b - *a).normalize().perp() * -1.0).collect();
        let closed = profile.first() == profile.last() && profile.len() > 3;
        let ends = edge_ends(&outward, closed);
        let total: f32 = edges.iter().map(|(a, b)| (*b - *a).length()).sum();
        let mut s = Shape::default();
        let row = sides + 1;
        let mut gone = 0.0;
        for (k, &(a, b)) in edges.iter().enumerate() {
            let length = (b - a).length();
            let base = s.positions.len() as u32;
            for (point, normal, v) in
                [(a, ends[k].0, 1.0 - gone / total), (b, ends[k].1, 1.0 - (gone + length) / total)]
            {
                for side in 0..=sides {
                    let u = side as f32 / sides as f32;
                    let phi = TAU * u;
                    let out = Vec3::new(phi.sin(), 0.0, phi.cos());
                    let n = out * normal.x + Vec3::Y * normal.y;
                    s.push(out * point.x + Vec3::Y * point.y, n, Vec3::new(phi.cos(), 0.0, -phi.sin()), [u, v]);
                }
            }
            for side in 0..sides {
                let (lo, hi) = (base + side, base + row + side);
                // Skip the triangle that closes to a line on the axis.
                if b.x > 0.0 {
                    s.indices.extend([hi, lo, hi + 1]);
                }
                if a.x > 0.0 {
                    s.indices.extend([hi + 1, lo, lo + 1]);
                }
            }
            gone += length;
        }
        if s.indices.is_empty() {
            return Err(MeshError::NoTriangles);
        }
        Ok(s.build())
    }
}
