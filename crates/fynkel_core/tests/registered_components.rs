#[path = "../examples/registered_components/health.rs"]
mod health;
use fynkel_core::*;
use health::Health;
fn world() -> World {
    let mut w = World::new(WorldConfig::default());
    w.register_component::<Health>(health::schema()).unwrap();
    w
}
fn player(w: &mut World) -> Entity {
    let c = w
        .edit_gameplay(|e| {
            let p = e.spawn("player", Transform::IDENTITY, None)?;
            e.add_registered_defaults(p, "example.Health")?;
            Ok(p)
        })
        .unwrap();
    c.resolve(c.value).unwrap()
}
#[test]
fn registered_values_share_pending_and_committed_ecs_ownership() {
    let mut w = world();
    let p = player(&mut w);
    assert_eq!(w.ecs().get::<Health>(p).unwrap().hp, 100.0);
    w.edit_gameplay(|e| {
        e.set_registered(
            p,
            Health {
                hp: 75.0,
                invulnerable: true,
            },
        )?;
        assert_eq!(e.registered::<Health>(p)?.unwrap().hp, 75.0);
        assert_eq!(
            e.registered_values(p, "example.Health")?,
            Some(vec![ScalarValue::Float(75.0), ScalarValue::Bool(true)])
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(w.ecs().get::<Health>(p).unwrap().hp, 75.0);
    assert_eq!(w.tick(), 0);
}
#[test]
fn caught_invalid_custom_write_rolls_back_builtin_data_and_spawns() {
    let mut w = world();
    let p = player(&mut w);
    assert!(w
        .edit_gameplay(|e| {
            e.set_name(p, "discard")?;
            e.spawn("discard", Transform::IDENTITY, None)?;
            assert!(e
                .set_registered(
                    p,
                    Health {
                        hp: -1.0,
                        invulnerable: false
                    }
                )
                .is_err());
            Ok(())
        })
        .is_err());
    assert_eq!(w.live_entities(), 1);
    assert_eq!(w.ecs().get::<Name>(p).unwrap().value, "player");
    assert_eq!(w.ecs().get::<Health>(p).unwrap().hp, 100.0);
}
#[test]
fn invalid_builtin_edit_rolls_back_registered_replacement_and_removal() {
    let mut w = world();
    let p = player(&mut w);
    for remove in [false, true] {
        assert!(w
            .edit_gameplay(|e| {
                if remove {
                    e.remove_registered(p, "example.Health")?;
                } else {
                    e.set_registered(
                        p,
                        Health {
                            hp: 20.0,
                            invulnerable: false,
                        },
                    )?;
                }
                e.set_parent(p, Some(p.into()), false)
            })
            .is_err());
        assert_eq!(w.ecs().get::<Health>(p).unwrap().hp, 100.0);
    }
}
#[test]
fn cloning_uses_logical_registered_values_and_removal_is_atomic() {
    let mut w = world();
    let p = player(&mut w);
    let c = w
        .edit_gameplay(|e| {
            e.set_registered(
                p,
                Health {
                    hp: 12.0,
                    invulnerable: false,
                },
            )?;
            let c = e.clone_tree(p.into(), None, None, None)?;
            assert_eq!(e.registered::<Health>(c.root)?.unwrap().hp, 12.0);
            e.remove_registered(p, "example.Health")?;
            assert!(e.registered::<Health>(p)?.is_none());
            Ok(c.root)
        })
        .unwrap();
    let copy = c.resolve(c.value).unwrap();
    assert!(!w.ecs().has::<Health>(p));
    assert_eq!(w.ecs().get::<Health>(copy).unwrap().hp, 12.0);
    w.edit_gameplay(|e| e.destroy(copy)).unwrap();
    assert!(!w.ecs().contains(copy));
}
#[test]
fn schemas_reject_conflicts_without_replacing_existing_registration_or_data() {
    let mut w = world();
    let p = player(&mut w);
    assert!(w.register_component::<Health>(health::schema()).is_ok());
    for change in 0..5 {
        let mut s = health::schema();
        match change {
            0 => s.version = 2,
            1 => s.id = "other.Health".into(),
            2 => s.fields[0].default = ScalarValue::Float(10.0),
            3 => s.fields[1].name = "hp".into(),
            _ => s.id = "fynkel.Health".into(),
        }
        assert!(w.register_component::<Health>(s).is_err());
    }
    assert_eq!(w.registered_schemas().count(), 1);
    assert_eq!(w.registered_schemas().next().unwrap(), &health::schema());
    assert_eq!(w.ecs().get::<Health>(p).unwrap().hp, 100.0);
}
#[test]
fn schema_defaults_and_dynamic_field_layout_are_validated_before_staging() {
    let mut w = World::new(WorldConfig::default());
    let mut invalid = health::schema();
    invalid.fields[0].default = ScalarValue::Float(-1.0);
    assert!(w.register_component::<Health>(invalid).is_err());
    assert_eq!(w.registered_schemas().count(), 0);
    w.register_component::<Health>(health::schema()).unwrap();
    let p = player(&mut w);
    for fields in [
        vec![],
        vec![ScalarValue::Int(1), ScalarValue::Bool(false)],
        vec![ScalarValue::Float(f64::NAN), ScalarValue::Bool(false)],
    ] {
        assert!(w
            .edit_gameplay(|e| e.set_registered_values(p, "example.Health", fields))
            .is_err());
        assert_eq!(w.ecs().get::<Health>(p).unwrap().hp, 100.0);
    }
    assert!(w
        .edit_gameplay(|e| {
            e.set_name(p, "discard")?;
            let _ = e.registered_values(p, "missing.Type");
            Ok(())
        })
        .is_err());
    assert_eq!(w.ecs().get::<Name>(p).unwrap().value, "player");
}
#[test]
fn owned_sessions_and_destroyed_pending_records_do_not_publish_early() {
    let mut w = world();
    let p = player(&mut w);
    let mut session = GameplaySession::begin(w.into()).unwrap_or_else(|_| panic!("begin"));
    session
        .access(|e| {
            e.set_registered(
                p,
                Health {
                    hp: 5.0,
                    invulnerable: false,
                },
            )?;
            let pending = e.spawn("canceled", Transform::IDENTITY, None)?;
            e.add_registered_defaults(pending, "example.Health")?;
            e.destroy(pending)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        session.committed().ecs().get::<Health>(p).unwrap().hp,
        100.0
    );
    let (w, result) = session.finish();
    assert!(result.is_ok());
    assert_eq!(w.live_entities(), 1);
    assert_eq!(w.ecs().get::<Health>(p).unwrap().hp, 5.0);
}

#[test]
fn registered_access_respects_foreign_world_and_expired_pending_identities() {
    let mut w = world();
    let p = player(&mut w);
    let mut other = world();
    let foreign = player(&mut other);
    assert!(w
        .edit_gameplay(|e| {
            e.remove_registered(p, "example.Health")?;
            e.set_registered(
                foreign,
                Health {
                    hp: 1.0,
                    invulnerable: false,
                },
            )
        })
        .is_err());
    assert!(w.ecs().has::<Health>(p));
    let c = w
        .edit_gameplay(|e| e.spawn("old pending", Transform::IDENTITY, None))
        .unwrap();
    assert!(w
        .edit_gameplay(|e| e.add_registered_defaults(c.value, "example.Health"))
        .is_err());
    assert_eq!(other.ecs().get::<Health>(foreign).unwrap().hp, 100.0);
}

#[test]
fn codecs_cannot_silently_change_values_and_type_ids_cannot_alias() {
    struct Broken;
    impl RegisteredComponent for Broken {
        fn encode(&self) -> Vec<ScalarValue> {
            vec![ScalarValue::Float(0.0), ScalarValue::Bool(false)]
        }
        fn decode(_: &[ScalarValue]) -> Result<Self, String> {
            Ok(Self)
        }
    }
    let mut w = World::new(WorldConfig::default());
    assert!(matches!(
        w.register_component::<Broken>(health::schema()),
        Err(RegistryError::InvalidValue(_))
    ));
    assert_eq!(w.registered_schemas().count(), 0);
    w.register_component::<Health>(health::schema()).unwrap();
    assert!(matches!(
        w.register_component::<Broken>(health::schema()),
        Err(RegistryError::IdentityConflict(_))
    ));
    assert_eq!(w.registered_schemas().count(), 1);
}
