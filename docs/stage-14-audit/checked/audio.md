# Stage 14 audit, checked: sound (AU1–AU10)

Checker pass on a worktree of main 2f72cbd5. The audio code is identical to stage-14 edbe57aa, which the reader
read (`git diff HEAD stage-14` over the audio crates and package is empty). Throwaway tests are in
/tmp/s14/tests/audio/, each with its repo path on line 1:

- `crates/fynkel_audio/tests/s14_audit.rs` (AU1 ×3, AU2 ×2, AU3). Run with `cargo test -p fynkel_audio --test s14_audit`.
- `crates/fynkel_audio/tests/s14_audit_stream.rs` (AU7, uses its own counting allocator).
- `packages/fynkel_audio/test/s14_audit_test.dart` (AU1, AU3, AU4, AU5). Run with `dart test test/s14_audit_test.dart`.

All 11 tests fail today.

---

## AU1. CONFIRMED (test). Bug, S. Severity is right, and the trigger is wider than claimed.

`Audio::play` checks only that `from` is finite and `>= 0` (audio.rs:325). The mixer sets `place = from`
(mixer.rs:412), and the decoded path calls `clip.frame(place as usize)` (mixer.rs:589) before anything checks
`place >= length`. `Clip::frame` indexes `samples[frame]` unchecked (clip.rs:272/275). In
`s14_audit.rs::au1_starting_a_decoded_clip_at_its_end…`, a 48 kHz clip of 48 000 frames played from 1.0 s panics
with "index out of bounds: the len is 48000 but the index is 48000". `au1_after_the_panic…` shows the mixer
`Mutex` is then poisoned: `render` returns Err for good. `au1_starting_past_the_end_by_rounding` shows a
non-whole-second stereo clip (1000 frames at 22.05 kHz, `from = 1000/22050`) panics too, so exact whole seconds
aren't needed. The Dart test reaches it the most likely way: `source.playTime = clip.length` while the source
plays. `playTime`'s setter calls `play()` again at once (source.dart:245-252), so a seek slider at its end is
enough. Headless, every later call throws `AudioException: an earlier call panicked`. On a device the panic is
inside cpal's output callback. Neither cpal 0.18.2 nor coreaudio-rs 0.14.2 has a `catch_unwind` (grepped), and
the callback is reached through an `extern "C"` render proc, so the process aborts. The reader's first fix is
right: clamp or finish in `Audio::play`/`Start`, and make `Clip::frame` use `get`. This is the top item.

## AU2. CONFIRMED (test). Bug, M. Part (b) is deliberate policy, but its effect is still a bug.

(a) `s14_audit.rs::au2_a_stolen_voice_ramps_out`: 32 voices at 0.02 each settle to 0.64 per ear. One steal by a
silent clip drops the output by exactly 0.02 between two samples. `Start` on a busy slot calls `self.end(i,
false)` and overwrites the voice (mixer.rs:410-411), with no ramp. (b)
`s14_audit.rs::au2_a_fade_out_isnt_cut…`: music at priority 0 is fading out over 3 s (`fade(.., stop: true)`) and
the other 31 voices are priority 0. A priority-256 sound still gets a voice, taking the fading track's slot and
cutting it from about 0.33 to 0 in one sample. The victim key `(stopped, priority, age)` (audio.rs:338-340) puts
any stopped slot first whatever the newcomer's priority. The comment ("one already stopping before all") shows
this is on purpose. The click and the cut-off crossfade are what's wrong with it. The reader's
`AudioSource.play()` restart point also holds: source.dart:272 stops the old voice, and `play` then takes the
first free slot or the stopped one.

## AU3. CONFIRMED (test). Bug, S.

`play()` pushes `Stopped { stolen: true }` (audio.rs:346-348). `update()` begins with `self.stopped.clear()`
(audio.rs:540), and `fa_audio_update` calls `update` and only then copies `stopped()` (ffi lib.rs:559-565). Dart
calls `play` during the frame and `fa_audio_update` in `afterFrame`, so no steal ever crosses the FFI.
`s14_audit.rs::au3_…` checks this at the Rust level: the steal is visible before `update` and gone after it.
Dart test `AU3` shows the effect. A source plays 32 long one-shots, world-level one-shots steal all of them,
those end, the source plays a loud new one-shot, and `source.volume = 0` leaves it at 0.5 because `_shotCount`
is stuck at 32 (source.dart:345) and the new shot is never tracked. The hidden second bug is real too:
audio.dart:721 calls `music._ended(handle)` without `stolen`, and `AudioMusic._ended` fires `onEnd`
(mix.dart:183-194). See NEW-1 for how music gets stolen.

