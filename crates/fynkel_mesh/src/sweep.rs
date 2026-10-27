//! A flat outline swept along a path (stage 13 §3.3): a road, a rope, a
//! pipe, a rail. The path is a spline's samples, worked out in Dart: each
//! a point, the way the path goes there and which way is up. The outline
//! is drawn across the path at each one, X to the right and Y up, and its
//! edges are joined from each to the next, as an extrusion's sides; the
//! ends are filled as an extrusion's are.

use glam::{Vec2, Vec3};

use crate::outline::{area, edge_ends, fill, square_to};
use crate::shapes::Shape;
use crate::{MeshData, MeshError};

/// A point of a path to sweep along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathPoint {
    pub position: Vec3,
    /// The way the path goes: any length but 0.
    pub forward: Vec3,
    /// Which way is up across it; only its part at right angles to
    /// `forward` counts.
    pub up: Vec3,
    /// How far along the path, in metres: the texture's v.
    pub distance: f32,
}

/// What to sweep, and along what.
#[derive(Clone, Copy, Debug)]
pub struct Sweep<'a> {
    /// Two or more points. For a path that closes, the last is where the
    /// first is, with the same frame, and there are no caps.
    pub path: &'a [PathPoint],
    /// The outline across the path, X to the right and Y up seen going
    /// along it; either way round.
    pub outline: &'a [Vec2],
    /// Holes in it, each going the length of the path: a pipe's inside.
    pub holes: &'a [&'a [Vec2]],
    /// The ends filled, as an extrusion's.
    pub caps: bool,
    /// The texture repeats this many times a metre, around the outline (u)
    /// and along the path (v).
    pub uv_per_metre: f32,
}

impl MeshData {
    /// `sweep`'s outline swept along its path. The sides wrap the texture
    /// around the outline and along the path, in metres; the caps show it
    /// flat, upright seen from outside. Corners of the outline sharper than
    /// 40° are sharp, the rest smooth.
    pub fn sweep(sweep: &Sweep<'_>) -> Result<Self, MeshError> {
        let mut mesh = MeshData::empty();
        mesh.sweep_into(sweep)?;
        Ok(mesh)
    }

    /// [`MeshData::sweep`] into this mesh in place of what it had, keeping
    /// whether it's dynamic: a rope swept again each frame. When the
    /// triangles come out as they were (the same number of points, the
    /// same outline), they're kept, so a copy into the GPU leaves them.
    pub fn sweep_into(&mut self, sweep: &Sweep<'_>) -> Result<(), MeshError> {
        if !self.skin.is_empty() {
            return Err(MeshError::Skinned);
        }
        let swept = swept(sweep)?;
        let same = self.indices == swept.indices && self.submeshes == swept.submeshes;
        self.vertices = swept.vertices;
        if !same {
            self.indices = swept.indices;
            self.submeshes = swept.submeshes;
            self.indices_made = swept.indices_made;
        }
        self.bounds = swept.bounds;
        Ok(())
    }
}

