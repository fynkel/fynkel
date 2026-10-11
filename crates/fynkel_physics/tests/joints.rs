//! Joints (docs/stage-5.md, step 8): each holds as Unity's does (a fixed
//! joint as one body, a hinge's period, limits, motor and spring, a
//! spring's rest length, a character joint's ball and its limits), and
//! breaks at its force.

use std::f32::consts::PI;

use fynkel_physics::*;

const DT: f32 = 1.0 / 50.0;

fn collider(shape: ColliderShape) -> ColliderSpec {
    ColliderSpec { shape, center: Vector::ZERO, is_trigger: false, layer: 0, material: Material::DEFAULT, listens: 0 }
}

/// A rigidbody of `mass` with a box `size` across, its middle at `at`.
fn body(physics: &mut Physics, key: u64, mass: f32, size: Vector, at: Vector) -> u64 {
    let spec = BodySpec { mass, angular_damping: 0.0, ..BodySpec::default() };
    let id = physics.add_rigidbody(key, spec, Pose::from_translation(at)).unwrap();
    let object = ObjectPose { position: at, ..ObjectPose::IDENTITY };
    physics.add_object_collider(key, collider(ColliderShape::Box { size }), object, Some(id)).unwrap();
    id
}

fn cube(physics: &mut Physics, key: u64, at: Vector) -> u64 {
    body(physics, key, 1.0, Vector::splat(0.5), at)
}

fn run(physics: &mut Physics, seconds: f32) {
    for _ in 0..(seconds / DT).round() as usize {
        physics.step(DT).unwrap();
    }
}

fn position(physics: &mut Physics, id: u64) -> Vector {
    physics.body_pose(id).unwrap().translation
}

fn floor(physics: &mut Physics) {
    let object = ObjectPose { position: Vector::new(0.0, -0.5, 0.0), ..ObjectPose::IDENTITY };
    physics
        .add_static_collider(1000, collider(ColliderShape::Box { size: Vector::new(100.0, 1.0, 100.0) }), object)
        .unwrap();
}

#[test]
fn a_fixed_joint_holds_its_body_where_it_was_and_says_how_hard() {
    let mut physics = Physics::new();
    let hanging = cube(&mut physics, 1, Vector::new(0.0, 5.0, 0.0));
    let joint = physics.add_joint(7, JointSpec::new(JointKind::Fixed, hanging)).unwrap();
    run(&mut physics, 3.0);
    let p = position(&mut physics, hanging);
    assert!((p - Vector::new(0.0, 5.0, 0.0)).length() < 0.01, "it sagged to {p}");
    // It holds 1 kg up against gravity: 9.81 N, up.
    let state = physics.joint_state(joint).unwrap();
    assert!((state.force.y - 9.81).abs() < 0.2 && state.force.x.abs() < 0.1, "force {}", state.force);
    assert!(state.torque.length() < 0.1, "torque {}", state.torque);
    assert_eq!(state.connected_anchor, Vector::new(0.0, 5.0, 0.0));
}

#[test]
fn a_fixed_joint_moves_two_bodies_as_one() {
    let mut physics = Physics::new();
    floor(&mut physics);
    let a = cube(&mut physics, 1, Vector::new(0.0, 3.0, 0.0));
    // B is off to the side, turned: the pair falls, tips, and lands as one.
    let b = cube(&mut physics, 2, Vector::new(0.7, 3.4, 0.0));
    let turned = Pose::from_parts(Vector::new(0.7, 3.4, 0.0), Rotation::from_rotation_y(0.5));
    physics.set_body_pose(b, turned).unwrap();
    let relative = physics.body_pose(a).unwrap().inverse() * physics.body_pose(b).unwrap();
    physics.add_joint(1, JointSpec { connected: a, ..JointSpec::new(JointKind::Fixed, b) }).unwrap();
    run(&mut physics, 3.0);
    let now = physics.body_pose(a).unwrap().inverse() * physics.body_pose(b).unwrap();
    assert!((now.translation - relative.translation).length() < 0.02, "{now:?} vs {relative:?}");
    assert!(now.rotation.angle_between(relative.rotation) < 0.02);
    assert!(position(&mut physics, a).y < 1.0, "they fell");
}

