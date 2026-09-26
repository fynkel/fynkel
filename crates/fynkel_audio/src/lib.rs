//! Fynkel audio (blueprint §9, §19.2d item 11; `docs/audio.md` is the
//! reference). Two halves joined by lock-free rings:
//!
//! - the [`Audio`] engine on the world's thread — the clip registry, voice
//!   allocation with priorities, the tick clock every voice's position and
//!   end is decided by (deterministic, so a headless run reports the same
//!   `PLAYBACK_ENDED` ticks as a device run), the bus/mute/listener gains;
//! - the [`Mixer`] on the callback's thread — fixed tables, linear-interp
//!   resampling, gain smoothing, no allocation (`tests/allocation.rs`);
//! - and, for a streamed sound ([`stream`]), a decoder thread per voice
//!   feeding the mixer through a third ring.
//!
//! The backend is null by default (a test or the headless host calls
//! `Audio::render`); the `device` feature adds the real output device
//! through `cpal` (`Audio::open_device`).

pub mod clip;
pub mod components;
pub mod types;

// The engine (mixer, stream decoders, backends) is the `engine` feature,
// on by default; without it (a runtime built with `modules.strip` and the
// audio module off, ADR-0029) a stand-in of the same API plays nothing
// and links no rtrb, symphonia or cpal. The types stay: they are the
// contract.
#[cfg(feature = "engine")]
pub mod backend;
#[cfg(feature = "engine")]
pub mod engine;
#[cfg(feature = "engine")]
pub mod mixer;
#[cfg(feature = "engine")]
pub mod stream;
#[cfg(not(feature = "engine"))]
mod stub;

pub use clip::{AudioClip, PeakBuilder, StreamSource, MAX_CLIP_CHANNELS};

/// A clip's waveform, `[channel][column][min, max]` flattened (the
/// editor's sound preview): a stream is decoded once for it. `Err` for a
/// stream that does not decode (or, without the engine, any stream).
pub fn peaks(clip: &AudioClip, columns: usize) -> Result<Vec<f32>, String> {
    if let Some(p) = clip.peaks(columns) {
        return Ok(p);
    }
    #[cfg(feature = "engine")]
    if let Some((source, frames)) = &clip.stream {
        return stream::peaks(source, clip.channels, *frames, columns);
    }
    Err("no waveform for a streamed sound in this build".to_string())
}
pub use components::{AudioBus, AudioListener, AudioSource, MAX_PITCH, MIN_PITCH};
#[cfg(feature = "engine")]
pub use engine::Audio;
#[cfg(feature = "engine")]
pub use mixer::{Mixer, MixerStats};
#[cfg(not(feature = "engine"))]
pub use stub::Audio;
pub use types::{
    AudioStats, DeviceInfo, Ended, PlayError, PlayParams, VoiceId, VoiceState,
    GAIN_SMOOTHING_SECONDS, MAX_CLIPS, MAX_VOICES, NULL_CHANNELS, NULL_SAMPLE_RATE,
    STREAM_RING_SECONDS,
};
