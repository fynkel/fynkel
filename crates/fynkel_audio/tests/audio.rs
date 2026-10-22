//! The game's side and the mixer together, without a device: what plays,
//! for how long, how loud, and which sound gives way.

use std::sync::Arc;

use fynkel_audio::{Audio, Clip, Handle, MAX_VOICES, Play, QUIET_RATE, Settings, Stopped};

const RATE: f64 = QUIET_RATE as f64;

/// `seconds` of a constant level, at `rate`.
fn level(rate: u32, seconds: f64, value: f32) -> Arc<Clip> {
    Arc::new(Clip::new(rate, 1, vec![value; (f64::from(rate) * seconds) as usize]).unwrap())
}

fn play(audio: &mut Audio, clip: &Arc<Clip>, play: Play) -> Handle {
    audio.play(clip, play).unwrap().expect("a voice")
}

/// Renders `frames` of stereo and returns them.
fn render(audio: &mut Audio, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    audio.render(&mut out).unwrap();
    out
}

/// How many frames until the voice stops playing, rendering in steps of 16.
fn frames_until_done(audio: &mut Audio, handle: Handle) -> usize {
    let mut frames = 0;
    while audio.playing(handle) {
        render(audio, 16);
        frames += 16;
        assert!(frames < 10 * QUIET_RATE as usize, "never ended");
    }
    frames
}

#[test]
fn a_sound_plays_at_its_volume_and_starts_without_a_click() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 1.0, 0.8);
    play(&mut audio, &clip, Play { settings: Settings { volume: 0.5, ..Settings::default() }, ..Play::default() });
    let out = render(&mut audio, 1024);
    assert!(out[0] < 0.1 && out[1] < 0.1, "the first frame jumps to {}", out[0]);
    // 5 ms in, the ramp is done: 0.8 × 0.5 in each ear.
    for &s in &out[2 * 480..] {
        assert!((s - 0.4).abs() < 0.005, "{s}");
    }
}

#[test]
fn a_sound_ends_after_its_length_and_says_so_once() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 0.1, 0.5);
    let handle = play(&mut audio, &clip, Play { owner: 7, ..Play::default() });
    assert!(audio.playing(handle));
    audio.update(0.05);
    assert!(audio.playing(handle));
    assert!(audio.stopped().is_empty());
    audio.update(0.06);
    assert!(!audio.playing(handle));
    assert_eq!(audio.stopped(), [Stopped { handle, owner: 7, stolen: false }]);
    audio.update(0.0);
    assert!(audio.stopped().is_empty());
}

#[test]
fn a_clip_at_another_rate_or_pitch_plays_for_its_own_time() {
    let mut audio = Audio::new();
    // A tenth of a second at 24 kHz plays a tenth of a second at 48.
    let clip = level(24_000, 0.1, 0.5);
    let handle = play(&mut audio, &clip, Play::default());
    let frames = frames_until_done(&mut audio, handle);
    assert!(frames.abs_diff(4_800) <= 16, "{frames}");
    // At pitch 2, half as long.
    let handle =
        play(&mut audio, &clip, Play { settings: Settings { pitch: 2.0, ..Settings::default() }, ..Play::default() });
    let frames = frames_until_done(&mut audio, handle);
    assert!(frames.abs_diff(2_400) <= 16, "{frames}");
}

#[test]
fn a_looping_sound_plays_on_until_stopped_and_a_stop_fades() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 0.01, 0.5);
    let handle = play(
        &mut audio,
        &clip,
        Play { settings: Settings { looping: true, ..Settings::default() }, ..Play::default() },
    );
    audio.update(1.0);
    assert!(audio.playing(handle));
    let time = audio.time(handle).unwrap();
    assert!((0.0..0.01).contains(&time), "{time}");

    audio.stop(handle).unwrap();
    // Not playing as soon as it's told, and not "stopped on its own".
    assert!(!audio.playing(handle));
    assert_eq!(audio.time(handle), None);
    let out = render(&mut audio, 480);
    assert!(out[0] > 0.4, "the fade starts where it was: {}", out[0]);
    assert!(out[out.len() - 1] < 0.01, "and ends silent: {}", out[out.len() - 1]);
    audio.update(0.0);
    assert!(audio.stopped().is_empty());
    assert_eq!(audio.stats().voices, 0);
}

#[test]
fn a_paused_sound_holds_its_place_and_goes_on_from_it() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 1.0, 0.5);
    let handle = play(&mut audio, &clip, Play::default());
    audio.update(0.25);
    audio.pause(handle, true).unwrap();
    audio.update(0.01);
    let held = audio.time(handle).unwrap();
    audio.update(2.0);
    assert!(audio.playing(handle));
    assert_eq!(audio.time(handle), Some(held));
    assert!(render(&mut audio, 64).iter().all(|&s| s == 0.0));
    audio.pause(handle, false).unwrap();
    audio.update(0.25);
    let time = audio.time(handle).unwrap();
    assert!((time - held - 0.25).abs() < 0.01, "{held} → {time}");
}

