//! Physics through the world (blueprint §9, §19.2d item 8): components set
//! by commands become bodies at the §5.1 physics slot, body commands move
//! them, contacts reach `after_physics_events`, dynamic transforms are
//! physics-owned.

use fynkel_contracts::ObjectRef;
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
                    assert!(c.impulse.length() > 0.0);
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
                    assert_eq!(c.impulse, Vec3::ZERO);
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
    // A collider naming a physics material nobody registered (ADR-0025):
    // it collides on its own friction and restitution, reported once.
    w.submit(
        Lane::Simulation,
        vec![Command::SetComponent {
            entity: e,
            value: ComponentValue::Collider(Collider {
                material: Some(AssetRef::new(
                    "70d7ba8b-f7fb-4eba-920d-ec9e6b968331".parse().unwrap(),
                )),
                ..Collider::default()
            }),
        }],
    );
    let r = w.step();
    assert_eq!(r.rejected, 0);
    assert_eq!(w.diagnostics().len(), 1, "{:?}", w.diagnostics());
    let d = &w.diagnostics()[0];
    assert_eq!(
        (d.code, d.component, d.command_index),
        (
            DiagnosticCode::INVALID_PAYLOAD,
            ComponentId::COLLIDER,
            u32::MAX
        )
    );
    assert!(d.message.contains("physics material"), "{}", d.message);
    w.step();
    assert!(w.diagnostics().is_empty(), "reported once");
    assert_eq!(w.physics().stats().colliders, 1);
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
            target: ObjectRef::to(player),
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
    // No target: the rig stays where it is; the pitch is clamped when
    // written back.
    let parked = pos(&w, camera);
    w.submit(
        Lane::Simulation,
        vec![Command::SetComponent {
            entity: camera,
            value: ComponentValue::ThirdPersonCamera(ThirdPersonCamera {
                target: ObjectRef::NONE,
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
                target: ObjectRef::to(player),
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

/// A script moves its character, then ends the level in the same tick:
/// the move is queued for the physics slot, the destroy lands first, and
/// the move finds nothing — dropped, not a rejection (ADR-0026's code-only
/// game does exactly this).
#[test]
fn a_move_before_a_destroy_in_one_batch_is_dropped() {
    let (mut w, player, _camera) = playground();
    w.step();
    let r = w.submit(
        Lane::Simulation,
        vec![
            Command::Body {
                entity: player,
                op: BodyCommand::Move(Vec3::X),
            },
            Command::Destroy(player),
        ],
    );
    assert_eq!(r.accepted, 2);
    let r = w.step();
    assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
    assert!(w.diagnostics().is_empty());
    assert!(w.physics().character_state(player).is_none());
}

/// `OnTriggerStay` (docs/foundation.md 2.3): a body resting inside a
/// sensor reports a stay every tick after the enter, until the exit — for
/// watched pairs, as collision stays are.
#[test]
fn a_body_inside_a_trigger_stays() {
    let mut w = world();
    let _floor = floor(&mut w);
    let zone = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 0.5, 0.0),
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(2.0, 1.0, 2.0),
            is_sensor: true,
            ..Collider::default()
        })],
    );
    let crate_ = spawn(
        &mut w,
        3,
        Vec3::new(0.0, 0.5, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    let (mut enters, mut stays, mut unwatched) = (0, 0, 0);
    for i in 0..60 {
        if i == 30 {
            w.subscribe(&[zone], true);
        }
        w.step();
        for e in w.after_physics_events() {
            match &e.payload {
                EventPayload::TriggerEnter(_) => enters += 1,
                EventPayload::TriggerStay(c) => {
                    assert!((c.a == zone && c.b == crate_) || (c.a == crate_ && c.b == zone));
                    assert_eq!(c.impulse, Vec3::ZERO);
                    if i < 30 {
                        unwatched += 1;
                    } else {
                        stays += 1;
                    }
                }
                _ => {}
            }
        }
    }
    assert_eq!(enters, 1);
    assert_eq!(unwatched, 0, "nobody watched the pair: no stays");
    assert_eq!(stays, 30, "one a tick once the zone is watched");
}

// Foundation 2.2: in Unity, changing a collider's size or values, the
// object's layer or its scale leaves the Rigidbody as it is — it keeps
// moving, and a pair that still touches fires neither Exit nor Enter.
#[test]
fn changing_a_collider_keeps_the_body_moving_and_fires_nothing() {
    let mut w = world();
    let floor = floor(&mut w);
    let crate_ = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 0.5, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    // A ball drifting without gravity.
    let ball_collider = Collider {
        shape: ColliderShape::Sphere,
        size: Vec3::new(0.5, 0.0, 0.0),
        ..Collider::default()
    };
    let ball = spawn(
        &mut w,
        3,
        Vec3::new(0.0, 5.0, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody {
                gravity_scale: 0.0,
                ..RigidBody::default()
            }),
            ComponentValue::Collider(ball_collider.clone()),
        ],
    );
    w.submit(
        Lane::Simulation,
        vec![Command::Body {
            entity: ball,
            op: BodyCommand::SetLinearVelocity(Vec3::X * 3.0),
        }],
    );
    for _ in 0..60 {
        w.step();
    }
    let scaled = |w: &World, e: Entity| {
        let mut t = w.ecs().copy::<Transform>(e).unwrap();
        t.scale = Vec3::splat(1.2);
        t
    };
    let changes: Vec<(&str, Vec<Command>)> = vec![
        (
            "a collider value",
            vec![
                Command::SetComponent {
                    entity: crate_,
                    value: ComponentValue::Collider(Collider {
                        friction: 0.2,
                        ..Collider::default()
                    }),
                },
                Command::SetComponent {
                    entity: ball,
                    value: ComponentValue::Collider(Collider {
                        size: Vec3::new(0.6, 0.0, 0.0),
                        ..ball_collider
                    }),
                },
            ],
        ),
        (
            "the layer",
            vec![
                Command::SetComponent {
                    entity: crate_,
                    value: ComponentValue::Layer(Layer { layer: 1 }),
                },
                Command::SetComponent {
                    entity: ball,
                    value: ComponentValue::Layer(Layer { layer: 1 }),
                },
            ],
        ),
        // Built when it is sent: the ball moves meanwhile.
        ("the scale", Vec::new()),
    ];
    for (what, cmds) in changes {
        let cmds = if what == "the scale" {
            vec![
                Command::SetTransform {
                    entity: crate_,
                    transform: scaled(&w, crate_),
                },
                Command::SetTransform {
                    entity: ball,
                    transform: scaled(&w, ball),
                },
            ]
        } else {
            cmds
        };
        w.submit(Lane::Simulation, cmds);
        let mut events = Vec::new();
        for _ in 0..10 {
            w.step();
            for e in w.after_physics_events() {
                match &e.payload {
                    EventPayload::CollisionEnter(c) | EventPayload::CollisionExit(c) => {
                        events.push((what, c.a, c.b))
                    }
                    _ => {}
                }
            }
        }
        assert!(
            events.is_empty(),
            "changing {what}: the crate still rests on the floor ({floor:?}), so nothing fires: {events:?}"
        );
        let v = w.physics().body_state(ball).unwrap().linear_velocity;
        assert!(
            (v - Vec3::X * 3.0).length() < 1e-3,
            "changing {what}: the ball keeps its velocity, got {v}"
        );
    }
}

// Foundation 2.6: in Unity no OnTriggerExit fires when the other object is
// destroyed or deactivated. Fynkel's is better and kept: the survivor hears
// the exit. Pinned here, with the zone's named exit — lost before when the
// zone itself went.
#[test]
fn a_destroyed_or_deactivated_side_exits() {
    fn zone_and_crate(w: &mut World, temp: u64) -> (Entity, Entity) {
        let zone = spawn(
            w,
            temp,
            Vec3::new(0.0, 0.5, 0.0),
            vec![
                ComponentValue::Collider(Collider {
                    size: Vec3::new(2.0, 1.0, 2.0),
                    is_sensor: true,
                    ..Collider::default()
                }),
                ComponentValue::TriggerZone(components::TriggerZone {
                    event: "area".into(),
                    once: false,
                }),
            ],
        );
        let crate_ = spawn(
            w,
            temp + 1,
            Vec3::new(0.0, 0.5, 0.0),
            vec![
                ComponentValue::RigidBody(RigidBody::default()),
                ComponentValue::Collider(Collider::default()),
            ],
        );
        for _ in 0..5 {
            w.step();
        }
        w.drain_events();
        (zone, crate_)
    }
    /// The trigger exits and the zone exits over the ticks after `cmd`.
    type Exits = (Vec<(Entity, Entity)>, Vec<(Entity, Entity, String)>);
    fn exits(w: &mut World, cmd: Command) -> Exits {
        w.submit(Lane::Simulation, vec![cmd]);
        let (mut triggers, mut zones) = (Vec::new(), Vec::new());
        for _ in 0..3 {
            w.step();
            for e in w.after_physics_events() {
                if let EventPayload::TriggerExit(c) = &e.payload {
                    triggers.push((c.a, c.b));
                }
            }
            for e in w.drain_events() {
                if let EventPayload::ZoneExited { zone, other, event } = e.payload {
                    zones.push((zone, other, event));
                }
            }
        }
        (triggers, zones)
    }
    let mut w = world();
    floor(&mut w);

    let (zone, crate_) = zone_and_crate(&mut w, 10);
    let (t, z) = exits(&mut w, Command::Destroy(crate_));
    assert_eq!(
        t.len(),
        1,
        "the zone hears the destroyed crate leave: {t:?}"
    );
    assert_eq!(z, vec![(zone, crate_, "area".to_string())]);

    let crate_ = spawn(
        &mut w,
        20,
        Vec3::new(0.0, 0.5, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    for _ in 0..5 {
        w.step();
    }
    w.drain_events();
    let (t, z) = exits(
        &mut w,
        Command::SetActive {
            entity: crate_,
            active: false,
        },
    );
    assert_eq!(t.len(), 1, "a deactivated crate leaves: {t:?}");
    assert_eq!(z, vec![(zone, crate_, "area".to_string())]);

    let (zone, crate_) = zone_and_crate(&mut w, 30);
    let (t, z) = exits(&mut w, Command::Destroy(zone));
    assert_eq!(t.len(), 1, "the crate hears the destroyed zone go: {t:?}");
    assert_eq!(
        z,
        vec![(zone, crate_, "area".to_string())],
        "the zone's named exit fires though the zone is gone"
    );
}

/// Foundation 2.4 (Unity's `OnControllerColliderHit` and
/// `CharacterController.collisionFlags`): a character's move reports what
/// it touched — static and kinematic geometry too — once per collider per
/// tick, `a` the character, `b` the collider, the normal pointing from the
/// character into it; the flags say where (below, sides, above).
#[test]
fn a_character_reports_what_its_move_touches() {
    use fynkel_physics::CharacterState;
    let mut w = world();
    let floor = floor(&mut w);
    let wall = spawn(
        &mut w,
        2,
        Vec3::new(2.0, 1.0, 0.0),
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(0.25, 1.0, 3.0),
            ..Collider::default()
        })],
    );
    let block = spawn(
        &mut w,
        3,
        Vec3::new(-2.0, 1.0, 0.0),
        vec![
            ComponentValue::RigidBody(RigidBody {
                body_type: BodyType::Kinematic,
                ..RigidBody::default()
            }),
            ComponentValue::Collider(Collider {
                size: Vec3::new(0.25, 1.0, 3.0),
                ..Collider::default()
            }),
        ],
    );
    let ceiling = spawn(
        &mut w,
        4,
        Vec3::new(0.0, 2.6, 0.0),
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(1.0, 0.2, 1.0),
            ..Collider::default()
        })],
    );
    let player = spawn(
        &mut w,
        5,
        Vec3::ZERO,
        vec![ComponentValue::CharacterController(
            CharacterController::default(),
        )],
    );
    w.subscribe(&[player], true);
    for _ in 0..10 {
        w.step();
    }
    w.drain_events();
    let flags = |w: &World| w.physics().character_state(player).unwrap().collision_flags;
    /// Moves the character at `v` for `ticks`; the colliders it hit (and
    /// how often, `a` always the character) and the union of its flags.
    fn walk(
        w: &mut World,
        player: Entity,
        v: Vec3,
        ticks: usize,
    ) -> (Vec<(Entity, Option<Entity>, Vec3)>, u32) {
        let (mut hits, mut flags) = (Vec::new(), 0);
        for _ in 0..ticks {
            w.submit(
                Lane::Simulation,
                vec![Command::Body {
                    entity: player,
                    op: BodyCommand::Move(v),
                }],
            );
            w.step();
            for e in w.after_physics_events() {
                if let EventPayload::ControllerHit(c) = &e.payload {
                    assert_eq!(c.a, player);
                    hits.push((c.b, c.body_b, c.normal));
                }
            }
            flags |= w.physics().character_state(player).unwrap().collision_flags;
        }
        w.drain_events();
        (hits, flags)
    }

    // Standing: the floor below, every tick.
    let (hits, f) = walk(&mut w, player, Vec3::ZERO, 5);
    assert!(hits.iter().all(|h| h.0 == floor), "{hits:?}");
    assert_eq!(hits.len(), 5, "{hits:?}");
    assert_eq!(f, CharacterState::BELOW);

    // Into the static wall: its hits point +x, the flags gain the sides.
    let (hits, f) = walk(&mut w, player, Vec3::X * 4.0, 40);
    let at_wall: Vec<_> = hits.iter().filter(|h| h.0 == wall).collect();
    assert!(at_wall.len() > 5, "{hits:?}");
    assert!(
        at_wall.iter().all(|h| h.1.is_none() && h.2.x > 0.9),
        "{at_wall:?}"
    );
    assert_eq!(flags(&w), CharacterState::BELOW | CharacterState::SIDES);
    assert_eq!(f, CharacterState::BELOW | CharacterState::SIDES);

    // Into the kinematic block: its body is reported.
    let (hits, _) = walk(&mut w, player, -Vec3::X * 4.0, 80);
    let at_block: Vec<_> = hits.iter().filter(|h| h.0 == block).collect();
    assert!(!at_block.is_empty(), "{hits:?}");
    assert!(
        at_block.iter().all(|h| h.1 == Some(block) && h.2.x < -0.9),
        "{at_block:?}"
    );

    // Back under the ceiling and jump: above.
    walk(&mut w, player, Vec3::X * 4.0, 25);
    let x = pos(&w, player).x;
    assert!(x.abs() < 0.6, "under the ceiling: {x}");
    w.submit(
        Lane::Simulation,
        vec![Command::Body {
            entity: player,
            op: BodyCommand::Jump(0.0),
        }],
    );
    let (hits, f) = walk(&mut w, player, Vec3::ZERO, 30);
    assert!(
        hits.iter().any(|h| h.0 == ceiling && h.2.y > 0.9),
        "{hits:?}"
    );
    assert!(f & CharacterState::ABOVE != 0, "{f}");
    assert_eq!(flags(&w), CharacterState::BELOW, "landed again");
}

