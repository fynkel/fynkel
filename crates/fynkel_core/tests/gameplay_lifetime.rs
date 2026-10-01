//! Native lifetime passes share the gameplay commit and event-clock contract.
use fynkel_core::*;

fn spawn(w: &mut World, seconds: f32, parent: Option<Entity>) -> Entity {
    let commit = w
        .edit_gameplay(|edit| {
            let e = edit.spawn("lifetime", Transform::default(), parent.map(Into::into))?;
            edit.set_component(
                e,
                ComponentValue::LifetimeDespawn(LifetimeDespawn { seconds }),
            )?;
            Ok(e)
        })
        .unwrap();
    commit.resolve(commit.value).unwrap()
}

#[test]
fn simultaneous_expiry_destroys_each_entity_once_at_the_computed_tick() {
    for hz in [30.0, 60.0, 120.0, 240.0] {
        let dt = 1.0 / hz;
        let mut w = World::new(WorldConfig {
            fixed_dt: dt,
            ..WorldConfig::default()
        });
        let parent = spawn(&mut w, dt * 1.5, None);
        let child = spawn(&mut w, dt * 1.5, Some(parent));
        let survivor = spawn(&mut w, dt * 4.0, None);
        w.drain_events();
        w.step();
        assert!(w.ecs().contains(parent));
        assert!(w.drain_events().is_empty());
        let report = w.step();
        let events = w.drain_events();
        let destroyed: Vec<_> = events
            .iter()
            .filter_map(|event| match event.payload {
                EventPayload::Destroyed { entity } => {
                    assert_eq!(event.tick, report.tick);
                    Some(entity)
                }
                _ => None,
            })
            .collect();
        assert_eq!(destroyed, vec![parent, child]);
        assert!(w.ecs().contains(survivor));
        assert!(
            (w.ecs().copy::<LifetimeDespawn>(survivor).unwrap().seconds - dt * 2.0).abs() < 1e-6
        );
    }
}

#[test]
fn failed_lifetime_pass_keeps_all_countdowns_and_hierarchy_until_repaired() {
    let mut w = World::new(WorldConfig::default());
    let dt = WorldConfig::default().fixed_dt;
    let expired = spawn(&mut w, 0.0, None);
    let survivor = spawn(&mut w, 1.0, Some(expired));
    let broken = spawn(&mut w, 2.0, None);
    // Simulate invalid data introduced by an unmigrated native mutation path.
    w.ecs_mut()
        .insert(
            broken,
            LifetimeDespawn {
                seconds: f32::INFINITY,
            },
        )
        .unwrap();
    w.drain_events();
    w.step();
    assert_eq!(w.live_entities(), 3);
    assert_eq!(
        w.ecs().copy::<LifetimeDespawn>(survivor).unwrap().seconds,
        1.0
    );
    assert_eq!(fynkel_scene::parent_of(w.ecs(), survivor), Some(expired));
    assert!(w.drain_events().is_empty());
    assert!(w
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("lifetime pass")));
    w.edit_gameplay(|edit| {
        edit.set_component(
            broken,
            ComponentValue::LifetimeDespawn(LifetimeDespawn { seconds: 2.0 }),
        )
    })
    .unwrap();
    w.step();
    assert_eq!(w.live_entities(), 1);
    assert!((w.ecs().copy::<LifetimeDespawn>(broken).unwrap().seconds - (2.0 - dt)).abs() < 1e-6);
}

#[test]
fn inactive_lifetimes_do_not_count_down_but_expiring_parent_removes_inactive_children() {
    let mut w = World::new(WorldConfig::default());
    let parent = spawn(&mut w, 1.0, None);
    let child = spawn(&mut w, 0.0, Some(parent));
    w.edit_gameplay(|edit| edit.set_active(parent, false))
        .unwrap();
    w.step();
    assert_eq!(
        w.ecs().copy::<LifetimeDespawn>(parent).unwrap().seconds,
        1.0
    );
    assert!(w.ecs().contains(child));
    w.edit_gameplay(|edit| {
        edit.set_active(parent, true)?;
        edit.set_active(child, false)?;
        edit.set_component(
            parent,
            ComponentValue::LifetimeDespawn(LifetimeDespawn { seconds: 0.0 }),
        )
    })
    .unwrap();
    w.step();
    assert_eq!(w.live_entities(), 0);
}