#[test]
fn a_hinged_pendulum_swings_with_the_formulas_period() {
    // A small 1 kg ball 2 m below a hinge to the world: T = 2π√(L/g).
    let mut physics = Physics::new();
    let ball = body(&mut physics, 1, 1.0, Vector::splat(0.1), Vector::new(0.0, 3.0, 0.0));
    let hinge =
        JointSpec { anchor: Vector::new(0.0, 2.0, 0.0), axis: Vector::Z, ..JointSpec::new(JointKind::Hinge, ball) };
    let joint = physics.add_joint(1, hinge).unwrap();
    // Pulled 10° aside.
    let pull = Rotation::from_rotation_z(10.0_f32.to_radians());
    let pivot = Vector::new(0.0, 5.0, 0.0);
    physics.set_body_pose(ball, Pose::from_parts(pivot + pull * Vector::new(0.0, -2.0, 0.0), pull)).unwrap();
    let mut crossings = Vec::new();
    let mut last = physics.joint_state(joint).unwrap().angle;
    for i in 1..=400 {
        physics.step(DT).unwrap();
        let angle = physics.joint_state(joint).unwrap().angle;
        if last > 0.0 && angle <= 0.0 {
            crossings.push(i as f32 * DT);
        }
        last = angle;
        // The ball keeps 2 m from the pivot.
        let r = (position(&mut physics, ball) - pivot).length();
        assert!((r - 2.0).abs() < 0.01, "{r} m from the pivot");
    }
    assert!((last.to_degrees().abs()) < 11.0);
    let period = crossings[2] - crossings[1];
    let expected = 2.0 * PI * (2.0 / 9.81_f32).sqrt();
    assert!((period - expected).abs() < 0.05, "period {period}, expected {expected}");
}

/// A door: 1 m wide, 2 m tall, hinged to the world on its left edge about Y.
fn door(physics: &mut Physics, spec: impl FnOnce(JointSpec) -> JointSpec) -> (u64, u64) {
    physics.set_gravity(Vector::ZERO).unwrap();
    let door = body(physics, 1, 10.0, Vector::new(1.0, 2.0, 0.1), Vector::new(0.5, 1.0, 0.0));
    let hinge =
        JointSpec { anchor: Vector::new(-0.5, 0.0, 0.0), axis: Vector::Y, ..JointSpec::new(JointKind::Hinge, door) };
    let joint = physics.add_joint(1, spec(hinge)).unwrap();
    (door, joint)
}

fn degrees(physics: &Physics, joint: u64) -> f32 {
    physics.joint_state(joint).unwrap().angle.to_degrees()
}

#[test]
fn a_hinge_turns_about_its_axis_and_stops_at_its_limits() {
    for limits in [None, Some([-30.0_f32, 90.0])] {
        let mut physics = Physics::new();
        let (door, joint) = door(&mut physics, |h| JointSpec { limits: limits.map(|l| l.map(f32::to_radians)), ..h });
        // Pushed at its free edge, away from the viewer (-Z): it turns about +Y.
        // Turning about the hinge, 0.5 m from its middle.
        physics.set_velocities(door, Vector::Z * -1.5, Vector::Y * 3.0).unwrap();
        let mut top = f32::MIN;
        for _ in 0..50 {
            physics.step(DT).unwrap();
            top = top.max(degrees(&physics, joint));
            // The hinge's edge stays put.
            let pose = physics.body_pose(door).unwrap();
            let edge = pose * Vector::new(-0.5, 0.0, 0.0);
            assert!(edge.abs_diff_eq(Vector::new(0.0, 1.0, 0.0), 0.01), "the edge went to {edge}");
        }
        match limits {
            None => assert!(top > 150.0, "it turned to {top}°"),
            Some(_) => assert!((top - 90.0).abs() < 3.0, "it stopped at {top}°"),
        }
        // Turned the other way, it stops at the min.
        if limits.is_some() {
            physics.set_velocities(door, Vector::ZERO, Vector::ZERO).unwrap();
            let pose = physics.body_pose(door).unwrap();
            let spin = Vector::Y * -3.0;
            physics.set_velocities(door, spin.cross(pose.rotation * Vector::new(0.5, 0.0, 0.0)), spin).unwrap();
            run(&mut physics, 1.0);
            let a = degrees(&physics, joint);
            assert!((a + 30.0).abs() < 3.0, "it stopped at {a}°");
        }
    }
}

