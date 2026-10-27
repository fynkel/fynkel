//! Streamed clips (docs/stage-11.md §3.2): decoded as they play, a second
//! ahead, and heard exactly as the same clip decoded whole.

use std::sync::Arc;

use fynkel_audio::{Audio, Clip, Play, QUIET_RATE, STREAM_SECONDS, Settings, Streaming, wav};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// `seconds` of a rising tone, at 8 kHz, as a WAV.
fn long_wav(seconds: f64, channels: u16) -> Vec<u8> {
    let frames = (8_000.0 * seconds) as usize;
    let samples: Vec<f32> = (0..frames * usize::from(channels))
        .map(|i| {
            let t = (i / usize::from(channels)) as f32 / 8_000.0;
            0.5 * (t * (300.0 + 20.0 * t) * std::f32::consts::TAU).sin()
        })
        .collect();
    wav(8_000, channels, &samples)
}

/// What `clip` sounds like for `frames`, played as `play` says.
fn heard(clip: &Arc<Clip>, play: Play, frames: usize) -> Vec<f32> {
    let mut audio = Audio::new();
    audio.play(clip, play).unwrap().unwrap();
    let mut out = vec![0.0; frames * 2];
    audio.render(&mut out).unwrap();
    assert_eq!(audio.stats().starved, 0, "it ran out");
    out
}

fn same(a: &[f32], b: &[f32], what: &str) {
    assert_eq!(a.len(), b.len());
    let worst = a.iter().zip(b).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(worst < 1e-6, "{what}: streamed is off the decoded by up to {worst}");
}

#[test]
fn a_clip_longer_than_ten_seconds_streams_unless_told_not_to() {
    let long = long_wav(STREAM_SECONDS + 1.0, 1);
    let clip = Clip::decode(&long, "long.wav").unwrap();
    assert!(clip.streamed());
    assert_eq!(clip.frames(), 88_000);
    assert_eq!(clip.bytes(), long.len(), "a streamed clip holds its file, not its samples");
    assert!(clip.samples().is_empty());
    assert!(!Clip::load(&long, "long.wav", Streaming::Never).unwrap().streamed());
    let short = long_wav(2.0, 1);
    assert!(!Clip::decode(&short, "short.wav").unwrap().streamed());
    let forced = Clip::load(&short, "short.wav", Streaming::Always).unwrap();
    assert!(forced.streamed());
    assert_eq!((forced.rate(), forced.channels(), forced.frames()), (8_000, 1, 16_000));
}

#[test]
fn wav_ogg_and_mp3_stream_as_they_decode() {
    for name in ["tone.wav", "tone.ogg", "tone.mp3"] {
        let bytes = fixture(name);
        let whole = Arc::new(Clip::load(&bytes, name, Streaming::Never).unwrap());
        let streamed = Arc::new(Clip::load(&bytes, name, Streaming::Always).unwrap());
        assert_eq!(
            (streamed.rate(), streamed.channels(), streamed.frames()),
            (whole.rate(), whole.channels(), whole.frames()),
            "{name}"
        );
        // To past the end: both stop at the same frame.
        let frames = (whole.seconds() * f64::from(QUIET_RATE)) as usize + 2_000;
        same(&heard(&whole, Play::default(), frames), &heard(&streamed, Play::default(), frames), name);
    }
}

#[test]
fn a_streamed_clip_loops_without_a_gap_and_its_time_goes_round() {
    let bytes = long_wav(0.5, 2);
    let whole = Arc::new(Clip::load(&bytes, "loop.wav", Streaming::Never).unwrap());
    let streamed = Arc::new(Clip::load(&bytes, "loop.wav", Streaming::Always).unwrap());
    let looping = Play { settings: Settings { looping: true, ..Settings::default() }, ..Play::default() };
    // Two and a half times round, at another rate and pitch.
    let pitched = Play { settings: Settings { pitch: 1.3, ..looping.settings }, ..looping };
    same(&heard(&whole, pitched, 60_000), &heard(&streamed, pitched, 60_000), "looping");

    let mut audio = Audio::new();
    let handle = audio.play(&streamed, looping).unwrap().unwrap();
    audio.update(0.6);
    audio.update(0.6);
    let time = audio.time(handle).unwrap();
    assert!((time - 0.2).abs() < 0.03, "{time}");
}

#[test]
fn a_streamed_clip_starts_where_its_asked_to() {
    for name in ["tone.wav", "tone.ogg", "tone.mp3"] {
        let bytes = fixture(name);
        let whole = Arc::new(Clip::load(&bytes, name, Streaming::Never).unwrap());
        let streamed = Arc::new(Clip::load(&bytes, name, Streaming::Always).unwrap());
        let from = Play { from: 0.2, ..Play::default() };
        same(&heard(&whole, from, 20_000), &heard(&streamed, from, 20_000), name);
    }
}

