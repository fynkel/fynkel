//! Streamed sounds (M3-11 part 2): a clip the cook kept encoded (`STRM`)
//! is decoded while it plays, on a thread of its own, into a lock-free
//! ring the mixer reads like it reads a clip's samples — one frame at a
//! time, resampled and gain-smoothed the same way. Nothing is decoded or
//! freed on the callback's thread; the decoder allocates and blocks
//! freely on its own.
//!
//! One decoder per playing voice: the engine spawns it at `play` (and
//! again at the voice's position when a device opens, since the mixer
//! moves), it exits when the voice stops — a flag the engine sets, or
//! the ring's consumer gone — or when a non-looping stream ends. A loop
//! is a seek back to the start when the last packet is out, so a looping
//! track never gaps. Reading an empty ring after the first samples came
//! is starvation: the voice holds silent for the rest of that callback
//! and `MixerStats::starved` counts it (`fynkel.rec.AudioStats.starved`).
//! The engine's clock, not the decoder, decides where a streamed voice
//! is and when it ends, so a headless run reports the same ticks.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

pub use crate::clip::StreamSource;
pub use crate::types::STREAM_RING_SECONDS;
/// How long the decoder sleeps when the ring is full.
const FULL_SLEEP: Duration = Duration::from_millis(5);

/// Flags and counters shared by the engine, the decoder and the mixer's
/// tap for one streamed voice.
#[derive(Debug, Default)]
pub struct StreamShared {
    /// The engine stopped the voice: the decoder exits.
    stop: AtomicBool,
    /// Wrap at the end (read by the decoder when it gets there).
    looping: AtomicBool,
    /// The decoder produced the last sample of a non-looping stream.
    finished: AtomicBool,
    /// The decoder could not open or read the source; the voice plays
    /// silence (the engine's clock ends it as usual).
    failed: AtomicBool,
    /// Frames the decoder has produced since it started (diagnostics).
    produced: AtomicU64,
}

impl StreamShared {
    pub fn new(looping: bool) -> Arc<StreamShared> {
        let s = StreamShared::default();
        s.looping.store(looping, Ordering::Relaxed);
        Arc::new(s)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn set_looping(&self, looping: bool) {
        self.looping.store(looping, Ordering::Relaxed);
    }

    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    pub fn produced(&self) -> u64 {
        self.produced.load(Ordering::Relaxed)
    }
}

/// The mixer's end of a streamed voice: the ring's consumer and the two
/// frames linear interpolation needs. Moved into the mixer with the
/// voice's start and handed back (`Return::StreamReleased`) when the
/// voice is done, so the ring is freed on the world's thread.
#[derive(Debug)]
pub struct StreamTap {
    consumer: rtrb::Consumer<f32>,
    shared: Arc<StreamShared>,
    channels: usize,
    /// The frame at the integer position and the one after it.
    prev: (f32, f32),
    next: (f32, f32),
    /// Fraction between `prev` and `next`.
    frac: f64,
    /// `next` has been read (the first frames arrived).
    primed: bool,
    /// The last read found the ring empty with the decoder still going.
    starved: bool,
    /// The decoder finished and the last frame moved up to `prev`.
    drained: bool,
}

/// What a tap says for one output frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapRead {
    /// A frame to mix.
    Frame,
    /// Nothing yet (the decoder has not delivered): silence, not counted.
    Waiting,
    /// The ring ran dry mid-stream: silence, counted once per callback.
    Starved,
    /// The stream ended (non-looping) and every frame is out.
    Ended,
}

impl StreamTap {
    fn pop_frame(&mut self) -> Option<(f32, f32)> {
        if self.channels == 1 {
            let s = self.consumer.pop().ok()?;
            Some((s, s))
        } else {
            if self.consumer.slots() < 2 {
                return None;
            }
            let l = self.consumer.pop().ok()?;
            let r = self.consumer.pop().ok()?;
            Some((l, r))
        }
    }

    /// Advances to the next output frame: `step` source frames further.
    /// Returns the interpolated frame or why there is none. Allocation-free.
    #[inline]
    pub fn read(&mut self, step: f64, advance: bool) -> (TapRead, (f32, f32)) {
        if !self.primed {
            match self.pop_frame() {
                Some(f) => {
                    self.prev = f;
                    self.next = self.pop_frame().unwrap_or(f);
                    self.frac = 0.0;
                    self.primed = true;
                }
                None => {
                    if self.shared.finished() || self.shared.failed() {
                        return (TapRead::Ended, (0.0, 0.0));
                    }
                    return (TapRead::Waiting, (0.0, 0.0));
                }
            }
        }
        while self.frac >= 1.0 {
            match self.pop_frame() {
                Some(f) => {
                    self.prev = self.next;
                    self.next = f;
                    self.frac -= 1.0;
                }
                None => {
                    if self.shared.finished() {
                        // The last frame is still in `next`: play it,
                        // then the end.
                        if self.drained {
                            return (TapRead::Ended, (0.0, 0.0));
                        }
                        self.drained = true;
                        self.prev = self.next;
                        self.frac -= 1.0;
                        continue;
                    }
                    self.starved = true;
                    return (TapRead::Starved, (0.0, 0.0));
                }
            }
        }
        let t = self.frac as f32;
        let l = self.prev.0 + (self.next.0 - self.prev.0) * t;
        let r = self.prev.1 + (self.next.1 - self.prev.1) * t;
        if advance {
            self.frac += step;
        }
        (TapRead::Frame, (l, r))
    }

