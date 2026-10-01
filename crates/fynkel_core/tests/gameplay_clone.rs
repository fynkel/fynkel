use fynkel_contracts::{AssetRef, ObjectRef, Uuid};
use fynkel_core::*;
use fynkel_scene as scene;
use glam::{Quat, Vec3};

fn at(x: f32) -> Transform {
    Transform::from_position(Vec3::X * x)
}
fn follow(target: impl Into<GameplayObjectRef>) -> GameplayComponentValue {
    let target = target.into();
    GameplayComponentValue::Follow(Follow::default().map_object_refs(|_| target))
}
fn spawn(w: &mut World, name: &str, pose: Transform, parent: Option<Entity>) -> Entity {
    let c = w
        .edit_gameplay(|e| e.spawn(name, pose, parent.map(Into::into)))
        .unwrap();
    c.resolve(c.value).unwrap()
}

#[test]
fn clones_staged_tree_in_order_with_internal_cycles_external_refs_and_resource_configs() {
    let mut w = World::new(WorldConfig::default());
    let external = spawn(&mut w, "external", at(10.0), None);
    let root = spawn(&mut w, "root", at(2.0), Some(external));
    let a = spawn(&mut w, "a", at(3.0), Some(root));
    let b = spawn(&mut w, "b", at(4.0), Some(root));
    let asset = AssetRef::new(Uuid::from_bytes(17u128.to_be_bytes()));
    let result = w
        .edit_gameplay(|e| {
            e.set_name(root, "staged")?;
            e.set_transform(root, at(5.0))?;
            e.set_active(root, false)?;
            e.set_component(root, follow(b))?;
            e.set_component(b, follow(root))?;
            e.set_component(a, follow(external))?;
            e.set_component(
                root,
                GameplayComponentValue::Animator(Animator {
                    clip: asset.clone(),
                    speed: 2.0,
                    ..Default::default()
                }),
            )?;
            e.set_component(
                a,
                GameplayComponentValue::AudioSource(AudioSource {
                    clip: asset.clone(),
                    volume: 0.3,
                    ..Default::default()
                }),
            )?;
            let copy = e.clone_tree(root.into(), None, None, Some("copy".into()))?;
            let ca = copy.handle_of(a.into()).unwrap();
            let cb = copy.handle_of(b.into()).unwrap();
            assert_eq!(
                copy.entities.iter().map(|p| p.0).collect::<Vec<_>>(),
                vec![root.into(), a.into(), b.into()]
            );
            assert_eq!(e.children(copy.root)?, vec![ca.into(), cb.into()]);
            assert_eq!(e.local_transform(copy.root)?.position.x, 15.0);
            assert!(!e.active_self(copy.root)?);
            assert!(e.active_self(ca)?);
            assert!(!e.active_in_hierarchy(ca)?);
            assert_eq!(e.name(copy.root)?.as_deref(), Some("copy"));
            assert_eq!(
                e.component(copy.root, ComponentId::FOLLOW)?,
                Some(follow(cb))
            );
            assert_eq!(
                e.component(cb, ComponentId::FOLLOW)?,
                Some(follow(copy.root))
            );
            assert_eq!(
                e.component(ca, ComponentId::FOLLOW)?,
                Some(follow(external))
            );
            // Freeze source configuration at the clone call, not final source state.
            e.set_name(root, "later")?;
            e.set_transform(b, at(99.0))?;
            Ok(copy)
        })
        .unwrap();
    let copy = result.resolve(result.value.root).unwrap();
    let ca = result
        .resolve(result.value.handle_of(a.into()).unwrap())
        .unwrap();
    let cb = result
        .resolve(result.value.handle_of(b.into()).unwrap())
        .unwrap();
    assert_eq!(
        w.ecs().copy::<Follow>(copy).unwrap().target,
        ObjectRef::to(cb)
    );
    assert_eq!(
        w.ecs().copy::<Follow>(cb).unwrap().target,
        ObjectRef::to(copy)
    );
    assert_eq!(
        w.ecs().copy::<Follow>(ca).unwrap().target,
        ObjectRef::to(external)
    );
    assert_eq!(w.ecs().copy::<Transform>(cb).unwrap().position.x, 4.0);
    assert_eq!(w.ecs().copy::<Animator>(copy).unwrap().clip, asset.clone());
    assert_eq!(w.ecs().copy::<AudioSource>(ca).unwrap().volume, 0.3);
}

