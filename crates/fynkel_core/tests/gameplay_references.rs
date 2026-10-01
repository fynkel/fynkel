use fynkel_contracts::{HasSchema, ObjectRef, Uuid};
use fynkel_core::*;
use glam::Vec3;

fn world() -> World {
    World::new(WorldConfig::default())
}
fn spawn(w: &mut World, name: &str) -> Entity {
    let c = w
        .edit_gameplay(|edit| edit.spawn(name, Transform::default(), None))
        .unwrap();
    c.resolve(c.value).unwrap()
}
fn follow<R: Into<GameplayObjectRef>>(target: R) -> GameplayComponentValue {
    let target = target.into();
    GameplayComponentValue::Follow(Follow::default().map_object_refs(|_| target))
}
fn camera<R: Into<GameplayObjectRef>>(target: R) -> GameplayComponentValue {
    let target = target.into();
    GameplayComponentValue::ThirdPersonCamera(
        ThirdPersonCamera::default().map_object_refs(|_| target),
    )
}
fn skin<R: Into<GameplayObjectRef>>(target: R) -> GameplayComponentValue {
    let target = target.into();
    GameplayComponentValue::Skin(<Skin>::default().map_object_refs(|_| target))
}
fn reference(value: GameplayComponentValue) -> GameplayObjectRef {
    match value {
        GameplayComponentValue::Follow(v) => v.target,
        GameplayComponentValue::ThirdPersonCamera(v) => v.target,
        GameplayComponentValue::Skin(v) => v.skeleton,
        _ => panic!("expected reference-bearing component"),
    }
}
const IDS: [ComponentId; 3] = [
    ComponentId::FOLLOW,
    ComponentId::THIRD_PERSON_CAMERA,
    ComponentId::SKIN,
];

