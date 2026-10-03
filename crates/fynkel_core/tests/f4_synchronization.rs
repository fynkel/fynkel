use fynkel_core::*;
use fynkel_scene::{self as scene, SceneSync, WorldTransform};
use glam::Vec3;

#[test]
fn sparse_direct_native_writes_refresh_scene_physics_and_membership() {
    let mut w = World::new(Default::default());
    let mut entities = Vec::new();
    for i in 0..2048 {
        let e = scene::spawn(
            w.ecs_mut(),
            Transform::from_position(Vec3::new(i as f32 * 4., 0., 0.)),
            None,
        )
        .unwrap();
        w.ecs_mut().insert(e, Collider::default()).unwrap();
        entities.push(e);
    }
    w.refresh_physics();
    w.extract();
    w.refresh_physics();
    assert_eq!(w.scene_sync_candidates(), (0, 0));
    #[cfg(feature = "physics")]
    assert_eq!(w.physics().sync_candidates(), 0);
    w.extract();
    assert_eq!(w.render_membership_candidates(), 0);
    let e = entities[600];
    w.ecs().get_mut::<Transform>(e).unwrap().position.y = 3.;
    w.refresh_physics();
    assert_eq!(w.scene_sync_candidates().0, 1);
    #[cfg(feature = "physics")]
    assert_eq!(w.physics().sync_candidates(), 1);
    assert_eq!(scene::world_matrix(w.ecs(), e).unwrap().w_axis.y, 3.);
    let frame = w.extract();
    assert_eq!(
        frame
            .records
            .iter()
            .find(|r| r.entity == e)
            .unwrap()
            .world
            .w_axis
            .y,
        3.
    );
    // Value changes update the snapshot without rechecking membership.
    assert_eq!(w.render_membership_candidates(), 0);
    w.ecs_mut().remove::<WorldTransform>(e).unwrap();
    assert_eq!(w.extract().records.len(), 2047);
    assert_eq!(w.render_membership_candidates(), 1);
    w.refresh_physics();
    #[cfg(feature = "physics")]
    {
        w.step();
        assert_eq!(w.physics().stats().colliders, 2047);
    }
}

#[test]
fn dirty_roots_and_activity_follow_reparenting_without_full_scans() {
    let mut w = fynkel_ecs::World::new();
    let a = scene::spawn(&mut w, Transform::IDENTITY, None).unwrap();
    let b = scene::spawn(&mut w, Transform::from_position(Vec3::X), None).unwrap();
    let child = scene::spawn(&mut w, Transform::from_position(Vec3::Y), Some(a)).unwrap();
    let mut sync = SceneSync::default();
    sync.propagate(&mut w);
    sync.refresh_active(&mut w);
    w.get_mut::<Transform>(a).unwrap().position.x = 3.;
    assert_eq!(sync.propagate(&mut w), 2);
    assert_eq!(sync.transform_candidates, 1);
    assert_eq!(scene::world_matrix(&w, child).unwrap().w_axis.x, 3.);
    scene::set_active(&mut w, b, false).unwrap();
    scene::set_parent_keep_local(&mut w, child, Some(b)).unwrap();
    sync.refresh_active(&mut w);
    sync.propagate(&mut w);
    assert!(!scene::is_active_in_hierarchy(&w, child));
    assert_eq!(scene::world_matrix(&w, child).unwrap().w_axis.x, 1.);
    sync.propagate(&mut w);
    sync.refresh_active(&mut w);
    assert_eq!(
        (sync.transform_candidates, sync.activity_candidates),
        (0, 0)
    );
}

#[test]
fn table_revisions_ignore_pose_values_but_follow_metadata_and_type_presence() {
    let mut w = World::new(Default::default());
    let c = w
        .edit_gameplay(|edit| edit.spawn("before", Transform::IDENTITY, None))
        .unwrap();
    let e = c.resolve(c.value).unwrap();
    w.step();
    let before = w.entity_table_revision();
    w.ecs().get_mut::<Transform>(e).unwrap().position.x = 4.;
    w.step();
    assert_eq!(before, w.entity_table_revision());
    w.ecs().get_mut::<Name>(e).unwrap().value = "after".into();
    assert_ne!(before, w.entity_table_revision());
    let renamed = w.entity_table_revision();
    w.dont_destroy_on_load(e).unwrap();
    assert_ne!(renamed, w.entity_table_revision());
    let persistent = w.entity_table_revision();
    w.ecs_mut().insert(e, Tags { bits: 8 }).unwrap();
    assert_ne!(persistent, w.entity_table_revision());
}

