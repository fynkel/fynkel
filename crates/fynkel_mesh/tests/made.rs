//! Shapes with settings, outlines made solid, and meshes joined (stage 13
//! phase 2): each is wound outwards with tangents along u, has the size it
//! was asked for, and is closed where it should be (every edge shared by
//! exactly two triangles, once each way).

mod common;

use common::*;
use fynkel_mesh::*;
use glam::{Mat4, Quat, Vec2, Vec3};

// --- Shapes with settings --------------------------------------------------------

#[test]
fn a_box_has_its_size_its_segments_and_uvs_in_metres() {
    let size = Vec3::new(4.0, 2.0, 1.0);
    let b = MeshData::box_shape(size, [4, 2, 1], 1.0);
    check_winding(&b);
    check_tangents(&b);
    check_closed(&b);
    bounds_are(&b, -size * 0.5, size * 0.5);
    assert!(close(volume(&b), 8.0, 1e-4), "volume {}", volume(&b));
    // Two faces of 4×2 squares, two of 4×1, two of 1×2: 2 triangles each.
    assert_eq!(b.indices().len() / 3, 2 * (2 * 8 + 2 * 4 + 2 * 2));
    // The front (+Z) is 4 m wide and 2 high: its UVs run to 4 and 2.
    let front: Vec<_> = b.vertices().iter().filter(|v| v.normal == [0.0, 0.0, 1.0]).collect();
    let max_u = front.iter().map(|v| v.uv[0]).fold(0.0, f32::max);
    let max_v = front.iter().map(|v| v.uv[1]).fold(0.0, f32::max);
    assert_eq!((max_u, max_v), (4.0, 2.0));
    let half = MeshData::box_shape(size, [1, 1, 1], 0.5);
    assert_eq!(half.vertices().iter().map(|v| v.uv[0]).fold(0.0, f32::max), 2.0, "half as many repeats");
}

#[test]
fn a_grid_is_flat_facing_up_cut_into_squares() {
    let g = MeshData::grid(Vec2::new(10.0, 6.0), [5, 3], 1.0);
    check_winding(&g);
    check_tangents(&g);
    assert_eq!(g.vertices().len(), 6 * 4);
    assert_eq!(g.indices().len(), 5 * 3 * 6);
    bounds_are(&g, Vec3::new(-5.0, 0.0, -3.0), Vec3::new(5.0, 0.0, 3.0));
    assert!(g.vertices().iter().all(|v| v.normal == [0.0, 1.0, 0.0]));
    // As the plane: image-up is -Z, so v is 0 at the far edge.
    let far = g.vertices().iter().find(|v| v.position == [-5.0, 0.0, -3.0]).unwrap();
    assert_eq!(far.uv, [0.0, 0.0]);
}

#[test]
fn a_sized_sphere_and_capsule_and_cylinder_have_the_sizes_asked_for() {
    let sphere = MeshData::sphere_sized(2.0, 24, 12);
    check_winding(&sphere);
    check_closed(&sphere);
    bounds_are(&sphere, Vec3::splat(-2.0), Vec3::splat(2.0));

    let capsule = MeshData::capsule_sized(0.25, 3.0, 16, 4);
    check_winding(&capsule);
    check_tangents(&capsule);
    check_closed(&capsule);
    bounds_are(&capsule, Vec3::new(-0.25, -1.5, -0.25), Vec3::new(0.25, 1.5, 0.25));
    let ball = MeshData::capsule_sized(0.5, 0.5, 16, 4);
    check_winding(&ball);
    bounds_are(&ball, Vec3::splat(-0.5), Vec3::splat(0.5));

    let cylinder = MeshData::cylinder_sized(1.0, 0.5, 32, true);
    check_winding(&cylinder);
    check_tangents(&cylinder);
    check_closed(&cylinder);
    bounds_are(&cylinder, Vec3::new(-1.0, -0.25, -1.0), Vec3::new(1.0, 0.25, 1.0));
    let tube = MeshData::cylinder_sized(1.0, 0.5, 32, false);
    assert_eq!(tube.indices().len(), 32 * 6, "no caps: only the side");
}

