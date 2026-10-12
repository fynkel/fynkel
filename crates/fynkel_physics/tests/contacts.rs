//! Who touches whom (docs/stage-5.md §3.4, §3.5): Unity's trigger and
//! collision table, row by row, enter, stay and exit in every case an exit
//! is owed, the layer matrix and ignored pairs.

use fynkel_physics::{
    BodySpec, ColliderShape, ColliderSpec, ContactEvent, ContactKind, LISTEN_COLLISIONS, LISTEN_TRIGGERS, Material,
    ObjectPose, Physics, Pose, Vector,
};

const STEP: f32 = 1.0 / 50.0;
const BOTH: u32 = LISTEN_TRIGGERS | LISTEN_COLLISIONS;

fn spec(is_trigger: bool, listens: u32) -> ColliderSpec {
    ColliderSpec {
        shape: ColliderShape::Box { size: Vector::ONE },
        center: Vector::ZERO,
        is_trigger,
        layer: 0,
        material: Material::DEFAULT,
        listens,
    }
}

fn at(position: Vector) -> ObjectPose {
    ObjectPose { position, ..ObjectPose::IDENTITY }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Static,
    Rigidbody,
    Kinematic,
}

/// A cube with `key` at `position`: static, or on a body of its own
/// (gravity off, so pairs stay where they're put).
fn cube(physics: &mut Physics, key: u64, kind: Kind, trigger: bool, position: Vector) -> (u64, Option<u64>) {
    let body = match kind {
        Kind::Static => None,
        Kind::Rigidbody | Kind::Kinematic => Some(
            physics
                .add_rigidbody(
                    key,
                    BodySpec { use_gravity: false, kinematic: kind == Kind::Kinematic, ..BodySpec::default() },
                    Pose::from_translation(position),
                )
                .unwrap(),
        ),
    };
    let collider = physics.add_object_collider(key, spec(trigger, BOTH), at(position), body).unwrap();
    (collider, body)
}

/// The last step's events as (kind, trigger, a, b).
fn events(physics: &Physics) -> Vec<(ContactKind, bool, u64, u64)> {
    physics.contact_events().iter().map(|e| (e.kind, e.trigger, e.a, e.b)).collect()
}

fn only(physics: &Physics) -> ContactEvent {
    let all = physics.contact_events();
    assert_eq!(all.len(), 1, "{all:?}");
    all[0]
}

#[test]
fn every_row_of_unitys_table_sends_what_unity_sends() {
    use Kind::*;
    let kinds =
        [(Static, false), (Rigidbody, false), (Kinematic, false), (Static, true), (Rigidbody, true), (Kinematic, true)];
    let name = |(kind, trigger): (Kind, bool)| format!("{kind:?}{}", if trigger { " trigger" } else { "" });
    for (i, &first) in kinds.iter().enumerate() {
        for &second in &kinds[i..] {
            let either_trigger = first.1 || second.1;
            let moving = |(kind, _): (Kind, bool)| kind != Static;
            let dynamic = |(kind, _): (Kind, bool)| kind == Rigidbody;
            // Unity's collision action matrix.
            let expected = if either_trigger {
                (moving(first) || moving(second)).then_some(true)
            } else {
                (dynamic(first) || dynamic(second)).then_some(false)
            };
            let mut physics = Physics::new();
            cube(&mut physics, 1, first.0, first.1, Vector::ZERO);
            cube(&mut physics, 2, second.0, second.1, Vector::new(0.8, 0.0, 0.0));
            physics.step(STEP).unwrap();
            let got: Vec<_> = events(&physics);
            let want: Vec<_> = expected.map(|trigger| (ContactKind::Enter, trigger, 1, 2)).into_iter().collect();
            assert_eq!(got, want, "{} and {}", name(first), name(second));
        }
    }
}

#[test]
fn a_pair_enters_once_stays_each_step_and_exits_when_apart() {
    let mut physics = Physics::new();
    cube(&mut physics, 1, Kind::Static, true, Vector::ZERO);
    let (_, body) = cube(&mut physics, 2, Kind::Rigidbody, false, Vector::new(0.5, 0.0, 0.0));
    let body = body.unwrap();
    physics.set_velocities(body, Vector::new(0.0, 0.0, 0.0), Vector::ZERO).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Enter, true, 1, 2)]);
    for _ in 0..3 {
        physics.step(STEP).unwrap();
        assert_eq!(events(&physics), [(ContactKind::Stay, true, 1, 2)]);
    }
    physics.set_body_pose(body, Pose::from_translation(Vector::new(5.0, 0.0, 0.0))).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Exit, true, 1, 2)]);
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), []);
}

