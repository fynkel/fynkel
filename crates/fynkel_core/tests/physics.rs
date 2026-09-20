//! Physics through the world (blueprint §9, §19.2d item 8): components set
//! by commands become bodies at the §5.1 physics slot, body commands move
//! them, contacts reach `after_physics_events`, dynamic transforms are
//! physics-owned.

use fynkel_core::components::ComponentValue;
use fynkel_core::*;
use glam::{Quat, Vec3};

fn world() -> World {
    World::new(WorldConfig::default())
}

fn spawn(w: &mut World, temp_id: u64, at: Vec3, components: Vec<ComponentValue>) -> Entity {
    let mut cmds = vec![Command::Spawn {
        temp_id,
        transform: Transform::from_position(at),
        parent: None,
        name: None,
    }];
    w.submit(Lane::Simulation, std::mem::take(&mut cmds));
    let r = w.step();
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
    let entity = w
        .drain_events()
        .into_iter()
        .find_map(|e| match e.payload {
            EventPayload::Spawned { temp_id: t, entity } if t == temp_id => Some(entity),
            _ => None,
        })
        .unwrap();
    let cmds: Vec<Command> = components
        .into_iter()
        .map(|value| Command::SetComponent { entity, value })
        .collect();
    if !cmds.is_empty() {
        w.submit(Lane::Simulation, cmds);
    }
    entity
}

fn floor(w: &mut World) -> Entity {
    spawn(
        w,
        1,
        Vec3::new(0.0, -0.5, 0.0),
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(20.0, 0.5, 20.0),
            ..Collider::default()
        })],
    )
}

fn y(w: &World, e: Entity) -> f32 {
    w.ecs().copy::<Transform>(e).unwrap().position.y
}

#[test]
fn a_dynamic_body_falls_is_owned_and_lands_once() {
    let mut w = world();
    let floor = floor(&mut w);
    let crate_ = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 3.0, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    // The components apply at the next tick; the body exists after it.
    assert!(!w.ecs().has::<PhysicsOwned>(crate_));
    w.step();
    assert!(w.ecs().has::<PhysicsOwned>(crate_));
    assert!(!w.ecs().has::<PhysicsOwned>(floor));
    assert!(y(&w, crate_) < 3.0, "fell during its first tick");

    let mut enters = 0;
    let mut stays = 0;
    for _ in 0..240 {
        w.step();
        for e in w.after_physics_events() {
            match &e.payload {
                EventPayload::CollisionEnter(c) => {
                    enters += 1;
                    assert!((c.a == crate_ && c.b == floor) || (c.a == floor && c.b == crate_));
                    assert!(c.normal.y.abs() > 0.9, "{:?}", c.normal);
                    assert!(c.impulse > 0.0);
                    assert!(c.point.y.abs() < 0.1, "{:?}", c.point);
                }
                EventPayload::CollisionStay(_) => stays += 1,
                _ => {}
            }
        }
    }
    assert_eq!(enters, 1);
    assert_eq!(stays, 0, "nobody subscribed: no stay stream");
    assert!((y(&w, crate_) - 0.5).abs() < 0.02, "{}", y(&w, crate_));
    // Physics events also arrive on the drained stream (physics lane).
    let drained = w.drain_events();
    assert!(drained
        .iter()
        .any(|e| matches!(e.payload, EventPayload::CollisionEnter(_))));

    // A presentation transform write on the owned body is rejected; the
    // simulation lane teleports it.
    let r = w.submit(
        Lane::Presentation,
        vec![Command::SetTransform {
            entity: crate_,
            transform: Transform::from_position(Vec3::new(0.0, 5.0, 0.0)),
        }],
    );
    assert_eq!(r.rejected, 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::PHYSICS_OWNED);
    w.submit(
        Lane::Simulation,
        vec![Command::SetTransform {
            entity: crate_,
            transform: Transform::from_position(Vec3::new(0.0, 5.0, 0.0)),
        }],
    );
    let r = w.step();
    assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
    assert!((y(&w, crate_) - 5.0).abs() < 0.01, "{}", y(&w, crate_));
    // Subscribed: the resting contact streams as stays.
    w.subscribe(&[crate_], true);
    for _ in 0..240 {
        w.step();
        stays += w
            .after_physics_events()
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::CollisionStay(_)))
            .count();
    }
    assert!(stays > 100, "{stays}");
    // Removing the body releases the transform.
    w.submit(
        Lane::Simulation,
        vec![Command::RemoveComponent {
            entity: crate_,
            component: ComponentId::RIGID_BODY,
        }],
    );
    w.step();
    assert!(!w.ecs().has::<PhysicsOwned>(crate_));
    assert_eq!(w.physics().body_state(crate_), None);
}

