use fynkel_core::*;
use glam::Vec3;

#[test]
fn disabled_physics_rejects_effects_stickily_in_full_and_stripped_builds() {
    let mut w = World::new(WorldConfig::default());
    w.set_modules(Modules {
        physics: false,
        ..Modules::ALL
    });
    assert!(w
        .edit_gameplay(|edit| {
            let p = edit.spawn("discard", Transform::default(), None)?;
            let _ = edit.body_effect(p, BodyCommand::ApplyImpulse(Vec3::X));
            Ok(())
        })
        .is_err());
    assert_eq!(w.ecs().len(), 0);
}

#[cfg(feature = "physics")]
mod enabled {
    use super::*;
    use glam::Quat;

    fn body(edit: &mut GameplayEdit<'_>, name: &str) -> Result<PendingEntity, EditError> {
        let p = edit.spawn(name, Transform::default(), None)?;
        edit.set_component(
            p,
            ComponentValue::RigidBody(RigidBody {
                body_type: BodyType::Dynamic,
                mass: 2.0,
                gravity_scale: 0.0,
                linear_damping: 0.0,
                angular_damping: 0.0,
                ..RigidBody::default()
            }),
        )?;
        edit.set_component(p, ComponentValue::Collider(Collider::default()))?;
        Ok(p)
    }
    fn velocity(w: &World, e: Entity) -> f32 {
        w.physics().body_state(e).unwrap().linear_velocity.x
    }

    #[test]
    fn pending_effects_commit_in_order_and_execute_only_once_at_next_tick() {
        let mut w = World::new(WorldConfig::default());
        let commit = w
            .edit_gameplay(|edit| {
                let p = body(edit, "body")?;
                edit.body_effect(p, BodyCommand::SetLinearVelocity(Vec3::X * 3.0))?;
                edit.body_effect(p, BodyCommand::ApplyImpulse(Vec3::X * 4.0))?;
                Ok(p)
            })
            .unwrap();
        let e = commit.resolve(commit.value).unwrap();
        assert!(w.physics().body_state(e).is_none());
        // Committed effects do not block another edit or require a legacy flush.
        w.edit_gameplay(|edit| edit.set_name(e, "renamed")).unwrap();
        w.begin_frame(0.0);
        assert!(w.physics().body_state(e).is_none());
        w.step();
        assert!((velocity(&w, e) - 5.0).abs() < 1e-5);
        w.step();
        assert!((velocity(&w, e) - 5.0).abs() < 1e-5);
    }

    #[test]
    fn abort_and_final_configuration_failure_publish_no_effect_or_data() {
        let mut w = World::new(WorldConfig::default());
        let c = w.edit_gameplay(|edit| body(edit, "body")).unwrap();
        let e = c.resolve(c.value).unwrap();
        w.step();
        for explicit in [false, true] {
            assert!(w
                .edit_gameplay(|edit| {
                    edit.set_name(e, "discard")?;
                    edit.body_effect(e, BodyCommand::ApplyImpulse(Vec3::X * 10.0))?;
                    if explicit {
                        return Err(EditError::Aborted);
                    }
                    edit.remove_component(e, ComponentId::RIGID_BODY)?;
                    Ok(())
                })
                .is_err());
            w.step();
            assert_eq!(velocity(&w, e), 0.0);
            assert_eq!(w.ecs().get::<Name>(e).unwrap().value.as_str(), "body");
        }
    }

    #[test]
    fn destruction_cancels_effects_within_and_after_commit() {
        let mut w = World::new(WorldConfig::default());
        let c = w
            .edit_gameplay(|edit| {
                let p = body(edit, "cancel")?;
                edit.body_effect(p, BodyCommand::ApplyImpulse(Vec3::X))?;
                edit.destroy(p)?;
                Ok(p)
            })
            .unwrap();
        assert!(c.resolve(c.value).is_none());
        let c = w
            .edit_gameplay(|edit| {
                let p = body(edit, "later")?;
                edit.body_effect(p, BodyCommand::ApplyImpulse(Vec3::X))?;
                Ok(p)
            })
            .unwrap();
        w.edit_gameplay(|edit| edit.destroy(c.resolve(c.value).unwrap()))
            .unwrap();
        w.step();
        assert!(w.diagnostics().is_empty());
        assert_eq!(w.ecs().len(), 0);
    }