#[test]
fn enters_come_before_stays_before_exits_each_in_the_order_of_their_keys() {
    let mut physics = Physics::new();
    // Triggers 10 and 11 (static) and bodies 20, 21, 22 in them.
    cube(&mut physics, 11, Kind::Static, true, Vector::ZERO);
    cube(&mut physics, 10, Kind::Static, true, Vector::new(10.0, 0.0, 0.0));
    let (_, b22) = cube(&mut physics, 22, Kind::Rigidbody, false, Vector::new(0.2, 0.0, 0.0));
    let (_, b20) = cube(&mut physics, 20, Kind::Rigidbody, false, Vector::new(10.2, 0.0, 0.0));
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Enter, true, 10, 20), (ContactKind::Enter, true, 11, 22)]);
    // 20 leaves 10 for 11, into 22 too, and 21 arrives in 10.
    physics.set_body_pose(b20.unwrap(), Pose::from_translation(Vector::new(-0.2, 0.0, 0.0))).unwrap();
    cube(&mut physics, 21, Kind::Rigidbody, false, Vector::new(10.0, 0.0, 0.0));
    let _ = b22;
    physics.step(STEP).unwrap();
    assert_eq!(
        events(&physics),
        [
            (ContactKind::Enter, true, 10, 21),
            (ContactKind::Enter, true, 11, 20),
            (ContactKind::Enter, false, 20, 22),
            (ContactKind::Stay, true, 11, 22),
            (ContactKind::Exit, true, 10, 20),
        ]
    );
}

#[test]
fn nothing_is_reported_for_colliders_that_dont_listen_or_listen_for_the_other_kind() {
    let mut physics = Physics::new();
    let mut s = spec(false, 0);
    let falling = physics.add_rigidbody(1, BodySpec::default(), Pose::from_translation(Vector::Y * 0.4)).unwrap();
    physics.add_object_collider(1, s, at(Vector::Y * 0.4), Some(falling)).unwrap();
    physics.add_object_collider(2, spec(false, 0), at(Vector::new(0.0, -0.5, 0.0)), None).unwrap();
    s.listens = LISTEN_TRIGGERS;
    physics.add_object_collider(3, s, at(Vector::new(0.0, -0.5, 0.0)), None).unwrap();
    for _ in 0..5 {
        physics.step(STEP).unwrap();
        assert_eq!(events(&physics), []);
    }
}

#[test]
fn a_collision_has_its_points_normal_impulse_and_speed_as_each_side_sees_it() {
    let mut physics = Physics::new();
    // The floor (key 1) and a ball (key 2) falling onto it at 4 m/s.
    let floor = ColliderSpec { shape: ColliderShape::Box { size: Vector::new(20.0, 1.0, 20.0) }, ..spec(false, 0) };
    physics.add_object_collider(1, floor, at(Vector::new(0.0, -0.5, 0.0)), None).unwrap();
    let ball = physics.add_rigidbody(2, BodySpec::default(), Pose::from_translation(Vector::Y * 0.55)).unwrap();
    let round = ColliderSpec { shape: ColliderShape::Sphere { radius: 0.5 }, ..spec(false, LISTEN_COLLISIONS) };
    physics.add_object_collider(2, round, at(Vector::Y * 0.55), Some(ball)).unwrap();
    physics.set_velocities(ball, Vector::new(0.0, -4.0, 0.0), Vector::ZERO).unwrap();
    // It's 5 cm up: the contact starts within a step or two.
    for _ in 0..3 {
        physics.step(STEP).unwrap();
        if !physics.contact_events().is_empty() {
            break;
        }
    }
    let hit = only(&physics);
    assert_eq!((hit.kind, hit.trigger, hit.a, hit.b), (ContactKind::Enter, false, 1, 2));
    // As the floor (a) sees it: the ball came down at 4 m/s, gravity's
    // step on top, the floor was pushed down, and its normal points down,
    // from the ball towards it.
    assert!((hit.relative_velocity.y - -4.0).abs() < 0.3, "{}", hit.relative_velocity);
    assert!(hit.impulse.y < -0.5, "{}", hit.impulse);
    assert!(hit.point_count >= 1);
    let points = &physics.contact_points()[hit.first_point as usize..][..hit.point_count as usize];
    for p in points {
        assert!(p.normal.abs_diff_eq(Vector::NEG_Y, 1e-3), "{}", p.normal);
        assert!(p.point.y.abs() < 0.1 && p.point.x.abs() < 0.1, "{}", p.point);
        assert!(p.separation < 0.1);
    }
}

