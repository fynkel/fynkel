//! 3D sound (docs/stage-11.md §3.3): how loud each ear hears a voice from
//! where it is, and the audio goldens: a tone rendered left, right, near,
//! far and behind, and one that flies past, each compared with a WAV in
//! tests/goldens (16-bit, so any player plays it). `FYNKEL_BLESS=1` writes
//! them again.

use std::f32::consts::{FRAC_PI_2, PI};
use std::path::PathBuf;
use std::sync::Arc;

use fynkel_audio::{Audio, Clip, Listener, Mix, Play, QUIET_RATE, Rolloff, Settings, Spatial, mix, rolloff, wav};

const THREE_D: Spatial =
    Spatial { blend: 1.0, pan: 0.0, min_distance: 1.0, max_distance: 50.0, rolloff: Rolloff::Logarithmic };

fn ears(at: [f32; 3]) -> (f32, f32) {
    mix(&THREE_D, at, &Listener::default()).ears()
}

#[test]
fn a_2d_sound_is_the_same_wherever_it_is() {
    let flat = Spatial::default();
    for at in [[0.0, 0.0, 0.0], [-30.0, 2.0, 5.0], [400.0, 0.0, -1.0]] {
        assert_eq!(mix(&flat, at, &Listener::default()), Mix::STRAIGHT);
    }
}

#[test]
fn a_2d_pan_moves_it_between_the_ears_keeping_the_near_one_full() {
    let pan = |pan| mix(&Spatial { pan, ..Spatial::default() }, [0.0; 3], &Listener::default()).gains;
    assert_eq!(pan(-1.0)[0], 1.0);
    assert!(pan(-1.0)[3] < 1e-6);
    assert!(pan(1.0)[0] < 1e-6);
    assert_eq!(pan(1.0)[3], 1.0);
    let half = pan(0.5);
    assert_eq!(half[3], 1.0);
    assert!((half[0] - 2f32.sqrt() * (0.375 * PI).cos()).abs() < 1e-6, "{half:?}");
}

#[test]
fn a_3d_sound_is_louder_in_the_ear_it_is_beside() {
    let (l, r) = ears([-3.0, 0.0, 0.0]);
    assert!(l > 0.3 && r < 1e-6, "{l} {r}");
    let (l, r) = ears([3.0, 0.0, 0.0]);
    assert!(r > 0.3 && l < 1e-6, "{l} {r}");
    // In front, the same in both, at full volume within the min distance.
    let (l, r) = ears([0.0, 0.0, -0.5]);
    assert!((l - 1.0).abs() < 1e-6 && (r - 1.0).abs() < 1e-6, "{l} {r}");
}

#[test]
fn the_listener_turning_turns_what_it_hears() {
    // Turned a quarter to the left (about +Y), what was on its left is in
    // front.
    let half = FRAC_PI_2 / 2.0;
    let turned = Listener { position: [0.0; 3], rotation: [0.0, half.sin(), 0.0, half.cos()] };
    let (l, r) = mix(&THREE_D, [-3.0, 0.0, 0.0], &turned).ears();
    assert!((l - r).abs() < 1e-5, "{l} {r}");
    // Moved, it hears from where it is.
    let moved = Listener { position: [-6.0, 0.0, 0.0], ..Listener::default() };
    let (l, r) = mix(&THREE_D, [-3.0, 0.0, 0.0], &moved).ears();
    assert!(r > 0.3 && l < 1e-6, "{l} {r}");
}

#[test]
fn behind_is_quieter_and_duller_than_in_front() {
    let front = mix(&THREE_D, [0.0, 0.0, -3.0], &Listener::default());
    let back = mix(&THREE_D, [0.0, 0.0, 3.0], &Listener::default());
    assert_eq!(front.dull, 0.0);
    assert_eq!(back.dull, 1.0);
    assert!((back.ears().0 / front.ears().0 - 0.7).abs() < 1e-5);
}

#[test]
fn the_rolloffs_are_unitys() {
    let log = Spatial { min_distance: 2.0, max_distance: 20.0, ..THREE_D };
    assert_eq!(rolloff(&log, 1.0), 1.0);
    assert_eq!(rolloff(&log, 4.0), 0.5);
    assert_eq!(rolloff(&log, 20.0), 0.1);
    // Past the max distance a logarithmic sound stops falling.
    assert_eq!(rolloff(&log, 100.0), 0.1);
    let linear = Spatial { rolloff: Rolloff::Linear, ..log };
    assert_eq!(rolloff(&linear, 2.0), 1.0);
    assert_eq!(rolloff(&linear, 11.0), 0.5);
    assert_eq!(rolloff(&linear, 20.0), 0.0);
    assert_eq!(rolloff(&linear, 30.0), 0.0);
}

