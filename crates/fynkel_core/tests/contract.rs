//! Runtime contract tests (blueprint §5.1, §5.4, §5.5; ADR-0008, ADR-0014).

use fynkel_core::*;
use glam::{Mat4, Vec3};

fn world() -> World {
    World::new(WorldConfig::default())
}

fn at(x: f32) -> Transform {
    Transform::from_position(Vec3::new(x, 0.0, 0.0))
}

fn spawn_one(w: &mut World, x: f32) -> Entity {
    let commit = w.edit_gameplay(|edit| edit.spawn("", at(x), None)).unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    w.drain_events();
    entity
}

fn pos(m: Mat4) -> Vec3 {
    m.transform_point3(Vec3::ZERO)
}

#[test]
fn data_commits_publish_now_and_the_next_tick_only_samples_them() {
    let mut w = world();
    let e = spawn_one(&mut w, 0.0);
    w.subscribe(&[e], true);
    w.edit_gameplay(|edit| edit.set_transform(e, at(5.0)))
        .unwrap();
    assert_eq!(pos(w.extract().records[0].world).x, 5.0);
    let r = w.step();
    assert_eq!((r.applied, r.rejected, r.snapshot_records), (0, 0, 1));
    assert_eq!(w.snapshot()[0].local.position.x, 5.0);
    assert_eq!(w.snapshot()[0].world.position.x, 5.0);
}

#[test]
fn scoped_spawns_resolve_at_commit_and_destroy_removes_subtree() {
    let mut w = world();
    let root = spawn_one(&mut w, 0.0);
    let c = w
        .edit_gameplay(|e| {
            let child = e.spawn("child", at(1.0), Some(root.into()))?;
            e.spawn("other", at(2.0), None)?;
            Ok(child)
        })
        .unwrap();
    let child = c.resolve(c.value).unwrap();
    assert!(w.drain_events().is_empty());
    assert_eq!(w.ecs().get::<Name>(child).unwrap().value, "child");
    assert_eq!(w.live_entities(), 3);
    w.subscribe(&[root, child], true);
    w.edit_gameplay(|e| e.destroy(root)).unwrap();
    let r = w.step();
    assert_eq!((r.live_entities, r.snapshot_records), (1, 0));
    assert_eq!(
        w.drain_events()
            .iter()
            .map(|e| e.payload.clone())
            .collect::<Vec<_>>(),
        vec![
            EventPayload::Destroyed { entity: root },
            EventPayload::Destroyed { entity: child }
        ]
    );
    assert!(
        matches!(w.edit_gameplay(|e|e.destroy(root)),Err(EditError::StaleEntity(e)) if e==root)
    );
    assert_eq!(w.step().rejected, 0);
    assert!(w.drain_events().is_empty());
}

/// Foundation 3.10: a despawn queued on the ECS (`despawn_deferred`,
/// reachable through `World::ecs()`) goes the way `Command::Destroy`
/// does at the tick's flush — the subtree with it, a `Destroyed` event
/// each, subscriptions dropped — instead of leaving the children behind.
#[test]
fn a_deferred_despawn_takes_the_subtree_like_destroy() {
    let mut w = world();
    let root = spawn_one(&mut w, 0.0);
    let c = w
        .edit_gameplay(|e| e.spawn("child", at(1.0), Some(root.into())))
        .unwrap();
    let child = c.resolve(c.value).unwrap();
    w.step();
    w.drain_events();
    w.subscribe(&[root, child], true);

    w.ecs().despawn_deferred(root);
    w.ecs().despawn_deferred(child); // already going with its parent: ignored
    let r = w.step();
    assert_eq!(r.live_entities, 0, "the child went with its parent");
    assert!(!w.ecs().contains(child));
    assert_eq!(
        r.snapshot_records, 0,
        "subscriptions of destroyed entities drop"
    );
    assert_eq!(
        w.drain_events()
            .iter()
            .map(|e| e.payload.clone())
            .collect::<Vec<_>>(),
        vec![
            EventPayload::Destroyed { entity: root },
            EventPayload::Destroyed { entity: child }
        ]
    );
}