#[test]
fn published_snapshot_is_owned_across_mutation_and_world_destruction() {
    let mut w = World::new(Default::default());
    let c = w
        .edit_gameplay(|edit| edit.spawn("object", Transform::IDENTITY, None))
        .unwrap();
    let e = c.resolve(c.value).unwrap();
    w.subscribe(&[e], true);
    w.step();
    let (tick, old) = w.published_snapshot();
    w.ecs().get_mut::<Transform>(e).unwrap().position.x = 9.;
    w.step();
    assert_eq!(old[0].local.position.x, 0.);
    assert_eq!(w.snapshot()[0].local.position.x, 9.);
    assert!(w.tick() > tick);
    drop(w);
    std::thread::spawn(move || assert_eq!(old[0].local.position.x, 0.))
        .join()
        .unwrap();
}

#[test]
fn dynamic_membership_invalidates_tables_but_value_updates_do_not() {
    let mut w = World::new(Default::default());
    w.register_dynamic_component(ComponentSchema {
        id: "test.Counter".into(),
        version: 1,
        fields: vec![RegisteredField {
            name: "value".into(),
            default: ScalarValue::Int(0),
        }],
    })
    .unwrap();
    let c = w
        .edit_gameplay(|edit| edit.spawn("object", Transform::IDENTITY, None))
        .unwrap();
    let e = c.resolve(c.value).unwrap();
    let before = w.entity_table_revision();
    w.edit_gameplay(|edit| edit.add_registered_defaults(e, "test.Counter"))
        .unwrap();
    let added = w.entity_table_revision();
    assert_ne!(before, added);
    w.edit_gameplay(|edit| {
        edit.set_registered_values(e, "test.Counter", vec![ScalarValue::Int(7)])
    })
    .unwrap();
    assert_eq!(added, w.entity_table_revision());
    w.edit_gameplay(|edit| edit.remove_registered(e, "test.Counter"))
        .unwrap();
    assert_ne!(added, w.entity_table_revision());
}

#[cfg(feature = "physics")]
#[test]
fn sleeping_bodies_stay_clean_and_churn_retires_exact_generations() {
    let mut w = World::new(Default::default());
    let mut ids = Vec::new();
    for i in 0..128 {
        let e = scene::spawn(
            w.ecs_mut(),
            Transform::from_position(Vec3::new(i as f32 * 4., 0., 0.)),
            None,
        )
        .unwrap();
        w.ecs_mut().insert(e, Collider::default()).unwrap();
        w.ecs_mut()
            .insert(
                e,
                RigidBody {
                    gravity_scale: 0.,
                    ..Default::default()
                },
            )
            .unwrap();
        ids.push(e);
    }
    for _ in 0..240 {
        w.step();
    }
    assert!(ids
        .iter()
        .all(|&e| w.physics().body_state(e).unwrap().sleeping));
    w.refresh_physics();
    assert_eq!(w.physics().sync_candidates(), 0);
    let old = ids[17];
    w.ecs_mut().despawn(old).unwrap();
    let replacement = scene::spawn(w.ecs_mut(), Transform::IDENTITY, None).unwrap();
    w.ecs_mut()
        .insert(replacement, Collider::default())
        .unwrap();
    w.refresh_physics();
    assert_eq!(w.physics().sync_candidates(), 2);
    assert!(w.physics().body_state(old).is_none());
    w.step();
    assert_eq!(w.physics().stats().colliders, 128);
}

