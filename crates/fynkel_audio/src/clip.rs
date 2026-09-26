//! A sound: interleaved `f32` PCM as the cooked `CLIP` chunk stores it
//! (`fynkel_cooked::payload::ClipPayload`), shared between the world and
//! the mixer through an `Arc` that only the world side ever drops (the
//! mixer hands its references back; see `mixer::Return`) — or, for a
//! streamed sound (`STRM`, M3-11 part 2), the encoded source and its
//! format, decoded per voice while it plays (`stream`).

use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct AudioClip {
    pub sample_rate: u32,
    /// 1 or 2; a clip with more channels is refused at registration.
    pub channels: u32,
    /// `frames * channels` samples in `[-1, 1]`; empty for a stream.
    pub samples: Vec<f32>,
    /// The encoded source of a streamed sound, with its frame count.
    pub stream: Option<(Arc<StreamSource>, u64)>,
}

/// The encoded half of a streamed clip (`AudioClip::stream`).
#[derive(Clone, Debug, PartialEq)]
pub struct StreamSource {
    /// `pcm` or `vorbis`, as the importer found it.
    pub codec: String,
    /// The source file's bytes (WAV or Ogg).
    pub bytes: Arc<[u8]>,
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
            stream: None,
        }))
    }

    /// A streamed sound from a cooked `STRM` chunk: `bytes` is the source
    /// file (WAV or Ogg Vorbis) and `frames` what the importer counted.
    /// The same rules as `new` on the format.
    pub fn streamed(
        sample_rate: u32,
        channels: u32,
        frames: u64,
        codec: &str,
        bytes: Vec<u8>,
    ) -> Result<Arc<AudioClip>, String> {
        if sample_rate == 0 {
            return Err("a clip needs a sample rate".into());
        }
        if channels == 0 || channels > MAX_CLIP_CHANNELS {
            return Err(format!(
                "a clip has 1 or {MAX_CLIP_CHANNELS} channels, got {channels}"
            ));
        }
        if bytes.is_empty() {
            return Err("a streamed clip needs its source bytes".into());
        }
        Ok(Arc::new(AudioClip {
            sample_rate,
            channels,
            samples: Vec::new(),
            stream: Some((
                Arc::new(StreamSource {
                    codec: codec.to_string(),
                    bytes: bytes.into(),
                }),
                frames,
            )),
        }))
    }

    /// Whether the sound is decoded as it plays.
    pub fn is_stream(&self) -> bool {
        self.stream.is_some()
    }

    pub fn frames(&self) -> u64 {
        match &self.stream {
            Some((_, frames)) => *frames,
            None => (self.samples.len() / self.channels as usize) as u64,
        }
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

/// A waveform's columns (the editor's sound preview): per channel, the
/// lowest and highest sample (and the zero line) of each of `columns` equal spans of the
/// clip, fed one frame at a time.
pub struct PeakBuilder {
    columns: usize,
    channels: usize,
    frames: u64,
    seen: u64,
    /// `[channel][column][min, max]`, flattened.
    out: Vec<f32>,
}

impl PeakBuilder {
    pub fn new(channels: u32, frames: u64, columns: usize) -> PeakBuilder {
        let channels = channels.max(1) as usize;
        PeakBuilder {
            columns,
            channels,
            frames,
            seen: 0,
            out: vec![0.0; channels * columns * 2],
        }
    }

    /// One interleaved frame.
    pub fn push(&mut self, frame: &[f32]) {
        if self.columns == 0 || self.frames == 0 {
            return;
        }
        let column =
            ((self.seen * self.columns as u64) / self.frames).min(self.columns as u64 - 1) as usize;
        for (c, &s) in frame.iter().take(self.channels).enumerate() {
            let i = (c * self.columns + column) * 2;
            self.out[i] = self.out[i].min(s);
            self.out[i + 1] = self.out[i + 1].max(s);
        }
        self.seen += 1;
    }

    pub fn finish(self) -> Vec<f32> {
        self.out
    }
}

impl AudioClip {
    /// The waveform of a clip held in memory ([`PeakBuilder`]); `None` for
    /// a stream (decoded by `stream::peaks`).
    pub fn peaks(&self, columns: usize) -> Option<Vec<f32>> {
        if self.is_stream() {
            return None;
        }
        let mut b = PeakBuilder::new(self.channels, self.frames(), columns);
        for frame in self.samples.chunks_exact(self.channels as usize) {
            b.push(frame);
        }
        Some(b.finish())
    }
}

#[cfg(test)]
mod peak_tests {
    use super::*;

    #[test]
    fn peaks_per_channel_and_column() {
        // Stereo, 4 frames, 2 columns: left rises, right falls.
        let clip = AudioClip::new(8, 2, vec![0.1, -0.1, 0.2, -0.2, 0.5, -0.5, 0.9, -0.9]).unwrap();
        let p = clip.peaks(2).unwrap();
        assert_eq!(p, vec![0.0, 0.2, 0.0, 0.9, -0.2, 0.0, -0.9, 0.0]);
    }
}