#[test]
fn body_commands_move_bodies_and_are_checked() {
    let mut w = world();
    let ball = spawn(
        &mut w,
        1,
        Vec3::ZERO,
        vec![
            ComponentValue::RigidBody(RigidBody {
                gravity_scale: 0.0,
                ..RigidBody::default()
            }),
            ComponentValue::Collider(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.5, 0.0, 0.0),
                ..Collider::default()
            }),
        ],
    );
    let platform = spawn(
        &mut w,
        2,
        Vec3::new(5.0, 0.0, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody {
                body_type: BodyType::Kinematic,
                ..RigidBody::default()
            }),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    let plain = spawn(&mut w, 3, Vec3::ZERO, vec![]);
    // Component writes and body commands in one batch: the body receives
    // the command the same tick.
    let r = w.submit(
        Lane::Simulation,
        vec![
            Command::Body {
                entity: ball,
                op: BodyCommand::SetLinearVelocity(Vec3::X * 6.0),
            },
            Command::Body {
                entity: plain,
                op: BodyCommand::ApplyImpulse(Vec3::X),
            },
            Command::Body {
                entity: platform,
                op: BodyCommand::ApplyForce(Vec3::X),
            },
            Command::Body {
                entity: ball,
                op: BodyCommand::ApplyTorque(Vec3::new(f32::NAN, 0.0, 0.0)),
            },
            Command::Body {
                entity: platform,
                op: BodyCommand::SetKinematicTarget {
                    position: Vec3::new(5.0, 1.0, 0.0),
                    rotation: Quat::IDENTITY,
                },
            },
        ],
    );
    assert_eq!(r.accepted, 5, "checked at the tick");
    let r = w.step();
    assert_eq!((r.applied, r.rejected), (2, 3), "{:?}", w.diagnostics());
    let d = w.diagnostics();
    assert_eq!(
        d.iter()
            .map(|d| (d.command_index, d.code, d.component))
            .collect::<Vec<_>>(),
        vec![
            (
                1,
                DiagnosticCode::MISSING_COMPONENT,
                ComponentId::RIGID_BODY
            ),
            (2, DiagnosticCode::INVALID_PAYLOAD, ComponentId::RIGID_BODY),
            (3, DiagnosticCode::INVALID_PAYLOAD, ComponentId::RIGID_BODY),
        ]
    );
    assert!(d[1].message.contains("dynamic"));
    let s = w.physics().body_state(ball).unwrap();
    assert!((s.linear_velocity.x - 6.0).abs() < 1e-4);
    assert_eq!(s.body_type, BodyType::Dynamic);
    let p = w.ecs().copy::<Transform>(platform).unwrap().position;
    assert!((p.y - 1.0).abs() < 1e-4, "{p:?}");
    assert!(
        !w.ecs().has::<PhysicsOwned>(platform),
        "kinematic: gameplay-owned"
    );

    // Body commands are simulation-only.
    let r = w.submit(
        Lane::Presentation,
        vec![Command::Body {
            entity: ball,
            op: BodyCommand::ApplyImpulse(Vec3::X),
        }],
    );
    assert_eq!(r.rejected, 1);
    assert_eq!(
        w.diagnostics().last().unwrap().code,
        DiagnosticCode::WRONG_LANE
    );

    // Teleport keeps the velocity and does not interpolate.
    w.submit(
        Lane::Simulation,
        vec![Command::Body {
            entity: ball,
            op: BodyCommand::Teleport {
                position: Vec3::new(0.0, 10.0, 0.0),
                rotation: Quat::IDENTITY,
            },
        }],
    );
    w.begin_frame(1.0 / 60.0);
    w.step();
    let t = w.ecs().copy::<Transform>(ball).unwrap().position;
    assert!(
        (t.y - 10.0).abs() < 1e-4 && (t.x - 0.1).abs() < 1e-3,
        "{t:?}"
    );
    assert!((w.physics().body_state(ball).unwrap().linear_velocity.x - 6.0).abs() < 1e-4);
    let x = w.extract();
    let rec = x.records.iter().find(|r| r.entity == ball).unwrap();
    assert!(
        (rec.world.transform_point3(Vec3::ZERO).y - 10.0).abs() < 1e-3,
        "shown at the teleported pose, not between"
    );
}