    /// Whether the last callback's reads starved; cleared for the next.
    pub fn take_starved(&mut self) -> bool {
        std::mem::replace(&mut self.starved, false)
    }
}

/// Starts the decoder for one voice at `start_frame`; the returned tap
/// goes to the mixer with the voice's start. The ring is sized for
/// `STREAM_RING_SECONDS` of the clip.
pub fn spawn(
    source: Arc<StreamSource>,
    sample_rate: u32,
    channels: u32,
    start_frame: u64,
    shared: Arc<StreamShared>,
) -> StreamTap {
    let channels = channels.clamp(1, 2) as usize;
    let capacity =
        ((sample_rate.max(1) as f32 * STREAM_RING_SECONDS) as usize).max(1024) * channels;
    let (producer, consumer) = rtrb::RingBuffer::new(capacity);
    let decoder_shared = shared.clone();
    thread::Builder::new()
        .name("fynkel-audio-stream".into())
        .spawn(move || decode_loop(source, channels, start_frame, decoder_shared, producer))
        .ok();
    StreamTap {
        consumer,
        shared,
        channels,
        prev: (0.0, 0.0),
        next: (0.0, 0.0),
        frac: 0.0,
        primed: false,
        starved: false,
        drained: false,
    }
}

struct Reader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    /// Frames to drop after a seek (the seek lands before the target).
    skip: u64,
}

fn open(bytes: &Arc<[u8]>, start_frame: u64) -> Result<Reader, SymphoniaError> {
    let source = MediaSourceStream::new(
        Box::new(Cursor::new(bytes.clone())),
        MediaSourceStreamOptions::default(),
    );
    let format = symphonia::default::get_probe().probe(
        &Hint::new(),
        source,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = format
        .first_track_known_codec(TrackType::Audio)
        .ok_or(SymphoniaError::Unsupported("no audio track"))?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err(SymphoniaError::Unsupported("no codec parameters"));
    };
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())?;
    let mut reader = Reader {
        format,
        decoder,
        track_id,
        skip: 0,
    };
    if start_frame > 0 {
        // The timebase of a WAV or Vorbis track is one frame; an accurate
        // seek lands at or before the target and says how far before.
        let seeked = reader.format.seek(
            SeekMode::Accurate,
            SeekTo::Timestamp {
                ts: Timestamp::new(start_frame as i64),
                track_id,
            },
        )?;
        reader.decoder.reset();
        reader.skip = (seeked.required_ts.get() - seeked.actual_ts.get()).max(0) as u64;
    }
    Ok(reader)
}

fn decode_loop(
    source: Arc<StreamSource>,
    channels: usize,
    start_frame: u64,
    shared: Arc<StreamShared>,
    producer: rtrb::Producer<f32>,
) {
    let failed = decode_into(&source, channels, start_frame, &shared, producer).is_err();
    if failed {
        shared.failed.store(true, Ordering::Release);
    }
    // However it ended — the last packet out, the engine's stop, the
    // mixer's consumer gone, an error — the tap plays out what the ring
    // holds and then reports the end.
    shared.finished.store(true, Ordering::Release);
}

