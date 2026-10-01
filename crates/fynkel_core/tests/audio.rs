//! Audio (blueprint §19.2d item 11, docs/audio.md): a `fynkel.AudioSource`
//! with `playing: true` starts its clip in the audio phase, `playing:
//! false` stops it, a non-looping clip raises `PLAYBACK_ENDED` when the
//! deterministic clock reaches its end, the lowest `AudioListener` sets
//! the gain, an unregistered clip is `SOUND_UNAVAILABLE` once,
//! `PlaySound` works on either lane, `AudioBus` validates, a stolen voice
//! clears `playing`, a destroyed entity's voice stops. All on the null
//! backend.

use std::sync::Arc;

use fynkel_audio::{AudioClip, MAX_VOICES, NULL_SAMPLE_RATE};
use fynkel_contracts::{AssetRef, DiagnosticCode, Uuid, DEFAULT_FIXED_DT};
use fynkel_core::components::ComponentValue;
use fynkel_core::*;
use glam::Vec3;

fn world() -> World {
    World::new(WorldConfig::default())
}

fn clip_ref(n: u8) -> AssetRef {
    AssetRef::new(Uuid::from_bytes([n; 16]))
}

/// A one-second mono clip at the null rate.
fn one_second() -> Arc<AudioClip> {
    AudioClip::new(NULL_SAMPLE_RATE, 1, vec![0.5; NULL_SAMPLE_RATE as usize]).unwrap()
}

fn spawn(w: &mut World, _fixture_id: u64, name: &str) -> Entity {
    let commit = w
        .edit_gameplay(|edit| {
            edit.spawn(name.to_owned(), Transform::from_position(Vec3::ZERO), None)
        })
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    w.drain_events();
    entity
}

fn set(w: &mut World, entity: Entity, value: ComponentValue) {
    w.edit_gameplay(|edit| {
        edit.set_component(entity, value)?;
        Ok(())
    })
    .unwrap();
    let r = w.step();
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
}

fn source(clip: AssetRef, playing: bool, looping: bool) -> AudioSource {
    AudioSource {
        clip,
        bus: AudioBus::Sfx,
        volume: 1.0,
        pitch: 1.0,
        looping,
        playing,
        priority: 0,
    }
}

fn source_of(w: &World, e: Entity) -> AudioSource {
    w.ecs().get::<AudioSource>(e).unwrap().clone()
}

