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
    /// Android, as `Playback`.
    VoiceChat,
}

/// Puts `session` into effect, before the device opens and whenever it
/// changes.
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
        let shared = AVAudioSession::sharedInstance();
        let missing = "the audio session has no such category";
        match session {
            Session::Ambient | Session::Playback => {
                let category = match session {
                    Session::Ambient => AVAudioSessionCategoryAmbient,
                    _ => AVAudioSessionCategoryPlayback,
                }
                .ok_or(missing)?;
                shared.setCategory_error(category)
            }
            // The default mode: `voiceChat` would turn the game's sound
            // down and colour it. Without `DefaultToSpeaker` the sound goes
            // to the earpiece. No preferred sample rate: the mic's reader
            // converts to its own.
            Session::VoiceChat => shared.setCategory_mode_options_error(
                AVAudioSessionCategoryPlayAndRecord.ok_or(missing)?,
                AVAudioSessionModeDefault.ok_or("the audio session has no default mode")?,
                AVAudioSessionCategoryOptions::DefaultToSpeaker
                    | AVAudioSessionCategoryOptions::AllowBluetoothHFP
                    | AVAudioSessionCategoryOptions::AllowBluetoothA2DP,
            ),
        }
        .map_err(|e| format!("the audio session's category: {e}"))?;
        shared.setActive_error(true).map_err(|e| format!("the audio session: {e}"))?;
    }
    Ok(())
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

/// Whether a phone call is on or ringing, when the sound should wait.
#[cfg(target_os = "android")]
pub(crate) fn in_call() -> bool {
    crate::android::in_call()
}

#[cfg(not(target_os = "android"))]
pub(crate) fn in_call() -> bool {
    false
}
