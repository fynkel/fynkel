//! `World::apply_gameplay_now` (ADR-0021 P7, live Play mode): the editor changes a
//! running or paused game at the current tick — the change is there at
//! once, nothing ticks, nothing is interpolated from the old value.

use fynkel_core::components::{ComponentValue, PointLight};
use fynkel_core::*;
use fynkel_scene::WorldTransform;
use glam::Vec3;

fn spawn(w: &mut World, fixture_id: u64, at: Vec3, parent: Option<Entity>) -> Entity {
    let commit = w
        .edit_gameplay(|edit| {
            edit.spawn(
                format!("e{fixture_id}"),
                Transform::from_position(at),
                parent.map(Into::into),
            )
        })
        .unwrap();
    commit.resolve(commit.value).unwrap()
}

fn world_pos(w: &World, e: Entity) -> Vec3 {
    w.ecs()
        .copy::<WorldTransform>(e)
        .unwrap()
        .matrix
        .transform_point3(Vec3::ZERO)
}

#[test]
fn a_live_edit_is_there_at_once_and_nothing_ticks() {
    let mut w = World::new(WorldConfig::default());
    let parent = spawn(&mut w, 1, Vec3::ZERO, None);
    let child = spawn(&mut w, 2, Vec3::new(0.0, 1.0, 0.0), Some(parent));
    let tick = w.tick();

    let rejected = w.apply_gameplay_now(vec![
        Command::SetTransform {
            entity: parent,
            transform: Transform::from_position(Vec3::new(5.0, 0.0, 0.0)),
        },
        Command::SetName {
            entity: child,
            name: "Renamed".into(),
        },
        Command::SetComponent {
            entity: child,
            value: ComponentValue::PointLight(PointLight {
                intensity: 7.0,
                ..PointLight::default()
            }),
        },
    ]);
    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(w.tick(), tick, "the tick does not advance");
    assert_eq!(world_pos(&w, child), Vec3::new(5.0, 1.0, 0.0), "propagated");
    assert_eq!(w.ecs().copy::<Name>(child).unwrap().value, "Renamed");
    assert!(w.ecs().has::<PointLight>(child));
    assert!(w.drain_events().is_empty(), "an edit is not an event");

    // Not interpolated from the old place: the frame shows the new one.
    let _ = w.begin_frame(0.0);
    let frame = w.extract();
    let drawn = frame
        .records
        .iter()
        .find(|r| r.entity == child)
        .expect("the child is drawn")
        .world
        .transform_point3(Vec3::ZERO);
    assert_eq!(drawn, Vec3::new(5.0, 1.0, 0.0));

    // And it stays through the next tick.
    assert_eq!(w.step().status, Status::Ok);
    assert_eq!(world_pos(&w, child), Vec3::new(5.0, 1.0, 0.0));
}

#[test]
fn a_bad_live_edit_is_refused_and_changes_nothing() {
    let mut w = World::new(WorldConfig::default());
    let e = spawn(&mut w, 1, Vec3::ZERO, None);
    let rejected = w.apply_gameplay_now(vec![
        Command::SetName {
            entity: e,
            name: "discarded".into(),
        },
        Command::SetTransform {
            entity: e,
            transform: Transform::from_position(Vec3::new(f32::NAN, 0.0, 0.0)),
        },
    ]);
    assert_eq!(rejected.len(), 1, "the entire edit rejects: {rejected:?}");
    assert_eq!(world_pos(&w, e), Vec3::ZERO);
    assert_eq!(w.ecs().len(), 1, "nothing spawned");
}

fn uuid(n: u8) -> Uuid {
    Uuid([n; 16])
}

