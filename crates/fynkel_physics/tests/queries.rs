//! Every query against each shape, with layers, triggers and scale, and
//! colliders found where they were moved, with no step between
//! (docs/stage-5.md §3.6, step 3).

use fynkel_physics::*;

const NEAR: f32 = 1e-3;

fn spec(shape: ColliderShape) -> ColliderSpec {
    ColliderSpec { shape, center: Vector::ZERO, is_trigger: false, layer: 0, material: Material::DEFAULT }
}

fn at(x: f32, y: f32, z: f32) -> ObjectPose {
    ObjectPose { position: Vector::new(x, y, z), ..ObjectPose::IDENTITY }
}

fn cube(size: f32) -> ColliderSpec {
    spec(ColliderShape::Box { size: Vector::splat(size) })
}

fn ball(radius: f32) -> ColliderSpec {
    spec(ColliderShape::Sphere { radius })
}

fn pill(radius: f32, height: f32) -> ColliderSpec {
    spec(ColliderShape::Capsule { radius, height, direction: Axis::Y })
}

const ALL: QueryOptions = QueryOptions { layers: u32::MAX, hit_triggers: true };

/// A box (key 1) at x = 0, a sphere (2) at x = 5 and a capsule (3) at
/// x = -5, each with its near side at z = 9.
fn three() -> Physics {
    let mut physics = Physics::new();
    physics.add_static_collider(1, cube(2.0), at(0.0, 0.0, 10.0)).unwrap();
    physics.add_static_collider(2, ball(1.0), at(5.0, 0.0, 10.0)).unwrap();
    physics.add_static_collider(3, pill(1.0, 4.0), at(-5.0, 0.0, 10.0)).unwrap();
    physics
}

fn ray(physics: &mut Physics, x: f32, options: QueryOptions) -> Option<Hit> {
    physics.raycast(Vector::new(x, 0.0, 0.0), Vector::Z, f32::INFINITY, options).unwrap()
}

#[track_caller]
fn near(a: Vector, b: Vector) {
    assert!(a.abs_diff_eq(b, NEAR), "{a} is not {b}");
}

#[test]
fn a_ray_hits_each_shape_where_its_surface_is() {
    let mut physics = three();
    for (x, key) in [(0.0, 1), (5.0, 2), (-5.0, 3)] {
        let hit = ray(&mut physics, x, ALL).expect("a hit");
        assert_eq!(hit.key, key);
        assert!((hit.distance - 9.0).abs() < NEAR, "{hit:?}");
        near(hit.point, Vector::new(x, 0.0, 9.0));
        near(hit.normal, -Vector::Z);
    }
    assert_eq!(ray(&mut physics, 2.5, ALL), None, "between them");
    // A direction needn't be unit length; none at all finds nothing.
    let hit = physics.raycast(Vector::ZERO, Vector::new(0.0, 0.0, 7.0), 100.0, ALL).unwrap().unwrap();
    assert!((hit.distance - 9.0).abs() < NEAR);
    assert_eq!(physics.raycast(Vector::ZERO, Vector::ZERO, 100.0, ALL).unwrap(), None);
    // Short of it.
    assert_eq!(physics.raycast(Vector::ZERO, Vector::Z, 8.9, ALL).unwrap(), None);
}

#[test]
fn a_ray_from_inside_a_collider_doesnt_see_it_and_finds_the_next() {
    let mut physics = three();
    physics.add_static_collider(9, cube(1.0), at(0.0, 0.0, 0.0)).unwrap();
    assert_eq!(ray(&mut physics, 0.0, ALL).unwrap().key, 1);
    let mut all = Vec::new();
    physics.raycast_all(Vector::ZERO, Vector::Z, f32::INFINITY, ALL, &mut all).unwrap();
    assert_eq!(all.iter().map(|h| h.key).collect::<Vec<_>>(), [1]);
    // From outside, it's the first.
    let hit = physics.raycast(Vector::new(0.0, 0.0, -3.0), Vector::Z, f32::INFINITY, ALL).unwrap().unwrap();
    assert_eq!(hit.key, 9);
    assert!((hit.distance - 2.5).abs() < NEAR);
}

