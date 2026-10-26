//! A sound: decoded whole as it loads (32-bit float samples at the file's
//! own rate, in one channel or two), or, for a long one, kept as its file
//! and decoded as it plays ([`Reader`]), a second ahead. The mixer
//! resamples to the device's rate as it plays.

use std::fmt;
use std::io::Cursor;
use std::sync::Arc;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// The most channels a clip may have: one or two. A file with more is
/// refused as it loads.
pub const MAX_CHANNELS: u16 = 2;

/// A clip longer than this streams unless told not to: a decoded minute of
/// stereo is about 21 MB.
pub const STREAM_SECONDS: f64 = 10.0;

/// Whether a clip is decoded whole as it loads, or as it plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Streaming {
    /// Streamed when it's longer than [`STREAM_SECONDS`].
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, PartialEq)]
enum Data {
    /// `frames * channels` samples, interleaved.
    Decoded(Box<[f32]>),
    /// The file, decoded as it plays; `extension` helps find its format.
    Streamed { file: Arc<[u8]>, extension: String },
}

/// A sound. Shared with the mixer through an `Arc` that's only ever
/// dropped off the audio thread.
#[derive(Clone, PartialEq)]
pub struct Clip {
    rate: u32,
    channels: u16,
    frames: usize,
    data: Data,
}

impl fmt::Debug for Clip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Clip")
            .field("rate", &self.rate)
            .field("channels", &self.channels)
            .field("frames", &self.frames())
            .field("streamed", &self.streamed())
            .finish()
    }
}

/// A file opened for decoding: its reader, its decoder and its track.
struct Opened {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    rate: u32,
    channels: usize,
    /// Its length in frames, if the file says.
    frames: Option<u64>,
}

impl Opened {
    fn new(file: &[u8], extension: &str) -> Result<Self, String> {
        let source = MediaSourceStream::new(Box::new(Cursor::new(file.to_vec())), Default::default());
        let mut hint = Hint::new();
        if !extension.is_empty() {
            hint.with_extension(extension);
        }
        let format = symphonia::default::get_probe()
            .probe(&hint, source, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| format!("not a WAV, OGG Vorbis or MP3 file ({e})"))?;
        let track = format.default_track(TrackType::Audio).ok_or("has no sound in it")?;
        let (id, frames) = (track.id, track.num_frames);
        let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("has no sound in it")?.clone();
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| format!("its sound can't be decoded ({e})"))?;
        let rate = params.sample_rate.unwrap_or(0);
        let channels = params.channels.as_ref().map_or(0, |c| c.count());
        Ok(Self { format, decoder, track: id, rate, channels, frames })
    }

    /// Decodes the next packet onto `samples`: the frame it starts at (as
    /// the file says), or none at the end of the file.
    fn next(&mut self, samples: &mut Vec<f32>) -> Result<Option<i64>, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(None),
                // A file cut short still plays what it has.
                Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(format!("can't be read ({e})")),
            };
            if packet.track_id != self.track {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buffer) => {
                    let spec = buffer.spec();
                    self.rate = spec.rate();
                    self.channels = spec.channels().count();
                    let at = samples.len();
                    samples.resize(at + buffer.samples_interleaved(), 0.0f32);
                    buffer.copy_to_slice_interleaved(&mut samples[at..]);
                    return Ok(Some(packet.pts.get()));
                }
                // A bad packet is skipped, as players do.
                Err(DecodeError::DecodeError(_)) => {}
                Err(e) => return Err(format!("can't be decoded ({e})")),
            }
        }
    }

    /// Goes to before `frame`, as near as the file allows; false if it
    /// can't, and is where it was.
    fn seek(&mut self, frame: u64) -> bool {
        let Ok(ts) = i64::try_from(frame) else { return false };
        let to = SeekTo::Timestamp { ts: ts.into(), track_id: self.track };
        let sought = self.format.seek(SeekMode::Accurate, to).is_ok();
        if sought {
            self.decoder.reset();
        }
        sought
    }

    fn check_channels(&self) -> Result<u16, String> {
        let channels = u16::try_from(self.channels).unwrap_or(u16::MAX);
        if channels > MAX_CHANNELS {
            return Err(format!("has {channels} channels; a clip has 1 or 2"));
        }
        Ok(channels)
    }
}

