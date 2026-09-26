//! The engine: the world's side of the audio system. It owns the clip
//! registry, allocates voices with the priority rules (steal the lowest
//! priority at or below the new one, oldest among equals, refuse when
//! every voice outranks it), tracks every voice's position in ticks so a
//! headless run reports the same ends as a device run, and queues the
//! mixer's commands over the ring. Everything here is deterministic: the
//! device only *renders* what the engine decided.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use fynkel_contracts::{AssetRef, Entity};

use crate::backend::Backend;
use crate::clip::AudioClip;
use crate::components::{AudioBus, MAX_PITCH, MIN_PITCH};
use crate::mixer::{
    Command, Mixer, MixerStats, Return, Start, COMMAND_CAPACITY, MAX_CLIPS, MAX_VOICES,
    RETURN_CAPACITY,
};
use crate::stream::{self, StreamShared};
use crate::types::{
    AudioStats, DeviceInfo, Ended, PlayError, PlayParams, VoiceId, VoiceState, NULL_CHANNELS,
    NULL_SAMPLE_RATE,
};

#[derive(Clone, Debug)]
struct Voice {
    generation: u32,
    source: Option<Entity>,
    clip_slot: u16,
    clip: AssetRef,
    bus: AudioBus,
    volume: f32,
    pitch: f32,
    looping: bool,
    priority: i32,
    paused: bool,
    /// Frames into the clip.
    position: f64,
    /// Start order, for the oldest-wins tie-break.
    seq: u64,
    /// The decoder's flags, for a streamed voice.
    stream: Option<Arc<StreamShared>>,
}

#[derive(Clone, Debug)]
struct ClipEntry {
    slot: u16,
    clip: Arc<AudioClip>,
}

