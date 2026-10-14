//! Mesh colliders (docs/stage-5.md, step 6): a mesh's triangles, or their
//! convex hull, scaled by the object, hit by rays and by bodies.

use fynkel_physics::*;

const STEP: f32 = 1.0 / 50.0;
const ALL: QueryOptions = QueryOptions { layers: u32::MAX, hit_triggers: true };

fn spec(mesh: u64, convex: bool) -> ColliderSpec {
    ColliderSpec {
        shape: ColliderShape::Mesh { mesh, convex },
        center: Vector::ZERO,
        is_trigger: false,
        layer: 0,
        material: Material::DEFAULT,
        listens: 0,
    }
}

fn at(x: f32, y: f32, z: f32) -> ObjectPose {
    ObjectPose { position: Vector::new(x, y, z), ..ObjectPose::IDENTITY }
}

/// A V-shaped trough along Z: its bottom on the Z axis, its sides rising
/// to y = 2 at x = ±2. Its hull is a prism with a flat top at y = 2.
fn trough(physics: &mut Physics) -> u64 {
    let points = [
        Vector::new(-2.0, 2.0, -1.0),
        Vector::new(0.0, 0.0, -1.0),
        Vector::new(2.0, 2.0, -1.0),
        Vector::new(-2.0, 2.0, 1.0),
        Vector::new(0.0, 0.0, 1.0),
        Vector::new(2.0, 2.0, 1.0),
    ];
    // Facing up, into the trough.
    let triangles = [[0, 3, 1], [1, 3, 4], [1, 4, 2], [2, 4, 5]];
    physics.add_mesh(&points, &triangles).unwrap()
}

/// How high a ray straight down at `x` hits.
fn height(physics: &mut Physics, x: f32) -> Option<f32> {
    let hit = physics.raycast(Vector::new(x, 10.0, 0.0), -Vector::Y, 20.0, ALL).unwrap()?;
    Some(hit.point.y)
}

#[track_caller]
fn near(a: Option<f32>, b: f32) {
    let a = a.expect("a hit");
    assert!((a - b).abs() < 1e-3, "{a} is not {b}");
}

#[test]
fn a_ray_hits_a_meshs_triangles_or_its_hull() {
    let mut physics = Physics::new();
    let mesh = trough(&mut physics);
    physics.add_static_collider(1, spec(mesh, false), at(0.0, 0.0, 0.0)).unwrap();
    physics.add_static_collider(2, spec(mesh, true), at(10.0, 0.0, 0.0)).unwrap();
    near(height(&mut physics, 0.0), 0.0);
    near(height(&mut physics, 1.0), 1.0);
    near(height(&mut physics, -1.5), 1.5);
    assert_eq!(height(&mut physics, 2.5), None);
    near(height(&mut physics, 10.0), 2.0);
    near(height(&mut physics, 11.0), 2.0);
}

#[test]
fn a_mesh_is_scaled_on_each_axis_by_its_object() {
    let mut physics = Physics::new();
    let mesh = trough(&mut physics);
    let squashed = ObjectPose { scale: Vector::new(2.0, 0.5, 1.0), ..ObjectPose::IDENTITY };
    let id = physics.add_static_collider(1, spec(mesh, false), squashed).unwrap();
    near(height(&mut physics, 2.0), 0.5);
    near(height(&mut physics, 4.0), 1.0);
    // Rescaled, it's the trough again; mirrored, too.
    physics.follow(&[id], &[ObjectPose { scale: Vector::new(-1.0, 1.0, 1.0), ..ObjectPose::IDENTITY }]).unwrap();
    near(height(&mut physics, 1.0), 1.0);
    assert_eq!(height(&mut physics, 3.0), None);
}

#[test]
fn a_ball_rolls_down_into_the_bottom_of_a_trough() {
    let mut physics = Physics::new();
    let mesh = trough(&mut physics);
    physics.add_static_collider(1, spec(mesh, false), ObjectPose::IDENTITY).unwrap();
    let start = Pose::from_translation(Vector::new(1.2, 3.0, 0.0));
    let body = physics.add_rigidbody(2, BodySpec::default(), start).unwrap();
    let ball = ColliderSpec { shape: ColliderShape::Sphere { radius: 0.25 }, ..spec(mesh, false) };
    physics.add_object_collider(2, ball, at(1.2, 3.0, 0.0), Some(body)).unwrap();
    for _ in 0..400 {
        physics.step(STEP).unwrap();
    }
    let (pose, ..) = physics.body_state(body).unwrap();
    // It rests against both sides, 0.25·√2 above the bottom.
    assert!(pose.translation.x.abs() < 0.05, "{pose:?}");
    assert!((pose.translation.y - 0.25 * 2f32.sqrt()).abs() < 0.02, "{pose:?}");
}

#[test]
fn a_convex_mesh_body_falls_and_lands_on_its_hull() {
    let mut physics = Physics::new();
    let mesh = trough(&mut physics);
    let floor = ColliderSpec { shape: ColliderShape::Box { size: Vector::new(20.0, 1.0, 20.0) }, ..spec(mesh, false) };
    physics.add_static_collider(1, floor, at(0.0, -0.5, 0.0)).unwrap();
    // Upside down, the trough's hull lands on its flat top.
    let turned = Rotation::from_rotation_z(std::f32::consts::PI);
    let pose = Pose::from_parts(Vector::new(0.0, 5.0, 0.0), turned);
    let body = physics.add_rigidbody(2, BodySpec::default(), pose).unwrap();
    let object = ObjectPose { rotation: turned, ..at(0.0, 5.0, 0.0) };
    physics.add_object_collider(2, spec(mesh, true), object, Some(body)).unwrap();
    for _ in 0..200 {
        physics.step(STEP).unwrap();
    }
    let (pose, ..) = physics.body_state(body).unwrap();
    assert!((pose.translation.y - 2.0).abs() < 0.02, "{pose:?}");
}

