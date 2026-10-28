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
fn a_vertex_is_52_bytes() {
    assert_eq!(std::mem::size_of::<Vertex>(), 52);
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

// --- Skins -------------------------------------------------------------------

#[test]
fn skin_weights_are_scaled_to_add_up_exactly_and_a_vertex_with_none_follows_its_first_joint() {
    let joints = [[3, 1, 2, 7], [4, 5, 0, 0], [6, 2, 9, 9]];
    // Each third: rounding alone would make 65535 ± 1. Then 2 to 1, and
    // nothing at all.
    let weights = [[1.0, 1.0, 1.0, 0.0], [2.0, 1.0, 0.0, 0.0], [0.0; 4]];
    let mesh = MeshData::new(MeshParts { skin: Some((&joints, &weights)), ..parts(&TRIANGLE, &[0, 1, 2]) }).unwrap();
    let skin = mesh.skin().expect("skinned");
    for v in skin {
        assert_eq!(v.weights.iter().map(|&w| u32::from(w)).sum::<u32>(), 65535);
    }
    assert_eq!(skin[0].joints, [3, 1, 2, 0], "a joint that moves nothing is 0");
    assert_eq!(skin[1].weights, [43690, 21845, 0, 0]);
    assert_eq!(skin[2], SkinVertex { joints: [6, 2, 9, 9], weights: [65535, 0, 0, 0] });
    assert!(MeshData::new(parts(&TRIANGLE, &[0, 1, 2])).unwrap().skin().is_none());

    let negative = [[1.0, -0.5, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]];
    let refused = MeshParts { skin: Some((&joints, &negative)), ..parts(&TRIANGLE, &[0, 1, 2]) };
    assert_eq!(error(refused), "vertex 0 has a skin weight below 0");
    let short = MeshParts { skin: Some((&joints[..2], &weights[..2])), ..parts(&TRIANGLE, &[0, 1, 2]) };
    assert_eq!(error(short), "joints has 2 items, but there are 3 positions");
}

#[test]
fn a_vertex_split_for_its_tangents_keeps_its_joints() {
    // As the mirrored squares above: vertices 1 and 4 split in two.
    let positions =
        [[-1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = [3, 4, 1, 3, 1, 0, 4, 5, 2, 4, 2, 1];
    // Each vertex follows the joint of its own number.
    let joints: Vec<[u16; 4]> = (0..6).map(|i| [i, 0, 0, 0]).collect();
    let weights = [[1.0, 0.0, 0.0, 0.0]; 6];
    let skin = Some((&joints[..], &weights[..]));
    let mesh = MeshData::new(MeshParts { uvs: Some(&uvs), skin, ..parts(&positions, &indices) }).unwrap();
    assert_eq!(mesh.vertices().len(), 8);
    let skin = mesh.skin().unwrap();
    assert_eq!(skin.len(), 8);
    for (v, s) in mesh.vertices().iter().zip(skin) {
        let original = positions.iter().position(|p| *p == v.position).unwrap() as u16;
        assert_eq!(s.joints[0], original);
    }
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
        ("quad", MeshData::quad(), 0.01),
        ("cylinder", MeshData::cylinder(24), 0.1),
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
fn the_capsule_is_two_high_with_round_ends_of_radius_half() {
    let capsule = MeshData::capsule(24, 8);
    check_winding(&capsule);
    check_tangents(&capsule);
    assert_eq!(capsule.bounds(), (Vec3::new(-0.5, -1.0, -0.5), Vec3::new(0.5, 1.0, 0.5)));
    for v in capsule.vertices() {
        let p = Vec3::from(v.position);
        // Every point is 0.5 from the segment between the ends' centres.
        let on_axis = Vec3::new(0.0, p.y.clamp(-0.5, 0.5), 0.0);
        assert!(((p - on_axis).length() - 0.5).abs() < 1e-5, "{:?}", v);
        assert!((Vec3::from(v.normal) - (p - on_axis) * 2.0).length() < 1e-4, "{:?}", v);
    }
    let vs: Vec<f32> = capsule.vertices().iter().map(|v| v.uv[1]).collect();
    assert_eq!((vs.iter().cloned().fold(1.0, f32::min), vs.iter().cloned().fold(0.0, f32::max)), (0.0, 1.0));
}

#[test]
fn the_cylinder_is_two_high_of_radius_half_with_flat_caps() {
    let cylinder = MeshData::cylinder(24);
    check_winding(&cylinder);
    check_tangents(&cylinder);
    let (min, max) = cylinder.bounds();
    assert!((min - Vec3::new(-0.5, -1.0, -0.5)).length() < 1e-5 && (max - Vec3::new(0.5, 1.0, 0.5)).length() < 1e-5);
    for v in cylinder.vertices() {
        let (p, n) = (Vec3::from(v.position), Vec3::from(v.normal));
        if n.y == 0.0 {
            assert!((Vec3::new(p.x, 0.0, p.z).length() - 0.5).abs() < 1e-5, "{:?}", v);
        } else {
            assert_eq!(p.y, n.y, "a cap at its end: {:?}", v);
            assert!(v.uv.iter().all(|c| (0.0..=1.0).contains(c)));
        }
    }
}

#[test]
fn the_quad_faces_plus_z_with_its_image_upright() {
    let quad = MeshData::quad();
    check_winding(&quad);
    check_tangents(&quad);
    assert!(quad.vertices().iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
    let top_left = quad.vertices().iter().find(|v| v.position == [-0.5, 0.5, 0.0]).unwrap();
    assert_eq!(top_left.uv, [0.0, 0.0], "the image's top left at the top left, seen from +Z");
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

// --- A mesh that changes (stage 13 phase 1) ----------------------------------

/// A flat grid of `n` × `n` squares on XZ, its image upright from above.
fn grid(n: u32) -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
    let (mut positions, mut uvs, mut indices) = (vec![], vec![], vec![]);
    for z in 0..=n {
        for x in 0..=n {
            positions.push([x as f32, 0.0, z as f32]);
            uvs.push([x as f32 / n as f32, z as f32 / n as f32]);
        }
    }
    for z in 0..n {
        for x in 0..n {
            let i = z * (n + 1) + x;
            indices.extend([i, i + n + 1, i + 1, i + 1, i + n + 1, i + n + 2]);
        }
    }
    (positions, uvs, indices)
}

#[test]
fn colours_are_kept_and_white_without_them() {
    let colors = [[255, 0, 0, 255], [0, 255, 0, 128], [0, 0, 255, 0]];
    let mesh = MeshData::new(MeshParts { colors: Some(&colors), ..parts(&TRIANGLE, &[0, 1, 2]) }).unwrap();
    assert_eq!(mesh.vertices().iter().map(|v| v.color).collect::<Vec<_>>(), colors);
    assert!(MeshData::new(parts(&TRIANGLE, &[0, 1, 2])).unwrap().vertices().iter().all(|v| v.color == WHITE));
    let short = MeshParts { colors: Some(&colors[..2]), ..parts(&TRIANGLE, &[0, 1, 2]) };
    assert_eq!(error(short), "colors has 2 items, but there are 3 positions");
}

#[test]
fn submeshes_split_the_triangles_where_they_say() {
    let (positions, _, indices) = grid(2);
    let mesh = MeshData::new(MeshParts { submeshes: &[6, 18], ..parts(&positions, &indices) }).unwrap();
    assert_eq!(mesh.submeshes(), [0..6, 6..18, 18..24]);
    assert_eq!(MeshData::new(parts(&positions, &indices)).unwrap().submeshes(), std::slice::from_ref(&(0..24)));
    for (starts, message) in [
        (&[7][..], "submesh 1 starts at index 7"),
        (&[12, 6][..], "submesh 2 starts at index 6"),
        (&[27][..], "submesh 1 starts at index 27"),
    ] {
        let error = error(MeshParts { submeshes: starts, ..parts(&positions, &indices) });
        assert!(error.starts_with(message), "{error}");
    }
}

#[test]
fn an_empty_mesh_has_nothing_and_takes_a_change() {
    let mut mesh = MeshData::empty();
    assert!(mesh.vertices().is_empty() && mesh.indices().is_empty());
    assert_eq!(mesh.bounds(), (Vec3::ZERO, Vec3::ZERO));
    mesh.change(MeshChange { positions: Some(&TRIANGLE), indices: Some((&[0, 1, 2], &[])), ..Default::default() })
        .unwrap();
    assert_eq!(mesh.vertices().len(), 3);
    assert_eq!(mesh.bounds(), (Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0)));
    for v in mesh.vertices() {
        assert_eq!(v.normal, [0.0, 0.0, 1.0], "worked out because the count changed");
        assert_eq!(v.color, WHITE);
    }
    // Back to nothing.
    mesh.change(MeshChange { positions: Some(&[]), indices: Some((&[], &[])), ..Default::default() }).unwrap();
    assert!(mesh.vertices().is_empty() && mesh.indices().is_empty());
}

#[test]
fn a_change_keeps_what_it_doesnt_give() {
    let (positions, uvs, indices) = grid(2);
    let colors = vec![[10, 20, 30, 40]; positions.len()];
    let mut mesh =
        MeshData::new(MeshParts { uvs: Some(&uvs), colors: Some(&colors), ..parts(&positions, &indices) }).unwrap();
    let raised: Vec<_> = positions.iter().map(|p| [p[0], 2.0, p[2]]).collect();
    mesh.change(MeshChange { positions: Some(&raised), ..Default::default() }).unwrap();
    assert!(mesh.vertices().iter().all(|v| v.position[1] == 2.0 && v.color == [10, 20, 30, 40]));
    assert_eq!(mesh.vertices().iter().map(|v| v.uv).collect::<Vec<_>>(), uvs);
    assert_eq!(mesh.indices(), indices);
    assert_eq!(mesh.bounds().0.y, 2.0);
}

#[test]
fn a_refused_change_leaves_the_mesh_as_it_was() {
    let mut mesh = MeshData::new(parts(&TRIANGLE, &[0, 1, 2])).unwrap();
    let before = mesh.vertices().to_vec();
    let bad = |change: MeshChange<'_>, mesh: &mut MeshData| mesh.change(change).unwrap_err().to_string();
    let nan = [[0.0, 0.0, 0.0], [f32::NAN, 0.0, 0.0], [0.0, 1.0, 0.0]];
    assert_eq!(bad(MeshChange { positions: Some(&nan), ..Default::default() }, &mut mesh), "position 1 isn't finite");
    let four = [[0.0; 3]; 4];
    let two_colors = [WHITE; 2];
    assert_eq!(
        bad(MeshChange { positions: Some(&four), colors: Some(&two_colors), ..Default::default() }, &mut mesh),
        "colors has 2 items, but there are 4 positions"
    );
    assert_eq!(
        bad(MeshChange { indices: Some((&[0, 1, 3], &[])), ..Default::default() }, &mut mesh),
        "index 2 is 3, but there are only 3 vertices"
    );
    // Fewer vertices than the triangles it keeps use.
    assert_eq!(
        bad(MeshChange { positions: Some(&TRIANGLE[..2]), ..Default::default() }, &mut mesh),
        "index 2 is 2, but there are only 2 vertices"
    );
    assert_eq!(
        bad(MeshChange { indices: Some((&[0, 1], &[])), ..Default::default() }, &mut mesh),
        "indices come in threes, one per triangle corner, but there are 2"
    );
    assert_eq!(mesh.vertices(), before);
}

#[test]
fn a_skinned_mesh_cant_change() {
    let joints = [[0, 0, 0, 0]; 3];
    let weights = [[1.0, 0.0, 0.0, 0.0]; 3];
    let mut mesh =
        MeshData::new(MeshParts { skin: Some((&joints, &weights)), ..parts(&TRIANGLE, &[0, 1, 2]) }).unwrap();
    let refused = mesh.change(MeshChange { positions: Some(&TRIANGLE), ..Default::default() });
    assert_eq!(refused.unwrap_err().to_string(), "a skinned mesh can't be changed");
}

#[test]
fn recalculated_normals_follow_the_surface() {
    let (positions, uvs, indices) = grid(8);
    let mut mesh = MeshData::new(MeshParts { uvs: Some(&uvs), ..parts(&positions, &indices) }).unwrap();
    // A ridge along x = 4: the slopes face away from it.
    let ridge: Vec<_> = positions.iter().map(|p| [p[0], 4.0 - (p[0] - 4.0).abs(), p[2]]).collect();
    mesh.change(MeshChange { positions: Some(&ridge), recalculate_normals: true, ..Default::default() }).unwrap();
    for v in mesh.vertices() {
        let n = Vec3::from(v.normal);
        assert!((n.length() - 1.0).abs() < 1e-5);
        let expected = match v.position[0] {
            x if x < 4.0 => Vec3::new(-1.0, 1.0, 0.0).normalize(),
            x if x > 4.0 => Vec3::new(1.0, 1.0, 0.0).normalize(),
            // On the ridge it depends on how many triangles each side has
            // there, which is what weighting by area does; up-ish.
            _ => {
                assert!(n.y > 0.9, "{n} on the ridge");
                continue;
            }
        };
        assert!(n.distance(expected) < 1e-4, "{n} at {:?}, not {expected}", v.position);
    }
}

#[test]
fn recalculated_tangents_point_along_u_with_the_bitangent_up_the_image() {
    let (positions, uvs, indices) = grid(4);
    let mut mesh = MeshData::new(MeshParts { uvs: Some(&uvs), ..parts(&positions, &indices) }).unwrap();
    let wave: Vec<_> = positions.iter().map(|p| [p[0], (p[0] * 0.7).sin() + (p[2] * 0.5).cos(), p[2]]).collect();
    mesh.change(MeshChange {
        positions: Some(&wave),
        recalculate_normals: true,
        recalculate_tangents: true,
        ..Default::default()
    })
    .unwrap();
    check_tangents(&mesh);
    // The image mirrored left to right: w flips.
    let mirrored: Vec<_> = uvs.iter().map(|uv| [1.0 - uv[0], uv[1]]).collect();
    mesh.change(MeshChange { uvs: Some(&mirrored), recalculate_tangents: true, ..Default::default() }).unwrap();
    check_tangents(&mesh);
}

/// What changing a 101 × 101 grid costs, part by part, each held to a bar
/// (stage 14's GT1): `cargo test --release -p fynkel_mesh -- --ignored bench_`.
#[test]
#[ignore = "a benchmark: run in release"]
fn bench_changing_a_grid() {
    let n = 101_u32;
    let positions: Vec<[f32; 3]> = (0..n * n).map(|i| [(i % n) as f32, 0.0, (i / n) as f32]).collect();
    let mut indices = Vec::new();
    for z in 0..n - 1 {
        for x in 0..n - 1 {
            let a = z * n + x;
            indices.extend([a, a + n, a + 1, a + 1, a + n, a + n + 1]);
        }
    }
    let mut mesh = MeshData::empty();
    mesh.change(MeshChange { positions: Some(&positions), indices: Some((&indices, &[])), ..MeshChange::default() })
        .unwrap();
    use fynkel_bench::bench;
    let shared = std::sync::Arc::new(mesh.clone());
    bench("a 101 × 101 grid: clone", 200, 0.03, || drop(std::hint::black_box((*shared).clone())));
    bench("a 101 × 101 grid: positions", 200, 0.07, || {
        mesh.change(MeshChange { positions: Some(&positions), ..MeshChange::default() }).unwrap();
    });
    bench("a 101 × 101 grid: normals", 200, 0.15, || mesh.recalculate_normals());
    bench("a 101 × 101 grid: tangents", 200, 0.12, || mesh.recalculate_tangents());
}

#[test]
fn a_copy_written_into_an_older_one_has_the_new_triangles() {
    let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0]];
    let mut mesh = MeshData::empty();
    mesh.change(MeshChange { positions: Some(&positions), indices: Some((&[0, 2, 1], &[])), ..MeshChange::default() })
        .unwrap();
    let mut older = mesh.clone();
    mesh.change(MeshChange { indices: Some((&[0, 2, 1, 1, 2, 3], &[3])), ..MeshChange::default() }).unwrap();
    older.clone_from(&mesh);
    assert_eq!(older, mesh);
    assert_eq!(older.indices(), &[0, 2, 1, 1, 2, 3]);
    assert_eq!(older.submeshes().len(), 2);

    // Only the positions changed since: the same triangles stay.
    let mut same = mesh.clone();
    mesh.change(MeshChange { positions: Some(&[[0.0, 1.0, 0.0]; 4]), ..MeshChange::default() }).unwrap();
    same.clone_from(&mesh);
    assert_eq!(same, mesh);
}