    #[test]
    fn invalid_payload_type_and_owner_fail_before_publication() {
        let mut w = World::new(WorldConfig::default());
        for op in [
            BodyCommand::ApplyForce(Vec3::splat(f32::NAN)),
            BodyCommand::Teleport {
                position: Vec3::ZERO,
                rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            },
            BodyCommand::SetKinematicTarget {
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            },
            BodyCommand::Move(Vec3::X),
        ] {
            assert!(w
                .edit_gameplay(|edit| {
                    let p = body(edit, "invalid")?;
                    let _ = edit.body_effect(p, op);
                    Ok(())
                })
                .is_err());
            assert_eq!(w.ecs().len(), 0);
        }
    }

    #[test]
    fn inactive_effects_are_consumed_without_replaying_on_activation() {
        let mut w = World::new(WorldConfig::default());
        let c = w
            .edit_gameplay(|edit| {
                let p = body(edit, "inactive")?;
                edit.body_effect(p, BodyCommand::ApplyImpulse(Vec3::X))?;
                edit.set_active(p, false)?;
                Ok(p)
            })
            .unwrap();
        let e = c.resolve(c.value).unwrap();
        w.step();
        w.edit_gameplay(|edit| edit.set_active(e, true)).unwrap();
        w.step();
        assert_eq!(velocity(&w, e), 0.0);
    }

    #[test]
    fn teleport_updates_simulation_pose_on_the_next_tick() {
        let mut w = World::new(WorldConfig::default());
        let c = w.edit_gameplay(|edit| body(edit, "body")).unwrap();
        let e = c.resolve(c.value).unwrap();
        w.step();
        w.edit_gameplay(|edit| {
            edit.body_effect(
                e,
                BodyCommand::Teleport {
                    position: Vec3::X * 8.0,
                    rotation: Quat::IDENTITY,
                },
            )
        })
        .unwrap();
        assert_eq!(w.ecs().get::<Transform>(e).unwrap().position.x, 0.0);
        w.step();
        assert_eq!(
            w.ecs().get::<Transform>(e).unwrap().position.x,
            8.0,
            "{:?} {:?}",
            w.physics().body_state(e),
            w.diagnostics()
        );
    }

    #[test]
    fn later_configuration_changes_are_revalidated_at_execution() {
        let mut w = World::new(WorldConfig::default());
        let c = w
            .edit_gameplay(|edit| {
                let p = body(edit, "body")?;
                edit.body_effect(p, BodyCommand::ApplyImpulse(Vec3::X))?;
                Ok(p)
            })
            .unwrap();
        let e = c.resolve(c.value).unwrap();
        w.edit_gameplay(|edit| edit.remove_component(e, ComponentId::RIGID_BODY))
            .unwrap();
        w.step();
        assert_eq!(w.diagnostics().len(), 1);
        w.step();
        assert!(w.diagnostics().is_empty());
    }
}

#[test]
fn disabled_audio_rejects_sound_and_rolls_back_data() {
    let mut w = World::new(WorldConfig::default());
    w.set_modules(Modules {
        audio: false,
        ..Modules::ALL
    });
    assert!(w
        .edit_gameplay(|edit| {
            edit.spawn("discard", Transform::default(), None)?;
            let _ = edit.play_sound(
                AssetRef::new(Uuid::from_bytes([1; 16])),
                PlayParams::default(),
            );
            Ok(())
        })
        .is_err());
    assert_eq!(w.ecs().len(), 0);
}