#[test]
fn a_streamed_clip_that_stops_looping_plays_to_its_end() {
    let bytes = long_wav(0.25, 1);
    let streamed = Arc::new(Clip::load(&bytes, "x.wav", Streaming::Always).unwrap());
    let mut audio = Audio::new();
    let play = Play { settings: Settings { looping: true, ..Settings::default() }, ..Play::default() };
    let handle = audio.play(&streamed, play).unwrap().unwrap();
    audio.update(0.6);
    assert!(audio.playing(handle));
    audio.set(handle, Settings::default()).unwrap();
    // To the end of the time round it's in (0.6 s is 0.1 into the third),
    // though its decoder is a second ahead.
    audio.update(0.14);
    assert!(audio.playing(handle));
    audio.update(0.02);
    assert!(!audio.playing(handle));
    assert_eq!(audio.stopped().len(), 1);
}

#[test]
fn a_stream_its_decoder_cant_keep_up_with_is_counted() {
    let bytes = long_wav(3.0, 1);
    let streamed = Arc::new(Clip::load(&bytes, "slow.wav", Streaming::Always).unwrap());
    let mut audio = Audio::new();
    let handle = audio.play(&streamed, Play::default()).unwrap().unwrap();
    // Decoded once (a second's worth), then mixed for two without decoding.
    audio.decode_streams();
    let mut out = vec![0.0; 1024 * 2];
    for _ in 0..100 {
        audio.render_mixer(&mut out).unwrap();
    }
    assert!(audio.stats().starved > 0);
    // It waits where it ran out, and goes on once decoded.
    assert!(audio.playing(handle));
    let starved = audio.stats().starved;
    audio.render(&mut out).unwrap();
    assert_eq!(audio.stats().starved, starved);
}

#[test]
fn many_streams_of_one_clip_each_play_on_their_own() {
    let bytes = long_wav(1.0, 1);
    let streamed = Arc::new(Clip::load(&bytes, "x.wav", Streaming::Always).unwrap());
    let whole = Arc::new(Clip::load(&bytes, "x.wav", Streaming::Never).unwrap());
    let starts = [0.0, 0.3, 0.6];
    let mixed = |clip: &Arc<Clip>| {
        let mut audio = Audio::new();
        for from in starts {
            let play = Play { from, settings: Settings { volume: 0.3, ..Settings::default() }, ..Play::default() };
            audio.play(clip, play).unwrap().unwrap();
        }
        let mut out = vec![0.0; 40_000 * 2];
        audio.render(&mut out).unwrap();
        out
    };
    same(&mixed(&whole), &mixed(&streamed), "three at once");
}

#[test]
fn a_long_streamed_clip_starts_far_in_where_its_asked_to() {
    // Far enough in for a seek, rather than decoding from the start.
    let bytes = long_wav(12.0, 2);
    let whole = Arc::new(Clip::load(&bytes, "far.wav", Streaming::Never).unwrap());
    let streamed = Arc::new(Clip::load(&bytes, "far.wav", Streaming::Always).unwrap());
    let from = Play { from: 9.37, ..Play::default() };
    same(&heard(&whole, from, 20_000), &heard(&streamed, from, 20_000), "far in");
}

#[test]
fn ogg_and_mp3_start_far_in_where_theyre_asked_to() {
    // Made with ffmpeg: a 3-second rising tone, so a start a little off
    // shows.
    for name in ["chirp.ogg", "chirp.mp3"] {
        let bytes = fixture(name);
        let whole = Arc::new(Clip::load(&bytes, name, Streaming::Never).unwrap());
        let streamed = Arc::new(Clip::load(&bytes, name, Streaming::Always).unwrap());
        let from = Play { from: 2.1, ..Play::default() };
        same(&heard(&whole, from, 30_000), &heard(&streamed, from, 30_000), name);
    }
}

#[test]
fn a_streamed_sample_that_isnt_a_number_silences_only_its_own_voice() {
    // Stage 14's AU5: a float WAV holds what it likes, and a streamed one
    // isn't checked as it's decoded; one sample that isn't a number went
    // into the mix and silenced every sound.
    let mut samples = vec![0.25f32; 8_000];
    samples[100] = f32::NAN;
    samples[200] = f32::INFINITY;
    let data = samples.len() * 4;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&((36 + data) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    // IEEE floats, mono, 8 kHz, 32 bits.
    for (value, size) in [(3u32, 2), (1, 2), (8_000, 4), (32_000, 4), (4, 2), (32, 2)] {
        bytes.extend_from_slice(&value.to_le_bytes()[..size]);
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data as u32).to_le_bytes());
    for s in &samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    let clip = Arc::new(Clip::load(&bytes, "bad.wav", Streaming::Always).unwrap());
    let mut audio = Audio::new();
    audio.play(&clip, Play::default()).unwrap().unwrap();
    let other = Arc::new(Clip::new(QUIET_RATE, 1, vec![0.5; QUIET_RATE as usize]).unwrap());
    audio.play(&other, Play::default()).unwrap().unwrap();
    let mut out = vec![0.0; 4_800 * 2];
    audio.render(&mut out).unwrap();
    assert!(out.iter().all(|s| s.is_finite()), "the mix isn't a number");
    // The other plays on, and so does the bad one, after its bad samples.
    let last = out[out.len() - 2];
    assert!((last - 0.75).abs() < 0.01, "{last}");
}