#[test]
fn a_half_blend_is_halfway_between() {
    let half = Spatial { blend: 0.5, ..THREE_D };
    let m = mix(&half, [5.0, 0.0, 0.0], &Listener::default());
    let (flat, around) = (Mix::STRAIGHT, mix(&THREE_D, [5.0, 0.0, 0.0], &Listener::default()));
    for i in 0..4 {
        assert!((m.gains[i] - (flat.gains[i] + around.gains[i]) / 2.0).abs() < 1e-6);
    }
}

#[test]
fn bad_values_are_refused_and_say_why() {
    let mut audio = Audio::new();
    let clip = tone(0.1);
    let bad = |spatial| Play { spatial, ..Play::default() };
    let err = audio.play(&clip, bad(Spatial { blend: 1.5, ..THREE_D })).unwrap_err();
    assert!(err.contains("spatial blend 1.5"), "{err}");
    let err = audio.play(&clip, bad(Spatial { max_distance: 0.5, ..THREE_D })).unwrap_err();
    assert!(err.contains("max distance 0.5"), "{err}");
    let err = audio.play(&clip, bad(Spatial { pan: -2.0, ..THREE_D })).unwrap_err();
    assert!(err.contains("pan -2"), "{err}");
    let err = audio.play(&clip, Play { position: [f32::NAN, 0.0, 0.0], ..Play::default() }).unwrap_err();
    assert!(err.contains("can't be at"), "{err}");
    let err = audio.set_listener(Listener { rotation: [0.0; 4], ..Listener::default() }).unwrap_err();
    assert!(err.contains("rotation"), "{err}");
}

#[test]
fn a_moved_voice_is_heard_from_its_new_place_after_the_update() {
    let mut audio = Audio::new();
    let clip = tone(1.0);
    let handle =
        audio.play(&clip, Play { spatial: THREE_D, position: [-3.0, 0.0, 0.0], ..Play::default() }).unwrap().unwrap();
    assert!(audio.mix(handle).unwrap().ears().0 > 0.3);
    audio.place(handle, [3.0, 0.0, 0.0]).unwrap();
    assert!(audio.mix(handle).unwrap().ears().0 > 0.3, "not before the update");
    audio.update(0.0);
    assert!(audio.mix(handle).unwrap().ears().0 < 1e-6);
    // The listener walking past it brings it to the other ear.
    audio.set_listener(Listener { position: [6.0, 0.0, 0.0], ..Listener::default() }).unwrap();
    audio.update(0.0);
    assert!(audio.mix(handle).unwrap().ears().0 > 0.3);
}

#[test]
fn a_sound_further_than_a_float_holds_is_heard_far_away_not_as_nan() {
    // Stage 14's AU5: each place a float, but their difference past what
    // one holds, gave a mix that isn't a number, and every sound went
    // silent with it.
    let far = Listener { position: [-3e38, 0.0, 0.0], ..Listener::default() };
    let heard = mix(&THREE_D, [3e38, 0.0, 0.0], &far);
    assert!(heard.gains.iter().all(|g| g.is_finite()) && heard.dull.is_finite(), "{heard:?}");
    // As far as can be: the max distance's, from the right.
    let (left, right) = heard.ears();
    assert!(left < 1e-6 && (right - 1.0 / 50.0).abs() < 1e-4, "{left} {right}");
    let mut audio = Audio::new();
    audio.set_listener(far).unwrap();
    audio.play(&tone(1.0), Play { spatial: THREE_D, position: [3e38, 0.0, 0.0], ..Play::default() }).unwrap();
    let near = Arc::new(Clip::new(QUIET_RATE, 1, vec![0.25; QUIET_RATE as usize]).unwrap());
    audio.play(&near, Play::default()).unwrap();
    let mut out = vec![0.0; 2048];
    audio.render(&mut out).unwrap();
    assert!(out.iter().all(|s| s.is_finite()), "the mix isn't a number");
    assert!((out[out.len() - 2] - 0.25).abs() < 0.02, "{}", out[out.len() - 2]);
}

// --- Goldens -------------------------------------------------------------------

