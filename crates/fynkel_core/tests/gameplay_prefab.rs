use fynkel_contracts::{ObjectRef, Uuid};
use fynkel_core::*;
use glam::Vec3;

fn id(n: u128) -> Uuid {
    Uuid::from_bytes(n.to_be_bytes())
}
fn at(x: f32) -> Transform {
    Transform::from_position(Vec3::X * x)
}
fn world() -> World {
    World::new(WorldConfig::default())
}
fn prefab() -> SceneDescription {
    SceneDescription {
        id: id(10),
        name: "prefab".into(),
        // Deliberately not in hierarchy order.
        entities: vec![
            EntityDescription {
                registered: Vec::new(),
                id: id(2),
                name: "child".into(),
                active: true,
                parent: Some(id(1)),
                children: vec![],
                components: vec![
                    ComponentValue::Transform(at(2.0)),
                    ComponentValue::Skin(Skin {
                        skeleton: ObjectRef::authored(id(1)),
                        ..Default::default()
                    }),
                ],
            },
            EntityDescription {
                registered: Vec::new(),
                id: id(1),
                name: "root".into(),
                active: false,
                parent: None,
                children: vec![id(2)],
                components: vec![
                    ComponentValue::Transform(at(99.0)),
                    ComponentValue::Follow(Follow {
                        target: ObjectRef::authored(id(2)),
                        ..Default::default()
                    }),
                    ComponentValue::ThirdPersonCamera(ThirdPersonCamera {
                        target: ObjectRef::authored(id(2)),
                        ..Default::default()
                    }),
                ],
            },
        ],
    }
}

#[test]
fn stages_hierarchy_activity_and_all_reference_families_under_pending_parent() {
    let mut w = world();
    let commit = w
        .edit_gameplay(|edit| {
            let parent = edit.spawn("parent", at(10.0), None)?;
            let instance = edit.instantiate(&prefab(), "placed", at(3.0), Some(parent.into()))?;
            let child = instance.handle_of(id(2)).unwrap();
            assert_eq!(
                instance.entities.iter().map(|p| p.0).collect::<Vec<_>>(),
                vec![id(1), id(2)]
            );
            assert_eq!(edit.parent(instance.root)?, Some(parent.into()));
            assert_eq!(edit.children(instance.root)?, vec![child.into()]);
            assert_eq!(edit.world_matrix(child)?.w_axis.x, 15.0);
            assert!(!edit.active_in_hierarchy(child)?);
            assert_eq!(edit.name(instance.root)?.as_deref(), Some("placed"));
            match edit.component(instance.root, ComponentId::FOLLOW)?.unwrap() {
                GameplayComponentValue::Follow(v) => assert_eq!(v.target, child.into()),
                _ => panic!("wrong component"),
            }
            Ok(instance)
        })
        .unwrap();
    let root = commit.resolve(commit.value.root).unwrap();
    let child = commit
        .resolve(commit.value.handle_of(id(2)).unwrap())
        .unwrap();
    assert_eq!(
        w.ecs().get::<Follow>(root).unwrap().target.get(),
        Some(child)
    );
    assert_eq!(
        w.ecs().get::<ThirdPersonCamera>(root).unwrap().target.get(),
        Some(child)
    );
    assert_eq!(
        w.ecs().get::<Skin>(child).unwrap().skeleton.get(),
        Some(root)
    );
}

#[test]
fn instances_remap_authoring_ids_independently_and_empty_name_keeps_root_name() {
    let mut w = world();
    let commit = w
        .edit_gameplay(|edit| {
            let a = edit.instantiate(&prefab(), "", at(1.0), None)?;
            let b = edit.instantiate(&prefab(), "copy", at(2.0), Some(a.root.into()))?;
            assert_eq!(edit.name(a.root)?.as_deref(), Some("root"));
            Ok((a, b))
        })
        .unwrap();
    for instance in [&commit.value.0, &commit.value.1] {
        let root = commit.resolve(instance.root).unwrap();
        let child = commit.resolve(instance.handle_of(id(2)).unwrap()).unwrap();
        assert_eq!(
            w.ecs().get::<Follow>(root).unwrap().target.get(),
            Some(child)
        );
    }
    assert_ne!(commit.value.0.root, commit.value.1.root);
}

#[test]
fn malformed_structure_and_multiple_roots_abort_prior_edits_even_when_caught() {
    let mut invalid = prefab();
    invalid.entities[0].parent = None;
    let mut duplicate = prefab();
    duplicate.entities.push(duplicate.entities[0].clone());
    let mut two_roots = prefab();
    two_roots.entities[0].parent = None;
    two_roots.entities[1].children.clear();
    let mut duplicate_component = prefab();
    duplicate_component.entities[0]
        .components
        .push(ComponentValue::Transform(at(0.0)));
    for p in [
        invalid,
        duplicate,
        two_roots,
        duplicate_component,
        SceneDescription::default(),
    ] {
        let mut w = world();
        assert!(w
            .edit_gameplay(|edit| {
                edit.spawn("earlier", at(0.0), None)?;
                assert!(matches!(
                    edit.instantiate(&p, "", at(0.0), None),
                    Err(EditError::InvalidPrefab(_))
                ));
                Ok(())
            })
            .is_err());
        assert!(w
            .edit_gameplay(|edit| edit.entities())
            .unwrap()
            .value
            .is_empty());
    }
}

