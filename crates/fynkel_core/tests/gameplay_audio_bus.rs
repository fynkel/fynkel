use fynkel_core::*;

#[test]
fn disabled_audio_allows_logical_reads_but_rejects_writes_stickily() {
    let mut world = World::new(WorldConfig::default());
    world.set_modules(Modules {
        audio: false,
        ..Modules::ALL
    });
    world
        .edit_gameplay(|edit| {
            assert_eq!(edit.audio_bus(AudioBus::Master)?, (1.0, false));
            Ok(())
        })
        .unwrap();
    assert!(world
        .edit_gameplay(|edit| {
            edit.spawn("discarded", Transform::IDENTITY, None)?;
            assert!(edit.set_audio_bus(AudioBus::Master, 0.3, true).is_err());
            Ok(())
        })
        .is_err());
    assert_eq!(world.live_entities(), 0);
    assert_eq!(world.audio().bus(AudioBus::Master), 1.0);
    assert!(!world.audio().muted());
}

#[cfg(feature = "audio")]
mod enabled {
    use super::*;
    use glam::Vec3;

    #[test]
    fn bus_gains_and_global_mute_commit_final_values_without_a_tick() {
        let mut world = World::new(WorldConfig::default());
        let tick = world.tick();
        world
            .edit_gameplay(|edit| {
                edit.set_audio_bus(AudioBus::Music, 0.2, true)?;
                assert_eq!(edit.audio_bus(AudioBus::Music)?, (0.2, true));
                assert_eq!(edit.audio_bus(AudioBus::Sfx)?, (1.0, true));
                edit.set_audio_bus(AudioBus::Master, 0.8, false)?;
                edit.set_audio_bus(AudioBus::Music, 0.6, true)?;
                edit.set_audio_bus(AudioBus::Sfx, 0.4, false)?;
                assert_eq!(edit.audio_bus(AudioBus::Music)?, (0.6, false));
                Ok(())
            })
            .unwrap();
        assert_eq!(world.tick(), tick);
        assert_eq!(world.audio().bus(AudioBus::Master), 0.8);
        assert_eq!(world.audio().bus(AudioBus::Music), 0.6);
        assert_eq!(world.audio().bus(AudioBus::Sfx), 0.4);
        assert!(!world.audio().muted());
        world
            .edit_gameplay(|edit| {
                assert_eq!(edit.audio_bus(AudioBus::Music)?, (0.6, false));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn abort_and_caught_invalid_bus_write_roll_back_data_and_mixer_configuration() {
        for volume in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            let mut world = World::new(WorldConfig::default());
            assert!(world
                .edit_gameplay(|edit| {
                    edit.spawn("discarded", Transform::IDENTITY, None)?;
                    edit.set_audio_bus(AudioBus::Music, 0.2, true)?;
                    assert!(edit.set_audio_bus(AudioBus::Sfx, volume, false).is_err());
                    Ok(())
                })
                .is_err());
            assert_eq!(world.live_entities(), 0);
            assert_eq!(world.audio().bus(AudioBus::Music), 1.0);
            assert!(!world.audio().muted());
            world.step();
            assert_eq!(world.audio().bus(AudioBus::Music), 1.0);
            assert!(!world.audio().muted());
        }
        let mut world = World::new(WorldConfig::default());
        assert!(world
            .edit_gameplay(|edit| {
                edit.set_audio_bus(AudioBus::Master, 0.0, true)?;
                Err::<(), _>(EditError::Poisoned)
            })
            .is_err());
        assert_eq!(world.audio().bus(AudioBus::Master), 1.0);
        assert!(!world.audio().muted());
        assert!(!world.is_poisoned());
    }

    #[test]
    fn final_reference_failure_does_not_publish_bus_changes() {
        let mut world = World::new(WorldConfig::default());
        assert!(world
            .edit_gameplay(|edit| {
                let owner = edit.spawn("owner", Transform::IDENTITY, None)?;
                let target = edit.spawn("target", Transform::IDENTITY, None)?;
                edit.set_component(
                    owner,
                    GameplayComponentValue::Follow(
                        Follow::default().map_object_refs(|_| target.into()),
                    ),
                )?;
                edit.set_audio_bus(AudioBus::Master, 0.1, true)?;
                edit.destroy(target)?;
                Ok(())
            })
            .is_err());
        assert_eq!(world.live_entities(), 0);
        assert_eq!(world.audio().bus(AudioBus::Master), 1.0);
        assert!(!world.audio().muted());
    }

    #[test]
    fn audio_bus_and_pose_writes_coalesce_without_replaying_old_values() {
        let mut world = World::new(WorldConfig::default());
        let c = world
            .edit_gameplay(|edit| {
                let root = edit.spawn("root", Transform::IDENTITY, None)?;
                edit.set_audio_bus(AudioBus::Music, 0.2, true)?;
                edit.set_audio_bus(AudioBus::Sfx, 0.3, false)?;
                edit.set_audio_bus(AudioBus::Music, 0.4, true)?;
                edit.set_transform(root, Transform::from_position(Vec3::X * 4.0))?;
                Ok(root)
            })
            .unwrap();
        let root = c.resolve(c.value).unwrap();
        assert_eq!(world.audio().bus(AudioBus::Music), 0.4);
        assert_eq!(world.audio().bus(AudioBus::Sfx), 0.3);
        assert!(world.audio().muted());
        assert_eq!(world.ecs().copy::<Transform>(root).unwrap().position.x, 4.0);
        world
            .edit_gameplay(|edit| edit.set_audio_bus(AudioBus::Music, 0.8, false))
            .unwrap();
        world.step();
        assert_eq!(world.audio().bus(AudioBus::Music), 0.8);
        assert!(!world.audio().muted());
    }

    #[test]
    fn rejected_bus_edit_publishes_nothing_and_leaves_no_deferred_writes() {
        let mut world = World::new(WorldConfig::default());
        assert!(world
            .edit_gameplay(|edit| {
                edit.set_audio_bus(AudioBus::Music, 0.2, true)?;
                edit.set_audio_bus(AudioBus::Sfx, 2.0, false)
            })
            .is_err());
        assert_eq!(world.audio().bus(AudioBus::Music), 1.0);
        assert!(!world.audio().muted());
        world.edit_gameplay(|_| Ok(())).unwrap();
        world.step();
        assert_eq!(world.audio().bus(AudioBus::Music), 1.0);
        assert!(!world.audio().muted());
        assert!(world.diagnostics().is_empty());
    }
}
