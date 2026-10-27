//! An outline swept along a path (stage 13 phase 3): along a straight
//! path it's the outline extruded; around a ring it closes on itself; its
//! frame puts the outline's X to the right and Y up; a sweep into a mesh
//! keeps the mesh's triangles when they're the same.

mod common;

use std::f32::consts::{PI, TAU};

use common::*;
use fynkel_mesh::*;
use glam::{Vec2, Vec3};

fn rect(w: f32, h: f32) -> Vec<Vec2> {
    let (w, h) = (w / 2.0, h / 2.0);
    vec![Vec2::new(-w, -h), Vec2::new(w, -h), Vec2::new(w, h), Vec2::new(-w, h)]
}

fn circle(radius: f32, n: usize) -> Vec<Vec2> {
    (0..n).map(|i| Vec2::from_angle(TAU * i as f32 / n as f32) * radius).collect()
}

/// `count` points from `from` to `to`, up +Y.
fn straight(from: Vec3, to: Vec3, count: usize) -> Vec<PathPoint> {
    (0..count)
        .map(|i| {
            let t = i as f32 / (count - 1) as f32;
            PathPoint {
                position: from.lerp(to, t),
                forward: to - from,
                up: Vec3::Y,
                distance: (to - from).length() * t,
            }
        })
        .collect()
}

/// A ring of `radius` around Y in the XZ plane, `count` pieces, the last
/// point where the first is.
fn ring(radius: f32, count: usize) -> Vec<PathPoint> {
    (0..=count)
        .map(|i| {
            let a = TAU * (i % count) as f32 / count as f32;
            PathPoint {
                position: Vec3::new(a.cos(), 0.0, a.sin()) * radius,
                forward: Vec3::new(-a.sin(), 0.0, a.cos()),
                up: Vec3::Y,
                distance: TAU * radius * i as f32 / count as f32,
            }
        })
        .collect()
}

