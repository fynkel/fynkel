//! The audio phase (M3-11, blueprint §9): every `fynkel.AudioSource` is
//! reconciled with the voice the engine plays for it — an authored or
//! written `playing: true` starts the clip, `playing: false` stops it, a
//! changed `volume`/`pitch`/`looping` reaches the voice, a changed `clip`
//! restarts with the new one, a removed component or a destroyed entity
//! stops the voice — then the engine's clock advances a tick and the
//! non-looping voices that reached their end clear `playing` and raise
//! `PLAYBACK_ENDED`. The lowest entity with a `fynkel.AudioListener` sets
//! the gain over everything. Deterministic: the device only renders what
//! this phase decided (`fynkel_audio`).

use std::sync::Arc;

use fynkel_audio::{
    AudioClip, AudioListener, AudioSource, PlayError, PlayParams, VoiceId, MAX_PITCH, MIN_PITCH,
};
use fynkel_contracts::{AssetRef, ComponentId, DiagnosticCode, Entity, Tick};
use fynkel_ecs::internal::{hecs, Handle};

use super::{Diagnostic, World};
use crate::command::{AudioOp, Lane};
use crate::event::{Event, EventPayload};
use crate::schema::{AudioStateRecord, AudioStatsRecord};

/// Run-state of an `AudioSource`, alongside the component.
#[derive(Clone, Debug, Default)]
pub struct AudioRun {
    /// The voice playing for this source.
    pub voice: Option<VoiceId>,
    /// What the voice was last given, to send only changes.
    pub clip: AssetRef,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    /// `SOUND_UNAVAILABLE` was reported for this clip.
    pub reported: Option<AssetRef>,
}

/// A pending start decided while the ECS was borrowed.
struct Reconcile {
    entity: Entity,
    start: bool,
    stop: Option<VoiceId>,
    set_volume: Option<(VoiceId, f32)>,
    set_pitch: Option<(VoiceId, f32)>,
    set_looping: Option<(VoiceId, bool)>,
}

impl World {
    /// Registers a decoded sound the world's sources may play
    /// (`fynkel_assets::sound` cooks it; the loaders register every clip a
    /// loaded scene's sources name).
    pub fn register_sound(&mut self, clip: AssetRef, data: Arc<AudioClip>) -> Result<(), String> {
        self.audio.register_clip(clip, data)
    }

    pub fn has_sound(&self, clip: &AssetRef) -> bool {
        self.audio.has_clip(clip)
    }

    /// The audio engine: the hosts open the device, suspend it and read
    /// the stats through it.
    pub fn audio(&self) -> &fynkel_audio::Audio {
        &self.audio
    }

    pub fn audio_mut(&mut self) -> &mut fynkel_audio::Audio {
        &mut self.audio
    }

    /// Opens the output device (`fynkel_world_audio_open`); a failure is
    /// an `AUDIO_DEVICE` diagnostic and the world keeps the null backend.
    pub fn open_audio_device(&mut self) -> bool {
        match self.audio.open_device() {
            Ok(_) => true,
            Err(msg) => {
                self.diagnostics.push(Diagnostic {
                    command_index: u32::MAX,
                    lane: Lane::Simulation,
                    code: DiagnosticCode::AUDIO_DEVICE,
                    entity: Entity::NULL,
                    component: ComponentId::NONE,
                    message: format!("audio device: {msg}"),
                });
                false
            }
        }
    }