#[test]
fn a_hinges_motor_turns_it_at_its_velocity_with_no_more_than_its_force() {
    let mut physics = Physics::new();
    let motor = JointMotor { velocity: 90.0_f32.to_radians(), force: 100.0 };
    let (_, joint) = door(&mut physics, |h| JointSpec { motor: Some(motor), ..h });
    run(&mut physics, 1.0);
    let v = physics.joint_state(joint).unwrap().velocity.to_degrees();
    assert!((v - 90.0).abs() < 1.0, "{v}°/s");
    assert!(degrees(&physics, joint) > 70.0);
    // A weak motor takes longer to get there: a door of 10 kg, 1 m wide,
    // about its edge: I = 10·1²/3 ≈ 3.4 kg·m²; 1 N·m gives 0.3 rad/s².
    let mut physics = Physics::new();
    let weak = JointMotor { velocity: 90.0_f32.to_radians(), force: 1.0 };
    let (_, joint) = door(&mut physics, |h| JointSpec { motor: Some(weak), ..h });
    run(&mut physics, 1.0);
    let v = physics.joint_state(joint).unwrap().velocity;
    let expected = 1.0 / (10.0 * (1.0 + 0.01) / 3.0);
    assert!((v - expected).abs() < 0.03, "{v} rad/s, expected {expected}");
    // A slow motor keeps turning: it doesn't fall asleep.
    let mut physics = Physics::new();
    let slow = JointMotor { velocity: 10.0_f32.to_radians(), force: 100.0 };
    let (_, joint) = door(&mut physics, |h| JointSpec { motor: Some(slow), ..h });
    run(&mut physics, 6.0);
    let a = degrees(&physics, joint);
    assert!((a - 60.0).abs() < 2.0, "{a}°");
    // Nor short of a limit, and it sleeps there.
    let mut physics = Physics::new();
    let (body, joint) = door(&mut physics, |h| JointSpec { motor: Some(slow), limits: Some([-1.0, 0.5]), ..h });
    run(&mut physics, 8.0);
    let a = degrees(&physics, joint);
    assert!((a - 0.5_f32.to_degrees()).abs() < 1.0, "{a}°");
    assert!(physics.is_sleeping(body).unwrap());
    // And none with no force.
    let mut physics = Physics::new();
    let off = JointMotor { velocity: 1.0, force: 0.0 };
    let (_, joint) = door(&mut physics, |h| JointSpec { motor: Some(off), ..h });
    run(&mut physics, 1.0);
    assert_eq!(degrees(&physics, joint), 0.0);
}

