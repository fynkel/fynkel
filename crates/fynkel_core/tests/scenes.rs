//! Runtime scenes and trigger zones (blueprint §19.2d item 12): replace /
//! additive loads, unload, the registry, `SceneReady`/`SceneUnloaded`, and
//! `TriggerZone` events from a sensor collider.

use fynkel_core::*;
use glam::Vec3;

fn id(n: u8) -> Uuid {
    let mut b = [0u8; 16];
    b[15] = n;
    b[6] = 0x40;
    b[8] = 0x80;
    Uuid(b)
}

fn entity(n: u8, parent: Option<u8>, children: &[u8], at: Vec3) -> EntityDescription {
    EntityDescription {
        id: id(n),
        name: format!("e{n}"),
        active: true,
        parent: parent.map(id),
        children: children.iter().map(|c| id(*c)).collect(),
        components: vec![ComponentValue::Transform(Transform::from_position(at))],
    }
}

/// A root with a child, and a lone root: three entities.
fn scene(n: u8) -> SceneDescription {
    SceneDescription {
        id: id(n),
        name: format!("Scene {n}"),
        entities: vec![
            entity(
                n.wrapping_mul(10) + 1,
                None,
                &[n.wrapping_mul(10) + 2],
                Vec3::X,
            ),
            entity(
                n.wrapping_mul(10) + 2,
                Some(n.wrapping_mul(10) + 1),
                &[],
                Vec3::Y,
            ),
            entity(n.wrapping_mul(10) + 3, None, &[], Vec3::Z),
        ],
    }
}

fn world() -> World {
    World::new(WorldConfig::default())
}

fn kinds(events: &[Event]) -> Vec<EventKind> {
    events.iter().map(|e| e.payload.kind()).collect()
}

fn spawn(w: &mut World, _fixture_id: u64, at: Vec3, components: Vec<ComponentValue>) -> Entity {
    let commit = w
        .edit_gameplay(|edit| edit.spawn("", Transform::from_position(at), None))
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    w.drain_events();
    if !components.is_empty() {
        w.edit_gameplay(|edit| {
            for value in components {
                edit.set_component(entity, value)?;
            }
            Ok(())
        })
        .unwrap();
    }
    entity
}

#[test]
fn a_load_replaces_by_default_and_names_the_scene() {
    let mut w = world();
    let a = w.load_scene(&scene(1)).unwrap();
    assert_eq!(w.live_entities(), 3);
    assert_eq!(w.loaded_scenes().len(), 1);
    let ev = w.drain_events();
    assert_eq!(kinds(&ev), vec![EventKind::SCENE_READY]);
    assert_eq!(
        ev[0].payload,
        EventPayload::SceneReady { scene: id(1) },
        "the event names the scene"
    );
    // Something runtime spawned since (a behaviour's object).
    let extra = spawn(&mut w, 7, Vec3::ZERO, vec![]);
    assert_eq!(w.live_entities(), 4);
    assert!(w.scene_of(extra).is_none());
    assert_eq!(w.scene_of(a.entities()[1].1).map(|s| s.id), Some(id(1)));

    let b = w.load_scene(&scene(2)).unwrap();
    assert_eq!(
        w.live_entities(),
        3,
        "nothing of the previous state survives"
    );
    assert_eq!(w.loaded_scenes().len(), 1);
    assert_eq!(w.loaded_scenes()[0].id, id(2));
    assert!(!w.ecs().contains(extra));
    assert!(!w.ecs().contains(a.entities()[0].1));
    assert!(w.ecs().contains(b.entities()[0].1));
    let ev = w.drain_events();
    // The atomic commit publishes all destruction events, then unload
    // summaries and finally the new scene's readiness event.
    assert_eq!(
        kinds(&ev),
        vec![
            EventKind::DESTROYED,
            EventKind::DESTROYED,
            EventKind::DESTROYED,
            EventKind::DESTROYED,
            EventKind::SCENE_UNLOADED,
            EventKind::SCENE_READY,
        ]
    );
    assert_eq!(
        ev[4].payload,
        EventPayload::SceneUnloaded {
            scene: id(1),
            entities: 3
        }
    );
    // The world keeps ticking on the new scene.
    w.step();
    assert_eq!(w.live_entities(), 3);
}