#[test]
fn a_pair_whose_collider_moves_onto_a_body_goes_on_touching() {
    let mut physics = Physics::new();
    let (trigger, _) = cube(&mut physics, 1, Kind::Static, true, Vector::ZERO);
    let (inside, body) = cube(&mut physics, 2, Kind::Rigidbody, false, Vector::ZERO);
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Enter, true, 1, 2)]);
    // The trigger goes onto a kinematic body, as when a game adds one: out
    // and in again, with the same key.
    physics.remove_collider(trigger).unwrap();
    let carrier =
        physics.add_rigidbody(1, BodySpec { kinematic: true, ..BodySpec::default() }, Pose::IDENTITY).unwrap();
    physics.add_object_collider(1, spec(true, BOTH), at(Vector::ZERO), Some(carrier)).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Stay, true, 1, 2)]);
    let _ = (inside, body);
}

#[test]
fn an_exit_comes_when_a_collider_is_removed_changes_layer_or_becomes_a_trigger() {
    // Removed.
    let mut physics = Physics::new();
    let (trigger, _) = cube(&mut physics, 1, Kind::Static, true, Vector::ZERO);
    cube(&mut physics, 2, Kind::Rigidbody, false, Vector::ZERO);
    physics.step(STEP).unwrap();
    physics.remove_collider(trigger).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Exit, true, 1, 2)]);

    // Its body removed, and the collider with it.
    let mut physics = Physics::new();
    cube(&mut physics, 1, Kind::Static, true, Vector::ZERO);
    let (_, body) = cube(&mut physics, 2, Kind::Rigidbody, false, Vector::ZERO);
    physics.step(STEP).unwrap();
    physics.remove_body(body.unwrap()).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Exit, true, 1, 2)]);

    // A layer that doesn't meet the other's.
    let mut physics = Physics::new();
    cube(&mut physics, 1, Kind::Static, true, Vector::ZERO);
    let (inside, _) = cube(&mut physics, 2, Kind::Rigidbody, false, Vector::ZERO);
    physics.set_layer_collision(0, 5, false).unwrap();
    physics.step(STEP).unwrap();
    physics.set_collider_spec(inside, ColliderSpec { layer: 5, ..spec(false, BOTH) }).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Exit, true, 1, 2)]);

    // A solid pair where one becomes a trigger: the collision ends and a
    // trigger pair starts.
    let mut physics = Physics::new();
    let (floor, _) = cube(&mut physics, 1, Kind::Static, false, Vector::ZERO);
    cube(&mut physics, 2, Kind::Rigidbody, false, Vector::new(0.0, 0.95, 0.0));
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Enter, false, 1, 2)]);
    physics.set_collider_spec(floor, spec(true, BOTH)).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Enter, true, 1, 2), (ContactKind::Exit, false, 1, 2)]);
}

#[test]
fn stay_stops_while_the_bodies_sleep_and_the_pair_still_touches() {
    let mut physics = Physics::new();
    let floor = ColliderSpec { shape: ColliderShape::Box { size: Vector::new(20.0, 1.0, 20.0) }, ..spec(false, 0) };
    physics.add_object_collider(1, floor, at(Vector::new(0.0, -0.5, 0.0)), None).unwrap();
    let body = physics.add_rigidbody(2, BodySpec::default(), Pose::from_translation(Vector::Y * 0.5)).unwrap();
    physics.add_object_collider(2, spec(false, LISTEN_COLLISIONS), at(Vector::Y * 0.5), Some(body)).unwrap();
    let mut asleep = false;
    for _ in 0..500 {
        physics.step(STEP).unwrap();
        if physics.is_sleeping(body).unwrap() {
            asleep = true;
            break;
        }
    }
    assert!(asleep, "the cube never slept");
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), []);
    physics.set_sleeping(body, false).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Stay, false, 1, 2)]);
}

