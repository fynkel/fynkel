//! The mixer: the half that runs on the audio thread (docs/stage-11.md
//! §3.1). It holds a fixed table of voices, takes the game's commands from
//! a lock-free ring, adds every voice into the output buffer, and sends
//! what it's done with back on a second ring, so nothing is allocated,
//! freed or waited for here. `tests/allocation.rs` counts it.
//!
//! The game's side ([`crate::Audio`]) decides everything: which slot a
//! voice takes, which one is stolen, and when. The mixer only plays: each
//! voice at its volume, its fade's and its group's, under the master
//! volume; a streamed clip from the ring its decoder fills.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::clip::Clip;
use crate::spatial::Mix;
use crate::stream::Tap;

/// How many sounds play at once, as Unity's 32 real voices.
pub const MAX_VOICES: usize = 32;

/// How many groups a voice may be in ([`Settings::group`]): group 0 is
/// none, under the master volume alone.
pub const MAX_GROUPS: usize = 16;

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

/// Where a sound right behind ([`Mix::dull`] 1) is cut: above it, the
/// highs fall away, so behind doesn't sound like in front.
const DULL_HERTZ: f32 = 5_000.0;

/// How a voice plays: its own volume (0 to 1), its pitch (1 is the clip's
/// own speed), whether it loops, and the group whose volume it's under (0
/// for none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub group: u8,
}

impl Settings {
    pub const DEFAULT: Self = Self { volume: 1.0, pitch: 1.0, looping: false, group: 0 };
}

impl Default for Settings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A gain that slides in a straight line to where it's sent, over the
/// seconds it's given: a fade. Moved once a callback ([`Span`]).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fader {
    gain: f32,
    target: f32,
    /// How far it moves a frame; infinite to get there over one callback.
    step: f32,
}

impl Fader {
    const FULL: Self = Self { gain: 1.0, target: 1.0, step: 0.0 };

    fn to(&mut self, target: f32, frames: f32) {
        self.target = target;
        self.step = if frames >= 1.0 { (target - self.gain).abs() / frames } else { f32::INFINITY };
    }

    /// Moves it on by a callback of `frames`: how it goes, frame by frame.
    fn advance(&mut self, frames: usize) -> Span {
        let (from, target) = (self.gain, self.target);
        if from == target {
            return Span::still(from);
        }
        let frames = frames.max(1) as f32;
        // Frames until it's there: as many as it takes, or this callback.
        let reach = if self.step.is_infinite() { frames } else { (target - from).abs() / self.step };
        if reach <= frames {
            self.gain = target;
            Span { from, to: target, per_frame: 1.0 / reach.max(1.0) }
        } else {
            self.gain = from + (target - from) * frames / reach;
            Span { from, to: self.gain, per_frame: 1.0 / frames }
        }
    }
}

/// A fader's way across one callback: from `from`, a line to `to`, there
/// after `1 / per_frame` frames.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Span {
    from: f32,
    to: f32,
    per_frame: f32,
}

impl Span {
    const fn still(gain: f32) -> Self {
        Self { from: gain, to: gain, per_frame: 1.0 }
    }

    /// Its gain at frame `f` of the callback.
    #[inline]
    fn at(&self, f: usize) -> f32 {
        let t = ((f + 1) as f32 * self.per_frame).min(1.0);
        self.from + (self.to - self.from) * t
    }
}