    /// `Command::Audio` at the tick boundary: edits the source and its
    /// voice; the phase then reconciles the rest.
    pub(super) fn apply_audio(&mut self, index: u32, entity: Entity, op: AudioOp) -> bool {
        let lane = Lane::Simulation;
        if !self.check_entity(index, lane, entity, "audio") {
            return false;
        }
        if !self.ecs.has::<AudioSource>(entity) {
            return self.reject(
                index,
                lane,
                DiagnosticCode::MISSING_COMPONENT,
                entity,
                ComponentId::AUDIO_SOURCE,
                "entity has no fynkel.AudioSource",
            );
        }
        match op {
            AudioOp::SetVolume(v) if !v.is_finite() || !(0.0..=1.0).contains(&v) => {
                return self.reject(
                    index,
                    lane,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::AUDIO_SOURCE,
                    format!("audio: volume must be within [0, 1], got {v}"),
                );
            }
            AudioOp::SetPitch(p) if !p.is_finite() || !(MIN_PITCH..=MAX_PITCH).contains(&p) => {
                return self.reject(
                    index,
                    lane,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::AUDIO_SOURCE,
                    format!("audio: pitch must be within [{MIN_PITCH}, {MAX_PITCH}], got {p}"),
                );
            }
            _ => {}
        }
        if !self.ecs.has::<AudioRun>(entity) {
            let _ = self.ecs.insert(entity, AudioRun::default());
        }
        let Ok(mut source) = self.ecs.get_mut::<AudioSource>(entity) else {
            return false;
        };
        let Ok(mut run) = self.ecs.get_mut::<AudioRun>(entity) else {
            return false;
        };
        match op {
            AudioOp::Play => {
                // Restart from the beginning: the running voice goes, the
                // phase starts a new one this tick.
                if let Some(v) = run.voice.take() {
                    self.audio.stop(v);
                }
                source.playing = true;
            }
            AudioOp::Stop => {
                if let Some(v) = run.voice.take() {
                    self.audio.stop(v);
                }
                source.playing = false;
            }
            AudioOp::Pause => {
                if let Some(v) = run.voice {
                    self.audio.pause(v);
                }
            }
            AudioOp::Resume => {
                if let Some(v) = run.voice {
                    self.audio.resume(v);
                }
            }
            AudioOp::SetVolume(v) => {
                source.volume = v;
                if let Some(id) = run.voice {
                    self.audio.set_volume(id, v);
                    run.volume = v;
                }
            }
            AudioOp::SetPitch(p) => {
                source.pitch = p;
                if let Some(id) = run.voice {
                    self.audio.set_pitch(id, p);
                    run.pitch = p;
                }
            }
        }
        true
    }

    /// `Command::PlaySound` (either lane): a fire-and-forget voice.
    pub(super) fn apply_play_sound(
        &mut self,
        index: u32,
        lane: Lane,
        clip: AssetRef,
        params: PlayParams,
    ) -> bool {
        if !params.volume.is_finite() || !(0.0..=1.0).contains(&params.volume) {
            return self.reject(
                index,
                lane,
                DiagnosticCode::INVALID_PAYLOAD,
                Entity::NULL,
                ComponentId::NONE,
                format!(
                    "playSound: volume must be within [0, 1], got {}",
                    params.volume
                ),
            );
        }
        if !params.pitch.is_finite() || !(MIN_PITCH..=MAX_PITCH).contains(&params.pitch) {
            return self.reject(
                index,
                lane,
                DiagnosticCode::INVALID_PAYLOAD,
                Entity::NULL,
                ComponentId::NONE,
                format!(
                    "playSound: pitch must be within [{MIN_PITCH}, {MAX_PITCH}], got {}",
                    params.pitch
                ),
            );
        }
        match self.audio.play(None, &clip, params) {
            Ok(_) => true,
            // Refused under the voice limit: the sound is simply not
            // heard (the stats count it); the command was valid.
            Err(PlayError::Refused) => true,
            Err(PlayError::UnknownClip) => self.reject(
                index,
                lane,
                DiagnosticCode::SOUND_UNAVAILABLE,
                Entity::NULL,
                ComponentId::NONE,
                format!(
                    "playSound: clip {clip} is not registered with the world (not in the loaded scene's closure?)"
                ),
            ),
        }
    }