#[test]
fn the_shapes_made_the_old_way_are_unchanged() {
    // The default shapes are the sized ones at their default sizes.
    assert_eq!(MeshData::capsule(24, 6), MeshData::capsule_sized(0.5, 2.0, 24, 6));
    assert_eq!(MeshData::cylinder(24), MeshData::cylinder_sized(0.5, 2.0, 24, true));
    assert_eq!(MeshData::cube().vertices().len(), MeshData::box_shape(Vec3::ONE, [1; 3], 1.0).vertices().len());
    // The same corners, normals and tangents, listed in another order.
    let corners = |m: &MeshData| {
        let mut c: Vec<String> =
            m.vertices().iter().map(|v| format!("{:?}", (v.position, v.normal, v.tangent))).collect();
        c.sort();
        c
    };
    assert_eq!(corners(&MeshData::cube()), corners(&MeshData::box_shape(Vec3::ONE, [1; 3], 1.0)));
}

#[test]
fn a_cone_comes_to_a_point_with_a_flat_base() {
    let cone = MeshData::cone(1.0, 2.0, 24, true);
    check_winding(&cone);
    check_tangents(&cone);
    check_closed(&cone);
    bounds_are(&cone, Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
    // π r² h / 3, a little less with 24 sides.
    let v = volume(&cone);
    assert!(v < std::f32::consts::PI * 2.0 / 3.0 && v > 2.0, "volume {v}");
    let open = MeshData::cone(1.0, 2.0, 24, false);
    assert_eq!(open.indices().len(), 24 * 3);
}

#[test]
fn a_torus_is_a_closed_ring() {
    let torus = MeshData::torus(1.0, 0.25, 32, 12);
    check_winding(&torus);
    check_tangents(&torus);
    check_closed(&torus);
    bounds_are(&torus, Vec3::new(-1.25, -0.25, -1.25), Vec3::new(1.25, 0.25, 1.25));
    // 2π² R r², a little less with flat sides.
    let v = volume(&torus);
    let exact = 2.0 * std::f32::consts::PI.powi(2) * 0.0625;
    assert!(v < exact && v > exact * 0.9, "volume {v} of {exact}");
}

#[test]
fn a_disc_faces_up_and_a_ring_has_a_hole() {
    let disc = MeshData::disc(1.0, 0.0, 32);
    check_winding(&disc);
    check_tangents(&disc);
    assert!(disc.vertices().iter().all(|v| v.normal == [0.0, 1.0, 0.0]));
    let ring = MeshData::disc(1.0, 0.5, 32);
    check_winding(&ring);
    check_tangents(&ring);
    assert_eq!(ring.indices().len(), 32 * 6);
    for v in ring.vertices() {
        let r = Vec2::new(v.position[0], v.position[2]).length();
        assert!(close(r, 1.0, 1e-5) || close(r, 0.5, 1e-5));
    }
}

// --- Outlines --------------------------------------------------------------------

fn square(half: f32, at: Vec2) -> Vec<Vec2> {
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|&(x, y)| at + Vec2::new(x, y) * half).collect()
}

fn circle(radius: f32, at: Vec2, n: usize) -> Vec<Vec2> {
    (0..n).map(|i| at + Vec2::from_angle(std::f32::consts::TAU * i as f32 / n as f32) * radius).collect()
}

/// The area the triangles cover, counter-clockwise as positive.
fn filled_area(points: &[Vec2], triangles: &[u32]) -> f32 {
    triangles
        .chunks_exact(3)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| points[t[k] as usize]);
            (b - a).perp_dot(c - a) * 0.5
        })
        .sum()
}

#[test]
fn an_outline_is_filled_either_way_round() {
    let mut s = square(1.0, Vec2::ZERO);
    let t = fill(&s, &[]).unwrap();
    assert_eq!(t.len(), 6);
    assert!(close(filled_area(&s, &t), 4.0, 1e-5));
    s.reverse();
    let t = fill(&s, &[]).unwrap();
    assert!(close(filled_area(&s, &t), 4.0, 1e-5), "clockwise given: still filled facing up");
}