#[cfg(feature = "audio")]
mod sounds {
    use super::*;
    fn fixture() -> (World, AssetRef) {
        let mut w = World::new(WorldConfig::default());
        let clip = AssetRef::new(Uuid::from_bytes([1; 16]));
        w.register_sound(
            clip.clone(),
            AudioClip::new(48000, 1, vec![0.5; 48000]).unwrap(),
        )
        .unwrap();
        (w, clip)
    }
    fn source(edit: &mut GameplayEdit<'_>, clip: AssetRef) -> Result<PendingEntity, EditError> {
        let p = edit.spawn("speaker", Transform::default(), None)?;
        edit.set_component(
            p,
            ComponentValue::AudioSource(AudioSource {
                clip,
                ..AudioSource::default()
            }),
        )?;
        Ok(p)
    }
    #[test]
    fn pending_play_pause_and_resume_preserve_order_and_logical_configuration() {
        let (mut w, clip) = fixture();
        let c = w
            .edit_gameplay(|edit| {
                let p = source(edit, clip)?;
                edit.audio_control(p, AudioOp::Play)?;
                edit.audio_control(p, AudioOp::Pause)?;
                edit.audio_control(p, AudioOp::SetVolume(0.25))?;
                let Some(GameplayComponentValue::AudioSource(s)) =
                    edit.component(p, ComponentId::AUDIO_SOURCE)?
                else {
                    panic!()
                };
                assert!(s.playing);
                assert_eq!(s.volume, 0.25);
                Ok(p)
            })
            .unwrap();
        let e = c.resolve(c.value).unwrap();
        assert_eq!(w.audio().voice_count(), 0);
        w.step();
        let state = w.audio_state(e).unwrap();
        assert!(state.playing && state.paused);
        assert_eq!(state.time, 0.0);
        assert_eq!(state.volume, 0.25);
        w.edit_gameplay(|edit| {
            edit.audio_control(e, AudioOp::Resume)?;
            edit.audio_control(e, AudioOp::SetPitch(2.0))
        })
        .unwrap();
        assert_eq!(w.audio_state(e).unwrap().pitch, 1.0);
        assert!(w.audio_state(e).unwrap().paused);
        w.step();
        assert!(!w.audio_state(e).unwrap().paused);
        assert_eq!(w.audio_state(e).unwrap().pitch, 2.0);
        assert_eq!(w.audio().stats().started, 1);
    }
    #[test]
    fn voice_query_does_not_mix_new_clip_configuration_with_old_playback() {
        let (mut w, clip) = fixture();
        let c = w
            .edit_gameplay(|edit| {
                let p = source(edit, clip.clone())?;
                edit.audio_control(p, AudioOp::Play)?;
                Ok(p)
            })
            .unwrap();
        let e = c.resolve(c.value).unwrap();
        w.step();
        let new_clip = AssetRef::new(Uuid::from_bytes([2; 16]));
        w.register_sound(
            new_clip.clone(),
            AudioClip::new(48000, 1, vec![0.5; 96000]).unwrap(),
        )
        .unwrap();
        w.edit_gameplay(|edit| {
            edit.set_component(
                e,
                ComponentValue::AudioSource(AudioSource {
                    clip: new_clip.clone(),
                    playing: true,
                    volume: 0.5,
                    pitch: 2.0,
                    looping: true,
                    ..AudioSource::default()
                }),
            )
        })
        .unwrap();
        let state = w.audio_state(e).unwrap();
        assert_eq!(state.clip, clip);
        assert_eq!(state.duration, 1.0);
        assert_eq!(state.volume, 1.0);
        assert!(!state.looping);
        w.step();
        let state = w.audio_state(e).unwrap();
        assert_eq!(state.clip, new_clip);
        assert_eq!(state.duration, 2.0);
        assert_eq!(state.volume, 0.5);
        assert!(state.looping);
    }
    #[test]
    fn transport_never_replays_configuration_over_later_writes() {
        let (mut w, clip) = fixture();
        let c = w
            .edit_gameplay(|edit| {
                let p = source(edit, clip.clone())?;
                edit.audio_control(p, AudioOp::Play)?;
                Ok(p)
            })
            .unwrap();
        let e = c.resolve(c.value).unwrap();
        w.edit_gameplay(|edit| {
            edit.set_component(
                e,
                ComponentValue::AudioSource(AudioSource {
                    clip: clip.clone(),
                    playing: false,
                    volume: 0.75,
                    ..AudioSource::default()
                }),
            )
        })
        .unwrap();
        w.step();
        assert_eq!(w.audio().voice_count(), 0);
        w.edit_gameplay(|edit| {
            edit.audio_control(e, AudioOp::Play)?;
            edit.audio_control(e, AudioOp::Stop)?;
            edit.set_component(
                e,
                ComponentValue::AudioSource(AudioSource {
                    clip,
                    playing: true,
                    volume: 0.5,
                    ..AudioSource::default()
                }),
            )
        })
        .unwrap();
        w.step();
        assert!(w.audio_state(e).unwrap().playing);
        assert_eq!(w.audio_state(e).unwrap().volume, 0.5);
    }
    #[test]
    fn source_abort_invalid_payload_and_final_removal_are_atomic() {
        let (mut w, clip) = fixture();
        let c = w.edit_gameplay(|edit| source(edit, clip)).unwrap();
        let e = c.resolve(c.value).unwrap();
        for variant in 0..4 {
            assert!(w
                .edit_gameplay(|edit| {
                    edit.audio_control(e, AudioOp::Play)?;
                    edit.set_name(e, "discard")?;
                    match variant {
                        0 => return Err(EditError::Aborted),
                        1 => edit.remove_component(e, ComponentId::AUDIO_SOURCE)?,
                        2 => {
                            let _ = edit.audio_control(e, AudioOp::SetVolume(f32::NAN));
                        }
                        _ => {
                            let _ = edit.audio_control(e, AudioOp::SetPitch(0.0));
                        }
                    }
                    Ok(())
                })
                .is_err());
            w.step();
            assert_eq!(w.audio().voice_count(), 0);
            assert_eq!(w.ecs().get::<Name>(e).unwrap().value.as_str(), "speaker");
        }
    }
    #[test]
    fn retired_source_intents_cancel_or_revalidate_once() {
        let (mut w, clip) = fixture();
        w.edit_gameplay(|edit| {
            let p = source(edit, clip.clone())?;
            edit.audio_control(p, AudioOp::Play)?;
            edit.destroy(p)
        })
        .unwrap();
        w.step();
        assert!(w.diagnostics().is_empty());
        let c = w
            .edit_gameplay(|edit| {
                let p = source(edit, clip)?;
                edit.audio_control(p, AudioOp::Play)?;
                Ok(p)
            })
            .unwrap();
        let e = c.resolve(c.value).unwrap();
        w.edit_gameplay(|edit| edit.remove_component(e, ComponentId::AUDIO_SOURCE))
            .unwrap();
        w.step();
        assert_eq!(w.diagnostics().len(), 1);
        assert_eq!(w.diagnostics()[0].code, DiagnosticCode::MISSING_COMPONENT);
        w.step();
        assert!(w.diagnostics().is_empty());
    }
    #[test]
    fn effects_only_commit_waits_for_tick_and_does_not_replay() {
        let (mut w, clip) = fixture();
        for _ in 0..2 {
            w.edit_gameplay(|edit| edit.play_sound(clip.clone(), PlayParams::default()))
                .unwrap();
        }
        w.begin_frame(0.0);
        assert_eq!(w.audio().voice_count(), 0);
        w.step();
        assert_eq!(w.audio().voice_count(), 2);
        w.step();
        assert_eq!(w.audio().voice_count(), 2);
    }
    #[test]
    fn abort_and_caught_invalid_sound_publish_nothing() {
        let (mut w, clip) = fixture();
        for params in [
            PlayParams {
                volume: f32::NAN,
                ..PlayParams::default()
            },
            PlayParams {
                pitch: 0.0,
                ..PlayParams::default()
            },
            PlayParams {
                looping: true,
                ..PlayParams::default()
            },
        ] {
            assert!(w
                .edit_gameplay(|edit| {
                    edit.spawn("discard", Transform::default(), None)?;
                    edit.play_sound(clip.clone(), PlayParams::default())?;
                    let _ = edit.play_sound(clip.clone(), params);
                    Ok(())
                })
                .is_err());
        }
        let result: Result<EditCommit<()>, _> = w.edit_gameplay(|edit| {
            edit.play_sound(clip.clone(), PlayParams::default())?;
            Err(EditError::Aborted)
        });
        assert!(result.is_err());
        w.step();
        assert_eq!(w.audio().voice_count(), 0);
        assert_eq!(w.ecs().len(), 0);
    }
    #[test]
    fn unavailable_clip_is_reported_at_execution_only_once() {
        let mut w = World::new(WorldConfig::default());
        w.edit_gameplay(|edit| {
            edit.play_sound(
                AssetRef::new(Uuid::from_bytes([1; 16])),
                PlayParams::default(),
            )
        })
        .unwrap();
        assert!(w.diagnostics().is_empty());
        w.step();
        assert_eq!(w.diagnostics().len(), 1);
        w.step();
        assert!(w.diagnostics().is_empty());
    }
}

