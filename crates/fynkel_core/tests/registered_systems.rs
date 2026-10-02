#[path = "../examples/registered_components/health.rs"]
mod health;
use fynkel_core::*;
use health::Health;
fn noop(_: &mut GameplayEdit<'_>, _: SystemContext) -> EditResult<u32> {
    Ok(0)
}
fn registration(id: &str) -> RegisteredSystem {
    RegisteredSystem {
        spec: SystemSpec::new(id, SystemPhase::Simulation),
        run: noop,
    }
}
fn world() -> World {
    let mut w = World::new(WorldConfig::default());
    w.register_component::<Health>(health::schema()).unwrap();
    w
}
#[test]
fn graph_is_atomic_and_ties_do_not_depend_on_registration_order() {
    let mut a = world();
    let mut b = world();
    a.register_systems(vec![registration("game.Z"), registration("game.A")])
        .unwrap();
    b.register_systems(vec![registration("game.A"), registration("game.Z")])
        .unwrap();
    let order = |w: &World| {
        w.registered_systems()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&a), order(&b));
    let before = order(&a);
    let mut x = registration("game.X");
    let mut y = registration("game.Y");
    x.spec.after.push("game.Y".into());
    y.spec.after.push("game.X".into());
    assert!(matches!(
        a.register_systems(vec![x, y]),
        Err(ScheduleError::Cycle(_))
    ));
    assert_eq!(before, order(&a));
    let mut bad = registration("game.Bad");
    bad.spec.after.push("game.Missing".into());
    assert!(matches!(
        a.register_systems(vec![bad]),
        Err(ScheduleError::UnknownDependency(..))
    ));
    let mut bad = registration("game.Bad");
    bad.spec.after.push("fynkel.propagate".into());
    assert!(matches!(
        a.register_systems(vec![bad]),
        Err(ScheduleError::ReversedPhase(..))
    ));
    assert!(matches!(
        a.register_systems(vec![registration("game.A")]),
        Err(ScheduleError::Duplicate(_))
    ));
    assert!(matches!(
        a.register_systems(vec![registration("fynkel.extension")]),
        Err(ScheduleError::ReservedIdentity(_))
    ));
}
#[test]
fn custom_system_commits_and_failed_system_rolls_back_before_the_next_system() {
    let mut w = world();
    let c = w
        .edit_gameplay(|e| {
            let p = e.spawn("player", Transform::IDENTITY, None)?;
            e.add_registered_defaults(p, "example.Health")?;
            Ok(p)
        })
        .unwrap();
    let player = c.resolve(c.value).unwrap();
    let mut damage = registration("game.Damage");
    damage.spec.writes.push("example.Health".into());
    damage.run = |e, t| {
        assert_eq!(t.tick, 1);
        let p = e.find("player")?.unwrap();
        let mut h = e.registered::<Health>(p)?.unwrap();
        h.hp -= 10.;
        e.set_registered(p, h)?;
        Ok(1)
    };
    let mut fail = registration("game.Failure");
    fail.spec.after.push("game.Damage".into());
    fail.run = |e, _| {
        let p = e.find("player")?.unwrap();
        e.set_name(p, "discard")?;
        Err(EditError::Aborted)
    };
    let mut check = registration("game.Check");
    check.spec.after.push("game.Failure".into());
    check.run = |e, _| {
        let p = e.find("player")?.unwrap();
        assert_eq!(e.registered::<Health>(p)?.unwrap().hp, 90.);
        Ok(1)
    };
    w.register_systems(vec![check, fail, damage]).unwrap();
    let report = w.step();
    assert_eq!(report.rejected, 1);
    assert_eq!(w.ecs().get::<Health>(player).unwrap().hp, 90.);
    assert!(w
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("game.Failure")));
    assert!(w
        .registered_system_times()
        .iter()
        .any(|t| t.id == "game.Check" && t.entities == 1));
}
#[test]
fn panic_discards_journal_and_keeps_schedule_for_next_tick() {
    let mut w = world();
    let n = w.registered_systems().count();
    let mut s = registration("game.Panic");
    s.run = |e, _| {
        e.spawn("discard", Transform::IDENTITY, None)?;
        panic!("oops")
    };
    w.register_systems(vec![s]).unwrap();
    for _ in 0..2 {
        assert_eq!(w.step().rejected, 1);
        assert_eq!(w.live_entities(), 0);
        assert_eq!(w.registered_systems().count(), n + 1);
    }
}
#[test]
fn invalid_access_and_disabled_modules_are_rejected_and_editor_does_not_run_game_code() {
    let mut w = world();
    let mut s = registration("game.Unknown");
    s.spec.reads.push("game.Unregistered".into());
    assert!(matches!(
        w.register_systems(vec![s]),
        Err(ScheduleError::UnknownComponent(..))
    ));
    w.set_modules(Modules {
        physics: false,
        audio: false,
        animation: false,
    });
    let mut s = registration("game.Physics");
    s.spec.requires.push(Module::Physics);
    assert!(matches!(
        w.register_systems(vec![s]),
        Err(ScheduleError::MissingModule(..))
    ));
    let mut w = World::new(WorldConfig {
        edit_mode: true,
        ..Default::default()
    });
    let mut s = registration("game.NoEditor");
    s.run = |_, _| panic!("editor executed game code");
    w.register_systems(vec![s]).unwrap();
    assert_eq!(w.step().rejected, 0);
}

#[test]
fn explicit_dependencies_inherit_module_requirements() {
    let mut w = world();
    w.set_modules(Modules {
        physics: false,
        animation: false,
        audio: false,
    });
    let mut s = registration("game.NeedsAnimation");
    s.spec.after.push("fynkel.animation".into());
    assert!(matches!(
        w.register_systems(vec![s]),
        Err(ScheduleError::MissingModule(..))
    ));
}