fn sweep<'a>(path: &'a [PathPoint], outline: &'a [Vec2], holes: &'a [&'a [Vec2]], caps: bool) -> Sweep<'a> {
    Sweep { path, outline, holes, caps, uv_per_metre: 1.0 }
}

/// Each vertex's position and normal, rounded and sorted.
fn corners(mesh: &MeshData) -> Vec<[i64; 6]> {
    let mut all: Vec<[i64; 6]> = mesh
        .vertices()
        .iter()
        .map(|v| {
            let [x, y, z] = v.position;
            let [a, b, c] = v.normal;
            [x, y, z, a, b, c].map(|f| (f * 1e4).round() as i64)
        })
        .collect();
    all.sort();
    all
}

#[test]
fn swept_along_a_straight_path_an_outline_is_extruded() {
    let outline = circle(1.0, 12);
    let hole = rect(0.6, 0.6);
    // An extrusion's front faces +Z: the path goes from front to back.
    let path = straight(Vec3::Z, Vec3::NEG_Z, 2);
    let swept = MeshData::sweep(&sweep(&path, &outline, &[&hole], true)).unwrap();
    let extruded = MeshData::extrude(&outline, &[&hole], 2.0, 1.0).unwrap();
    check_winding(&swept);
    check_tangents(&swept);
    check_closed(&swept);
    assert_eq!(corners(&swept), corners(&extruded));
    assert!(close(volume(&swept), volume(&extruded), 1e-4), "volume {}", volume(&swept));
}

#[test]
fn the_outline_is_across_the_path_x_to_the_right_y_up() {
    // Going along +X with +Y up, right is +Z.
    let path = straight(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 5);
    let road = MeshData::sweep(&sweep(&path, &rect(4.0, 0.5), &[], true)).unwrap();
    bounds_are(&road, Vec3::new(0.0, -0.25, -2.0), Vec3::new(10.0, 0.25, 2.0));
    check_winding(&road);
    check_closed(&road);
    assert!(close(volume(&road), 10.0 * 4.0 * 0.5, 1e-4));
    // The texture runs along the path in metres, and around the outline.
    let top: Vec<_> = road.vertices().iter().filter(|v| v.normal == [0.0, 1.0, 0.0]).collect();
    assert_eq!(top.iter().map(|v| v.uv[1]).fold(0.0, f32::max), 10.0);
    let widths: Vec<f32> = top.iter().map(|v| v.uv[0]).collect();
    let (lo, hi) = (widths.iter().copied().fold(f32::MAX, f32::min), widths.iter().copied().fold(0.0, f32::max));
    assert!(close(hi - lo, 4.0, 1e-5), "the top's u spans {lo}..{hi}");
    // Five points along a side: each side is a strip of 4 quads.
    assert_eq!(road.indices().len() / 3, 4 * 4 * 2 + 2 * 2);
}

#[test]
fn a_circle_swept_around_a_ring_is_a_closed_torus() {
    let (big, small) = (3.0, 0.5);
    let path = ring(big, 96);
    let torus = MeshData::sweep(&sweep(&path, &circle(small, 24), &[], false)).unwrap();
    check_winding(&torus);
    check_tangents(&torus);
    check_closed(&torus);
    let wanted = 2.0 * PI * PI * big * small * small;
    assert!(close(volume(&torus), wanted, 0.02), "volume {} wanted {wanted}", volume(&torus));
    // Smooth around the tube: each corner's normal points away from the
    // tube's middle.
    for v in torus.vertices() {
        let p = Vec3::from(v.position);
        let middle = Vec3::new(p.x, 0.0, p.z).normalize() * big;
        assert!(Vec3::from(v.normal).dot((p - middle).normalize()) > 0.95);
    }
}

#[test]
fn a_pipe_round_a_bend_is_closed_and_hollow() {
    // A quarter of a ring, its ends capped, with a hole all the way.
    let path: Vec<PathPoint> = ring(4.0, 64).into_iter().take(17).collect();
    let (outer, inner) = (circle(0.5, 16), circle(0.4, 16));
    let pipe = MeshData::sweep(&sweep(&path, &outer, &[&inner], true)).unwrap();
    check_winding(&pipe);
    check_closed(&pipe);
    let area = |r: f32| 16.0 / 2.0 * r * r * (TAU / 16.0).sin();
    let wanted = (area(0.5) - area(0.4)) * TAU * 4.0 / 4.0;
    assert!(close(volume(&pipe), wanted, 0.02), "volume {} wanted {wanted}", volume(&pipe));
}

#[test]
fn a_square_outline_stays_sharp_and_up_is_kept_across_the_path() {
    // Up tilted towards the path is made square to it.
    let mut path = straight(Vec3::ZERO, Vec3::new(0.0, 0.0, -4.0), 3);
    for p in &mut path {
        p.up = Vec3::new(0.0, 1.0, -0.5);
    }
    let beam = MeshData::sweep(&sweep(&path, &rect(1.0, 1.0), &[], false)).unwrap();
    let normals: std::collections::HashSet<[i64; 3]> =
        beam.vertices().iter().map(|v| v.normal.map(|c| (c * 1e4).round() as i64)).collect();
    assert_eq!(normals.len(), 4, "four flat sides: {normals:?}");
    bounds_are(&beam, Vec3::new(-0.5, -0.5, -4.0), Vec3::new(0.5, 0.5, 0.0));
}

#[test]
fn a_sweep_into_a_mesh_keeps_its_triangles_when_theyre_the_same() {
    let outline = circle(0.1, 8);
    let mut rope = MeshData::sweep(&sweep(&straight(Vec3::ZERO, Vec3::X, 10), &outline, &[], true)).unwrap();
    rope.mark_dynamic();
    let indices = rope.indices().as_ptr();
    // The rope sags: the same number of points, moved.
    let sagged: Vec<PathPoint> = straight(Vec3::ZERO, Vec3::X, 10)
        .into_iter()
        .map(|mut p| {
            p.position.y -= (p.position.x * PI).sin() * 0.3;
            p
        })
        .collect();
    rope.sweep_into(&sweep(&sagged, &outline, &[], true)).unwrap();
    assert_eq!(rope.indices().as_ptr(), indices, "the triangles are kept");
    assert!(rope.is_dynamic());
    assert!(rope.bounds().0.y < -0.35);
    check_winding(&rope);
    check_closed(&rope);
    // More points: new triangles.
    rope.sweep_into(&sweep(&straight(Vec3::ZERO, Vec3::X, 20), &outline, &[], true)).unwrap();
    assert_eq!(rope.indices().len() / 3, 8 * 19 * 2 + 2 * 6);
    check_closed(&rope);
}

#[test]
fn a_broken_sweep_is_refused() {
    let outline = rect(1.0, 1.0);
    let one = straight(Vec3::ZERO, Vec3::X, 2)[..1].to_vec();
    assert!(matches!(MeshData::sweep(&sweep(&one, &outline, &[], true)), Err(MeshError::Outline(_))));
    let mut still = straight(Vec3::ZERO, Vec3::X, 2);
    still[1].forward = Vec3::ZERO;
    assert_eq!(
        MeshData::sweep(&sweep(&still, &outline, &[], true)),
        Err(MeshError::Outline("path point 1 has no direction: its forward is 0".into()))
    );
    let mut lost = straight(Vec3::ZERO, Vec3::X, 2);
    lost[0].position.x = f32::NAN;
    assert_eq!(
        MeshData::sweep(&sweep(&lost, &outline, &[], true)),
        Err(MeshError::NotFinite { what: "path point", at: 0 })
    );
    let line = [Vec2::ZERO, Vec2::X];
    assert!(matches!(
        MeshData::sweep(&sweep(&straight(Vec3::ZERO, Vec3::X, 2), &line, &[], false)),
        Err(MeshError::Outline(_))
    ));
    let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let (joints, weights) = ([[0u16; 4]; 3], [[1.0, 0.0, 0.0, 0.0]; 3]);
    let mut skinned = MeshData::new(MeshParts {
        positions: &positions,
        indices: &[0, 1, 2],
        skin: Some((&joints, &weights)),
        ..MeshParts::default()
    })
    .unwrap();
    assert_eq!(
        skinned.sweep_into(&sweep(&straight(Vec3::ZERO, Vec3::X, 2), &outline, &[], true)),
        Err(MeshError::Skinned)
    );
}

/// What sweeping a road again takes, held to a bar (stage 14's GT1):
/// `cargo test --release -p fynkel_mesh -- --ignored bench_`.
#[test]
#[ignore = "a benchmark: run in release"]
fn bench_sweeping_a_road_again() {
    // A 100 m road, a point every 0.5 m, its outline 12 points.
    let path: Vec<PathPoint> = (0..=200)
        .map(|i| {
            let d = i as f32 * 0.5;
            let a = d / 30.0;
            PathPoint {
                position: Vec3::new(a.sin() * 30.0, (d / 10.0).sin(), -a.cos() * 30.0 + 30.0),
                forward: Vec3::new(a.cos(), 0.0, a.sin()),
                up: Vec3::Y,
                distance: d,
            }
        })
        .collect();
    let profile = circle(3.0, 12);
    let mut mesh = MeshData::sweep(&sweep(&path, &profile, &[], true)).unwrap();
    fynkel_bench::bench("a 100 m road of 201 points, a 12-point outline, swept again", 200, 0.2, || {
        mesh.sweep_into(&sweep(&path, &profile, &[], true)).unwrap();
    });
}

// Stage 14's WO14: where the path went straight up its up, the right was
// any line at right angles, turned 180° from the level part before it, and
// the outline was swept inside out there. Now the right is carried along.
#[test]
fn a_path_going_straight_up_keeps_its_right_where_it_was() {
    let point = |position: Vec3, forward: Vec3, distance: f32| PathPoint { position, forward, up: Vec3::Y, distance };
    let path =
        [point(Vec3::ZERO, Vec3::X, 0.0), point(Vec3::X, Vec3::Y, 1.0), point(Vec3::new(1.0, 1.0, 0.0), Vec3::Y, 2.0)];
    let outline = [Vec2::new(1.0, 0.0), Vec2::new(-0.5, 0.5), Vec2::new(-0.5, -0.5)];
    let mesh = MeshData::sweep(&sweep(&path, &outline, &[], false)).unwrap();
    let near = |at: Vec3| mesh.vertices().iter().any(|v| Vec3::from(v.position).distance(at) < 1e-5);
    // Level along +X, the outline's right is +Z.
    assert!(near(Vec3::new(0.0, 0.0, 1.0)));
    // Going up, it stays +Z.
    assert!(near(Vec3::new(1.0, 0.0, 1.0)), "turned at the first upward point");
    assert!(near(Vec3::new(1.0, 1.0, 1.0)), "turned at the second upward point");
    // And going up from the start, it takes the right of the first level point.
    let up_first = [point(Vec3::ZERO, Vec3::Y, 0.0), point(Vec3::Y, Vec3::X, 1.0)];
    let mesh = MeshData::sweep(&sweep(&up_first, &outline, &[], false)).unwrap();
    assert!(mesh.vertices().iter().any(|v| Vec3::from(v.position).distance(Vec3::new(0.0, 0.0, 1.0)) < 1e-5));
}