#[test]
fn disabled_audio_source_controls_reject_stickily() {
    let mut w = World::new(WorldConfig::default());
    w.set_modules(Modules {
        audio: false,
        ..Modules::ALL
    });
    assert!(w
        .edit_gameplay(|edit| {
            let p = edit.spawn("discard", Transform::default(), None)?;
            edit.set_component(p, ComponentValue::AudioSource(AudioSource::default()))?;
            let _ = edit.audio_control(p, AudioOp::Play);
            Ok(())
        })
        .is_err());
    assert_eq!(w.ecs().len(), 0);
}

#[test]
fn disabled_animation_configuration_fails_stickily() {
    let mut w = World::new(WorldConfig::default());
    w.set_modules(Modules {
        animation: false,
        ..Modules::ALL
    });
    assert!(w
        .edit_gameplay(|edit| {
            let p = edit.spawn("discard", Transform::default(), None)?;
            edit.set_component(p, ComponentValue::Animator(Animator::default()))?;
            let _ = edit.animate_control(p, AnimateOp::Pause);
            Ok(())
        })
        .is_err());
    assert_eq!(w.ecs().len(), 0);
}

#[cfg(feature = "animation")]
mod animation_configuration {
    use super::*;
    fn animator(edit: &mut GameplayEdit<'_>) -> Result<PendingEntity, EditError> {
        let p = edit.spawn("animator", Transform::default(), None)?;
        edit.set_component(p, ComponentValue::Animator(Animator::default()))?;
        Ok(p)
    }
    #[test]
    fn configuration_is_logical_immediately_and_never_replays() {
        let mut w = World::new(WorldConfig::default());
        let c = w
            .edit_gameplay(|edit| {
                let p = animator(edit)?;
                edit.animate_control(p, AnimateOp::Pause)?;
                edit.animate_control(p, AnimateOp::SetSpeed(-2.0))?;
                edit.animate_control(p, AnimateOp::SetWeight(0.5))?;
                let Some(GameplayComponentValue::Animator(a)) =
                    edit.component(p, ComponentId::ANIMATOR)?
                else {
                    panic!()
                };
                assert!(!a.playing);
                assert_eq!(a.speed, -2.0);
                assert_eq!(a.weight, 0.5);
                Ok(p)
            })
            .unwrap();
        let e = c.resolve(c.value).unwrap();
        w.edit_gameplay(|edit| {
            edit.animate_control(e, AnimateOp::Resume)?;
            edit.set_component(
                e,
                ComponentValue::Animator(Animator {
                    playing: false,
                    speed: 3.0,
                    weight: 0.75,
                    ..Animator::default()
                }),
            )
        })
        .unwrap();
        w.step();
        let a = w.ecs().copy::<Animator>(e).unwrap();
        assert!(!a.playing);
        assert_eq!(a.speed, 3.0);
        assert_eq!(a.weight, 0.75);
    }
    #[test]
    fn invalid_or_unsupported_animation_operations_rollback_prior_edits() {
        let mut w = World::new(WorldConfig::default());
        for op in [
            AnimateOp::SetSpeed(f32::NAN),
            AnimateOp::SetWeight(2.0),
            AnimateOp::PlayState {
                state: "idle".into(),
                layer: None,
                seconds: 0.0,
                offset: 0.0,
            },
            AnimateOp::SetTime(-0.5),
        ] {
            assert!(w
                .edit_gameplay(|edit| {
                    let p = animator(edit)?;
                    edit.animate_control(p, AnimateOp::Pause)?;
                    let _ = edit.animate_control(p, op);
                    Ok(())
                })
                .is_err());
            assert_eq!(w.ecs().len(), 0);
        }
        assert!(w
            .edit_gameplay(|edit| {
                let p = edit.spawn("no animator", Transform::default(), None)?;
                let _ = edit.animate_control(p, AnimateOp::Resume);
                Ok(())
            })
            .is_err());
        assert_eq!(w.ecs().len(), 0);
    }
    #[test]
    fn explicit_abort_preserves_existing_configuration() {
        let mut w = World::new(WorldConfig::default());
        let c = w.edit_gameplay(animator).unwrap();
        let e = c.resolve(c.value).unwrap();
        let before = w.ecs().copy::<Animator>(e).unwrap();
        let result: Result<EditCommit<()>, _> = w.edit_gameplay(|edit| {
            edit.animate_control(e, AnimateOp::SetSpeed(5.0))?;
            edit.animate_control(e, AnimateOp::Pause)?;
            Err(EditError::Aborted)
        });
        assert!(result.is_err());
        assert_eq!(w.ecs().copy::<Animator>(e).unwrap(), before);
    }
}
