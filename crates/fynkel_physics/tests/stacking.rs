//! Bodies kept to a plane, stacked and hung in loops (playtest game 12,
//! a stacking puzzle): friction holds with any constraints, a loop of
//! hinges hangs still, and a box dropped on a stack stays on it.

use fynkel_physics::*;

const DT: f32 = 1.0 / 50.0;

fn collider(shape: ColliderShape) -> ColliderSpec {
    ColliderSpec { shape, center: Vector::ZERO, is_trigger: false, layer: 0, material: Material::DEFAULT, listens: 0 }
}

/// A box `size` across of `mass`, its middle at `at`.
fn body(physics: &mut Physics, key: u64, spec: BodySpec, size: Vector, at: Vector) -> u64 {
    let id = physics.add_rigidbody(key, spec, Pose::from_translation(at)).unwrap();
    let object = ObjectPose { position: at, ..ObjectPose::IDENTITY };
    physics.add_object_collider(key, collider(ColliderShape::Box { size }), object, Some(id)).unwrap();
    id
}

/// A floor, its top at y = 0.
fn floor(physics: &mut Physics) {
    let object = ObjectPose { position: Vector::new(0.0, -0.5, 0.0), ..ObjectPose::IDENTITY };
    physics
        .add_static_collider(1000, collider(ColliderShape::Box { size: Vector::new(20.0, 1.0, 4.0) }), object)
        .unwrap();
}

fn run(physics: &mut Physics, seconds: f32) {
    for _ in 0..(seconds / DT).round() as usize {
        physics.step(DT).unwrap();
    }
}

fn position(physics: &mut Physics, id: u64) -> Vector {
    physics.body_pose(id).unwrap().translation
}

/// How far a 3 kg box pushed at 2 m/s along X slides on the floor.
fn slid(constraints: Constraints) -> f32 {
    let mut physics = Physics::new();
    floor(&mut physics);
    let spec = BodySpec { mass: 3.0, constraints, ..BodySpec::default() };
    let id = body(&mut physics, 1, spec, Vector::new(1.0, 0.6, 0.8), Vector::new(0.0, 0.3, 0.0));
    run(&mut physics, 0.5);
    physics.set_velocities(id, Vector::new(2.0, 0.0, 0.0), Vector::ZERO).unwrap();
    run(&mut physics, 2.0);
    position(&mut physics, id).x
}

#[test]
fn a_body_kept_to_a_plane_slides_and_stops_as_any_other() {
    // Rapier 0.36 solves the two directions of friction together, through
    // the inverse of a 2 × 2 matrix. Frozen along Z and turning only about
    // Z, a body can't be pushed along Z at all: the matrix is singular, its
    // "inverse" was zero, and there was no friction either way.
    let free = slid(Constraints::NONE);
    assert!((0.2..0.6).contains(&free), "slid {free} m free");
    let planar = Constraints(
        Constraints::FREEZE_POSITION_Z.0 | Constraints::FREEZE_ROTATION_X.0 | Constraints::FREEZE_ROTATION_Y.0,
    );
    let sideways = Constraints(
        Constraints::FREEZE_POSITION_X.0 | Constraints::FREEZE_ROTATION_Y.0 | Constraints::FREEZE_ROTATION_Z.0,
    );
    for constraints in [Constraints::FREEZE_POSITION_Z, planar] {
        let d = slid(constraints);
        assert!((d - free).abs() < 0.05, "{constraints:?}: slid {d} m, free {free} m");
    }
    // Frozen along X, pushed along it, it doesn't move.
    let d = slid(sideways);
    assert!(d.abs() < 0.01, "frozen along X, it moved {d} m");
}

/// The highest a bar hung from the world by two hinged links (a loop of
/// hinges, all about Z) gets in 12 s, starting still at y = 4.
fn swing(constraints: Constraints) -> f32 {
    let mut physics = Physics::new();
    let spec = |mass| BodySpec { mass, constraints, ..BodySpec::default() };
    let hinge = |body, anchor, connected, connected_anchor| JointSpec {
        anchor,
        connected,
        connected_anchor,
        axis: Vector::Z,
        ..JointSpec::new(JointKind::Hinge, body)
    };
    // Bodies with no collider, as the game's chains were.
    let mut add = |key, mass, at| physics.add_rigidbody(key, spec(mass), Pose::from_translation(at)).unwrap();
    let a = add(1, 1.0, Vector::new(-1.0, 4.5, 0.0));
    let b = add(2, 1.0, Vector::new(1.0, 4.5, 0.0));
    let bar = add(3, 4.0, Vector::new(0.0, 4.0, 0.0));
    physics.add_joint(1, hinge(a, Vector::new(0.0, 0.5, 0.0), 0, Some(Vector::new(-1.0, 5.0, 0.0)))).unwrap();
    physics.add_joint(2, hinge(b, Vector::new(0.0, 0.5, 0.0), 0, Some(Vector::new(1.0, 5.0, 0.0)))).unwrap();
    physics.add_joint(3, hinge(bar, Vector::new(-1.0, 0.0, 0.0), a, Some(Vector::new(0.0, -0.5, 0.0)))).unwrap();
    physics.add_joint(4, hinge(bar, Vector::new(1.0, 0.0, 0.0), b, Some(Vector::new(0.0, -0.5, 0.0)))).unwrap();
    let mut highest = f32::MIN;
    for _ in 0..600 {
        physics.step(DT).unwrap();
        highest = highest.max(position(&mut physics, bar).y);
    }
    highest
}

#[test]
fn a_loop_of_hinges_hangs_still_with_its_bodies_frozen_along_the_hinges() {
    // With Z frozen and X and Y turns free, one of a hinge's rows is made of
    // the others. Rapier makes the rows independent, which left that one as
    // float noise, then divided by it: kicks from nothing, and the bar flew
    // up past where its links can reach.
    for constraints in [Constraints::NONE, Constraints::FREEZE_POSITION_Z] {
        let highest = swing(constraints);
        assert!((highest - 4.0).abs() < 0.02, "{constraints:?}: the bar rose to {highest}");
    }
}

#[test]
fn a_box_dropped_on_a_stack_stays_on_it() {
    // A 3 kg box dropped 1.8 m onto a 1 kg box on the floor, neither
    // bouncy. At 50 steps a second the lower box is pushed into the floor;
    // pushing it out again, at up to 3 m/s, threw the top box 0.46 m up.
    let mut physics = Physics::new();
    floor(&mut physics);
    let spec = |mass| BodySpec { mass, ..BodySpec::default() };
    body(&mut physics, 1, spec(1.0), Vector::new(0.5, 0.4, 0.8), Vector::new(0.0, 0.2, 0.0));
    run(&mut physics, 1.0);
    let top = body(&mut physics, 2, spec(3.0), Vector::new(1.0, 0.6, 0.8), Vector::new(0.0, 2.5, 0.0));
    let (mut landed, mut highest) = (false, f32::MIN);
    for i in 0..100 {
        physics.step(DT).unwrap();
        let (_, velocity, ..) = physics.body_state(top).unwrap();
        landed |= i > 8 && velocity.y > -0.5;
        if landed {
            highest = highest.max(position(&mut physics, top).y);
        }
    }
    // It rests with its middle at 0.4 + 0.3.
    assert!(landed && highest - 0.7 < 0.03, "it sprang {} m up", highest - 0.7);
}
