//! The C interface (`fa_*`) that `package:fynkel_audio` calls through
//! `dart:ffi`: clips decoded from files, voices played from them, and the
//! device they play through (crates/fynkel_audio, docs/stage-11.md).
//! `include/fynkel_audio.h` is generated from this file by cbindgen.
//!
//! As in the engine's and the physics' interfaces (crates/fynkel_cabi):
//! every call that can fail returns an [`FaStatus`] and writes its results
//! through out-pointers, a voice's handle is a `uint64_t` with 0 meaning
//! "none", and a panic never crosses into the caller. An `FaAudio` is used
//! from one thread at a time; clips may be decoded on any thread, and
//! freed on any.
//!
//! This is the only audio crate where unsafe code is allowed.
#![allow(unsafe_code)]

use std::ffi::c_char;
use std::sync::Arc;

use fynkel_audio::{Audio, Clip, Listener, Play, Rolloff, Session, Settings, Spatial};

/// Bumped on any change to the functions or types in this file.
pub const FA_ABI_VERSION: u32 = 3;

/// What a call did: `Ok`, or why not, with a message for `fa_last_error`.
/// Those the engine's interface has too have its numbers
/// (`fynkel_cabi::status`; tests/abi.rs checks).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaStatus {
    Ok = 0,
    /// There's no sound output, or it couldn't be opened. Sound goes on
    /// without one.
    NoDevice = 1,
    /// The bytes aren't a WAV, OGG Vorbis or MP3 file this can decode.
    CantDecode = 2,
    /// A value was out of range, not finite, or not one of the choices.
    InvalidValue = 6,
    /// A pointer argument was null.
    NullPointer = 7,
    /// A buffer had the wrong size.
    WrongSize = 11,
    /// The call panicked. Nothing crossed into the caller; the message says
    /// what happened.
    Panic = 12,
    /// An earlier call panicked while changing this audio, so it can't be
    /// trusted any more.
    Poisoned = 13,
}

impl fynkel_cabi::Status for FaStatus {
    const OK: Self = Self::Ok;
    const PANIC: Self = Self::Panic;
    const NULL_POINTER: Self = Self::NullPointer;
}

/// How the game's sound sits with the phone's (iOS; elsewhere it does
/// nothing).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaSession {
    /// The silent switch mutes the game; the player's music plays on.
    Ambient = 0,
    /// The game plays with the silent switch on; the player's music stops.
    Playback = 1,
}

/// How a voice plays: `volume` 0 to 1, `pitch` 0.05 to 8 (1 is the clip's
/// own speed), `looping` 0 or 1.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaSettings {
    pub volume: f32,
    pub pitch: f32,
    pub looping: u32,
}

/// How a 3D sound fades with distance, as Unity's rolloff modes.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaRolloff {
    /// Full within the min distance, then as 1 over the distance, until
    /// the max distance, where it stops falling.
    Logarithmic = 0,
    /// Full within the min distance, falling in a line to silence at the
    /// max distance.
    Linear = 1,
}

/// Whether a voice is 2D or 3D: `blend` 0 (2D) to 1 (3D); `pan` -1 (left)
/// to 1 (right) for its 2D part; `min_distance` above 0, within which it's
/// full; `max_distance` above that; `rolloff` an `FaRolloff`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaSpatial {
    pub blend: f32,
    pub pan: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub rolloff: u32,
}

/// How to play a clip: its settings; `priority` 0 (kept longest) to 256
/// (given up first); `pausable` 1 to hold while the game is paused; whose
/// it is (`owner`, 0 for no one's); where to start, in seconds; whether
/// it's 3D, and where it is.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaPlay {
    pub settings: FaSettings,
    pub priority: u32,
    pub pausable: u32,
    pub owner: u64,
    pub from: f64,
    pub spatial: FaSpatial,
    pub position: [f32; 3],
}

/// Who hears: where, and which way they face (a quaternion x, y, z, w,
/// looking along its -Z with +X on its right).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaListener {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// Where a voice is now.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaPlace {
    pub voice: u64,
    pub position: [f32; 3],
}

/// A voice that stopped without being told to: it reached its clip's end
/// (`stolen` 0), or gave way to a more important sound (1).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaStopped {
    pub handle: u64,
    pub owner: u64,
    pub stolen: u32,
}

/// A voice now: `playing` 0 or 1, and where it is in its clip, in seconds
/// (0 when it isn't playing).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaVoice {
    pub playing: u32,
    pub time: f64,
}