#[test]
fn a_hinges_spring_turns_it_to_its_target() {
    let mut physics = Physics::new();
    let spring = JointSpring { spring: 50.0, damper: 10.0, target: 45.0_f32.to_radians() };
    let (_, joint) = door(&mut physics, |h| JointSpec { spring: Some(spring), ..h });
    run(&mut physics, 2.0);
    let first = degrees(&physics, joint);
    assert!(first > 30.0, "{first}°");
    run(&mut physics, 4.0);
    // It doesn't fall asleep short of it while the spring still pulls.
    let a = degrees(&physics, joint);
    assert!((a - 45.0).abs() < 0.2, "{a}°");
    // A spring's settings change on the joint as it is.
    let spec = JointSpec {
        spring: Some(JointSpring { target: -20.0_f32.to_radians(), ..spring }),
        ..physics.joint_spec(joint).unwrap()
    };
    physics.set_joint_spec(joint, spec).unwrap();
    run(&mut physics, 6.0);
    let a = degrees(&physics, joint);
    assert!((a + 20.0).abs() < 0.2, "{a}°");
    // A weak, underdamped spring swings about its target a long time,
    // slowly near the end, and still gets there.
    let mut physics = Physics::new();
    let weak = JointSpring { spring: 20.0, damper: 2.0, target: 0.0 };
    let (body, joint) = door(&mut physics, |h| JointSpec { spring: Some(weak), ..h });
    physics.set_velocities(body, Vector::Z * -1.5, Vector::Y * 3.0).unwrap();
    run(&mut physics, 30.0);
    let a = degrees(&physics, joint);
    assert!(a.abs() < 0.2, "{a}°");
}

#[test]
fn a_spring_pulls_its_anchors_together_unless_they_are_within_its_distances() {
    // Pulled from 1 m, it ends within its distances: at 0 with none, in the
    // band with one. Pushed out past the min, it carries on freely through
    // the band, and the spring only slows it outside it: it stays about the
    // band, but needn't stop.
    for (min, max, near, far) in [(0.0, 0.0, 0.0, 0.02), (0.0, 0.5, 0.0, 0.52), (1.5, 3.0, 1.45, 3.1)] {
        let mut physics = Physics::new();
        physics.set_gravity(Vector::ZERO).unwrap();
        let ball = cube(&mut physics, 1, Vector::new(1.0, 0.0, 0.0));
        let spring = JointSpec {
            connected_anchor: Some(Vector::ZERO),
            spring: Some(JointSpring { spring: 20.0, damper: 2.0, target: 0.0 }),
            min_distance: min,
            max_distance: max,
            ..JointSpec::new(JointKind::Spring, ball)
        };
        physics.add_joint(1, spring).unwrap();
        run(&mut physics, 10.0);
        let d = position(&mut physics, ball).length();
        assert!(d >= near && d <= far, "{min} to {max}: it rests {d} m away");
        if min == 0.0 {
            assert!(physics.body_state(ball).unwrap().1.length() < 0.05, "{min} to {max}: it moves");
        }
    }
}

#[test]
fn a_spring_lets_its_body_fall_freely_within_its_distances() {
    // Hanging 1 m below its anchor, within 0 to 3 m: it falls as if free,
    // and the spring catches it only past 3 m. (Rapier holds a motor of no
    // stiffness and no damping still, as a rod.)
    let mut physics = Physics::new();
    let ball = cube(&mut physics, 1, Vector::new(0.0, -1.0, 0.0));
    let spring = JointSpec {
        connected_anchor: Some(Vector::ZERO),
        spring: Some(JointSpring { spring: 40.0, damper: 1.0, target: 0.0 }),
        min_distance: 0.0,
        max_distance: 3.0,
        ..JointSpec::new(JointKind::Spring, ball)
    };
    physics.add_joint(1, spring).unwrap();
    run(&mut physics, 0.4);
    let fell = -1.0 - position(&mut physics, ball).y;
    assert!((fell - 0.5 * 9.81 * 0.4 * 0.4).abs() < 0.05, "it fell {fell} m in 0.4 s");
    run(&mut physics, 10.0);
    let y = position(&mut physics, ball).y;
    assert!((y - (-3.0 - 9.81 / 40.0)).abs() < 0.02, "it hangs at {y}, past 3 m by its weight");
}