#[test]
fn layers_and_triggers_decide_what_a_query_sees() {
    let mut physics = Physics::new();
    let front = ColliderSpec { layer: 3, ..cube(2.0) };
    let middle = ColliderSpec { is_trigger: true, ..cube(2.0) };
    physics.add_static_collider(1, front, at(0.0, 0.0, 5.0)).unwrap();
    physics.add_static_collider(2, middle, at(0.0, 0.0, 10.0)).unwrap();
    physics.add_static_collider(3, cube(2.0), at(0.0, 0.0, 15.0)).unwrap();
    let keys = |physics: &mut Physics, options: QueryOptions| ray(physics, 0.0, options).map(|h| h.key);
    assert_eq!(keys(&mut physics, ALL), Some(1));
    assert_eq!(keys(&mut physics, QueryOptions { layers: !(1 << 3), hit_triggers: true }), Some(2));
    assert_eq!(keys(&mut physics, QueryOptions { layers: !(1 << 3), hit_triggers: false }), Some(3));
    assert_eq!(keys(&mut physics, QueryOptions { layers: 1 << 3, hit_triggers: false }), Some(1));
    assert_eq!(keys(&mut physics, QueryOptions { layers: 0, hit_triggers: true }), None);

    let sphere = QueryShape::Sphere { center: Vector::new(0.0, 0.0, 10.0), radius: 0.5 };
    assert!(physics.check(sphere, ALL).unwrap());
    assert!(!physics.check(sphere, QueryOptions { layers: u32::MAX, hit_triggers: false }).unwrap());
    let mut all = Vec::new();
    physics
        .raycast_all(Vector::ZERO, Vector::Z, 100.0, QueryOptions { layers: 1, hit_triggers: false }, &mut all)
        .unwrap();
    assert_eq!(all.iter().map(|h| h.key).collect::<Vec<_>>(), [3]);
}

#[test]
fn colliders_are_scaled_by_their_object() {
    let mut physics = Physics::new();
    let scaled = ObjectPose { scale: Vector::new(1.0, 1.0, 3.0), ..at(0.0, 0.0, 10.0) };
    physics.add_static_collider(1, cube(2.0), scaled).unwrap();
    physics.add_static_collider(2, ball(1.0), ObjectPose { scale: Vector::splat(2.0), ..at(5.0, 0.0, 10.0) }).unwrap();
    assert!((ray(&mut physics, 0.0, ALL).unwrap().distance - 7.0).abs() < NEAR, "3 deep: from z = 7");
    assert!((ray(&mut physics, 5.0, ALL).unwrap().distance - 8.0).abs() < NEAR, "radius 2");
    // A center is in the object's space: scaled and turned with it.
    let turned = ObjectPose {
        rotation: Rotation::from_rotation_y(std::f32::consts::FRAC_PI_2),
        scale: Vector::splat(2.0),
        ..at(-5.0, 0.0, 10.0)
    };
    // Its center 1 along X, doubled and turned: 2 along -Z, so the ball,
    // radius 2, starts at z = 6.
    physics.add_static_collider(3, ColliderSpec { center: Vector::X, ..ball(1.0) }, turned).unwrap();
    assert!((ray(&mut physics, -5.0, ALL).unwrap().distance - 6.0).abs() < NEAR);
}

#[test]
fn a_moved_collider_is_found_where_it_went_with_no_step_between() {
    let mut physics = three();
    let id = ray(&mut physics, 0.0, ALL).unwrap().collider;
    physics.follow(&[id], &[at(0.0, 0.0, 20.0)]).unwrap();
    assert!((ray(&mut physics, 0.0, ALL).unwrap().distance - 19.0).abs() < NEAR);
    physics.follow(&[id], &[at(0.0, 50.0, 20.0)]).unwrap();
    assert_eq!(ray(&mut physics, 0.0, ALL), None);
    // Grown in place: the spec changes, and a query sees it at once.
    physics.follow(&[id], &[at(0.0, 0.0, 10.0)]).unwrap();
    physics.set_collider_spec(id, cube(6.0)).unwrap();
    assert!((ray(&mut physics, 0.0, ALL).unwrap().distance - 7.0).abs() < NEAR);
    // Removed, and another added in its slot elsewhere: the old place is empty.
    physics.remove_collider(id).unwrap();
    assert_eq!(ray(&mut physics, 0.0, ALL), None);
    physics.add_static_collider(7, cube(2.0), at(0.0, 0.0, 30.0)).unwrap();
    let hit = ray(&mut physics, 0.0, ALL).unwrap();
    assert_eq!((hit.key, (hit.distance - 29.0).abs() < NEAR), (7, true));
    // And so after a step.
    physics.step(1.0 / 50.0).unwrap();
    assert_eq!(ray(&mut physics, 0.0, ALL).unwrap().key, 7);
}

#[test]
fn raycast_all_gives_every_hit_nearest_first() {
    let mut physics = Physics::new();
    for (key, z) in [(1, 30.0), (2, 10.0), (3, 20.0)] {
        physics.add_static_collider(key, cube(2.0), at(0.0, 0.0, z)).unwrap();
    }
    let mut all = Vec::new();
    physics.raycast_all(Vector::ZERO, Vector::Z, 25.0, ALL, &mut all).unwrap();
    assert_eq!(all.iter().map(|h| h.key).collect::<Vec<_>>(), [2, 3]);
    assert!((all[1].distance - 19.0).abs() < NEAR);
    physics.raycast_all(Vector::ZERO, -Vector::Z, 25.0, ALL, &mut all).unwrap();
    assert!(all.is_empty());
}