#[test]
fn pending_sources_and_descendant_destinations_freeze_original_membership() {
    let mut w = World::new(WorldConfig::default());
    let result = w.edit_gameplay(|e| {
        let root = e.spawn("root", at(2.0), None)?;
        let child = e.spawn("child", at(3.0), Some(root.into()))?;
        e.set_component(root, follow(child))?;
        e.set_component(child, follow(root))?;
        let copy = e.clone_tree(root.into(), Some(child.into()), None, None)?;
        assert_eq!(copy.entities.len(), 2);
        assert_eq!(e.parent(copy.root)?, Some(child.into()));
        assert_eq!(e.local_transform(copy.root)?.position.x, 2.0);
        let copied_child = copy.handle_of(child.into()).unwrap();
        assert_eq!(
            e.component(copy.root, ComponentId::FOLLOW)?,
            Some(follow(copied_child))
        );
        assert_eq!(
            e.component(copied_child, ComponentId::FOLLOW)?,
            Some(follow(copy.root))
        );
        let explicit = e.clone_tree(copy.root.into(), None, Some(at(30.0)), Some(String::new()))?;
        assert_eq!(e.local_transform(explicit.root)?.position.x, 30.0);
        assert_eq!(e.name(explicit.root)?.as_deref(), Some(""));
        Ok(())
    });
    assert!(result.is_ok());
    assert_eq!(w.live_entities(), 6);
}

#[test]
fn clone_validation_is_sticky_and_allocates_nothing_on_invalid_references_or_placement() {
    let mut w = World::new(WorldConfig::default());
    let root = spawn(&mut w, "root", at(0.0), None);
    let mut other = World::new(WorldConfig::default());
    let foreign = spawn(&mut other, "foreign", at(0.0), None);
    // Native escape-hatch corruption must not be replicated by cloning.
    w.ecs_mut()
        .insert(
            root,
            Follow {
                target: ObjectRef::to(foreign),
                ..Default::default()
            },
        )
        .unwrap();
    let count = w.live_entities();
    assert!(w
        .edit_gameplay(|e| {
            e.set_name(root, "discarded")?;
            assert!(e.clone_tree(root.into(), None, None, None).is_err());
            Ok(())
        })
        .is_err());
    assert_eq!(w.live_entities(), count);
    assert_eq!(w.ecs().get::<Name>(root).unwrap().value.as_str(), "root");
    w.ecs_mut().remove::<Follow>(root).unwrap();
    assert!(w
        .edit_gameplay(|e| e.clone_tree(root.into(), None, Some(at(f32::NAN)), None))
        .is_err());
    assert_eq!(w.live_entities(), count);
    assert!(w
        .edit_gameplay(|e| {
            let copy = e.clone_tree(root.into(), None, None, None)?;
            e.set_component(copy.root, follow(root))?;
            e.destroy(root)?;
            Ok(())
        })
        .is_err());
    assert_eq!(w.live_entities(), count);
    assert!(w.ecs().contains(root));
}

#[test]
fn world_pose_with_shear_is_rejected_but_explicit_or_parented_placement_is_allowed() {
    let mut w = World::new(WorldConfig::default());
    let parent = spawn(
        &mut w,
        "parent",
        Transform {
            scale: Vec3::new(2.0, 1.0, 1.0),
            ..Default::default()
        },
        None,
    );
    let source = spawn(
        &mut w,
        "source",
        Transform {
            rotation: Quat::from_rotation_z(0.6),
            ..Default::default()
        },
        Some(parent),
    );
    assert!(w
        .edit_gameplay(|e| e.clone_tree(source.into(), None, None, None))
        .is_err());
    assert_eq!(w.live_entities(), 2);
    let c = w
        .edit_gameplay(|e| e.clone_tree(source.into(), Some(parent.into()), None, None))
        .unwrap();
    let clone = c.resolve(c.value.root).unwrap();
    assert_eq!(scene::parent_of(w.ecs(), clone), Some(parent));
    assert_eq!(
        w.ecs().copy::<Transform>(clone).unwrap(),
        w.ecs().copy::<Transform>(source).unwrap()
    );
    assert!(w
        .edit_gameplay(|e| e.clone_tree(source.into(), None, Some(Transform::IDENTITY), None))
        .is_ok());
}