#[test]
fn a_concave_outline_is_filled_without_covering_its_notch() {
    // An L.
    let l: Vec<Vec2> = [(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)]
        .iter()
        .map(|&(x, y)| Vec2::new(x, y))
        .collect();
    let t = fill(&l, &[]).unwrap();
    assert_eq!(t.len() / 3, 4);
    assert!(close(filled_area(&l, &t), 3.0, 1e-5));
    for tri in t.chunks_exact(3) {
        let a = filled_area(&l, tri);
        assert!(a > 0.0, "every triangle faces up");
    }
}

#[test]
fn holes_are_left_open() {
    let outer = square(2.0, Vec2::ZERO);
    let (left, right) = (square(0.5, Vec2::new(-1.0, 0.0)), circle(0.5, Vec2::new(1.0, 0.5), 16));
    let t = fill(&outer, &[&left, &right]).unwrap();
    let points: Vec<Vec2> = [outer.as_slice(), &left, &right].concat();
    let holes = 1.0 + 16.0 * 0.5 * 0.25 * (std::f32::consts::TAU / 16.0).sin();
    assert!(close(filled_area(&points, &t), 16.0 - holes, 1e-4), "area {}", filled_area(&points, &t));
    // n + 2h - 2 triangles for n points and h holes.
    assert_eq!(t.len() / 3, points.len() + 2 * 2 - 2);
    for tri in t.chunks_exact(3) {
        assert!(filled_area(&points, tri) > 0.0);
    }
}

#[test]
fn a_broken_outline_is_refused() {
    let two = [Vec2::ZERO, Vec2::X];
    assert_eq!(fill(&two, &[]).unwrap_err().to_string(), "the outline has 2 points; it needs at least 3");
    let sq = square(1.0, Vec2::ZERO);
    assert_eq!(fill(&sq, &[&two]).unwrap_err().to_string(), "hole 1 has 2 points; it needs at least 3");
    let nan = [Vec2::ZERO, Vec2::X, Vec2::new(f32::NAN, 1.0)];
    assert_eq!(fill(&nan, &[]).unwrap_err().to_string(), "outline point 2 isn't finite");
}