#[test]
fn unresolved_authored_reference_does_not_fall_back_to_stored_runtime_handle() {
    let mut w = world();
    let live = w
        .edit_gameplay(|edit| edit.spawn("live", at(0.0), None))
        .unwrap();
    let live = live.resolve(live.value).unwrap();
    let mut p = prefab();
    p.entities[0].components = vec![ComponentValue::Follow(Follow {
        target: ObjectRef {
            id: id(99),
            entity: live,
        },
        ..Default::default()
    })];
    assert_eq!(
        w.edit_gameplay(|edit| edit.instantiate(&p, "", at(0.0), None))
            .unwrap_err(),
        EditError::UnresolvedObjectReference(id(99))
    );
    assert_eq!(
        w.edit_gameplay(|edit| edit.entities()).unwrap().value,
        vec![live.into()]
    );
}

#[test]
fn invalid_component_rejects_whole_prefab_instead_of_partial_acceptance() {
    let mut w = world();
    let mut p = prefab();
    p.entities[0]
        .components
        .push(ComponentValue::PointLight(PointLight {
            intensity: f32::NAN,
            ..Default::default()
        }));
    assert!(matches!(
        w.edit_gameplay(|edit| edit.instantiate(&p, "", at(0.0), None)),
        Err(EditError::InvalidComponent { .. })
    ));
    assert!(w
        .edit_gameplay(|edit| edit.entities())
        .unwrap()
        .value
        .is_empty());
}

#[test]
fn final_reference_validation_and_explicit_abort_discard_staged_instances() {
    let mut w = world();
    assert!(w
        .edit_gameplay(|edit| {
            let p = edit.instantiate(&prefab(), "", at(0.0), None)?;
            edit.destroy(p.handle_of(id(2)).unwrap())
        })
        .is_err());
    assert!(w
        .edit_gameplay(|edit| edit.entities())
        .unwrap()
        .value
        .is_empty());
    assert_eq!(
        w.edit_gameplay(|edit| {
            edit.instantiate(&prefab(), "", at(0.0), None)?;
            Err::<(), _>(EditError::Aborted)
        })
        .unwrap_err(),
        EditError::Aborted
    );
    assert!(w
        .edit_gameplay(|edit| edit.entities())
        .unwrap()
        .value
        .is_empty());
}

#[test]
fn destroying_whole_instance_cancels_all_pending_handles() {
    let mut w = world();
    let c = w
        .edit_gameplay(|edit| {
            let p = edit.instantiate(&prefab(), "", at(0.0), None)?;
            edit.destroy(p.root)?;
            Ok(p)
        })
        .unwrap();
    for (_, pending) in c.value.entities.iter() {
        assert!(c.resolve(*pending).is_none());
    }
    assert_eq!(c.spawned().count(), 0);
}

#[test]
fn runtime_only_references_use_normal_world_validation() {
    let mut w = world();
    let c = w
        .edit_gameplay(|edit| edit.spawn("external", at(0.0), None))
        .unwrap();
    let external = c.resolve(c.value).unwrap();
    let mut p = prefab();
    p.entities[0].components = vec![ComponentValue::Follow(Follow {
        target: ObjectRef::to(external),
        ..Default::default()
    })];
    let c = w
        .edit_gameplay(|edit| edit.instantiate(&p, "", at(0.0), None))
        .unwrap();
    let child = c.resolve(c.value.handle_of(id(2)).unwrap()).unwrap();
    assert_eq!(
        w.ecs().get::<Follow>(child).unwrap().target.get(),
        Some(external)
    );
    let mut other = world();
    assert!(matches!(
        other.edit_gameplay(|edit| edit.instantiate(&p, "", at(0.0), None)),
        Err(EditError::StaleEntity(_))
    ));
    assert!(other
        .edit_gameplay(|edit| edit.entities())
        .unwrap()
        .value
        .is_empty());
}

#[test]
fn foreign_parent_expired_parent_and_invalid_placement_are_sticky() {
    let mut w = world();
    let prior = w
        .edit_gameplay(|edit| edit.spawn("prior", at(0.0), None))
        .unwrap();
    let mut other = world();
    let foreign = other
        .edit_gameplay(|edit| edit.spawn("foreign", at(0.0), None))
        .unwrap();
    let foreign = foreign.resolve(foreign.value).unwrap();
    for (pose, parent) in [
        (at(0.0), Some(prior.value.into())),
        (at(0.0), Some(foreign.into())),
        (at(f32::NAN), None),
    ] {
        assert!(w
            .edit_gameplay(|edit| {
                edit.spawn("discard", at(0.0), None)?;
                assert!(edit.instantiate(&prefab(), "", pose, parent).is_err());
                Ok(())
            })
            .is_err());
        assert_eq!(
            w.edit_gameplay(|edit| edit.entities()).unwrap().value.len(),
            1
        );
    }
}
