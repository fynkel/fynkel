use fynkel_core::*;
use fynkel_scene::{self as scene, Inactive};
use glam::Vec3;
use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn world() -> World {
    World::new(WorldConfig::default())
}
fn at(x: f32) -> Transform {
    Transform::from_position(Vec3::X * x)
}
fn node(w: &mut World, name: &str, parent: Option<Entity>) -> Entity {
    let e = scene::spawn(w.ecs_mut(), at(1.0), parent).unwrap();
    w.ecs_mut().insert(e, Name::new(name)).unwrap();
    e
}

#[test]
fn pending_objects_share_lookup_hierarchy_activity_and_geometry_then_resolve_on_commit() {
    let mut w = world();
    let live = node(&mut w, "live", None);
    let tick = w.tick();
    let commit = w
        .edit_gameplay(|edit| {
            let root = edit.spawn("root", at(10.0), None)?;
            let child = edit.spawn("child", at(2.0), Some(root.into()))?;
            edit.set_parent(live, Some(child.into()), false)?;
            edit.set_active(root, false)?;
            edit.set_name(child, "renamed")?;
            assert_eq!(edit.find("renamed")?, Some(child.into()));
            assert_eq!(edit.find("child")?, None);
            assert_eq!(edit.parent(live)?, Some(child.into()));
            assert_eq!(edit.children(root)?, vec![child.into()]);
            assert_eq!(edit.world_matrix(live)?.w_axis.x, 13.0);
            assert!(!edit.active_in_hierarchy(live)?);
            assert_eq!(edit.entities()?.len(), 3);
            Ok((root, child))
        })
        .unwrap();
    let (p_root, p_child) = commit.value;
    let root = commit.resolve(p_root).unwrap();
    let child = commit.resolve(p_child).unwrap();
    assert_eq!(commit.spawned().count(), 2);
    assert!(w.ecs().contains(root) && w.ecs().contains(child));
    assert_eq!(scene::parent_of(w.ecs(), live), Some(child));
    assert_eq!(scene::parent_of(w.ecs(), child), Some(root));
    assert_eq!(scene::current_world(w.ecs(), live).unwrap().w_axis.x, 13.0);
    assert!(w.ecs().has::<Inactive>(live));
    assert_eq!(w.tick(), tick);
    assert!(
        !w.drain_events()
            .iter()
            .any(|e| matches!(e.payload, EventPayload::Spawned { .. })),
        "typed reports must not manufacture legacy temporary IDs"
    );
    w.edit_gameplay(|edit| {
        assert_eq!(edit.find("renamed")?, Some(child.into()));
        edit.set_active(root, true)
    })
    .unwrap();
    assert!(!w.ecs().has::<Inactive>(live));
}

#[test]
fn abort_and_panic_allocate_no_live_entities_and_pending_tokens_never_revive() {
    let mut w = world();
    let live = node(&mut w, "original", None);
    let mut abandoned = None;
    assert_eq!(
        w.edit_gameplay::<()>(|edit| {
            let p = edit.spawn("abandoned", at(0.0), None)?;
            abandoned = Some(p);
            edit.destroy(live)?;
            Err(EditError::Aborted)
        }),
        Err(EditError::Aborted)
    );
    assert_eq!(w.live_entities(), 1);
    assert!(w.ecs().contains(live));
    assert!(w.drain_events().is_empty());
    let pending = abandoned.unwrap();
    assert_eq!(
        w.edit_gameplay::<()>(|edit| {
            let replacement = edit.spawn("replacement", at(0.0), None)?;
            assert_ne!(replacement, pending);
            let _ = edit.set_name(pending, "must fail");
            Ok(())
        }),
        Err(EditError::ExpiredPending(pending))
    );
    assert_eq!(w.live_entities(), 1);
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _: EditResult<EditCommit<()>> = w.edit_gameplay(|edit| {
            edit.spawn("panic", at(0.0), None)?;
            edit.destroy(live)?;
            panic!("callback panic");
        });
    }))
    .is_err());
    assert_eq!(w.live_entities(), 1);
    assert!(!w.is_poisoned());
    let commit = w
        .edit_gameplay(|edit| edit.spawn("success", at(0.0), None))
        .unwrap();
    assert_eq!(
        commit.resolve(commit.value).unwrap().slot(),
        live.slot() + 1,
        "aborted journals do not allocate slots"
    );
    assert_eq!(commit.resolve(pending), None);
}

