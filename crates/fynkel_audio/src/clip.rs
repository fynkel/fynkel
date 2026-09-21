//! A decoded sound: interleaved `f32` PCM as the cooked `CLIP` chunk
//! stores it (`fynkel_cooked::payload::ClipPayload`), shared between the
//! world and the mixer through an `Arc` that only the world side ever
//! drops (the mixer hands its references back; see `mixer::Return`).

use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct AudioClip {
    pub sample_rate: u32,
    /// 1 or 2; a clip with more channels is refused at registration.
    pub channels: u32,
    /// `frames * channels` samples in `[-1, 1]`.
    pub samples: Vec<f32>,
}

/// Channels a clip may have (mono, stereo).
pub const MAX_CLIP_CHANNELS: u32 = 2;

impl AudioClip {
    /// A clip from cooked samples; `Err` for a channel count the mixer
    /// cannot play, a zero sample rate, or samples that are not a whole
    /// number of frames.
    pub fn new(
        sample_rate: u32,
        channels: u32,
        samples: Vec<f32>,
    ) -> Result<Arc<AudioClip>, String> {
        if sample_rate == 0 {
            return Err("a clip needs a sample rate".into());
        }
        if channels == 0 || channels > MAX_CLIP_CHANNELS {
            return Err(format!(
                "a clip has 1 or {MAX_CLIP_CHANNELS} channels, got {channels}"
            ));
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err(format!(
                "{} samples are not a whole number of {channels}-channel frames",
                samples.len()
            ));
        }
        Ok(Arc::new(AudioClip {
            sample_rate,
            channels,
            samples,
        }))
    }

    pub fn frames(&self) -> u64 {
        (self.samples.len() / self.channels as usize) as u64
    }

    /// Length in seconds at pitch 1.
    pub fn duration(&self) -> f32 {
        self.frames() as f32 / self.sample_rate as f32
    }

    /// The left and right sample of frame `i` (a mono clip on both).
    #[inline]
    pub fn frame(&self, i: usize) -> (f32, f32) {
        if self.channels == 1 {
            let s = self.samples[i];
            (s, s)
        } else {
            (self.samples[i * 2], self.samples[i * 2 + 1])
        }
    }
}