#[test]
fn a_spring_keeps_its_bodies_as_they_were_and_hangs_its_weight_by_its_stiffness() {
    // Unity's default: the connected anchor where the anchor was, so it
    // rests there; 1 kg hanging stretches it m·g/k.
    let mut physics = Physics::new();
    let top = cube(&mut physics, 1, Vector::new(0.0, 5.0, 0.0));
    physics.add_joint(1, JointSpec::new(JointKind::Fixed, top)).unwrap();
    let hanging = cube(&mut physics, 2, Vector::new(0.0, 3.0, 0.0));
    let spring = JointSpec {
        connected: top,
        spring: Some(JointSpring { spring: 100.0, damper: 5.0, target: 0.0 }),
        ..JointSpec::new(JointKind::Spring, hanging)
    };
    physics.add_joint(2, spring).unwrap();
    run(&mut physics, 10.0);
    let y = position(&mut physics, hanging).y;
    assert!((y - (3.0 - 9.81 / 100.0)).abs() < 0.01, "it hangs at {y}");
}

/// A rod 1 m long and 0.1 thick, hanging from (0, 5, 0) on a character
/// joint twisting about the rod (Y) and swinging about X and Z.
fn rod(physics: &mut Physics, twist: [f32; 2], swing: [f32; 2]) -> (u64, u64) {
    let rod = body(physics, 1, 1.0, Vector::new(0.1, 1.0, 0.1), Vector::new(0.0, 4.5, 0.0));
    let spec = JointSpec {
        anchor: Vector::new(0.0, 0.5, 0.0),
        axis: Vector::Y,
        swing_axis: Vector::X,
        limits: Some(twist),
        swing_limits: Some(swing),
        ..JointSpec::new(JointKind::Character, rod)
    };
    let joint = physics.add_joint(7, spec).unwrap();
    (rod, joint)
}

/// The rod's tilt from hanging straight down, and its twist about itself.
fn tilt_and_twist(physics: &mut Physics, rod: u64) -> (f32, f32) {
    let r = physics.body_pose(rod).unwrap().rotation;
    let tilt = (r * Vector::Y).dot(Vector::Y).clamp(-1.0, 1.0).acos();
    // The twist: the turn about the rod's own axis, once its swing is taken out.
    let swing = Rotation::from_rotation_arc(Vector::Y, r * Vector::Y);
    let twist = swing.inverse() * r;
    (tilt, 2.0 * twist.y.atan2(twist.w))
}

#[test]
fn a_free_character_joint_swings_every_way_and_twists_about_its_anchor() {
    let mut physics = Physics::new();
    let (rod, _) = rod(&mut physics, [-PI, PI], [PI, PI]);
    // Pushed at its foot, sideways and forwards, and spun about itself.
    let foot = Vector::new(0.0, 4.0, 0.0);
    let push = Push::AtPosition { force: Vector::new(1.0, 0.0, 0.8), position: foot, mode: ForceMode::Impulse };
    physics.push(rod, push).unwrap();
    physics.push(rod, Push::Torque { torque: Vector::new(0.0, 0.005, 0.0), mode: ForceMode::Impulse }).unwrap();
    let (mut far_x, mut far_z, mut twisted) = (0f32, 0f32, 0f32);
    for _ in 0..100 {
        run(&mut physics, DT);
        let pose = physics.body_pose(rod).unwrap();
        let hook = pose * Vector::new(0.0, 0.5, 0.0);
        assert!((hook - Vector::new(0.0, 5.0, 0.0)).length() < 0.01, "its anchor left the hook: {hook}");
        let foot = pose * Vector::new(0.0, -0.5, 0.0);
        (far_x, far_z) = (far_x.max(foot.x.abs()), far_z.max(foot.z.abs()));
        twisted = twisted.max(tilt_and_twist(&mut physics, rod).1.abs());
    }
    assert!(far_x > 0.3 && far_z > 0.2, "it swung {far_x} m across and {far_z} m forwards");
    assert!(twisted > 0.5, "it twisted {twisted} rad");
}

