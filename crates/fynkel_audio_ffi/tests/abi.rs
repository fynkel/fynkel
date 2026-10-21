//! The C interface, called the way `package:fynkel_audio` calls it.
//! Calling C functions is unsafe, so these tests allow it, like the crate.
#![allow(unsafe_code)]

use std::ffi::CStr;
use std::ptr;

use fynkel_audio_ffi::*;
use fynkel_cabi::status;

fn last_error() -> String {
    // SAFETY: valid until the next failed call on this thread.
    unsafe { CStr::from_ptr(fa_last_error()) }.to_str().unwrap().to_owned()
}

struct Audio(*mut FaAudio);

impl Drop for Audio {
    fn drop(&mut self) {
        // SAFETY: made by fa_audio_new, not used after.
        unsafe { fa_audio_free(self.0) };
    }
}

fn clip(rate: u32, samples: &[f32]) -> *mut FaClip {
    let mut clip = ptr::null_mut();
    // SAFETY: a readable slice and a writable out.
    let status = unsafe { fa_clip_new(rate, 1, samples.as_ptr(), samples.len(), &mut clip) };
    assert_eq!(status, FaStatus::Ok, "{}", last_error());
    clip
}

fn play(volume: f32) -> FaPlay {
    FaPlay {
        settings: FaSettings { volume, pitch: 1.0, looping: 0 },
        priority: 128,
        pausable: 1,
        owner: 3,
        from: 0.0,
        spatial: FaSpatial { blend: 0.0, pan: 0.0, min_distance: 1.0, max_distance: 500.0, rolloff: 0 },
        position: [0.0; 3],
    }
}

#[test]
fn the_statuses_both_interfaces_have_share_their_numbers() {
    assert_eq!(FaStatus::Ok as i32, status::OK);
    assert_eq!(FaStatus::InvalidValue as i32, status::INVALID_VALUE);
    assert_eq!(FaStatus::NullPointer as i32, status::NULL_POINTER);
    assert_eq!(FaStatus::WrongSize as i32, status::WRONG_SIZE);
    assert_eq!(FaStatus::Panic as i32, status::PANIC);
    assert_eq!(FaStatus::Poisoned as i32, status::POISONED);
}

#[test]
fn a_decoded_clip_plays_to_its_end_and_says_so() {
    let audio = Audio(fa_audio_new());
    let bytes = fynkel_audio::wav(24_000, 1, &[0.5; 2_400]);
    let name = "beep.wav";
    let mut clip = ptr::null_mut();
    // SAFETY: readable bytes and name, and a writable out.
    let status = unsafe { fa_clip_decode(bytes.as_ptr(), bytes.len(), name.as_ptr(), name.len(), &mut clip) };
    assert_eq!(status, FaStatus::Ok, "{}", last_error());
    let mut info = FaClipInfo::default();
    // SAFETY: a live clip and a writable out.
    assert_eq!(unsafe { fa_clip_info(clip, &mut info) }, FaStatus::Ok);
    assert_eq!((info.rate, info.channels, info.frames, info.bytes), (24_000, 1, 2_400, 9_600));
    assert!((info.seconds - 0.1).abs() < 1e-9);

    let mut handle = 0;
    // SAFETY: live audio and clip, a readable play and a writable out.
    assert_eq!(unsafe { fa_audio_play(audio.0, clip, &play(1.0), &mut handle) }, FaStatus::Ok, "{}", last_error());
    assert_ne!(handle, 0);
    // The game lets go of its clip; the voice keeps it.
    // SAFETY: made above, not used after.
    unsafe { fa_clip_free(clip) };

    let mut out = vec![0.0f32; 1_024 * 2];
    // SAFETY: live audio, and room for 1,024 stereo frames.
    assert_eq!(unsafe { fa_audio_render(audio.0, out.as_mut_ptr(), 1_024) }, FaStatus::Ok);
    assert!((out[out.len() - 1] - 0.5).abs() < 0.01);

    let mut voice = FaVoice::default();
    // SAFETY: live audio and a writable out.
    assert_eq!(unsafe { fa_voice_get(audio.0, handle, &mut voice) }, FaStatus::Ok);
    assert_eq!(voice.playing, 1);
    assert!((voice.time - 1_024.0 / 48_000.0).abs() < 1e-3, "{}", voice.time);

    let mut count = 0;
    // SAFETY: live audio and a writable out.
    assert_eq!(unsafe { fa_audio_update(audio.0, &0.2, &mut count) }, FaStatus::Ok);
    assert_eq!(count, 1);
    let mut stopped = vec![FaStopped::default(); count];
    // SAFETY: room for count.
    assert_eq!(unsafe { fa_stopped_read(audio.0, stopped.as_mut_ptr(), count) }, FaStatus::Ok);
    assert_eq!(stopped, [FaStopped { handle, owner: 3, stolen: 0 }]);
    // SAFETY: room for 0, which is the wrong size.
    assert_eq!(unsafe { fa_stopped_read(audio.0, stopped.as_mut_ptr(), 0) }, FaStatus::WrongSize);

    let mut stats = FaStats::default();
    // SAFETY: live audio and a writable out.
    assert_eq!(unsafe { fa_audio_stats(audio.0, &mut stats) }, FaStatus::Ok);
    assert_eq!((stats.device, stats.rate, stats.channels, stats.started, stats.voices), (0, 48_000, 2, 1, 0));
}