/// The game's side to the mixer.
#[derive(Debug)]
pub(crate) enum Command {
    Start {
        slot: u8,
        generation: u32,
        clip: Arc<Clip>,
        /// A streamed clip's ring.
        stream: Option<Tap>,
        settings: Settings,
        pausable: bool,
        from: f64,
        mix: Mix,
        /// Seconds to fade in over; 0 for none.
        fade_in: f32,
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
    /// Where a voice is heard: ramped to over the next callback, so one
    /// that moves doesn't click.
    Mix {
        slot: u8,
        generation: u32,
        mix: Mix,
    },
    /// A fade of one voice to `to`, over `seconds`, after which it stops
    /// when `stop`.
    Fade {
        slot: u8,
        generation: u32,
        to: f32,
        seconds: f32,
        stop: bool,
    },
    /// A group's volume, faded to over `seconds` (or over one callback).
    Group {
        group: u8,
        to: f32,
        seconds: f32,
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
    pub stream: Option<Tap>,
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
    /// Callbacks in which a streamed voice ran out of decoded sound.
    pub starved: AtomicU64,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            voices: std::array::from_fn(|_| Place::default()),
            callbacks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            playing: AtomicU32::new(0),
            lost_returns: AtomicU64::new(0),
            starved: AtomicU64::new(0),
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
    stream: Option<Tap>,
    generation: u32,
    settings: Settings,
    pausable: bool,
    paused: bool,
    stopping: bool,
    /// In the clip's frames: a streamed one's from its start, counting
    /// every loop.
    place: f64,
    /// A streamed voice's two frames either side of its place, left and
    /// right, the first of them frame `window_at`; how many of them are
    /// read yet (0 to 2); and whether the second is the first again, past
    /// the clip's end, as a decoded clip's last frame is.
    window: [f32; 4],
    window_at: u64,
    have: u8,
    drained: bool,
    /// The gain applied, ramping to the target.
    gain: f32,
    /// Its fade, and whether it stops once that's at 0.
    fade: Fader,
    fade_stop: bool,
    /// The mix applied ([`Mix::gains`], then [`Mix::dull`]), and the one
    /// to ramp to over the next callback.
    mix: [f32; 5],
    target: [f32; 5],
    /// The high cut's state, left and right.
    low: [f32; 2],
}

impl Voice {
    const IDLE: Self = Self {
        clip: None,
        stream: None,
        generation: 0,
        settings: Settings::DEFAULT,
        pausable: false,
        paused: false,
        stopping: false,
        place: 0.0,
        window: [0.0; 4],
        window_at: 0,
        have: 0,
        drained: false,
        gain: 0.0,
        fade: Fader::FULL,
        fade_stop: false,
        mix: STRAIGHT,
        target: STRAIGHT,
        low: [0.0; 2],
    };
}

const STRAIGHT: [f32; 5] = flat(Mix::STRAIGHT);

const fn flat(mix: Mix) -> [f32; 5] {
    let g = mix.gains;
    [g[0], g[1], g[2], g[3], mix.dull]
}

/// The audio thread's half. Made with every table it'll need.
#[derive(Debug)]
pub struct Mixer {
    rate: u32,
    channels: usize,
    ramp: f32,
    /// The high cut's coefficient for a sound right behind.
    dull: f32,
    voices: [Voice; MAX_VOICES],
    groups: [Fader; MAX_GROUPS],
    /// Each group's way across this callback.
    spans: [Span; MAX_GROUPS],
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
            dull: 1.0,
            voices: [Voice::IDLE; MAX_VOICES],
            groups: [Fader::FULL; MAX_GROUPS],
            spans: [Span::still(1.0); MAX_GROUPS],
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
        self.dull = 1.0 - (-std::f32::consts::TAU * DULL_HERTZ / self.rate as f32).exp();
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
            let (generation, stream) = (voice.generation, voice.stream.take());
            self.give_back(Ended { slot: slot as u8, generation, clip, stream, finished });
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
                Command::Start { slot, generation, clip, stream, settings, pausable, from, mix, fade_in } => {
                    let i = usize::from(slot);
                    if i >= MAX_VOICES {
                        self.give_back(Ended { slot: 0, generation, clip, stream, finished: false });
                        continue;
                    }
                    self.end(i, false);
                    let place = from.max(0.0);
                    let mut fade = Fader::FULL;
                    if fade_in > 0.0 {
                        fade.gain = 0.0;
                        fade.to(1.0, fade_in * self.rate as f32);
                    }
                    self.voices[i] = Voice {
                        clip: Some(clip),
                        stream,
                        generation,
                        settings,
                        pausable,
                        paused: false,
                        stopping: false,
                        place,
                        window: [0.0; 4],
                        window_at: place as u64,
                        have: 0,
                        drained: false,
                        gain: 0.0,
                        fade,
                        fade_stop: false,
                        mix: flat(mix),
                        target: flat(mix),
                        low: [0.0; 2],
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
                        if let Some(tap) = &voice.stream {
                            tap.state.looping.store(settings.looping, Ordering::Relaxed);
                        }
                    }
                }
                Command::Fade { slot, generation, to, seconds, stop } => {
                    let rate = self.rate as f32;
                    if let Some(voice) = self.voice(slot, generation) {
                        voice.fade.to(to, seconds * rate);
                        voice.fade_stop = stop;
                    }
                }
                Command::Group { group, to, seconds } => {
                    if let Some(fader) = self.groups.get_mut(usize::from(group)) {
                        fader.to(to, seconds * self.rate as f32);
                    }
                }
                Command::Mix { slot, generation, mix } => {
                    if let Some(voice) = self.voice(slot, generation) {
                        voice.target = flat(mix);
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
        for (span, group) in self.spans.iter_mut().zip(self.groups.iter_mut()) {
            *span = group.advance(frames);
        }
        let ramp = self.ramp;
        let mut playing = 0;
        let mut starved = false;
        for slot in 0..MAX_VOICES {
            let voice = &mut self.voices[slot];
            let Some(clip) = voice.clip.as_ref() else { continue };
            let held = voice.paused || (voice.pausable && self.game_paused);
            let target = if voice.stopping || held { 0.0 } else { voice.settings.volume * self.volume };
            if held && !voice.stopping && voice.gain < SILENT {
                voice.gain = 0.0;
                voice.mix = voice.target;
                playing += 1;
                continue;
            }
            let step = f64::from(clip.rate()) / f64::from(self.rate) * f64::from(voice.settings.pitch);
            let length = clip.frames();
            let looping = voice.settings.looping;
            let mut place = voice.place;
            let mut gain = voice.gain;
            let mut finished = length == 0 && voice.stream.is_none();
            // The fade (which waits while the voice is held) and the group.
            let fade = if held { Span::still(voice.fade.gain) } else { voice.fade.advance(frames) };
            let group = self.spans.get(usize::from(voice.settings.group)).copied().unwrap_or(Span::still(1.0));
            let still = fade.from == fade.to && group.from == group.to;
            let faded = fade.from * group.from;
            // Faded out to its stop: silent already, so it ends with this
            // callback.
            let faded_out = voice.fade_stop && voice.fade.gain <= 0.0;
            voice.stopping |= faded_out;
            // The mix ramps in a straight line to its target over this
            // callback.
            let mut mix = voice.mix;
            let mut mix_step = [0.0f32; 5];
            let ramping = mix != voice.target;
            if ramping {
                for k in 0..5 {
                    mix_step[k] = (voice.target[k] - mix[k]) / frames.max(1) as f32;
                }
            }
            let [mut low_l, mut low_r] = voice.low;
            let full_dull = self.dull;
            let (mut window, mut window_at) = (voice.window, voice.window_at);
            let (mut have, mut drained) = (voice.have, voice.drained);
            for f in 0..frames {
                if finished {
                    break;
                }
                let (l0, r0, l1, r1, fraction);
                if let Some(tap) = voice.stream.as_mut() {
                    // A streamed clip: the frames either side of the place,
                    // read from the ring as it gets there.
                    let at = place as u64;
                    let mut ready = true;
                    while have < 2 || window_at < at {
                        // Whether it's ended, read before the ring: the
                        // last samples go in before it's said.
                        let ended = tap.state.ended.load(Ordering::Acquire);
                        if tap.samples.slots() >= usize::from(clip.channels()) && !drained {
                            let (l, r) = pop(&mut tap.samples, clip.channels());
                            match have {
                                0 => window[..2].copy_from_slice(&[l, r]),
                                1 => window[2..].copy_from_slice(&[l, r]),
                                _ => {
                                    window = [window[2], window[3], l, r];
                                    window_at += 1;
                                }
                            }
                            have = (have + 1).min(2);
                        } else if ended && have > 0 && !drained {
                            // Past the end: the last frame again.
                            if have == 2 {
                                window = [window[2], window[3], window[2], window[3]];
                                window_at += 1;
                            } else {
                                window = [window[0], window[1], window[0], window[1]];
                                have = 2;
                            }
                            drained = true;
                        } else {
                            if ended || drained {
                                finished = true;
                            } else if have == 2 {
                                starved = true;
                            }
                            ready = false;
                            break;
                        }
                    }
                    if !ready {
                        break;
                    }
                    (l0, r0, l1, r1) = (window[0], window[1], window[2], window[3]);
                    fraction = (place - window_at as f64) as f32;
                } else {
                    let at = place as usize;
                    fraction = (place - at as f64) as f32;
                    (l0, r0) = clip.frame(at);
                    let next = if at + 1 < length {
                        at + 1
                    } else if looping {
                        0
                    } else {
                        at
                    };
                    (l1, r1) = clip.frame(next);
                }
                gain += (target - gain) * ramp;
                if ramping {
                    for k in 0..5 {
                        mix[k] += mix_step[k];
                    }
                }
                let g = if still { gain * faded } else { gain * fade.at(f) * group.at(f) };
                let l = (l0 + (l1 - l0) * fraction) * g;
                let r = (r0 + (r1 - r0) * fraction) * g;
                let (l, r) = (l * mix[0] + r * mix[1], l * mix[2] + r * mix[3]);
                // A one-pole low pass, open (1) unless the sound is behind.
                let a = 1.0 + (full_dull - 1.0) * mix[4];
                low_l += (l - low_l) * a;
                low_r += (r - low_r) * a;
                let (l, r) = (low_l, low_r);
                let o = f * channels;
                if channels == 1 {
                    out[o] += (l + r) * 0.5;
                } else {
                    out[o] += l;
                    out[o + 1] += r;
                }
                // A held voice ramping down stays where it is.
                if !held {
                    let before = place;
                    place += step;
                    if voice.stream.is_some() {
                        // Not looping (any more), it ends at the end of the
                        // time round it's in, as a decoded clip does, though
                        // its decoder is ahead.
                        let length = length as f64;
                        if !looping && length > 0.0 && (place / length).floor() > (before / length).floor() {
                            finished = true;
                        }
                    } else if place >= length as f64 {
                        if looping {
                            place %= length as f64;
                        } else {
                            finished = true;
                        }
                    }
                }
            }
            voice.place = place;
            (voice.window, voice.window_at) = (window, window_at);
            (voice.have, voice.drained) = (have, drained);
            voice.gain = gain;
            voice.mix = voice.target;
            voice.low = [low_l, low_r];
            let (generation, stopping) = (voice.generation, voice.stopping);
            // A streamed clip's place in it, whichever time round it is.
            let shown = if voice.stream.is_some() && length > 0 { place % length as f64 } else { place };
            self.shared.voices[slot].write(generation, shown);
            if finished {
                self.end(slot, true);
            } else if stopping && (gain < SILENT || faded_out) {
                self.end(slot, false);
            } else {
                playing += 1;
            }
        }
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
        if starved {
            self.shared.starved.fetch_add(1, Ordering::Relaxed);
        }
        self.shared.playing.store(playing, Ordering::Relaxed);
        self.shared.callbacks.fetch_add(1, Ordering::Relaxed);
        self.shared.frames.fetch_add(frames as u64, Ordering::Relaxed);
    }
}

/// A frame from a stream's ring, which has one: left and right (a mono
/// clip's on both).
#[inline]
fn pop(samples: &mut rtrb::Consumer<f32>, channels: u16) -> (f32, f32) {
    let l = samples.pop().unwrap_or(0.0);
    let r = if channels == 2 { samples.pop().unwrap_or(0.0) } else { l };
    (l, r)
}