#[test]
fn clone_internal_references_survive_source_destruction_and_staged_removal() {
    let mut w = World::new(WorldConfig::default());
    let root = spawn(&mut w, "root", at(0.0), None);
    let child = spawn(&mut w, "child", at(1.0), Some(root));
    w.edit_gameplay(|e| {
        e.set_component(root, follow(child))?;
        e.set_component(root, ComponentValue::Camera(Camera::default()))?;
        e.set_component(
            child,
            GameplayComponentValue::ThirdPersonCamera(
                ThirdPersonCamera::default().map_object_refs(|_| root.into()),
            ),
        )?;
        e.set_component(
            child,
            GameplayComponentValue::Skin(<Skin>::default().map_object_refs(|_| child.into())),
        )
    })
    .unwrap();
    let c = w
        .edit_gameplay(|e| {
            e.remove_component(root, ComponentId::CAMERA)?;
            let copy = e.clone_tree(root.into(), None, None, None)?;
            assert!(!e.has_component(copy.root, ComponentId::CAMERA)?);
            e.destroy(root)?;
            Ok(copy)
        })
        .unwrap();
    let new_root = c.resolve(c.value.root).unwrap();
    let new_child = c.resolve(c.value.handle_of(child.into()).unwrap()).unwrap();
    assert!(!w.ecs().contains(root));
    assert!(!w.ecs().contains(child));
    assert_eq!(
        w.ecs().copy::<Follow>(new_root).unwrap().target.get(),
        Some(new_child)
    );
    assert_eq!(
        w.ecs()
            .copy::<ThirdPersonCamera>(new_child)
            .unwrap()
            .target
            .get(),
        Some(new_root)
    );
    assert_eq!(
        w.ecs().copy::<Skin>(new_child).unwrap().skeleton.get(),
        Some(new_child)
    );
}

#[test]
fn expired_external_weak_reference_is_not_silently_repaired_or_replicated() {
    let mut w = World::new(WorldConfig::default());
    let root = spawn(&mut w, "root", at(0.0), None);
    let external = spawn(&mut w, "external", at(1.0), None);
    w.edit_gameplay(|e| e.set_component(root, follow(external)))
        .unwrap();
    w.edit_gameplay(|e| e.destroy(external)).unwrap();
    assert!(w
        .edit_gameplay(|e| e.clone_tree(root.into(), None, None, None))
        .is_err());
    assert_eq!(w.live_entities(), 1);
    assert_eq!(
        w.ecs().copy::<Follow>(root).unwrap().target.get(),
        Some(external)
    );
}

#[cfg(feature = "physics")]
#[test]
fn runtime_body_velocity_is_not_cloned_with_rigid_body_configuration() {
    let mut w = World::new(WorldConfig::default());
    let source = spawn(&mut w, "body", at(0.0), None);
    w.edit_gameplay(|e| {
        e.set_component(
            source,
            ComponentValue::RigidBody(RigidBody {
                body_type: BodyType::Dynamic,
                gravity_scale: 0.0,
                linear_damping: 0.0,
                ..Default::default()
            }),
        )?;
        e.set_component(source, ComponentValue::Collider(Collider::default()))?;
        e.body_effect(source, BodyCommand::SetLinearVelocity(Vec3::X * 3.0))
    })
    .unwrap();
    w.step();
    assert_eq!(
        w.physics().body_state(source).unwrap().linear_velocity.x,
        3.0
    );
    let c = w
        .edit_gameplay(|e| e.clone_tree(source.into(), None, Some(at(100.0)), None))
        .unwrap();
    let copy = c.resolve(c.value.root).unwrap();
    assert!(w.physics().body_state(copy).is_none());
    w.step();
    assert_eq!(
        w.physics().body_state(copy).unwrap().linear_velocity,
        Vec3::ZERO
    );
    assert_eq!(
        w.physics().body_state(source).unwrap().linear_velocity.x,
        3.0
    );
}

