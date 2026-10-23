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
                Play { settings: fynkel_audio::Settings { volume: 0.5, pitch, looping: false }, ..Play::default() },
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
}
