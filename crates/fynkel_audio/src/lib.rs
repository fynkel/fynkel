//! Fynkel's sound (docs/stage-11.md): clips decoded from WAV, OGG Vorbis
//! and MP3 ([`Clip`]); the game's side, which decides what plays
//! ([`Audio`]); and the mixer, which plays it on the audio thread without
//! allocating, freeing or waiting ([`Mixer`]); and 3D sound, how loud each
//! voice is in each ear from where it is around the [`Listener`] ([`mix`]). `crates/fynkel_audio_ffi` is
//! its C interface, and `package:fynkel_audio` the Dart one.

// JNI and dlsym, which are unsafe by nature, to give cpal the app's Java VM.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod android;
mod audio;
mod clip;
mod device;
mod mixer;
mod session;
mod spatial;

pub use audio::{Audio, Handle, LOWEST_PRIORITY, MAX_PITCH, MIN_PITCH, Play, QUIET_RATE, Stats, Stopped};
pub use clip::{Clip, MAX_CHANNELS, wav};
pub use mixer::{MAX_VOICES, Mixer, Settings};
pub use session::Session;
pub use spatial::{Listener, Mix, Rolloff, Spatial, mix, rolloff};