/// A 440 Hz tone, `seconds` long, at half volume, which loops without a
/// seam.
fn tone(seconds: f64) -> Arc<Clip> {
    let frames = (f64::from(QUIET_RATE) * seconds) as usize;
    let samples = (0..frames).map(|i| 0.5 * (2.0 * PI * 440.0 * i as f32 / QUIET_RATE as f32).sin()).collect();
    Arc::new(Clip::new(QUIET_RATE, 1, samples).unwrap())
}

/// Plays the tone from `at` and renders `frames`, after the start's ramp.
fn heard_from(at: [f32; 3], frames: usize) -> Vec<f32> {
    let mut audio = Audio::new();
    let clip = tone(1.0);
    let play = Play {
        spatial: THREE_D,
        position: at,
        settings: Settings { looping: true, ..Settings::default() },
        ..Play::default()
    };
    audio.play(&clip, play).unwrap().unwrap();
    let mut out = vec![0.0; frames * 2];
    audio.render(&mut out).unwrap();
    out
}

fn rms(out: &[f32], channel: usize) -> f32 {
    let skip = 480; // the start's ramp
    let side: Vec<f32> = out.chunks(2).skip(skip).map(|f| f[channel]).collect();
    (side.iter().map(|s| s * s).sum::<f32>() / side.len() as f32).sqrt()
}

/// Compares `out` with the golden `name`.wav, within a 16-bit step or two;
/// writes it with `FYNKEL_BLESS=1`.
fn golden(name: &str, out: &[f32]) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens").join(format!("{name}.wav"));
    if std::env::var_os("FYNKEL_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, wav(QUIET_RATE, 2, out)).unwrap();
        return;
    }
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("no golden {path:?}: run with FYNKEL_BLESS=1"));
    let want = Clip::decode(&bytes, name).unwrap();
    assert_eq!(want.samples().len(), out.len(), "{name}: a different length");
    let worst = want.samples().iter().zip(out).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(worst < 2e-3, "{name}: off the golden by up to {worst}");
}

#[test]
fn the_goldens_left_right_near_far_and_behind() {
    let frames = 12_000;
    let left = heard_from([-3.0, 0.0, 0.0], frames);
    let right = heard_from([3.0, 0.0, 0.0], frames);
    let near = heard_from([0.0, 0.0, -1.0], frames);
    let far = heard_from([0.0, 0.0, -10.0], frames);
    let behind = heard_from([0.0, 0.0, 1.0], frames);
    // A sine of 0.5 has an RMS of 0.354.
    let full = 0.5 / 2f32.sqrt();
    assert!(rms(&left, 0) > 0.3 * full && rms(&left, 1) < 1e-4);
    assert!(rms(&right, 1) > 0.3 * full && rms(&right, 0) < 1e-4);
    assert!((rms(&near, 0) - full).abs() < 0.01 && (rms(&near, 1) - full).abs() < 0.01);
    assert!((rms(&far, 0) - full / 10.0).abs() < 0.003, "{}", rms(&far, 0));
    // Quieter (0.7), and its 440 Hz a little lower still through the cut.
    assert!(rms(&behind, 0) < 0.7 * full && rms(&behind, 0) > 0.6 * full, "{}", rms(&behind, 0));
    for (name, out) in [("left", &left), ("right", &right), ("near", &near), ("far", &far), ("behind", &behind)] {
        golden(&format!("spatial_{name}"), out);
    }
}

#[test]
fn a_fast_source_flying_past_doesnt_crackle() {
    // From 20 m on the left to 20 m on the right in a second, 2 m in
    // front, moved at 30 frames a second: the mix jumps a lot each frame.
    let mut audio = Audio::new();
    let clip = tone(1.0);
    let play = Play {
        spatial: THREE_D,
        position: [-20.0, 0.0, -2.0],
        settings: Settings { looping: true, ..Settings::default() },
        ..Play::default()
    };
    let handle = audio.play(&clip, play).unwrap().unwrap();
    let mut out = Vec::new();
    let mut chunk = vec![0.0; 1600 * 2];
    for frame in 0..30 {
        audio.place(handle, [-20.0 + 40.0 * frame as f32 / 29.0, 0.0, -2.0]).unwrap();
        audio.update(0.0);
        audio.render(&mut chunk).unwrap();
        out.extend_from_slice(&chunk);
    }
    // A 440 Hz sine of 0.5 moves at most 0.029 a sample; a click would
    // jump far more.
    let worst = out
        .chunks(2)
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs()))
        .fold(0.0, f32::max);
    assert!(worst < 0.035, "a jump of {worst}");
    golden("spatial_fly_past", &out);
}
