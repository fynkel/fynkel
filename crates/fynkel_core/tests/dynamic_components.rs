use fynkel_core::*;
fn schema() -> ComponentSchema {
    ComponentSchema {
        id: "game.Data".into(),
        version: 1,
        fields: vec![
            RegisteredField {
                name: "hp".into(),
                default: ScalarValue::Float(100.),
            },
            RegisteredField {
                name: "target".into(),
                default: ScalarValue::Object(None),
            },
        ],
    }
}
fn world() -> World {
    let mut w = World::new(WorldConfig::default());
    w.register_dynamic_component(schema()).unwrap();
    w
}
#[test]
fn dynamic_references_remap_pending_clones_and_destroyed_targets() {
    let mut w = world();
    let c = w
        .edit_gameplay(|e| {
            let p = e.spawn("p", Transform::IDENTITY, None)?;
            e.set_registered_values(
                p,
                "game.Data",
                vec![ScalarValue::Float(5.), ScalarValue::Object(Some(p.into()))],
            )?;
            let copy = e.clone_tree(p.into(), None, None, None)?.root;
            assert_eq!(
                e.registered_values(copy, "game.Data")?.unwrap()[1],
                ScalarValue::Object(Some(copy.into()))
            );
            Ok((p, copy))
        })
        .unwrap();
    let p = c.resolve(c.value.0).unwrap();
    let copy = c.resolve(c.value.1).unwrap();
    w.edit_gameplay(|e| {
        assert_eq!(
            e.registered_values(copy, "game.Data")?.unwrap()[1],
            ScalarValue::Object(Some(copy.into()))
        );
        e.set_registered_values(
            copy,
            "game.Data",
            vec![ScalarValue::Float(6.), ScalarValue::Object(Some(p.into()))],
        )?;
        e.destroy(p)?;
        assert_eq!(
            e.registered_values(copy, "game.Data")?.unwrap()[1],
            ScalarValue::Object(None)
        );
        Ok(())
    })
    .unwrap();
    w.edit_gameplay(|e| {
        assert_eq!(
            e.registered_values(copy, "game.Data")?.unwrap()[1],
            ScalarValue::Object(None)
        );
        Ok(())
    })
    .unwrap();
}
#[test]
fn bulk_view_cannot_escape_its_edit_or_overwrite_intervening_values() {
    let mut w = world();
    let c = w
        .edit_gameplay(|e| {
            let p = e.spawn("p", Transform::IDENTITY, None)?;
            e.add_registered_defaults(p, "game.Data")?;
            Ok(p)
        })
        .unwrap();
    let p = c.resolve(c.value).unwrap();
    let batch = w
        .edit_gameplay(|e| e.query_registered("game.Data"))
        .unwrap()
        .value;
    assert!(w
        .edit_gameplay(|e| e.apply_registered_batch(
            &batch,
            vec![vec![ScalarValue::Float(0.), ScalarValue::Object(None)]]
        ))
        .is_err());
    assert!(w
        .edit_gameplay(|e| {
            let batch = e.query_registered("game.Data")?;
            e.remove_registered(p, "game.Data")?;
            e.add_registered_defaults(p, "game.Data")?;
            e.apply_registered_batch(
                &batch,
                vec![vec![ScalarValue::Float(0.), ScalarValue::Object(None)]],
            )
        })
        .is_err());
    w.edit_gameplay(|e| {
        let batch = e.query_registered("game.Data")?;
        let values = batch
            .rows()
            .iter()
            .map(|_| vec![ScalarValue::Float(50.), ScalarValue::Object(None)])
            .collect();
        e.apply_registered_batch(&batch, values)?;
        assert_eq!(
            e.registered_values(p, "game.Data")?.unwrap()[0],
            ScalarValue::Float(50.)
        );
        Ok(())
    })
    .unwrap();
}
#[test]
fn foreign_reference_invalid_batch_and_version_conflict_preserve_world() {
    let mut w = world();
    let mut other = world();
    let foreign = other
        .edit_gameplay(|e| e.spawn("foreign", Transform::IDENTITY, None))
        .unwrap();
    let foreign = foreign.resolve(foreign.value).unwrap();
    assert!(w
        .edit_gameplay(|e| {
            let p = e.spawn("discard", Transform::IDENTITY, None)?;
            e.set_registered_values(
                p,
                "game.Data",
                vec![
                    ScalarValue::Float(1.),
                    ScalarValue::Object(Some(foreign.into())),
                ],
            )
        })
        .is_err());
    assert_eq!(w.live_entities(), 0);
    let mut changed = schema();
    changed.version = 2;
    assert!(w.register_dynamic_component(changed).is_err());
    assert_eq!(w.registered_schemas().next().unwrap().version, 1);
    assert!(w
        .edit_gameplay(|e| {
            let p = e.spawn("discard", Transform::IDENTITY, None)?;
            e.add_registered_defaults(p, "game.Data")?;
            let b = e.query_registered("game.Data")?;
            e.apply_registered_batch(
                &b,
                vec![vec![
                    ScalarValue::Float(f64::NAN),
                    ScalarValue::Object(None),
                ]],
            )
        })
        .is_err());
    assert_eq!(w.live_entities(), 0);
}

#[test]
fn duplicate_authored_components_are_rejected_before_spawn() {
    let mut w = world();
    let value = RegisteredDescription {
        id: "game.Data".into(),
        version: 1,
        fields: Default::default(),
    };
    assert!(w
        .spawn_now_registered(
            "discard",
            Transform::IDENTITY,
            None,
            vec![],
            vec![value.clone(), value],
            None
        )
        .is_err());
    assert_eq!(w.live_entities(), 0);
}
