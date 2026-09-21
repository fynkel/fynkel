//! The real device (`device` feature): opens the default output, plays a
//! short tone, checks the callbacks ran. Ignored by default (a machine
//! without an output device, CI); run it by hand with
//! `cargo test -p fynkel_audio --features device --test device -- --ignored`.

#![cfg(feature = "device")]

use std::sync::Arc;

use fynkel_audio::{Audio, AudioClip, PlayParams};
use fynkel_contracts::{AssetId, AssetRef};

#[test]
#[ignore]
fn the_default_output_device_plays() {
    let mut audio = Audio::new();
    let rate = 48_000;
    let samples: Vec<f32> = (0..rate / 4)
        .map(|i| (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.2)
        .collect();
    let clip: Arc<AudioClip> = AudioClip::new(rate, 1, samples).unwrap();
    let r = AssetRef::new(AssetId::from_bytes([1; 16]));
    audio.register_clip(r.clone(), clip).unwrap();
    let info = audio.open_device().expect("an output device");
    assert!(info.device);
    assert!(info.sample_rate > 0 && info.channels > 0);
    let id = audio.play(None, &r, PlayParams::default()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    let ended = audio.advance(0.4);
    assert_eq!(ended.len(), 1);
    assert!(!audio.is_playing(id));
    let stats = audio.stats();
    assert!(stats.device);
    assert!(stats.callbacks > 0, "{stats:?}");
    audio.suspend(true);
    audio.suspend(false);
    audio.close_device();
    assert!(!audio.has_device());
    println!("{info:?} {stats:?}");
}
