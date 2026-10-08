//! What a mesh promises (stage 2 plan §3.2): refused input gets a message
//! that says what's wrong; computed normals are smooth across shared
//! vertices and don't depend on how a face was cut into triangles; every
//! tangent points along u with a bitangent pointing up the image, also
//! where a texture is mirrored; and the built-in shapes keep stage 1's
//! geometry.

use fynkel_mesh::*;
use glam::{Vec2, Vec3};

fn parts<'a>(positions: &'a [[f32; 3]], indices: &'a [u32]) -> MeshParts<'a> {
    MeshParts { positions, indices, ..MeshParts::default() }
}

fn error(parts: MeshParts<'_>) -> String {
    MeshData::new(parts).unwrap_err().to_string()
}

const TRIANGLE: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

// --- Checks ------------------------------------------------------------------

#[test]
fn a_vertex_is_48_bytes() {
    assert_eq!(std::mem::size_of::<Vertex>(), 48);
}

#[test]
fn broken_meshes_are_refused_with_a_message_that_says_why() {
    assert_eq!(error(parts(&TRIANGLE, &[])), "a mesh needs at least one triangle");
    assert_eq!(error(parts(&TRIANGLE, &[0, 1])), "indices come in threes, one per triangle corner, but there are 2");
    assert_eq!(error(parts(&TRIANGLE, &[0, 1, 3])), "index 2 is 3, but there are only 3 vertices");
    let nan = [[0.0, 0.0, 0.0], [1.0, f32::NAN, 0.0], [0.0, 1.0, 0.0]];
    assert_eq!(error(parts(&nan, &[0, 1, 2])), "position 1 isn't finite");

    let two = [[0.0, 0.0, 1.0]; 2];
    let short = MeshParts { normals: Some(&two), ..parts(&TRIANGLE, &[0, 1, 2]) };
    assert_eq!(error(short), "normals has 2 items, but there are 3 positions");
    let zero = [[0.0, 0.0, 1.0], [0.0; 3], [0.0, 0.0, 1.0]];
    let flat = MeshParts { normals: Some(&zero), ..parts(&TRIANGLE, &[0, 1, 2]) };
    assert_eq!(error(flat), "normal 1 has no direction (zero length)");
    let uvs = [[0.0, 0.0], [f32::INFINITY, 0.0], [0.0, 0.0]];
    assert_eq!(error(MeshParts { uvs: Some(&uvs), ..parts(&TRIANGLE, &[0, 1, 2]) }), "uv 1 isn't finite");
    let tangents = [[1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.5]];
    let sign = MeshParts { tangents: Some(&tangents), ..parts(&TRIANGLE, &[0, 1, 2]) };
    assert_eq!(error(sign), "tangent 2 has w = 0.5; it must be 1 or -1");
}

#[test]
fn given_normals_and_tangents_are_kept_at_unit_length() {
    let normals = [[0.0, 0.0, 2.0]; 3];
    let tangents = [[3.0, 0.0, 0.0, -1.0]; 3];
    let mesh =
        MeshData::new(MeshParts { normals: Some(&normals), tangents: Some(&tangents), ..parts(&TRIANGLE, &[0, 1, 2]) })
            .unwrap();
    assert_eq!(mesh.vertices().len(), 3);
    for v in mesh.vertices() {
        assert_eq!(v.normal, [0.0, 0.0, 1.0]);
        assert_eq!(v.tangent, [1.0, 0.0, 0.0, -1.0]);
        assert_eq!(v.uv, [0.0, 0.0], "no UVs given: zero");
    }
}

// --- Normals -----------------------------------------------------------------

/// A cube of 8 shared corners. `diagonal` picks which way each face is cut
/// into two triangles.
fn shared_cube(diagonal: bool) -> MeshData {
    let positions: Vec<[f32; 3]> =
        (0..8).map(|i| [(i & 1) as f32 - 0.5, ((i >> 1) & 1) as f32 - 0.5, ((i >> 2) & 1) as f32 - 0.5]).collect();
    // Each face's corners, counter-clockwise from outside.
    let faces = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut indices = vec![];
    for [a, b, c, d] in faces {
        if diagonal {
            indices.extend([a, b, c, a, c, d]);
        } else {
            indices.extend([a, b, d, b, c, d]);
        }
    }
    MeshData::new(parts(&positions, &indices)).unwrap()
}

