//! How the game's sound sits with the phone's (docs/stage-11.md §3.4): on
//! iOS, the audio session's category. Elsewhere there's nothing to set.
//! cpal follows interruptions (a call, an alarm) and new routes
//! (headphones) itself.

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

#[cfg(not(target_os = "ios"))]
pub(crate) fn apply(_session: Session) -> Result<(), String> {
    Ok(())
}
