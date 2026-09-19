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
    w.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id: 1,
            transform: at(x),
            parent: None,
            name: None,
        }],
    );
    let r = w.step();
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
    let ev = w.drain_events();
    match ev.as_slice() {
        [Event {
            payload: EventPayload::Spawned { temp_id: 1, entity },
            ..
        }] => *entity,
        other => panic!("{other:?}"),
    }
}

fn pos(m: Mat4) -> Vec3 {
    m.transform_point3(Vec3::ZERO)
}

#[test]
fn simulation_commands_apply_at_the_next_tick_only() {
    let mut w = world();
    let e = spawn_one(&mut w, 0.0);
    w.subscribe(&[e], true);
    w.submit(
        Lane::Simulation,
        vec![Command::SetTransform {
            entity: e,
            transform: at(5.0),
        }],
    );
    // Not applied yet: storage and extract still at the origin.
    assert_eq!(w.extract().records[0].world, Mat4::IDENTITY);
    let r = w.step();
    assert_eq!((r.applied, r.rejected, r.snapshot_records), (1, 0, 1));
    assert_eq!(w.snapshot()[0].local.position.x, 5.0);
    assert_eq!(w.snapshot()[0].world.position.x, 5.0);
}

#[test]
fn spawn_maps_temp_ids_and_destroy_removes_subtree() {
    let mut w = world();
    let root = spawn_one(&mut w, 0.0);
    w.submit(
        Lane::Simulation,
        vec![
            Command::Spawn {
                temp_id: 7,
                transform: at(1.0),
                parent: Some(root),
                name: Some("child".into()),
            },
            Command::Spawn {
                temp_id: 8,
                transform: at(2.0),
                parent: None,
                name: None,
            },
        ],
    );
    w.step();
    let ev = w.drain_events();
    assert_eq!(ev.len(), 2);
    let child = match &ev[0].payload {
        EventPayload::Spawned { temp_id: 7, entity } => *entity,
        p => panic!("{p:?}"),
    };
    assert_eq!(w.ecs().get::<Name>(child).unwrap().0, "child");
    assert_eq!(w.live_entities(), 3);
    w.subscribe(&[root, child], true);

    w.submit(Lane::Simulation, vec![Command::Destroy(root)]);
    let r = w.step();
    assert_eq!(r.live_entities, 1);
    assert_eq!(
        r.snapshot_records, 0,
        "subscriptions of destroyed entities drop"
    );
    let ev = w.drain_events();
    assert_eq!(
        ev.iter().map(|e| e.payload.clone()).collect::<Vec<_>>(),
        vec![
            EventPayload::Destroyed { entity: root },
            EventPayload::Destroyed { entity: child }
        ]
    );
    // Stale handle fails safely with a diagnostic.
    w.submit(Lane::Simulation, vec![Command::Destroy(root)]);
    let r = w.step();
    assert_eq!(r.status, Status::CommandsRejected);
    assert_eq!(r.rejected, 1);
    let d = &w.diagnostics()[0];
    assert_eq!(
        (d.command_index, d.code, d.entity),
        (0, DiagnosticCode::STALE_ENTITY, root)
    );
}

#[test]
fn presentation_lane_overrides_extract_and_stages_authoritative_value() {
    let mut w = world();
    let e = spawn_one(&mut w, 0.0);
    // Frame with no tick: presentation write must still show in the extract.
    let plan = w.begin_frame(0.001);
    assert_eq!(plan.steps, 0);
    let r = w.submit(
        Lane::Presentation,
        vec![Command::SetTransform {
            entity: e,
            transform: at(3.0),
        }],
    );
    assert_eq!(r.status, Status::Ok);
    let x = w.extract();
    assert_eq!(pos(x.records[0].world).x, 3.0);
    assert!(x.records[0].overridden);
    // Storage untouched until the next tick...
    assert_eq!(w.ecs().copy::<Transform>(e).unwrap().position.x, 0.0);
    // ...next frame's tick makes it authoritative; no snap back.
    let plan = w.begin_frame(1.0 / 60.0);
    assert_eq!(plan.steps, 1);
    w.step();
    assert_eq!(w.ecs().copy::<Transform>(e).unwrap().position.x, 3.0);
    let x = w.extract();
    assert!(!x.records[0].overridden);
    // Not interpolated from the old S[n]: the value was already shown.
    assert_eq!(pos(x.records[0].world).x, 3.0);
    let _ = w.begin_frame(1.0 / 120.0);
    assert_eq!(pos(w.extract().records[0].world).x, 3.0);
}