#[test]
fn a_character_joint_stops_at_its_swing_and_twist_limits() {
    let mut physics = Physics::new();
    let (rod, joint) = rod(&mut physics, [-20f32.to_radians(), 20f32.to_radians()], [30f32.to_radians(); 2]);
    let foot = Vector::new(0.0, 4.0, 0.0);
    // About 4.5 rad/s, enough to swing it 70° unlimited; and 3 rad/s about itself.
    let push = Push::AtPosition { force: Vector::new(1.5, 0.0, 0.0), position: foot, mode: ForceMode::Impulse };
    physics.push(rod, push).unwrap();
    physics.push(rod, Push::Torque { torque: Vector::new(0.0, 0.005, 0.0), mode: ForceMode::Impulse }).unwrap();
    let (mut tilted, mut twisted) = (0f32, 0f32);
    for _ in 0..100 {
        run(&mut physics, DT);
        let (tilt, twist) = tilt_and_twist(&mut physics, rod);
        (tilted, twisted) = (tilted.max(tilt), twisted.max(twist.abs()));
    }
    assert!(tilted > 25f32.to_radians() && tilted < 33f32.to_radians(), "it swung to {}°", tilted.to_degrees());
    assert!(twisted > 15f32.to_radians() && twisted < 23f32.to_radians(), "it twisted to {}°", twisted.to_degrees());
    // Its limits change on the joint as it is: freed, it swings past them.
    let spec = JointSpec { swing_limits: Some([PI, PI]), ..physics.joint_spec(joint).unwrap() };
    physics.set_joint_spec(joint, spec).unwrap();
    physics
        .push(rod, Push::AtPosition { force: Vector::new(1.5, 0.0, 0.0), position: foot, mode: ForceMode::Impulse })
        .unwrap();
    let mut tilted = 0f32;
    for _ in 0..50 {
        run(&mut physics, DT);
        tilted = tilted.max(tilt_and_twist(&mut physics, rod).0);
    }
    assert!(tilted > 45f32.to_radians(), "freed, it swung to {}°", tilted.to_degrees());
}

#[test]
fn a_character_joints_axes_and_limits_are_checked() {
    let mut physics = Physics::new();
    let id = cube(&mut physics, 1, Vector::ZERO);
    let spec = JointSpec::new(JointKind::Character, id);
    assert_eq!(spec.limits, Some([(-20f32).to_radians(), 70f32.to_radians()]), "Unity's defaults");
    assert_eq!(spec.swing_limits, Some([40f32.to_radians(); 2]));
    for bad in [
        JointSpec { swing_axis: Vector::X, ..spec },
        JointSpec { swing_axis: Vector::new(-2.0, 0.0, 0.0), ..spec },
        JointSpec { swing_axis: Vector::ZERO, ..spec },
        JointSpec { swing_limits: Some([-0.1, 0.5]), ..spec },
        JointSpec { swing_limits: Some([0.5, 4.0]), ..spec },
        JointSpec { limits: Some([-4.0, 0.0]), ..spec },
    ] {
        assert!(physics.add_joint(1, bad).is_err(), "{bad:?} was taken");
    }
    // A swing axis not square to the axis is made square to it.
    assert!(physics.add_joint(1, JointSpec { swing_axis: Vector::new(1.0, 1.0, 0.0), ..spec }).is_ok());
}