#[test]
fn shared_corners_get_the_same_smooth_normal_however_the_faces_are_cut() {
    for diagonal in [true, false] {
        let mesh = shared_cube(diagonal);
        assert_eq!(mesh.vertices().len(), 8);
        for v in mesh.vertices() {
            // Straight out from the centre: each corner touches three faces
            // at 90°, whichever triangles they're made of.
            let expected = Vec3::from(v.position).normalize();
            assert!(Vec3::from(v.normal).abs_diff_eq(expected, 1e-5), "{:?} at {:?}", v.normal, v.position);
        }
    }
}

#[test]
fn triangles_that_share_no_vertices_are_flat() {
    let positions =
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, -1.0]];
    let mesh = MeshData::new(parts(&positions, &[0, 1, 2, 3, 4, 5])).unwrap();
    let normals: Vec<[f32; 3]> = mesh.vertices().iter().map(|v| v.normal).collect();
    assert_eq!(&normals[..3], [[0.0, 1.0, 0.0]; 3]);
    assert_eq!(&normals[3..], [[1.0, 0.0, 0.0]; 3]);
}

// --- Tangents ----------------------------------------------------------------

/// For every triangle: each corner's tangent is a unit vector at right
/// angles to its normal, points where u grows on that triangle, and its
/// bitangent points where v shrinks (up the image).
fn check_tangents(mesh: &MeshData) {
    let v = mesh.vertices();
    for tri in mesh.indices().chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| v[tri[k] as usize]);
        let (e1, e2) =
            (Vec3::from(b.position) - Vec3::from(a.position), Vec3::from(c.position) - Vec3::from(a.position));
        let (d1, d2) = (Vec2::from(b.uv) - Vec2::from(a.uv), Vec2::from(c.uv) - Vec2::from(a.uv));
        let det = d1.x * d2.y - d2.x * d1.y;
        if det.abs() < 1e-9 || e1.cross(e2).length() < 1e-9 {
            continue; // no UV direction on this triangle (a pole)
        }
        // Where u and v grow on the triangle, in 3D.
        let dp_du = (e1 * d2.y - e2 * d1.y) / det;
        let dp_dv = (e2 * d1.x - e1 * d2.x) / det;
        for corner in [a, b, c] {
            let n = Vec3::from(corner.normal);
            let t = Vec3::from_slice(&corner.tangent[..3]);
            let w = corner.tangent[3];
            assert!((t.length() - 1.0).abs() < 1e-4, "tangent {t} isn't unit");
            assert!(t.dot(n).abs() < 1e-3, "tangent {t} isn't at right angles to normal {n}");
            assert!(w == 1.0 || w == -1.0, "w = {w}");
            assert!(t.dot(dp_du) > 0.0, "tangent {t} doesn't point along u ({dp_du}) in {tri:?}");
            let bitangent = n.cross(t) * w;
            assert!(bitangent.dot(dp_dv) < 0.0, "bitangent {bitangent} doesn't point up the image in {tri:?}");
        }
    }
}

#[test]
fn a_texture_shown_the_right_way_round_has_tangents_with_w_plus_one() {
    let mesh = MeshData::plane();
    check_tangents(&mesh);
    for v in mesh.vertices() {
        assert_eq!(v.tangent, [1.0, 0.0, 0.0, 1.0], "image-right is +X on the plane");
    }
}

/// Two squares side by side facing +Z, sharing their middle edge. The right
/// one shows the texture mirrored, so the middle vertices need a tangent
/// pointing right for one square and left for the other.
#[test]
fn a_mirrored_texture_splits_the_vertices_on_the_mirror_line() {
    //  0 --- 1 --- 2      u: 0 → 1 ← 0
    //  |     |     |
    //  3 --- 4 --- 5
    let positions =
        [[-1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = [3, 4, 1, 3, 1, 0, 4, 5, 2, 4, 2, 1];
    let mesh = MeshData::new(MeshParts { uvs: Some(&uvs), ..parts(&positions, &indices) }).unwrap();
    check_tangents(&mesh);
    assert_eq!(mesh.vertices().len(), 8, "vertices 1 and 4 split in two");
    let on_line = mesh.vertices().iter().filter(|v| v.position[0] == 0.0);
    let mut signs: Vec<f32> = on_line.map(|v| v.tangent[0]).collect();
    signs.sort_by(f32::total_cmp);
    assert_eq!(signs, [-1.0, -1.0, 1.0, 1.0], "pointing right on the left square, left on the right one");
}

#[test]
fn without_uvs_any_tangent_at_right_angles_to_the_normal_is_used() {
    let mesh = shared_cube(true);
    for v in mesh.vertices() {
        let t = Vec3::from_slice(&v.tangent[..3]);
        assert!((t.length() - 1.0).abs() < 1e-5 && t.dot(Vec3::from(v.normal)).abs() < 1e-5);
        assert_eq!(v.tangent[3], 1.0);
    }
}

// --- Built-in shapes ---------------------------------------------------------

/// Every triangle winds counter-clockwise seen from where its normals point,
/// and every normal is unit length.
fn check_winding(mesh: &MeshData) {
    let v = mesh.vertices();
    for tri in mesh.indices().chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| v[tri[k] as usize]);
        let face =
            (Vec3::from(b.position) - Vec3::from(a.position)).cross(Vec3::from(c.position) - Vec3::from(a.position));
        assert!(face.length() > 1e-7, "degenerate triangle {tri:?}");
        let normals = Vec3::from(a.normal) + Vec3::from(b.normal) + Vec3::from(c.normal);
        assert!(face.dot(normals) > 0.0, "triangle {tri:?} winds clockwise");
    }
    for v in v {
        assert!((Vec3::from(v.normal).length() - 1.0).abs() < 1e-5);
    }
}

