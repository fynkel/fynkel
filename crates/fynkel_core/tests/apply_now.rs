//! `World::apply_now` (ADR-0021 P7, live Play mode): the editor changes a
//! running or paused game at the current tick — the change is there at
//! once, nothing ticks, nothing is interpolated from the old value.

use fynkel_core::components::{ComponentValue, PointLight};
use fynkel_core::*;
use fynkel_scene::WorldTransform;
use glam::Vec3;

fn spawn(w: &mut World, temp_id: u64, at: Vec3, parent: Option<Entity>) -> Entity {
    w.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id,
            transform: Transform::from_position(at),
            parent,
            name: Some(format!("e{temp_id}")),
        }],
    );
    assert_eq!(w.step().status, Status::Ok, "{:?}", w.diagnostics());
    w.drain_events()
        .into_iter()
        .find_map(|e| match e.payload {
            EventPayload::Spawned { temp_id: t, entity } if t == temp_id => Some(entity),
            _ => None,
        })
        .unwrap()
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

    let rejected = w.apply_now(vec![
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
    let rejected = w.apply_now(vec![
        Command::Spawn {
            temp_id: 9,
            transform: Transform::default(),
            parent: None,
            name: None,
        },
        Command::SetTransform {
            entity: e,
            transform: Transform::from_position(Vec3::new(f32::NAN, 0.0, 0.0)),
        },
    ]);
    assert_eq!(rejected.len(), 2, "{rejected:?}");
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
            id: uuid(2),
            name: "Root".into(),
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