fn ended_events(w: &mut World) -> Vec<(Entity, AssetRef)> {
    w.drain_events()
        .into_iter()
        .filter_map(|e| match e.payload {
            EventPayload::PlaybackEnded { entity, clip } => Some((entity, clip)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_playing_source_starts_and_ends_with_the_event() {
    let mut w = world();
    let clip = clip_ref(1);
    w.register_sound(clip.clone(), one_second()).unwrap();
    let e = spawn(&mut w, 1, "Speaker");
    // The set tick runs the audio phase: the voice starts in that tick.
    set(
        &mut w,
        e,
        ComponentValue::AudioSource(source(clip.clone(), true, false)),
    );
    let state = w.audio_state(e).unwrap();
    assert!(state.playing && !state.paused);
    assert_eq!(state.duration, 1.0);
    assert_eq!(w.audio().voice_count(), 1);
    assert!(ended_events(&mut w).is_empty());

    // One second of ticks minus the one already run: the clip ends on
    // the tick whose clock reaches 1 s.
    let ticks = (1.0 / DEFAULT_FIXED_DT).round() as usize;
    let mut ended_at = None;
    for i in 1..ticks + 5 {
        w.step();
        let ended = ended_events(&mut w);
        if !ended.is_empty() {
            assert_eq!(ended, vec![(e, clip.clone())]);
            ended_at = Some(i);
            break;
        }
    }
    assert_eq!(ended_at, Some(ticks - 1), "ends on the tick reaching 1 s");
    assert!(!source_of(&w, e).playing, "playing clears at the end");
    assert!(!w.audio_state(e).unwrap().playing);
    assert_eq!(w.audio().voice_count(), 0);
    let stats = w.audio_stats();
    assert_eq!((stats.started, stats.ended, stats.clips), (1, 1, 1));
    assert!(!stats.device);
}

#[test]
fn playing_false_stops_and_a_looping_clip_never_ends() {
    let mut w = world();
    let clip = clip_ref(1);
    w.register_sound(clip.clone(), one_second()).unwrap();
    let e = spawn(&mut w, 1, "Loop");
    set(
        &mut w,
        e,
        ComponentValue::AudioSource(source(clip.clone(), true, true)),
    );
    for _ in 0..150 {
        w.step();
    }
    assert!(ended_events(&mut w).is_empty());
    let state = w.audio_state(e).unwrap();
    assert!(state.playing);
    assert!(state.time < 1.0, "wrapped: {}", state.time);

    // Written playing=false: the phase stops the voice, no event.
    set(
        &mut w,
        e,
        ComponentValue::AudioSource(source(clip.clone(), false, true)),
    );
    assert!(!w.audio_state(e).unwrap().playing);
    assert_eq!(w.audio().voice_count(), 0);
    assert!(ended_events(&mut w).is_empty());

    // Command::Audio drives the same voice.
    w.edit_gameplay(|edit| {
        edit.audio_control(e, AudioOp::Play)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert!(w.audio_state(e).unwrap().playing);
    w.edit_gameplay(|edit| {
        edit.audio_control(e, AudioOp::Pause)?;
        Ok(())
    })
    .unwrap();
    w.step();
    let paused = w.audio_state(e).unwrap();
    assert!(paused.playing && paused.paused);
    w.step();
    assert_eq!(
        w.audio_state(e).unwrap().time,
        paused.time,
        "a paused voice holds"
    );
    w.edit_gameplay(|edit| {
        edit.audio_control(e, AudioOp::Resume)?;
        edit.audio_control(e, AudioOp::SetVolume(0.25))?;
        edit.audio_control(e, AudioOp::SetPitch(2.0))?;
        Ok(())
    })
    .unwrap();
    w.step();
    let s = w.audio_state(e).unwrap();
    assert!(!s.paused);
    assert_eq!((s.volume, s.pitch), (0.25, 2.0));
    assert_eq!(
        source_of(&w, e).pitch,
        2.0,
        "the command wrote the component"
    );
    w.edit_gameplay(|edit| {
        edit.audio_control(e, AudioOp::Stop)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert!(!w.audio_state(e).unwrap().playing);
    assert!(!source_of(&w, e).playing);
}

#[test]
fn audio_controls_validate_before_publication() {
    let mut w = world();
    let clip = clip_ref(1);
    w.register_sound(clip.clone(), one_second()).unwrap();
    let e = spawn(&mut w, 1, "Mute");
    assert!(matches!(
        w.edit_gameplay(|edit| edit.audio_control(e, AudioOp::Play)),
        Err(EditError::MissingComponent(..))
    ));
    set(
        &mut w,
        e,
        ComponentValue::AudioSource(source(clip.clone(), false, false)),
    );
    for op in [AudioOp::SetVolume(1.5), AudioOp::SetPitch(0.0)] {
        assert!(w
            .edit_gameplay(|edit| {
                edit.audio_control(e, AudioOp::Play)?;
                edit.audio_control(e, op)
            })
            .is_err());
    }
    assert!(w
        .edit_gameplay(|edit| edit.play_sound(
            clip.clone(),
            PlayParams {
                volume: -1.0,
                ..Default::default()
            }
        ))
        .is_err());
    assert!(w
        .edit_gameplay(|edit| edit.set_audio_bus(AudioBus::Music, 2.0, false))
        .is_err());
    assert_eq!(w.step().rejected, 0);
    assert_eq!(w.audio().voice_count(), 0);
    assert_eq!(source_of(&w, e).volume, 1.0);
    assert!(!source_of(&w, e).playing);
}

#[test]
fn committed_play_sound_and_the_buses() {
    let mut w = world();
    let clip = clip_ref(1);
    w.register_sound(clip.clone(), one_second()).unwrap();
    w.edit_gameplay(|edit| {
        edit.play_sound(
            clip.clone(),
            PlayParams {
                bus: AudioBus::Sfx,
                volume: 0.5,
                pitch: 1.0,
                priority: 0,
                looping: false,
            },
        )?;
        Ok(())
    })
    .unwrap();
    w.edit_gameplay(|edit| {
        edit.play_sound(
            clip.clone(),
            PlayParams {
                bus: AudioBus::Music,
                volume: 1.0,
                pitch: 0.5,
                priority: 1,
                looping: false,
            },
        )?;
        edit.set_audio_bus(AudioBus::Music, 0.3, false)?;
        edit.set_audio_bus(AudioBus::Master, 0.9, true)?;
        Ok(())
    })
    .unwrap();
    let r = w.step();
    assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
    assert_eq!(w.audio().voice_count(), 2);
    assert_eq!(w.audio().bus(AudioBus::Music), 0.3);
    assert_eq!(w.audio().bus(AudioBus::Master), 0.9);
    assert!(w.audio().muted());
    // Fire-and-forget voices have no entity: no event when they end.
    for _ in 0..70 {
        w.step();
    }
    assert!(ended_events(&mut w).is_empty());
    assert_eq!(
        w.audio().voice_count(),
        1,
        "the half-speed one is still going"
    );
    assert_eq!(w.audio_stats().ended, 1);
}

#[test]
fn an_unregistered_clip_is_reported_once() {
    let mut w = world();
    let missing = clip_ref(9);
    let e = spawn(&mut w, 1, "Ghost");
    // Diagnostics are per step: collect them across the ticks.
    let unavailable = |w: &World| -> Vec<(Entity, u32)> {
        w.diagnostics()
            .iter()
            .filter(|d| d.code == DiagnosticCode::SOUND_UNAVAILABLE)
            .map(|d| (d.entity, d.command_index))
            .collect()
    };
    set(
        &mut w,
        e,
        ComponentValue::AudioSource(source(missing.clone(), true, false)),
    );
    let mut seen = unavailable(&w);
    for _ in 0..5 {
        w.step();
        seen.extend(unavailable(&w));
    }
    assert_eq!(seen, vec![(e, u32::MAX)], "once, against the entity");
    assert!(source_of(&w, e).playing, "stays wanting to play");

    // PlaySound: every such command is rejected with the diagnostic.
    let mut rejected = 0;
    for _ in 0..3 {
        w.edit_gameplay(|edit| {
            edit.play_sound(
                missing.clone(),
                PlayParams {
                    bus: AudioBus::Sfx,
                    volume: 1.0,
                    pitch: 1.0,
                    priority: 0,
                    looping: false,
                },
            )?;
            Ok(())
        })
        .unwrap();
        rejected += w.step().rejected;
        seen.extend(unavailable(&w));
    }
    assert_eq!(rejected, 3);
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[1], (Entity::NULL, u32::MAX));

    // A late registration starts the waiting source.
    w.register_sound(missing.clone(), one_second()).unwrap();
    w.step();
    assert!(w.audio_state(e).unwrap().playing);
}

#[test]
fn the_lowest_listener_sets_the_gain() {
    let mut w = world();
    assert_eq!(w.audio().listener_gain(), 1.0);
    let a = spawn(&mut w, 1, "A");
    let b = spawn(&mut w, 2, "B");
    set(
        &mut w,
        b,
        ComponentValue::AudioListener(AudioListener { volume: 0.25 }),
    );
    assert_eq!(w.audio().listener_gain(), 0.25);
    set(
        &mut w,
        a,
        ComponentValue::AudioListener(AudioListener { volume: 0.5 }),
    );
    assert_eq!(w.audio().listener_gain(), 0.5, "the lower entity wins");
    w.edit_gameplay(|edit| {
        edit.remove_component(a, fynkel_contracts::ComponentId::AUDIO_LISTENER)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert_eq!(w.audio().listener_gain(), 0.25);
    w.edit_gameplay(|edit| {
        edit.destroy(b)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert_eq!(w.audio().listener_gain(), 1.0);
}

#[test]
fn a_stolen_voice_clears_playing_and_a_destroyed_entity_stops_its_voice() {
    let mut w = world();
    let clip = clip_ref(1);
    w.register_sound(clip.clone(), one_second()).unwrap();
    // Fill every voice with looping priority-0 sources.
    let mut entities = Vec::new();
    for i in 0..MAX_VOICES as u64 {
        let e = spawn(&mut w, 10 + i, "Voice");
        entities.push(e);
    }
    w.edit_gameplay(|edit| {
        for &e in &entities {
            edit.set_component(
                e,
                ComponentValue::AudioSource(source(clip.clone(), true, true)),
            )?;
        }
        Ok(())
    })
    .unwrap();
    w.step();
    assert_eq!(w.audio().voice_count(), MAX_VOICES);

    // A higher-priority source steals the oldest equal-or-lower voice.
    let thief = spawn(&mut w, 99, "Thief");
    let mut s = source(clip.clone(), true, true);
    s.priority = 5;
    set(&mut w, thief, ComponentValue::AudioSource(s));
    assert_eq!(w.audio().voice_count(), MAX_VOICES);
    assert!(w.audio_state(thief).unwrap().playing);
    let victim = entities[0];
    assert!(!w.audio_state(victim).unwrap().playing);
    assert!(
        !source_of(&w, victim).playing,
        "the stolen source's playing clears"
    );
    assert_eq!(w.audio_stats().stolen, 1);

    // A lower-priority newcomer is refused: playing clears, nothing else moves.
    let meek = spawn(&mut w, 100, "Meek");
    let mut s = source(clip.clone(), true, true);
    s.priority = -1;
    set(&mut w, meek, ComponentValue::AudioSource(s));
    assert!(!source_of(&w, meek).playing);
    assert_eq!(w.audio_stats().refused, 1);

    // Destroying the thief frees its voice on the next phase.
    w.edit_gameplay(|edit| {
        edit.destroy(thief)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert_eq!(w.audio().voice_count(), MAX_VOICES - 1);
    // And removing a source stops its voice too.
    w.edit_gameplay(|edit| {
        edit.remove_component(entities[1], fynkel_contracts::ComponentId::AUDIO_SOURCE)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert_eq!(w.audio().voice_count(), MAX_VOICES - 2);
}

#[test]
fn a_changed_clip_restarts_and_the_scene_load_registers_nothing_by_itself() {
    let mut w = world();
    let a = clip_ref(1);
    let b = clip_ref(2);
    w.register_sound(a.clone(), one_second()).unwrap();
    w.register_sound(b.clone(), one_second()).unwrap();
    let e = spawn(&mut w, 1, "Switch");
    set(
        &mut w,
        e,
        ComponentValue::AudioSource(source(a.clone(), true, true)),
    );
    for _ in 0..10 {
        w.step();
    }
    let before = w.audio_state(e).unwrap();
    assert!(before.time > 0.1);
    set(
        &mut w,
        e,
        ComponentValue::AudioSource(source(b.clone(), true, true)),
    );
    let after = w.audio_state(e).unwrap();
    assert_eq!(after.clip, b);
    assert!(after.time < before.time, "restarted from the beginning");
    assert_eq!(w.audio_stats().started, 2);
    assert!(w.has_sound(&a) && !w.has_sound(&clip_ref(3)));
}

#[test]
fn play_then_pause_in_one_legacy_batch_holds_the_new_voice() {
    let mut w = world();
    let clip = clip_ref(1);
    w.register_sound(clip.clone(), one_second()).unwrap();
    let e = spawn(&mut w, 1, "Speaker");
    set(
        &mut w,
        e,
        ComponentValue::AudioSource(source(clip, false, false)),
    );
    w.edit_gameplay(|edit| {
        edit.audio_control(e, AudioOp::Play)?;
        edit.audio_control(e, AudioOp::Pause)?;
        Ok(())
    })
    .unwrap();
    w.step();
    let state = w.audio_state(e).unwrap();
    assert!(state.playing && state.paused);
    assert_eq!(state.time, 0.0);
    w.edit_gameplay(|edit| {
        edit.audio_control(e, AudioOp::Play)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert!(!w.audio_state(e).unwrap().paused);
    assert!(w.audio_state(e).unwrap().time > 0.0);
}

#[test]
fn native_audio_playing_outputs_validate_as_one_batch_without_rolling_back_mixer_progress() {
    let mut w = world();
    let clip = clip_ref(1);
    w.register_sound(clip.clone(), one_second()).unwrap();
    let a = spawn(&mut w, 1, "a");
    let b = spawn(&mut w, 2, "b");
    set(
        &mut w,
        a,
        ComponentValue::AudioSource(source(clip.clone(), true, false)),
    );
    set(
        &mut w,
        b,
        ComponentValue::AudioSource(source(clip, true, false)),
    );
    assert_eq!(w.audio().voice_count(), 2);
    w.edit_gameplay(|edit| {
        edit.set_active(a, false)?;
        edit.set_active(b, false)
    })
    .unwrap();
    // Model an invalid internal producer bypassing the public validator.
    w.ecs().get_mut::<AudioSource>(b).unwrap().volume = f32::NAN;
    w.step();
    assert!(w
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("audio output")));
    assert!(
        source_of(&w, a).playing,
        "another source cannot partially publish before a rejected output"
    );
    assert!(source_of(&w, b).playing);
    assert_eq!(
        w.audio().voice_count(),
        0,
        "mixer reconciliation is not a rollbackable simulation copy"
    );
}

#[test]
fn native_audio_end_is_immediately_visible_through_the_gameplay_journal() {
    let mut w = world();
    let clip = clip_ref(1);
    w.register_sound(
        clip.clone(),
        AudioClip::new(NULL_SAMPLE_RATE, 1, vec![0.5]).unwrap(),
    )
    .unwrap();
    let owner = w
        .edit_gameplay(|edit| {
            let owner = edit.spawn("short", Transform::default(), None)?;
            edit.set_component(
                owner,
                ComponentValue::AudioSource(source(clip.clone(), true, false)),
            )?;
            Ok(owner)
        })
        .unwrap();
    let owner = owner.resolve(owner.value).unwrap();
    w.step();
    w.edit_gameplay(|edit| {
        match edit.component(owner, ComponentId::AUDIO_SOURCE)? {
            Some(GameplayComponentValue::AudioSource(source)) => assert!(!source.playing),
            _ => panic!("missing source"),
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(ended_events(&mut w), vec![(owner, clip)]);
}