/// `Err` for a source that does not open or read.
fn decode_into(
    source: &Arc<StreamSource>,
    channels: usize,
    start_frame: u64,
    shared: &StreamShared,
    mut producer: rtrb::Producer<f32>,
) -> Result<(), SymphoniaError> {
    let mut reader = open(&source.bytes, start_frame)?;
    let mut chunk: Vec<f32> = Vec::new();
    let mut pending: Vec<f32> = Vec::new();
    let done = |producer: &rtrb::Producer<f32>| {
        shared.stop.load(Ordering::Relaxed) || producer.is_abandoned()
    };
    loop {
        if done(&producer) {
            return Ok(());
        }
        let packet = match reader.format.next_packet()? {
            Some(p) => p,
            None => {
                if shared.looping.load(Ordering::Relaxed) {
                    reader = open(&source.bytes, 0)?;
                    continue;
                }
                return Ok(());
            }
        };
        if packet.track_id != reader.track_id {
            continue;
        }
        let buffer = match reader.decoder.decode(&packet) {
            Ok(b) => b,
            // A corrupt packet: skipped, as symphonia advises (the
            // importer refused the file if it saw one, so this is
            // damage after the cook).
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e),
        };
        let source_channels = buffer.spec().channels().count().max(1);
        buffer.copy_to_vec_interleaved(&mut chunk);
        // Down to the clip's channel count (the importer refused anything
        // but mono and stereo, so this is a safety net), clamped.
        pending.clear();
        for frame in chunk.chunks_exact(source_channels) {
            for s in frame.iter().take(channels) {
                pending.push(s.clamp(-1.0, 1.0));
            }
            for _ in source_channels..channels {
                pending.push(frame[0].clamp(-1.0, 1.0));
            }
        }
        let skip = (reader.skip as usize * channels).min(pending.len());
        reader.skip -= (skip / channels) as u64;
        let mut rest: &[f32] = &pending[skip..];
        shared
            .produced
            .fetch_add((rest.len() / channels) as u64, Ordering::Relaxed);
        while !rest.is_empty() {
            if done(&producer) {
                return Ok(());
            }
            let (_, left) = producer.push_partial_slice(rest);
            rest = left;
            if !rest.is_empty() {
                thread::sleep(FULL_SLEEP);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PCM16 mono WAV whose sample `i` is `i / 32768`.
    fn ramp_wav(rate: u32, frames: u32) -> Vec<u8> {
        let data_len = frames * 2;
        let mut w = Vec::with_capacity(44 + data_len as usize);
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data_len).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * 2).to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            w.extend_from_slice(&((i % 32_768) as i16).to_le_bytes());
        }
        w
    }

    fn source(frames: u32) -> Arc<StreamSource> {
        Arc::new(StreamSource {
            codec: "pcm".into(),
            bytes: ramp_wav(8_000, frames).into(),
        })
    }

    /// Reads one frame, waiting for the decoder when it has not delivered
    /// (a test reads as fast as it can, so it outruns the decoder — at a
    /// loop's reopen, say — where a device callback, paced by the clock,
    /// would not: a starved read is retried here, not a failure).
    fn next(tap: &mut StreamTap) -> (TapRead, f32) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let (read, (l, _)) = tap.read(1.0, true);
            let empty = read == TapRead::Waiting || read == TapRead::Starved;
            if !empty || std::time::Instant::now() > deadline {
                return (read, l);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_stream_starts_where_asked_and_ends_when_the_frames_are_out() {
        let shared = StreamShared::new(false);
        let mut tap = spawn(source(1000), 8_000, 1, 600, shared.clone());
        let (read, l) = next(&mut tap);
        assert_eq!(read, TapRead::Frame);
        assert!(
            (l - 600.0 / 32_768.0).abs() < 1e-6,
            "started at {}",
            l * 32_768.0
        );
        for i in 601..1000 {
            let (read, l) = next(&mut tap);
            assert_eq!(read, TapRead::Frame);
            assert!(
                (l - i as f32 / 32_768.0).abs() < 1e-6,
                "frame {i}: {}",
                l * 32_768.0
            );
        }
        // Past the last frame: the end, once the decoder has said so.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let (read, _) = tap.read(1.0, true);
            if read == TapRead::Ended || std::time::Instant::now() > deadline {
                assert_eq!(read, TapRead::Ended);
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(shared.produced(), 400);
        assert!(!shared.failed());
    }

    #[test]
    fn a_looping_stream_wraps_to_its_first_frame_and_a_stop_ends_it() {
        let shared = StreamShared::new(true);
        let mut tap = spawn(source(500), 8_000, 1, 0, shared.clone());
        for i in 0..1200u32 {
            let (read, l) = next(&mut tap);
            assert_eq!(read, TapRead::Frame, "frame {i}");
            let expect = (i % 500) as f32 / 32_768.0;
            assert!((l - expect).abs() < 1e-6, "frame {i}: {}", l * 32_768.0);
        }
        assert!(shared.produced() >= 1200);
        shared.stop();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !shared.finished() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(shared.finished(), "the decoder exits on stop");
        drop(tap);
    }

    #[test]
    fn a_bad_source_fails_and_ends() {
        let shared = StreamShared::new(false);
        let source = Arc::new(StreamSource {
            codec: "pcm".into(),
            bytes: vec![0u8; 100].into(),
        });
        let mut tap = spawn(source, 8_000, 1, 0, shared.clone());
        let (read, _) = next(&mut tap);
        assert_eq!(read, TapRead::Ended);
        assert!(shared.failed());
    }
}
