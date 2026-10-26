//! How the game's sound sits with the phone's (docs/stage-11.md §3.4): on
//! iOS, the audio session's category; on Android, the audio focus.
//! Elsewhere there's nothing to set. On iOS cpal follows interruptions (a
//! call, an alarm) and new routes (headphones) itself; on Android a call
//! is watched for by `Audio::update`.

/// How the game's sound sits with the phone's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Session {
    /// The silent switch mutes the game, and the player's own music keeps
    /// playing under it: what most games do (iOS's `ambient` category).
    #[default]
    Ambient,
    /// The game plays with the silent switch on, and the player's music
    /// stops (`playback`): for a game whose sound is the point.
    Playback,
    /// The game talks and listens at once, like a call: the mic works
    /// alongside the game's sound (iOS's `playAndRecord`), which plays from
    /// the loudspeaker, not the earpiece, and ignores the silent switch. On
    /// Android, as `Playback`, but the phone's mode for a call over the
    /// internet (which the game's own voice chat sets) doesn't pause it: a
    /// phone call still does.
    VoiceChat,
}

/// Puts `session` into effect, before the device opens and whenever it
/// changes. A session that's already in effect is left alone: setting it
/// again posts a route change (for `VoiceChat`, the category's and the
/// loudspeaker's), which ends the stream just opened, and so on, a
/// reopen a second.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
pub(crate) fn apply(session: Session) -> Result<(), String> {
    use objc2_avf_audio::{
        AVAudioSession, AVAudioSessionCategoryAmbient, AVAudioSessionCategoryOptions,
        AVAudioSessionCategoryPlayAndRecord, AVAudioSessionCategoryPlayback, AVAudioSessionModeDefault,
    };

    // SAFETY: the categories and mode are constants the framework defines,
    // and the shared session may be used from any thread.
    unsafe {
        let missing = "the audio session has no such category";
        let (category, options) = match session {
            Session::Ambient => (AVAudioSessionCategoryAmbient.ok_or(missing)?, AVAudioSessionCategoryOptions::empty()),
            Session::Playback => {
                (AVAudioSessionCategoryPlayback.ok_or(missing)?, AVAudioSessionCategoryOptions::empty())
            }
            // Without `DefaultToSpeaker` the sound goes to the earpiece.
            Session::VoiceChat => (
                AVAudioSessionCategoryPlayAndRecord.ok_or(missing)?,
                AVAudioSessionCategoryOptions::DefaultToSpeaker
                    | AVAudioSessionCategoryOptions::AllowBluetoothHFP
                    | AVAudioSessionCategoryOptions::AllowBluetoothA2DP,
            ),
        };
        // The default mode: `voiceChat` would turn the game's sound down
        // and colour it. No preferred sample rate: the mic's reader
        // converts to its own.
        let mode = AVAudioSessionModeDefault.ok_or("the audio session has no default mode")?;
        let shared = AVAudioSession::sharedInstance();
        if &*shared.category() == category && &*shared.mode() == mode && shared.categoryOptions() == options {
            // Active too: the system activates a session with the sound
            // that starts in it, and cpal does again after an interruption.
            return Ok(());
        }
        shared
            .setCategory_mode_options_error(category, mode, options)
            .map_err(|e| format!("the audio session's category: {e}"))?;
        shared.setActive_error(true).map_err(|e| format!("the audio session: {e}"))?;
    }
    Ok(())
}

/// What the sound plays through: the route's outputs, the rate and the
/// channels. A route change that keeps them all (a category's, or the
/// loudspeaker's, with the loudspeaker before and after) leaves the stream
/// playing, so the device isn't lost.
#[cfg(target_os = "ios")]
#[derive(Debug, PartialEq)]
pub(crate) struct Output {
    ports: Vec<String>,
    rate: f64,
    channels: isize,
}

#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
pub(crate) fn output() -> Option<Output> {
    use objc2_avf_audio::AVAudioSession;

    // SAFETY: the shared session's own getters, from any thread.
    unsafe {
        let shared = AVAudioSession::sharedInstance();
        let ports = shared.currentRoute().outputs().iter().map(|port| port.portType().to_string()).collect();
        Some(Output { ports, rate: shared.sampleRate(), channels: shared.outputNumberOfChannels() })
    }
}

/// Elsewhere every invalidated stream is lost.
#[cfg(not(target_os = "ios"))]
#[derive(Debug, PartialEq)]
pub(crate) struct Output;

#[cfg(not(target_os = "ios"))]
pub(crate) fn output() -> Option<Output> {
    None
}

/// Asks for the audio focus for `Playback` and `VoiceChat`, so the
/// player's music stops; gives it up for `Ambient`, so it plays on under
/// the game. The mic works next to the game's sound either way.
#[cfg(target_os = "android")]
pub(crate) fn apply(session: Session) -> Result<(), String> {
    crate::android::prepare()?;
    // Sound without the focus is still sound: not a reason to go without.
    let _ = crate::android::focus(session != Session::Ambient);
    Ok(())
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn apply(_session: Session) -> Result<(), String> {
    Ok(())
}

/// Android's `AudioManager` modes.
const MODE_RINGTONE: i32 = 1;
const MODE_IN_CALL: i32 = 2;
const MODE_IN_COMMUNICATION: i32 = 3;

/// Whether the game's sound in `session` waits while the phone is in
/// `mode` (Android's `AudioManager.getMode`): a call ringing or on. A
/// game in `VoiceChat` talks itself, and its voice library (WebRTC's,
/// say) puts the phone in `MODE_IN_COMMUNICATION` while it does: not a
/// call to wait for. Since Android 11 the mode is the whole phone's, so
/// another app's call in that mode doesn't pause it either.
#[cfg_attr(not(any(test, target_os = "android")), allow(dead_code))]
fn waits_for(session: Session, mode: i32) -> bool {
    match mode {
        MODE_RINGTONE | MODE_IN_CALL => true,
        MODE_IN_COMMUNICATION => session != Session::VoiceChat,
        _ => false,
    }
}

/// Whether a phone call is on or ringing, when the sound in `session`
/// should wait.
#[cfg(target_os = "android")]
pub(crate) fn in_call(session: Session) -> bool {
    crate::android::mode().is_some_and(|mode| waits_for(session, mode))
}

#[cfg(not(target_os = "android"))]
pub(crate) fn in_call(_session: Session) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_chat_isnt_paused_by_its_own_call() {
        // Stage 14's AU6: a voice chat's library (WebRTC's) puts the phone
        // in MODE_IN_COMMUNICATION while it talks, which paused the game.
        assert!(!waits_for(Session::VoiceChat, MODE_IN_COMMUNICATION));
        // A phone call, ringing or on, still pauses it.
        assert!(waits_for(Session::VoiceChat, MODE_IN_CALL));
        assert!(waits_for(Session::VoiceChat, MODE_RINGTONE));
        // As every call pauses a game that doesn't talk itself.
        for session in [Session::Ambient, Session::Playback] {
            for mode in [MODE_RINGTONE, MODE_IN_CALL, MODE_IN_COMMUNICATION] {
                assert!(waits_for(session, mode), "{session:?} {mode}");
            }
        }
        // Not the phone's normal mode, nor one it doesn't say.
        assert!(!waits_for(Session::Ambient, 0));
        assert!(!waits_for(Session::Ambient, -2));
    }
}
