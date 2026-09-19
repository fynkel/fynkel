//! Tangent generation at cook time (blueprint §19.2d item 4): a mesh whose
//! source had no `TANGENT` attribute (the glTF importer writes zeros)
//! gets per-vertex tangents derived from its positions and `uv0`, so the
//! renderer can normal-map it. The classic per-triangle accumulation
//! (Lengyel): each triangle's tangent/bitangent in texture space is added
//! to its vertices, then Gram-Schmidt-orthogonalised against the normal;
//! `w` carries the handedness (`-1` for mirrored UVs). Deterministic:
//! a pure function of the vertex and index arrays.

use fynkel_cooked::payload::Vertex;
use glam::Vec3;

/// What [`ensure`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tangents {
    /// The source carried tangents; nothing was changed.
    Authored,
    /// Tangents were generated from positions and `uv0`.
    Generated,
    /// No tangents and no usable UVs: left at zero (the renderer falls
    /// back to the vertex normal).
    None,
}

impl Tangents {
    pub const fn name(self) -> &'static str {
        match self {
            Tangents::Authored => "authored",
            Tangents::Generated => "generated",
            Tangents::None => "none",
        }
    }
}

/// Generates tangents in place when every vertex has a zero tangent.
pub fn ensure(vertices: &mut [Vertex], indices: &[u32]) -> Tangents {
    if vertices.is_empty() {
        return Tangents::None;
    }
    let all_zero = vertices
        .iter()
        .all(|v| v.tangent[..3].iter().all(|c| *c == 0.0));
    if !all_zero {
        return Tangents::Authored;
    }
    let has_uvs = vertices.iter().any(|v| v.uv0 != [0.0, 0.0]);
    if !has_uvs {
        return Tangents::None;
    }
    let n = vertices.len();
    let mut tan = vec![Vec3::ZERO; n];
    let mut bitan = vec![Vec3::ZERO; n];
    for tri in indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if i0 >= n || i1 >= n || i2 >= n {
            continue;
        }
        let p0 = Vec3::from(vertices[i0].position);
        let p1 = Vec3::from(vertices[i1].position);
        let p2 = Vec3::from(vertices[i2].position);
        let [u0, v0] = vertices[i0].uv0;
        let [u1, v1] = vertices[i1].uv0;
        let [u2, v2] = vertices[i2].uv0;
        let e1 = p1 - p0;
        let e2 = p2 - p0;
        let (du1, dv1) = (u1 - u0, v1 - v0);
        let (du2, dv2) = (u2 - u0, v2 - v0);
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-12 || !det.is_finite() {
            continue;
        }
        let r = 1.0 / det;
        let t = (e1 * dv2 - e2 * dv1) * r;
        let b = (e2 * du1 - e1 * du2) * r;
        for i in [i0, i1, i2] {
            tan[i] += t;
            bitan[i] += b;
        }
    }
    for (i, v) in vertices.iter_mut().enumerate() {
        let normal = Vec3::from(v.normal).normalize_or(Vec3::Y);
        let t = tan[i];
        // Gram-Schmidt against the normal; a degenerate accumulation gets
        // an arbitrary perpendicular so the frame is still orthonormal.
        let mut t = (t - normal * normal.dot(t)).normalize_or(Vec3::ZERO);
        if t == Vec3::ZERO {
            t = normal.any_orthonormal_vector();
        }
        let w = if normal.cross(t).dot(bitan[i]) < 0.0 {
            -1.0
        } else {
            1.0
        };
        v.tangent = [t.x, t.y, t.z, w];
    }
    Tangents::Generated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> (Vec<Vertex>, Vec<u32>) {
        // A unit quad in the XY plane facing +Z, u along +X, v along -Y
        // (image rows go down) — the tangent must come out as +X.
        let mk = |x: f32, y: f32, u: f32, v: f32| Vertex {
            position: [x, y, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv0: [u, v],
            ..Vertex::default()
        };
        (
            vec![
                mk(0.0, 0.0, 0.0, 1.0),
                mk(1.0, 0.0, 1.0, 1.0),
                mk(1.0, 1.0, 1.0, 0.0),
                mk(0.0, 1.0, 0.0, 0.0),
            ],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    #[test]
    fn generates_an_orthonormal_frame_with_handedness() {
        let (mut v, i) = quad();
        assert_eq!(ensure(&mut v, &i), Tangents::Generated);
        for vert in &v {
            let t = Vec3::from([vert.tangent[0], vert.tangent[1], vert.tangent[2]]);
            assert!((t - Vec3::X).length() < 1e-5, "{t:?}");
            assert!((t.length() - 1.0).abs() < 1e-5);
            assert!(t.dot(Vec3::from(vert.normal)).abs() < 1e-5);
            // n × t = +Y but the bitangent (dv) points -Y: mirrored.
            assert_eq!(vert.tangent[3], -1.0);
        }
        // Idempotent: authored tangents are left alone.
        let before = v.clone();
        assert_eq!(ensure(&mut v, &i), Tangents::Authored);
        assert_eq!(v, before);
    }

    #[test]
    fn no_uvs_means_no_tangents() {
        let (mut v, i) = quad();
        for vert in &mut v {
            vert.uv0 = [0.0, 0.0];
        }
        assert_eq!(ensure(&mut v, &i), Tangents::None);
        assert!(v.iter().all(|v| v.tangent == [0.0; 4]));
        assert_eq!(ensure(&mut [], &[]), Tangents::None);
    }

    #[test]
    fn degenerate_triangles_are_skipped_but_vertices_still_get_a_frame() {
        let (mut v, _) = quad();
        // All three vertices share a UV: zero texture-space area.
        for vert in &mut v[..3] {
            vert.uv0 = [0.5, 0.5];
        }
        assert_eq!(ensure(&mut v, &[0, 1, 2]), Tangents::Generated);
        for vert in &v {
            let t = Vec3::from([vert.tangent[0], vert.tangent[1], vert.tangent[2]]);
            assert!((t.length() - 1.0).abs() < 1e-5);
            assert!(t.dot(Vec3::from(vert.normal)).abs() < 1e-5);
        }
    }
}