/// A clip: its rate in frames a second, 1 or 2 channels, its length in
/// frames and seconds, and the memory its samples take.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaClipInfo {
    pub rate: u32,
    pub channels: u32,
    pub frames: u64,
    pub seconds: f64,
    pub bytes: u64,
}

/// The sound at a glance. `device` 1 when an output plays it; its `rate`
/// and `channels`; the voices the mixer has; and counts since the start.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaStats {
    pub device: u32,
    pub rate: u32,
    pub channels: u32,
    pub voices: u32,
    pub started: u64,
    pub stolen: u64,
    pub refused: u64,
    pub callbacks: u64,
    pub underruns: u64,
    pub reopened: u64,
}

/// The sound of one world. Made by `fa_audio_new`, freed by
/// `fa_audio_free`.
#[derive(Debug)]
pub struct FaAudio {
    audio: Audio,
    stopped: Vec<FaStopped>,
    poisoned: bool,
}

/// A decoded clip. Made by `fa_clip_decode` or `fa_clip_new`, freed by
/// `fa_clip_free`; voices playing it keep it until they end.
#[derive(Debug)]
pub struct FaClip(Arc<Clip>);

// --- Errors --------------------------------------------------------------------

/// cbindgen:ignore
type Error = fynkel_cabi::Error<FaStatus>;

fn invalid(message: String) -> Error {
    Error::new(FaStatus::InvalidValue, message)
}

fn guard(body: impl FnOnce() -> Result<(), Error>) -> FaStatus {
    fynkel_cabi::guard(body)
}

/// Runs `body` on live audio: a panic in it poisons the audio, so every
/// later call on it fails with [`FaStatus::Poisoned`].
///
/// # Safety
///
/// `audio` is null or live, and not used by another thread meanwhile.
unsafe fn with_audio(audio: *mut FaAudio, body: impl FnOnce(&mut FaAudio) -> Result<(), Error>) -> FaStatus {
    if audio.is_null() {
        return guard(|| Err(Error::null("audio")));
    }
    // SAFETY: live and not shared, as the caller promises.
    let a = unsafe { &mut *audio };
    if a.poisoned {
        return guard(|| Err(Error::new(FaStatus::Poisoned, "an earlier call panicked while changing this audio")));
    }
    let status = guard(|| body(&mut *a));
    if status == FaStatus::Panic {
        a.poisoned = true;
    }
    status
}

/// Writes `value` through `out`, or fails when it's null.
///
/// # Safety
///
/// `out` is null or writable.
unsafe fn write<T>(out: *mut T, what: &str, value: T) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: not null, and writable as the caller promises.
    unsafe { out.write(value) };
    Ok(())
}

/// The bytes at `at`, `len` of them; empty when `len` is 0.
///
/// # Safety
///
/// `at` is null or readable for `len` elements.
unsafe fn slice<'a, T>(at: *const T, len: usize, what: &str) -> Result<&'a [T], Error> {
    if len == 0 {
        return Ok(&[]);
    }
    if at.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: readable for len, as the caller promises.
    Ok(unsafe { std::slice::from_raw_parts(at, len) })
}

fn flag(value: u32, what: &str) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid(format!("{what} is {value}, not 0 or 1"))),
    }
}

fn settings(s: FaSettings) -> Result<Settings, Error> {
    Ok(Settings { volume: s.volume, pitch: s.pitch, looping: flag(s.looping, "looping")? })
}

fn spatial(s: FaSpatial) -> Result<Spatial, Error> {
    let rolloff = match s.rolloff {
        0 => Rolloff::Logarithmic,
        1 => Rolloff::Linear,
        other => return Err(invalid(format!("rolloff {other} isn't an FaRolloff"))),
    };
    Ok(Spatial { blend: s.blend, pan: s.pan, min_distance: s.min_distance, max_distance: s.max_distance, rolloff })
}

// --- The library -----------------------------------------------------------------

/// The ABI version the library was built with: `FA_ABI_VERSION` in the
/// header the caller was built with must match it.
#[unsafe(no_mangle)]
pub extern "C" fn fa_abi_version() -> u32 {
    FA_ABI_VERSION
}

/// The message of the last failed call on this thread, as UTF-8 ("" before
/// any). Valid until the next failed call on this thread.
#[unsafe(no_mangle)]
pub extern "C" fn fa_last_error() -> *const c_char {
    fynkel_cabi::last_error()
}

