//! A flat outline swept along a path (stage 13 §3.3): a road, a rope, a
//! pipe, a rail. The path is a spline's samples, worked out in Dart: each
//! a point, the way the path goes there and which way is up. The outline
//! is drawn across the path at each one, X to the right and Y up, and its
//! edges are joined from each to the next, as an extrusion's sides; the
//! ends are filled as an extrusion's are.

use std::cell::RefCell;

use glam::{Vec2, Vec3};

use crate::outline::{edge_ends_into, fill, square_to};
use crate::{Made, MeshData, MeshError, Vertex, WHITE};

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
    /// Once a sweep of its size has been made on the thread, it allocates
    /// nothing (stage 14's WO8).
    pub fn sweep_into(&mut self, sweep: &Sweep<'_>) -> Result<(), MeshError> {
        if !self.skin.is_empty() {
            return Err(MeshError::Skinned);
        }
        SCRATCH.with_borrow_mut(|scratch| {
            let bounds = swept(sweep, scratch)?;
            // The old vertices become the next sweep's room.
            std::mem::swap(&mut self.vertices, &mut scratch.vertices);
            let whole = 0..scratch.indices.len() as u32;
            if self.indices != scratch.indices || self.submeshes[..] != [whole.clone()] {
                std::mem::swap(&mut self.indices, &mut scratch.indices);
                self.submeshes.clear();
                self.submeshes.push(whole);
                self.indices_made = Made::next();
            }
            self.bounds = bounds;
            Ok(())
        })
    }
}

/// A point's frame: its position, right, up and forward.
type Frame = (Vec3, Vec3, Vec3, Vec3);

/// What sweeping works in, kept on each thread between sweeps so one again
/// allocates nothing.
#[derive(Default)]
struct Scratch {
    frames: Vec<Frame>,
    /// One loop of the outline at a time, the outside on its right, its
    /// edges' directions, their outward normals, and the normals at each
    /// edge's ends.
    ring: Vec<Vec2>,
    along: Vec<Vec2>,
    outward: Vec<Vec2>,
    ends: Vec<(Vec2, Vec2)>,
    /// What's swept: the mesh's next vertices and triangles.
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    /// The outline and holes (one after another, and each one's length)
    /// the caps were last filled for, and their triangles: filling is slow,
    /// and a rope keeps its outline.
    filled: Vec<Vec2>,
    filled_lengths: Vec<usize>,
    triangles: Vec<u32>,
}

thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::default();
}

/// Each point's frame: its position, then right and up, at right angles
/// to each other and to the way the path goes, which is `forward`. Where
/// the point's up is (nearly) along its forward, the right is carried over
/// from the point before (or the first point after, at the start), turned
/// as little as it can be: an outline doesn't flip where a path goes
/// straight up.
fn frames(path: &[PathPoint], frames: &mut Vec<Frame>) -> Result<(), MeshError> {
    if path.len() < 2 {
        return Err(MeshError::Outline(format!("a path needs at least 2 points; this has {}", path.len())));
    }
    // Each point's forward, and its right where its up gives one.
    frames.clear();
    for (at, p) in path.iter().enumerate() {
        if !(p.position.is_finite() && p.forward.is_finite() && p.up.is_finite() && p.distance.is_finite()) {
            return Err(MeshError::NotFinite { what: "path point", at });
        }
        let forward = p
            .forward
            .try_normalize()
            .ok_or_else(|| MeshError::Outline(format!("path point {at} has no direction: its forward is 0")))?;
        let right = forward.cross(p.up);
        let right = if right.length_squared() > 1e-8 * p.up.length_squared() { right.normalize() } else { Vec3::NAN };
        frames.push((p.position, right, Vec3::ZERO, forward));
    }
    // `from` at right angles to `forward`, turned as little as it can be.
    let carried = |from: Vec3, forward: Vec3| {
        (from - forward * from.dot(forward)).try_normalize().unwrap_or_else(|| forward.any_orthonormal_vector())
    };
    let good = |f: &Frame| !f.1.is_nan();
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
    for (_, right, up, forward) in frames.iter_mut() {
        *up = right.cross(*forward);
    }
    Ok(())
}