#[test]
fn a_joint_breaks_when_it_holds_harder_than_its_break_force_or_torque() {
    // 1 kg hanging: 9.81 N. It holds with 20, breaks with 5.
    for (force, breaks) in [(20.0, false), (5.0, true)] {
        let mut physics = Physics::new();
        let hanging = cube(&mut physics, 1, Vector::new(0.0, 5.0, 0.0));
        let joint = physics
            .add_joint(42, JointSpec { break_force: force, ..JointSpec::new(JointKind::Fixed, hanging) })
            .unwrap();
        let mut broken = Vec::new();
        for _ in 0..50 {
            physics.step(DT).unwrap();
            broken.extend_from_slice(physics.broken_joints());
        }
        if breaks {
            assert_eq!(broken.len(), 1);
            assert_eq!(broken[0].key, 42);
            assert!(broken[0].force > 5.0 && broken[0].force < 12.0, "{:?}", broken[0]);
            assert!(physics.joint_state(joint).is_err(), "it's gone");
            assert!(position(&mut physics, hanging).y < 2.0, "it fell");
        } else {
            assert!(broken.is_empty());
            assert!(physics.joint_state(joint).is_ok());
        }
    }
    // Held 1 m to the side: a torque of 9.81 N·m.
    for (torque, breaks) in [(20.0, false), (5.0, true)] {
        let mut physics = Physics::new();
        let hanging = cube(&mut physics, 1, Vector::new(1.0, 5.0, 0.0));
        let spec = JointSpec {
            anchor: Vector::new(-1.0, 0.0, 0.0),
            break_torque: torque,
            ..JointSpec::new(JointKind::Fixed, hanging)
        };
        physics.add_joint(1, spec).unwrap();
        run(&mut physics, 1.0);
        assert_eq!(position(&mut physics, hanging).y < 4.0, breaks, "break torque {torque}");
    }
}

#[test]
fn joined_bodies_collide_only_if_told_to() {
    for enable_collision in [false, true] {
        let mut physics = Physics::new();
        physics.set_gravity(Vector::ZERO).unwrap();
        // Two cubes side by side, overlapping, joined by a spring that
        // doesn't pull (its band holds anything up to 10 m).
        let a = cube(&mut physics, 1, Vector::new(0.0, 0.0, 0.0));
        let b = cube(&mut physics, 2, Vector::new(0.3, 0.0, 0.0));
        let spring =
            JointSpec { connected: a, enable_collision, max_distance: 10.0, ..JointSpec::new(JointKind::Spring, b) };
        physics.add_joint(1, spring).unwrap();
        run(&mut physics, 1.0);
        let apart = position(&mut physics, b).x - position(&mut physics, a).x;
        assert_eq!(apart > 0.35, enable_collision, "{apart} apart");
    }
}

#[test]
fn removing_a_body_removes_its_joints_and_bad_joints_are_refused() {
    let mut physics = Physics::new();
    let a = cube(&mut physics, 1, Vector::ZERO);
    let b = cube(&mut physics, 2, Vector::X);
    let joint = physics.add_joint(1, JointSpec { connected: b, ..JointSpec::new(JointKind::Hinge, a) }).unwrap();
    assert_eq!(physics.joint_count(), 1);
    physics.remove_body(b).unwrap();
    assert_eq!(physics.joint_count(), 0);
    assert_eq!(physics.joint_state(joint), Err(PhysicsError::NoJoint(joint)));
    assert_eq!(physics.remove_joint(joint), Err(PhysicsError::NoJoint(joint)));

    let s = JointSpec::new(JointKind::Hinge, a);
    for bad in [
        JointSpec { axis: Vector::ZERO, ..s },
        JointSpec { anchor: Vector::NAN, ..s },
        JointSpec { break_force: 0.0, ..s },
        JointSpec { break_torque: f32::NAN, ..s },
        JointSpec { limits: Some([1.0, -1.0]), ..s },
        JointSpec { limits: Some([-4.0, 0.0]), ..s },
        JointSpec { motor: Some(JointMotor { velocity: 1.0, force: -1.0 }), ..s },
        JointSpec { spring: Some(JointSpring { spring: -1.0, damper: 0.0, target: 0.0 }), ..s },
        JointSpec { min_distance: 2.0, max_distance: 1.0, ..s },
        JointSpec { connected: a, ..s },
    ] {
        assert!(matches!(physics.add_joint(1, bad), Err(PhysicsError::Value(_))), "{bad:?}");
    }
    assert_eq!(physics.add_joint(1, JointSpec { connected: b, ..s }), Err(PhysicsError::NoBody(b)));
    // A plain static collider's body is no rigidbody.
    let plain = physics.add_body(BodyKind::Dynamic, 9, Pose::IDENTITY).unwrap();
    assert!(physics.add_joint(1, JointSpec::new(JointKind::Fixed, plain)).is_err());
    // What a joint is joined to can't change as it is.
    let joint = physics.add_joint(1, s).unwrap();
    assert!(physics.set_joint_spec(joint, JointSpec { anchor: Vector::Y, ..s }).is_err());
    physics.set_joint_spec(joint, JointSpec { limits: Some([0.0, 1.0]), ..s }).unwrap();
    // But an anchor read back from a turned object's scale, a hair off, is
    // the same anchor: the joint keeps the one it was made with.
    let hair = JointSpec { anchor: s.anchor + Vector::splat(1e-6), break_force: 5.0, ..s };
    physics.set_joint_spec(joint, hair).unwrap();
    assert_eq!(physics.joint_spec(joint).unwrap().anchor, s.anchor);
    assert_eq!(physics.joint_spec(joint).unwrap().break_force, 5.0);
    physics.remove_joint(joint).unwrap();
    assert_eq!(physics.joint_count(), 0);
}