// --- Clips -------------------------------------------------------------------------

/// Decodes a WAV, OGG Vorbis or MP3 file from `len` bytes at `bytes`, and
/// writes the clip to `out`. `name` (`name_len` bytes of UTF-8, or none)
/// is the file's name: it helps find the format and goes in the error.
/// Any thread may call it.
///
/// # Safety
///
/// `bytes` is readable for `len` bytes, `name` for `name_len`, and `out`
/// is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_clip_decode(
    bytes: *const u8,
    len: usize,
    name: *const u8,
    name_len: usize,
    out: *mut *mut FaClip,
) -> FaStatus {
    guard(|| {
        // SAFETY: as this function's.
        let (bytes, name) = unsafe { (slice(bytes, len, "bytes")?, slice(name, name_len, "name")?) };
        let name = std::str::from_utf8(name).map_err(|_| invalid("the name isn't UTF-8".into()))?;
        let clip = Clip::decode(bytes, name).map_err(|e| Error::new(FaStatus::CantDecode, e))?;
        // SAFETY: as this function's.
        unsafe { write(out, "out", Box::into_raw(Box::new(FaClip(Arc::new(clip))))) }
    })
}

/// A clip from `len` samples at `samples`, interleaved in `channels` (1 or
/// 2), at `rate` frames a second: a sound made in code. Writes it to `out`.
///
/// # Safety
///
/// `samples` is readable for `len` floats, and `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_clip_new(
    rate: u32,
    channels: u32,
    samples: *const f32,
    len: usize,
    out: *mut *mut FaClip,
) -> FaStatus {
    guard(|| {
        // SAFETY: as this function's.
        let samples = unsafe { slice(samples, len, "samples")? };
        let channels =
            u16::try_from(channels).map_err(|_| invalid(format!("a clip has 1 or 2 channels, not {channels}")))?;
        let clip = Clip::new(rate, channels, samples.to_vec()).map_err(invalid)?;
        // SAFETY: as this function's.
        unsafe { write(out, "out", Box::into_raw(Box::new(FaClip(Arc::new(clip))))) }
    })
}

/// Writes what `clip` is to `out`.
///
/// # Safety
///
/// `clip` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_clip_info(clip: *const FaClip, out: *mut FaClipInfo) -> FaStatus {
    guard(|| {
        if clip.is_null() {
            return Err(Error::null("clip"));
        }
        // SAFETY: live, as the caller promises.
        let clip = unsafe { &(*clip).0 };
        let info = FaClipInfo {
            rate: clip.rate(),
            channels: u32::from(clip.channels()),
            frames: clip.frames() as u64,
            seconds: clip.seconds(),
            bytes: clip.bytes() as u64,
        };
        // SAFETY: as this function's.
        unsafe { write(out, "out", info) }
    })
}

/// Lets go of a clip. Voices playing it keep it until they end. Any thread
/// may call it.
///
/// # Safety
///
/// `clip` is null or live, and not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_clip_free(clip: *mut FaClip) {
    if !clip.is_null() {
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in fa_clip_decode or
            // fa_clip_new; the caller gives it up.
            drop(unsafe { Box::from_raw(clip) });
            Ok(())
        });
    }
}

// --- Audio -------------------------------------------------------------------------

/// New sound for a world, without a device until `fa_audio_open_device`.
/// Free it with `fa_audio_free`.
#[unsafe(no_mangle)]
pub extern "C" fn fa_audio_new() -> *mut FaAudio {
    Box::into_raw(Box::new(FaAudio { audio: Audio::new(), stopped: Vec::new(), poisoned: false }))
}

/// Stops the sound, closes its device and frees it.
///
/// # Safety
///
/// `audio` is null or live, and not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_free(audio: *mut FaAudio) {
    if !audio.is_null() {
        // A panic while dropping must not unwind into the caller: what's
        // left leaks instead, and the message is the last error.
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in fa_audio_new; the caller
            // gives it up.
            drop(unsafe { Box::from_raw(audio) });
            Ok(())
        });
    }
}

/// Plays through the default output from now on, and again after it's
/// lost. `NoDevice` when it can't (the message says why): sound goes on
/// without one, and a later `fa_audio_update` tries again.
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_open_device(audio: *mut FaAudio) -> FaStatus {
    // SAFETY: as this function's.
    unsafe { with_audio(audio, |a| a.audio.open_device().map_err(|e| Error::new(FaStatus::NoDevice, e))) }
}