#[test]
fn bytes_that_arent_sound_cant_be_decoded_and_say_whose_they_are() {
    let bytes = b"hello";
    let name = "music/theme.mp3";
    let mut clip = ptr::null_mut();
    // SAFETY: readable bytes and name, and a writable out.
    let status = unsafe { fa_clip_decode(bytes.as_ptr(), bytes.len(), name.as_ptr(), name.len(), &mut clip) };
    assert_eq!(status, FaStatus::CantDecode);
    assert!(last_error().starts_with("music/theme.mp3: "), "{}", last_error());
    assert!(clip.is_null());
}

#[test]
fn bad_values_are_refused_and_say_why() {
    let audio = Audio(fa_audio_new());
    let clip = clip(48_000, &[0.1; 480]);
    let mut handle = 0;
    let mut bad = play(2.0);
    // SAFETY: live audio and clip, a readable play and a writable out.
    assert_eq!(unsafe { fa_audio_play(audio.0, clip, &bad, &mut handle) }, FaStatus::InvalidValue);
    assert!(last_error().contains("volume 2"), "{}", last_error());
    bad = FaPlay { pausable: 2, ..play(1.0) };
    // SAFETY: as above.
    assert_eq!(unsafe { fa_audio_play(audio.0, clip, &bad, &mut handle) }, FaStatus::InvalidValue);
    assert!(last_error().contains("pausable is 2"), "{}", last_error());
    bad = FaPlay { priority: 300, ..play(1.0) };
    // SAFETY: as above.
    assert_eq!(unsafe { fa_audio_play(audio.0, clip, &bad, &mut handle) }, FaStatus::InvalidValue);
    // SAFETY: live audio.
    assert_eq!(unsafe { fa_audio_session_set(audio.0, 5) }, FaStatus::InvalidValue);
    // SAFETY: a null clip is refused, not read.
    assert_eq!(unsafe { fa_audio_play(audio.0, ptr::null(), &play(1.0), &mut handle) }, FaStatus::NullPointer);
    // SAFETY: null settings are refused, not read.
    assert_eq!(unsafe { fa_voice_set(audio.0, 1, ptr::null()) }, FaStatus::NullPointer);
    assert_eq!(last_error(), "settings is null");
    // SAFETY: a null audio is refused, not read.
    assert_eq!(unsafe { fa_audio_volume_set(ptr::null_mut(), 1.0) }, FaStatus::NullPointer);
    assert_eq!(last_error(), "audio is null");
    // SAFETY: made above, not used after.
    unsafe { fa_clip_free(clip) };
}

#[test]
fn stopping_pausing_and_the_owners_reach_their_voices() {
    let audio = Audio(fa_audio_new());
    let clip = clip(48_000, &[0.1; 48_000]);
    let mut handles = [0u64; 2];
    for (i, handle) in handles.iter_mut().enumerate() {
        let p = FaPlay { owner: i as u64 + 1, ..play(1.0) };
        // SAFETY: live audio and clip, a readable play and a writable out.
        assert_eq!(unsafe { fa_audio_play(audio.0, clip, &p, handle) }, FaStatus::Ok);
    }
    let state = |handle| {
        let mut voice = FaVoice::default();
        // SAFETY: live audio and a writable out.
        unsafe { fa_voice_get(audio.0, handle, &mut voice) };
        voice
    };
    // SAFETY: live audio.
    unsafe {
        assert_eq!(fa_voice_pause(audio.0, handles[0], 1), FaStatus::Ok);
        assert_eq!(
            fa_voice_set(audio.0, handles[0], &FaSettings { volume: 0.5, pitch: 2.0, looping: 1 }),
            FaStatus::Ok
        );
        assert_eq!(fa_owner_stop(audio.0, 2), FaStatus::Ok);
        assert_eq!(fa_audio_game_paused_set(audio.0, 1), FaStatus::Ok);
    }
    assert_eq!(state(handles[0]).playing, 1);
    assert_eq!(state(handles[1]).playing, 0);
    // SAFETY: live audio.
    assert_eq!(unsafe { fa_voice_stop(audio.0, handles[0]) }, FaStatus::Ok);
    assert_eq!(state(handles[0]).playing, 0);
    // SAFETY: made above, not used after.
    unsafe { fa_clip_free(clip) };
}