#[test]
fn native_destruction_retires_snapshot_subscriptions() {
    let mut w = World::new(Default::default());
    let old = scene::spawn(w.ecs_mut(), Transform::IDENTITY, None).unwrap();
    w.subscribe(&[old], true);
    w.step();
    let (_, retained) = w.published_snapshot();
    w.ecs_mut().despawn(old).unwrap();
    let replacement = scene::spawn(w.ecs_mut(), Transform::IDENTITY, None).unwrap();
    w.subscribe(&[replacement], true);
    w.step();
    assert_eq!(w.subscriptions().collect::<Vec<_>>(), vec![replacement]);
    assert_eq!(w.subscription_count(), 1);
    assert_eq!(retained[0].entity, old);
    // The reset path must retire tombstones even after history overflow.
    w.ecs_mut().despawn(replacement).unwrap();
    for _ in 0..17000 {
        let e = w.ecs_mut().spawn_empty();
        w.ecs_mut().despawn(e).unwrap();
    }
    w.step();
    assert_eq!(w.subscription_count(), 0);
    assert!(w.snapshot().is_empty());
}

#[test]
fn incremental_scene_matches_reference_through_mixed_edits_and_overflow() {
    let mut worlds = [fynkel_ecs::World::new(), fynkel_ecs::World::new()];
    let mut ids = [Vec::new(), Vec::new()];
    for lane in 0..2 {
        for i in 0..96 {
            let parent = (i > 0).then(|| ids[lane][(i - 1) / 2]);
            ids[lane].push(scene::spawn(&mut worlds[lane], Transform::IDENTITY, parent).unwrap());
        }
    }
    let mut sync = SceneSync::default();
    let mut seed = 12345u64;
    for round in 0..600 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let i = ((seed >> 32) as usize % 95) + 1;
        for lane in 0..2 {
            let e = ids[lane][i];
            match round % 3 {
                0 => {
                    worlds[lane].get_mut::<Transform>(e).unwrap().position.x = round as f32;
                    if lane == 1 {
                        scene::mark_dirty(&worlds[lane], e).unwrap();
                    }
                }
                1 => {
                    scene::set_active(&mut worlds[lane], e, round % 2 == 0).unwrap();
                }
                _ => {
                    scene::set_parent_keep_local(&mut worlds[lane], e, Some(ids[lane][i / 3]))
                        .unwrap();
                }
            }
            if round == 300 {
                for _ in 0..17000 {
                    worlds[lane].get_mut::<Transform>(e).unwrap().position.y += 1.;
                }
                if lane == 1 {
                    scene::mark_dirty(&worlds[lane], e).unwrap();
                }
            }
        }
        sync.refresh_active(&mut worlds[0]);
        sync.propagate(&mut worlds[0]);
        scene::refresh_active(&mut worlds[1]);
        scene::propagate(&mut worlds[1]);
        for (i, (&actual, &expected)) in ids[0].iter().zip(&ids[1]).enumerate() {
            assert_eq!(
                scene::world_matrix(&worlds[0], actual),
                scene::world_matrix(&worlds[1], expected),
                "round {round}, entity {i}"
            );
            assert_eq!(
                scene::is_active_in_hierarchy(&worlds[0], actual),
                scene::is_active_in_hierarchy(&worlds[1], expected)
            );
        }
    }
}

#[test]
fn resource_owners_remain_sparse_when_particle_spawn_precedes_dense_pose_writes() {
    let mut w = World::new(Default::default());
    let moving = scene::spawn(w.ecs_mut(), Transform::IDENTITY, None).unwrap();
    let (cursor, _, _) = w.resource_changes(0).unwrap();
    let particle = scene::spawn(w.ecs_mut(), Transform::IDENTITY, None).unwrap();
    w.ecs_mut()
        .insert(particle, MeshRenderer::default())
        .unwrap();
    for i in 0..20000 {
        w.ecs_mut().get_mut::<Transform>(moving).unwrap().position.x = i as f32;
    }
    let (next, reset, owners) = w.resource_changes(cursor).unwrap();
    assert!(
        !reset,
        "pose history overflow must not rescan every asset owner"
    );
    assert_eq!(owners, vec![particle]);
    assert!(w.resource_changes(next).unwrap().2.is_empty());
    w.ecs_mut().despawn(particle).unwrap();
    for i in 0..20000 {
        w.ecs_mut().get_mut::<Transform>(moving).unwrap().position.x = i as f32;
    }
    let (_, reset, owners) = w.resource_changes(next).unwrap();
    assert!(!reset);
    assert_eq!(
        owners,
        vec![particle],
        "destroyed owners must still release assets"
    );
}
