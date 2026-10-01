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

fn spawn(w: &mut World, _fixture_id: u64, at: Vec3, components: Vec<ComponentValue>) -> Entity {
    let commit = w
        .edit_gameplay(|edit| edit.spawn("", Transform::from_position(at), None))
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    w.drain_events();
    if !components.is_empty() {
        w.edit_gameplay(|edit| {
            for value in components {
                edit.set_component(entity, value)?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(w.step().status, Status::Ok);
    }
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
fn a_world_runs_at_most_the_modules_its_build_carries() {
    let mut w = World::new(WorldConfig::default());
    assert_eq!(w.modules(), Modules::COMPILED);
    w.set_modules(Modules::ALL);
    assert_eq!(w.modules(), Modules::COMPILED);
    #[cfg(all(feature = "physics", feature = "audio", feature = "animation"))]
    for m in Module::ALL {
        assert!(w.modules().has(m));
    }
    // A stripped module stays off whatever the project says.
    #[cfg(not(feature = "animation"))]
    assert!(!w.modules().animation);
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

    let error = w
        .edit_gameplay(|edit| edit.body_effect(e, BodyCommand::ApplyImpulse(Vec3::Y)))
        .unwrap_err();
    assert!(error.to_string().contains("physics"));
    assert_eq!(w.step().rejected, 0);
}

#[cfg(feature = "physics")]
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
fn audio_off_rejects_audio_edits() {
    let mut w = world(Modules {
        audio: false,
        ..Modules::ALL
    });
    let r = w.edit_gameplay(|edit| {
        edit.set_audio_bus(AudioBus::Music, 0.5, false)?;
        Ok(())
    });
    let error = r.unwrap_err();
    assert!(error.to_string().contains("audio"));
    assert_eq!(w.step().rejected, 0);
}

#[test]
fn animation_off_rejects_animate_and_leaves_physics_running() {
    let mut w = world(Modules {
        animation: false,
        ..Modules::ALL
    });
    let e = spawn(&mut w, 1, Vec3::new(0.0, 3.0, 0.0), body());
    let error = w
        .edit_gameplay(|edit| edit.animate_control(e, AnimateOp::Stop))
        .unwrap_err();
    assert!(error.to_string().contains("animation"));
    assert_eq!(w.step().rejected, 0);
    #[cfg(feature = "physics")]
    {
        for _ in 0..30 {
            w.step();
        }
        assert!(y(&w, e) < 3.0, "physics still steps");
    }
}