/// Closes the device: sound goes on without one.
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_close_device(audio: *mut FaAudio) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            a.audio.close_device();
            Ok(())
        })
    }
}

/// Stops the device and the sound while the app is away (`suspended` 1),
/// or starts them again (0).
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_suspend(audio: *mut FaAudio, suspended: u32) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            a.audio.suspend(flag(suspended, "suspended")?).map_err(|e| Error::new(FaStatus::NoDevice, e))
        })
    }
}

/// How the game's sound sits with the phone's: an `FaSession`.
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_session_set(audio: *mut FaAudio, session: u32) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            let session = match session {
                0 => Session::Ambient,
                1 => Session::Playback,
                _ => return Err(invalid(format!("session {session} isn't an FaSession"))),
            };
            a.audio.set_session(session).map_err(|e| Error::new(FaStatus::NoDevice, e))
        })
    }
}

/// Once a frame, with the frame's real time in seconds at `seconds` (by
/// pointer, as a double passed to a native call is an allocation in Dart's
/// JIT): takes back the voices that ended, makes the sound of that time
/// when no device plays, and opens a lost device again. Writes how many
/// voices stopped on their own to `stopped`: read them with
/// `fa_stopped_read`.
///
/// # Safety
///
/// `audio` is null or live; `seconds` is null or readable; `stopped` is
/// null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_update(audio: *mut FaAudio, seconds: *const f64, stopped: *mut usize) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            if seconds.is_null() {
                return Err(Error::null("seconds"));
            }
            if stopped.is_null() {
                return Err(Error::null("stopped"));
            }
            a.audio.update(*seconds);
            a.stopped.clear();
            a.stopped.extend(a.audio.stopped().iter().map(|s| FaStopped {
                handle: s.handle,
                owner: s.owner,
                stolen: u32::from(s.stolen),
            }));
            write(stopped, "stopped", a.stopped.len())
        })
    }
}

/// Copies the last `fa_audio_update`'s stopped voices to `out`, `len` of
/// them (its count).
///
/// # Safety
///
/// `audio` is null or live; `out` is writable for `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_stopped_read(audio: *mut FaAudio, out: *mut FaStopped, len: usize) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            if len != a.stopped.len() {
                return Err(Error::new(FaStatus::WrongSize, format!("{} stopped, not {len}", a.stopped.len())));
            }
            if len > 0 {
                if out.is_null() {
                    return Err(Error::null("out"));
                }
                std::ptr::copy_nonoverlapping(a.stopped.as_ptr(), out, len);
            }
            Ok(())
        })
    }
}

/// Plays `clip` as `play` says, and writes its voice's handle to `out`: 0
/// when every voice is busy with a sound at least as important.
///
/// # Safety
///
/// `audio` and `clip` are null or live; `play` is null or readable; `out`
/// is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_play(
    audio: *mut FaAudio,
    clip: *const FaClip,
    play: *const FaPlay,
    out: *mut u64,
) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            if clip.is_null() {
                return Err(Error::null("clip"));
            }
            if play.is_null() {
                return Err(Error::null("play"));
            }
            let p = *play;
            let priority = u16::try_from(p.priority)
                .map_err(|_| invalid(format!("priority {} isn't between 0 and 256", p.priority)))?;
            let handle = a
                .audio
                .play(
                    &(*clip).0,
                    Play {
                        settings: settings(p.settings)?,
                        priority,
                        pausable: flag(p.pausable, "pausable")?,
                        owner: p.owner,
                        from: p.from,
                        spatial: spatial(p.spatial)?,
                        position: p.position,
                    },
                )
                .map_err(invalid)?;
            write(out, "out", handle.unwrap_or(0))
        })
    }
}

/// Stops a voice, with a short fade. One that already ended is left alone.
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_voice_stop(audio: *mut FaAudio, voice: u64) -> FaStatus {
    // SAFETY: as this function's.
    unsafe { with_audio(audio, |a| a.audio.stop(voice).map_err(invalid)) }
}

/// Pauses a voice where it is (`paused` 1), or plays it on (0).
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_voice_pause(audio: *mut FaAudio, voice: u64, paused: u32) -> FaStatus {
    // SAFETY: as this function's.
    unsafe { with_audio(audio, |a| a.audio.pause(voice, flag(paused, "paused")?).map_err(invalid)) }
}

