use fynkel_core::{Transform, World, WorldConfig};
use glam::Vec3;

#[test]
fn callback_changes_ignore_pose_traffic_and_cover_native_activity_and_hierarchy() {
    let mut world = World::new(WorldConfig::default());
    let parent = fynkel_scene::spawn(world.ecs_mut(), Transform::IDENTITY, None).unwrap();
    let child = fynkel_scene::spawn(world.ecs_mut(), Transform::IDENTITY, None).unwrap();
    world
        .edit_gameplay(|edit| edit.set_name(child, "child"))
        .unwrap();
    let initial = world.callback_changes_since(0).unwrap();
    assert!(initial.reset);
    let token = initial.cursor.token();
    // Beyond journal capacity: unrelated dense writes must not cause a reset.
    for i in 0..20_000 {
        world.ecs().get_mut::<Transform>(child).unwrap().position = Vec3::splat(i as f32);
    }
    let clean = world.callback_changes_since(token).unwrap();
    assert!(!clean.reset);
    assert!(clean.entities.is_empty());
    let token = clean.cursor.token();
    world
        .edit_gameplay(|edit| edit.set_active(parent, false))
        .unwrap();
    let inactive = world.callback_changes_since(token).unwrap();
    assert!(!inactive.reset);
    assert!(inactive.entities.contains(&parent));
    let token = inactive.cursor.token();
    world
        .edit_gameplay(|edit| edit.set_parent(child, Some(parent.into()), false))
        .unwrap();
    let reparented = world.callback_changes_since(token).unwrap();
    assert!(!reparented.reset);
    assert!(reparented.entities.contains(&child));
    let token = reparented.cursor.token();
    world
        .edit_gameplay(|edit| edit.set_active(parent, true))
        .unwrap();
    for _ in 0..20_000 {
        world.ecs().get_mut::<Transform>(child).unwrap().position.x += 1.0;
    }
    let overflow = world.callback_changes_since(token).unwrap();
    assert!(
        !overflow.reset,
        "activity membership survives in structural history"
    );
    assert!(overflow.entities.contains(&parent));
    assert!(overflow.entities.contains(&child));
    let token = overflow.cursor.token();
    // An overwritten name is a value change, not recoverable membership.
    world
        .edit_gameplay(|edit| edit.set_name(child, "renamed"))
        .unwrap();
    for _ in 0..20_000 {
        world.ecs().get_mut::<Transform>(child).unwrap().position.x += 1.0;
    }
    let lost_name = world.callback_changes_since(token).unwrap();
    assert!(
        lost_name.reset,
        "evicted relevant values still force reconciliation"
    );
    let token = lost_name.cursor.token();
    world.edit_gameplay(|edit| edit.destroy(parent)).unwrap();
    let destroyed = world.callback_changes_since(token).unwrap();
    assert!(destroyed.entities.contains(&parent));
    assert!(destroyed.entities.contains(&child));
}