/// Whether `q` is inside the loop `ring`, by the even-odd rule.
fn inside_loop(q: Vec2, ring: &[Vec2]) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// The area inside a loop.
fn ring_area(ring: &[Vec2]) -> f32 {
    (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum::<f32>().abs() * 0.5
}

/// Checks a fill as a grid of samples over the shape's bounds: each one
/// inside the outline and outside every hole is covered by exactly one
/// triangle, and every other by none. The area is the outline's less the
/// holes', and every triangle faces up.
fn check_fill(outline: &[Vec2], holes: &[&[Vec2]], triangles: &[u32]) {
    let points: Vec<Vec2> = std::iter::once(outline).chain(holes.iter().copied()).flatten().copied().collect();
    let want = ring_area(outline) - holes.iter().map(|h| ring_area(h)).sum::<f32>();
    let got = filled_area(&points, triangles);
    assert!(close(got, want, 1e-3 * want.max(1.0)), "area {got}, want {want}");
    for tri in triangles.chunks_exact(3) {
        assert!(filled_area(&points, tri) >= 0.0, "a triangle faces down");
    }
    let (min, max) = outline.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    let steps = 48;
    let (mut twice, mut outside, mut missed) = (0, 0, 0);
    for gy in 0..steps {
        for gx in 0..steps {
            // Off the grid a little, so few samples lie on an edge.
            let f = Vec2::new(gx as f32 + 0.5137, gy as f32 + 0.4871) / steps as f32;
            let q = min + (max - min) * f;
            // Covered inside a triangle, and on its edges too.
            let (mut within, mut touching) = (0, 0);
            for t in triangles.chunks_exact(3) {
                let [a, b, c] = [0, 1, 2].map(|k| points[t[k] as usize]);
                let d = [(b - a).perp_dot(q - a), (c - b).perp_dot(q - b), (a - c).perp_dot(q - c)];
                within += d.iter().all(|&d| d > 0.0) as usize;
                touching += d.iter().all(|&d| d >= 0.0) as usize;
            }
            let inside = inside_loop(q, outline) && !holes.iter().any(|h| inside_loop(q, h));
            twice += (within > 1) as usize;
            outside += (within > 0 && !inside) as usize;
            missed += (touching == 0 && inside) as usize;
        }
    }
    assert_eq!(
        (twice, outside, missed),
        (0, 0, 0),
        "samples covered twice, covered outside the shape or in a hole, left uncovered"
    );
}

// Stage 14's WO9: a B and an 8, each with two holes, once filled with
// triangles over each other and over the holes.
#[test]
fn a_b_and_an_8_keep_both_their_holes() {
    // A B: a straight back, two bowls, two holes the same width.
    let bowl = |at: Vec2| {
        (0..=12).map(move |i| {
            at + Vec2::from_angle(-std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.0) * 0.6
        })
    };
    let mut b: Vec<Vec2> = vec![Vec2::new(0.0, 0.0)];
    b.extend(bowl(Vec2::new(1.2, 0.6)));
    b.extend(bowl(Vec2::new(1.2, 1.8)).skip(1));
    b.push(Vec2::new(0.0, 2.4));
    let holes_b = [square(0.25, Vec2::new(0.9, 0.6)), square(0.25, Vec2::new(0.9, 1.8))];
    let refs: Vec<&[Vec2]> = holes_b.iter().map(Vec::as_slice).collect();
    let t = fill(&b, &refs).unwrap();
    check_fill(&b, &refs, &t);
    // An 8: a tall rounded outline, two round holes one above the other.
    let eight: Vec<Vec2> = (0..64)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / 64.0;
            Vec2::new(a.cos() * (1.0 - 0.3 * (2.0 * a).cos().max(0.0) * a.cos().abs()), 2.0 * a.sin())
        })
        .collect();
    let holes_8 = [circle(0.45, Vec2::new(0.0, 0.9), 20), circle(0.45, Vec2::new(0.0, -0.9), 20)];
    let refs: Vec<&[Vec2]> = holes_8.iter().map(Vec::as_slice).collect();
    let t = fill(&eight, &refs).unwrap();
    check_fill(&eight, &refs, &t);
    // An 8 on its side, coarse: both holes are cut to the same corner, on
    // the right, and the second cut once went in at the wrong one of the
    // corner's two visits.
    let points = |xy: &[(f32, f32)]| xy.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>();
    let side = points(&[
        (3.129_141_8, 0.0),
        (1.942_408, 1.411_242),
        (0.794_105_2, 2.444_004_5),
        (-1.351_300_1, 4.158_873_6),
        (-2.546_645_9, 1.850_246_2),
        (-4.735_096, -4.139_552_3e-7),
        (-2.077_305_3, -1.509_251_1),
        (-1.354_524_1, -4.168_802),
        (1.470_043_9, -4.524_328),
        (3.250_333, -2.361_505_5),
    ]);
    let holes_side = [
        points(&[(0.857_964_9, 0.395_539_28), (0.548_866_6, 0.573_997_2), (0.548_866_7, 0.217_081_37)]),
        points(&[(0.268_200_93, 0.002_021_828_4), (-0.121_326_91, 0.226_915_82), (-0.121_326_864, -0.222_872_17)]),
    ];
    let refs: Vec<&[Vec2]> = holes_side.iter().map(Vec::as_slice).collect();
    let t = fill(&side, &refs).unwrap();
    check_fill(&side, &refs, &t);
}

// Stage 14's WO9: many outlines with up to five holes, each filled
// exactly: the area less the holes, nothing twice, nothing outside.
#[test]
fn outlines_with_many_holes_are_filled_exactly() {
    let mut seed = 0x1234_5678_u64;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32
    };
    for _ in 0..200 {
        // A star-shaped outline, its nearest edge at least 1.5 from the
        // centre; holes no more than 1.15 from it, apart from each other.
        let n = 5 + (random() * 40.0) as usize;
        let outline: Vec<Vec2> = (0..n)
            .map(|i| Vec2::from_angle(std::f32::consts::TAU * i as f32 / n as f32) * (2.0 + random() * 3.0))
            .collect();
        let mut holes: Vec<Vec<Vec2>> = Vec::new();
        for _ in 0..(random() * 6.0) as usize {
            let at = Vec2::from_angle(random() * std::f32::consts::TAU) * random() * 0.8;
            if holes.iter().all(|h| (h[0] - at).length() > 0.8) {
                holes.push(circle(0.12 + random() * 0.2, at, 3 + (random() * 8.0) as usize));
            }
        }
        let refs: Vec<&[Vec2]> = holes.iter().map(Vec::as_slice).collect();
        let t = fill(&outline, &refs).unwrap();
        check_fill(&outline, &refs, &t);
    }
}