#[test]
fn a_joint_says_the_force_it_holds_with_and_the_torque_about_its_anchor() {
    // 1 kg held 1 m to the side: 9.81 N up, 9.81 N·m about Z.
    let mut physics = Physics::new();
    let held = cube(&mut physics, 1, Vector::new(1.0, 5.0, 0.0));
    let spec = JointSpec { anchor: Vector::new(-1.0, 0.0, 0.0), ..JointSpec::new(JointKind::Fixed, held) };
    let joint = physics.add_joint(1, spec).unwrap();
    run(&mut physics, 0.5);
    let s = physics.joint_state(joint).unwrap();
    assert!(s.force.abs_diff_eq(Vector::new(0.0, 9.81, 0.0), 0.05), "{}", s.force);
    assert!(s.torque.abs_diff_eq(Vector::new(0.0, 0.0, 9.81), 0.05), "{}", s.torque);
    // A 10 kg door on its hinge: 98.1 N, and 49 N·m to keep it from
    // tipping; none about its axis.
    let mut physics = Physics::new();
    let (_, joint) = door(&mut physics, |h| h);
    physics.set_gravity(Vector::new(0.0, -9.81, 0.0)).unwrap();
    run(&mut physics, 0.5);
    let s = physics.joint_state(joint).unwrap();
    assert!(s.force.abs_diff_eq(Vector::new(0.0, 98.1, 0.0), 0.5), "{}", s.force);
    assert!(s.torque.abs_diff_eq(Vector::new(0.0, 0.0, 49.05), 0.5), "{}", s.torque);
    // A spring holding 1 kg up: about 9.81 N, up.
    let mut physics = Physics::new();
    let ball = cube(&mut physics, 1, Vector::new(0.0, 3.0, 0.0));
    let spring = JointSpec {
        connected_anchor: Some(Vector::new(0.0, 4.0, 0.0)),
        spring: Some(JointSpring { spring: 100.0, damper: 5.0, target: 0.0 }),
        ..JointSpec::new(JointKind::Spring, ball)
    };
    let joint = physics.add_joint(1, spring).unwrap();
    run(&mut physics, 10.0);
    let s = physics.joint_state(joint).unwrap();
    assert!(s.force.abs_diff_eq(Vector::new(0.0, 9.81, 0.0), 0.5), "{}", s.force);
    assert_eq!(s.torque, Vector::ZERO);
}
