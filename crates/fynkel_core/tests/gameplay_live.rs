use fynkel_contracts::ObjectRef;
use fynkel_core::*;
use glam::Vec3;

fn spawn(w: &mut World, name: &str) -> Entity {
    let commit = w
        .edit_gameplay(|edit| edit.spawn(name, Transform::default(), None))
        .unwrap();
    commit.resolve(commit.value).unwrap()
}

#[test]
fn live_batch_is_atomic_and_uses_final_reference_validation() {
    let mut w = World::new(WorldConfig::default());
    let owner = spawn(&mut w, "owner");
    let target = spawn(&mut w, "target");
    let rejected = w.apply_gameplay_now(vec![
        Command::SetName {
            entity: owner,
            name: "changed".into(),
        },
        Command::SetComponent {
            entity: owner,
            value: ComponentValue::Follow(Follow {
                target: ObjectRef::to(target),
                ..Follow::default()
            }),
        },
        Command::Destroy(target),
    ]);
    assert_eq!(rejected.len(), 1);
    assert_eq!(w.ecs().copy::<Name>(owner).unwrap().value, "owner");
    assert!(!w.ecs().has::<Follow>(owner));
    assert!(w.ecs().contains(target));
    assert_eq!(rejected[0].command_index, u32::MAX);
}

#[test]
fn live_batch_refreshes_paused_render_and_preserves_tick() {
    let mut w = World::new(WorldConfig::default());
    let owner = spawn(&mut w, "owner");
    let tick = w.tick();
    assert!(w
        .apply_gameplay_now(vec![Command::SetTransform {
            entity: owner,
            transform: Transform::from_position(Vec3::X * 7.0),
        }])
        .is_empty());
    assert_eq!(w.tick(), tick);
    w.begin_frame(0.0);
    let frame = w.extract();
    let drawn = frame.records.iter().find(|r| r.entity == owner).unwrap();
    assert_eq!(drawn.world.transform_point3(Vec3::ZERO), Vec3::X * 7.0);
    assert_eq!(w.tick(), tick);
}

#[test]
fn live_batch_never_publishes_or_consumes_older_queued_work() {
    let mut w = World::new(WorldConfig::default());
    let owner = spawn(&mut w, "owner");
    w.submit(
        Lane::Simulation,
        vec![Command::SetName {
            entity: owner,
            name: "queued".into(),
        }],
    );
    let rejected = w.apply_gameplay_now(vec![Command::SetActive {
        entity: owner,
        active: false,
    }]);
    assert_eq!(rejected.len(), 1);
    assert!(rejected[0].message.contains("PendingCommands"));
    assert_eq!(w.ecs().copy::<Name>(owner).unwrap().value, "owner");
    assert!(fynkel_scene::is_active_self(w.ecs(), owner));
    w.step();
    assert_eq!(w.ecs().copy::<Name>(owner).unwrap().value, "queued");
    assert!(fynkel_scene::is_active_self(w.ecs(), owner));
}

#[test]
fn live_data_boundary_rejects_runtime_actions_without_partial_changes() {
    let mut w = World::new(WorldConfig::default());
    let owner = spawn(&mut w, "owner");
    let rejected = w.apply_gameplay_now(vec![
        Command::SetName {
            entity: owner,
            name: "changed".into(),
        },
        Command::Spawn {
            temp_id: 1,
            transform: Transform::default(),
            parent: None,
            name: None,
        },
    ]);
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].command_index, 1);
    assert_eq!(w.ecs().len(), 1);
    assert_eq!(w.ecs().copy::<Name>(owner).unwrap().value, "owner");
}