## AU4. CONFIRMED (test). Bug (use after free), S.

`Audio.dispose` `calloc.free`s every scratch block (audio.dart:752-765), which breaks the contract in
systems.dart:122-128. `playOneShot` writes `_playFloats[0..1]` (audio.dart:336-344) and only reaches `_native`'s
`StateError` in `_start`. Dart test `AU4`: after `world.dispose()`, 64 zeroed `calloc` blocks of `FaPlay`'s size
(80 B) are taken, then `audio.playOneShot(clip, volume: 0.5, pitch: 0.75)` throws `StateError` as it should, but
one of those blocks now reads 0.5 / 0.75. The freed `_play` block had been handed back to the test, and the call
wrote into it. The other cited paths (`playAt`; `AudioGroup.volume`/`fade` via `_fadeGroup`; `AudioMusic.volume`
via `_settingsFloats`; `_writeSpatial`'s `_ids`) do the same write-then-`_native` sequence when read.
`AudioMusic.volume` returns early when `_voice == 0`, so it's only hit with a track playing.

## AU5. PARTLY (test). Bug, S. The mechanism is exact, the trigger narrower.

The mechanism is as described. `fa_audio_place` fails the whole call on the first bad place (ffi lib.rs:769-772).
`_place`'s `_check` throws before `fa_audio_update` (audio.dart:533 vs 707), so no ends, reopen or quiet render
happen, and after 5 frames the system is turned off for good. The reader's NaN example can't happen that way:
`GameObject.transform=` refuses NaN or anything not finite as f32 (game_object.dart:992-1001, "Transform.position:
NaN or infinity"), so the Dart test can't set it. What does reach the mixer is a world transform that overflows
f32 although every local value is finite. Dart test `AU5` puts the source at local x = 3e38 under a parent moved
to x = 3e38 (world x = 6e38 = inf). After 6 frames `world.audio.isTurnedOff` is true, and it stays off after the
parent is moved back. Other routes are a world-space result that comes back non-finite from the engine (e.g. a
degenerate parent scale) or physics writing a bad pose, if either can happen. Physics checks its inputs, and I
didn't find a route there. The fix still stands: skip and count bad places, and always run `fa_audio_update`.

## AU6. CONFIRMED (reading). Bug (conditional), S.

android.rs:91 counts `MODE_RINGTONE | MODE_IN_CALL | MODE_IN_COMMUNICATION` as a call, whatever the session.
`Audio::update` polls it every 500 ms (`CALL_CHECK`, audio.rs:38, 577-588) and `device.suspend(true)`s the
stream while it's true. `Session::VoiceChat` is just `Playback` on Android (session.rs:107-116; documented
"On Android, as `Playback`"). So whenever the game's own voice stack puts `AudioManager` into
`MODE_IN_COMMUNICATION`, the game's sound pauses for the whole chat. WebRTC-based stacks such as flutter_webrtc
do this by default. Whether the user's game hits it depends on its mic stack (`record` leaves the mode at
normal unless asked). It can't be tested: `in_call` is `#[cfg(target_os = "android")]` and calls JNI directly,
with no seam. One caveat on the fix: since Android 11 `getMode` reports the mode system-wide, so "does this
app own the mode" isn't available from `getMode` alone. Ignoring `MODE_IN_COMMUNICATION` when the session is
`VoiceChat` is the simple version.

## AU7. CONFIRMED (test). Slow, S. The severity is minor.

clip.rs:77 builds every reader with `Cursor::new(file.to_vec())`. `Reader::open` runs per voice start, and on
each loop `fill` sets `opened = None` (clip.rs:378-383), which reopens it. `s14_audit_stream.rs`: a 256 KB
streamed WAV looping for a bit over 3 passes makes 4 allocations at least the file's size (1 start + 3 loops).
That's 2.1 MB allocated in all, and each is a full memcpy plus a new probe and decoder. With a device this runs
on the streams thread, so it's not a glitch. It is per-loop garbage that scales with track size, and on the
game thread headless. The `Arc<[u8]>`-backed cursor fix is right (`Data::Streamed { file: Arc<[u8]> }` already).
Seek-to-0 instead of reopening is optional.

