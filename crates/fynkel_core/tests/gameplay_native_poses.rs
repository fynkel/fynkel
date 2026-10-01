use fynkel_contracts::{ComponentId, ObjectRef};
use fynkel_core::world::Oscillation;
use fynkel_core::*;
use glam::Vec3;

fn node(w: &mut World, position: Vec3, parent: Option<Entity>) -> Entity {
    let c = w
        .edit_gameplay(|e| {
            e.spawn(
                "node",
                Transform::from_position(position),
                parent.map(Into::into),
            )
        })
        .unwrap();
    c.resolve(c.value).unwrap()
}
fn pose(w: &World, e: Entity) -> Transform {
    w.ecs().copy::<Transform>(e).unwrap()
}
fn rejected(w: &World, component: ComponentId) {
    assert!(
        w.diagnostics().iter().any(|d| d.component == component),
        "{:?}",
        w.diagnostics()
    );
}

#[test]
fn rotator_pass_rejects_every_pose_when_one_raw_configuration_is_invalid() {
    let mut w = World::new(WorldConfig::default());
    let good = node(&mut w, Vec3::ZERO, None);
    let bad = node(&mut w, Vec3::X, None);
    w.ecs_mut().insert(good, Rotator::default()).unwrap();
    w.ecs_mut()
        .insert(
            bad,
            Rotator {
                speed: f32::NAN,
                ..Default::default()
            },
        )
        .unwrap();
    let before = pose(&w, good);
    w.step();
    assert_eq!(pose(&w, good), before);
    rejected(&w, ComponentId::ROTATOR);
    w.ecs_mut().remove::<Rotator>(bad).unwrap();
    w.step();
    assert_ne!(pose(&w, good).rotation, before.rotation);
}

#[test]
fn oscillator_rejection_preserves_pose_clock_and_offset_as_one_stage() {
    let mut w = World::new(WorldConfig::default());
    let good = node(&mut w, Vec3::ZERO, None);
    let bad = node(&mut w, Vec3::X, None);
    w.ecs_mut().insert(good, Oscillate::default()).unwrap();
    w.ecs_mut()
        .insert(
            bad,
            Oscillate {
                amplitude: f32::NAN,
                ..Default::default()
            },
        )
        .unwrap();
    w.step();
    assert_eq!(pose(&w, good).position, Vec3::ZERO);
    assert!(!w.ecs().has::<Oscillation>(good));
    rejected(&w, ComponentId::OSCILLATE);
    w.ecs_mut().remove::<Oscillate>(bad).unwrap();
    w.step();
    let prior_pose = pose(&w, good);
    let prior = w.ecs().copy::<Oscillation>(good).unwrap();
    assert!((prior.elapsed - w.config().fixed_dt).abs() < 1e-6);
    w.ecs_mut()
        .insert(
            bad,
            Oscillate {
                amplitude: f32::NAN,
                ..Default::default()
            },
        )
        .unwrap();
    w.step();
    assert_eq!(pose(&w, good), prior_pose);
    let after = w.ecs().copy::<Oscillation>(good).unwrap();
    assert_eq!((after.elapsed, after.offset), (prior.elapsed, prior.offset));
}

fn singular_parent(w: &mut World) -> Entity {
    let parent = node(w, Vec3::ZERO, None);
    w.edit_gameplay(|edit| {
        edit.set_transform(
            parent,
            Transform {
                scale: Vec3::ZERO,
                ..Default::default()
            },
        )
    })
    .unwrap();
    parent
}