#[test]
fn zero_tick_transform_commits_are_authoritative_and_never_replayed() {
    let mut w = world();
    let e = spawn_one(&mut w, 0.0);
    assert_eq!(w.begin_frame(0.001).steps, 0);
    w.edit_gameplay(|edit| edit.set_transform(e, at(3.0)))
        .unwrap();
    assert_eq!(w.ecs().copy::<Transform>(e).unwrap().position.x, 3.0);
    assert_eq!(pos(w.extract().records[0].world).x, 3.0);
    w.edit_gameplay(|edit| edit.set_transform(e, at(7.0)))
        .unwrap();
    w.begin_frame(1.0 / 60.0);
    w.step();
    assert_eq!(pos(w.extract().records[0].world).x, 7.0);
    w.begin_frame(1.0 / 120.0);
    assert_eq!(pos(w.extract().records[0].world).x, 7.0);
}

#[test]
fn invalid_pose_rolls_back_other_data_and_structure_in_the_same_edit() {
    let mut w = world();
    let a = spawn_one(&mut w, 0.0);
    let b = spawn_one(&mut w, 1.0);
    assert!(w
        .edit_gameplay(|e| {
            e.destroy(b)?;
            e.set_name(a, "changed")?;
            e.set_transform(a, at(f32::NAN))
        })
        .is_err());
    assert!(w.ecs().contains(b));
    assert_eq!(w.ecs().copy::<Transform>(a).unwrap(), at(0.0));
    assert!(w.ecs().get::<Name>(a).unwrap().value.is_empty());
    assert!(w.drain_events().is_empty());
    assert_eq!(w.step().rejected, 0);
}

#[test]
fn extract_interpolates_between_ticks() {
    let mut w = world();
    let e = spawn_one(&mut w, 0.0);
    w.prepare_gameplay_tick();
    w.edit_gameplay(|edit| {
        edit.set_transform(e, at(10.0))?;
        Ok(())
    })
    .unwrap();
    // 60 Hz tick, then a half-step frame at 120 Hz presentation.
    let p = w.begin_frame(1.0 / 60.0);
    assert_eq!(p.steps, 1);
    w.step();
    // Right after the tick alpha is ~0: the extract shows S[n], one tick
    // behind, and moves towards S[n+1] over the next fixed interval.
    assert!(pos(w.extract().records[0].world).x.abs() < 1e-3);
    let p = w.begin_frame(1.0 / 120.0);
    assert_eq!(p.steps, 0);
    let x = w.extract();
    assert!((x.alpha - 0.5).abs() < 1e-3);
    assert!(
        (pos(x.records[0].world).x - 5.0).abs() < 1e-3,
        "{}",
        pos(x.records[0].world).x
    );
    let p = w.begin_frame(1.0 / 120.0);
    assert_eq!(p.steps, 1);
    w.step();
    assert!((pos(w.extract().records[0].world).x - 10.0).abs() < 1e-3);
}

#[test]
fn missed_deadline_is_bounded_and_diagnosed() {
    let mut w = World::new(WorldConfig {
        max_steps_per_frame: 2,
        ..WorldConfig::default()
    });
    let plan = w.begin_frame(0.25);
    assert_eq!(plan.steps, 2);
    assert!(plan.dropped_seconds > 0.0);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::MISSED_DEADLINE);
    let ev = w.drain_events();
    assert!(matches!(ev[0].payload, EventPayload::MissedDeadline { .. }));
    for _ in 0..plan.steps {
        w.step();
    }
    assert_eq!(w.tick(), 2);
}

#[test]
fn hierarchy_edits_reparent_and_reject_cycles_atomically() {
    let mut w = world();
    let a = spawn_one(&mut w, 1.0);
    let b = spawn_one(&mut w, 2.0);
    assert!(w
        .edit_gameplay(|e| {
            e.set_parent(b, Some(a.into()), true)?;
            e.set_parent(a, Some(b.into()), false)
        })
        .is_err());
    assert!(fynkel_scene::children_of(w.ecs(), a).is_empty());
    assert!(w.drain_events().is_empty());
    w.edit_gameplay(|e| e.set_parent(b, Some(a.into()), true))
        .unwrap();
    assert_eq!(pos(w.extract().records[1].world).x, 2.0);
    assert!((w.ecs().copy::<Transform>(b).unwrap().position.x - 1.0).abs() < 1e-5);
    assert!(
        matches!(w.drain_events()[0].payload,EventPayload::ParentChanged{child,parent:Some(p)} if child==b && p==a)
    );
}

