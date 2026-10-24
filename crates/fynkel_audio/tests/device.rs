//! The real output, by ear: `cargo test -p fynkel_audio --test device --
//! --ignored` plays a rising chord for a second. Not run in CI, which
//! must not make a sound.

use std::sync::Arc;

use fynkel_audio::{Audio, Clip, Play};

#[test]
#[ignore = "plays through the speaker"]
fn a_chord_plays_through_the_default_output() {
    let mut audio = Audio::new();
    audio.open_device().unwrap();
    let stats = audio.stats();
    assert!(stats.device, "{stats:?}");
    let rate = 44_100;
    let clip = Arc::new(
        Clip::new(
            rate,
            1,
            (0..rate).map(|i| (i as f32 / rate as f32 * 330.0 * std::f32::consts::TAU).sin() * 0.2).collect(),
        )
        .unwrap(),
    );
    for pitch in [1.0, 1.25, 1.5] {
        audio
            .play(
                &clip,
                Play {
                    settings: fynkel_audio::Settings { volume: 0.5, pitch, looping: false, group: 0 },
                    ..Play::default()
                },
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        audio.update(0.15);
    }
    std::thread::sleep(std::time::Duration::from_millis(900));
    audio.update(0.9);
    let stats = audio.stats();
    eprintln!("{stats:?}");
    assert!(stats.callbacks > 10, "{stats:?}");
    assert_eq!(stats.voices, 0, "{stats:?}");

    // A device that plays, or is paused on purpose, isn't taken for one
    // that stopped by itself.
    for suspended in [false, true, false] {
        audio.suspend(suspended).unwrap();
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            audio.update(0.1);
        }
    }
    let stats = audio.stats();
    assert!(stats.device, "{stats:?}");
    assert_eq!(stats.reopened, 0, "{stats:?}");
}

/// The lab's chase track, streamed and looping, through the speaker for
/// `FYNKEL_STREAM_SECONDS` (600 unless set), quietly: never starved.
#[test]
#[ignore = "plays through the speaker, for ten minutes"]
fn a_streamed_track_plays_without_a_gap() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/lab/assets/music/chase.ogg");
    let clip = Arc::new(Clip::decode(&std::fs::read(path).unwrap(), "chase.ogg").unwrap());
    assert!(clip.streamed());
    let seconds: u64 = std::env::var("FYNKEL_STREAM_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(600);
    let mut audio = Audio::new();
    audio.open_device().unwrap();
    let settings = fynkel_audio::Settings { volume: 0.05, pitch: 1.0, looping: true, group: 0 };
    let handle = audio.play(&clip, Play { settings, ..Play::default() }).unwrap().unwrap();
    // As a game would: an update a frame.
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(16));
        audio.update(0.016);
    }
    let stats = audio.stats();
    eprintln!("{seconds} s: {stats:?}");
    assert!(audio.playing(handle));
    assert!(stats.callbacks > seconds * 20, "{stats:?}");
    assert_eq!(stats.starved, 0, "{stats:?}");
    assert_eq!(stats.underruns, 0, "{stats:?}");
}