// Stage 14's WO9: a hole outside the outline was joined to it when to its
// left, putting triangles outside, and dropped when to its right.
#[test]
fn a_hole_outside_the_outline_is_refused() {
    let sq = square(0.5, Vec2::splat(0.5));
    for at in [Vec2::new(-2.0, 0.5), Vec2::new(3.0, 0.5), Vec2::new(0.5, 4.0)] {
        let hole = circle(0.2, at, 6);
        assert_eq!(fill(&sq, &[&hole]).unwrap_err().to_string(), "hole 1 isn't inside the outline");
        assert!(MeshData::extrude(&sq, &[&hole], 0.5, 1.0).is_err());
    }
}

// Stage 14's WO9: an outline that touches itself at a corner (two squares
// walked as one loop) was filled outside itself.
#[test]
fn an_outline_touching_itself_is_filled_inside_only() {
    let touching: Vec<Vec2> =
        [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0), (1.0, 1.0), (0.0, 1.0)]
            .iter()
            .map(|&(x, y)| Vec2::new(x, y))
            .collect();
    let t = fill(&touching, &[]).unwrap();
    check_fill(&touching, &[], &t);
    // A notch whose point touches the far wall.
    let notch: Vec<Vec2> = [(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0), (0.0, 2.0), (3.0, 1.5), (0.0, 1.0)]
        .iter()
        .map(|&(x, y)| Vec2::new(x, y))
        .collect();
    let t = fill(&notch, &[]).unwrap();
    check_fill(&notch, &[], &t);
}

// Stage 14's WO9: filling took time by the square of the points, 120 ms
// for a circle of 10,000 in release. Debug builds are some ten times
// slower, so they're given more.
#[test]
fn ten_thousand_points_are_filled_quickly() {
    let big = circle(1.0, Vec2::ZERO, 10_000);
    let star: Vec<Vec2> = (0..10_000)
        .map(|i| Vec2::from_angle(std::f32::consts::TAU * i as f32 / 10_000.0) * if i % 2 == 0 { 1.0 } else { 0.6 })
        .collect();
    let holes: Vec<Vec<Vec2>> =
        (0..8).map(|k| circle(0.05, Vec2::from_angle(std::f32::consts::TAU * k as f32 / 8.0) * 0.3, 100)).collect();
    let refs: Vec<&[Vec2]> = holes.iter().map(Vec::as_slice).collect();
    let bound = if cfg!(debug_assertions) { 400.0 } else { 20.0 };
    for (outline, holes) in [(&big, &[][..]), (&star, &refs[..])] {
        let start = std::time::Instant::now();
        let t = fill(outline, holes).unwrap();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        assert!(ms < bound, "{} points took {ms:.1} ms", outline.len());
        // Too many triangles to sample in a debug build: the area only.
        let points: Vec<Vec2> = std::iter::once(&outline[..]).chain(holes.iter().copied()).flatten().copied().collect();
        let want = ring_area(outline) - holes.iter().map(|h| ring_area(h)).sum::<f32>();
        assert!(close(filled_area(&points, &t), want, 1e-4), "area {}", filled_area(&points, &t));
    }
}