#[test]
fn committed_and_foreign_pending_tokens_cannot_be_used_as_live_references() {
    let mut w = world();
    let committed = w
        .edit_gameplay(|edit| edit.spawn("committed", at(0.0), None))
        .unwrap();
    let p = committed.value;
    let live = committed.resolve(p).unwrap();
    assert_eq!(
        w.edit_gameplay(|edit| edit.set_name(p, "invalid")),
        Err(EditError::ExpiredPending(p))
    );
    w.edit_gameplay(|edit| edit.set_name(live, "valid"))
        .unwrap();
    let mut other = world();
    let err = other.edit_gameplay(|edit| {
        edit.spawn("other", at(0.0), None)?;
        edit.set_active(p, false)
    });
    assert_eq!(err, Err(EditError::ExpiredPending(p)));
    assert_eq!(other.live_entities(), 0);
}

#[test]
fn destruction_follows_staged_hierarchy_and_preserves_moved_out_subtrees() {
    let mut w = world();
    let root = node(&mut w, "root", None);
    let escaped = node(&mut w, "escaped", Some(root));
    let leaf = node(&mut w, "leaf", Some(escaped));
    let incoming = node(&mut w, "incoming", None);
    w.step();
    w.drain_events();
    w.subscribe(&[root, incoming, escaped], true);
    let commit = w
        .edit_gameplay(|edit| {
            edit.set_parent(escaped, None, true)?;
            edit.set_parent(incoming, Some(root.into()), false)?;
            let temporary = edit.spawn("temporary", at(0.0), Some(incoming.into()))?;
            edit.destroy(root)?;
            assert!(!edit.contains(root)?);
            assert!(!edit.contains(incoming)?);
            assert!(!edit.contains(temporary)?);
            assert!(edit.contains(escaped)? && edit.contains(leaf)?);
            assert_eq!(edit.find("incoming")?, None);
            assert_eq!(edit.entities()?, vec![escaped.into(), leaf.into()]);
            Ok(temporary)
        })
        .unwrap();
    assert_eq!(commit.resolve(commit.value), None);
    assert!(w.ecs().contains(escaped) && w.ecs().contains(leaf));
    assert!(!w.ecs().contains(root) && !w.ecs().contains(incoming));
    assert_eq!(scene::parent_of(w.ecs(), escaped), None);
    assert_eq!(scene::current_world(w.ecs(), leaf).unwrap().w_axis.x, 3.0);
    let events = w.drain_events();
    let destroyed: Vec<_> = events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::Destroyed { entity } => Some(entity),
            _ => None,
        })
        .collect();
    assert_eq!(
        destroyed.iter().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([root, incoming])
    );
    assert_eq!(destroyed.len(), 2);
    assert_eq!(
        events.len(),
        3,
        "only final survivor reparent plus two destructions"
    );
    w.step();
    assert_eq!(
        w.snapshot().len(),
        1,
        "destroyed subscriptions were removed"
    );
    assert_eq!(w.snapshot()[0].entity, escaped);
}

#[test]
fn destroying_pending_parent_retires_live_moved_in_descendants_without_creating_the_parent() {
    let mut w = world();
    let live = node(&mut w, "live", None);
    let leaf = node(&mut w, "leaf", Some(live));
    let result = w
        .edit_gameplay(|edit| {
            let p = edit.spawn("pending", at(0.0), None)?;
            edit.set_parent(live, Some(p.into()), false)?;
            edit.destroy(p)?;
            assert!(edit.entities()?.is_empty());
            Ok(p)
        })
        .unwrap();
    assert!(result.spawned().next().is_none());
    assert_eq!(result.resolve(result.value), None);
    assert_eq!(w.live_entities(), 0);
    let gone: BTreeSet<_> = w
        .drain_events()
        .into_iter()
        .filter_map(|e| match e.payload {
            EventPayload::Destroyed { entity } => Some(entity),
            _ => None,
        })
        .collect();
    assert_eq!(gone, BTreeSet::from([live, leaf]));
}

#[test]
fn invalid_edit_after_destroy_rolls_back_deletions_spawns_and_earlier_data() {
    let mut w = world();
    let root = node(&mut w, "original", None);
    let child = node(&mut w, "child", Some(root));
    w.step();
    w.drain_events();
    let error = w.edit_gameplay(|edit| {
        edit.set_name(root, "leaked")?;
        edit.spawn("leaked spawn", at(0.0), None)?;
        edit.destroy(root)?;
        let _ = edit.set_active(child, false);
        Ok(())
    });
    assert_eq!(error, Err(EditError::Destroyed(child.into())));
    assert_eq!(w.live_entities(), 2);
    assert_eq!(w.ecs().get::<Name>(root).unwrap().value, "original");
    assert_eq!(scene::parent_of(w.ecs(), child), Some(root));
    assert!(w.drain_events().is_empty());
}

