//! Native emitter bursts use shared prefab validation and phase-aware effects.
use fynkel_contracts::{AssetRef, Uuid};
use fynkel_core::*;
use glam::Vec3;

fn uid(n: u8) -> Uuid {
    Uuid([n; 16])
}
fn prefab(bad: bool, dynamic: bool) -> SceneDescription {
    let mut child = vec![];
    if bad {
        child.push(ComponentValue::PointLight(PointLight {
            intensity: f32::NAN,
            ..PointLight::default()
        }));
    }
    if dynamic {
        child.push(ComponentValue::RigidBody(RigidBody::default()));
    }
    SceneDescription {
        id: uid(1),
        name: "Burst".into(),
        entities: vec![
            EntityDescription {
                id: uid(2),
                name: "Root".into(),
                active: true,
                parent: None,
                children: vec![uid(3)],
                components: vec![],
            },
            EntityDescription {
                id: uid(3),
                name: "Child".into(),
                active: true,
                parent: Some(uid(2)),
                children: vec![],
                components: child,
            },
        ],
    }
}
fn emitter(w: &mut World, dt: f32, velocity: Vec3) -> Entity {
    let commit = w
        .edit_gameplay(|edit| {
            let e = edit.spawn("Emitter", Transform::IDENTITY, None)?;
            edit.set_component(
                e,
                ComponentValue::Emitter(Emitter {
                    prefab: AssetRef::new(uid(1)),
                    rate: 2.0 / dt,
                    lifetime: 1.0,
                    max_alive: 2,
                    velocity,
                    enabled: true,
                }),
            )?;
            Ok(e)
        })
        .unwrap();
    commit.resolve(commit.value).unwrap()
}
#[cfg(feature = "physics")]
fn stop(w: &mut World, e: Entity) {
    let mut config = w.ecs().copy::<Emitter>(e).unwrap();
    config.rate = 0.0;
    w.edit_gameplay(|edit| edit.set_component(e, ComponentValue::Emitter(config)))
        .unwrap();
}

#[test]
fn malformed_prefab_leaves_no_partial_entities_and_corrected_burst_can_retry() {
    let config = WorldConfig::default();
    let mut w = World::new(config.clone());
    w.register_prefab(uid(1), prefab(true, false));
    let e = emitter(&mut w, config.fixed_dt, Vec3::ZERO);
    w.step();
    assert_eq!(w.live_entities(), 1);
    assert!(w
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("emitter burst")));
    w.register_prefab(uid(1), prefab(false, false));
    w.step();
    assert_eq!(
        w.live_entities(),
        5,
        "two complete instances, no failed-attempt backlog"
    );
    w.step();
    assert_eq!(
        w.live_entities(),
        5,
        "only committed instances count toward ceiling"
    );
    assert!(w.ecs().contains(e));
    let roots: Vec<_> = w
        .ecs()
        .entities()
        .filter(|&e| w.ecs().has::<LifetimeDespawn>(e))
        .collect();
    assert_eq!(roots.len(), 2);
    for root in roots {
        assert_eq!(fynkel_scene::children_of(w.ecs(), root).len(), 1);
    }
}

#[test]
fn disabled_physics_keeps_emitted_configuration_without_runtime_launches() {
    let config = WorldConfig::default();
    let mut w = World::new(config.clone());
    let mut modules = w.modules();
    modules.physics = false;
    w.set_modules(modules);
    w.register_prefab(uid(1), prefab(false, true));
    emitter(&mut w, config.fixed_dt, Vec3::X);
    w.step();
    assert_eq!(w.live_entities(), 5);
    assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    assert_eq!(
        w.ecs()
            .entities()
            .filter(|&e| w.ecs().has::<RigidBody>(e))
            .count(),
        2
    );
}

#[cfg(feature = "physics")]
#[test]
fn launches_reach_this_ticks_physics_once_at_four_rates() {
    for hz in [30.0, 60.0, 120.0, 240.0] {
        let dt = 1.0 / hz;
        let mut w = World::new(WorldConfig {
            fixed_dt: dt,
            ..WorldConfig::default()
        });
        w.register_prefab(uid(1), prefab(false, true));
        let e = emitter(&mut w, dt, Vec3::new(3.0, 0.0, 0.0));
        w.step();
        let bodies: Vec<_> = w
            .ecs()
            .entities()
            .filter(|&e| w.ecs().has::<RigidBody>(e))
            .collect();
        assert_eq!(bodies.len(), 2);
        for &body in &bodies {
            assert!((w.physics().body_state(body).unwrap().linear_velocity.x - 3.0).abs() < 1e-6);
            assert!(w.ecs().copy::<Transform>(body).unwrap().position.x > 0.0);
        }
        stop(&mut w, e);
        w.edit_gameplay(|edit| {
            for &body in &bodies {
                edit.body_effect(body, BodyCommand::SetLinearVelocity(Vec3::ZERO))?;
            }
            Ok(())
        })
        .unwrap();
        w.step();
        for &body in &bodies {
            assert!(
                w.physics()
                    .body_state(body)
                    .unwrap()
                    .linear_velocity
                    .x
                    .abs()
                    < 1e-6
            );
        }
    }
}

#[cfg(feature = "physics")]
#[test]
fn invalid_launch_rolls_back_instances_lifetimes_and_effects() {
    let config = WorldConfig::default();
    let mut w = World::new(config.clone());
    w.register_prefab(uid(1), prefab(false, true));
    let e = emitter(&mut w, config.fixed_dt, Vec3::ZERO);
    let mut invalid = w.ecs().copy::<Emitter>(e).unwrap();
    invalid.velocity.x = f32::NAN;
    w.ecs_mut().insert(e, invalid).unwrap();
    w.step();
    assert_eq!(w.live_entities(), 1);
    assert!(w
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("emitter burst")));
    let mut repaired = w.ecs().copy::<Emitter>(e).unwrap();
    repaired.velocity = Vec3::ZERO;
    w.edit_gameplay(|edit| edit.set_component(e, ComponentValue::Emitter(repaired)))
        .unwrap();
    w.step();
    assert_eq!(w.live_entities(), 5);
    assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    stop(&mut w, e);
    w.step();
    assert!(w.diagnostics().is_empty());
}

#[test]
fn separate_emitters_have_independent_burst_boundaries() {
    let config = WorldConfig::default();
    let mut w = World::new(config.clone());
    w.register_prefab(uid(1), prefab(true, false));
    w.register_prefab(uid(4), prefab(false, false));
    emitter(&mut w, config.fixed_dt, Vec3::ZERO);
    let healthy = emitter(&mut w, config.fixed_dt, Vec3::ZERO);
    let mut settings = w.ecs().copy::<Emitter>(healthy).unwrap();
    settings.prefab = AssetRef::new(uid(4));
    w.edit_gameplay(|edit| edit.set_component(healthy, ComponentValue::Emitter(settings)))
        .unwrap();
    w.step();
    assert_eq!(
        w.live_entities(),
        6,
        "two emitters plus the healthy burst's four entities"
    );
    assert_eq!(
        w.diagnostics()
            .iter()
            .filter(|d| d.message.contains("emitter burst"))
            .count(),
        1
    );
}