#[test]
fn a_non_convex_mesh_on_a_dynamic_body_is_its_hull_until_it_turns_kinematic() {
    let mut physics = Physics::new();
    let mesh = trough(&mut physics);
    let spec_of_body = BodySpec { use_gravity: false, ..BodySpec::default() };
    let body = physics.add_rigidbody(1, spec_of_body, Pose::IDENTITY).unwrap();
    physics.add_object_collider(1, spec(mesh, false), ObjectPose::IDENTITY, Some(body)).unwrap();
    near(height(&mut physics, 0.0), 2.0);
    physics.set_body_spec(body, BodySpec { kinematic: true, ..spec_of_body }).unwrap();
    near(height(&mut physics, 0.0), 0.0);
    physics.set_body_spec(body, spec_of_body).unwrap();
    near(height(&mut physics, 0.0), 2.0);
}

#[test]
fn a_flat_mesh_is_hit_as_triangles_or_as_hull_but_a_line_has_no_hull() {
    let mut physics = Physics::new();
    let corners = [Vector::new(-1.0, 0.0, -1.0), Vector::new(1.0, 0.0, -1.0), Vector::new(1.0, 0.0, 1.0)];
    let plane = physics.add_mesh(&corners, &[[0, 2, 1]]).unwrap();
    physics.add_static_collider(1, spec(plane, false), ObjectPose::IDENTITY).unwrap();
    physics.add_static_collider(2, spec(plane, true), at(10.0, 0.0, 0.0)).unwrap();
    near(height(&mut physics, 0.5), 0.0);
    near(height(&mut physics, 10.5), 0.0);
    let line = physics.add_mesh(&[Vector::ZERO, Vector::X, Vector::X * 2.0], &[[0, 1, 2]]).unwrap();
    let error = physics.add_static_collider(3, spec(line, true), ObjectPose::IDENTITY).unwrap_err();
    assert!(error.to_string().contains("on a line"), "{error}");
}

#[test]
fn bad_meshes_and_meshes_in_use_are_refused() {
    let mut physics = Physics::new();
    let one = [Vector::ZERO, Vector::X, Vector::Y];
    for (points, triangles) in [
        (&one[..], &[][..]),
        (&one[..], &[[0, 1, 3]][..]),
        (&[Vector::ZERO, Vector::X, Vector::NAN][..], &[[0, 1, 2]][..]),
    ] {
        assert!(physics.add_mesh(points, triangles).is_err());
    }
    assert!(physics.add_static_collider(1, spec(99, false), ObjectPose::IDENTITY).is_err());
    let mesh = trough(&mut physics);
    let id = physics.add_static_collider(1, spec(mesh, false), ObjectPose::IDENTITY).unwrap();
    assert!(physics.remove_mesh(mesh).unwrap_err().to_string().contains("used"));
    physics.remove_collider(id).unwrap();
    physics.remove_mesh(mesh).unwrap();
    assert_eq!(physics.mesh_count(), 0);
    assert!(physics.remove_mesh(mesh).is_err());
    let again = trough(&mut physics);
    assert_ne!(again, mesh, "ids aren't reused");
}

#[test]
fn a_mesh_colliders_edges_are_read_where_it_is_each_once() {
    let mut physics = Physics::new();
    let mesh = trough(&mut physics);
    let triangles = physics.add_static_collider(1, spec(mesh, false), at(0.0, 1.0, 0.0)).unwrap();
    let hull = physics.add_static_collider(2, spec(mesh, true), ObjectPose::IDENTITY).unwrap();
    let mut edges = vec![];
    physics.collider_edges(triangles, &mut edges).unwrap();
    // Four triangles in two strips: 9 edges, raised by its object.
    assert_eq!(edges.len(), 9);
    assert!(edges.iter().flatten().all(|p| p.y >= 1.0 && p.y <= 3.0));
    physics.collider_edges(hull, &mut edges).unwrap();
    // A prism: three edges at each end and three along it, and the
    // diagonals of the sides the hull splits.
    assert!((9..=12).contains(&edges.len()), "{}", edges.len());
    let ball = ColliderSpec { shape: ColliderShape::Sphere { radius: 1.0 }, ..spec(mesh, false) };
    let ball = physics.add_static_collider(3, ball, ObjectPose::IDENTITY).unwrap();
    assert!(physics.collider_edges(ball, &mut edges).is_err());
}

#[test]
fn colliders_of_one_mesh_at_one_scale_share_one_shape() {
    let mut physics = Physics::new();
    let mesh = trough(&mut physics);
    let twice =
        |x: f32| ObjectPose { position: Vector::new(x, 0.0, 0.0), scale: Vector::splat(2.0), ..ObjectPose::IDENTITY };
    let before = physics.mesh_shapes_made();
    let ids: Vec<u64> =
        (0..100).map(|i| physics.add_static_collider(i, spec(mesh, false), twice(i as f32 * 10.0)).unwrap()).collect();
    assert_eq!(physics.mesh_shapes_made() - before, 2, "the mesh's own, and one for a hundred colliders at 2");
    for (i, &id) in ids.iter().enumerate() {
        let back = ObjectPose { scale: Vector::splat(3.0), ..twice(i as f32 * 10.0) };
        physics.follow(&[id], &[back]).unwrap();
    }
    assert_eq!(physics.mesh_shapes_made() - before, 3, "and one more for all of them at a new scale");
    // Still the trough, at its new scale: its sides reach y = 6 at x = ±6.
    let y = height(&mut physics, 5.0).expect("a hit");
    assert!((y - 5.0).abs() < 1e-3, "{y}");
}
