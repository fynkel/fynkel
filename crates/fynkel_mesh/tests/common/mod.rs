//! Checks the mesh tests share.

#![allow(dead_code, unreachable_pub)]

use std::collections::HashMap;

use fynkel_mesh::MeshData;
use glam::{Vec2, Vec3};

/// Every triangle has an area and winds counter-clockwise around its
/// vertices' normals; every normal is unit length.
pub fn check_winding(mesh: &MeshData) {
    let v = mesh.vertices();
    for tri in mesh.indices().chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| v[tri[k] as usize]);
        let face =
            (Vec3::from(b.position) - Vec3::from(a.position)).cross(Vec3::from(c.position) - Vec3::from(a.position));
        assert!(face.length() > 1e-7, "degenerate triangle {tri:?}");
        let normals = Vec3::from(a.normal) + Vec3::from(b.normal) + Vec3::from(c.normal);
        assert!(face.dot(normals) > 0.0, "triangle {tri:?} winds clockwise: {a:?} {b:?} {c:?}");
    }
    for v in v {
        assert!((Vec3::from(v.normal).length() - 1.0).abs() < 1e-5);
    }
}

/// Each corner's tangent is unit, at right angles to its normal, points
/// where u grows, and its bitangent points up the image.
pub fn check_tangents(mesh: &MeshData) {
    let v = mesh.vertices();
    for tri in mesh.indices().chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| v[tri[k] as usize]);
        let (e1, e2) =
            (Vec3::from(b.position) - Vec3::from(a.position), Vec3::from(c.position) - Vec3::from(a.position));
        let (d1, d2) = (Vec2::from(b.uv) - Vec2::from(a.uv), Vec2::from(c.uv) - Vec2::from(a.uv));
        let det = d1.x * d2.y - d2.x * d1.y;
        if det.abs() < 1e-9 || e1.cross(e2).length() < 1e-9 {
            continue;
        }
        let dp_du = (e1 * d2.y - e2 * d1.y) / det;
        let dp_dv = (e2 * d1.x - e1 * d2.x) / det;
        for corner in [a, b, c] {
            let n = Vec3::from(corner.normal);
            let t = Vec3::from_slice(&corner.tangent[..3]);
            let w = corner.tangent[3];
            assert!((t.length() - 1.0).abs() < 1e-4, "tangent {t} isn't unit");
            assert!(t.dot(n).abs() < 1e-3, "tangent {t} isn't at right angles to normal {n}");
            assert!(t.dot(dp_du) > 0.0, "tangent {t} doesn't point along u ({dp_du}) in {tri:?}");
            assert!((n.cross(t) * w).dot(dp_dv) < 0.0, "bitangent doesn't point up the image in {tri:?}");
        }
    }
}

/// Every edge, by where its ends are, is used by two triangles, once each
/// way: no gaps, nothing inside out.
pub fn check_closed(mesh: &MeshData) {
    let key = |i: u32| {
        let p = mesh.vertices()[i as usize].position;
        p.map(|c| (c * 1e4).round() as i64)
    };
    let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
    for tri in mesh.indices().chunks_exact(3) {
        for k in 0..3 {
            let (a, b) = (key(tri[k]), key(tri[(k + 1) % 3]));
            if a == b {
                continue;
            }
            *edges.entry((a, b)).or_default() += 1;
            *edges.entry((b, a)).or_default() -= 1;
        }
    }
    let open: Vec<_> = edges.iter().filter(|(_, n)| **n != 0).collect();
    assert!(open.is_empty(), "{} edges aren't shared, e.g. {:?}", open.len(), open.first());
}

/// The volume inside a closed mesh.
pub fn volume(mesh: &MeshData) -> f32 {
    let v = mesh.vertices();
    let p = |i: u32| Vec3::from(v[i as usize].position);
    mesh.indices().chunks_exact(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum()
}

pub fn close(a: f32, b: f32, within: f32) -> bool {
    (a - b).abs() <= within * b.abs().max(1.0)
}

pub fn bounds_are(mesh: &MeshData, min: Vec3, max: Vec3) {
    let (lo, hi) = mesh.bounds();
    assert!((lo - min).length() < 1e-4 && (hi - max).length() < 1e-4, "bounds {lo} {hi}, wanted {min} {max}");
}
