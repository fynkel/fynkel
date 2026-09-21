//! The mixer: the half of the engine that runs inside the output
//! callback (blueprint §9 "keep blocking allocation/decoding/I/O out of
//! audio callbacks"). It owns fixed tables of clips and voices, pumps a
//! lock-free command ring the world side fills, renders every active voice
//! into the device buffer — linear interpolation at the voice's rate ratio
//! times its pitch, a one-pole gain smoother that also declicks starts,
//! stops and pauses — and hands what it is done with back over a second
//! ring (a released clip's `Arc`, a voice that ended, a stream's tap) so
//! that nothing is freed on the audio thread. A streamed voice reads its
//! frames from a decoder's ring through a `StreamTap` instead of a clip's
//! samples, everything else the same. `tests/allocation.rs` counts
//! allocations across a render to keep it that way.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::clip::AudioClip;
use crate::components::AudioBus;
use crate::stream::{StreamTap, TapRead};

/// Voices the mixer renders at once; the engine steals or refuses beyond.
pub const MAX_VOICES: usize = 32;
/// Clips loaded into the mixer at once.
pub const MAX_CLIPS: usize = 256;
/// Command ring capacity (commands the world may queue between callbacks).
pub const COMMAND_CAPACITY: usize = 1024;
/// Return ring capacity.
pub const RETURN_CAPACITY: usize = 512;
/// Seconds a gain change takes to settle (declick).
pub const GAIN_SMOOTHING_SECONDS: f32 = 0.005;
/// Below this a fading voice counts as silent.
const SILENCE: f32 = 1e-4;

/// A voice start, everything the mixer needs in one record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    pub voice: u8,
    /// Distinguishes reuses of the same voice slot; a command whose
    /// generation is not the slot's current one is ignored.
    pub generation: u32,
    pub clip: u16,
    pub bus: AudioBus,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    /// Frames into the clip to start at.
    pub position: f64,
}

/// World → mixer.
#[derive(Debug)]
pub enum Command {
    LoadClip {
        slot: u16,
        clip: Arc<AudioClip>,
    },
    UnloadClip {
        slot: u16,
    },
    Start(Start),
    /// A streamed voice: the start plus the decoder's tap.
    StartStream {
        start: Start,
        tap: Box<StreamTap>,
    },
    Stop {
        voice: u8,
        generation: u32,
    },
    Pause {
        voice: u8,
        generation: u32,
    },
    Resume {
        voice: u8,
        generation: u32,
    },
    SetVolume {
        voice: u8,
        generation: u32,
        volume: f32,
    },
    SetPitch {
        voice: u8,
        generation: u32,
        pitch: f32,
    },
    SetLooping {
        voice: u8,
        generation: u32,
        looping: bool,
    },
    /// The bus gain (`master` scales the other two).
    SetBus {
        bus: AudioBus,
        gain: f32,
    },
    SetMuted(bool),
    SetListener(f32),
}

/// Mixer → world.
#[derive(Debug)]
pub enum Return {
    /// An unloaded clip's reference, for the world side to drop.
    ClipReleased(Arc<AudioClip>),
    /// The voice reached its clip's end (non-looping) or finished its
    /// stop fade; the slot is free on the mixer side.
    VoiceEnded { voice: u8, generation: u32 },
    /// A streamed voice's tap, done with (its ring is freed on the
    /// world's thread).
    StreamReleased(Box<StreamTap>),
}

/// Counters the callback bumps and the world reads.
#[derive(Debug, Default)]
pub struct MixerStats {
    pub callbacks: AtomicU64,
    pub frames: AtomicU64,
    /// Returns the ring could not take (the clip stays loaded, the voice
    /// end is reported at the next opportunity).
    pub returns_dropped: AtomicU64,
    /// Voices rendered by the last callback.
    pub active_voices: AtomicU64,
    /// Callbacks in which a streamed voice found its ring empty after
    /// its first frames had come (the decoder fell behind).
    pub starved: AtomicU64,
}

#[derive(Debug)]
struct Voice {
    active: bool,
    generation: u32,
    clip: u16,
    bus: AudioBus,
    volume: f32,
    pitch: f32,
    looping: bool,
    position: f64,
    /// The smoothed gain actually applied.
    gain: f32,
    stopping: bool,
    paused: bool,
    /// The decoder's ring, for a streamed voice.
    stream: Option<Box<StreamTap>>,
}

impl Voice {
    const fn idle() -> Voice {
        Voice {
            active: false,
            generation: 0,
            clip: 0,
            bus: AudioBus::Sfx,
            volume: 0.0,
            pitch: 1.0,
            looping: false,
            position: 0.0,
            gain: 0.0,
            stopping: false,
            paused: false,
            stream: None,
        }
    }
}

pub struct Mixer {
    sample_rate: u32,
    channels: usize,
    smoothing: f32,
    clips: Vec<Option<Arc<AudioClip>>>,
    voices: Vec<Voice>,
    bus_gain: [f32; AudioBus::COUNT],
    muted: bool,
    listener: f32,
    commands: rtrb::Consumer<Command>,
    returns: rtrb::Producer<Return>,
    stats: Arc<MixerStats>,
}

