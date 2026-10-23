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
}

/// Puts `session` into effect, before the device opens and whenever it
/// changes.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
pub(crate) fn apply(session: Session) -> Result<(), String> {
    use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryAmbient, AVAudioSessionCategoryPlayback};

    // SAFETY: the categories are constants the framework defines, and the
    // shared session may be used from any thread.
    unsafe {
        let category = match session {
            Session::Ambient => AVAudioSessionCategoryAmbient,
            Session::Playback => AVAudioSessionCategoryPlayback,
        }
        .ok_or("the audio session has no such category")?;
        let shared = AVAudioSession::sharedInstance();
        shared.setCategory_error(category).map_err(|e| format!("the audio session's category: {e}"))?;
        shared.setActive_error(true).map_err(|e| format!("the audio session: {e}"))?;
    }
    Ok(())
}

/// Asks for the audio focus for `Playback`, so the player's music stops;
/// gives it up for `Ambient`, so it plays on under the game.
#[cfg(target_os = "android")]
pub(crate) fn apply(session: Session) -> Result<(), String> {
    crate::android::prepare()?;
    // Sound without the focus is still sound: not a reason to go without.
    let _ = crate::android::focus(session == Session::Playback);
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
