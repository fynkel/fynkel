//! The mix (docs/stage-11.md, phase 3): fades, groups under the master
//! volume, and the crossfade's golden (tests/goldens/crossfade.wav;
//! `FYNKEL_BLESS=1` writes it again).

use std::path::PathBuf;
use std::sync::Arc;

use fynkel_audio::{Audio, Clip, MAX_GROUPS, Play, QUIET_RATE, Settings, wav};

fn level(seconds: f64, value: f32) -> Arc<Clip> {
    Arc::new(Clip::new(QUIET_RATE, 1, vec![value; (f64::from(QUIET_RATE) * seconds) as usize]).unwrap())
}

fn tone(hertz: f32, seconds: f32) -> Arc<Clip> {
    let frames = (QUIET_RATE as f32 * seconds) as usize;
    let samples = (0..frames).map(|i| 0.5 * (i as f32 / QUIET_RATE as f32 * hertz * std::f32::consts::TAU).sin());
    Arc::new(Clip::new(QUIET_RATE, 1, samples.collect()).unwrap())
}

fn render(audio: &mut Audio, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    audio.render(&mut out).unwrap();
    out
}

fn in_group(group: u8) -> Play {
    Play { settings: Settings { looping: true, group, ..Settings::default() }, ..Play::default() }
}

#[test]
fn a_groups_volume_reaches_its_voices_within_one_callback() {
    let mut audio = Audio::new();
    let clip = level(1.0, 0.8);
    audio.play(&clip, in_group(3)).unwrap().unwrap();
    audio.play(&clip, in_group(0)).unwrap().unwrap();
    render(&mut audio, 2_048);
    audio.set_group(3, 0.25, 0.0).unwrap();
    // One callback of 512: a line from 0.8 + 0.8 to 0.2 + 0.8, without a
    // jump, there by its end.
    let out = render(&mut audio, 512);
    assert!((out[0] - 1.0).abs() < 0.01, "clipped at 1: {}", out[0]);
    assert!((out[2 * 511] - 1.0).abs() < 0.01, "{}", out[2 * 511]);
    let after = render(&mut audio, 512);
    assert!((after[0] - 1.0).abs() < 0.01 && (after[2 * 511] - 1.0).abs() < 0.01);
    // Alone, so it isn't clipped.
    let mut audio = Audio::new();
    audio.play(&clip, in_group(3)).unwrap().unwrap();
    render(&mut audio, 2_048);
    audio.set_group(3, 0.25, 0.0).unwrap();
    let out = render(&mut audio, 512);
    let worst_step =
        out.chunks(2).collect::<Vec<_>>().windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0, f32::max);
    assert!(worst_step < 0.01, "a jump of {worst_step}");
    assert!((out[2 * 511] - 0.2).abs() < 0.002, "{}", out[2 * 511]);
    let after = render(&mut audio, 512);
    assert!(after.iter().all(|s| (s - 0.2).abs() < 0.002));
}

#[test]
fn a_group_fades_over_its_seconds_under_the_master_volume() {
    let mut audio = Audio::new();
    let clip = level(3.0, 0.8);
    audio.play(&clip, in_group(1)).unwrap().unwrap();
    audio.set_volume(0.5).unwrap();
    render(&mut audio, 4_800);
    audio.set_group(1, 0.0, 1.0).unwrap();
    let out = render(&mut audio, QUIET_RATE as usize);
    let at = |second: f32| out[2 * ((QUIET_RATE as f32 * second) as usize).min(QUIET_RATE as usize - 1)];
    assert!((at(0.0) - 0.4).abs() < 0.01, "{}", at(0.0));
    assert!((at(0.5) - 0.2).abs() < 0.01, "{}", at(0.5));
    assert!(at(1.0) < 0.01, "{}", at(1.0));
    // A group at 0 keeps its voices, silent; brought back, they're heard.
    assert_eq!(audio.stats().voices, 1);
    audio.set_group(1, 1.0, 0.0).unwrap();
    render(&mut audio, 512);
    assert!((render(&mut audio, 16)[0] - 0.4).abs() < 0.01);
}