/// Foundation 2.5 (Unity's `Rigidbody.constraints`): a dynamic body's
/// frozen axes do not move — a frozen Y does not fall, a frozen X ignores
/// a push along X, frozen rotations ignore a spin — while its free axes
/// behave as an unconstrained twin's. Unity's bit values.
#[test]
fn a_bodys_frozen_axes_stay_put() {
    use fynkel_physics::components::constraints as c;
    let mut w = world();
    let body = |constraints| {
        vec![
            ComponentValue::RigidBody(RigidBody {
                gravity_scale: 1.0,
                constraints,
                ..RigidBody::default()
            }),
            ComponentValue::Collider(Collider::default()),
        ]
    };
    let free = spawn(&mut w, 1, Vec3::new(0.0, 5.0, 0.0), body(0));
    let frozen = spawn(
        &mut w,
        2,
        Vec3::new(10.0, 5.0, 0.0),
        body(c::FREEZE_POSITION_X | c::FREEZE_POSITION_Y | c::FREEZE_ROTATION),
    );
    w.step();
    let push = |e| {
        vec![
            Command::Body {
                entity: e,
                op: BodyCommand::SetLinearVelocity(Vec3::new(2.0, 0.0, 3.0)),
            },
            Command::Body {
                entity: e,
                op: BodyCommand::ApplyTorqueImpulse(Vec3::new(0.3, 0.3, 0.3)),
            },
        ]
    };
    let mut cmds = push(free);
    cmds.extend(push(frozen));
    w.submit(Lane::Simulation, cmds);
    for _ in 0..30 {
        w.step();
    }
    let (f, z) = (pos(&w, free), pos(&w, frozen));
    assert!(
        f.y < 4.5 && f.x > 0.5,
        "the free twin falls and slides: {f}"
    );
    assert!((z.y - 5.0).abs() < 1e-4, "frozen Y: {z}");
    assert!((z.x - 10.0).abs() < 1e-4, "frozen X: {z}");
    assert!(
        (z.z - f.z).abs() < 1e-3 && z.z > 0.5,
        "Z is free: {z} vs {f}"
    );
    let turned = |e| {
        w.ecs()
            .copy::<Transform>(e)
            .unwrap()
            .rotation
            .angle_between(Quat::IDENTITY)
    };
    assert!(turned(free) > 0.1, "the free twin spins");
    assert!(turned(frozen) < 1e-4, "frozen rotation: {}", turned(frozen));
    let s = w.physics().body_state(frozen).unwrap();
    assert_eq!(s.angular_velocity, Vec3::ZERO);
    assert_eq!((s.linear_velocity.x, s.linear_velocity.y), (0.0, 0.0));
}

