//! The engine's values that are part of the contract (ADR-0029): what the
//! world, the wire and the hosts name whether or not the engine is built
//! (the `engine` feature).

use fynkel_contracts::{AssetRef, Entity};

use crate::components::AudioBus;

/// Voices the mixer renders at once; the engine steals or refuses beyond.
pub const MAX_VOICES: usize = 32;
/// Clips loaded into the mixer at once.
pub const MAX_CLIPS: usize = 256;
/// Seconds a gain change takes to settle (declick).
pub const GAIN_SMOOTHING_SECONDS: f32 = 0.005;
/// Seconds of decoded audio the ring holds ahead of the mixer.
pub const STREAM_RING_SECONDS: f32 = 1.0;

/// The output format the mixer renders for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub sample_rate: u32,
    pub channels: usize,
    /// A real device (else the null backend).
    pub device: bool,
}

/// Sample rate the null backend renders at (what a headless run reports).
pub const NULL_SAMPLE_RATE: u32 = 48_000;
/// Channels the null backend renders.
pub const NULL_CHANNELS: usize = 2;

/// A playing voice: its slot and the generation that makes a stale handle
/// harmless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceId {
    pub slot: u8,
    pub generation: u32,
}

impl VoiceId {
    /// Packed for the wire (`fynkel.rec.AudioState.voice`).
    pub fn packed(self) -> u32 {
        (self.generation << 8) | self.slot as u32
    }
}

/// How a voice is started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayParams {
    pub bus: AudioBus,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub priority: i32,
}

impl PlayParams {
    /// Shared validation for gameplay intents and legacy command execution.
    pub fn validate(&self) -> Result<(), String> {
        if !self.volume.is_finite() || !(0.0..=1.0).contains(&self.volume) {
            return Err(format!(
                "playSound: volume must be within [0, 1], got {}",
                self.volume
            ));
        }
        if !self.pitch.is_finite() || !(crate::MIN_PITCH..=crate::MAX_PITCH).contains(&self.pitch) {
            return Err(format!(
                "playSound: pitch must be within [{}, {}], got {}",
                crate::MIN_PITCH,
                crate::MAX_PITCH,
                self.pitch
            ));
        }
        Ok(())
    }
}

impl Default for PlayParams {
    fn default() -> Self {
        Self {
            bus: AudioBus::Sfx,
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            priority: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayError {
    /// The clip was never registered (`SOUND_UNAVAILABLE`).
    UnknownClip,
    /// Every voice is taken by a higher priority.
    Refused,
}

/// What `advance` reports for a voice that ended this tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ended {
    pub voice: VoiceId,
    pub source: Option<Entity>,
    pub clip: AssetRef,
}

/// A voice as the engine tracks it (the wire's `AudioState` is built from
/// this).
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceState {
    pub id: VoiceId,
    pub source: Option<Entity>,
    pub clip: AssetRef,
    pub bus: AudioBus,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub priority: i32,
    pub paused: bool,
    /// Seconds into the clip.
    pub time: f32,
    pub duration: f32,
}

/// Counters the world reports (`fynkel.rec.AudioStats`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioStats {
    pub device: bool,
    pub sample_rate: u32,
    pub channels: u32,
    pub voices: u32,
    pub max_voices: u32,
    pub started: u64,
    pub stolen: u64,
    pub refused: u64,
    pub ended: u64,
    pub clips: u32,
    pub callbacks: u64,
    pub underruns: u64,
    pub suspended: bool,
    /// Streamed clips registered (decoded as they play, not held).
    pub streams: u32,
    /// Callbacks in which a streamed voice's decoder had fallen behind
    /// (the voice held silent for the rest of that callback).
    pub starved: u64,
}