#[test]
fn additive_loads_stack_and_refuse_a_loaded_id() {
    let mut w = world();
    w.load_scene(&scene(1)).unwrap();
    w.load_scene_with(&scene(2), LoadMode::Additive).unwrap();
    assert_eq!(w.live_entities(), 6);
    assert_eq!(
        w.loaded_scenes().iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![id(1), id(2)]
    );
    assert_eq!(
        w.load_scene_with(&scene(2), LoadMode::Additive),
        Err(LoadError::AlreadyLoaded(id(2)))
    );
    assert_eq!(w.live_entities(), 6, "refused before anything was spawned");
    let ev = w.drain_events();
    assert_eq!(
        kinds(&ev),
        vec![EventKind::SCENE_READY, EventKind::SCENE_READY]
    );
    // A structurally broken additive scene loads nothing and unloads
    // nothing either — and a broken replace load leaves the world alone.
    let mut bad = scene(3);
    bad.entities[1].parent = Some(id(200));
    let err = w.load_scene_with(&bad, LoadMode::Additive).unwrap_err();
    assert!(
        matches!(
            err,
            LoadError::UnknownParent { .. } | LoadError::ChildParentMismatch { .. }
        ),
        "{err}"
    );
    assert!(w.load_scene(&bad).is_err());
    assert_eq!(w.live_entities(), 6);
    assert_eq!(w.loaded_scenes().len(), 2);
    assert!(w.drain_events().is_empty());
}

#[test]
fn unload_takes_the_scenes_live_entities_and_leaves_the_rest() {
    let mut w = world();
    let a = w.load_scene(&scene(1)).unwrap();
    let b = w.load_scene_with(&scene(2), LoadMode::Additive).unwrap();
    let extra = spawn(&mut w, 9, Vec3::ZERO, vec![]);
    // Gameplay destroyed one of scene 1's roots already (its child goes
    // with it).
    w.edit_gameplay(|edit| {
        edit.destroy(a.entities()[0].1)?;
        Ok(())
    })
    .unwrap();
    w.step();
    w.drain_events();
    assert_eq!(w.live_entities(), 1 + 3 + 1);

    let gone = w.unload_scene(id(1)).unwrap();
    assert_eq!(
        gone,
        UnloadedScene {
            id: id(1),
            name: "Scene 1".into(),
            entities: 1
        },
        "only what was still alive"
    );
    assert_eq!(w.live_entities(), 4);
    assert!(w.ecs().contains(extra), "runtime spawns are nobody's");
    assert!(w.ecs().contains(b.entities()[0].1));
    assert_eq!(w.loaded_scenes().len(), 1);
    let ev = w.drain_events();
    assert_eq!(
        kinds(&ev),
        vec![EventKind::DESTROYED, EventKind::SCENE_UNLOADED]
    );
    assert_eq!(w.unload_scene(id(1)), Err(LoadError::NotLoaded(id(1))));
    assert_eq!(w.unload_scene(id(42)), Err(LoadError::NotLoaded(id(42))));
    assert!(w.drain_events().is_empty());
    // Loading it again is fine (fresh handles).
    let a2 = w.load_scene_with(&scene(1), LoadMode::Additive).unwrap();
    assert_ne!(a2.entities()[0].1, a.entities()[0].1);
    assert_eq!(w.live_entities(), 7);
}

