//! Native stage commits preserve ordering and interpolation through catch-up ticks.
use fynkel_core::*;
use glam::Vec3;

#[test]
fn mover_oscillator_and_follower_share_current_data_without_snapping_history() {
    for hz in [30.0, 60.0, 120.0, 240.0] {
        let dt = 1.0 / hz;
        let mut w = World::new(WorldConfig {
            fixed_dt: dt,
            ..WorldConfig::default()
        });
        let c = w
            .edit_gameplay(|edit| {
                let mover = edit.spawn("mover", Transform::IDENTITY, None)?;
                let follower = edit.spawn("follower", Transform::IDENTITY, None)?;
                Ok((mover, follower))
            })
            .unwrap();
        let mover = c.resolve(c.value.0).unwrap();
        let follower = c.resolve(c.value.1).unwrap();
        w.step();
        w.edit_gameplay(|edit| {
            edit.set_component(
                mover,
                ComponentValue::Rotator(Rotator {
                    axis: Vec3::Y,
                    speed: 1.0,
                    space: MotionSpace::World,
                }),
            )?;
            edit.set_component(
                mover,
                ComponentValue::Oscillate(Oscillate {
                    axis: Vec3::X,
                    amplitude: 1.0,
                    frequency: 1.0,
                    phase: 0.0,
                    space: MotionSpace::World,
                }),
            )?;
            edit.set_component(
                follower,
                ComponentValue::Follow(Follow {
                    target: fynkel_contracts::ObjectRef::to(mover),
                    offset: Vec3::ZERO,
                    smoothing: 0.0,
                    look_at: false,
                }),
            )
        })
        .unwrap();
        let before_tick = w.tick();
        w.begin_frame(dt * 0.25);
        assert_eq!(
            w.tick(),
            before_tick,
            "zero-tick frame does not run systems"
        );
        assert_eq!(
            w.ecs().copy::<Transform>(mover).unwrap(),
            Transform::IDENTITY
        );
        w.begin_frame(dt * 2.25);
        w.step();
        let first = w.ecs().copy::<Transform>(mover).unwrap().position.x;
        w.step();
        let pose = w.ecs().copy::<Transform>(mover).unwrap();
        let expected = (std::f32::consts::TAU * 2.0 * dt).sin();
        assert!((pose.position.x - expected).abs() < 1e-5);
        assert!(pose
            .rotation
            .abs_diff_eq(glam::Quat::from_rotation_y(2.0 * dt), 1e-5));
        assert!((w.ecs().copy::<Transform>(follower).unwrap().position.x - expected).abs() < 1e-5);
        assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
        let rendered = w.extract();
        for entity in [mover, follower] {
            let record = rendered
                .records
                .iter()
                .find(|r| r.entity == entity)
                .unwrap();
            assert!(
                (record.world.w_axis.x - (first + expected) * 0.5).abs() < 1e-5,
                "native publication must not teleport {entity:?} at {hz} Hz"
            );
        }
    }
}

#[test]
fn transform_validation_agrees_across_component_scene_and_gameplay_entries() {
    let mut w = World::new(WorldConfig::default());
    let (entity, _) = w
        .spawn_now("valid", Transform::IDENTITY, None, vec![], None)
        .unwrap();
    let invalid = Transform {
        rotation: glam::Quat::from_xyzw(0.0, 0.0, 0.0, 2.0),
        ..Transform::IDENTITY
    };
    assert!(ComponentValue::Transform(invalid).validate().is_err());
    assert!(fynkel_scene::set_local(w.ecs(), entity, invalid).is_err());
    assert!(fynkel_scene::spawn(w.ecs_mut(), invalid, None).is_err());
    assert!(w
        .edit_gameplay(|edit| edit.set_transform(entity, invalid))
        .is_err());
    w.submit(
        Lane::Simulation,
        vec![Command::SetTransform {
            entity,
            transform: invalid,
        }],
    );
    assert_eq!(w.step().rejected, 1);
    assert_eq!(
        w.ecs().copy::<Transform>(entity).unwrap(),
        Transform::IDENTITY
    );
    assert_eq!(w.live_entities(), 1);
}