#[test]
fn the_game_paused_holds_its_sounds_but_not_the_menus() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 1.0, 0.5);
    let game = play(&mut audio, &clip, Play::default());
    let menu = play(&mut audio, &clip, Play { pausable: false, ..Play::default() });
    audio.set_game_paused(true).unwrap();
    audio.update(0.5);
    let (held, went_on) = (audio.time(game).unwrap(), audio.time(menu).unwrap());
    assert!(held < 0.01, "{held}");
    assert!(went_on > 0.49, "{went_on}");
    audio.set_game_paused(false).unwrap();
    audio.update(0.6);
    assert!(audio.playing(game));
    assert!(!audio.playing(menu));
}

#[test]
fn the_least_important_oldest_voice_gives_way_and_a_less_important_sound_doesnt_play() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 5.0, 0.01);
    let mut handles = Vec::new();
    for i in 0..MAX_VOICES {
        // One voice less important than the rest.
        let priority = if i == 5 { 200 } else { 128 };
        handles.push(play(&mut audio, &clip, Play { priority, owner: i as u64, ..Play::default() }));
    }
    audio.update(0.01);

    // Less important than every voice: refused.
    assert_eq!(audio.play(&clip, Play { priority: 201, ..Play::default() }).unwrap(), None);
    // As important as the least, or more: it takes that one.
    let first = play(&mut audio, &clip, Play { priority: 128, ..Play::default() });
    assert!(!audio.playing(handles[5]));
    // Then, all as important, the oldest.
    let second = play(&mut audio, &clip, Play { priority: 128, ..Play::default() });
    assert!(!audio.playing(handles[0]));
    assert_eq!(
        audio.stopped(),
        [
            Stopped { handle: handles[5], owner: 5, stolen: true },
            Stopped { handle: handles[0], owner: 0, stolen: true }
        ]
    );
    assert!(audio.playing(first) && audio.playing(second));
    let stats = audio.stats();
    assert_eq!((stats.started, stats.stolen, stats.refused), (34, 2, 1));
    audio.update(0.01);
    assert_eq!(audio.stats().voices, MAX_VOICES as u32);
}

#[test]
fn a_voice_stopping_gives_way_first() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 5.0, 0.01);
    let handles: Vec<_> =
        (0..MAX_VOICES).map(|_| play(&mut audio, &clip, Play { priority: 0, ..Play::default() })).collect();
    audio.stop(handles[9]).unwrap();
    // Every voice is as important as it can be, but one is fading out.
    let handle = play(&mut audio, &clip, Play { priority: 256, ..Play::default() });
    assert!(audio.playing(handle));
    assert_eq!(audio.stats().stolen, 0);
    assert!(audio.stopped().is_empty());
}

#[test]
fn an_owner_stops_all_its_sounds() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 1.0, 0.1);
    let a = play(&mut audio, &clip, Play { owner: 1, ..Play::default() });
    let b = play(&mut audio, &clip, Play { owner: 1, ..Play::default() });
    let c = play(&mut audio, &clip, Play { owner: 2, ..Play::default() });
    audio.stop_owner(1).unwrap();
    assert!(!audio.playing(a) && !audio.playing(b) && audio.playing(c));
}

#[test]
fn a_sound_can_start_part_way_and_its_settings_change_while_it_plays() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 1.0, 0.5);
    let handle = play(&mut audio, &clip, Play { from: 0.9, ..Play::default() });
    render(&mut audio, 16);
    assert!((audio.time(handle).unwrap() - 0.9).abs() < 0.001);
    audio.set(handle, Settings { volume: 0.25, pitch: 1.0, looping: false, group: 0 }).unwrap();
    let out = render(&mut audio, 1024);
    assert!((out[out.len() - 1] - 0.125).abs() < 0.005, "{}", out[out.len() - 1]);
    let frames = frames_until_done(&mut audio, handle);
    assert!(frames.abs_diff((0.1 * RATE) as usize - 1024 - 16) <= 16, "{frames}");
}

#[test]
fn the_whole_volume_scales_every_voice() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 1.0, 0.5);
    play(&mut audio, &clip, Play::default());
    audio.set_volume(0.5).unwrap();
    let out = render(&mut audio, 1024);
    assert!((out[out.len() - 1] - 0.25).abs() < 0.005);
}

#[test]
fn values_out_of_range_are_refused() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 1.0, 0.5);
    let with =
        |volume, pitch| Play { settings: Settings { volume, pitch, looping: false, group: 0 }, ..Play::default() };
    assert!(audio.play(&clip, with(1.5, 1.0)).unwrap_err().contains("volume 1.5"));
    assert!(audio.play(&clip, with(f32::NAN, 1.0)).unwrap_err().contains("volume NaN"));
    assert!(audio.play(&clip, with(1.0, 0.0)).unwrap_err().contains("pitch 0"));
    assert!(audio.play(&clip, with(1.0, 9.0)).unwrap_err().contains("pitch 9"));
    assert!(audio.play(&clip, Play { priority: 257, ..Play::default() }).unwrap_err().contains("priority 257"));
    assert!(audio.play(&clip, Play { from: -1.0, ..Play::default() }).unwrap_err().contains("-1 s"));
    assert!(audio.set_volume(-0.1).is_err());
    assert_eq!(audio.stats().started, 0);
}

#[test]
fn many_sounds_together_are_kept_within_full_scale() {
    let mut audio = Audio::new();
    let clip = level(QUIET_RATE, 1.0, 0.9);
    for _ in 0..4 {
        play(&mut audio, &clip, Play::default());
    }
    let out = render(&mut audio, 1024);
    assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
}
