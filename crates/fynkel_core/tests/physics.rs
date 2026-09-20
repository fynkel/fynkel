//! Physics through the world (blueprint §9, §19.2d item 8): components set
//! by commands become bodies at the §5.1 physics slot, body commands move
//! them, contacts reach `after_physics_events`, dynamic transforms are
//! physics-owned.

use fynkel_core::components::ComponentValue;
use fynkel_core::*;
use glam::{Quat, Vec3, Vec3Swizzles};

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

fn named(
    w: &mut World,
    temp_id: u64,
    name: &str,
    at: Vec3,
    components: Vec<ComponentValue>,
) -> Entity {
    let e = spawn(w, temp_id, at, components);
    w.submit(
        Lane::Simulation,
        vec![Command::SetName {
            entity: e,
            name: name.into(),
        }],
    );
    e
}

fn playground() -> (World, Entity, Entity) {
    let mut w = world();
    let errors = w.bind_input(&[
        ActionSpec {
            name: "move".into(),
            kind: ActionKind::Axis2d,
            bindings: vec!["keyboard:WASD".into()],
        },
        ActionSpec {
            name: "jump".into(),
            kind: ActionKind::Button,
            bindings: vec!["keyboard:Space".into()],
        },
        ActionSpec {
            name: "look".into(),
            kind: ActionKind::Axis2d,
            bindings: vec!["mouse:delta".into(), "gamepad:rightStick".into()],
        },
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    floor(&mut w);
    let player = named(
        &mut w,
        2,
        "Player",
        Vec3::ZERO,
        vec![ComponentValue::CharacterController(CharacterController {
            move_action: "move".into(),
            jump_action: "jump".into(),
            ..CharacterController::default()
        })],
    );
    let camera = named(
        &mut w,
        3,
        "Camera",
        Vec3::new(0.0, 2.0, 6.0),
        vec![ComponentValue::ThirdPersonCamera(ThirdPersonCamera {
            target: "Player".into(),
            look_action: "look".into(),
            ..ThirdPersonCamera::default()
        })],
    );
    (w, player, camera)
}

fn pos(w: &World, e: Entity) -> Vec3 {
    w.ecs().copy::<Transform>(e).unwrap().position
}

fn frame(w: &mut World, events: &[InputEvent]) {
    w.begin_frame(1.0 / 60.0);
    w.capture_input(events);
    let r = w.step();
    assert_eq!(r.status, Status::Ok);
}

/// Blueprint §19.2d item 9 through the world: the controller's actions
/// drive it in the frame of the rig that targets it, a jump consumes the
/// press edge, the rig turns by its look action, follows the pivot and
/// pulls in before a wall.
#[test]
fn a_character_is_driven_by_actions_in_its_cameras_frame() {
    let (mut w, player, camera) = playground();
    // The components apply at this tick; the rig is placed the same tick
    // (pitch 0.35 rad, 5 m behind the pivot 1.5 m above the feet).
    w.step();
    let rig = w.ecs().copy::<ThirdPersonCamera>(camera).unwrap();
    assert_eq!((rig.yaw, rig.pitch), (0.0, 0.35));
    let expected = Vec3::new(0.0, 1.5 + 0.35f32.sin() * 5.0, 0.35f32.cos() * 5.0);
    assert!(
        pos(&w, camera).abs_diff_eq(expected, 1e-3),
        "{:?}",
        pos(&w, camera)
    );
    let state = w.physics().character_state(player).unwrap();
    assert_eq!(state.velocity, Vec3::ZERO);

    // W walks along −Z (the rig's forward at yaw 0) at `speed`; the
    // character stays grounded and the rig follows.
    frame(&mut w, &[InputEvent::down(Device::Keyboard, "W")]);
    for _ in 0..29 {
        frame(&mut w, &[]);
    }
    let p = pos(&w, player);
    assert!((p.z + 30.0 * 4.0 / 60.0).abs() < 1e-3, "{p:?}");
    assert!(p.x.abs() < 1e-4 && p.y.abs() < 0.01, "{p:?}");
    let state = w.physics().character_state(player).unwrap();
    assert!(state.grounded && state.velocity.abs_diff_eq(Vec3::new(0.0, 0.0, -4.0), 1e-4));
    assert!(pos(&w, camera).abs_diff_eq(expected + Vec3::new(0.0, 0.0, p.z), 1e-3));

    // The stick turns the rig: yaw −= x·sensitivity·dt per tick, written
    // back to the component; the next walk follows the new forward.
    frame(
        &mut w,
        &[InputEvent::axis(Device::Gamepad, "rightStick", 1.0, 0.0)],
    );
    let rig = w.ecs().copy::<ThirdPersonCamera>(camera).unwrap();
    assert!((rig.yaw + 2.5 / 60.0).abs() < 1e-5, "{}", rig.yaw);
    frame(&mut w, &[]);
    let yaw = w.ecs().copy::<ThirdPersonCamera>(camera).unwrap().yaw;
    assert!((yaw + 2.0 * 2.5 / 60.0).abs() < 1e-5, "held: {yaw}");
    frame(
        &mut w,
        &[InputEvent::axis(Device::Gamepad, "rightStick", 0.0, 0.0)],
    );
    assert_eq!(w.ecs().copy::<ThirdPersonCamera>(camera).unwrap().yaw, yaw);
    let before = pos(&w, player);
    frame(&mut w, &[]);
    let step = pos(&w, player) - before;
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos()) * 4.0 / 60.0;
    assert!(
        step.xz().abs_diff_eq(forward.xz(), 1e-4),
        "{step:?} vs {forward:?}"
    );
    // The character faces its motion.
    let facing = w.ecs().copy::<Transform>(player).unwrap().rotation * Vec3::NEG_Z;
    assert!(facing.dot(forward.normalize()) > 0.99, "{facing:?}");

    // The mouse turns it too: a relative control's motion is the rate that
    // covers it in one tick, so one tick turns by motion·sensitivity —
    // seen by the rig even though it runs late in the tick — and the
    // catch-up ticks of the same frame add nothing.
    w.begin_frame(2.0 / 60.0);
    w.capture_input(&[InputEvent::moved(Device::Mouse, "delta", 0.2, 0.0)]);
    w.step();
    let turned = w.ecs().copy::<ThirdPersonCamera>(camera).unwrap().yaw;
    assert!(
        (turned - (yaw - 0.2 * 2.5)).abs() < 1e-4,
        "{turned} vs {yaw}"
    );
    w.step();
    assert_eq!(
        w.ecs().copy::<ThirdPersonCamera>(camera).unwrap().yaw,
        turned
    );

    // Space: the press edge is consumed by the first tick (a jump), the
    // catch-up ticks of the same frame see held only (no second jump).
    w.begin_frame(3.0 / 60.0);
    w.capture_input(&[InputEvent::down(Device::Keyboard, "Space")]);
    let mut vy = Vec::new();
    for _ in 0..3 {
        w.step();
        vy.push(w.physics().character_state(player).unwrap().velocity.y);
    }
    assert!(vy[0] > 4.5 && vy[1] < vy[0] && vy[2] < vy[1], "{vy:?}");
    assert!(!w.physics().character_state(player).unwrap().grounded);
    // Held Space in the air does nothing; landing restores grounded.
    for _ in 0..80 {
        frame(&mut w, &[]);
    }
    let state = w.physics().character_state(player).unwrap();
    assert!(state.grounded && state.velocity.y == 0.0, "{state:?}");
    assert!(pos(&w, player).y.abs() < 0.03);
    frame(
        &mut w,
        &[
            InputEvent::up(Device::Keyboard, "Space"),
            InputEvent::up(Device::Keyboard, "W"),
        ],
    );
    frame(&mut w, &[]);
    assert_eq!(
        w.physics().character_state(player).unwrap().velocity,
        Vec3::ZERO
    );
    assert_eq!(w.diagnostics().len(), 0);
}

#[test]
fn the_rig_pulls_in_before_a_wall_and_needs_a_named_target() {
    let (mut w, player, camera) = playground();
    let wall_z = 3.0;
    let wall = spawn(
        &mut w,
        4,
        Vec3::new(0.0, 2.0, wall_z),
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(3.0, 3.0, 0.25),
            ..Collider::default()
        })],
    );
    w.step();
    // A sphere of `collision_radius` cast from the pivot along the rig's
    // direction touches the wall's near face; the rig sits there.
    let rig = w.ecs().copy::<ThirdPersonCamera>(camera).unwrap();
    let pivot = Vec3::new(0.0, 1.5, 0.0);
    let dir = Vec3::new(0.0, rig.pitch.sin(), rig.pitch.cos());
    let expected = (wall_z - 0.25 - rig.collision_radius) / rig.pitch.cos();
    let d = (pos(&w, camera) - pivot).length();
    assert!((d - expected).abs() < 1e-3, "{d} vs {expected}");
    assert!((pos(&w, camera) - pivot).normalize().abs_diff_eq(dir, 1e-4));
    // Looking at the pivot.
    let look = w.ecs().copy::<Transform>(camera).unwrap().rotation * Vec3::NEG_Z;
    assert!(look.abs_diff_eq(-dir, 1e-4), "{look:?}");
    // The wall gone, the rig backs out to `distance`.
    w.submit(Lane::Simulation, vec![Command::Destroy(wall)]);
    w.step();
    w.step();
    assert!(((pos(&w, camera) - pivot).length() - rig.distance).abs() < 1e-3);
    // No entity of that name: the rig stays where it is; the pitch is
    // clamped when written back.
    let parked = pos(&w, camera);
    w.submit(
        Lane::Simulation,
        vec![Command::SetComponent {
            entity: camera,
            value: ComponentValue::ThirdPersonCamera(ThirdPersonCamera {
                target: "Nobody".into(),
                pitch: 3.0,
                ..ThirdPersonCamera::default()
            }),
        }],
    );
    w.step();
    assert_eq!(pos(&w, camera), parked);
    w.submit(
        Lane::Simulation,
        vec![Command::SetComponent {
            entity: camera,
            value: ComponentValue::ThirdPersonCamera(ThirdPersonCamera {
                target: "Player".into(),
                pitch: 3.0,
                ..ThirdPersonCamera::default()
            }),
        }],
    );
    w.step();
    let rig = w.ecs().copy::<ThirdPersonCamera>(camera).unwrap();
    assert_eq!(rig.pitch, rig.max_pitch);
    assert!((pos(&w, camera).y - (1.5 + rig.max_pitch.sin() * 5.0)).abs() < 1e-3);
    let _ = player;
}

