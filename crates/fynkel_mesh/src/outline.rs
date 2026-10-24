//! Flat outlines made solid (stage 13 §3.2): an outline with holes
//! extruded into a slab, its ends filled by ear clipping with each hole
//! joined to the outside, as fonts and maps do; and a profile turned
//! around Y, as on a lathe.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3};

use crate::shapes::Shape;
use crate::{MeshData, MeshError};

/// How sharp a corner of an outline or profile may be and still be shaded
/// smoothly across: a circle's corners are, a square's aren't.
const SMOOTH_DEGREES: f32 = 40.0;

/// Triangles that fill `outline` (counter-clockwise or not) less its
/// `holes`, three indices each into the points taken in order: the
/// outline's, then each hole's. Wound counter-clockwise as seen with X
/// right and Y up.
pub fn fill(outline: &[Vec2], holes: &[&[Vec2]]) -> Result<Vec<u32>, MeshError> {
    let mut loops: Vec<Vec<u32>> = Vec::with_capacity(holes.len() + 1);
    let mut points: Vec<Vec2> = Vec::new();
    for (k, ring) in std::iter::once(outline).chain(holes.iter().copied()).enumerate() {
        if ring.len() < 3 {
            return Err(MeshError::Outline(format!(
                "{} has {} points; it needs at least 3",
                if k == 0 { "the outline".to_string() } else { format!("hole {k}") },
                ring.len()
            )));
        }
        if let Some(at) = ring.iter().position(|p| !p.is_finite()) {
            return Err(MeshError::NotFinite { what: "outline point", at: points.len() + at });
        }
        let start = points.len() as u32;
        points.extend_from_slice(ring);
        let mut ids: Vec<u32> = (start..start + ring.len() as u32).collect();
        // The outline counter-clockwise, the holes clockwise.
        if (area(&points, &ids) > 0.0) != (k == 0) {
            ids.reverse();
        }
        loops.push(ids);
    }
    let mut polygon = loops.remove(0);
    // Holes from the rightmost in, each joined to what's there by a cut to
    // a point it can see.
    loops.sort_by(|a, b| rightmost(&points, b).1.x.total_cmp(&rightmost(&points, a).1.x));
    for hole in loops {
        join(&points, &mut polygon, &hole);
    }
    Ok(clip_ears(&points, polygon))
}

/// Twice the signed area: more than 0 for counter-clockwise.
fn area(points: &[Vec2], ids: &[u32]) -> f32 {
    let mut sum = 0.0;
    for i in 0..ids.len() {
        let (a, b) = (points[ids[i] as usize], points[ids[(i + 1) % ids.len()] as usize]);
        sum += a.perp_dot(b);
    }
    sum
}

/// Where in `ids` its rightmost point is, and that point.
fn rightmost(points: &[Vec2], ids: &[u32]) -> (usize, Vec2) {
    let (at, &id) = ids
        .iter()
        .enumerate()
        .max_by(|a, b| points[*a.1 as usize].x.total_cmp(&points[*b.1 as usize].x))
        .expect("at least three points");
    (at, points[id as usize])
}

/// Joins `hole` into `polygon` by a cut from the hole's rightmost point to
/// a point of the polygon it can see to its right: the polygon then walks
/// to it, round the hole, and back.
fn join(points: &[Vec2], polygon: &mut Vec<u32>, hole: &[u32]) {
    let p = |id: u32| points[id as usize];
    let (m_at, m) = rightmost(points, hole);
    // The nearest edge a ray from m to +X crosses, and the point on it.
    let mut best: Option<(f32, usize)> = None;
    for i in 0..polygon.len() {
        let (a, b) = (p(polygon[i]), p(polygon[(i + 1) % polygon.len()]));
        if (a.y > m.y) == (b.y > m.y) {
            continue;
        }
        let x = a.x + (m.y - a.y) / (b.y - a.y) * (b.x - a.x);
        if x >= m.x && best.is_none_or(|(bx, _)| x < bx) {
            best = Some((x, i));
        }
    }
    let Some((x, i)) = best else {
        // Outside the polygon altogether: nothing to join it to.
        return;
    };
    let hit = Vec2::new(x, m.y);
    let (a_at, b_at) = (i, (i + 1) % polygon.len());
    let mut to = if p(polygon[a_at]).x > p(polygon[b_at]).x { a_at } else { b_at };
    // A corner of the polygon inside the triangle m, hit, to would block
    // the view: take the one at the smallest angle from the ray instead.
    let candidate = p(polygon[to]);
    let mut angle = f32::INFINITY;
    for (k, &id) in polygon.iter().enumerate() {
        let q = p(id);
        if k == to || q == candidate || !inside(q, m, hit, candidate) {
            continue;
        }
        let a = (q - m).angle_to(Vec2::X).abs();
        if a < angle || (a == angle && (q - m).length() < (p(polygon[to]) - m).length()) {
            angle = a;
            to = k;
        }
    }
    let mut walk = Vec::with_capacity(hole.len() + 2);
    for k in 0..=hole.len() {
        walk.push(hole[(m_at + k) % hole.len()]);
    }
    walk.push(polygon[to]);
    polygon.splice(to + 1..to + 1, walk);
}

