//! SetActive / activeInHierarchy (docs/design/set-active.md): an entity's
//! own flag, the derived hierarchy state after loads, spawns, reparents and
//! `SetActive`, and what an inactive subtree does — nothing: not drawn,
//! not simulated, not moved by the catalogue, not heard.

use fynkel_audio::{AudioClip, NULL_SAMPLE_RATE};
use fynkel_contracts::{AssetRef, Uuid};
use fynkel_core::components::ComponentValue;
use fynkel_core::*;
use fynkel_scene::{is_active_in_hierarchy, is_active_self};
use glam::Vec3;

fn id(n: u8) -> Uuid {
    let mut b = [0u8; 16];
    b[15] = n;
    b[6] = 0x40;
    b[8] = 0x80;
    Uuid(b)
}

fn world() -> World {
    World::new(WorldConfig::default())
}

fn drawn(n: u8, parent: Option<u8>, children: &[u8], active: bool) -> EntityDescription {
    EntityDescription {
        registered: Vec::new(),
        id: id(n),
        name: format!("e{n}"),
        active,
        parent: parent.map(id),
        children: children.iter().map(|c| id(*c)).collect(),
        components: vec![
            ComponentValue::Transform(Transform::from_position(Vec3::X * n as f32)),
            ComponentValue::MeshRenderer(MeshRenderer::default()),
        ],
    }
}

/// Root 1 (inactive when `root_active` is false) with child 2 and
/// grandchild 3; lone root 4.
fn scene(root_active: bool) -> SceneDescription {
    SceneDescription {
        id: id(100),
        name: "Active".into(),
        entities: vec![
            drawn(1, None, &[2], root_active),
            drawn(2, Some(1), &[3], true),
            drawn(3, Some(2), &[], true),
            drawn(4, None, &[], true),
        ],
    }
}

fn drawn_entities(w: &World) -> Vec<Entity> {
    let x = w.extract();
    x.meshes
        .iter()
        .map(|m| x.records[m.record as usize].entity)
        .collect()
}

fn set_active(w: &mut World, entity: Entity, active: bool) {
    let r = w.edit_gameplay(|edit| {
        edit.set_active(entity, active)?;
        Ok(())
    });
    assert!(r.is_ok());
    let r = w.step();
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
}

fn spawn(w: &mut World, _fixture_id: u64, parent: Option<Entity>) -> Entity {
    let commit = w
        .edit_gameplay(|edit| {
            edit.spawn(
                "",
                Transform::from_position(Vec3::new(0.0, 3.0, 0.0)),
                parent.map(Into::into),
            )
        })
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    w.drain_events();
    entity
}

fn set(w: &mut World, entity: Entity, value: ComponentValue) {
    w.edit_gameplay(|edit| {
        edit.set_component(entity, value)?;
        Ok(())
    })
    .unwrap();
    let r = w.step();
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
}

#[test]
fn a_loaded_inactive_root_hides_its_subtree_until_activated() {
    let mut w = world();
    let loaded = w.load_scene(&scene(false)).unwrap();
    let e = |n: u8| loaded.handle_of(id(n)).unwrap();
    // Straight after the load, before any step.
    assert!(!is_active_self(w.ecs(), e(1)));
    assert!(
        is_active_self(w.ecs(), e(2)),
        "the child's own flag is untouched"
    );
    for n in 1..=3 {
        assert!(!is_active_in_hierarchy(w.ecs(), e(n)), "e{n}");
    }
    assert!(is_active_in_hierarchy(w.ecs(), e(4)));
    assert_eq!(drawn_entities(&w), vec![e(4)]);

    // Activating the root brings back exactly the active children.
    set_active(&mut w, e(3), false);
    set_active(&mut w, e(1), true);
    assert_eq!(drawn_entities(&w), vec![e(1), e(2), e(4)]);
    assert!(!is_active_in_hierarchy(w.ecs(), e(3)));

    // Deactivating the middle takes the grandchild along; the flags stay.
    set_active(&mut w, e(3), true);
    set_active(&mut w, e(2), false);
    assert_eq!(drawn_entities(&w), vec![e(1), e(4)]);
    assert!(is_active_self(w.ecs(), e(3)));

    // Setting the same value again is a no-op; a stale entity is rejected.
    set_active(&mut w, e(2), false);
    w.edit_gameplay(|edit| {
        edit.destroy(e(4))?;
        Ok(())
    })
    .unwrap();
    w.step();
    let r = w.edit_gameplay(|edit| {
        edit.set_active(e(4), false)?;
        Ok(())
    });
    assert!(matches!(r, Err(EditError::StaleEntity(_))));
    assert_eq!(w.step().rejected, 0);
    w.edit_gameplay(|edit| edit.set_active(e(1), false))
        .unwrap();
    assert!(!is_active_self(w.ecs(), e(1)));
}