pub struct Audio {
    clips: HashMap<AssetRef, ClipEntry>,
    free_clip_slots: Vec<u16>,
    voices: Vec<Option<Voice>>,
    generations: Vec<u32>,
    next_seq: u64,
    bus_gain: [f32; AudioBus::COUNT],
    muted: bool,
    listener: f32,
    suspended: bool,
    commands: rtrb::Producer<Command>,
    returns: rtrb::Consumer<Return>,
    /// Clips the mixer released but the world has not dropped yet: they
    /// are dropped here, on the world's thread, when `advance` runs.
    released: Vec<Arc<AudioClip>>,
    mixer_stats: Arc<MixerStats>,
    backend: Backend,
    started: u64,
    stolen: u64,
    refused: u64,
    ended: u64,
    /// Starvations counted by mixers since replaced (a device opened or
    /// closed); the current mixer's count adds to it.
    starved_before: u64,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio {
    /// An engine on the null backend: the mixer lives here and renders
    /// when a test or the headless host asks (`render`).
    pub fn new() -> Audio {
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
        let (ret_tx, ret_rx) = rtrb::RingBuffer::new(RETURN_CAPACITY);
        let stats = Arc::new(MixerStats::default());
        let mixer = Mixer::new(
            NULL_SAMPLE_RATE,
            NULL_CHANNELS,
            cmd_rx,
            ret_tx,
            stats.clone(),
        );
        Audio {
            clips: HashMap::new(),
            free_clip_slots: (0..MAX_CLIPS as u16).rev().collect(),
            voices: (0..MAX_VOICES).map(|_| None).collect(),
            generations: vec![0; MAX_VOICES],
            next_seq: 0,
            bus_gain: [1.0; AudioBus::COUNT],
            muted: false,
            listener: 1.0,
            suspended: false,
            commands: cmd_tx,
            returns: ret_rx,
            released: Vec::new(),
            mixer_stats: stats,
            backend: Backend::Null(Box::new(mixer)),
            started: 0,
            stolen: 0,
            refused: 0,
            ended: 0,
            starved_before: 0,
        }
    }

    // ---- clips -----------------------------------------------------------

    /// Registers a clip under its asset reference; a second registration
    /// of the same reference replaces the clip (voices playing the old one
    /// stop). `Err` when every clip slot is taken.
    pub fn register_clip(&mut self, r: AssetRef, clip: Arc<AudioClip>) -> Result<(), String> {
        let slot = match self.clips.get(&r) {
            Some(entry) => entry.slot,
            None => self
                .free_clip_slots
                .pop()
                .ok_or_else(|| format!("no clip slot free ({MAX_CLIPS} clips loaded)"))?,
        };
        self.stop_voices_of_slot(slot);
        self.send(Command::LoadClip {
            slot,
            clip: clip.clone(),
        });
        self.clips.insert(r, ClipEntry { slot, clip });
        Ok(())
    }

    /// Forgets a clip; its voices stop.
    pub fn unregister_clip(&mut self, r: &AssetRef) -> bool {
        let Some(entry) = self.clips.remove(r) else {
            return false;
        };
        self.stop_voices_of_slot(entry.slot);
        self.send(Command::UnloadClip { slot: entry.slot });
        self.free_clip_slots.push(entry.slot);
        true
    }

    pub fn has_clip(&self, r: &AssetRef) -> bool {
        self.clips.contains_key(r)
    }

    pub fn clip(&self, r: &AssetRef) -> Option<&Arc<AudioClip>> {
        self.clips.get(r).map(|e| &e.clip)
    }

    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    fn stop_voices_of_slot(&mut self, slot: u16) {
        for i in 0..MAX_VOICES {
            if self.voices[i].as_ref().is_some_and(|v| v.clip_slot == slot) {
                self.forget_voice(i);
            }
        }
    }

    /// Drops the engine's record of a voice; a streamed voice's decoder
    /// is told to exit.
    fn forget_voice(&mut self, i: usize) {
        if let Some(v) = self.voices[i].take() {
            if let Some(shared) = v.stream {
                shared.stop();
            }
        }
    }

    /// Whether `clip` is registered as a stream.
    pub fn is_stream(&self, r: &AssetRef) -> bool {
        self.clips.get(r).is_some_and(|e| e.clip.is_stream())
    }

    /// Starts a decoder for a streamed voice at `position` and sends the
    /// mixer its start with the tap.
    fn start_stream(&mut self, start: Start, clip: &Arc<AudioClip>, shared: Arc<StreamShared>) {
        let Some((source, _)) = &clip.stream else {
            return;
        };
        let tap = stream::spawn(
            source.clone(),
            clip.sample_rate,
            clip.channels,
            start.position.max(0.0) as u64,
            shared,
        );
        self.send(Command::StartStream {
            start,
            tap: Box::new(tap),
        });
    }

    // ---- voices ----------------------------------------------------------

    /// Starts `clip` for `source` (an entity's `AudioSource`, or none for a
    /// fire-and-forget sound). Volume and pitch are clamped to their rules.
    pub fn play(
        &mut self,
        source: Option<Entity>,
        clip: &AssetRef,
        params: PlayParams,
    ) -> Result<VoiceId, PlayError> {
        self.play_at(source, clip, params, 0)
    }

    /// [`play`](Audio::play) from frame `start` of the clip (the
    /// editor's preview, started where its waveform was clicked); a start
    /// past the end starts at the end.
    pub fn play_at(
        &mut self,
        source: Option<Entity>,
        clip: &AssetRef,
        params: PlayParams,
        start: u64,
    ) -> Result<VoiceId, PlayError> {
        let (clip_slot, frames, data) = {
            let entry = self.clips.get(clip).ok_or(PlayError::UnknownClip)?;
            (entry.slot, entry.clip.frames(), entry.clip.clone())
        };
        let slot = match self.allocate(params.priority) {
            Some(slot) => slot,
            None => {
                self.refused += 1;
                return Err(PlayError::Refused);
            }
        };
        let generation = self.generations[slot].wrapping_add(1).max(1);
        self.generations[slot] = generation;
        let volume = params.volume.clamp(0.0, 1.0);
        let pitch = if params.pitch.is_finite() {
            params.pitch.clamp(MIN_PITCH, MAX_PITCH)
        } else {
            1.0
        };
        let seq = self.next_seq;
        self.next_seq += 1;
        let shared = data.is_stream().then(|| StreamShared::new(params.looping));
        self.voices[slot] = Some(Voice {
            generation,
            source,
            clip_slot,
            clip: clip.clone(),
            bus: params.bus,
            volume,
            pitch,
            looping: params.looping,
            priority: params.priority,
            paused: false,
            position: start.min(frames) as f64,
            seq,
            stream: shared.clone(),
        });
        self.started += 1;
        if frames > 0 {
            let start = Start {
                voice: slot as u8,
                generation,
                clip: clip_slot,
                bus: params.bus,
                volume,
                pitch,
                looping: params.looping,
                position: start.min(frames) as f64,
            };
            match shared {
                Some(shared) => self.start_stream(start, &data, shared),
                None => self.send(Command::Start(start)),
            }
        }
        Ok(VoiceId {
            slot: slot as u8,
            generation,
        })
    }

    /// A free slot, else the slot of the voice to steal.
    fn allocate(&mut self, priority: i32) -> Option<usize> {
        if let Some(free) = self.voices.iter().position(|v| v.is_none()) {
            return Some(free);
        }
        let mut victim: Option<(usize, i32, u64)> = None;
        for (i, v) in self.voices.iter().enumerate() {
            let v = v.as_ref().expect("no free slot");
            if v.priority > priority {
                continue;
            }
            let better = match victim {
                None => true,
                Some((_, p, seq)) => v.priority < p || (v.priority == p && v.seq < seq),
            };
            if better {
                victim = Some((i, v.priority, v.seq));
            }
        }
        let (i, _, _) = victim?;
        self.stolen += 1;
        // The mixer replaces the voice on Start; the old generation's
        // end is never reported (the sound was stolen, not ended).
        self.forget_voice(i);
        Some(i)
    }

    fn voice_mut(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices
            .get_mut(id.slot as usize)?
            .as_mut()
            .filter(|v| v.generation == id.generation)
    }

    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.voices
            .get(id.slot as usize)
            .and_then(|v| v.as_ref())
            .is_some_and(|v| v.generation == id.generation)
    }