#[test]
fn pending_cycle_or_invalid_parent_poison_the_journal_not_the_world() {
    let mut w = world();
    let root = node(&mut w, "root", None);
    let mut pending = None;
    let result = w.edit_gameplay(|edit| {
        let p = edit.spawn("parent", at(0.0), None)?;
        pending = Some(p);
        edit.set_parent(root, Some(p.into()), false)?;
        let _ = edit.set_parent(p, Some(root.into()), false);
        Ok(())
    });
    assert_eq!(
        result,
        Err(EditError::HierarchyCycle(pending.unwrap().into()))
    );
    assert_eq!(scene::parent_of(w.ecs(), root), None);
    assert_eq!(w.live_entities(), 1);
    assert!(!w.is_poisoned());
    let result = w.edit_gameplay(|edit| {
        edit.set_name(root, "leaked")?;
        edit.spawn("invalid parent", at(0.0), Some(Entity::NULL.into()))?;
        Ok(())
    });
    assert_eq!(result, Err(EditError::StaleEntity(Entity::NULL)));
    assert_eq!(w.ecs().get::<Name>(root).unwrap().value, "root");
}

#[test]
fn frozen_queries_skip_destroyed_members_read_staged_values_and_exclude_later_spawns() {
    let mut w = world();
    let first = node(&mut w, "first", None);
    let second = node(&mut w, "second", None);
    w.edit_gameplay(|edit| {
        let old_pending = edit.spawn("old pending", at(0.0), None)?;
        let mut query = edit.query()?;
        assert_eq!(query.next(edit)?, Some(first.into()));
        edit.destroy(second)?;
        edit.set_name(old_pending, "updated")?;
        let new_pending = edit.spawn("new pending", at(0.0), None)?;
        let next = query.next(edit)?.unwrap();
        assert_eq!(next, old_pending.into());
        assert_eq!(edit.name(next)?.as_deref(), Some("updated"));
        assert_eq!(query.next(edit)?, None);
        let mut next_query = edit.query()?;
        let mut found = Vec::new();
        while let Some(e) = next_query.next(edit)? {
            found.push(e);
        }
        assert_eq!(
            found,
            vec![first.into(), old_pending.into(), new_pending.into()]
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(w.live_entities(), 3);
}

#[test]
fn a_query_cursor_expires_at_commit_or_abort_and_cannot_cross_worlds() {
    let mut w = world();
    let mut query = w.edit_gameplay(|edit| edit.query()).unwrap().value;
    assert_eq!(
        w.edit_gameplay(|edit| {
            let _ = query.next(edit);
            Ok(())
        }),
        Err(EditError::ExpiredQuery)
    );
    let mut other = world();
    assert_eq!(
        other.edit_gameplay(|edit| query.next(edit)),
        Err(EditError::ExpiredQuery)
    );
    let mut aborted = None;
    let _: EditResult<EditCommit<()>> = w.edit_gameplay(|edit| {
        aborted = Some(edit.query()?);
        Err(EditError::Aborted)
    });
    assert_eq!(
        w.edit_gameplay(|edit| aborted.as_mut().unwrap().next(edit)),
        Err(EditError::ExpiredQuery)
    );
}

#[test]
fn canceled_spawn_does_not_consume_a_slot_and_committed_destroy_retires_live_identity() {
    let mut w = world();
    let commit = w
        .edit_gameplay(|edit| {
            let p = edit.spawn("canceled", at(0.0), None)?;
            edit.destroy(p)?;
            let survivor = edit.spawn("survivor", at(0.0), None)?;
            Ok((p, survivor))
        })
        .unwrap();
    assert_eq!(commit.resolve(commit.value.0), None);
    let entity = commit.resolve(commit.value.1).unwrap();
    assert_eq!(entity.slot(), 0);
    w.edit_gameplay(|edit| edit.destroy(entity)).unwrap();
    let next = w
        .edit_gameplay(|edit| edit.spawn("next", at(0.0), None))
        .unwrap();
    let replacement = next.resolve(next.value).unwrap();
    assert_eq!(replacement.slot(), entity.slot());
    assert_ne!(replacement.generation(), entity.generation());
    assert_eq!(
        w.edit_gameplay(|edit| edit.name(entity)),
        Err(EditError::StaleEntity(entity))
    );
}

#[test]
fn pending_descendant_moved_out_before_parent_cancellation_still_materializes() {
    let mut w = world();
    let result = w
        .edit_gameplay(|edit| {
            let root = edit.spawn("canceled root", at(10.0), None)?;
            let child = edit.spawn("surviving child", at(2.0), Some(root.into()))?;
            edit.set_parent(child, None, true)?;
            edit.destroy(root)?;
            assert_eq!(edit.world_matrix(child)?.w_axis.x, 12.0);
            Ok((root, child))
        })
        .unwrap();
    assert_eq!(result.resolve(result.value.0), None);
    let child = result.resolve(result.value.1).unwrap();
    assert_eq!(child.slot(), 0);
    assert_eq!(scene::parent_of(w.ecs(), child), None);
    assert_eq!(scene::current_world(w.ecs(), child).unwrap().w_axis.x, 12.0);
    assert_eq!(w.live_entities(), 1);
    assert!(w.drain_events().is_empty());
}