/// Unity's `Collision` (foundation 2.9): every contact point of the pair
/// (the deepest first), the impulse as a vector and the relative velocity
/// at impact — before the solver stops the fall, so a landing's speed is
/// readable (`relativeVelocity.magnitude > 2` plays the thud).
#[test]
fn a_collision_carries_its_points_impulse_and_relative_velocity() {
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
    let mut enter = None;
    for _ in 0..240 {
        w.step();
        for e in w.after_physics_events() {
            if let EventPayload::CollisionEnter(c) = &e.payload {
                enter = Some(*c);
            }
        }
        if enter.is_some() {
            break;
        }
    }
    let c = enter.expect("the crate lands");
    // Seen from the crate: the side it is on.
    let (up, sign) = if c.a == crate_ {
        assert_eq!(c.b, floor);
        (-c.normal, 1.0)
    } else {
        (c.normal, -1.0)
    };
    assert!(up.y > 0.9, "{:?}", c.normal);
    // A flat face on a flat floor: four corners.
    assert_eq!(c.point_count, 4, "{c:?}");
    let points = &c.points[..c.point_count as usize];
    for p in points {
        assert!(p.point.y.abs() < 0.1, "{p:?}");
        assert!((p.point.x.abs() - 0.5).abs() < 0.05, "{p:?}");
        assert!((p.point.z.abs() - 0.5).abs() < 0.05, "{p:?}");
        assert!((p.normal - c.normal).length() < 1e-3, "{p:?}");
        assert!(p.separation <= 0.05, "{p:?}");
        assert!(p.impulse >= 0.0, "{p:?}");
    }
    assert_eq!(points[0].point, c.point, "the deepest first");
    for p in &c.points[c.point_count as usize..] {
        assert_eq!(*p, ContactPoint::default());
    }
    // The impulse pushes the crate up (on `a`), the points' sum.
    let impulse = c.impulse * sign;
    assert!(impulse.y > 0.1, "{:?}", c.impulse);
    let sum: f32 = points.iter().map(|p| p.impulse).sum();
    assert!(
        (c.impulse.length() - sum).abs() < 1e-3 * sum.max(1.0),
        "{c:?}"
    );
    // Relative velocity (the other's minus this one's, as Unity): the
    // floor comes up at the falling crate at about √(2·g·2.5) ≈ 7 m/s.
    let relative = c.relative_velocity * sign;
    assert!(relative.y > 5.0, "{:?}", c.relative_velocity);
    assert!(relative.xz().length() < 0.1, "{:?}", c.relative_velocity);

    // A trigger and an exit carry no points and no impulse.
    w.submit(
        Lane::Simulation,
        vec![Command::SetTransform {
            entity: crate_,
            transform: Transform::from_position(Vec3::new(0.0, 5.0, 0.0)),
        }],
    );
    let mut exit = None;
    for _ in 0..3 {
        w.step();
        for e in w.after_physics_events() {
            if let EventPayload::CollisionExit(c) = &e.payload {
                exit = Some(*c);
            }
        }
    }
    let x = exit.expect("teleporting off the floor exits");
    assert_eq!(x.point_count, 0);
    assert_eq!(x.impulse, Vec3::ZERO);
}