/// Sweeps into `scratch.vertices` and `scratch.indices`, as
/// [`MeshData::new`] would make them from the same corners: the box around
/// them.
fn swept(sweep: &Sweep<'_>, scratch: &mut Scratch) -> Result<(Vec3, Vec3), MeshError> {
    let Sweep { path, outline, holes, caps, uv_per_metre: k } = *sweep;
    let Scratch { frames, ring, along, outward, ends, vertices, indices, filled, filled_lengths, triangles } = scratch;
    self::frames(path, frames)?;
    let loops = || std::iter::once(outline).chain(holes.iter().copied());
    for ring in loops() {
        if ring.len() < 3 {
            return Err(MeshError::Outline(format!("an outline has {} points; it needs at least 3", ring.len())));
        }
        if let Some(at) = ring.iter().position(|p| !p.is_finite()) {
            return Err(MeshError::NotFinite { what: "outline point", at });
        }
    }
    if caps {
        let same = filled_lengths.iter().copied().eq(loops().map(<[_]>::len)) && filled.iter().eq(loops().flatten());
        if !same {
            *triangles = fill(outline, holes)?;
            filled.clear();
            filled.extend(loops().flatten());
            filled_lengths.clear();
            filled_lengths.extend(loops().map(<[_]>::len));
        }
    }
    vertices.clear();
    indices.clear();
    let across = |(at, right, up, _): Frame, p: Vec2| at + right * p.x + up * p.y;
    // The sides, each loop walked with the outside on its right, as an
    // extrusion's: the path goes as the extrusion's from front to back.
    for (index, points) in loops().enumerate() {
        ring.clear();
        ring.extend_from_slice(points);
        // Twice its area: more than 0 for counter-clockwise.
        let area: f32 = (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum();
        if (area > 0.0) != (index == 0) {
            ring.reverse();
        }
        let n = ring.len();
        along.clear();
        along.extend((0..n).map(|i| (ring[(i + 1) % n] - ring[i]).normalize_or_zero()));
        outward.clear();
        outward.extend(along.iter().map(|d| Vec2::new(d.y, -d.x)));
        edge_ends_into(outward, true, ends);
        let mut around = 0.0;
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let length = (b - a).length();
            if length == 0.0 {
                continue;
            }
            let (u0, u1) = (around * k, (around + length) * k);
            let base = vertices.len() as u32;
            for (&frame, point) in frames.iter().zip(path) {
                let (_, right, up, _) = frame;
                let turn = |v: Vec2| right * v.x + up * v.y;
                let tangent = turn(along[i]);
                let (na, nb) = (turn(ends[i].0), turn(ends[i].1));
                let v = point.distance * k;
                corner(vertices, across(frame, a), na, square_to(tangent, na), [u0, v])?;
                corner(vertices, across(frame, b), nb, square_to(tangent, nb), [u1, v])?;
            }
            for j in 0..frames.len() as u32 - 1 {
                // This point's a and b, then the next's.
                let (a0, b0, a1, b1) = (base + 2 * j, base + 2 * j + 1, base + 2 * j + 2, base + 2 * j + 3);
                indices.extend([a0, a1, b1, a0, b1, b0]);
            }
            around += length;
        }
    }
    if caps {
        let (first, last) = (frames[0], frames[frames.len() - 1]);
        // The start faces back along the path, as an extrusion's front;
        // the end forward, as its back.
        for (frame, start) in [(first, true), (last, false)] {
            let (_, right, _, forward) = frame;
            let (normal, image_right, mirror) = if start { (-forward, right, 1.0) } else { (forward, -right, -1.0) };
            let base = vertices.len() as u32;
            for &p in loops().flatten() {
                corner(vertices, across(frame, p), normal, image_right, [p.x * mirror * k, -p.y * k])?;
            }
            for tri in triangles.chunks_exact(3) {
                let [a, b, c] = [tri[0] + base, tri[1] + base, tri[2] + base];
                indices.extend(if start { [a, b, c] } else { [a, c, b] });
            }
        }
    }
    if indices.is_empty() {
        return Err(MeshError::NoTriangles);
    }
    let (lo, hi) = vertices.iter().fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(Vec3::from(v.position)), hi.max(Vec3::from(v.position)))
    });
    Ok((lo, hi))
}

/// A corner pushed onto `vertices` as [`MeshData::new`] takes it: normal
/// and tangent made unit length, the position finite.
fn corner(
    vertices: &mut Vec<Vertex>,
    position: Vec3,
    normal: Vec3,
    tangent: Vec3,
    uv: [f32; 2],
) -> Result<(), MeshError> {
    let at = vertices.len();
    if !position.is_finite() {
        return Err(MeshError::NotFinite { what: "position", at });
    }
    let normal = normal.try_normalize().ok_or(MeshError::ZeroLength { what: "normal", at })?;
    let tangent = tangent.try_normalize().ok_or(MeshError::ZeroLength { what: "tangent", at })?;
    vertices.push(Vertex {
        position: position.into(),
        normal: normal.into(),
        tangent: tangent.extend(1.0).into(),
        uv,
        color: WHITE,
    });
    Ok(())
}