/// Whether `q` is inside the triangle a, b, c (either winding), or on its
/// edges.
fn inside(q: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let (d1, d2, d3) = ((b - a).perp_dot(q - a), (c - b).perp_dot(q - b), (a - c).perp_dot(q - c));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// Cuts ears off a simple counter-clockwise `polygon` (which may visit a
/// point twice, where a hole was joined) until it's all triangles.
fn clip_ears(points: &[Vec2], mut polygon: Vec<u32>) -> Vec<u32> {
    let p = |id: u32| points[id as usize];
    let mut out = Vec::with_capacity((polygon.len().saturating_sub(2)) * 3);
    while polygon.len() > 3 {
        let n = polygon.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (polygon[(i + n - 1) % n], polygon[i], polygon[(i + 1) % n]);
            let (pa, pb, pc) = (p(a), p(b), p(c));
            if (pb - pa).perp_dot(pc - pb) <= 0.0 {
                return false; // not a convex corner
            }
            // No other point in it; a point visited twice where a hole was
            // joined is the same point as one of the corners, not inside.
            !polygon.iter().any(|&id| {
                let q = p(id);
                q != pa && q != pb && q != pc && inside(q, pa, pb, pc)
            })
        });
        // A polygon with no ear left is broken (it crosses itself): take
        // the most convex corner, so it still ends.
        let i = ear.unwrap_or_else(|| {
            (0..n)
                .max_by(|&x, &y| {
                    let turn = |i: usize| {
                        let (a, b, c) = (p(polygon[(i + n - 1) % n]), p(polygon[i]), p(polygon[(i + 1) % n]));
                        (b - a).perp_dot(c - b)
                    };
                    turn(x).total_cmp(&turn(y))
                })
                .expect("more than three")
        });
        let (a, b, c) = (polygon[(i + n - 1) % n], polygon[i], polygon[(i + 1) % n]);
        if (p(b) - p(a)).perp_dot(p(c) - p(b)) > 0.0 {
            out.extend([a, b, c]);
        }
        polygon.remove(i);
    }
    let (a, b, c) = (p(polygon[0]), p(polygon[1]), p(polygon[2]));
    if (b - a).perp_dot(c - b) > 0.0 {
        out.extend_from_slice(&polygon);
    }
    out
}

/// Each corner's normal on the edges either side of it: the two edges'
/// own for a sharp corner, one between them for a smooth one. Edge k runs
/// from point k to k + 1; `normal` gives an edge's outward normal.
fn edge_ends(edges: &[Vec2], closed: bool) -> Vec<(Vec2, Vec2)> {
    let n = edges.len();
    let limit = SMOOTH_DEGREES.to_radians().cos();
    (0..n)
        .map(|k| {
            let own = edges[k];
            let start = if closed || k > 0 { Some(edges[(k + n - 1) % n]) } else { None };
            let end = if closed || k + 1 < n { Some(edges[(k + 1) % n]) } else { None };
            let blend = |other: Option<Vec2>| match other {
                Some(o) if o.dot(own) >= limit => (o + own).normalize_or(own),
                _ => own,
            };
            (blend(start), blend(end))
        })
        .collect()
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
                s.push(a.extend(half), na, tangent, [u0, 0.0]);
                s.push(b.extend(half), nb, tangent, [u1, 0.0]);
                s.push(b.extend(-half), nb, tangent, [u1, depth * k]);
                s.push(a.extend(-half), na, tangent, [u0, depth * k]);
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