#[test]
fn a_3d_voice_is_heard_from_where_the_frames_moves_put_it() {
    let audio = Audio(fa_audio_new());
    let clip = clip(48_000, &[0.5; 48_000]);
    let spatial = FaSpatial { blend: 1.0, pan: 0.0, min_distance: 1.0, max_distance: 50.0, rolloff: 0 };
    let p = FaPlay { spatial, position: [-3.0, 0.0, 0.0], ..play(1.0) };
    let mut handle = 0;
    // SAFETY: live audio and clip, a readable play and a writable out.
    assert_eq!(unsafe { fa_audio_play(audio.0, clip, &p, &mut handle) }, FaStatus::Ok, "{}", last_error());
    let mut out = vec![0.0f32; 2_048 * 2];
    let ears = |out: &[f32]| (out[out.len() - 2], out[out.len() - 1]);
    // SAFETY: live audio, and room for 2,048 stereo frames.
    assert_eq!(unsafe { fa_audio_render(audio.0, out.as_mut_ptr(), 2_048) }, FaStatus::Ok);
    let (l, r) = ears(&out);
    assert!(l > 0.1 && r.abs() < 1e-4, "{l} {r}");

    // The listener turns to face it (a half turn about +Y faces +Z; a
    // quarter to the left faces -X), and it's in front.
    let half = std::f32::consts::FRAC_PI_4;
    let listener = FaListener { position: [0.0; 3], rotation: [0.0, half.sin(), 0.0, half.cos()] };
    let seconds = 0.0;
    let mut stopped = 0;
    // SAFETY: live audio, a readable listener, none to place, and readable
    // and writable outs.
    unsafe {
        assert_eq!(fa_audio_place(audio.0, &listener, ptr::null(), 0), FaStatus::Ok, "{}", last_error());
        assert_eq!(fa_audio_update(audio.0, &seconds, &mut stopped), FaStatus::Ok);
        assert_eq!(fa_audio_render(audio.0, out.as_mut_ptr(), 2_048), FaStatus::Ok);
    }
    let (l, r) = ears(&out);
    assert!((l - r).abs() < 1e-4 && l > 0.1, "{l} {r}");

    // Moved to the listener's right.
    let place = FaPlace { voice: handle, position: [0.0, 0.0, -3.0] };
    // SAFETY: as above, with one place.
    unsafe {
        assert_eq!(fa_audio_place(audio.0, &listener, &place, 1), FaStatus::Ok);
        assert_eq!(fa_audio_update(audio.0, &seconds, &mut stopped), FaStatus::Ok);
        assert_eq!(fa_audio_render(audio.0, out.as_mut_ptr(), 2_048), FaStatus::Ok);
    }
    let (l, r) = ears(&out);
    assert!(r > 0.1 && l.abs() < 1e-4, "{l} {r}");

    // And made 2D, panned hard left, at once.
    let flat = FaSpatial { blend: 0.0, pan: -1.0, ..spatial };
    // SAFETY: live audio and a readable spatial.
    unsafe {
        assert_eq!(fa_voice_spatial_set(audio.0, handle, &flat), FaStatus::Ok);
        assert_eq!(fa_audio_render(audio.0, out.as_mut_ptr(), 2_048), FaStatus::Ok);
    }
    let (l, r) = ears(&out);
    assert!((l - 0.5).abs() < 1e-3 && r.abs() < 1e-4, "{l} {r}");
    // SAFETY: made above, not used after.
    unsafe { fa_clip_free(clip) };
}

#[test]
fn bad_3d_values_are_refused_and_say_why() {
    let audio = Audio(fa_audio_new());
    let clip = clip(48_000, &[0.1; 480]);
    let mut handle = 0;
    let bad = FaPlay { spatial: FaSpatial { rolloff: 9, ..play(1.0).spatial }, ..play(1.0) };
    // SAFETY: live audio and clip, a readable play and a writable out.
    assert_eq!(unsafe { fa_audio_play(audio.0, clip, &bad, &mut handle) }, FaStatus::InvalidValue);
    assert!(last_error().contains("rolloff 9"), "{}", last_error());
    let listener = FaListener { position: [f32::INFINITY, 0.0, 0.0], rotation: [0.0, 0.0, 0.0, 1.0] };
    // SAFETY: live audio, a readable listener, none to place.
    assert_eq!(unsafe { fa_audio_place(audio.0, &listener, ptr::null(), 0) }, FaStatus::InvalidValue);
    // SAFETY: a null listener and spatial are refused, not read.
    unsafe {
        assert_eq!(fa_audio_place(audio.0, ptr::null(), ptr::null(), 0), FaStatus::NullPointer);
        assert_eq!(fa_audio_place(audio.0, &FaListener::default(), ptr::null(), 2), FaStatus::NullPointer);
        assert_eq!(fa_voice_spatial_set(audio.0, 1, ptr::null()), FaStatus::NullPointer);
    }
    // SAFETY: made above, not used after.
    unsafe { fa_clip_free(clip) };
}