#[test]
fn events_are_pull_only_and_drained_once() {
    let mut w = world();
    let _ = spawn_one(&mut w, 0.0);
    assert!(w.drain_events().is_empty(), "already drained by spawn_one");
    w.push_event(EventPayload::SceneReady {
        scene: fynkel_core::Uuid::NIL,
    });
    w.step();
    let ev = w.drain_events();
    assert_eq!(ev.len(), 1);
    assert!(w.drain_events().is_empty());
    assert!(w.after_physics_events().is_empty());
}

#[test]
fn wrong_world_and_poison() {
    let mut w = world();
    let e = spawn_one(&mut w, 0.0);
    let foreign = Entity::new(world().id(), e.slot(), e.generation());
    assert!(
        matches!(w.edit_gameplay(|edit|edit.destroy(foreign)),Err(EditError::StaleEntity(x)) if x==foreign)
    );
    assert_eq!(w.step().rejected, 0);
    w.poison();
    assert_eq!(w.step().status, Status::Poisoned);
    assert_eq!(
        w.edit_gameplay(|edit| edit.destroy(e)),
        Err(EditError::Poisoned)
    );
    assert!(w.ecs().contains(e));
}

#[test]
fn transaction_of_ten_thousand_round_trips() {
    let mut w = world();
    let c = w
        .edit_gameplay(|edit| {
            (1..=10_000)
                .map(|i| edit.spawn("", at(i as f32), None))
                .collect::<EditResult<Vec<_>>>()
        })
        .unwrap();
    let ids: Vec<_> = c.value.iter().map(|p| c.resolve(*p).unwrap()).collect();
    assert_eq!(w.live_entities(), 10_000);
    assert!(w.drain_events().is_empty());
    w.subscribe(&ids, true);
    w.edit_gameplay(|edit| {
        for &e in &ids {
            edit.set_transform(e, at(-1.0))?;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(w.step().snapshot_records, 10_000);
    assert!(w.snapshot().iter().all(|s| s.local.position.x == -1.0));
    w.edit_gameplay(|edit| {
        for &e in &ids {
            edit.destroy(e)?;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(w.step().live_entities, 0);
    assert!(w.extract().records.is_empty());
}

#[test]
fn scoped_pending_references_build_an_object_and_expire_after_commit() {
    let mut w = world();
    let root = spawn_one(&mut w, 0.0);
    let c = w
        .edit_gameplay(|edit| {
            let lamp = edit.spawn("", at(1.0), None)?;
            edit.set_name(lamp, "lamp")?;
            edit.set_component(lamp, ComponentValue::PointLight(PointLight::default()))?;
            edit.set_parent(lamp, Some(root.into()), false)?;
            let bulb = edit.spawn("", at(0.5), Some(lamp.into()))?;
            edit.set_transform(bulb, at(0.25))?;
            Ok((lamp, bulb))
        })
        .unwrap();
    let (lamp, bulb) = (c.resolve(c.value.0).unwrap(), c.resolve(c.value.1).unwrap());
    assert_eq!(w.ecs().get::<Name>(lamp).unwrap().value, "lamp");
    assert!(w.ecs().has::<PointLight>(lamp));
    assert_eq!(w.ecs().get::<fynkel_scene::Parent>(lamp).unwrap().0, root);
    assert_eq!(w.ecs().get::<fynkel_scene::Parent>(bulb).unwrap().0, lamp);
    assert_eq!(w.ecs().copy::<Transform>(bulb).unwrap().position.x, 0.25);
    assert!(w.edit_gameplay(|edit| edit.destroy(c.value.0)).is_err());
    assert_eq!(w.live_entities(), 3);
    assert_eq!(w.step().rejected, 0);
}

#[test]
fn the_extract_carries_the_worlds_time_between_ticks() {
    let mut w = world();
    assert_eq!(w.extract().time, 0.0);
    // Shader time uses accepted gameplay time; transform interpolation
    // remains independently based on completed simulation ticks.
    let plan = w.begin_frame(2.5 / 60.0);
    assert_eq!(plan.steps, 2);
    w.step();
    w.step();
    let t = w.extract().time;
    assert!((t - 2.5 / 60.0).abs() < 1e-5, "{t}");
    // No frame, no time: a paused world's time stands still.
    assert_eq!(w.extract().time, t);
    let plan = w.begin_frame_scaled(2.0, 1.0);
    assert!(plan.dropped_seconds > 0.0);
    let accepted = t + 0.25;
    assert!((w.extract().time - accepted).abs() < 1e-5);
    for _ in 0..plan.steps {
        w.step();
    }
    assert!((w.extract().time - accepted).abs() < 1e-5);
    w.step(); // Editor Step advances simulation, not shader gameplay time.
    assert!((w.extract().time - accepted).abs() < 1e-5);
}

#[test]
fn interpolation_composes_local_poses_without_losing_hierarchy_shear() {
    let mut w = world();
    let root = fynkel_scene::spawn(
        w.ecs_mut(),
        Transform {
            scale: Vec3::new(2.0, 1.0, 3.0),
            ..Transform::IDENTITY
        },
        None,
    )
    .unwrap();
    let before = Transform {
        rotation: glam::Quat::from_rotation_z(0.3),
        ..Transform::IDENTITY
    };
    let child = fynkel_scene::spawn(w.ecs_mut(), before, Some(root)).unwrap();
    let leaf =
        fynkel_scene::spawn(w.ecs_mut(), Transform::from_position(Vec3::X), Some(child)).unwrap();
    w.step();
    let after = Transform {
        rotation: glam::Quat::from_rotation_z(1.1),
        position: Vec3::Y,
        ..Transform::IDENTITY
    };
    w.prepare_gameplay_tick();
    w.edit_gameplay(|edit| {
        edit.set_transform(child, after)?;
        Ok(())
    })
    .unwrap();
    w.step();
    for fraction in [0.0, 0.5, 0.999] {
        // Each frame contributes only the difference; no simulation tick.
        let delta = if fraction == 0.0 {
            0.0
        } else if fraction == 0.5 {
            0.5
        } else {
            0.499
        };
        w.begin_frame(w.config().fixed_dt * delta);
        let extract = w.extract();
        let alpha = extract.alpha;
        let local = Transform {
            position: before.position.lerp(after.position, alpha),
            rotation: before.rotation.slerp(after.rotation, alpha),
            scale: Vec3::ONE,
        };
        let expected = Mat4::from_scale(Vec3::new(2.0, 1.0, 3.0)) * local.matrix();
        let actual = extract
            .records
            .iter()
            .find(|r| r.entity == child)
            .unwrap()
            .world;
        assert!(
            actual.abs_diff_eq(expected, 1e-5),
            "alpha={alpha}: {actual:?}"
        );
        let leaf_world = extract
            .records
            .iter()
            .find(|r| r.entity == leaf)
            .unwrap()
            .world;
        assert!(leaf_world.abs_diff_eq(expected * Mat4::from_translation(Vec3::X), 1e-5));
    }
    // A presentation ancestor uses exact matrix composition for descendants.
    let override_root = Transform {
        scale: Vec3::new(5.0, 2.0, 1.0),
        ..Transform::IDENTITY
    };
    w.edit_gameplay(|edit| {
        edit.set_transform(root, override_root)?;
        Ok(())
    })
    .unwrap();
    let extract = w.extract();
    let actual = extract
        .records
        .iter()
        .find(|r| r.entity == leaf)
        .unwrap()
        .world;
    assert!(actual.abs_diff_eq(
        override_root.matrix() * after.matrix() * Mat4::from_translation(Vec3::X),
        1e-5
    ));
}
