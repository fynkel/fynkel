//! Persistent transform commits have one authority and no replay after publication.
use fynkel_core::*;
use fynkel_scene as scene;
use glam::Vec3;

fn at(x: f32) -> Transform {
    Transform::from_position(Vec3::X * x)
}
fn shown(world: &mut World, entity: Entity) -> f32 {
    let extract = world.extract();
    let record = extract.records.iter().find(|r| r.entity == entity).unwrap();
    record.world.w_axis.x
}
#[test]
fn transform_commits_publish_without_a_tick_and_never_replay_over_newer_edits() {
    let mut world = World::new(WorldConfig::default());
    let entity = scene::spawn(world.ecs_mut(), at(0.0), None).unwrap();
    world.step();
    let tick = world.tick();
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(4.0)))
        .unwrap();
    assert_eq!(world.tick(), tick);
    assert_eq!(
        world.ecs().copy::<Transform>(entity).unwrap().position.x,
        4.0
    );
    assert_eq!(shown(&mut world, entity), 4.0);
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(9.0)))
        .unwrap();
    world.begin_frame(0.0);
    assert_eq!(shown(&mut world, entity), 9.0);
    world.step();
    assert_eq!(shown(&mut world, entity), 9.0);
}

#[test]
fn separate_commits_and_one_atomic_edit_have_the_same_last_writer_visibility() {
    for separate in [false, true] {
        let mut world = World::new(WorldConfig::default());
        let entity = scene::spawn(world.ecs_mut(), at(0.0), None).unwrap();
        world.step();
        if separate {
            world
                .edit_gameplay(|edit| edit.set_transform(entity, at(3.0)))
                .unwrap();
        }
        world
            .edit_gameplay(|edit| {
                if !separate {
                    edit.set_transform(entity, at(3.0))?;
                }
                edit.set_transform(entity, at(7.0))
            })
            .unwrap();
        assert_eq!(shown(&mut world, entity), 7.0);
        world.step();
        assert_eq!(shown(&mut world, entity), 7.0);
    }
}

#[test]
fn failed_pose_edit_preserves_the_prior_visible_commit_without_replay() {
    let mut world = World::new(WorldConfig::default());
    let entity = scene::spawn(world.ecs_mut(), at(0.0), None).unwrap();
    world.step();
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(4.0)))
        .unwrap();
    assert!(world
        .edit_gameplay(|edit| {
            edit.set_transform(entity, at(6.0))?;
            edit.set_transform(entity, at(f32::NAN))
        })
        .is_err());
    assert_eq!(shown(&mut world, entity), 4.0);
    world.edit_gameplay(|_| Ok(())).unwrap();
    world.begin_frame(0.0);
    world.step();
    assert_eq!(shown(&mut world, entity), 4.0);
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
    assert_eq!(shown(&mut world, entity), 4.0);
    world.step(); // Prepared state is consumed; the next tick records normally.
    assert_eq!(shown(&mut world, entity), 8.0);
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(12.0)))
        .unwrap();
    assert_eq!(shown(&mut world, entity), 12.0);
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
    assert_eq!(shown(&mut world, entity), 8.0);
    world.prepare_gameplay_tick();
    world
        .edit_gameplay(|edit| edit.set_transform(entity, at(12.0)))
        .unwrap();
    world.step();
    world.cancel_gameplay_tick(); // A consumed preparation is unaffected.
    assert_eq!(shown(&mut world, entity), 10.0);
}
