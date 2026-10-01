use fynkel_core::world::AudioRun;
use fynkel_core::*;
use glam::Vec3;

fn setup() -> (World, Entity) {
    let mut w = World::new(WorldConfig::default());
    let e = fynkel_scene::spawn(w.ecs_mut(), Transform::default(), None).unwrap();
    w.ecs_mut().insert(e, Name::new("original")).unwrap();
    (w, e)
}
fn layer(n: u32) -> ComponentValue {
    ComponentValue::Layer(Layer { layer: n })
}

macro_rules! roundtrip {
    ($($(#[$doc:meta])* $variant:ident($ty:ty) = $id:ident [$($field:ident),*];)*) => {
        #[test]
        fn catalogue_values_roundtrip_on_live_and_pending_objects() {
            $({
                let (mut w, live) = setup();
                let value = ComponentValue::$variant(<$ty>::default());
                let commit = w.edit_gameplay(|edit| {
                    let pending = edit.spawn("pending", Transform::default(), None)?;
                    for e in [GameplayEntity::from(live), pending.into()] {
                        edit.set_component(e, value.clone())?;
                        assert!(edit.has_component(e, ComponentId::$id)?);
                        assert_eq!(edit.component(e, ComponentId::$id)?, Some(value.clone().into()));
                    }
                    Ok(pending)
                }).unwrap();
                let pending = commit.resolve(commit.value).unwrap();
                for e in [live, pending] {
                    assert_eq!(*w.ecs().get::<$ty>(e).unwrap(), <$ty>::default());
                }
                if ComponentId::$id != ComponentId::TRANSFORM {
                    w.edit_gameplay(|edit| {
                        for e in [live, pending] {
                            edit.remove_component(e, ComponentId::$id)?;
                            assert!(!edit.has_component(e, ComponentId::$id)?);
                            assert_eq!(edit.component(e, ComponentId::$id)?, None);
                        }
                        Ok(())
                    }).unwrap();
                    for e in [live, pending] { assert!(!w.ecs().has::<$ty>(e)); }
                }
            })*
        }
    }
}
fynkel_core::builtin_components!(roundtrip);

#[test]
fn caught_validation_failure_rolls_back_components_and_structure() {
    let (mut w, e) = setup();
    let invalid = ComponentValue::AudioSource(AudioSource {
        volume: -1.0,
        ..Default::default()
    });
    let expected = EditError::InvalidComponent {
        entity: e.into(),
        component: ComponentId::AUDIO_SOURCE,
        message: invalid.validate().unwrap_err(),
    };
    assert_eq!(
        w.edit_gameplay(|edit| {
            edit.set_name(e, "changed")?;
            edit.set_component(e, layer(3))?;
            let p = edit.spawn("pending", Transform::default(), None)?;
            edit.set_parent(e, Some(p.into()), false)?;
            assert_eq!(edit.set_component(e, invalid), Err(expected.clone()));
            Ok(())
        }),
        Err(expected)
    );
    assert_eq!(w.live_entities(), 1);
    assert_eq!(w.ecs().get::<Name>(e).unwrap().value, "original");
    assert!(!w.ecs().has::<Layer>(e));
    assert_eq!(fynkel_scene::parent_of(w.ecs(), e), None);
}

#[test]
fn transform_component_and_helper_share_one_value_and_validation() {
    let (mut w, e) = setup();
    let a = Transform::from_position(Vec3::X);
    let b = Transform::from_position(Vec3::Y);
    w.edit_gameplay(|edit| {
        edit.set_component(e, ComponentValue::Transform(a))?;
        assert_eq!(edit.local_transform(e)?, a);
        edit.set_transform(e, b)?;
        assert_eq!(
            edit.component(e, ComponentId::TRANSFORM)?,
            Some(GameplayComponentValue::Transform(b))
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(*w.ecs().get::<Transform>(e).unwrap(), b);
    let mut invalid = a;
    invalid.rotation = glam::Quat::from_xyzw(0.0, 0.0, 0.0, 2.0);
    assert_eq!(
        w.edit_gameplay(|edit| edit.set_component(e, ComponentValue::Transform(invalid))),
        Err(EditError::InvalidTransform(e.into()))
    );
    assert_eq!(*w.ecs().get::<Transform>(e).unwrap(), b);
}

#[test]
fn protected_missing_unknown_and_helper_only_components_fail_atomically() {
    let (mut w, e) = setup();
    for id in [
        ComponentId::TRANSFORM,
        ComponentId::NAME,
        ComponentId::HIERARCHY,
    ] {
        assert_eq!(
            w.edit_gameplay(|edit| {
                edit.set_name(e, "bad")?;
                edit.remove_component(e, id)
            }),
            Err(EditError::ProtectedComponent(id))
        );
    }
    assert_eq!(
        w.edit_gameplay(|edit| edit.remove_component(e, ComponentId::LAYER)),
        Err(EditError::MissingComponent(e.into(), ComponentId::LAYER))
    );
    for id in [ComponentId::NAME, ComponentId::HIERARCHY] {
        assert_eq!(
            w.edit_gameplay(|edit| edit.component(e, id)),
            Err(EditError::UnsupportedComponent(id))
        );
        assert!(
            matches!(w.edit_gameplay(|edit| edit.query_with(&[id])), Err(EditError::UnsupportedComponent(c)) if c == id)
        );
    }
    let unknown = ComponentId(u32::MAX);
    assert_eq!(
        w.edit_gameplay(|edit| edit.has_component(e, unknown)),
        Err(EditError::UnknownComponent(unknown))
    );
    assert_eq!(w.ecs().get::<Name>(e).unwrap().value, "original");
}

#[test]
fn queries_freeze_membership_but_read_current_values() {
    let (mut w, e) = setup();
    w.edit_gameplay(|edit| {
        edit.set_component(e, layer(1))?;
        let removed = edit.spawn("removed", Transform::default(), None)?;
        let readded = edit.spawn("readded", Transform::default(), None)?;
        for p in [removed, readded] {
            edit.set_component(p, layer(1))?;
        }
        let mut query = edit.query_with(&[
            ComponentId::LAYER,
            ComponentId::TRANSFORM,
            ComponentId::LAYER,
        ])?;
        edit.set_component(e, layer(2))?;
        edit.remove_component(removed, ComponentId::LAYER)?;
        edit.remove_component(readded, ComponentId::LAYER)?;
        edit.set_component(readded, layer(3))?;
        let later = edit.spawn("later", Transform::default(), None)?;
        edit.set_component(later, layer(4))?;
        assert_eq!(query.next(edit)?, Some(e.into()));
        assert_eq!(
            edit.component(e, ComponentId::LAYER)?,
            Some(layer(2).into())
        );
        assert_eq!(query.next(edit)?, None);
        let mut fresh = edit.query_with(&[ComponentId::LAYER])?;
        let mut count = 0;
        while fresh.next(edit)?.is_some() {
            count += 1;
        }
        assert_eq!(count, 3);
        Ok(())
    })
    .unwrap();
}

#[test]
fn returned_values_are_owned_and_cancelled_spawns_publish_no_components() {
    let (mut w, e) = setup();
    let commit = w
        .edit_gameplay(|edit| {
            edit.set_component(e, layer(1))?;
            let mut copy = edit.component(e, ComponentId::LAYER)?.unwrap();
            if let GameplayComponentValue::Layer(ref mut v) = copy {
                v.layer = 9;
            }
            assert_eq!(
                edit.component(e, ComponentId::LAYER)?,
                Some(layer(1).into())
            );
            let p = edit.spawn("cancelled", Transform::default(), None)?;
            edit.set_component(p, layer(2))?;
            edit.destroy(p)?;
            Ok(p)
        })
        .unwrap();
    assert_eq!(commit.resolve(commit.value), None);
    assert_eq!(w.live_entities(), 1);
    assert_eq!(w.ecs().get::<Layer>(e).unwrap().layer, 1);
}

#[test]
fn audio_runtime_cleanup_happens_only_for_committed_removal() {
    let (mut w, e) = setup();
    w.ecs_mut().insert(e, AudioSource::default()).unwrap();
    w.ecs_mut()
        .insert(
            e,
            AudioRun {
                volume: 0.7,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        w.edit_gameplay::<()>(|edit| {
            edit.remove_component(e, ComponentId::AUDIO_SOURCE)?;
            Err(EditError::Aborted)
        }),
        Err(EditError::Aborted)
    );
    assert!(w.ecs().has::<AudioSource>(e));
    assert_eq!(w.ecs().get::<AudioRun>(e).unwrap().volume, 0.7);
    w.edit_gameplay(|edit| {
        edit.remove_component(e, ComponentId::AUDIO_SOURCE)?;
        edit.set_component(e, ComponentValue::AudioSource(AudioSource::default()))
    })
    .unwrap();
    assert_eq!(w.ecs().get::<AudioRun>(e).unwrap().volume, 0.7);
    w.edit_gameplay(|edit| edit.remove_component(e, ComponentId::AUDIO_SOURCE))
        .unwrap();
    assert!(!w.ecs().has::<AudioSource>(e));
    assert!(!w.ecs().has::<AudioRun>(e));
}

#[test]
fn committed_configuration_is_consumed_at_the_next_simulation_tick() {
    let (mut w, e) = setup();
    let tick = w.tick();
    w.edit_gameplay(|edit| edit.set_component(e, ComponentValue::Rotator(Rotator::default())))
        .unwrap();
    assert_eq!(w.tick(), tick);
    assert_eq!(
        w.ecs().get::<Transform>(e).unwrap().rotation,
        glam::Quat::IDENTITY
    );
    w.step();
    let rotated = w.ecs().get::<Transform>(e).unwrap().rotation;
    assert_ne!(rotated, glam::Quat::IDENTITY);
    w.edit_gameplay(|edit| edit.remove_component(e, ComponentId::ROTATOR))
        .unwrap();
    w.step();
    assert_eq!(w.ecs().get::<Transform>(e).unwrap().rotation, rotated);
}
