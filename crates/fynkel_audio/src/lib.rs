//! Fynkel audio (blueprint §9, §19.2d item 11; `docs/audio.md` is the
//! reference). Two halves joined by lock-free rings:
//!
//! - the [`Audio`] engine on the world's thread — the clip registry, voice
//!   allocation with priorities, the tick clock every voice's position and
//!   end is decided by (deterministic, so a headless run reports the same
//!   `PLAYBACK_ENDED` ticks as a device run), the bus/mute/listener gains;
//! - the [`Mixer`] on the callback's thread — fixed tables, linear-interp
//!   resampling, gain smoothing, no allocation (`tests/allocation.rs`).
//!
//! The backend is null by default (a test or the headless host calls
//! `Audio::render`); the `device` feature adds the real output device
//! through `cpal` (`Audio::open_device`).

pub mod backend;
pub mod clip;
pub mod components;
pub mod engine;
pub mod mixer;

pub use backend::DeviceInfo;
pub use clip::{AudioClip, MAX_CLIP_CHANNELS};
pub use components::{AudioBus, AudioListener, AudioSource, MAX_PITCH, MIN_PITCH};
pub use engine::{
    Audio, AudioStats, Ended, PlayError, PlayParams, VoiceId, VoiceState, NULL_CHANNELS,
    NULL_SAMPLE_RATE,
};
pub use mixer::{Mixer, MixerStats, GAIN_SMOOTHING_SECONDS, MAX_CLIPS, MAX_VOICES};
