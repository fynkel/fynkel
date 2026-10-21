//! A sound, decoded: 32-bit float samples at the file's own rate, in one
//! channel or two. The mixer resamples to the device's rate as it plays.

use std::fmt;
use std::io::Cursor;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// The most channels a clip may have: one or two. A file with more is
/// refused as it loads.
pub const MAX_CHANNELS: u16 = 2;

/// A decoded sound. Shared with the mixer through an `Arc` that's only
/// ever dropped off the audio thread.
#[derive(Clone, PartialEq)]
pub struct Clip {
    rate: u32,
    channels: u16,
    /// `frames * channels` samples, interleaved.
    samples: Box<[f32]>,
}

impl fmt::Debug for Clip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Clip")
            .field("rate", &self.rate)
            .field("channels", &self.channels)
            .field("frames", &self.frames())
            .finish()
    }
}

impl Clip {
    /// A clip from samples already decoded. Fails for a rate of 0, a
    /// channel count other than 1 or 2, samples that aren't whole frames,
    /// or a sample that isn't finite.
    pub fn new(rate: u32, channels: u16, samples: Vec<f32>) -> Result<Self, String> {
        if rate == 0 {
            return Err("a clip needs a sample rate above 0".into());
        }
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(format!("a clip has 1 or 2 channels, not {channels}"));
        }
        if !samples.len().is_multiple_of(usize::from(channels)) {
            return Err(format!("{} samples aren't whole frames of {channels} channels", samples.len()));
        }
        if let Some(at) = samples.iter().position(|s| !s.is_finite()) {
            return Err(format!("sample {at} isn't a number"));
        }
        Ok(Self { rate, channels, samples: samples.into_boxed_slice() })
    }

    /// Decodes a WAV, OGG Vorbis or MP3 file from its bytes. `name` (the
    /// file's name, or "") helps find the format and goes in the error.
    pub fn decode(bytes: &[u8], name: &str) -> Result<Self, String> {
        let fail = |what: String| if name.is_empty() { what } else { format!("{name}: {what}") };
        let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
        let mut hint = Hint::new();
        if let Some((_, extension)) = name.rsplit_once('.') {
            hint.with_extension(&extension.to_ascii_lowercase());
        }
        let mut format = symphonia::default::get_probe()
            .probe(&hint, source, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| fail(format!("not a WAV, OGG Vorbis or MP3 file ({e})")))?;
        let track = format.default_track(TrackType::Audio).ok_or_else(|| fail("has no sound in it".into()))?;
        let track_id = track.id;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| fail("has no sound in it".into()))?
            .clone();
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| fail(format!("its sound can't be decoded ({e})")))?;

        let mut rate = params.sample_rate.unwrap_or(0);
        let mut channels = params.channels.as_ref().map_or(0, |c| c.count());
        let mut samples = Vec::new();
        let mut packet_samples = Vec::new();
        loop {
            let packet = match format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                // A file cut short still plays what it has.
                Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(fail(format!("can't be read ({e})"))),
            };
            if packet.track_id != track_id {
                continue;
            }
            match decoder.decode(&packet) {
                Ok(buffer) => {
                    let spec = buffer.spec();
                    rate = spec.rate();
                    channels = spec.channels().count();
                    packet_samples.resize(buffer.samples_interleaved(), 0.0f32);
                    buffer.copy_to_slice_interleaved(&mut packet_samples);
                    samples.extend_from_slice(&packet_samples);
                }
                // A bad packet is skipped, as players do.
                Err(DecodeError::DecodeError(_)) => {}
                Err(e) => return Err(fail(format!("can't be decoded ({e})"))),
            }
        }
        let channels = u16::try_from(channels).unwrap_or(u16::MAX);
        if channels > MAX_CHANNELS {
            return Err(fail(format!("has {channels} channels; a clip has 1 or 2")));
        }
        Self::new(rate, channels, samples).map_err(fail)
    }

    /// Its sample rate, in frames a second.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// 1 or 2.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// How many frames (a sample per channel) it has.
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    /// How long it plays, in seconds, at pitch 1.
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate)
    }

    /// The memory its samples take, in bytes.
    pub fn bytes(&self) -> usize {
        std::mem::size_of_val(&*self.samples)
    }

    /// Its samples, interleaved.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// The left and right sample of `frame` (a mono clip's on both).
    #[inline]
    pub(crate) fn frame(&self, frame: usize) -> (f32, f32) {
        if self.channels == 1 {
            let s = self.samples[frame];
            (s, s)
        } else {
            (self.samples[frame * 2], self.samples[frame * 2 + 1])
        }
    }
}

/// A WAV file of 16-bit samples: for tests, and for a game's tests to make
/// clips without files.
pub fn wav(rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
    let data = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    out
}