impl Clip {
    /// A clip from samples already decoded. Fails for a rate of 0, a
    /// channel count other than 1 or 2, samples that aren't whole frames,
    /// or a sample that isn't finite.
    pub fn new(rate: u32, channels: u16, samples: Vec<f32>) -> Result<Self, String> {
        check_format(rate, channels)?;
        if !samples.len().is_multiple_of(usize::from(channels)) {
            return Err(format!("{} samples aren't whole frames of {channels} channels", samples.len()));
        }
        if let Some(at) = samples.iter().position(|s| !s.is_finite()) {
            return Err(format!("sample {at} isn't a number"));
        }
        let frames = samples.len() / usize::from(channels);
        Ok(Self { rate, channels, frames, data: Data::Decoded(samples.into_boxed_slice()) })
    }

    /// Decodes a WAV, OGG Vorbis or MP3 file from its bytes, whole unless
    /// it's longer than [`STREAM_SECONDS`]. `name` (the file's name, or "")
    /// helps find the format and goes in the error.
    pub fn decode(bytes: &[u8], name: &str) -> Result<Self, String> {
        Self::load(bytes, name, Streaming::Auto)
    }

    /// Loads a WAV, OGG Vorbis or MP3 file from its bytes: decoded whole,
    /// or kept to be decoded as it plays, as `streaming` says.
    pub fn load(bytes: &[u8], name: &str, streaming: Streaming) -> Result<Self, String> {
        let fail = |what: String| if name.is_empty() { what } else { format!("{name}: {what}") };
        let extension = name.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_ascii_lowercase());
        let mut opened = Opened::new(bytes, &extension).map_err(fail)?;
        let stream = match streaming {
            Streaming::Always => true,
            Streaming::Never => false,
            Streaming::Auto => {
                opened.rate > 0 && opened.frames.is_some_and(|f| f as f64 / f64::from(opened.rate) > STREAM_SECONDS)
            }
        };
        if !stream {
            let mut samples = Vec::new();
            while opened.next(&mut samples).map_err(fail)?.is_some() {
                // A file that doesn't say its length, decoded until it's
                // known to be long: streamed after all.
                if streaming == Streaming::Auto
                    && opened.frames.is_none()
                    && opened.channels > 0
                    && opened.rate > 0
                    && (samples.len() / opened.channels) as f64 / f64::from(opened.rate) > STREAM_SECONDS
                {
                    return Self::load(bytes, name, Streaming::Always);
                }
            }
            let channels = opened.check_channels().map_err(fail)?;
            return Self::new(opened.rate, channels, samples).map_err(fail);
        }
        // Its length: as the file says, else counted by decoding it once,
        // which a file that doesn't say needs.
        let frames = match opened.frames {
            Some(frames) => frames,
            None => {
                let (mut frames, mut samples) = (0u64, Vec::new());
                while opened.next(&mut samples).map_err(fail)?.is_some() {
                    frames += (samples.len() / opened.channels.max(1)) as u64;
                    samples.clear();
                }
                frames
            }
        };
        let channels = opened.check_channels().map_err(fail)?;
        check_format(opened.rate, channels).map_err(fail)?;
        Ok(Self {
            rate: opened.rate,
            channels,
            frames: usize::try_from(frames).unwrap_or(usize::MAX),
            data: Data::Streamed { file: bytes.into(), extension },
        })
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
        self.frames
    }

    /// How long it plays, in seconds, at pitch 1.
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate)
    }

    /// Whether it's decoded as it plays, rather than whole.
    pub fn streamed(&self) -> bool {
        matches!(self.data, Data::Streamed { .. })
    }

    /// The memory it takes, in bytes: its samples, or a streamed one's
    /// file.
    pub fn bytes(&self) -> usize {
        match &self.data {
            Data::Decoded(samples) => std::mem::size_of_val(&**samples),
            Data::Streamed { file, .. } => file.len(),
        }
    }

    /// Its samples, interleaved; none for a streamed clip.
    pub fn samples(&self) -> &[f32] {
        match &self.data {
            Data::Decoded(samples) => samples,
            Data::Streamed { .. } => &[],
        }
    }

    /// The left and right sample of `frame` (a mono clip's on both), of a
    /// decoded clip.
    #[inline]
    pub(crate) fn frame(&self, frame: usize) -> (f32, f32) {
        // Never past the end, which would panic on the audio thread.
        let Data::Decoded(samples) = &self.data else { return (0.0, 0.0) };
        if self.channels == 1 {
            let s = samples.get(frame).copied().unwrap_or(0.0);
            (s, s)
        } else {
            let at = frame * 2;
            match samples.get(at..at + 2) {
                Some(&[l, r]) => (l, r),
                _ => (0.0, 0.0),
            }
        }
    }
}

