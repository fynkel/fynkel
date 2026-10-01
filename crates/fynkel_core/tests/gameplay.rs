//! First production transaction-boundary slice: existing entity data only.
use std::panic::{catch_unwind, AssertUnwindSafe};

use fynkel_core::*;
use fynkel_scene::{self as scene, Inactive, WorldTransform};
use glam::{Mat4, Quat, Vec3};

fn node(world: &mut World, name: &str, parent: Option<Entity>, x: f32) -> Entity {
    let e = scene::spawn(
        world.ecs_mut(),
        Transform::from_position(Vec3::X * x),
        parent,
    )
    .unwrap();
    world.ecs_mut().insert(e, Name::new(name)).unwrap();
    e
}
fn world() -> World {
    World::new(WorldConfig::default())
}
fn at(x: f32) -> Transform {
    Transform::from_position(Vec3::X * x)
}
fn matrix_close(a: Mat4, b: Mat4) {
    assert!(a.abs_diff_eq(b, 1e-5), "{a:?} != {b:?}");
}

#[test]
fn lookup_query_hierarchy_and_descendants_share_the_journal_then_the_ecs() {
    let mut w = world();
    let root = node(&mut w, "root", None, 1.0);
    let branch = node(&mut w, "branch", None, 10.0);
    let child = node(&mut w, "child", Some(root), 2.0);
    let leaf = node(&mut w, "leaf", Some(child), 3.0);
    w.step();
    w.begin_frame(w.config().fixed_dt * 0.5);
    let clocks = (
        w.tick(),
        w.clock().frame(),
        w.clock().real_elapsed(),
        w.clock().gameplay_elapsed(),
    );
    w.edit_gameplay(|edit| {
        let candidates = edit.entities()?;
        edit.set_name(root, "renamed")?;
        assert_eq!(edit.find("root")?, None);
        assert_eq!(edit.find("renamed")?, Some(root.into()));
        edit.set_transform(branch, at(20.0))?;
        edit.set_active(branch, false)?;
        edit.set_parent(child, Some(branch.into()), false)?;
        assert!(edit.children(root)?.is_empty());
        assert_eq!(edit.children(branch)?, vec![child.into()]);
        assert_eq!(edit.parent(child)?, Some(branch.into()));
        assert!(!edit.active_in_hierarchy(leaf)?);
        assert!(edit.active_self(leaf)?);
        assert_eq!(edit.world_matrix(leaf)?.w_axis.x, 25.0);
        for e in candidates {
            if edit.name(e)?.as_deref() == Some("renamed") {
                assert_eq!(e, root.into());
            }
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        clocks,
        (
            w.tick(),
            w.clock().frame(),
            w.clock().real_elapsed(),
            w.clock().gameplay_elapsed()
        )
    );
    assert_eq!(scene::parent_of(w.ecs(), child), Some(branch));
    assert!(w.ecs().has::<Inactive>(leaf));
    assert_eq!(
        w.ecs().get::<WorldTransform>(leaf).unwrap().matrix.w_axis.x,
        25.0
    );
    assert!(!w.extract().records.iter().any(|r| r.entity == leaf));
    w.edit_gameplay(|edit| {
        assert_eq!(edit.find("renamed")?, Some(root.into()));
        edit.set_active(branch, true)?;
        assert!(edit.active_in_hierarchy(leaf)?);
        Ok(())
    })
    .unwrap();
    let extract = w.extract();
    assert_eq!(
        extract
            .records
            .iter()
            .find(|r| r.entity == leaf)
            .unwrap()
            .world
            .w_axis
            .x,
        25.0
    );
}

#[test]
fn caught_invalid_edit_rolls_back_all_fields_without_events_or_derived_changes() {
    let mut w = world();
    let root = node(&mut w, "root", None, 1.0);
    let child = node(&mut w, "child", Some(root), 2.0);
    w.step();
    w.drain_events();
    let before = w.ecs().get::<WorldTransform>(child).unwrap().matrix;
    let error = w.edit_gameplay(|edit| {
        edit.set_name(root, "must not survive")?;
        edit.set_transform(root, at(99.0))?;
        edit.set_active(root, false)?;
        assert_eq!(
            edit.set_parent(root, Some(child.into()), false),
            Err(EditError::HierarchyCycle(root.into()))
        );
        // Catching the error cannot make the rest of this callback committable.
        assert_eq!(
            edit.set_name(child, "also rejected"),
            Err(EditError::HierarchyCycle(root.into()))
        );
        Ok(())
    });
    assert_eq!(error, Err(EditError::HierarchyCycle(root.into())));
    assert_eq!(w.ecs().get::<Name>(root).unwrap().value, "root");
    assert_eq!(*w.ecs().get::<Transform>(root).unwrap(), at(1.0));
    assert!(!w.ecs().has::<Inactive>(child));
    assert_eq!(scene::parent_of(w.ecs(), child), Some(root));
    assert_eq!(w.ecs().get::<WorldTransform>(child).unwrap().matrix, before);
    assert!(w.drain_events().is_empty());
}

#[test]
fn abort_and_callback_panic_preserve_prior_commits_and_leave_world_usable() {
    let mut w = world();
    let e = node(&mut w, "before", None, 0.0);
    w.edit_gameplay(|edit| edit.set_name(e, "committed"))
        .unwrap();
    assert_eq!(
        w.edit_gameplay::<()>(|edit| {
            edit.set_name(e, "aborted")?;
            Err(EditError::Aborted)
        }),
        Err(EditError::Aborted)
    );
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        let _: EditResult<EditCommit<()>> = w.edit_gameplay(|edit| {
            edit.set_name(e, "panicked")?;
            panic!("script failed before publication");
        });
    }));
    assert!(panicked.is_err());
    assert!(!w.is_poisoned());
    w.edit_gameplay(|edit| {
        assert_eq!(edit.name(e)?.as_deref(), Some("committed"));
        edit.set_name(e, "still usable")
    })
    .unwrap();
}

