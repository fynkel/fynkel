//! All mutation entry points share transaction validation and publication.
use fynkel_contracts::ObjectRef;
use fynkel_core::*;
use glam::{Quat, Vec3};
fn spawn(w: &mut World) -> Entity {
    let c = w
        .edit_gameplay(|e| e.spawn("owner", Transform::default(), None))
        .unwrap();
    c.resolve(c.value).unwrap()
}
#[test]
fn gameplay_and_editor_reject_stale_references_without_publishing() {
    for editor in [false, true] {
        let mut w = World::new(Default::default());
        let owner = spawn(&mut w);
        let dead = spawn(&mut w);
        w.edit_gameplay(|e| e.destroy(dead)).unwrap();
        let value = ComponentValue::Follow(Follow {
            target: ObjectRef::to(dead),
            ..Default::default()
        });
        if editor {
            assert_eq!(
                w.apply_gameplay_now(vec![Command::SetComponent {
                    entity: owner,
                    value
                }])
                .len(),
                1
            );
        } else {
            assert!(w.edit_gameplay(|e| e.set_component(owner, value)).is_err());
        }
        assert!(!w.ecs().has::<Follow>(owner));
        assert_eq!(w.step().rejected, 0);
    }
}
#[test]
fn foreign_reference_rolls_back_other_writes_and_preserves_previous_value() {
    let mut w = World::new(Default::default());
    let owner = spawn(&mut w);
    let target = spawn(&mut w);
    let mut other = World::new(Default::default());
    let foreign = spawn(&mut other);
    w.edit_gameplay(|e| {
        e.set_component(
            owner,
            ComponentValue::Follow(Follow {
                target: ObjectRef::to(target),
                ..Default::default()
            }),
        )
    })
    .unwrap();
    assert!(w
        .edit_gameplay(|e| {
            e.set_name(owner, "discarded")?;
            e.set_component(
                owner,
                ComponentValue::Follow(Follow {
                    target: ObjectRef::to(foreign),
                    ..Default::default()
                }),
            )
        })
        .is_err());
    assert_eq!(w.ecs().get::<Name>(owner).unwrap().value, "owner");
    assert_eq!(
        w.ecs().get::<Follow>(owner).unwrap().target,
        ObjectRef::to(target)
    );
}
#[test]
fn editor_reads_previous_commit_without_advancing_a_tick() {
    let mut w = World::new(Default::default());
    let owner = spawn(&mut w);
    let tick = w.tick();
    w.edit_gameplay(|e| e.set_name(owner, "latest")).unwrap();
    assert!(w
        .apply_gameplay_now(vec![Command::SetActive {
            entity: owner,
            active: false
        }])
        .is_empty());
    w.edit_gameplay(|e| {
        assert_eq!(e.name(owner)?.as_deref(), Some("latest"));
        assert!(!e.active_self(owner)?);
        Ok(())
    })
    .unwrap();
    assert_eq!(w.tick(), tick);
}
#[test]
fn invalid_rotation_rejects_edit_and_spawn_without_later_replay() {
    let mut w = World::new(Default::default());
    let owner = spawn(&mut w);
    let invalid = Transform {
        rotation: Quat::from_xyzw(0., 0., 0., 0.),
        ..Default::default()
    };
    assert!(w
        .edit_gameplay(|e| e.set_transform(owner, invalid))
        .is_err());
    assert!(w
        .edit_gameplay(|e| e.spawn("invalid", invalid, None))
        .is_err());
    assert_eq!(w.step().rejected, 0);
    assert_eq!(w.live_entities(), 1);
    assert_eq!(
        w.ecs().copy::<Transform>(owner).unwrap(),
        Transform::default()
    );
}
#[test]
fn scoped_spawn_handles_are_unique_and_cannot_retarget_later_commits() {
    let mut w = World::new(Default::default());
    let c = w
        .edit_gameplay(|e| {
            let a = e.spawn("same", Transform::default(), None)?;
            let b = e.spawn("same", Transform::default(), None)?;
            e.set_transform(b, Transform::from_position(Vec3::X))?;
            Ok((a, b))
        })
        .unwrap();
    let a = c.resolve(c.value.0).unwrap();
    let b = c.resolve(c.value.1).unwrap();
    assert_ne!(a, b);
    assert!(w.edit_gameplay(|e| e.destroy(c.value.0)).is_err());
    assert_eq!(w.live_entities(), 2);
    assert_eq!(w.ecs().copy::<Transform>(a).unwrap().position, Vec3::ZERO);
    assert_eq!(w.ecs().copy::<Transform>(b).unwrap().position, Vec3::X);
}

#[cfg(feature = "audio")]
#[test]
fn playback_delivery_cannot_replay_old_configuration_over_a_later_commit() {
    let mut w = World::new(WorldConfig::default());
    let owner = spawn(&mut w);
    w.edit_gameplay(|e| {
        e.set_component(owner, ComponentValue::AudioSource(AudioSource::default()))?;
        e.audio_control(owner, AudioOp::SetVolume(0.2))?;
        e.audio_control(owner, AudioOp::SetPitch(0.5))
    })
    .unwrap();
    w.edit_gameplay(|edit| {
        edit.set_component(
            owner,
            ComponentValue::AudioSource(AudioSource {
                volume: 0.8,
                pitch: 1.5,
                ..Default::default()
            }),
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(w.step().rejected, 0);
    let source = w.ecs().copy::<AudioSource>(owner).unwrap();
    assert_eq!((source.volume, source.pitch), (0.8, 1.5));
    w.step();
    let source = w.ecs().copy::<AudioSource>(owner).unwrap();
    assert_eq!((source.volume, source.pitch), (0.8, 1.5));
}