#[test]
fn character_commands_and_components_are_checked() {
    let (mut w, player, _camera) = playground();
    let plain = spawn(&mut w, 5, Vec3::new(3.0, 0.0, 0.0), vec![]);
    let ball = spawn(
        &mut w,
        6,
        Vec3::new(-3.0, 1.0, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    w.step();
    let r = w.submit(
        Lane::Simulation,
        vec![
            Command::Body {
                entity: player,
                op: BodyCommand::Move(Vec3::new(2.0, 0.0, 0.0)),
            },
            Command::Body {
                entity: plain,
                op: BodyCommand::Move(Vec3::X),
            },
            Command::Body {
                entity: ball,
                op: BodyCommand::Jump(1.0),
            },
            Command::Body {
                entity: player,
                op: BodyCommand::ApplyImpulse(Vec3::X),
            },
            Command::Body {
                entity: player,
                op: BodyCommand::Jump(f32::NAN),
            },
        ],
    );
    assert_eq!(r.accepted, 5);
    let r = w.step();
    assert_eq!((r.applied, r.rejected), (1, 4), "{:?}", w.diagnostics());
    let d = w.diagnostics();
    assert_eq!(
        d.iter()
            .map(|d| (d.command_index, d.code, d.component))
            .collect::<Vec<_>>(),
        vec![
            (
                1,
                DiagnosticCode::MISSING_COMPONENT,
                ComponentId::CHARACTER_CONTROLLER
            ),
            (
                2,
                DiagnosticCode::MISSING_COMPONENT,
                ComponentId::CHARACTER_CONTROLLER
            ),
            (
                3,
                DiagnosticCode::MISSING_COMPONENT,
                ComponentId::RIGID_BODY
            ),
            (
                4,
                DiagnosticCode::INVALID_PAYLOAD,
                ComponentId::CHARACTER_CONTROLLER
            ),
        ]
    );
    let s = w.physics().character_state(player).unwrap();
    assert!(
        s.velocity.abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1e-4),
        "{s:?}"
    );
    assert!((pos(&w, player).x - 2.0 / 60.0).abs() < 1e-4);
    // The next tick has no `move`: standing still.
    w.step();
    assert_eq!(
        w.physics().character_state(player).unwrap().velocity,
        Vec3::ZERO
    );

    // A RigidBody next to the controller is reported once (index
    // u32::MAX) and ignored; an invalid controller value is rejected at
    // the tick.
    w.submit(
        Lane::Simulation,
        vec![Command::SetComponent {
            entity: player,
            value: ComponentValue::RigidBody(RigidBody::default()),
        }],
    );
    w.step();
    let d = w.diagnostics();
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(
        (d[0].command_index, d[0].code, d[0].component),
        (
            u32::MAX,
            DiagnosticCode::INVALID_PAYLOAD,
            ComponentId::CHARACTER_CONTROLLER
        )
    );
    w.step();
    assert!(w.diagnostics().is_empty());
    assert!(w.physics().character_state(player).is_some());
    let r = w.submit(
        Lane::Simulation,
        vec![Command::SetComponent {
            entity: player,
            value: ComponentValue::CharacterController(CharacterController {
                height: 0.1,
                ..CharacterController::default()
            }),
        }],
    );
    assert_eq!(r.accepted, 1);
    let r = w.step();
    assert_eq!(r.rejected, 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::INVALID_PAYLOAD);
    assert_eq!(
        w.diagnostics()[0].component,
        ComponentId::CHARACTER_CONTROLLER
    );

    // Removing the controller ends the character; the body takes over.
    w.submit(
        Lane::Simulation,
        vec![Command::RemoveComponent {
            entity: player,
            component: ComponentId::CHARACTER_CONTROLLER,
        }],
    );
    w.step();
    assert!(w.physics().character_state(player).is_none());
    assert!(w.ecs().has::<PhysicsOwned>(player));
}