/// Each point's frame: its position, then right and up, at right angles
/// to each other and to the way the path goes, which is `forward`. Where
/// the point's up is (nearly) along its forward, the right is carried over
/// from the point before (or the first point after, at the start), turned
/// as little as it can be: an outline doesn't flip where a path goes
/// straight up.
fn frames(path: &[PathPoint]) -> Result<Vec<(Vec3, Vec3, Vec3, Vec3)>, MeshError> {
    if path.len() < 2 {
        return Err(MeshError::Outline(format!("a path needs at least 2 points; this has {}", path.len())));
    }
    // Each point's forward, and its right where its up gives one.
    let mut frames = path
        .iter()
        .enumerate()
        .map(|(at, p)| {
            if !(p.position.is_finite() && p.forward.is_finite() && p.up.is_finite() && p.distance.is_finite()) {
                return Err(MeshError::NotFinite { what: "path point", at });
            }
            let forward = p
                .forward
                .try_normalize()
                .ok_or_else(|| MeshError::Outline(format!("path point {at} has no direction: its forward is 0")))?;
            let right = forward.cross(p.up);
            let right =
                if right.length_squared() > 1e-8 * p.up.length_squared() { right.normalize() } else { Vec3::NAN };
            Ok((p.position, right, Vec3::ZERO, forward))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // `from` at right angles to `forward`, turned as little as it can be.
    let carried = |from: Vec3, forward: Vec3| {
        (from - forward * from.dot(forward)).try_normalize().unwrap_or_else(|| forward.any_orthonormal_vector())
    };
    let good = |f: &(Vec3, Vec3, Vec3, Vec3)| !f.1.is_nan();
    let first = frames.iter().position(good).unwrap_or_else(|| {
        frames[0].1 = frames[0].3.any_orthonormal_vector();
        0
    });
    for at in (0..first).rev() {
        frames[at].1 = carried(frames[at + 1].1, frames[at].3);
    }
    for at in first + 1..frames.len() {
        if !good(&frames[at]) {
            frames[at].1 = carried(frames[at - 1].1, frames[at].3);
        }
    }
    for (_, right, up, forward) in &mut frames {
        *up = right.cross(*forward);
    }
    Ok(frames)
}

fn swept(sweep: &Sweep<'_>) -> Result<MeshData, MeshError> {
    let Sweep { path, outline, holes, caps, uv_per_metre: k } = *sweep;
    let frames = frames(path)?;
    let loops = || std::iter::once(outline).chain(holes.iter().copied());
    for ring in loops() {
        if ring.len() < 3 {
            return Err(MeshError::Outline(format!("an outline has {} points; it needs at least 3", ring.len())));
        }
        if let Some(at) = ring.iter().position(|p| !p.is_finite()) {
            return Err(MeshError::NotFinite { what: "outline point", at });
        }
    }
    let across = |(at, right, up, _): (Vec3, Vec3, Vec3, Vec3), p: Vec2| at + right * p.x + up * p.y;
    let mut s = Shape::default();
    // The sides, each loop walked with the outside on its right, as an
    // extrusion's: the path goes as the extrusion's from front to back.
    for (index, ring) in loops().enumerate() {
        let mut ring: Vec<Vec2> = ring.to_vec();
        let ids: Vec<u32> = (0..ring.len() as u32).collect();
        if (area(&ring, &ids) > 0.0) != (index == 0) {
            ring.reverse();
        }
        let n = ring.len();
        let along: Vec<Vec2> = (0..n).map(|i| (ring[(i + 1) % n] - ring[i]).normalize_or_zero()).collect();
        let outward: Vec<Vec2> = along.iter().map(|d| Vec2::new(d.y, -d.x)).collect();
        let ends = edge_ends(&outward, true);
        let mut around = 0.0;
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let length = (b - a).length();
            if length == 0.0 {
                continue;
            }
            let (u0, u1) = (around * k, (around + length) * k);
            let base = s.positions.len() as u32;
            for (frame, point) in frames.iter().zip(path) {
                let (_, right, up, _) = *frame;
                let turn = |v: Vec2| right * v.x + up * v.y;
                let tangent = turn(along[i]);
                let (na, nb) = (turn(ends[i].0), turn(ends[i].1));
                let v = point.distance * k;
                s.push(across(*frame, a), na, square_to(tangent, na), [u0, v]);
                s.push(across(*frame, b), nb, square_to(tangent, nb), [u1, v]);
            }
            for j in 0..frames.len() as u32 - 1 {
                // This point's a and b, then the next's.
                let (a0, b0, a1, b1) = (base + 2 * j, base + 2 * j + 1, base + 2 * j + 2, base + 2 * j + 3);
                s.indices.extend([a0, a1, b1, a0, b1, b0]);
            }
            around += length;
        }
    }
    if caps {
        let triangles = fill(outline, holes)?;
        let points: Vec<Vec2> = loops().flatten().copied().collect();
        let (first, last) = (frames[0], frames[frames.len() - 1]);
        // The start faces back along the path, as an extrusion's front;
        // the end forward, as its back.
        for (frame, start) in [(first, true), (last, false)] {
            let (_, right, _, forward) = frame;
            let (normal, image_right, mirror) = if start { (-forward, right, 1.0) } else { (forward, -right, -1.0) };
            let base = s.positions.len() as u32;
            for &p in &points {
                s.push(across(frame, p), normal, image_right, [p.x * mirror * k, -p.y * k]);
            }
            for tri in triangles.chunks_exact(3) {
                let [a, b, c] = [tri[0] + base, tri[1] + base, tri[2] + base];
                s.indices.extend(if start { [a, b, c] } else { [a, c, b] });
            }
        }
    }
    if s.indices.is_empty() {
        return Err(MeshError::NoTriangles);
    }
    Ok(s.build())
}