    /// The audio phase; returns the number of sources.
    pub(super) fn run_audio(&mut self, tick: Tick) -> u32 {
        let dt = self.config.fixed_dt;
        let mut count = 0;
        let mut work: Vec<Reconcile> = Vec::new();
        let mut listener: Option<(Entity, f32)> = None;

        // Voices whose source is gone (component removed, entity
        // destroyed) stop.
        let mut orphans: Vec<VoiceId> = Vec::new();
        for (voice, source) in self.audio.voice_sources() {
            if !self.ecs.has::<AudioSource>(source) {
                orphans.push(voice);
            }
        }
        for v in orphans {
            self.audio.stop(v);
        }

        {
            let h = self.ecs.hecs();
            for (handle, l) in h
                .query::<(&Handle, &AudioListener)>()
                .without::<&fynkel_scene::Inactive>()
                .iter()
            {
                let better = match listener {
                    None => true,
                    Some((e, _)) => handle.0 < e,
                };
                if better {
                    listener = Some((handle.0, l.volume));
                }
            }
            for (handle, source, run, inactive) in h
                .query::<(
                    &Handle,
                    &mut AudioSource,
                    Option<&mut AudioRun>,
                    hecs::Satisfies<&fynkel_scene::Inactive>,
                )>()
                .iter()
            {
                count += 1;
                let entity = handle.0;
                if inactive {
                    // Not active in the hierarchy: silent. A looping
                    // source wants to play again when it comes back; a
                    // one-shot is over.
                    if let Some(run) = run {
                        if let Some(v) = run.voice.take() {
                            work.push(Reconcile {
                                entity,
                                start: false,
                                stop: Some(v),
                                set_volume: None,
                                set_pitch: None,
                                set_looping: None,
                            });
                            source.playing = source.looping;
                        }
                    }
                    continue;
                }
                let Some(run) = run else {
                    // First sight of the source: its run-state is added
                    // below, then it is reconciled next tick — unless it
                    // wants to play now.
                    work.push(Reconcile {
                        entity,
                        start: source.playing,
                        stop: None,
                        set_volume: None,
                        set_pitch: None,
                        set_looping: None,
                    });
                    continue;
                };
                // A voice the engine stole (or that ended: handled below,
                // but the order within a tick makes both harmless).
                if let Some(v) = run.voice {
                    if !self.audio.is_playing(v) {
                        run.voice = None;
                        source.playing = false;
                    }
                }
                let mut r = Reconcile {
                    entity,
                    start: false,
                    stop: None,
                    set_volume: None,
                    set_pitch: None,
                    set_looping: None,
                };
                match run.voice {
                    Some(v) if !source.playing => {
                        r.stop = Some(v);
                        run.voice = None;
                    }
                    Some(v) if source.clip != run.clip => {
                        // A new clip while playing: restart with it.
                        r.stop = Some(v);
                        run.voice = None;
                        r.start = true;
                    }
                    Some(v) => {
                        if source.volume != run.volume {
                            r.set_volume = Some((v, source.volume));
                            run.volume = source.volume;
                        }
                        if source.pitch != run.pitch {
                            r.set_pitch = Some((v, source.pitch));
                            run.pitch = source.pitch;
                        }
                        if source.looping != run.looping {
                            r.set_looping = Some((v, source.looping));
                            run.looping = source.looping;
                        }
                    }
                    None if source.playing => r.start = true,
                    None => {}
                }
                if r.start
                    || r.stop.is_some()
                    || r.set_volume.is_some()
                    || r.set_pitch.is_some()
                    || r.set_looping.is_some()
                {
                    work.push(r);
                }
            }
        }

        self.audio
            .set_listener_gain(listener.map(|(_, g)| g).unwrap_or(1.0));

        let stolen_before = self.audio.stats().stolen;
        for r in work {
            if let Some(v) = r.stop {
                self.audio.stop(v);
            }
            if let Some((v, g)) = r.set_volume {
                self.audio.set_volume(v, g);
            }
            if let Some((v, p)) = r.set_pitch {
                self.audio.set_pitch(v, p);
            }
            if let Some((v, l)) = r.set_looping {
                self.audio.set_looping(v, l);
            }
            if !self.ecs.has::<AudioRun>(r.entity) {
                let _ = self.ecs.insert(r.entity, AudioRun::default());
            }
            if r.start {
                self.start_source(r.entity);
            }
        }

        // A voice the starts above stole: its source is not playing any
        // more, this tick (a stolen voice raises no event).
        if self.audio.stats().stolen > stolen_before {
            let h = self.ecs.hecs();
            for (source, run) in h.query::<(&mut AudioSource, &mut AudioRun)>().iter() {
                if let Some(v) = run.voice {
                    if !self.audio.is_playing(v) {
                        run.voice = None;
                        source.playing = false;
                    }
                }
            }
        }

        // The clock: a voice that reached its end clears `playing` and
        // raises the event; a stopped or stolen one raised nothing.
        for ended in self.audio.advance(dt) {
            let Some(entity) = ended.source else {
                continue;
            };
            if let Ok(mut source) = self.ecs.get_mut::<AudioSource>(entity) {
                source.playing = false;
            }
            if let Ok(mut run) = self.ecs.get_mut::<AudioRun>(entity) {
                if run.voice == Some(ended.voice) {
                    run.voice = None;
                }
            }
            self.events.push(Event {
                tick,
                payload: EventPayload::PlaybackEnded {
                    entity,
                    clip: ended.clip,
                },
            });
        }
        count
    }