#[test]
fn an_extruded_outline_with_a_hole_is_closed_with_the_volume_it_should_have() {
    let outer = square(1.0, Vec2::ZERO);
    let hole = square(0.5, Vec2::ZERO);
    let slab = MeshData::extrude(&outer, &[&hole], 0.5, 1.0).unwrap();
    check_winding(&slab);
    check_tangents(&slab);
    check_closed(&slab);
    assert!(close(volume(&slab), 3.0 * 0.5, 1e-4), "volume {}", volume(&slab));
    bounds_are(&slab, Vec3::new(-1.0, -1.0, -0.25), Vec3::new(1.0, 1.0, 0.25));
    // Square corners are sharp: each side is flat.
    for v in slab.vertices() {
        let n = Vec3::from(v.normal);
        assert!(n.abs().max_element() == 1.0, "a flat side's normal: {n}");
    }
}

#[test]
fn an_extruded_circle_is_smooth_round_its_side() {
    let c = circle(1.0, Vec2::ZERO, 32);
    let disc = MeshData::extrude(&c, &[], 0.2, 1.0).unwrap();
    check_winding(&disc);
    check_tangents(&disc);
    check_closed(&disc);
    for v in disc.vertices() {
        let (p, n) = (Vec3::from(v.position), Vec3::from(v.normal));
        if n.z == 0.0 {
            assert!((n - Vec3::new(p.x, p.y, 0.0).normalize()).length() < 1e-3, "smooth: {n} at {p}");
        }
    }
}

#[test]
fn a_lathed_profile_is_a_closed_vase() {
    // From the middle of the base out, up the side, in over the rim and
    // down inside to the floor, back to the middle.
    let profile: Vec<Vec2> = [(0.0, 0.0), (0.5, 0.0), (0.6, 1.0), (0.5, 1.0), (0.4, 0.1), (0.0, 0.1)]
        .iter()
        .map(|&(r, y)| Vec2::new(r, y))
        .collect();
    let vase = MeshData::lathe(&profile, 32).unwrap();
    check_winding(&vase);
    check_tangents(&vase);
    check_closed(&vase);
    bounds_are(&vase, Vec3::new(-0.6, 0.0, -0.6), Vec3::new(0.6, 1.0, 0.6));
    assert!(volume(&vase) > 0.0, "facing out");
}

#[test]
fn a_lathed_sphere_profile_is_smooth() {
    // A half circle from the bottom pole to the top.
    let profile: Vec<Vec2> = (0..=16)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
            Vec2::new(a.cos().max(0.0), a.sin())
        })
        .collect();
    let ball = MeshData::lathe(&profile, 32).unwrap();
    check_winding(&ball);
    check_closed(&ball);
    for v in ball.vertices() {
        let (p, n) = (Vec3::from(v.position), Vec3::from(v.normal));
        assert!(n.dot(p.normalize()) > 0.98, "smooth: {n} at {p}");
    }
    assert!(MeshData::lathe(&[Vec2::ZERO], 8).is_err());
}

// --- Combining ---------------------------------------------------------------------

#[test]
fn combined_meshes_are_placed_and_keep_their_parts() {
    // A house: walls (submesh 0) and a roof (submesh 1).
    let walls = MeshData::box_shape(Vec3::ONE, [1; 3], 1.0);
    let roof = MeshData::cone(0.8, 0.6, 4, true);
    let house = MeshData::combine(&[(&walls, Mat4::IDENTITY), (&roof, Mat4::from_translation(Vec3::Y * 0.8))]).unwrap();
    assert_eq!(house.submeshes().len(), 1, "one part each: one part");
    assert_eq!(house.indices().len(), walls.indices().len() + roof.indices().len());

    // Two parts in each of three houses: two parts in the town.
    let positions: Vec<[f32; 3]> = house.vertices().iter().map(|v| v.position).collect();
    let mut indices = walls.indices().to_vec();
    let start = indices.len() as u32;
    indices.extend(roof.indices().iter().map(|i| i + walls.vertices().len() as u32));
    let two = MeshData::new(MeshParts {
        positions: &positions,
        indices: &indices,
        submeshes: &[start],
        ..MeshParts::default()
    })
    .unwrap();
    let at = |x: f32| Mat4::from_translation(Vec3::X * x);
    let town = MeshData::combine(&[(&two, at(0.0)), (&two, at(2.0)), (&two, at(4.0))]).unwrap();
    assert_eq!(town.vertices().len(), 3 * two.vertices().len());
    assert_eq!(town.submeshes().len(), 2);
    let (walls_n, roofs_n) = (town.submeshes()[0].len(), town.submeshes()[1].len());
    assert_eq!((walls_n, roofs_n), (3 * start as usize, 3 * (indices.len() - start as usize)));
    bounds_are(&town, Vec3::new(-0.8, -0.5, -0.8), Vec3::new(4.8, 1.1, 0.8));
}