#[test]
fn a_live_spawn_is_there_at_once_and_joins_the_scene_under_its_id() {
    let mut w = World::new(WorldConfig::default());
    let scene = SceneDescription {
        id: uuid(1),
        name: "Main".into(),
        entities: vec![EntityDescription {
            registered: Vec::new(),
            id: uuid(2),
            name: "Root".into(),
            active: true,
            parent: None,
            children: vec![],
            components: vec![ComponentValue::Transform(Transform::from_position(Vec3::X))],
        }],
    };
    w.load_scene(&scene).unwrap();
    w.drain_events();
    let root = w.loaded_scenes()[0].handle_of(uuid(2)).unwrap();
    let tick = w.tick();

    let (e, rejected) = w
        .spawn_now(
            "Lamp",
            Transform::from_position(Vec3::Y),
            Some(root),
            vec![ComponentValue::PointLight(PointLight {
                intensity: 3.0,
                ..PointLight::default()
            })],
            Some((uuid(1), uuid(3))),
        )
        .unwrap();
    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(w.tick(), tick, "nothing ticks");
    assert!(w.drain_events().is_empty(), "no event for the edit");
    assert_eq!(
        world_pos(&w, e),
        Vec3::new(1.0, 1.0, 0.0),
        "propagated under its parent"
    );
    assert_eq!(w.ecs().copy::<PointLight>(e).unwrap().intensity, 3.0);
    assert_eq!(w.loaded_scenes()[0].handle_of(uuid(3)), Some(e));
    assert_eq!(w.scene_of(e).map(|s| s.id), Some(uuid(1)));

    assert_eq!(
        w.spawn_now(
            "Again",
            Transform::default(),
            None,
            vec![],
            Some((uuid(1), uuid(3)))
        )
        .unwrap_err(),
        LoadError::DuplicateEntity(uuid(3))
    );
    assert_eq!(
        w.spawn_now(
            "Nowhere",
            Transform::default(),
            None,
            vec![],
            Some((uuid(9), uuid(4)))
        )
        .unwrap_err(),
        LoadError::NotLoaded(uuid(9))
    );
    let (runtime, _) = w
        .spawn_now("", Transform::default(), None, vec![], None)
        .unwrap();
    assert!(w.scene_of(runtime).is_none());
    assert_eq!(w.ecs().len(), 3);
}

fn empty_loaded_world() -> World {
    let mut world = World::new(WorldConfig::default());
    world
        .load_scene(&SceneDescription {
            id: uuid(1),
            name: "Main".into(),
            entities: vec![],
        })
        .unwrap();
    world.drain_events();
    world
}

#[test]
fn live_spawn_component_failure_preserves_scene_identity_and_can_retry() {
    let mut w = empty_loaded_world();
    let tick = w.tick();
    let bad = ComponentValue::PointLight(PointLight {
        intensity: f32::NAN,
        ..PointLight::default()
    });
    let error = w
        .spawn_now(
            "partial",
            Transform::default(),
            None,
            vec![ComponentValue::Rotator(Rotator::default()), bad],
            Some((uuid(1), uuid(2))),
        )
        .unwrap_err();
    assert!(matches!(error, LoadError::LiveSpawnRejected(_)));
    assert_eq!(w.live_entities(), 0);
    assert!(w.loaded_scenes()[0].handle_of(uuid(2)).is_none());
    assert!(w.drain_events().is_empty());
    assert_eq!(w.tick(), tick);
    let (e, rejected) = w
        .spawn_now(
            "retry",
            Transform::default(),
            None,
            vec![],
            Some((uuid(1), uuid(2))),
        )
        .unwrap();
    assert!(rejected.is_empty());
    assert_eq!(w.loaded_scenes()[0].handle_of(uuid(2)), Some(e));
    w.unload_scene(uuid(1)).unwrap();
    assert!(!w.ecs().contains(e));
}

#[test]
fn live_spawn_rejects_stale_references_without_publishing_hierarchy() {
    let mut w = empty_loaded_world();
    let (parent, _) = w
        .spawn_now("parent", Transform::default(), None, vec![], None)
        .unwrap();
    let (stale, _) = w
        .spawn_now("stale", Transform::default(), None, vec![], None)
        .unwrap();
    w.edit_gameplay(|edit| edit.destroy(stale)).unwrap();
    w.drain_events();
    let error = w
        .spawn_now(
            "invalid follower",
            Transform::default(),
            Some(parent),
            vec![ComponentValue::Follow(Follow {
                target: fynkel_contracts::ObjectRef::to(stale),
                ..Follow::default()
            })],
            Some((uuid(1), uuid(2))),
        )
        .unwrap_err();
    assert!(matches!(error, LoadError::LiveSpawnRejected(_)));
    assert_eq!(w.live_entities(), 1);
    assert_eq!(fynkel_scene::children_of(w.ecs(), parent).len(), 0);
    assert!(w.loaded_scenes()[0].handle_of(uuid(2)).is_none());
    assert!(w.drain_events().is_empty());
}

#[test]
fn live_spawn_preserves_existing_committed_identity_without_a_tick() {
    let mut w = empty_loaded_world();
    let c = w
        .edit_gameplay(|e| e.spawn("gameplay", Transform::default(), None))
        .unwrap();
    let existing = c.resolve(c.value).unwrap();
    let tick = w.tick();
    let (editor, diagnostics) = w
        .spawn_now(
            "editor",
            Transform::default(),
            None,
            vec![],
            Some((uuid(1), uuid(2))),
        )
        .unwrap();
    assert!(diagnostics.is_empty());
    assert!(w.ecs().contains(existing));
    assert_ne!(existing, editor);
    assert_eq!(w.live_entities(), 2);
    assert_eq!(w.tick(), tick);
    assert_eq!(w.loaded_scenes()[0].handle_of(uuid(2)), Some(editor));
}