#[test]
fn legacy_clone_adopts_queued_edits_and_preserves_breadth_first_report_order() {
    let mut w = World::new(WorldConfig::default());
    let root = spawn(&mut w, "root", at(0.0), None);
    let a = spawn(&mut w, "a", at(1.0), Some(root));
    let b = spawn(&mut w, "b", at(2.0), Some(root));
    let grandchild = spawn(&mut w, "grandchild", at(3.0), Some(a));
    w.submit(
        Lane::Simulation,
        vec![
            Command::SetName {
                entity: a,
                name: "queued".into(),
            },
            Command::SetTransform {
                entity: b,
                transform: at(9.0),
            },
        ],
    );
    let cloned = w.clone_subtree(root, None, at(10.0), None).unwrap();
    assert_eq!(
        cloned.pairs.iter().map(|p| p.0).collect::<Vec<_>>(),
        vec![root, a, b, grandchild]
    );
    assert_eq!(
        w.ecs()
            .get::<Name>(cloned.pairs[1].1)
            .unwrap()
            .value
            .as_str(),
        "queued"
    );
    assert_eq!(
        w.ecs()
            .copy::<Transform>(cloned.pairs[2].1)
            .unwrap()
            .position
            .x,
        9.0
    );
    assert_eq!(cloned.rejected_components, 0);
    w.edit_gameplay(|e| e.set_transform(cloned.root, at(20.0)))
        .unwrap();
    w.step();
    assert_eq!(
        w.ecs().copy::<Transform>(cloned.root).unwrap().position.x,
        20.0
    );
}

#[test]
fn legacy_clone_rejects_invalid_configuration_atomically_after_older_commit() {
    let mut w = World::new(WorldConfig::default());
    let root = spawn(&mut w, "root", at(0.0), None);
    let target = spawn(&mut w, "target", at(1.0), None);
    w.edit_gameplay(|e| e.set_component(root, follow(target)))
        .unwrap();
    w.edit_gameplay(|e| e.destroy(target)).unwrap();
    w.submit(
        Lane::Simulation,
        vec![Command::SetName {
            entity: root,
            name: "older commit".into(),
        }],
    );
    assert!(matches!(
        w.clone_subtree(root, None, at(5.0), None),
        Err(LoadError::CloneRejected(_))
    ));
    assert_eq!(w.live_entities(), 1);
    assert_eq!(
        w.ecs().get::<Name>(root).unwrap().value.as_str(),
        "older commit"
    );
}

#[test]
fn legacy_clone_adoption_reports_spawn_mappings_once_even_when_clone_fails() {
    for fail_clone in [false, true] {
        let mut w = World::new(WorldConfig::default());
        let root = spawn(&mut w, "root", at(0.0), None);
        if fail_clone {
            let stale = spawn(&mut w, "stale", at(0.0), None);
            w.edit_gameplay(|e| e.set_component(root, follow(stale)))
                .unwrap();
            w.edit_gameplay(|e| e.destroy(stale)).unwrap();
        }
        w.drain_events();
        w.submit(
            Lane::Simulation,
            vec![
                Command::Spawn {
                    temp_id: 17,
                    transform: at(1.0),
                    parent: None,
                    name: None,
                },
                Command::Spawn {
                    temp_id: 18,
                    transform: at(2.0),
                    parent: None,
                    name: None,
                },
                Command::Destroy(Entity::temp(18)),
            ],
        );
        assert_eq!(
            w.clone_subtree(root, None, at(5.0), None).is_err(),
            fail_clone
        );
        let spawned: Vec<_> = w
            .drain_events()
            .into_iter()
            .filter_map(|event| match event.payload {
                EventPayload::Spawned { temp_id, entity } => Some((temp_id, entity)),
                _ => None,
            })
            .collect();
        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned[0].0, 17);
        assert!(w.ecs().contains(spawned[0].1));
        w.step();
        assert!(!w
            .drain_events()
            .iter()
            .any(|event| matches!(event.payload, EventPayload::Spawned { .. })));
    }
}