#[test]
fn a_mirrored_mesh_still_faces_out_when_combined() {
    let cone = MeshData::cone(0.5, 1.0, 12, true);
    let turned = Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(1.0, 2.0, 3.0));
    let mirrored = Mat4::from_scale(Vec3::new(-1.0, 2.0, 1.0));
    let both = MeshData::combine(&[(&cone, turned), (&cone, mirrored)]).unwrap();
    check_winding(&both);
    check_tangents(&both);
    check_closed(&both);
    assert!(volume(&both) > 0.0);
}

#[test]
fn a_skinned_mesh_cant_be_combined() {
    let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let joints = [[0u16; 4]; 3];
    let weights = [[1.0, 0.0, 0.0, 0.0]; 3];
    let skinned = MeshData::new(MeshParts {
        positions: &positions,
        indices: &[0, 1, 2],
        skin: Some((&joints, &weights)),
        ..MeshParts::default()
    })
    .unwrap();
    assert_eq!(MeshData::combine(&[(&skinned, Mat4::IDENTITY)]), Err(MeshError::Skinned));
}

/// What making each kind of mesh takes, each held to a bar (stage 14's
/// GT1): `cargo test --release -p fynkel_mesh -- --ignored bench_`.
#[test]
#[ignore = "a benchmark: run in release"]
fn bench_making_meshes() {
    fn time(name: &str, bar_ms: f64, make: impl Fn() -> MeshData) {
        let triangles = make().indices().len() / 3;
        fynkel_bench::bench(&format!("{name}, {triangles} triangles"), 50, bar_ms, || {
            drop(std::hint::black_box(make()))
        });
    }
    time("box 8 a side", 0.03, || MeshData::box_shape(Vec3::ONE, [8; 3], 1.0));
    time("grid 100 × 100", 0.35, || MeshData::grid(Vec2::splat(10.0), [100, 100], 1.0));
    time("sphere 48 × 24", 0.06, || MeshData::sphere_sized(1.0, 48, 24));
    time("torus 64 × 32", 0.1, || MeshData::torus(1.0, 0.25, 64, 32));
    let gear: Vec<Vec2> = (0..96)
        .map(|i| Vec2::from_angle(std::f32::consts::TAU * i as f32 / 96.0) * if (i / 4) % 2 == 0 { 1.0 } else { 0.85 })
        .collect();
    let holes: Vec<Vec<Vec2>> = (0..6)
        .map(|k| {
            let at = Vec2::from_angle(std::f32::consts::TAU * k as f32 / 6.0) * 0.5;
            (0..16).map(|i| at + Vec2::from_angle(std::f32::consts::TAU * i as f32 / 16.0) * 0.12).collect()
        })
        .collect();
    let hole_refs: Vec<&[Vec2]> = holes.iter().map(Vec::as_slice).collect();
    time("a gear of 96 points with 6 holes, extruded", 0.08, || {
        MeshData::extrude(&gear, &hole_refs, 0.2, 1.0).unwrap()
    });
    let profile: Vec<Vec2> = (0..=40).map(|i| Vec2::new(0.3 + 0.1 * (i as f32 * 0.3).sin(), i as f32 * 0.05)).collect();
    time("a profile of 41 points, lathed 48 times round", 0.15, || MeshData::lathe(&profile, 48).unwrap());
    let house = MeshData::box_shape(Vec3::ONE, [1; 3], 1.0);
    let at: Vec<(&MeshData, glam::Mat4)> =
        (0..100).map(|i| (&house, Mat4::from_translation(Vec3::new(i as f32, 0.0, 0.0)))).collect();
    time("100 boxes combined", 0.17, || MeshData::combine(&at).unwrap());
}