/// Changes a playing voice's volume, pitch or looping to `settings`'s (by
/// pointer, as a struct passed by value is an allocation in Dart).
///
/// # Safety
///
/// `audio` is null or live; `settings` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_voice_set(audio: *mut FaAudio, voice: u64, settings: *const FaSettings) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            if settings.is_null() {
                return Err(Error::null("settings"));
            }
            a.audio.set(voice, self::settings(*settings)?).map_err(invalid)
        })
    }
}

/// Changes a playing voice's 2D or 3D settings to `spatial`'s, heard at
/// once.
///
/// # Safety
///
/// `audio` is null or live; `spatial` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_voice_spatial_set(audio: *mut FaAudio, voice: u64, spatial: *const FaSpatial) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            if spatial.is_null() {
                return Err(Error::null("spatial"));
            }
            a.audio.set_spatial(voice, self::spatial(*spatial)?).map_err(invalid)
        })
    }
}

/// The frame's moves, in one call: the listener, at `listener`, and
/// `count` voices at `places`, each where it is now. Heard from the next
/// `fa_audio_update`. A voice that ended is skipped.
///
/// # Safety
///
/// `audio` is null or live; `listener` is null or readable; `places` is
/// readable for `count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_place(
    audio: *mut FaAudio,
    listener: *const FaListener,
    places: *const FaPlace,
    count: usize,
) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            if listener.is_null() {
                return Err(Error::null("listener"));
            }
            let places = slice(places, count, "places")?;
            let l = *listener;
            a.audio.set_listener(Listener { position: l.position, rotation: l.rotation }).map_err(invalid)?;
            for place in places {
                a.audio.place(place.voice, place.position).map_err(invalid)?;
            }
            Ok(())
        })
    }
}

/// Writes whether a voice plays, and where it is, to `out`.
///
/// # Safety
///
/// `audio` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_voice_get(audio: *mut FaAudio, voice: u64, out: *mut FaVoice) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            let time = a.audio.time(voice);
            write(out, "out", FaVoice { playing: u32::from(time.is_some()), time: time.unwrap_or(0.0) })
        })
    }
}

/// Stops every voice of `owner`.
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_owner_stop(audio: *mut FaAudio, owner: u64) -> FaStatus {
    // SAFETY: as this function's.
    unsafe { with_audio(audio, |a| a.audio.stop_owner(owner).map_err(invalid)) }
}

/// Whether the game is paused (1): pausable voices fall silent and hold.
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_game_paused_set(audio: *mut FaAudio, paused: u32) -> FaStatus {
    // SAFETY: as this function's.
    unsafe { with_audio(audio, |a| a.audio.set_game_paused(flag(paused, "paused")?).map_err(invalid)) }
}

/// Every voice's gain, 0 to 1.
///
/// # Safety
///
/// `audio` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_volume_set(audio: *mut FaAudio, volume: f32) -> FaStatus {
    // SAFETY: as this function's.
    unsafe { with_audio(audio, |a| a.audio.set_volume(volume).map_err(invalid)) }
}

/// Writes the sound at a glance to `out`.
///
/// # Safety
///
/// `audio` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_stats(audio: *mut FaAudio, out: *mut FaStats) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            let s = a.audio.stats();
            write(
                out,
                "out",
                FaStats {
                    device: u32::from(s.device),
                    rate: s.rate,
                    channels: s.channels,
                    voices: s.voices,
                    started: s.started,
                    stolen: s.stolen,
                    refused: s.refused,
                    callbacks: s.callbacks,
                    underruns: s.underruns,
                    reopened: s.reopened,
                },
            )
        })
    }
}

/// Makes `frames` frames of the sound into `out` (interleaved stereo at
/// 48 kHz) as a device would have: for tests. `NoDevice` while a device
/// plays.
///
/// # Safety
///
/// `audio` is null or live; `out` is writable for `frames * 2` floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fa_audio_render(audio: *mut FaAudio, out: *mut f32, frames: usize) -> FaStatus {
    // SAFETY: as this function's.
    unsafe {
        with_audio(audio, |a| {
            if frames == 0 {
                return Ok(());
            }
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let out = std::slice::from_raw_parts_mut(out, frames * 2);
            a.audio.render(out).map_err(|e| Error::new(FaStatus::NoDevice, e))
        })
    }
}