#[test]
fn new_gravity_wakes_every_sleeping_body() {
    let mut physics = Physics::new();
    let floor = ColliderSpec { shape: ColliderShape::Box { size: Vector::new(20.0, 1.0, 20.0) }, ..spec(false, 0) };
    physics.add_object_collider(1, floor, at(Vector::new(0.0, -0.5, 0.0)), None).unwrap();
    let body = physics.add_rigidbody(2, BodySpec::default(), Pose::from_translation(Vector::Y * 0.5)).unwrap();
    physics.add_object_collider(2, spec(false, 0), at(Vector::Y * 0.5), Some(body)).unwrap();
    for _ in 0..500 {
        physics.step(STEP).unwrap();
    }
    assert!(physics.is_sleeping(body).unwrap(), "the cube never slept");
    physics.set_gravity(physics.gravity()).unwrap();
    assert!(physics.is_sleeping(body).unwrap(), "the same gravity leaves it asleep");
    physics.set_gravity(Vector::new(0.0, 9.81, 0.0)).unwrap();
    assert!(!physics.is_sleeping(body).unwrap(), "it would hang in the air");
}

#[test]
fn layers_that_dont_meet_pass_through_each_other_and_queries_still_see_both() {
    let mut physics = Physics::new();
    let floor =
        ColliderSpec { shape: ColliderShape::Box { size: Vector::new(20.0, 1.0, 20.0) }, layer: 3, ..spec(false, 0) };
    physics.add_object_collider(1, floor, at(Vector::new(0.0, -0.5, 0.0)), None).unwrap();
    let body = physics.add_rigidbody(2, BodySpec::default(), Pose::from_translation(Vector::Y)).unwrap();
    physics.add_object_collider(2, ColliderSpec { layer: 4, ..spec(false, BOTH) }, at(Vector::Y), Some(body)).unwrap();
    assert!(physics.layer_collision(3, 4).unwrap());
    physics.set_layer_collision(4, 3, false).unwrap();
    assert!(!physics.layer_collision(3, 4).unwrap() && !physics.layer_collision(4, 3).unwrap());
    assert!(physics.layer_collision(3, 3).unwrap() && physics.layer_collision(4, 4).unwrap());
    for _ in 0..50 {
        physics.step(STEP).unwrap();
        assert_eq!(events(&physics), []);
    }
    let y = physics.body_pose(body).unwrap().translation.y;
    assert!(y < -1.0, "it fell through: {y}");
    // Meeting again, a body under the floor is pushed out of it; one
    // dropped onto it lands.
    physics.set_layer_collision(3, 4, true).unwrap();
    physics.set_body_pose(body, Pose::from_translation(Vector::Y)).unwrap();
    physics.set_velocities(body, Vector::ZERO, Vector::ZERO).unwrap();
    for _ in 0..50 {
        physics.step(STEP).unwrap();
    }
    let y = physics.body_pose(body).unwrap().translation.y;
    assert!((y - 0.5).abs() < 0.05, "it landed: {y}");
    assert!(physics.set_layer_collision(32, 0, false).is_err());
}

#[test]
fn two_colliders_that_ignore_each_other_neither_collide_nor_trigger_until_they_dont() {
    let mut physics = Physics::new();
    let floor = ColliderSpec { shape: ColliderShape::Box { size: Vector::new(20.0, 1.0, 20.0) }, ..spec(false, BOTH) };
    let ground = physics.add_object_collider(1, floor, at(Vector::new(0.0, -0.5, 0.0)), None).unwrap();
    let body = physics.add_rigidbody(2, BodySpec::default(), Pose::from_translation(Vector::Y)).unwrap();
    let falling = physics.add_object_collider(2, spec(false, BOTH), at(Vector::Y), Some(body)).unwrap();
    let (trigger, _) = cube(&mut physics, 3, Kind::Static, true, Vector::new(0.0, -3.0, 0.0));
    physics.set_ignores(ground, falling, true).unwrap();
    physics.set_ignores(trigger, falling, true).unwrap();
    assert!(physics.ignores(falling, ground).unwrap());
    assert!(physics.set_ignores(ground, ground, true).is_err());
    for _ in 0..50 {
        physics.step(STEP).unwrap();
        assert_eq!(events(&physics), []);
    }
    assert!(physics.body_pose(body).unwrap().translation.y < -1.0);
    // Forgetting key 3 lets the trigger and the body meet again.
    physics.forget_key(3);
    assert!(!physics.ignores(trigger, falling).unwrap());
    physics.set_body_pose(body, Pose::from_translation(Vector::new(0.0, -3.0, 0.0))).unwrap();
    physics.step(STEP).unwrap();
    assert_eq!(events(&physics), [(ContactKind::Enter, true, 2, 3)]);
}