#[test]
fn a_voice_fades_in_and_fades_out_to_its_stop() {
    let mut audio = Audio::new();
    let clip = level(3.0, 0.5);
    let handle = audio.play(&clip, Play { fade_in: 0.5, ..Play::default() }).unwrap().unwrap();
    let out = render(&mut audio, QUIET_RATE as usize);
    assert!((out[2 * 12_000] - 0.25).abs() < 0.01, "a quarter of a second in: {}", out[2 * 12_000]);
    assert!((out[2 * 30_000] - 0.5).abs() < 0.005);
    audio.fade(handle, 0.0, 0.25, true).unwrap();
    assert!(!audio.playing(handle), "it's stopping");
    let out = render(&mut audio, 6_000);
    assert!((out[2 * 6_000 - 2] - 0.25).abs() < 0.01, "half way: {}", out[2 * 6_000 - 2]);
    render(&mut audio, 6_000 + 1_024);
    audio.update(0.0);
    assert_eq!(audio.stats().voices, 0, "it ended");
}

#[test]
fn bad_fades_and_groups_are_refused() {
    let mut audio = Audio::new();
    let clip = level(0.1, 0.5);
    let handle = audio.play(&clip, Play::default()).unwrap().unwrap();
    assert!(audio.fade(handle, 1.5, 1.0, false).unwrap_err().contains("between 0 and 1"));
    assert!(audio.fade(handle, 0.5, -1.0, false).unwrap_err().contains("fade"));
    assert!(audio.fade(handle, 0.5, f32::NAN, false).is_err());
    assert!(audio.set_group(0, 0.5, 0.0).unwrap_err().contains("group 0"));
    assert!(audio.set_group(MAX_GROUPS as u8, 0.5, 0.0).is_err());
    let play = Play { settings: Settings { group: MAX_GROUPS as u8, ..Settings::default() }, ..Play::default() };
    assert!(audio.play(&clip, play).is_err());
}

#[test]
fn the_crossfade_golden() {
    // One track for half a second, then over a second to the other: the
    // first fades out to its stop as the second fades in, in a group of
    // music at 0.8.
    let mut audio = Audio::new();
    let (a, b) = (tone(330.0, 4.0), tone(495.0, 4.0));
    // The group's volume is there after the next callback.
    audio.set_group(2, 0.8, 0.0).unwrap();
    render(&mut audio, 512);
    let first = audio.play(&a, in_group(2)).unwrap().unwrap();
    let mut out = render(&mut audio, 24_000);
    audio.fade(first, 0.0, 1.0, true).unwrap();
    audio.play(&b, Play { fade_in: 1.0, ..in_group(2) }).unwrap().unwrap();
    out.extend(render(&mut audio, 60_000));
    // Halfway through, both at half.
    let rms = |from: usize, to: usize| {
        (out[from * 2..to * 2].iter().step_by(2).map(|s| s * s).sum::<f32>() / (to - from) as f32).sqrt()
    };
    let full = 0.8 * 0.5 / 2f32.sqrt();
    assert!((rms(4_800, 24_000) - full).abs() < 0.01, "{}", rms(4_800, 24_000));
    assert!((rms(70_000, 84_000) - full).abs() < 0.01, "{}", rms(70_000, 84_000));
    golden("crossfade", &out);
}

fn golden(name: &str, out: &[f32]) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens").join(format!("{name}.wav"));
    if std::env::var_os("FYNKEL_BLESS").is_some() {
        std::fs::write(&path, wav(QUIET_RATE, 2, out)).unwrap();
        return;
    }
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("no golden {path:?}: run with FYNKEL_BLESS=1"));
    let want = Clip::decode(&bytes, name).unwrap();
    assert_eq!(want.samples().len(), out.len(), "{name}: a different length");
    let worst = want.samples().iter().zip(out).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(worst < 2e-3, "{name}: off the golden by up to {worst}");
}
