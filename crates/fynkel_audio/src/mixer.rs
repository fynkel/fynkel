//! The mixer: the half that runs on the audio thread (docs/stage-11.md
//! §3.1). It holds a fixed table of voices, takes the game's commands from
//! a lock-free ring, adds every voice into the output buffer, and sends
//! what it's done with back on a second ring, so nothing is allocated,
//! freed or waited for here. `tests/allocation.rs` counts it.
//!
//! The game's side ([`crate::Audio`]) decides everything: which slot a
//! voice takes, which one is stolen, and when. The mixer only plays.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::clip::Clip;

/// How many sounds play at once, as Unity's 32 real voices.
pub const MAX_VOICES: usize = 32;

/// How many commands the game may send between two callbacks.
pub(crate) const COMMANDS: usize = 1024;

/// Room for every voice's return: at most one for each command since the
/// game last read them, and one for each voice that ends on its own. So
/// the ring is never full while the game reads it once a frame.
pub(crate) const RETURNS: usize = COMMANDS + MAX_VOICES;

/// How long a start, stop or pause ramps the gain, so none clicks.
const RAMP_SECONDS: f32 = 0.005;

/// A gain below this is silence: a stopping voice ends there.
const SILENT: f32 = 1e-4;

/// How a voice plays: its own volume (0 to 1), its pitch (1 is the clip's
/// own speed), and whether it loops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { volume: 1.0, pitch: 1.0, looping: false }
    }
}

/// The game's side to the mixer.
#[derive(Debug)]
pub(crate) enum Command {
    Start {
        slot: u8,
        generation: u32,
        clip: Arc<Clip>,
        settings: Settings,
        pausable: bool,
        from: f64,
    },
    Stop {
        slot: u8,
        generation: u32,
    },
    Pause {
        slot: u8,
        generation: u32,
        paused: bool,
    },
    Set {
        slot: u8,
        generation: u32,
        settings: Settings,
    },
    /// Whether the game is paused: voices that follow it fall silent and
    /// hold.
    GamePaused(bool),
    /// Every voice's gain (Unity's `AudioListener.volume`).
    Volume(f32),
}

/// A voice the mixer is done with, and its clip, to be dropped off the
/// audio thread.
#[derive(Debug)]
pub(crate) struct Ended {
    pub slot: u8,
    pub generation: u32,
    pub clip: Arc<Clip>,
    /// It reached the end of its clip, rather than being stopped or
    /// replaced.
    pub finished: bool,
}

/// What the audio thread tells the game without the rings: each voice's
/// place in its clip, and counts.
#[derive(Debug)]
pub(crate) struct Shared {
    pub voices: [Place; MAX_VOICES],
    pub callbacks: AtomicU64,
    pub frames: AtomicU64,
    pub playing: AtomicU32,
    /// Returns the ring couldn't take. Never, while the game reads them
    /// each frame ([`RETURNS`]).
    pub lost_returns: AtomicU64,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            voices: std::array::from_fn(|_| Place::default()),
            callbacks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            playing: AtomicU32::new(0),
            lost_returns: AtomicU64::new(0),
        }
    }
}

/// A voice's place in its clip, in the clip's frames, written after each
/// callback, with the generation it's for.
#[derive(Debug, Default)]
pub(crate) struct Place {
    generation: AtomicU32,
    frame: AtomicU64,
}

impl Place {
    /// The place of the voice of `generation`, if this is still its.
    pub(crate) fn read(&self, generation: u32) -> Option<f64> {
        // The generation is written last and read first and last, so a
        // place read between is the voice's.
        if self.generation.load(Ordering::Acquire) != generation {
            return None;
        }
        let frame = f64::from_bits(self.frame.load(Ordering::Acquire));
        (self.generation.load(Ordering::Acquire) == generation).then_some(frame)
    }

    fn write(&self, generation: u32, frame: f64) {
        self.generation.store(0, Ordering::Release);
        self.frame.store(frame.to_bits(), Ordering::Release);
        self.generation.store(generation, Ordering::Release);
    }
}

#[derive(Debug)]
struct Voice {
    clip: Option<Arc<Clip>>,
    generation: u32,
    settings: Settings,
    pausable: bool,
    paused: bool,
    stopping: bool,
    /// In the clip's frames.
    place: f64,
    /// The gain applied, ramping to the target.
    gain: f32,
}

impl Voice {
    const IDLE: Self = Self {
        clip: None,
        generation: 0,
        settings: Settings { volume: 1.0, pitch: 1.0, looping: false },
        pausable: false,
        paused: false,
        stopping: false,
        place: 0.0,
        gain: 0.0,
    };
}

/// The audio thread's half. Made with every table it'll need.
#[derive(Debug)]
pub struct Mixer {
    rate: u32,
    channels: usize,
    ramp: f32,
    voices: [Voice; MAX_VOICES],
    game_paused: bool,
    volume: f32,
    commands: rtrb::Consumer<Command>,
    returns: rtrb::Producer<Ended>,
    /// Ends the return ring couldn't take, kept to send next time.
    held: [Option<Ended>; MAX_VOICES],
    shared: Arc<Shared>,
}

impl Mixer {
    pub(crate) fn new(
        rate: u32,
        channels: usize,
        commands: rtrb::Consumer<Command>,
        returns: rtrb::Producer<Ended>,
        shared: Arc<Shared>,
    ) -> Self {
        let mut mixer = Self {
            rate: 1,
            channels: 1,
            ramp: 1.0,
            voices: [Voice::IDLE; MAX_VOICES],
            game_paused: false,
            volume: 1.0,
            commands,
            returns,
            held: std::array::from_fn(|_| None),
            shared,
        };
        mixer.set_output(rate, channels);
        mixer
    }