#[test]
fn presentation_lane_rejects_structural_and_physics_owned() {
    let mut w = world();
    let e = spawn_one(&mut w, 0.0);
    w.ecs_mut().insert(e, PhysicsOwned).unwrap();
    let r = w.submit(
        Lane::Presentation,
        vec![
            Command::SetTransform {
                entity: e,
                transform: at(1.0),
            },
            Command::Destroy(e),
            Command::SetName {
                entity: e,
                name: "ok".into(),
            },
        ],
    );
    assert_eq!(
        (r.status, r.accepted, r.rejected),
        (Status::CommandsRejected, 1, 2)
    );
    let codes: Vec<_> = w
        .diagnostics()
        .iter()
        .map(|d| (d.command_index, d.code))
        .collect();
    assert_eq!(
        codes,
        vec![
            (0, DiagnosticCode::PHYSICS_OWNED),
            (1, DiagnosticCode::WRONG_LANE)
        ]
    );
    assert!(w.diagnostics().iter().all(|d| d.lane == Lane::Presentation));
    assert!(w.ecs().contains(e));
    assert_eq!(
        w.extract().records[0].world,
        Mat4::IDENTITY,
        "rejected override not applied"
    );
}

#[test]
fn extract_interpolates_between_ticks() {
    let mut w = world();
    let e = spawn_one(&mut w, 0.0);
    w.submit(
        Lane::Simulation,
        vec![Command::SetTransform {
            entity: e,
            transform: at(10.0),
        }],
    );
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
fn hierarchy_commands_reparent_and_reject_cycles() {
    let mut w = world();
    let a = spawn_one(&mut w, 1.0);
    let b = spawn_one(&mut w, 2.0);
    w.submit(
        Lane::Simulation,
        vec![
            Command::SetParent {
                child: b,
                parent: Some(a),
                keep_world: true,
            },
            Command::SetParent {
                child: a,
                parent: Some(b),
                keep_world: false,
            },
        ],
    );
    let r = w.step();
    assert_eq!((r.applied, r.rejected), (1, 1));
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::HIERARCHY_CYCLE);
    assert_eq!(w.diagnostics()[0].command_index, 1);
    // keep_world: b still at x=2 in world space, local is x=1.
    assert_eq!(pos(w.extract().records[1].world).x, 2.0);
    assert!((w.ecs().copy::<Transform>(b).unwrap().position.x - 1.0).abs() < 1e-5);
    let ev = w.drain_events();
    assert!(
        matches!(ev[0].payload, EventPayload::ParentChanged { child, parent: Some(p) } if child == b && p == a)
    );
}

#[test]
fn events_are_pull_only_and_drained_once() {
    let mut w = world();
    let _ = spawn_one(&mut w, 0.0);
    assert!(w.drain_events().is_empty(), "already drained by spawn_one");
    w.push_event(EventPayload::SceneReady);
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
    let foreign = Entity::new(WorldId(2), e.slot(), e.generation());
    w.submit(Lane::Simulation, vec![Command::Destroy(foreign)]);
    let r = w.step();
    assert_eq!(r.rejected, 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::WRONG_WORLD);

    w.poison();
    assert_eq!(w.step().status, Status::Poisoned);
    assert_eq!(
        w.submit(Lane::Simulation, vec![Command::Destroy(e)]).status,
        Status::Poisoned
    );
    assert!(w.ecs().contains(e));
}

#[test]
fn batch_of_ten_thousand_round_trips() {
    let mut w = world();
    let cmds: Vec<Command> = (1..=10_000)
        .map(|i| Command::Spawn {
            temp_id: i,
            transform: at(i as f32),
            parent: None,
            name: None,
        })
        .collect();
    w.submit(Lane::Simulation, cmds);
    let r = w.step();
    assert_eq!(
        (r.applied, r.live_entities, r.event_records),
        (10_000, 10_000, 10_000)
    );
    let ids: Vec<Entity> = w
        .drain_events()
        .into_iter()
        .filter_map(|e| match e.payload {
            EventPayload::Spawned { entity, .. } => Some(entity),
            _ => None,
        })
        .collect();
    w.subscribe(&ids, true);
    w.submit(
        Lane::Simulation,
        ids.iter()
            .map(|&e| Command::SetTransform {
                entity: e,
                transform: at(-1.0),
            })
            .collect(),
    );
    let r = w.step();
    assert_eq!(r.snapshot_records, 10_000);
    assert!(w.snapshot().iter().all(|s| s.local.position.x == -1.0));
    w.submit(
        Lane::Simulation,
        ids.iter().map(|&e| Command::Destroy(e)).collect(),
    );
    assert_eq!(w.step().live_entities, 0);
    assert_eq!(w.extract().records.len(), 0);
}