#[test]
fn the_cube_has_flat_faces_wound_outwards_each_showing_the_whole_texture() {
    let cube = MeshData::cube();
    assert_eq!((cube.vertices().len(), cube.indices().len()), (24, 36), "no splits");
    check_winding(&cube);
    check_tangents(&cube);
    for v in cube.vertices() {
        assert!(v.position.iter().all(|c| c.abs() == 0.5));
        assert!(v.uv.iter().all(|&c| c == 0.0 || c == 1.0));
    }
}

#[test]
fn the_sphere_has_radius_half_and_wraps_the_texture_once() {
    let sphere = MeshData::sphere(32, 16);
    assert_eq!(sphere.vertices().len(), 33 * 17, "no splits");
    check_winding(&sphere);
    check_tangents(&sphere);
    for v in sphere.vertices() {
        assert!((Vec3::from(v.position).length() - 0.5).abs() < 1e-6);
        // v runs from the north pole (0, the texture's top) to the south (1).
        let v_expected = (v.position[1] * 2.0).clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
        assert!((v.uv[1] - v_expected).abs() < 1e-3, "{:?}", v);
    }
}

/// The shapes' hand-worked tangents point where MikkTSpace's do, so a
/// normal map baked for MikkTSpace looks right on them. On the sphere they
/// differ a little near the poles: MikkTSpace follows the flat triangles,
/// ours the round sphere they stand for.
#[test]
fn the_shapes_tangents_agree_with_mikktspace() {
    for (name, mesh, limit_degrees) in [
        ("cube", MeshData::cube(), 0.01),
        ("sphere", MeshData::sphere(48, 24), 4.0),
        ("plane", MeshData::plane(), 0.01),
    ] {
        let v = mesh.vertices();
        let positions: Vec<[f32; 3]> = v.iter().map(|v| v.position).collect();
        let normals: Vec<[f32; 3]> = v.iter().map(|v| v.normal).collect();
        let uvs: Vec<[f32; 2]> = v.iter().map(|v| v.uv).collect();
        let generated =
            MeshData::new(MeshParts { normals: Some(&normals), uvs: Some(&uvs), ..parts(&positions, mesh.indices()) })
                .unwrap();
        assert_eq!(generated.vertices().len(), v.len(), "{name}: MikkTSpace split no vertices");
        let mut worst = 0.0f32;
        for (ours, theirs) in v.iter().zip(generated.vertices()) {
            // At a pole every direction lies on the line of latitude.
            if ours.position[1].abs() == 0.5 && name == "sphere" {
                continue;
            }
            let (a, b) = (Vec3::from_slice(&ours.tangent[..3]), Vec3::from_slice(&theirs.tangent[..3]));
            worst = worst.max(a.angle_between(b).to_degrees());
            assert_eq!(ours.tangent[3], theirs.tangent[3], "{name} at {:?}: same side", ours.position);
        }
        assert!(worst < limit_degrees, "{name}: up to {worst}° from MikkTSpace's");
    }
}

#[test]
fn the_plane_faces_up() {
    let plane = MeshData::plane();
    check_winding(&plane);
    assert!(plane.vertices().iter().all(|v| v.normal == [0.0, 1.0, 0.0]));
}

#[test]
fn the_bounds_hold_every_vertex() {
    let mesh = MeshData::new(parts(&[[1.0, -2.0, 0.5], [-3.0, 4.0, 0.0], [0.0, 0.0, 6.0]], &[0, 1, 2])).unwrap();
    assert_eq!(mesh.bounds(), (Vec3::new(-3.0, -2.0, 0.0), Vec3::new(1.0, 4.0, 6.0)));
    assert_eq!(MeshData::cube().bounds(), (Vec3::splat(-0.5), Vec3::splat(0.5)));
}