    /// Stops a voice (a short fade on the device); no `Ended` is reported.
    pub fn stop(&mut self, id: VoiceId) -> bool {
        if self.voice_mut(id).is_none() {
            return false;
        }
        self.forget_voice(id.slot as usize);
        self.send(Command::Stop {
            voice: id.slot,
            generation: id.generation,
        });
        true
    }

    pub fn pause(&mut self, id: VoiceId) -> bool {
        let Some(v) = self.voice_mut(id) else {
            return false;
        };
        v.paused = true;
        self.send(Command::Pause {
            voice: id.slot,
            generation: id.generation,
        });
        true
    }

    pub fn resume(&mut self, id: VoiceId) -> bool {
        let Some(v) = self.voice_mut(id) else {
            return false;
        };
        v.paused = false;
        self.send(Command::Resume {
            voice: id.slot,
            generation: id.generation,
        });
        true
    }

    pub fn set_volume(&mut self, id: VoiceId, volume: f32) -> bool {
        let volume = volume.clamp(0.0, 1.0);
        let Some(v) = self.voice_mut(id) else {
            return false;
        };
        if v.volume != volume {
            v.volume = volume;
            self.send(Command::SetVolume {
                voice: id.slot,
                generation: id.generation,
                volume,
            });
        }
        true
    }

    pub fn set_pitch(&mut self, id: VoiceId, pitch: f32) -> bool {
        let pitch = if pitch.is_finite() {
            pitch.clamp(MIN_PITCH, MAX_PITCH)
        } else {
            1.0
        };
        let Some(v) = self.voice_mut(id) else {
            return false;
        };
        if v.pitch != pitch {
            v.pitch = pitch;
            self.send(Command::SetPitch {
                voice: id.slot,
                generation: id.generation,
                pitch,
            });
        }
        true
    }