/// ADR-0037: a persistent root and its subtree outlive a replace load and
/// the unload of the scene it came from; only roots can be persistent; a
/// persistent root given a parent stops being one.
#[test]
fn dont_destroy_on_load_keeps_a_root_across_loads() {
    let mut w = world();
    let a = w.load_scene(&scene(1)).unwrap();
    let (root, child) = (a.entities()[0].1, a.entities()[1].1);
    let extra = spawn(&mut w, 7, Vec3::ZERO, vec![]);
    w.drain_events();

    assert_eq!(
        w.dont_destroy_on_load(child),
        Err(LoadError::NotARoot(child)),
        "a child is refused, as Unity's warning"
    );
    w.dont_destroy_on_load(root).unwrap();
    w.dont_destroy_on_load(extra).unwrap();
    w.dont_destroy_on_load(extra).unwrap(); // twice is fine
    assert!(w.is_persistent(root) && w.is_persistent(child) && w.is_persistent(extra));
    assert_eq!(w.persistent_roots(), vec![root, extra]);

    // A child made later under the persistent root goes with it.
    w.edit_gameplay(|edit| edit.spawn("", Transform::default(), Some(root.into())))
        .unwrap();
    w.step();
    w.drain_events();
    assert_eq!(w.live_entities(), 5);

    let b = w.load_scene(&scene(2)).unwrap();
    assert!(w.ecs().contains(root) && w.ecs().contains(child) && w.ecs().contains(extra));
    assert!(
        !w.ecs().contains(a.entities()[2].1),
        "the scene's other root went"
    );
    assert_eq!(w.live_entities(), 4 + 3);
    let ev = w.drain_events();
    assert_eq!(
        kinds(&ev),
        vec![
            EventKind::DESTROYED,
            EventKind::SCENE_UNLOADED,
            EventKind::SCENE_READY
        ]
    );
    assert_eq!(
        ev[1].payload,
        EventPayload::SceneUnloaded {
            scene: id(1),
            entities: 1
        }
    );

    // Unloading the scene a persistent root came from leaves it too.
    let c = w.load_scene_with(&scene(3), LoadMode::Additive).unwrap();
    let c_root = c.entities()[0].1;
    w.dont_destroy_on_load(c_root).unwrap();
    let gone = w.unload_scene(id(3)).unwrap();
    assert_eq!(gone.entities, 1, "only the lone root");
    assert!(w.ecs().contains(c_root) && w.ecs().contains(c.entities()[1].1));

    // Parented under a scene object, it belongs to that scene's root now.
    w.edit_gameplay(|edit| {
        edit.set_parent(extra, Some(b.entities()[0].1.into()), false)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert!(!w.is_persistent(extra));
    // Destroyed, it is no longer listed.
    w.edit_gameplay(|edit| {
        edit.destroy(c_root)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert_eq!(w.persistent_roots(), vec![root]);

    w.load_scene(&scene(4)).unwrap();
    assert!(
        !w.ecs().contains(extra),
        "no longer persistent: it went with scene 2"
    );
    assert!(w.ecs().contains(root));
    assert_eq!(w.live_entities(), 3 + 3);
}

#[cfg(feature = "physics")]
#[test]
fn a_trigger_zone_fires_its_named_event_and_once_removes_it() {
    let mut w = world();
    let _floor = spawn(
        &mut w,
        1,
        Vec3::ZERO,
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(20.0, 0.5, 20.0),
            ..Collider::default()
        })],
    );
    let zone = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 1.5, 0.0),
        vec![
            ComponentValue::Collider(Collider {
                size: Vec3::new(1.0, 0.25, 1.0),
                is_sensor: true,
                ..Collider::default()
            }),
            ComponentValue::TriggerZone(TriggerZone {
                event: "collect".into(),
                once: false,
            }),
        ],
    );
    let ball = spawn(
        &mut w,
        3,
        Vec3::new(0.0, 4.0, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.25, 0.0, 0.0),
                ..Collider::default()
            }),
        ],
    );
    let mut zone_events = Vec::new();
    let mut triggers = 0;
    for _ in 0..240 {
        w.step();
        for e in w.drain_events() {
            match e.payload {
                EventPayload::ZoneEntered { .. } | EventPayload::ZoneExited { .. } => {
                    assert!(!e.payload.is_physics(), "general lane");
                    zone_events.push(e.payload.clone());
                }
                EventPayload::TriggerEnter(_) | EventPayload::TriggerExit(_) => triggers += 1,
                _ => {}
            }
        }
    }
    assert_eq!(triggers, 2, "the physics-lane events are still there");
    assert_eq!(
        zone_events,
        vec![
            EventPayload::ZoneEntered {
                zone,
                other: ball,
                event: "collect".into()
            },
            EventPayload::ZoneExited {
                zone,
                other: ball,
                event: "collect".into()
            },
        ]
    );
    assert!(w.ecs().has::<TriggerZone>(zone), "kept without `once`");

    // `once`: the entry fires, the component goes, no exit follows.
    let mut w = world();
    let zone = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 1.5, 0.0),
        vec![
            ComponentValue::Collider(Collider {
                size: Vec3::new(1.0, 0.25, 1.0),
                is_sensor: true,
                ..Collider::default()
            }),
            ComponentValue::TriggerZone(TriggerZone {
                event: "collect".into(),
                once: true,
            }),
        ],
    );
    let _ball = spawn(
        &mut w,
        3,
        Vec3::new(0.0, 4.0, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.25, 0.0, 0.0),
                ..Collider::default()
            }),
        ],
    );
    let mut zone_events = Vec::new();
    for _ in 0..240 {
        w.step();
        for e in w.drain_events() {
            if matches!(
                e.payload,
                EventPayload::ZoneEntered { .. } | EventPayload::ZoneExited { .. }
            ) {
                zone_events.push(e.payload.kind());
            }
        }
    }
    assert_eq!(zone_events, vec![EventKind::ZONE_ENTERED]);
    assert!(!w.ecs().has::<TriggerZone>(zone));
    assert!(w.ecs().has::<Collider>(zone), "the collider stays");
}

#[test]
fn a_trigger_zone_value_is_validated() {
    assert!(TriggerZone {
        event: "  ".into(),
        once: false
    }
    .validate()
    .is_err());
    assert!(TriggerZone::default().validate().is_ok());
    let mut w = world();
    let e = spawn(&mut w, 1, Vec3::ZERO, vec![]);
    assert!(matches!(
        w.edit_gameplay(|edit| edit.set_component(
            e,
            ComponentValue::TriggerZone(TriggerZone {
                event: String::new(),
                once: false
            })
        )),
        Err(EditError::InvalidComponent {
            component: ComponentId::TRIGGER_ZONE,
            ..
        })
    ));
    assert!(!w.ecs().has::<TriggerZone>(e));
    assert_eq!(w.step().rejected, 0);
}