## AU8. CONFIRMED (reading) for the mechanism. The consequence is uncertain. Bug?, M.

On interruption-began, cpal 0.18.2 stops the unit itself and emits no error (session_event_manager.rs: the
`Began` branch calls `stop_for_interruption` and returns). On `Ended` + `ShouldResume` it resumes, but only for a
stream that's still alive (`Weak` upgrade). Fynkel's watchdog (audio.rs:551-564) sees no callbacks for 2 s while
not suspended, marks the device lost, and drops it. That drops the cpal stream and its observers, so cpal's own
resume is lost. It then calls `try_device` every second (audio.rs:570-576). `fynkel_view.dart:532` leaves
`applicationPaused` alone on `inactive`, so Siri, alarms and a ringing call shown over the app all go down this
path. A call that is answered usually moves the app to background, which suspends it. On iOS `session::apply` is
a no-op when the category is unchanged (session.rs:57-61), so each retry is spawn/join of the streams thread,
`build_output_stream` and `play` during the interruption. Whether that start fails, or succeeds with no
callbacks (lost again 2 s later), or plays over Siri for `Ambient` can't be settled from the code. In any case
it recovers after the interruption through the same reopen loop, so the fault is wasted work and possibly a
wrong-time start, not lasting silence. It needs a device check (Siri for more than 3 s, watching
`stats.reopened`).

## AU9. NOT as stated. Awkward → nit, S.

Fynkel 2 has no scene loading: no `loadScene`/`LoadSceneMode` exists in packages/fynkel/lib. A world is the
scene, and `World.dispose` ("Destroys every object, as a scene unloads in Unity", world.dart:97) frees the
`Audio` and every voice with it. The proposed test ("load another scene in single mode") can't be written. What
is true: within one world, a game that swaps levels by destroying the level root keeps world-level `playOneShot`
and `playAt` voices (owner 0, audio.dart:345/388), and `Audio` has no stop-all or returned handle to cut them. A
long `playAt` keeps playing from where it was. The writing-a-game.md row mapping `PlayClipAtPoint` → `playAt` is
fair for a one-off sound. A `stopAll()`/handle would close the gap if it ever matters.

## AU10. CONFIRMED (reading). Slow → nit, S. One detail is wrong.

`try_device` (audio.rs:244-255) runs `session::apply` (on Android `prepare()` + a JNI `requestAudioFocus` call),
`streamer.start()` (thread spawn), `Device::open`, and on failure `streamer.stop()` (`join`, stream.rs:143-151).
`update` retries every second while `wants_device` (audio.rs:570-576). The claim that it "gets more frequent as
`last_try` loops" is wrong. `last_try` is set at the top of every try, and the interval is a fixed 1 s with no
backoff. The cost is one thread spawn/join a second on the game thread, tens of µs. One Android side effect
the reader didn't mention: each retry calls `requestAudioFocus(GAIN)` again for `Playback`/`VoiceChat`, so
if the output keeps failing it keeps taking focus from another app every second. That needs a failing output
on Android, which is rare. It can't be tested without a `Device` seam: this Mac has an output, so `open`
succeeds. Opening the device before spawning the thread, plus a backoff, is a fine cheap fix.

---

## NEW-1. Music isn't "never given up": a priority-0 sound steals it. Bug (doc vs behaviour), S. CONFIRMED (reading).

`AudioMusic` plays at priority 0 (mix.dart:174), and its doc says it "is never given up for other sounds"
(mix.dart, class doc). The victim filter takes slots whose `priority >= play.priority` (audio.rs:338), so with
all 32 voices busy at priority 0, a priority-0 sound steals the oldest of them. A track playing since the level
started is the oldest. Today the track just stops silently (AU3 drops the notice). After AU3 is fixed,
audio.dart:721 would also fire `music.onEnd`. Fix: exclude music's voice from stealing (or reserve priority 0
for it, documented), and pass `stolen` to `AudioMusic._ended`.