#[test]
fn spawning_or_reparenting_under_an_inactive_parent_deactivates() {
    let mut w = world();
    let parent = spawn(&mut w, 1, None);
    set_active(&mut w, parent, false);
    let child = spawn(&mut w, 2, Some(parent));
    assert!(!is_active_in_hierarchy(w.ecs(), child));
    let loose = spawn(&mut w, 3, None);
    assert!(is_active_in_hierarchy(w.ecs(), loose));
    w.edit_gameplay(|edit| {
        edit.set_parent(loose, Some(child.into()), false)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert!(!is_active_in_hierarchy(w.ecs(), loose));
    w.edit_gameplay(|edit| {
        edit.set_parent(loose, None, false)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert!(is_active_in_hierarchy(w.ecs(), loose));

    // A prefab instantiated under the inactive parent, with an inactive
    // entity of its own.
    let prefab = SceneDescription {
        id: id(50),
        name: "Prefab".into(),
        entities: vec![drawn(1, None, &[2], true), drawn(2, Some(1), &[], false)],
    };
    let (anchor, loaded) = w
        .instantiate(&prefab, "P", Transform::IDENTITY, None)
        .unwrap();
    let inner = loaded.handle_of(id(2)).unwrap();
    assert!(is_active_in_hierarchy(w.ecs(), anchor));
    assert!(!is_active_in_hierarchy(w.ecs(), inner));
    let (_, loaded) = w
        .instantiate(&prefab, "Q", Transform::IDENTITY, Some(parent))
        .unwrap();
    assert!(!is_active_in_hierarchy(
        w.ecs(),
        loaded.handle_of(id(1)).unwrap()
    ));
}

#[test]
fn an_inactive_rotator_does_not_turn() {
    let mut w = world();
    let e = spawn(&mut w, 1, None);
    set(
        &mut w,
        e,
        ComponentValue::Rotator(Rotator {
            axis: Vec3::Y,
            speed: 2.0,
            space: MotionSpace::Local,
        }),
    );
    set_active(&mut w, e, false);
    let before = w.ecs().copy::<Transform>(e).unwrap().rotation;
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(w.ecs().copy::<Transform>(e).unwrap().rotation, before);
    set_active(&mut w, e, true);
    assert_ne!(w.ecs().copy::<Transform>(e).unwrap().rotation, before);
}

#[test]
fn an_inactive_body_leaves_the_simulation_and_returns_at_rest() {
    let mut w = world();
    let e = spawn(&mut w, 1, None);
    w.edit_gameplay(|edit| {
        edit.set_component(e, ComponentValue::RigidBody(RigidBody::default()))?;
        edit.set_component(e, ComponentValue::Collider(Collider::default()))?;
        Ok(())
    })
    .unwrap();
    for _ in 0..10 {
        w.step();
    }
    assert!(w.physics().body_state(e).is_some());
    let y = |w: &World| w.ecs().copy::<Transform>(e).unwrap().position.y;
    set_active(&mut w, e, false);
    assert_eq!(w.physics().body_state(e), None, "out of the simulation");
    let held = y(&w);
    // A force on it is dropped, not rejected.
    w.edit_gameplay(|edit| {
        edit.body_effect(e, BodyCommand::ApplyImpulse(Vec3::Y))?;
        Ok(())
    })
    .unwrap();
    let r = w.step();
    assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(y(&w), held, "does not fall while inactive");
    set_active(&mut w, e, true);
    let state = w.physics().body_state(e).unwrap();
    assert!(state.linear_velocity.y > -1.0, "starts again at rest");
    assert!(y(&w) < held, "falls again");
}

#[test]
fn an_inactive_source_goes_quiet_and_a_looping_one_resumes() {
    let mut w = world();
    let clip = AssetRef::new(Uuid::from_bytes([1; 16]));
    w.register_sound(
        clip.clone(),
        AudioClip::new(NULL_SAMPLE_RATE, 1, vec![0.5; NULL_SAMPLE_RATE as usize]).unwrap(),
    )
    .unwrap();
    let source = |looping| AudioSource {
        clip: clip.clone(),
        bus: AudioBus::Sfx,
        volume: 1.0,
        pitch: 1.0,
        looping,
        playing: true,
        priority: 0,
    };
    let looping = spawn(&mut w, 1, None);
    let once = spawn(&mut w, 2, None);
    set(&mut w, looping, ComponentValue::AudioSource(source(true)));
    set(&mut w, once, ComponentValue::AudioSource(source(false)));
    assert_eq!(w.audio().voice_count(), 2);

    w.edit_gameplay(|edit| {
        edit.set_active(looping, false)?;
        edit.set_active(once, false)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert_eq!(w.audio().voice_count(), 0);
    let playing = |w: &World, e| w.ecs().get::<AudioSource>(e).unwrap().playing;
    assert!(playing(&w, looping));
    assert!(!playing(&w, once), "a one-shot is over");
    w.step();
    assert_eq!(w.audio().voice_count(), 0, "stays quiet");

    set_active(&mut w, looping, true);
    set_active(&mut w, once, true);
    assert_eq!(w.audio().voice_count(), 1, "the looping source again");
}

#[test]
fn one_batch_of_flag_and_parent_changes_settles_every_subtree() {
    // Foundation 3.4: each change refreshes only its own subtree, in the
    // batch's order — a descendant's change before its ancestor's must
    // still end right.
    let mut w = world();
    let top = spawn(&mut w, 1, None);
    let mid = spawn(&mut w, 2, Some(top));
    let leaf = spawn(&mut w, 3, Some(mid));
    let loose = spawn(&mut w, 4, None);
    let commit = w
        .edit_gameplay(|edit| {
            edit.set_parent(loose, Some(leaf.into()), true)?;
            edit.set_active(mid, false)?;
            edit.spawn("born", Transform::IDENTITY, Some(loose.into()))
        })
        .unwrap();
    let born = commit.resolve(commit.value).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    assert!(is_active_in_hierarchy(w.ecs(), top));
    for e in [mid, leaf, loose, born] {
        assert!(!is_active_in_hierarchy(w.ecs(), e), "{e:?}");
    }
    // And back on: the whole chain below `mid` returns.
    set_active(&mut w, mid, true);
    for e in [mid, leaf, loose, born] {
        assert!(is_active_in_hierarchy(w.ecs(), e), "{e:?}");
    }
}