fn check_format(rate: u32, channels: u16) -> Result<(), String> {
    if rate == 0 {
        return Err("a clip needs a sample rate above 0".into());
    }
    if channels == 0 || channels > MAX_CHANNELS {
        return Err(format!("a clip has 1 or 2 channels, not {channels}"));
    }
    Ok(())
}

/// A streamed clip, decoded as it plays: from a frame, again from the
/// start when it loops. Runs off the audio thread.
pub(crate) struct Reader {
    clip: Arc<Clip>,
    opened: Option<Opened>,
    /// Decoded, not yet handed on, from `pending_at`.
    pending: Vec<f32>,
    pending_at: usize,
    /// Where to start: frames before it are skipped, by where each
    /// packet says it starts after a seek, else by counting from the start.
    from: u64,
    sought: bool,
    /// Frames decoded since the start, without a seek.
    counted: u64,
    pub(crate) ended: bool,
}

impl fmt::Debug for Reader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader").field("clip", &self.clip).field("ended", &self.ended).finish()
    }
}

impl Reader {
    pub(crate) fn new(clip: Arc<Clip>, from: u64) -> Self {
        Self { clip, opened: None, pending: Vec::new(), pending_at: 0, from, sought: false, counted: 0, ended: false }
    }

    fn open(&mut self) -> Result<(), String> {
        let Data::Streamed { file, extension } = &self.clip.data else { return Err("not a streamed clip".into()) };
        let mut opened = Opened::new(file, extension)?;
        // A fifth of a second early, decoded and dropped: a decoder needs
        // what came before to make the first frames after a seek as they'd
        // be had it played through.
        let early = self.from.saturating_sub(u64::from(self.clip.rate / 5));
        self.sought = early > 0 && opened.seek(early);
        self.counted = 0;
        self.opened = Some(opened);
        Ok(())
    }

    /// Hands on what's decoded to `out`, whole frames only, decoding more
    /// while it has room; from the start again at the end when `looping`.
    /// A file that fails part way ends there.
    pub(crate) fn fill(&mut self, out: &mut rtrb::Producer<f32>, looping: bool) {
        let channels = usize::from(self.clip.channels);
        let mut empty_loops = 0;
        while !self.ended {
            if self.pending_at < self.pending.len() {
                let room = out.slots() / channels * channels;
                if room == 0 {
                    return;
                }
                let n = room.min(self.pending.len() - self.pending_at);
                if let Ok(chunk) = out.write_chunk_uninit(n) {
                    chunk.fill_from_iter(self.pending[self.pending_at..self.pending_at + n].iter().copied());
                }
                self.pending_at += n;
                continue;
            }
            self.pending.clear();
            self.pending_at = 0;
            if self.opened.is_none() && self.open().is_err() {
                self.ended = true;
                return;
            }
            let Some(opened) = self.opened.as_mut() else { return };
            match opened.next(&mut self.pending) {
                Ok(Some(pts)) => {
                    empty_loops = 0;
                    // Another channel count than the clip's can't be played.
                    if opened.channels != channels {
                        self.pending.clear();
                    }
                    let frames = (self.pending.len() / channels) as u64;
                    if self.from > 0 {
                        // Where this packet starts: as it says after a seek
                        // (where the seek lands isn't always where it says),
                        // else counted.
                        let start = if self.sought { u64::try_from(pts).unwrap_or(0) } else { self.counted };
                        let skip = self.from.saturating_sub(start).min(frames);
                        self.pending_at = skip as usize * channels;
                        if start + frames >= self.from {
                            self.from = 0;
                        }
                    }
                    self.counted += frames;
                }
                Ok(None) if looping && empty_loops == 0 => {
                    // From the start: a loop of a file with no sound ends.
                    empty_loops += 1;
                    self.opened = None;
                    self.from = 0;
                }
                Ok(None) | Err(_) => self.ended = true,
            }
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