    /// Starts the source's clip for the entity; `SOUND_UNAVAILABLE` once
    /// per clip when the world was never given it (the source stays
    /// `playing` so a later registration starts it).
    fn start_source(&mut self, entity: Entity) {
        let Ok(source) = self.ecs.copy::<AudioSource>(entity) else {
            return;
        };
        let params = PlayParams {
            bus: source.bus,
            volume: source.volume,
            pitch: source.pitch,
            looping: source.looping,
            priority: source.priority,
        };
        match self.audio.play(Some(entity), &source.clip, params) {
            Ok(v) => {
                if let Ok(mut run) = self.ecs.get_mut::<AudioRun>(entity) {
                    run.voice = Some(v);
                    run.clip = source.clip.clone();
                    run.volume = source.volume;
                    run.pitch = source.pitch;
                    run.looping = source.looping;
                    run.reported = None;
                }
            }
            Err(PlayError::Refused) => {
                // Not heard this time; `playing` clears so the game can
                // ask again.
                if let Ok(mut s) = self.ecs.get_mut::<AudioSource>(entity) {
                    s.playing = false;
                }
            }
            Err(PlayError::UnknownClip) => {
                let Ok(mut run) = self.ecs.get_mut::<AudioRun>(entity) else {
                    return;
                };
                if run.reported.as_ref() == Some(&source.clip) {
                    return;
                }
                run.reported = Some(source.clip.clone());
                drop(run);
                self.diagnostics.push(Diagnostic {
                    command_index: u32::MAX,
                    lane: Lane::Simulation,
                    code: DiagnosticCode::SOUND_UNAVAILABLE,
                    entity,
                    component: ComponentId::AUDIO_SOURCE,
                    message: format!(
                        "audio source: clip {} is not registered with the world (not in the loaded scene's closure?)",
                        source.clip
                    ),
                });
            }
        }
    }

    /// `FYNKEL_QUERY_AUDIO`: the source's playback state, `None` without
    /// an `AudioSource`.
    pub fn audio_state(&self, entity: Entity) -> Option<AudioStateRecord> {
        let source = self.ecs.get::<AudioSource>(entity).ok()?;
        let voice = self
            .ecs
            .get::<AudioRun>(entity)
            .ok()
            .and_then(|r| r.voice)
            .and_then(|v| self.audio.state(v));
        let duration = self
            .audio
            .clip(&source.clip)
            .map(|c| c.duration())
            .unwrap_or(0.0);
        Some(match voice {
            Some(v) => AudioStateRecord {
                entity,
                playing: true,
                paused: v.paused,
                time: v.time,
                duration,
                volume: source.volume,
                pitch: source.pitch,
                looping: source.looping,
                clip: source.clip.clone(),
                bus: source.bus,
                voice: v.id.packed(),
            },
            None => AudioStateRecord {
                entity,
                playing: false,
                paused: false,
                time: 0.0,
                duration,
                volume: source.volume,
                pitch: source.pitch,
                looping: source.looping,
                clip: source.clip.clone(),
                bus: source.bus,
                voice: 0,
            },
        })
    }

    /// `FYNKEL_QUERY_AUDIO_STATS`: the engine's counters.
    pub fn audio_stats(&self) -> AudioStatsRecord {
        let s = self.audio.stats();
        let clamp = |v: u64| v.min(u32::MAX as u64) as u32;
        AudioStatsRecord {
            device: s.device,
            sample_rate: s.sample_rate,
            channels: s.channels,
            voices: s.voices,
            max_voices: s.max_voices,
            started: clamp(s.started),
            stolen: clamp(s.stolen),
            refused: clamp(s.refused),
            ended: clamp(s.ended),
            clips: s.clips,
            callbacks: clamp(s.callbacks),
            underruns: clamp(s.underruns),
            suspended: s.suspended,
            streams: s.streams,
            starved: clamp(s.starved),
        }
    }
}