impl Mixer {
    /// A mixer for a device of `sample_rate` Hz and `channels` interleaved
    /// output channels (1 or more; the first two carry the sound, the rest
    /// stay silent). Every table is allocated here, once.
    pub fn new(
        sample_rate: u32,
        channels: usize,
        commands: rtrb::Consumer<Command>,
        returns: rtrb::Producer<Return>,
        stats: Arc<MixerStats>,
    ) -> Mixer {
        let rate = sample_rate.max(1) as f32;
        Mixer {
            sample_rate: sample_rate.max(1),
            channels: channels.max(1),
            smoothing: 1.0 - (-1.0 / (GAIN_SMOOTHING_SECONDS * rate)).exp(),
            clips: (0..MAX_CLIPS).map(|_| None).collect(),
            voices: (0..MAX_VOICES).map(|_| Voice::idle()).collect(),
            bus_gain: [1.0; AudioBus::COUNT],
            muted: false,
            listener: 1.0,
            commands,
            returns,
            stats,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    fn push_return(&mut self, r: Return) {
        match self.returns.push(r) {
            Ok(()) => {}
            Err(rtrb::PushError::Full(r)) => {
                self.stats.returns_dropped.fetch_add(1, Ordering::Relaxed);
                // Never free on this thread: a released clip goes back
                // into its slot (the world unloads it again later); a
                // stream's tap is leaked rather than freed here (its
                // decoder exits on its own once the consumer is gone —
                // it is not, so the engine's stop flag ends it; the
                // ring's memory is the price of a full return ring,
                // unreachable in practice since the world drains it
                // every tick).
                match r {
                    Return::ClipReleased(clip) => {
                        if let Some(free) = self.clips.iter().position(|c| c.is_none()) {
                            self.clips[free] = Some(clip);
                        } else {
                            // Every slot full and the ring full: keep the
                            // reference alive by leaking it rather than
                            // freeing here (unreachable in practice: the
                            // world unloads one clip at a time).
                            std::mem::forget(clip);
                        }
                    }
                    Return::StreamReleased(tap) => std::mem::forget(tap),
                    Return::VoiceEnded { .. } => {}
                }
            }
        }
    }

    /// Deactivates a voice, handing its tap back if it streams.
    fn release_voice(&mut self, i: usize) {
        self.voices[i].active = false;
        if let Some(tap) = self.voices[i].stream.take() {
            self.push_return(Return::StreamReleased(tap));
        }
    }

    fn start_voice(&mut self, s: Start, tap: Option<Box<StreamTap>>) {
        let i = s.voice as usize;
        if i >= MAX_VOICES {
            if let Some(tap) = tap {
                self.push_return(Return::StreamReleased(tap));
            }
            return;
        }
        if let Some(old) = self.voices[i].stream.take() {
            self.push_return(Return::StreamReleased(old));
        }
        self.voices[i] = Voice {
            active: true,
            generation: s.generation,
            clip: s.clip,
            bus: s.bus,
            volume: s.volume,
            pitch: s.pitch,
            looping: s.looping,
            position: s.position.max(0.0),
            gain: 0.0,
            stopping: false,
            paused: false,
            stream: tap,
        };
    }

    /// Applies every queued command. Called at the top of `render`; a
    /// host without a device calls it through `render` too.
    pub fn pump(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                Command::LoadClip { slot, clip } => {
                    let slot = slot as usize;
                    if slot < MAX_CLIPS {
                        if let Some(old) = self.clips[slot].take() {
                            self.push_return(Return::ClipReleased(old));
                        }
                        self.clips[slot] = Some(clip);
                    } else {
                        self.push_return(Return::ClipReleased(clip));
                    }
                }
                Command::UnloadClip { slot } => {
                    let slot = slot as usize;
                    if slot < MAX_CLIPS {
                        for i in 0..MAX_VOICES {
                            if self.voices[i].active && self.voices[i].clip as usize == slot {
                                self.release_voice(i);
                            }
                        }
                        if let Some(old) = self.clips[slot].take() {
                            self.push_return(Return::ClipReleased(old));
                        }
                    }
                }
                Command::Start(s) => self.start_voice(s, None),
                Command::StartStream { start, tap } => self.start_voice(start, Some(tap)),
                Command::Stop { voice, generation } => {
                    if let Some(v) = self.voice_mut(voice, generation) {
                        v.stopping = true;
                    }
                }
                Command::Pause { voice, generation } => {
                    if let Some(v) = self.voice_mut(voice, generation) {
                        v.paused = true;
                    }
                }
                Command::Resume { voice, generation } => {
                    if let Some(v) = self.voice_mut(voice, generation) {
                        v.paused = false;
                    }
                }
                Command::SetVolume {
                    voice,
                    generation,
                    volume,
                } => {
                    if let Some(v) = self.voice_mut(voice, generation) {
                        v.volume = volume;
                    }
                }
                Command::SetPitch {
                    voice,
                    generation,
                    pitch,
                } => {
                    if let Some(v) = self.voice_mut(voice, generation) {
                        v.pitch = pitch;
                    }
                }
                Command::SetLooping {
                    voice,
                    generation,
                    looping,
                } => {
                    if let Some(v) = self.voice_mut(voice, generation) {
                        v.looping = looping;
                    }
                }
                Command::SetBus { bus, gain } => self.bus_gain[bus.index()] = gain,
                Command::SetMuted(m) => self.muted = m,
                Command::SetListener(g) => self.listener = g,
            }
        }
    }

