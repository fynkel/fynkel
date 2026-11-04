//! The audio session as iOS itself reports it, in the iOS Simulator
//! (`tools/test-mobile.sh ios`). Elsewhere there's no session to set, so a
//! test on the Mac can't tell whether a game that talks and listens (Ring
//! Ring Deals) gets what it asks for. Silent: CI must not make a sound.
#![cfg(target_os = "ios")]
#![allow(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use fynkel_audio::{Audio, Clip, Play, Session};
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionCategoryAmbient, AVAudioSessionCategoryOptions, AVAudioSessionCategoryPlayAndRecord,
    AVAudioSessionCategoryPlayback, AVAudioSessionModeDefault,
};

/// The category and options the system holds now, and whether its mode is
/// the default one (the `voiceChat` mode would turn the game's sound down).
fn held() -> (String, AVAudioSessionCategoryOptions, bool) {
    // SAFETY: the shared session's own getters, and a constant the
    // framework defines.
    unsafe {
        let shared = AVAudioSession::sharedInstance();
        (shared.category().to_string(), shared.categoryOptions(), &*shared.mode() == AVAudioSessionModeDefault.unwrap())
    }
}

/// Half a second of the device working: long enough to hear a stream lost.
fn play_on(audio: &mut Audio) {
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(50));
        audio.update(0.05);
    }
}

#[test]
fn each_session_is_what_the_system_holds_while_sound_plays() {
    let (ambient, playback, voice) = unsafe {
        (
            AVAudioSessionCategoryAmbient.unwrap().to_string(),
            AVAudioSessionCategoryPlayback.unwrap().to_string(),
            AVAudioSessionCategoryPlayAndRecord.unwrap().to_string(),
        )
    };
    let mut audio = Audio::new();
    // Before the device opens, as a game's `onLoad` does: it takes effect
    // when the device opens.
    audio.set_session(Session::VoiceChat).unwrap();
    audio.open_device().unwrap();
    assert!(audio.stats().device, "{:?}", audio.stats());
    let silence = Arc::new(Clip::new(44_100, 1, vec![0.0; 44_100]).unwrap());
    let settings = fynkel_audio::Settings { volume: 0.0, pitch: 1.0, looping: true, group: 0 };
    audio.play(&silence, Play { settings, ..Play::default() }).unwrap();
    play_on(&mut audio);

    // The mic works next to the game's sound; the sound comes from the
    // loudspeaker, not the earpiece, and in the default mode.
    let (category, options, default_mode) = held();
    assert_eq!(category, voice);
    assert!(options.contains(AVAudioSessionCategoryOptions::DefaultToSpeaker), "{options:?}");
    assert!(options.contains(AVAudioSessionCategoryOptions::AllowBluetoothHFP), "{options:?}");
    assert!(options.contains(AVAudioSessionCategoryOptions::AllowBluetoothA2DP), "{options:?}");
    assert!(default_mode);

    // Set again, as a world's second load does: left alone, the stream
    // goes on playing.
    let before = audio.stats().callbacks;
    audio.set_session(Session::VoiceChat).unwrap();
    for _ in 0..6 {
        play_on(&mut audio);
    }
    let stats = audio.stats();
    assert!(stats.callbacks > before, "{stats:?}");
    assert_eq!(stats.reopened, 0, "{stats:?}");

    // The others, and back, while it plays.
    for (session, category) in
        [(Session::Playback, &playback), (Session::Ambient, &ambient), (Session::VoiceChat, &voice)]
    {
        audio.set_session(session).unwrap();
        play_on(&mut audio);
        assert_eq!(&held().0, category, "{session:?}");
    }
    let before = audio.stats().callbacks;
    for _ in 0..6 {
        play_on(&mut audio);
    }
    let stats = audio.stats();
    assert!(stats.device && stats.callbacks > before, "{stats:?}");
}