    /// The output's rate and channels: those of a new device.
    pub(crate) fn set_output(&mut self, rate: u32, channels: usize) {
        self.rate = rate.max(1);
        self.channels = channels.max(1);
        // A one-pole ramp that's within 1% of its target after RAMP_SECONDS.
        self.ramp = 1.0 - (0.01f32.ln() / (RAMP_SECONDS * self.rate as f32)).exp();
    }

    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    fn give_back(&mut self, ended: Ended) {
        let slot = usize::from(ended.slot);
        if let Err(rtrb::PushError::Full(ended)) = self.returns.push(ended) {
            // Kept, never dropped here; sent with the next callback's.
            self.shared.lost_returns.fetch_add(1, Ordering::Relaxed);
            if let Some(old) = self.held[slot].replace(ended) {
                // A second for the same slot: can't happen while the game
                // reads (RETURNS). Leaked rather than freed on this thread.
                std::mem::forget(old);
            }
        }
    }

    fn end(&mut self, slot: usize, finished: bool) {
        let voice = &mut self.voices[slot];
        if let Some(clip) = voice.clip.take() {
            let generation = voice.generation;
            self.give_back(Ended { slot: slot as u8, generation, clip, finished });
        }
    }

    fn voice(&mut self, slot: u8, generation: u32) -> Option<&mut Voice> {
        let voice = self.voices.get_mut(usize::from(slot))?;
        (voice.clip.is_some() && voice.generation == generation).then_some(voice)
    }

    /// Applies the game's commands.
    fn take_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            match command {
                Command::Start { slot, generation, clip, settings, pausable, from } => {
                    let i = usize::from(slot);
                    if i >= MAX_VOICES {
                        self.give_back(Ended { slot: 0, generation, clip, finished: false });
                        continue;
                    }
                    self.end(i, false);
                    self.voices[i] = Voice {
                        clip: Some(clip),
                        generation,
                        settings,
                        pausable,
                        paused: false,
                        stopping: false,
                        place: from.max(0.0),
                        gain: 0.0,
                    };
                }
                Command::Stop { slot, generation } => {
                    if let Some(voice) = self.voice(slot, generation) {
                        voice.stopping = true;
                    }
                }
                Command::Pause { slot, generation, paused } => {
                    if let Some(voice) = self.voice(slot, generation) {
                        voice.paused = paused;
                    }
                }
                Command::Set { slot, generation, settings } => {
                    if let Some(voice) = self.voice(slot, generation) {
                        voice.settings = settings;
                    }
                }
                Command::GamePaused(paused) => self.game_paused = paused,
                Command::Volume(volume) => self.volume = volume,
            }
        }
    }

    /// Fills `out`, interleaved in the output's channels, with every voice
    /// added up. Allocates nothing.
    pub fn render(&mut self, out: &mut [f32]) {
        for slot in 0..MAX_VOICES {
            if let Some(ended) = self.held[slot].take() {
                self.give_back(ended);
            }
        }
        self.take_commands();
        out.fill(0.0);
        let channels = self.channels;
        let frames = out.len() / channels;
        let ramp = self.ramp;
        let mut playing = 0;
        for slot in 0..MAX_VOICES {
            let voice = &mut self.voices[slot];
            let Some(clip) = voice.clip.as_ref() else { continue };
            let held = voice.paused || (voice.pausable && self.game_paused);
            let target = if voice.stopping || held { 0.0 } else { voice.settings.volume * self.volume };
            if held && !voice.stopping && voice.gain < SILENT {
                voice.gain = 0.0;
                playing += 1;
                continue;
            }
            let step = f64::from(clip.rate()) / f64::from(self.rate) * f64::from(voice.settings.pitch);
            let length = clip.frames();
            let looping = voice.settings.looping;
            let mut place = voice.place;
            let mut gain = voice.gain;
            let mut finished = length == 0;
            for f in 0..frames {
                if finished {
                    break;
                }
                gain += (target - gain) * ramp;
                let at = place as usize;
                let fraction = (place - at as f64) as f32;
                let (l0, r0) = clip.frame(at);
                let next = if at + 1 < length {
                    at + 1
                } else if looping {
                    0
                } else {
                    at
                };
                let (l1, r1) = clip.frame(next);
                let l = (l0 + (l1 - l0) * fraction) * gain;
                let r = (r0 + (r1 - r0) * fraction) * gain;
                let o = f * channels;
                if channels == 1 {
                    out[o] += (l + r) * 0.5;
                } else {
                    out[o] += l;
                    out[o + 1] += r;
                }
                // A held voice ramping down stays where it is.
                if !held {
                    place += step;
                    if place >= length as f64 {
                        if looping {
                            place %= length as f64;
                        } else {
                            finished = true;
                        }
                    }
                }
            }
            voice.place = place;
            voice.gain = gain;
            let (generation, stopping) = (voice.generation, voice.stopping);
            self.shared.voices[slot].write(generation, place);
            if finished {
                self.end(slot, true);
            } else if stopping && gain < SILENT {
                self.end(slot, false);
            } else {
                playing += 1;
            }
        }
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
        self.shared.playing.store(playing, Ordering::Relaxed);
        self.shared.callbacks.fetch_add(1, Ordering::Relaxed);
        self.shared.frames.fetch_add(frames as u64, Ordering::Relaxed);
    }
}