    pub fn set_looping(&mut self, id: VoiceId, looping: bool) -> bool {
        let Some(v) = self.voice_mut(id) else {
            return false;
        };
        if v.looping != looping {
            v.looping = looping;
            if let Some(shared) = &v.stream {
                shared.set_looping(looping);
            }
            self.send(Command::SetLooping {
                voice: id.slot,
                generation: id.generation,
                looping,
            });
        }
        true
    }

    /// The voice an entity's source plays, if any.
    pub fn voice_of(&self, source: Entity) -> Option<VoiceId> {
        self.voices.iter().enumerate().find_map(|(i, v)| {
            v.as_ref()
                .filter(|v| v.source == Some(source))
                .map(|v| VoiceId {
                    slot: i as u8,
                    generation: v.generation,
                })
        })
    }

    /// Every voice started for an entity's source, slot order.
    pub fn voice_sources(&self) -> impl Iterator<Item = (VoiceId, Entity)> + '_ {
        self.voices.iter().enumerate().filter_map(|(i, v)| {
            let v = v.as_ref()?;
            Some((
                VoiceId {
                    slot: i as u8,
                    generation: v.generation,
                },
                v.source?,
            ))
        })
    }

    pub fn state(&self, id: VoiceId) -> Option<VoiceState> {
        let v = self
            .voices
            .get(id.slot as usize)?
            .as_ref()
            .filter(|v| v.generation == id.generation)?;
        let (rate, duration) = self
            .clips
            .get(&v.clip)
            .map(|e| (e.clip.sample_rate as f64, e.clip.duration()))
            .unwrap_or((1.0, 0.0));
        Some(VoiceState {
            id,
            source: v.source,
            clip: v.clip.clone(),
            bus: v.bus,
            volume: v.volume,
            pitch: v.pitch,
            looping: v.looping,
            priority: v.priority,
            paused: v.paused,
            time: (v.position / rate) as f32,
            duration,
        })
    }

    /// Every playing voice, slot order.
    pub fn voices(&self) -> Vec<VoiceState> {
        (0..MAX_VOICES)
            .filter_map(|i| {
                let v = self.voices[i].as_ref()?;
                self.state(VoiceId {
                    slot: i as u8,
                    generation: v.generation,
                })
            })
            .collect()
    }

    pub fn voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_some()).count()
    }

    // ---- buses -----------------------------------------------------------

    pub fn set_bus(&mut self, bus: AudioBus, gain: f32) {
        let gain = gain.clamp(0.0, 1.0);
        if self.bus_gain[bus.index()] != gain {
            self.bus_gain[bus.index()] = gain;
            self.send(Command::SetBus { bus, gain });
        }
    }

    pub fn bus(&self, bus: AudioBus) -> f32 {
        self.bus_gain[bus.index()]
    }

    pub fn set_muted(&mut self, muted: bool) {
        if self.muted != muted {
            self.muted = muted;
            self.send(Command::SetMuted(muted));
        }
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn set_listener_gain(&mut self, gain: f32) {
        let gain = gain.clamp(0.0, 1.0);
        if self.listener != gain {
            self.listener = gain;
            self.send(Command::SetListener(gain));
        }
    }

    pub fn listener_gain(&self) -> f32 {
        self.listener
    }

    // ---- stepping --------------------------------------------------------

    /// Advances every voice by `dt` seconds of game time and reports the
    /// non-looping voices that reached their end (their slots are freed;
    /// looping voices wrap). A paused voice holds. Also drains the mixer's
    /// returns so released clips are dropped here.
    pub fn advance(&mut self, dt: f32) -> Vec<Ended> {
        self.drain_returns();
        let mut ended = Vec::new();
        let dt = dt.max(0.0) as f64;
        for i in 0..MAX_VOICES {
            let Some(v) = self.voices[i].as_mut() else {
                continue;
            };
            let Some(entry) = self.clips.get(&v.clip) else {
                self.forget_voice(i);
                continue;
            };
            let frames = entry.clip.frames() as f64;
            if frames <= 0.0 {
                let voice = VoiceId {
                    slot: i as u8,
                    generation: v.generation,
                };
                ended.push(Ended {
                    voice,
                    source: v.source,
                    clip: v.clip.clone(),
                });
                self.forget_voice(i);
                continue;
            }
            if v.paused {
                continue;
            }
            v.position += dt * v.pitch as f64 * entry.clip.sample_rate as f64;
            if v.position >= frames {
                if v.looping {
                    v.position %= frames;
                } else {
                    let voice = VoiceId {
                        slot: i as u8,
                        generation: v.generation,
                    };
                    ended.push(Ended {
                        voice,
                        source: v.source,
                        clip: v.clip.clone(),
                    });
                    self.forget_voice(i);
                }
            }
        }
        self.ended += ended.len() as u64;
        self.released.clear();
        ended
    }

    fn drain_returns(&mut self) {
        while let Ok(r) = self.returns.pop() {
            match r {
                Return::ClipReleased(clip) => self.released.push(clip),
                // The engine's own clock decides when a voice ends; the
                // mixer's report only confirms the slot is free there.
                Return::VoiceEnded { .. } => {}
                // The ring dies here, on this thread; its decoder sees
                // the consumer gone and exits.
                Return::StreamReleased(tap) => drop(tap),
            }
        }
    }

    fn send(&mut self, cmd: Command) {
        if let Err(rtrb::PushError::Full(cmd)) = self.commands.push(cmd) {
            // The ring holds a second of commands at any sane tick rate;
            // with a device the callback drains it, without one `render`
            // does. A full ring means nothing is draining: pump the null
            // mixer ourselves, else drop the command (the engine's own
            // state stays right; only the sound is lost).
            if let Backend::Null(mixer) = &mut self.backend {
                mixer.pump();
                let _ = self.commands.push(cmd);
            } else {
                drop(cmd);
            }
        }
    }

    // ---- backends --------------------------------------------------------

    /// Renders `out` on the null backend (a test, the headless host); a
    /// no-op with a device open. Pumps the queued commands first.
    pub fn render(&mut self, out: &mut [f32]) {
        if let Backend::Null(mixer) = &mut self.backend {
            mixer.render(out);
        }
        self.drain_returns();
        self.released.clear();
    }

    /// Whether a device is open.
    pub fn has_device(&self) -> bool {
        !matches!(self.backend, Backend::Null(_))
    }

    /// The open device's format, or the null backend's.
    pub fn device_info(&self) -> DeviceInfo {
        self.backend.info()
    }

    /// Opens the default output device and moves the mixer into its
    /// callback (`device` feature; `Err` otherwise or when no device can be
    /// opened — the null backend stays). The clips and the mix state are
    /// resent so the new mixer starts where the old one was; playing
    /// voices restart at their current positions.
    pub fn open_device(&mut self) -> Result<DeviceInfo, String> {
        if self.has_device() {
            return Ok(self.backend.info());
        }
        #[cfg(not(feature = "device"))]
        {
            Err("built without the `device` feature".to_string())
        }
        #[cfg(feature = "device")]
        {
            let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
            let (ret_tx, ret_rx) = rtrb::RingBuffer::new(RETURN_CAPACITY);
            let stats = Arc::new(MixerStats::default());
            let device = crate::backend::cpal::open(cmd_rx, ret_tx, stats.clone())?;
            let old_commands = std::mem::replace(&mut self.commands, cmd_tx);
            let old_returns = std::mem::replace(&mut self.returns, ret_rx);
            self.starved_before += self.mixer_stats.starved.load(Ordering::Relaxed);
            self.mixer_stats = stats;
            let old = std::mem::replace(&mut self.backend, Backend::Device(device));
            // The old mixer's tables die with it, here on this thread.
            drop(old);
            drop(old_commands);
            drop(old_returns);
            self.resend_state();
            if self.suspended {
                self.backend.set_suspended(true);
            }
            Ok(self.backend.info())
        }
    }

    /// Closes the device (a no-op without one): the mixer returns to the
    /// null backend with the same clips and voices.
    pub fn close_device(&mut self) {
        if !self.has_device() {
            return;
        }
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
        let (ret_tx, ret_rx) = rtrb::RingBuffer::new(RETURN_CAPACITY);
        let stats = Arc::new(MixerStats::default());
        let mixer = Mixer::new(
            NULL_SAMPLE_RATE,
            NULL_CHANNELS,
            cmd_rx,
            ret_tx,
            stats.clone(),
        );
        let old_commands = std::mem::replace(&mut self.commands, cmd_tx);
        let old_returns = std::mem::replace(&mut self.returns, ret_rx);
        self.starved_before += self.mixer_stats.starved.load(Ordering::Relaxed);
        self.mixer_stats = stats;
        let old = std::mem::replace(&mut self.backend, Backend::Null(Box::new(mixer)));
        drop(old);
        drop(old_commands);
        drop(old_returns);
        self.resend_state();
    }

    /// Everything a fresh mixer needs to continue.
    fn resend_state(&mut self) {
        let clips: Vec<(u16, Arc<AudioClip>)> = self
            .clips
            .values()
            .map(|e| (e.slot, e.clip.clone()))
            .collect();
        for (slot, clip) in clips {
            self.send(Command::LoadClip { slot, clip });
        }
        for bus in [AudioBus::Master, AudioBus::Music, AudioBus::Sfx] {
            let gain = self.bus_gain[bus.index()];
            self.send(Command::SetBus { bus, gain });
        }
        let (muted, listener) = (self.muted, self.listener);
        self.send(Command::SetMuted(muted));
        self.send(Command::SetListener(listener));
        // A streamed voice gets a fresh decoder at its position: the old
        // one's consumer died with the old mixer, so it exits on its own
        // (and is told to, in case it is mid-sleep).
        let starts: Vec<(Start, bool, Option<Arc<AudioClip>>)> = self
            .voices
            .iter_mut()
            .enumerate()
            .filter_map(|(i, v)| {
                let v = v.as_mut()?;
                let stream = v.stream.take().map(|old| {
                    old.stop();
                    let fresh = StreamShared::new(v.looping);
                    v.stream = Some(fresh);
                    self.clips[&v.clip].clip.clone()
                });
                Some((
                    Start {
                        voice: i as u8,
                        generation: v.generation,
                        clip: v.clip_slot,
                        bus: v.bus,
                        volume: v.volume,
                        pitch: v.pitch,
                        looping: v.looping,
                        position: v.position,
                    },
                    v.paused,
                    stream,
                ))
            })
            .collect();
        for (start, paused, stream) in starts {
            let (voice, generation) = (start.voice, start.generation);
            match stream {
                Some(clip) => {
                    let shared = self.voices[voice as usize]
                        .as_ref()
                        .and_then(|v| v.stream.clone())
                        .expect("a streamed voice keeps its flags");
                    self.start_stream(start, &clip, shared);
                }
                None => self.send(Command::Start(start)),
            }
            if paused {
                self.send(Command::Pause { voice, generation });
            }
        }
    }

    /// Pauses the device stream (the host going to the background) or
    /// resumes it. The engine's clock keeps running so game time is not
    /// affected; a suspended world usually stops ticking anyway.
    pub fn suspend(&mut self, suspended: bool) {
        self.suspended = suspended;
        self.backend.set_suspended(suspended);
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    pub fn stats(&self) -> AudioStats {
        let info = self.backend.info();
        AudioStats {
            device: self.has_device(),
            sample_rate: info.sample_rate,
            channels: info.channels as u32,
            voices: self.voice_count() as u32,
            max_voices: MAX_VOICES as u32,
            started: self.started,
            stolen: self.stolen,
            refused: self.refused,
            ended: self.ended,
            clips: self.clips.len() as u32,
            callbacks: self.mixer_stats.callbacks.load(Ordering::Relaxed),
            underruns: self.backend.underruns(),
            suspended: self.suspended,
            streams: self.clips.values().filter(|e| e.clip.is_stream()).count() as u32,
            starved: self.starved_before + self.mixer_stats.starved.load(Ordering::Relaxed),
        }
    }
}