#[test]
fn a_trigger_reaches_after_physics_exactly_once() {
    let mut w = world();
    let _floor = floor(&mut w);
    let zone = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 1.5, 0.0),
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(1.0, 0.25, 1.0),
            is_sensor: true,
            ..Collider::default()
        })],
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
    let (mut enter, mut exit) = (0, 0);
    let mut enter_tick = 0;
    for _ in 0..240 {
        w.step();
        for e in w.after_physics_events() {
            match &e.payload {
                EventPayload::TriggerEnter(c) => {
                    enter += 1;
                    enter_tick = e.tick;
                    assert!((c.a == zone && c.b == ball) || (c.a == ball && c.b == zone));
                    assert_eq!(c.impulse, 0.0);
                    assert!(w.tick() == e.tick, "reported the tick it happened");
                }
                EventPayload::TriggerExit(_) => exit += 1,
                _ => {}
            }
        }
    }
    assert_eq!((enter, exit), (1, 1));
    assert!(enter_tick > 3);
    // The drained stream carried each once too.
    let drained = w.drain_events();
    assert_eq!(
        drained
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::TriggerEnter(_)))
            .count(),
        1
    );
    assert!(w.drain_events().is_empty());
}

#[test]
fn invalid_component_values_are_rejected_at_the_tick() {
    let mut w = world();
    let e = spawn(&mut w, 1, Vec3::ZERO, vec![]);
    w.submit(
        Lane::Simulation,
        vec![
            Command::SetComponent {
                entity: e,
                value: ComponentValue::RigidBody(RigidBody {
                    mass: 0.0,
                    ..RigidBody::default()
                }),
            },
            Command::SetComponent {
                entity: e,
                value: ComponentValue::Collider(Collider {
                    shape: ColliderShape::Mesh,
                    ..Collider::default()
                }),
            },
            Command::SetComponent {
                entity: e,
                value: ComponentValue::Collider(Collider {
                    restitution: 2.0,
                    ..Collider::default()
                }),
            },
        ],
    );
    let r = w.step();
    assert_eq!(r.rejected, 3, "{:?}", w.diagnostics());
    assert!(w
        .diagnostics()
        .iter()
        .all(|d| d.code == DiagnosticCode::INVALID_PAYLOAD));
    assert!(!w.ecs().has::<RigidBody>(e));
    assert!(!w.ecs().has::<Collider>(e));
    // A mesh collider whose mesh is not registered: the component is
    // stored, the collider skipped with `MESH_UNAVAILABLE` (once).
    w.submit(
        Lane::Simulation,
        vec![Command::SetComponent {
            entity: e,
            value: ComponentValue::Collider(Collider {
                shape: ColliderShape::Mesh,
                mesh: Some(AssetRef::new(
                    "70d7ba8b-f7fb-4eba-920d-ec9e6b968330".parse().unwrap(),
                )),
                ..Collider::default()
            }),
        }],
    );
    let r = w.step();
    assert_eq!(r.rejected, 0);
    assert_eq!(w.diagnostics().len(), 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::MESH_UNAVAILABLE);
    assert_eq!(w.diagnostics()[0].command_index, u32::MAX);
    w.step();
    assert!(w.diagnostics().is_empty(), "reported once");
    assert!(w.ecs().has::<Collider>(e));
    assert_eq!(w.physics().stats().colliders, 0);
}

#[test]
fn the_step_is_deterministic_across_worlds() {
    let run = || {
        let mut w = world();
        let _floor = floor(&mut w);
        for i in 0..20u64 {
            spawn(
                &mut w,
                10 + i,
                Vec3::new(
                    (i % 5) as f32 * 0.9 - 2.0,
                    2.0 + i as f32 * 1.1,
                    (i / 5) as f32 * 0.9,
                ),
                vec![
                    ComponentValue::RigidBody(RigidBody::default()),
                    ComponentValue::Collider(Collider {
                        friction: 0.3,
                        restitution: 0.2,
                        ..Collider::default()
                    }),
                ],
            );
        }
        for _ in 0..300 {
            w.step();
        }
        w.extract()
            .records
            .iter()
            .map(|r| r.world.to_cols_array())
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}