#[test]
fn each_shape_cast_hits_each_shape() {
    let shapes = [
        QueryShape::Sphere { center: Vector::ZERO, radius: 0.5 },
        QueryShape::Box { center: Vector::ZERO, half_extents: Vector::splat(0.5), rotation: Rotation::IDENTITY },
        QueryShape::Capsule { a: Vector::new(0.0, -0.5, 0.0), b: Vector::new(0.0, 0.5, 0.0), radius: 0.5 },
    ];
    let mut physics = three();
    for shape in shapes {
        for (x, key) in [(0.0, 1), (5.0, 2), (-5.0, 3)] {
            let moved = shift(shape, Vector::new(x, 0.0, 0.0));
            let hit =
                physics.cast(moved, Vector::Z, f32::INFINITY, ALL).unwrap().unwrap_or_else(|| panic!("{moved:?}"));
            assert_eq!(hit.key, key, "{moved:?}");
            // Its front, 0.5 ahead, meets the near side at z = 9.
            assert!((hit.distance - 8.5).abs() < 2e-3, "{moved:?}: {hit:?}");
            assert!((hit.point.z - 9.0).abs() < 2e-3, "{moved:?}: {hit:?}");
            near(hit.normal, -Vector::Z);
        }
    }
}

fn shift(shape: QueryShape, by: Vector) -> QueryShape {
    match shape {
        QueryShape::Sphere { center, radius } => QueryShape::Sphere { center: center + by, radius },
        QueryShape::Box { center, half_extents, rotation } => {
            QueryShape::Box { center: center + by, half_extents, rotation }
        }
        QueryShape::Capsule { a, b, radius } => QueryShape::Capsule { a: a + by, b: b + by, radius },
    }
}

#[test]
fn a_cast_skips_what_it_starts_in_and_a_cast_all_reports_it_at_0() {
    let mut physics = three();
    physics.add_static_collider(9, cube(2.0), at(0.0, 0.0, 0.0)).unwrap();
    let sphere = QueryShape::Sphere { center: Vector::ZERO, radius: 0.5 };
    assert_eq!(physics.cast(sphere, Vector::Z, f32::INFINITY, ALL).unwrap().unwrap().key, 1);
    let mut all = Vec::new();
    physics.cast_all(sphere, Vector::Z, f32::INFINITY, ALL, &mut all).unwrap();
    assert_eq!(all.iter().map(|h| h.key).collect::<Vec<_>>(), [9, 1]);
    assert_eq!((all[0].distance, all[0].point, all[0].normal), (0.0, Vector::ZERO, -Vector::Z));
    assert!((all[1].distance - 8.5).abs() < 2e-3);
    // A wide sphere sweeps all three.
    let wide = QueryShape::Sphere { center: Vector::new(0.0, 0.0, -5.0), radius: 6.0 };
    physics.cast_all(wide, Vector::Z, 20.0, QueryOptions { layers: u32::MAX, hit_triggers: false }, &mut all).unwrap();
    let mut keys: Vec<_> = all.iter().map(|h| h.key).collect();
    assert_eq!(keys[0], 9);
    keys.sort_unstable();
    assert_eq!(keys, [1, 2, 3, 9]);
}

#[test]
fn overlaps_and_checks_find_what_touches_or_holds_the_shape() {
    let mut physics = three();
    let mut found = Vec::new();
    let keys = |found: &Vec<(u64, u64)>| {
        let mut keys: Vec<_> = found.iter().map(|&(_, key)| key).collect();
        keys.sort_unstable();
        keys
    };
    // Reaches the box (its side at x = 1) but not the sphere (x = 4).
    physics.overlap(QueryShape::Sphere { center: Vector::new(2.0, 0.0, 10.0), radius: 1.5 }, ALL, &mut found).unwrap();
    assert_eq!(keys(&found), [1]);
    physics.overlap(QueryShape::Sphere { center: Vector::new(2.0, 0.0, 10.0), radius: 0.9 }, ALL, &mut found).unwrap();
    assert!(found.is_empty());
    // Inside one counts.
    physics.overlap(QueryShape::Sphere { center: Vector::new(0.0, 0.0, 10.0), radius: 0.1 }, ALL, &mut found).unwrap();
    assert_eq!(keys(&found), [1]);
    // A long box turned to lie along X reaches all three.
    let along_x = QueryShape::Box {
        center: Vector::new(0.0, 0.0, 10.0),
        half_extents: Vector::new(0.1, 0.1, 6.0),
        rotation: Rotation::from_rotation_y(std::f32::consts::FRAC_PI_2),
    };
    physics.overlap(along_x, ALL, &mut found).unwrap();
    assert_eq!(keys(&found), [1, 2, 3]);
    // A capsule from the sphere to the capsule, above the box.
    let capsule = QueryShape::Capsule { a: Vector::new(-5.0, 1.5, 10.0), b: Vector::new(5.0, 1.5, 10.0), radius: 0.4 };
    physics.overlap(capsule, ALL, &mut found).unwrap();
    assert_eq!(keys(&found), [3], "only the capsule (top at y = 2) reaches y = 1.1");
    assert!(physics.check(capsule, ALL).unwrap());
    assert!(!physics.check(shift(capsule, Vector::new(0.0, 1.0, 0.0)), ALL).unwrap());
}