#[test]
fn foreign_stale_and_legacy_temporary_references_fail_the_whole_edit() {
    let mut w = world();
    let local = node(&mut w, "local", None, 0.0);
    let dead = node(&mut w, "dead", None, 0.0);
    scene::despawn_subtree(w.ecs_mut(), dead).unwrap();
    let mut other = world();
    let foreign = node(&mut other, "foreign", None, 0.0);
    for invalid in [dead, foreign, Entity::NULL, Entity::from_bits(1u128 << 64)] {
        let result = w.edit_gameplay(|edit| {
            edit.set_name(local, "leaked")?;
            let _ = edit.set_active(invalid, false);
            Ok(())
        });
        assert_eq!(result, Err(EditError::StaleEntity(invalid)));
        assert_eq!(w.ecs().get::<Name>(local).unwrap().value, "local");
    }
}

#[test]
fn keep_world_uses_staged_ancestors_and_exact_affine_matrices() {
    let mut w = world();
    let root = node(&mut w, "root", None, 0.0);
    let child = node(&mut w, "child", Some(root), 2.0);
    let destination = node(&mut w, "destination", None, 10.0);
    let pose = Transform {
        position: Vec3::X * 3.0,
        rotation: Quat::from_rotation_z(0.7),
        scale: Vec3::splat(2.0),
    };
    let expected = pose.matrix() * at(2.0).matrix();
    w.edit_gameplay(|edit| {
        edit.set_transform(root, pose)?;
        edit.set_transform(destination, at(20.0))?;
        matrix_close(edit.world_matrix(child)?, expected);
        edit.set_parent(child, Some(destination.into()), true)?;
        matrix_close(edit.world_matrix(child)?, expected);
        Ok(())
    })
    .unwrap();
    matrix_close(scene::current_world(w.ecs(), child).unwrap(), expected);
}

#[test]
fn unsupported_keep_world_and_invalid_pose_abort_earlier_data_edits() {
    for singular in [false, true] {
        let mut w = world();
        let parent = node(&mut w, "parent", None, 0.0);
        let child = node(&mut w, "child", None, 0.0);
        scene::set_local(
            w.ecs(),
            child,
            Transform {
                rotation: Quat::from_rotation_z(0.7),
                ..Transform::IDENTITY
            },
        )
        .unwrap();
        let scale = if singular {
            Vec3::new(0.0, 1.0, 1.0)
        } else {
            Vec3::new(2.0, 1.0, 1.0)
        };
        assert_eq!(
            w.edit_gameplay(|edit| {
                edit.set_name(child, "leaked")?;
                edit.set_transform(
                    parent,
                    Transform {
                        scale,
                        ..Transform::IDENTITY
                    },
                )?;
                edit.set_parent(child, Some(parent.into()), true)
            }),
            Err(EditError::UnsupportedTransform(child.into()))
        );
        assert_eq!(scene::parent_of(w.ecs(), child), None);
        assert_eq!(w.ecs().get::<Name>(child).unwrap().value, "child");
        assert_eq!(
            *w.ecs().get::<Transform>(parent).unwrap(),
            Transform::IDENTITY
        );
    }
    let mut w = world();
    let e = node(&mut w, "good", None, 0.0);
    for invalid in [
        Transform {
            position: Vec3::splat(f32::NAN),
            ..Transform::IDENTITY
        },
        Transform {
            rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            ..Transform::IDENTITY
        },
    ] {
        assert_eq!(
            w.edit_gameplay(|edit| {
                edit.set_name(e, "leaked")?;
                edit.set_transform(e, invalid)
            }),
            Err(EditError::InvalidTransform(e.into()))
        );
        assert_eq!(w.ecs().get::<Name>(e).unwrap().value, "good");
    }
}