    fn voice_mut(&mut self, voice: u8, generation: u32) -> Option<&mut Voice> {
        let v = self.voices.get_mut(voice as usize)?;
        (v.active && v.generation == generation).then_some(v)
    }

    /// Renders one buffer of interleaved output (`out.len()` a multiple of
    /// the channel count; a remainder is left silent). Allocation-free.
    pub fn render(&mut self, out: &mut [f32]) {
        self.pump();
        out.fill(0.0);
        let channels = self.channels;
        let frames = out.len() / channels;
        let master = self.bus_gain[AudioBus::Master.index()];
        let global = if self.muted {
            0.0
        } else {
            self.listener * master
        };
        let k = self.smoothing;
        let mut active = 0u64;
        let mut starved = 0u64;
        for vi in 0..MAX_VOICES {
            let v = &mut self.voices[vi];
            if !v.active {
                continue;
            }
            let Some(clip) = self.clips[v.clip as usize].as_ref() else {
                self.release_voice(vi);
                let generation = self.voices[vi].generation;
                self.push_return(Return::VoiceEnded {
                    voice: vi as u8,
                    generation,
                });
                continue;
            };
            if v.paused && !v.stopping && v.gain < SILENCE {
                continue; // holding, already silent
            }
            active += 1;
            let bus = if v.bus == AudioBus::Master {
                1.0
            } else {
                self.bus_gain[v.bus.index()]
            };
            let target = if v.stopping || v.paused {
                0.0
            } else {
                v.volume * bus * global
            };
            let step = clip.sample_rate as f64 / self.sample_rate as f64 * v.pitch as f64;
            let mut gain = v.gain;
            let mut ended = false;
            let advance = !v.paused;
            if let Some(tap) = v.stream.as_mut() {
                // A streamed voice: frames from the decoder's ring. The
                // engine's clock tracks the position; here only the
                // samples matter.
                for f in 0..frames {
                    gain += (target - gain) * k;
                    let (read, (l, r)) = tap.read(step, advance);
                    match read {
                        TapRead::Frame => {
                            let o = f * channels;
                            if channels == 1 {
                                out[o] += (l + r) * 0.5 * gain;
                            } else {
                                out[o] += l * gain;
                                out[o + 1] += r * gain;
                            }
                        }
                        TapRead::Waiting => {}
                        TapRead::Starved => break,
                        TapRead::Ended => {
                            ended = true;
                            break;
                        }
                    }
                }
                if tap.take_starved() {
                    starved += 1;
                }
            } else {
                let clip_frames = clip.frames() as usize;
                let mut position = v.position;
                for f in 0..frames {
                    gain += (target - gain) * k;
                    if clip_frames == 0 {
                        ended = true;
                        break;
                    }
                    let i = position as usize;
                    let (i, frac) = if i >= clip_frames {
                        if v.looping {
                            position %= clip_frames as f64;
                            let i = position as usize;
                            (i, (position - i as f64) as f32)
                        } else {
                            ended = true;
                            break;
                        }
                    } else {
                        (i, (position - i as f64) as f32)
                    };
                    let (l0, r0) = clip.frame(i);
                    let next = if i + 1 < clip_frames {
                        i + 1
                    } else if v.looping {
                        0
                    } else {
                        i
                    };
                    let (l1, r1) = clip.frame(next);
                    let l = (l0 + (l1 - l0) * frac) * gain;
                    let r = (r0 + (r1 - r0) * frac) * gain;
                    let o = f * channels;
                    if channels == 1 {
                        out[o] += (l + r) * 0.5;
                    } else {
                        out[o] += l;
                        out[o + 1] += r;
                    }
                    if advance {
                        position += step;
                    }
                }
                v.position = position;
            }
            v.gain = gain;
            if ended || (v.stopping && gain < SILENCE) {
                let generation = v.generation;
                self.release_voice(vi);
                self.push_return(Return::VoiceEnded {
                    voice: vi as u8,
                    generation,
                });
            }
        }
        if starved > 0 {
            self.stats.starved.fetch_add(starved, Ordering::Relaxed);
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
        self.stats.callbacks.fetch_add(1, Ordering::Relaxed);
        self.stats
            .frames
            .fetch_add(frames as u64, Ordering::Relaxed);
        self.stats.active_voices.store(active, Ordering::Relaxed);
    }
}
