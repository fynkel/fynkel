//! The project's modules (ADR-0029 Decision 3): a module turned off skips
//! its system at `step()` and rejects its commands with `MODULE_OFF`
//! (FYN-R0014); the registry and the other systems are untouched.

use fynkel_core::components::ComponentValue;
use fynkel_core::*;
use glam::Vec3;

fn world(modules: Modules) -> World {
    let mut w = World::new(WorldConfig::default());
    w.set_modules(modules);
    w
}

fn spawn(w: &mut World, temp_id: u64, at: Vec3, components: Vec<ComponentValue>) -> Entity {
    w.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id,
            transform: Transform::from_position(at),
            parent: None,
            name: None,
        }],
    );
    w.step();
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
    w.submit(Lane::Simulation, cmds);
    w.step();
    entity
}

fn y(w: &World, e: Entity) -> f32 {
    w.ecs().copy::<Transform>(e).unwrap().position.y
}

fn body() -> Vec<ComponentValue> {
    vec![
        ComponentValue::RigidBody(RigidBody::default()),
        ComponentValue::Collider(Collider::default()),
    ]
}

#[test]
fn every_module_is_on_by_default() {
    let w = World::new(WorldConfig::default());
    assert_eq!(w.modules(), Modules::ALL);
    for m in Module::ALL {
        assert!(w.modules().has(m));
    }
}

#[test]
fn physics_off_leaves_a_body_in_place_and_rejects_body_commands() {
    let mut w = world(Modules {
        physics: false,
        ..Modules::ALL
    });
    let e = spawn(&mut w, 1, Vec3::new(0.0, 3.0, 0.0), body());
    // The components are there (the contract is whole), the body is not.
    assert!(w.ecs().has::<RigidBody>(e));
    assert!(!w.ecs().has::<PhysicsOwned>(e));
    for _ in 0..30 {
        w.step();
    }
    assert_eq!(y(&w, e), 3.0);

    w.submit(
        Lane::Simulation,
        vec![Command::Body {
            entity: e,
            op: BodyCommand::ApplyImpulse(Vec3::Y),
        }],
    );
    w.step();
    let d = w.diagnostics().last().unwrap();
    assert_eq!(d.code, DiagnosticCode::MODULE_OFF);
    assert!(d.message.contains("FYN-R0014"), "{}", d.message);
    assert!(d.message.contains("physics"), "{}", d.message);
}

#[test]
fn the_same_body_falls_with_physics_on() {
    let mut w = world(Modules::ALL);
    let e = spawn(&mut w, 1, Vec3::new(0.0, 3.0, 0.0), body());
    for _ in 0..30 {
        w.step();
    }
    assert!(y(&w, e) < 3.0);
}

#[test]
fn audio_off_rejects_audio_commands_on_either_lane() {
    let mut w = world(Modules {
        audio: false,
        ..Modules::ALL
    });
    let r = w.submit(
        Lane::Presentation,
        vec![Command::AudioBus {
            bus: AudioBus::Music,
            volume: 0.5,
            muted: false,
        }],
    );
    assert_eq!(r.rejected, 1);
    let d = w.diagnostics().last().unwrap();
    assert_eq!(d.code, DiagnosticCode::MODULE_OFF);
    assert!(d.message.contains("audio"), "{}", d.message);
}

#[test]
fn animation_off_rejects_animate_and_leaves_physics_running() {
    let mut w = world(Modules {
        animation: false,
        ..Modules::ALL
    });
    let e = spawn(&mut w, 1, Vec3::new(0.0, 3.0, 0.0), body());
    w.submit(
        Lane::Simulation,
        vec![Command::Animate {
            entity: e,
            op: AnimateOp::Stop,
        }],
    );
    w.step();
    let d = w.diagnostics().last().unwrap();
    assert_eq!(d.code, DiagnosticCode::MODULE_OFF);
    assert!(d.message.contains("animation"), "{}", d.message);
    for _ in 0..30 {
        w.step();
    }
    assert!(y(&w, e) < 3.0, "physics still steps");
}