#[test]
fn a_colliders_closest_point_and_its_own_raycast() {
    let mut physics = three();
    let id = ray(&mut physics, 0.0, ALL).unwrap().collider;
    near(physics.closest_point(id, Vector::new(0.0, 5.0, 10.0)).unwrap(), Vector::new(0.0, 1.0, 10.0));
    near(physics.closest_point(id, Vector::new(0.2, 0.3, 10.0)).unwrap(), Vector::new(0.2, 0.3, 10.0));
    let hit = physics.raycast_collider(id, Vector::new(0.0, 0.0, 20.0), -Vector::Z, 100.0).unwrap().unwrap();
    assert!((hit.distance - 9.0).abs() < NEAR);
    near(hit.normal, Vector::Z);
    assert_eq!(
        physics.raycast_collider(id, Vector::new(0.0, 0.0, 10.0), Vector::Z, 100.0).unwrap(),
        None,
        "from inside"
    );
    assert_eq!(physics.raycast_collider(id, Vector::new(5.0, 0.0, 0.0), Vector::Z, 100.0).unwrap(), None, "beside it");
}

#[test]
fn bad_queries_are_refused() {
    let mut physics = three();
    let bad = |r: Result<Option<Hit>, PhysicsError>| assert!(matches!(r, Err(PhysicsError::Value(_))), "{r:?}");
    bad(physics.raycast(Vector::new(f32::NAN, 0.0, 0.0), Vector::Z, 1.0, ALL));
    bad(physics.raycast(Vector::ZERO, Vector::Z, -1.0, ALL));
    bad(physics.raycast(Vector::ZERO, Vector::Z, f32::NAN, ALL));
    bad(physics.cast(QueryShape::Sphere { center: Vector::ZERO, radius: 0.0 }, Vector::Z, 1.0, ALL));
    bad(physics.cast(
        QueryShape::Box {
            center: Vector::ZERO,
            half_extents: Vector::ONE,
            rotation: Rotation::from_xyzw(0.0, 0.0, 0.0, 3.0),
        },
        Vector::Z,
        1.0,
        ALL,
    ));
    assert!(physics.check(QueryShape::Capsule { a: Vector::ZERO, b: Vector::ONE, radius: -1.0 }, ALL).is_err());
}

#[test]
fn queries_between_steps_dont_upset_the_simulation() {
    // The ground is queried (so put in the queries' tree by hand) before
    // the first step, and moved and queried again mid-fall: a ball still
    // lands on it, where it was moved to.
    let mut physics = Physics::new();
    let ground = physics
        .add_static_collider(1, cube(1.0), ObjectPose { scale: Vector::new(20.0, 1.0, 20.0), ..at(0.0, -0.5, 0.0) })
        .unwrap();
    assert!(physics.raycast(Vector::new(0.0, 5.0, 0.0), -Vector::Y, 10.0, ALL).unwrap().is_some());
    let ball = physics.add_body(BodyKind::Dynamic, 2, Pose::from_translation(Vector::new(0.0, 5.0, 0.0))).unwrap();
    physics.add_collider(Some(ball), Shape::Sphere { radius: 0.5 }, Pose::IDENTITY).unwrap();
    for i in 0..300 {
        if i == 20 {
            physics
                .follow(&[ground], &[ObjectPose { scale: Vector::new(20.0, 1.0, 20.0), ..at(0.0, -2.5, 0.0) }])
                .unwrap();
            let hit = physics.raycast(Vector::new(3.0, 5.0, 0.0), -Vector::Y, 10.0, ALL).unwrap().unwrap();
            assert!((hit.point.y + 2.0).abs() < NEAR, "{hit:?}");
        }
        physics.step(1.0 / 50.0).unwrap();
    }
    let y = physics.body_pose(ball).unwrap().translation.y;
    assert!((y + 1.5).abs() < 0.02, "rests on the moved ground at y = {y}");
}