#[test]
fn hierarchy_reversal_publishes_only_final_edges_and_consistent_sibling_order() {
    let mut w = world();
    let root = node(&mut w, "root", None, 0.0);
    let child = node(&mut w, "child", Some(root), 0.0);
    let first = node(&mut w, "first", None, 0.0);
    let second = node(&mut w, "second", None, 0.0);
    w.edit_gameplay(|edit| {
        edit.set_parent(child, None, false)?;
        edit.set_parent(root, Some(child.into()), false)?;
        edit.set_parent(second, Some(child.into()), false)?;
        edit.set_parent(first, Some(child.into()), false)?;
        assert_eq!(
            edit.children(child)?,
            vec![root.into(), second.into(), first.into()]
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(
        scene::children_of(w.ecs(), child),
        vec![root, second, first]
    );
    let events = w.drain_events();
    assert_eq!(events.len(), 4, "no temporary detach events");
    assert!(events.iter().any(|e| matches!(e.payload, EventPayload::ParentChanged { child: c, parent: Some(p) } if c==root && p==child)));
}

#[test]
fn completed_snapshots_stay_at_their_tick_while_gameplay_and_extract_advance() {
    let mut w = world();
    let e = node(&mut w, "object", None, 1.0);
    w.subscribe(&[e], true);
    w.step();
    let tick = w.tick();
    let old = w.snapshot()[0].local;
    w.edit_gameplay(|edit| edit.set_transform(e, at(9.0)))
        .unwrap();
    assert_eq!(w.tick(), tick);
    assert_eq!(w.snapshot()[0].local, old);
    assert_eq!(
        w.extract()
            .records
            .iter()
            .find(|r| r.entity == e)
            .unwrap()
            .world
            .w_axis
            .x,
        9.0
    );
    w.edit_gameplay(|edit| {
        assert_eq!(edit.local_transform(e)?, at(9.0));
        Ok(())
    })
    .unwrap();
    w.step();
    assert_eq!(w.snapshot()[0].local, at(9.0));
}

#[test]
fn successive_commits_are_visible_and_poisoned_worlds_cannot_enter_the_boundary() {
    let mut w = world();
    let e = node(&mut w, "before", None, 0.0);
    w.edit_gameplay(|edit| {
        edit.set_name(e, "queued")?;
        Ok(())
    })
    .unwrap();
    w.edit_gameplay(|edit| {
        assert_eq!(edit.name(e)?.as_deref(), Some("queued"));
        Ok(())
    })
    .unwrap();
    w.apply_gameplay_now(vec![Command::SetName {
        entity: e,
        name: "native writer".into(),
    }]);
    w.edit_gameplay(|edit| {
        assert_eq!(edit.name(e)?.as_deref(), Some("native writer"));
        Ok(())
    })
    .unwrap();
    w.poison();
    assert_eq!(
        w.edit_gameplay::<()>(|_| panic!("callback must not run")),
        Err(EditError::Poisoned)
    );
}

#[test]
fn missing_spatial_components_fail_atomically_without_creating_partial_hierarchy() {
    let mut w = world();
    let spatial = node(&mut w, "spatial", None, 0.0);
    let data = w.ecs_mut().spawn_empty();
    w.ecs_mut().insert(data, Name::new("data")).unwrap();
    assert_eq!(
        w.edit_gameplay(|edit| {
            assert_eq!(edit.find("data")?, Some(data.into()));
            edit.set_name(spatial, "leaked")?;
            edit.set_parent(spatial, Some(data.into()), false)
        }),
        Err(EditError::MissingTransform(data.into()))
    );
    assert_eq!(w.ecs().get::<Name>(spatial).unwrap().value, "spatial");
    assert_eq!(scene::parent_of(w.ecs(), spatial), None);
    assert!(scene::children_of(w.ecs(), data).is_empty());
}

#[test]
fn scoped_spawn_commit_resolves_hierarchy_and_component_references_without_a_tick() {
    let mut w = world();
    let tick = w.tick();
    let commit = w
        .edit_gameplay(|edit| {
            let root = edit.spawn("root", at(3.0), None)?;
            let child = edit.spawn("child", at(2.0), Some(root.into()))?;
            edit.set_component(
                child,
                GameplayComponentValue::Follow(Follow::default().map_object_refs(|_| root.into())),
            )?;
            edit.set_name(root, "committed")?;
            Ok((root, child))
        })
        .unwrap();
    let root = commit.resolve(commit.value.0).unwrap();
    let child = commit.resolve(commit.value.1).unwrap();
    assert_eq!(w.tick(), tick);
    assert_eq!(w.ecs().get::<Name>(root).unwrap().value, "committed");
    assert_eq!(w.ecs().get::<Follow>(child).unwrap().target.entity, root);
    assert_eq!(scene::parent_of(w.ecs(), child), Some(root));
    w.step();
    assert_eq!(w.live_entities(), 2);
    assert!(w.drain_events().is_empty());
}

#[test]
fn rejected_edits_leave_no_work_to_replay_and_do_not_block_retry() {
    let mut w = world();
    let root = node(&mut w, "original", None, 0.0);
    assert!(w
        .edit_gameplay(|edit| {
            edit.set_name(root, "discarded")?;
            edit.set_transform(root, at(f32::NAN))
        })
        .is_err());
    assert_eq!(w.ecs().get::<Name>(root).unwrap().value, "original");
    w.edit_gameplay(|edit| edit.set_name(root, "retry"))
        .unwrap();
    w.step();
    assert_eq!(w.ecs().get::<Name>(root).unwrap().value, "retry");
    assert!(w.diagnostics().is_empty());
}

#[test]
fn scoped_pending_ids_are_unique_and_expire_after_abort() {
    let mut w = world();
    let mut expired = None;
    let result = w.edit_gameplay(|edit| {
        let first = edit.spawn("first", at(0.0), None)?;
        let second = edit.spawn("second", at(0.0), None)?;
        assert_ne!(first, second);
        expired = Some(first);
        Err::<(), _>(EditError::Aborted)
    });
    assert!(result.is_err());
    assert!(w
        .edit_gameplay(|edit| edit.set_name(expired.unwrap(), "expired"))
        .is_err());
    assert_eq!(w.live_entities(), 0);
    w.edit_gameplay(|_| Ok(())).unwrap();
}

#[test]
fn canceled_scoped_spawn_never_allocates_a_live_identity() {
    let mut w = world();
    let commit = w
        .edit_gameplay(|edit| {
            let pending = edit.spawn("canceled", at(0.0), None)?;
            edit.destroy(pending)?;
            Ok(pending)
        })
        .unwrap();
    assert_eq!(commit.resolve(commit.value), None);
    assert_eq!(w.live_entities(), 0);
    assert!(w.drain_events().is_empty());
    w.edit_gameplay(|_| Ok(())).unwrap();
}

#[cfg(feature = "physics")]
#[test]
fn committed_body_effect_executes_once_after_publication() {
    let mut w = world();
    let commit = w
        .edit_gameplay(|edit| {
            let pending = edit.spawn("body", at(0.0), None)?;
            edit.set_component(
                pending,
                ComponentValue::RigidBody(RigidBody {
                    body_type: BodyType::Dynamic,
                    mass: 2.0,
                    gravity_scale: 0.0,
                    linear_damping: 0.0,
                    ..Default::default()
                }),
            )?;
            edit.set_component(pending, ComponentValue::Collider(Collider::default()))?;
            edit.body_effect(pending, BodyCommand::ApplyImpulse(Vec3::X * 4.0))?;
            Ok(pending)
        })
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    assert!(w.physics().body_state(entity).is_none());
    w.step();
    assert_eq!(
        w.physics().body_state(entity).unwrap().linear_velocity.x,
        2.0
    );
    w.step();
    assert_eq!(
        w.physics().body_state(entity).unwrap().linear_velocity.x,
        2.0
    );
}

#[test]
fn caught_invalid_transaction_request_is_sticky_without_deferred_diagnostics() {
    let mut w = world();
    assert!(w
        .edit_gameplay(|edit| {
            let pending = edit.spawn("discarded", at(0.0), None)?;
            let _ = edit.set_transform(pending, at(f32::NAN));
            Ok(())
        })
        .is_err());
    assert_eq!(w.live_entities(), 0);
    assert_eq!(w.step().rejected, 0);
    assert!(w.diagnostics().is_empty());
}

#[test]
fn final_reference_validation_discards_all_changes_without_replay() {
    let mut w = world();
    let owner = node(&mut w, "owner", None, 0.0);
    assert!(w
        .edit_gameplay(|edit| {
            let target = edit.spawn("target", at(0.0), None)?;
            edit.set_component(
                owner,
                GameplayComponentValue::Follow(
                    Follow::default().map_object_refs(|_| target.into()),
                ),
            )?;
            edit.destroy(target)?;
            Ok(())
        })
        .is_err());
    assert_eq!(w.live_entities(), 1);
    assert!(!w.ecs().has::<Follow>(owner));
    w.edit_gameplay(|_| Ok(())).unwrap();
    w.step();
    assert_eq!(w.live_entities(), 1);
    assert!(!w.ecs().has::<Follow>(owner));
}