#[test]
fn follower_pass_rejects_all_placements_when_one_parent_cannot_be_inverted() {
    let mut w = World::new(WorldConfig::default());
    let target = node(&mut w, Vec3::X * 10.0, None);
    let good = node(&mut w, Vec3::ZERO, None);
    let parent = singular_parent(&mut w);
    let bad = node(&mut w, Vec3::ZERO, Some(parent));
    for owner in [good, bad] {
        w.ecs_mut()
            .insert(
                owner,
                Follow {
                    target: ObjectRef::to(target),
                    smoothing: 0.0,
                    offset: Vec3::ZERO,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    w.step();
    assert_eq!(pose(&w, good).position, Vec3::ZERO);
    rejected(&w, ComponentId::FOLLOW);
    w.edit_gameplay(|e| e.set_transform(parent, Transform::IDENTITY))
        .unwrap();
    w.step();
    assert_eq!(pose(&w, good).position, Vec3::X * 10.0);
    assert_eq!(pose(&w, bad).position, Vec3::X * 10.0);
}

#[test]
fn billboard_pass_rejects_all_rotations_when_one_placement_is_invalid() {
    let mut w = World::new(WorldConfig::default());
    let camera = node(&mut w, Vec3::X * 10.0, None);
    w.ecs_mut().insert(camera, Camera::default()).unwrap();
    let good = node(&mut w, Vec3::ZERO, None);
    let parent = singular_parent(&mut w);
    let bad = node(&mut w, Vec3::ZERO, Some(parent));
    for owner in [good, bad] {
        w.ecs_mut().insert(owner, LookAtCamera::default()).unwrap();
    }
    let before = pose(&w, good);
    w.step();
    assert_eq!(pose(&w, good), before);
    rejected(&w, ComponentId::LOOK_AT_CAMERA);
    w.ecs_mut().remove::<LookAtCamera>(bad).unwrap();
    w.step();
    assert_ne!(pose(&w, good).rotation, before.rotation);
}

#[test]
fn camera_rig_configuration_and_pose_roll_back_together_on_failed_placement() {
    let mut w = World::new(WorldConfig::default());
    let target = node(&mut w, Vec3::ZERO, None);
    let good = node(&mut w, Vec3::ZERO, None);
    let parent = singular_parent(&mut w);
    let bad = node(&mut w, Vec3::ZERO, Some(parent));
    w.bind_input(&[ActionSpec {
        name: "look".into(),
        kind: ActionKind::Axis2d,
        bindings: vec!["mouse:delta".into()],
    }]);
    for owner in [good, bad] {
        w.ecs_mut()
            .insert(
                owner,
                ThirdPersonCamera {
                    target: ObjectRef::to(target),
                    look_action: "look".into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let before = w.ecs().copy::<ThirdPersonCamera>(good).unwrap();
    w.capture_input(&[InputEvent::moved(Device::Mouse, "delta", 0.5, 0.25)]);
    w.step();
    assert_eq!(w.ecs().copy::<ThirdPersonCamera>(good).unwrap(), before);
    assert_eq!(pose(&w, good).position, Vec3::ZERO);
    rejected(&w, ComponentId::THIRD_PERSON_CAMERA);
    w.ecs_mut().remove::<ThirdPersonCamera>(bad).unwrap();
    w.capture_input(&[InputEvent::moved(Device::Mouse, "delta", 0.5, 0.25)]);
    w.step();
    assert_ne!(
        w.ecs().copy::<ThirdPersonCamera>(good).unwrap().yaw,
        before.yaw
    );
    assert_ne!(pose(&w, good).position, Vec3::ZERO);
}

#[test]
fn removing_the_last_oscillator_discards_its_runtime_clock_before_readding() {
    for raw_removal in [false, true] {
        let mut w = World::new(WorldConfig::default());
        let owner = node(&mut w, Vec3::ZERO, None);
        w.edit_gameplay(|edit| {
            edit.set_component(owner, ComponentValue::Oscillate(Oscillate::default()))
        })
        .unwrap();
        w.step();
        w.step();
        assert!(w.ecs().copy::<Oscillation>(owner).unwrap().elapsed > w.config().fixed_dt);
        if raw_removal {
            w.ecs_mut().remove::<Oscillate>(owner).unwrap();
        } else {
            w.edit_gameplay(|edit| edit.remove_component(owner, ComponentId::OSCILLATE))
                .unwrap();
            assert!(!w.ecs().has::<Oscillation>(owner));
        }
        w.step();
        assert!(!w.ecs().has::<Oscillation>(owner));
        w.edit_gameplay(|edit| {
            edit.set_component(owner, ComponentValue::Oscillate(Oscillate::default()))
        })
        .unwrap();
        w.step();
        assert!(
            (w.ecs().copy::<Oscillation>(owner).unwrap().elapsed - w.config().fixed_dt).abs()
                < 1e-6
        );
    }
}

#[test]
fn invalid_follower_configuration_cannot_silently_fall_back_to_a_valid_pose() {
    let mut w = World::new(WorldConfig::default());
    let target = node(&mut w, Vec3::X * 4.0, None);
    let good = node(&mut w, Vec3::ZERO, None);
    let bad = node(&mut w, Vec3::ZERO, None);
    for (owner, smoothing) in [(good, 0.0), (bad, f32::NAN)] {
        w.ecs_mut()
            .insert(
                owner,
                Follow {
                    target: ObjectRef::to(target),
                    smoothing,
                    offset: Vec3::ZERO,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    w.step();
    // NaN > 0 is false: the old pass silently treated this as a snap.
    assert_eq!(pose(&w, good).position, Vec3::ZERO);
    assert_eq!(pose(&w, bad).position, Vec3::ZERO);
    rejected(&w, ComponentId::FOLLOW);
}