#[test]
fn all_reference_families_share_pending_reads_queries_and_commit_resolution() {
    let mut w = world();
    let live = spawn(&mut w, "live");
    let c = w
        .edit_gameplay(|edit| {
            let owner = edit.spawn("owner", Transform::default(), None)?;
            let target = edit.spawn("target", Transform::default(), None)?;
            for e in [GameplayEntity::from(live), owner.into()] {
                for value in [follow(target), camera(target), skin(target)] {
                    let id = value.id();
                    edit.set_component(e, value.clone())?;
                    assert_eq!(edit.component(e, id)?, Some(value));
                }
            }
            let mut q = edit.query_with(&IDS)?;
            assert_eq!(q.next(edit)?, Some(live.into()));
            assert_eq!(q.next(edit)?, Some(owner.into()));
            assert_eq!(q.next(edit)?, None);
            Ok((owner, target))
        })
        .unwrap();
    let owner = c.resolve(c.value.0).unwrap();
    let target = c.resolve(c.value.1).unwrap();
    for e in [live, owner] {
        assert_eq!(
            w.ecs().get::<Follow>(e).unwrap().target,
            ObjectRef::to(target)
        );
        assert_eq!(
            w.ecs().get::<ThirdPersonCamera>(e).unwrap().target,
            ObjectRef::to(target)
        );
        assert_eq!(
            w.ecs().get::<Skin>(e).unwrap().skeleton,
            ObjectRef::to(target)
        );
        w.edit_gameplay(|edit| {
            for id in IDS {
                assert_eq!(reference(edit.component(e, id)?.unwrap()), target.into());
            }
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn forward_self_and_cyclic_pending_references_resolve_after_allocation() {
    let mut w = world();
    let c = w
        .edit_gameplay(|edit| {
            let a = edit.spawn("a", Transform::default(), None)?;
            let b = edit.spawn("b", Transform::default(), None)?;
            edit.set_component(a, follow(b))?;
            edit.set_component(b, follow(a))?;
            edit.set_component(a, skin(a))?;
            Ok((a, b))
        })
        .unwrap();
    let a = c.resolve(c.value.0).unwrap();
    let b = c.resolve(c.value.1).unwrap();
    assert_eq!(w.ecs().get::<Follow>(a).unwrap().target.get(), Some(b));
    assert_eq!(w.ecs().get::<Follow>(b).unwrap().target.get(), Some(a));
    assert_eq!(w.ecs().get::<Skin>(a).unwrap().skeleton.get(), Some(a));
}

#[test]
fn expired_foreign_and_stale_targets_fail_stickily_without_publication() {
    let mut w = world();
    let owner = spawn(&mut w, "owner");
    let c = w
        .edit_gameplay(|edit| edit.spawn("old pending", Transform::default(), None))
        .unwrap();
    let expired = c.value;
    let mut other = world();
    let foreign = spawn(&mut other, "foreign");
    let dead = spawn(&mut w, "dead");
    w.edit_gameplay(|edit| edit.destroy(dead)).unwrap();
    for (target, expected) in [
        (
            GameplayObjectRef::from(expired),
            EditError::ExpiredPending(expired),
        ),
        (foreign.into(), EditError::StaleEntity(foreign)),
        (dead.into(), EditError::StaleEntity(dead)),
    ] {
        for value in [follow(target), camera(target), skin(target)] {
            assert_eq!(
                w.edit_gameplay(|edit| {
                    edit.set_name(owner, "must rollback")?;
                    assert_eq!(edit.set_component(owner, value), Err(expected.clone()));
                    Ok(())
                }),
                Err(expected.clone())
            );
        }
    }
    assert_eq!(w.ecs().get::<Name>(owner).unwrap().value, "owner");
    assert!(!w.ecs().has::<Follow>(owner));
    // A token from an aborted transaction cannot be resurrected either.
    let mut abandoned = None;
    let _ = w.edit_gameplay::<()>(|edit| {
        abandoned = Some(edit.spawn("aborted", Transform::default(), None)?);
        Err(EditError::Aborted)
    });
    let p = abandoned.unwrap();
    assert_eq!(
        w.edit_gameplay(|edit| edit.set_component(owner, follow(p))),
        Err(EditError::ExpiredPending(p))
    );
}

#[test]
fn retiring_a_staged_target_aborts_before_allocation_for_every_family() {
    for make in [
        follow::<GameplayObjectRef>,
        camera::<GameplayObjectRef>,
        skin::<GameplayObjectRef>,
    ] {
        for pending in [false, true] {
            let mut w = world();
            let owner = spawn(&mut w, "owner");
            let live = spawn(&mut w, "target");
            let mut target = GameplayEntity::from(live);
            let result = w.edit_gameplay(|edit| {
                edit.set_name(owner, "changed")?;
                if pending {
                    target = edit
                        .spawn("pending target", Transform::default(), None)?
                        .into();
                }
                edit.spawn("must not allocate", Transform::default(), None)?;
                edit.set_component(owner, make(target.into()))?;
                edit.destroy(target)?;
                Ok(())
            });
            assert_eq!(result, Err(EditError::Destroyed(target)));
            assert_eq!(w.live_entities(), 2);
            assert_eq!(w.ecs().get::<Name>(owner).unwrap().value, "owner");
            assert!(!w.ecs().has::<Follow>(owner));
            assert!(!w.ecs().has::<ThirdPersonCamera>(owner));
            assert!(!w.ecs().has::<Skin>(owner));
            assert_eq!(spawn(&mut w, "next").slot(), live.slot() + 1);
        }
    }
}

#[test]
fn removed_components_and_retired_owners_do_not_keep_pending_targets_alive() {
    let mut w = world();
    let owner = spawn(&mut w, "owner");
    w.edit_gameplay(|edit| {
        let target = edit.spawn("target", Transform::default(), None)?;
        let retired = edit.spawn("retired", Transform::default(), None)?;
        for value in [follow(target), camera(target), skin(target)] {
            let id = value.id();
            edit.set_component(owner, value.clone())?;
            edit.set_component(retired, value)?;
            edit.remove_component(owner, id)?;
        }
        edit.destroy(target)?;
        edit.destroy(retired)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(w.live_entities(), 1);
    assert!(!w.ecs().has::<Follow>(owner));
}

#[test]
fn final_reference_replacement_can_clear_or_retarget_a_retired_pending_object() {
    let mut w = world();
    let owner = spawn(&mut w, "owner");
    let live = spawn(&mut w, "live");
    w.edit_gameplay(|edit| {
        let target = edit.spawn("target", Transform::default(), None)?;
        edit.set_component(owner, follow(target))?;
        edit.set_component(owner, camera(target))?;
        edit.destroy(target)?;
        edit.set_component(owner, follow(GameplayObjectRef::NONE))?;
        edit.set_component(owner, camera(live))?;
        Ok(())
    })
    .unwrap();
    assert!(w.ecs().get::<Follow>(owner).unwrap().target.is_none());
    assert_eq!(
        w.ecs()
            .get::<ThirdPersonCamera>(owner)
            .unwrap()
            .target
            .get(),
        Some(live)
    );
}

#[test]
fn stored_authoring_metadata_survives_but_unresolved_authoring_ids_cannot_be_written() {
    let mut w = world();
    let owner = spawn(&mut w, "owner");
    let target = spawn(&mut w, "target");
    let id = Uuid::from_bytes([7; 16]);
    let stored = ObjectRef { entity: target, id };
    w.edit_gameplay(|edit| {
        edit.set_component(
            owner,
            ComponentValue::Follow(Follow {
                target: stored,
                ..Default::default()
            }),
        )
    })
    .unwrap();
    w.edit_gameplay(|edit| {
        let value = edit.component(owner, ComponentId::FOLLOW)?.unwrap();
        assert_eq!(reference(value.clone()), GameplayObjectRef::Stored(stored));
        edit.set_component(owner, value)
    })
    .unwrap();
    assert_eq!(w.ecs().get::<Follow>(owner).unwrap().target, stored);
    assert_eq!(
        w.edit_gameplay(|edit| edit.set_component(owner, follow(ObjectRef::authored(id)))),
        Err(EditError::UnresolvedObjectReference(id))
    );
    assert_eq!(w.ecs().get::<Follow>(owner).unwrap().target, stored);
}

#[test]
fn existing_references_remain_weak_after_target_destruction_and_never_rebind() {
    let mut w = world();
    let owner = spawn(&mut w, "owner");
    let target = spawn(&mut w, "target");
    w.edit_gameplay(|edit| edit.set_component(owner, follow(target)))
        .unwrap();
    w.edit_gameplay(|edit| {
        edit.destroy(target)?;
        let r = reference(edit.component(owner, ComponentId::FOLLOW)?.unwrap());
        assert_eq!(r.entity(), Some(target.into()));
        assert!(!edit.contains(r.entity().unwrap())?);
        Ok(())
    })
    .unwrap();
    let replacement = spawn(&mut w, "replacement");
    assert_ne!(replacement, target);
    assert_eq!(
        w.ecs().get::<Follow>(owner).unwrap().target.get(),
        Some(target)
    );
    w.step();
    assert_eq!(
        w.ecs().get::<Transform>(owner).unwrap().position,
        Vec3::ZERO
    );
}

#[test]
fn pending_follow_configuration_reaches_the_existing_simulation_consumer() {
    let mut w = world();
    let c = w
        .edit_gameplay(|edit| {
            let owner = edit.spawn("owner", Transform::default(), None)?;
            let target = edit.spawn("target", Transform::from_position(Vec3::X * 4.0), None)?;
            edit.set_component(owner, follow(target))?;
            Ok(owner)
        })
        .unwrap();
    let owner = c.resolve(c.value).unwrap();
    assert_eq!(
        w.ecs().get::<Transform>(owner).unwrap().position,
        Vec3::ZERO
    );
    w.step();
    assert_eq!(
        w.ecs().get::<Transform>(owner).unwrap().position,
        Vec3::new(4.0, 2.0, 0.0)
    );
}

#[test]
fn generic_reference_data_retains_the_native_schema_and_scalar_validation() {
    assert_eq!(Follow::SCHEMA.id, ComponentId::FOLLOW.0);
    assert_eq!(
        ThirdPersonCamera::SCHEMA.id,
        ComponentId::THIRD_PERSON_CAMERA.0
    );
    assert_eq!(Skin::SCHEMA.id, ComponentId::SKIN.0);
    let native = Follow {
        smoothing: -1.0,
        ..Default::default()
    };
    let scoped = native.clone().map_object_refs(GameplayObjectRef::from);
    assert_eq!(native.validate(), scoped.validate());
    assert_eq!(
        ComponentValue::Follow(native).validate(),
        GameplayComponentValue::Follow(scoped).validate()
    );
}

#[test]
fn reference_conversion_preserves_nondefault_configuration_and_validator_failures() {
    let mut w = world();
    let owner = spawn(&mut w, "owner");
    let target = spawn(&mut w, "target");
    let expected = ThirdPersonCamera {
        target: ObjectRef::to(target),
        pivot: Vec3::new(1.0, 2.0, 3.0),
        distance: 9.0,
        min_distance: 1.0,
        yaw: 0.7,
        pitch: 0.2,
        min_pitch: -0.6,
        max_pitch: 1.2,
        look_action: "orbit".into(),
        sensitivity: 3.0,
        collision_radius: 0.4,
        mask: 17,
    };
    let scoped = GameplayComponentValue::from(ComponentValue::ThirdPersonCamera(expected.clone()));
    w.edit_gameplay(|edit| edit.set_component(owner, scoped))
        .unwrap();
    assert_eq!(*w.ecs().get::<ThirdPersonCamera>(owner).unwrap(), expected);
    let mut invalid = expected.clone().map_object_refs(GameplayObjectRef::from);
    invalid.distance = -1.0;
    let value = GameplayComponentValue::ThirdPersonCamera(invalid);
    let error = EditError::InvalidComponent {
        entity: owner.into(),
        component: ComponentId::THIRD_PERSON_CAMERA,
        message: value.validate().unwrap_err(),
    };
    assert_eq!(
        w.edit_gameplay(|edit| {
            edit.spawn("must not allocate", Transform::default(), None)?;
            edit.set_component(owner, value)
        }),
        Err(error)
    );
    assert_eq!(w.live_entities(), 2);
    assert_eq!(*w.ecs().get::<ThirdPersonCamera>(owner).unwrap(), expected);
}
