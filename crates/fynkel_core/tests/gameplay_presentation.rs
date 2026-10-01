//! Presentation persistence uses the same ordered adoption boundary as gameplay.
use fynkel_core::*;
use fynkel_scene as scene;
use glam::Vec3;

fn at(x: f32) -> Transform {
    Transform::from_position(Vec3::X * x)
}
fn shown(world: &mut World, entity: Entity) -> (f32, bool) {
    let extract = world.extract();
    let record = extract.records.iter().find(|r| r.entity == entity).unwrap();
    (record.world.w_axis.x, record.overridden)
}
fn set(world: &mut World, lane: Lane, entity: Entity, x: f32) {
    world.submit(
        lane,
        vec![Command::SetTransform {
            entity,
            transform: at(x),
        }],
    );
}

#[test]
fn presentation_adoption_publishes_without_a_tick_and_never_replays_over_newer_edits() {
    let mut world = World::new(WorldConfig::default());
    let entity = scene::spawn(world.ecs_mut(), at(0.0), None).unwrap();
    world.step();
    set(&mut world, Lane::Presentation, entity, 4.0);
    assert_eq!(shown(&mut world, entity), (4.0, true));
    assert_eq!(
        world.ecs().copy::<Transform>(entity).unwrap().position.x,
        0.0
    );
    let tick = world.tick();
    world.adopt_gameplay_commands().unwrap();
    assert_eq!(world.tick(), tick);
    assert_eq!(
        world.ecs().copy::<Transform>(entity).unwrap().position.x,
        4.0
    );
    assert_eq!(shown(&mut world, entity), (4.0, false));
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(9.0)))
        .unwrap();
    world.begin_frame(0.0);
    assert_eq!(shown(&mut world, entity), (9.0, false));
    world.step();
    assert_eq!(
        world.ecs().copy::<Transform>(entity).unwrap().position.x,
        9.0
    );
    assert_eq!(shown(&mut world, entity), (9.0, false));
}

#[test]
fn adoption_preserves_mixed_lane_order_and_last_writer_visibility() {
    for presentation_last in [false, true] {
        let mut world = World::new(WorldConfig::default());
        let entity = scene::spawn(world.ecs_mut(), at(0.0), None).unwrap();
        world.step();
        let lanes = if presentation_last {
            [Lane::Simulation, Lane::Presentation]
        } else {
            [Lane::Presentation, Lane::Simulation]
        };
        set(&mut world, lanes[0], entity, 3.0);
        set(&mut world, lanes[1], entity, 7.0);
        world.adopt_gameplay_commands().unwrap();
        assert_eq!(
            world.ecs().copy::<Transform>(entity).unwrap().position.x,
            7.0
        );
        assert_eq!(shown(&mut world, entity), (7.0, false));
        world.step();
        assert_eq!(shown(&mut world, entity), (7.0, false));
    }
}

#[test]
fn failed_adoption_preserves_presentation_override_and_original_replay() {
    let mut world = World::new(WorldConfig::default());
    let entity = scene::spawn(world.ecs_mut(), at(0.0), None).unwrap();
    world.step();
    set(&mut world, Lane::Presentation, entity, 4.0);
    set(&mut world, Lane::Simulation, entity, f32::NAN);
    assert!(world.adopt_gameplay_commands().is_err());
    assert_eq!(
        world.ecs().copy::<Transform>(entity).unwrap().position.x,
        0.0
    );
    assert_eq!(shown(&mut world, entity), (4.0, true));
    assert!(matches!(
        world.edit_gameplay(|_| Ok(())),
        Err(EditError::PendingCommands)
    ));
    world.begin_frame(0.0);
    world.step();
    assert_eq!(
        world.ecs().copy::<Transform>(entity).unwrap().position.x,
        4.0
    );
    assert_eq!(shown(&mut world, entity), (4.0, false));
    world.adopt_gameplay_commands().unwrap();
}

#[test]
fn prepared_tick_preserves_interpolation_across_multiple_callback_commits() {
    let mut world = World::new(WorldConfig::default());
    let entity = scene::spawn(world.ecs_mut(), at(0.0), None).unwrap();
    world.step();
    world.begin_frame(world.config().fixed_dt * 0.5);
    world.prepare_gameplay_tick();
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(4.0)))
        .unwrap();
    world.prepare_gameplay_tick(); // Does not replace the original source.
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(8.0)))
        .unwrap();
    world.step();
    assert_eq!(
        world.ecs().copy::<Transform>(entity).unwrap().position.x,
        8.0
    );
    assert_eq!(shown(&mut world, entity), (4.0, false));
    world.step(); // Prepared state is consumed; the next tick records normally.
    assert_eq!(shown(&mut world, entity), (8.0, false));
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(12.0)))
        .unwrap();
    assert_eq!(shown(&mut world, entity), (12.0, false));
}

#[test]
fn canceling_a_prepared_tick_leaves_committed_edits_visible_and_no_stale_history() {
    let mut world = World::new(WorldConfig::default());
    let entity = scene::spawn(world.ecs_mut(), at(0.0), None).unwrap();
    world.step();
    world.begin_frame(world.config().fixed_dt * 0.5);
    world.prepare_gameplay_tick();
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(8.0)))
        .unwrap();
    world.cancel_gameplay_tick();
    assert_eq!(shown(&mut world, entity), (8.0, false));
    world.prepare_gameplay_tick();
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(12.0)))
        .unwrap();
    world.step();
    world.cancel_gameplay_tick(); // A consumed preparation is unaffected.
    assert_eq!(shown(&mut world, entity), (10.0, false));
}
