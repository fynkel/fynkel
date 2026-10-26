# Stage 14 audit: sound (crates/fynkel_audio, crates/fynkel_audio_ffi, packages/fynkel_audio)

Reader: audio. Read-only pass on branch stage-14 (edbe57aa). Nothing was built or run. Every claim below
comes from reading the code. Where I'm not sure, I say so.

---

## AU1. Starting a decoded clip at its end panics on the audio thread (and the app aborts)

- **Severity:** bug. **Size:** S.
- **Where:** crates/fynkel_audio/src/audio.rs:325 (`from` is only checked for being finite and `>= 0`; there's
  no upper bound); crates/fynkel_audio/src/mixer.rs:412 (`place = from`), :589-591 (`clip.frame(place as usize)`);
  crates/fynkel_audio/src/clip.rs:272/275 (`samples[frame]` has no bounds check). Dart sends exactly this
  value: packages/fynkel_audio/lib/src/source.dart:276 (`_from < clip.length ? _from : clip.length`).
- **How it shows:** take `source.playTime = clip.length` (or any larger value, which Dart clamps to
  `clip.length`) followed by `play()`. Or call `Audio::play(.., Play { from: clip.seconds() })` in Rust. If
  `length * rate` comes out as exactly `frames` (it does for whole-second clips, e.g. 24000 frames at 24 kHz),
  the first frame the mixer renders indexes `samples[frames]` and panics. On a device this happens inside
  cpal's CoreAudio/AAudio callback. Neither cpal 0.18.2 nor coreaudio-rs has a `catch_unwind` (I grepped the
  registry sources), so the unwind reaches an `extern "C"` frame and the process aborts. Without a device (in
  tests, or before the view opens), the panic happens in `update`'s `render_quietly` under `with_audio`'s
  guard. That poisons the `FaAudio` and the mixer `Mutex`, so every later call fails. The mixer's opening
  claim that "nothing [...] waits" also assumes it can't panic.
- **First fix:** clamp in `Audio::play`. A decoded clip starting at or past its end should finish at once
  (e.g. `from >= frames` gives `finished = true` before the loop). Separately, make `Clip::frame` defensive
  (`get(..).unwrap_or(0)`) so no index on the audio thread can panic.
- **Test that fails today:** `let clip = Arc::new(Clip::new(48_000, 1, vec![0.1; 48_000]).unwrap());
  audio.play(&clip, Play { from: 1.0, ..Default::default() }); audio.render(&mut [0.0; 1024])` panics.
  Expected: `Ok`, and the voice is reported as finished.

## AU2. Voice stealing cuts the stolen voice with no ramp, and a fading-out voice is always stolen first

- **Severity:** bug. **Size:** M.
- **Where:** crates/fynkel_audio/src/mixer.rs:411 (`Start` on a busy slot calls `self.end(i, false)` and
  replaces the voice at once); crates/fynkel_audio/src/audio.rs:338-340 (the victim is the `max_by_key((stopped,
  priority, age))`, so any `stopped` slot wins at any priority).
- **How it shows:** (a) With all 32 voices busy, each steal removes a playing voice between two samples.
  This is the one start/stop path the 5 ms ramp (mixer.rs:35) doesn't cover, so it clicks. Busy scenes
  (footsteps, gunfire) are exactly where all 32 are in use. (b) `music.crossFade(next, 3)` or
  `music.stop(fadeOut: 3)` marks the old track `stopped` (audio.rs:423-425). While all voices are busy, the
  next sound of any priority, even 256, takes that slot and the old track's fade-out is cut off. The same
  happens in `AudioSource.play()` while busy: it stops its old voice (source.dart:272), and the new voice
  then takes that same slot, so a restart clicks.
- **First fix:** keep a few hidden "tail" voices in the mixer (e.g. 32 audible + 8 tails). On `Start` into a
  busy slot, move the old voice to a free tail with `stopping = true` so it ramps out over 5 ms (or finishes
  its fade). Only cut it if every tail is busy. On the game side, prefer a victim that isn't stopped (or
  whose fade is nearly over) when the new sound's priority is low.
- **Test that fails today:** fill 32 voices with a constant 0.02 level clip, play a silent clip at the same
  priority, and render one callback. Today the first frame drops by 0.02 at once. Expected: it falls
  smoothly (each step < 0.005). Second test: crossfade music with 31 other voices busy, play a priority-256
  one-shot, and assert the old track is still audible 1 s into the 3 s fade.

## AU3. Notices of stolen voices never reach Dart, so one-shot handles leak in `AudioSource`

- **Severity:** bug. **Size:** S.
- **Where:** crates/fynkel_audio/src/audio.rs:348 (`play()` pushes `Stopped { stolen: true }`), :540
  (`update()` starts with `self.stopped.clear()`); crates/fynkel_audio_ffi/src/lib.rs:559-565
  (`fa_audio_update` calls `update` and then copies `stopped()`). Dart only learns of stops this way
  (audio.dart:707-722).
- **How it shows:** a steal is recorded during the frame and then cleared by the next `update` before the
  FFI copies the list, so `stolen == 1` never reaches Dart. Effects: `AudioSource._shots`
  (source.dart:346-350) keeps dead handles. `_shotCount` only drops through `_ended`, so on a busy source it
  climbs to 32. After that, new one-shots aren't tracked: a 3D one-shot no longer follows its object
  (`_place` only walks `_shots`, audio.dart:521), and `mute`/`volume` no longer reach it (source.dart:388).
  `_place` also sends up to 32 stale places every frame. The Rust test
  (tests/audio.rs:144-174) reads `stopped()` without calling `update` in between, which is why it passes.
  A second bug is hidden by this one: audio.dart:721 calls `music._ended(handle)` without checking
  `stolen`, so once steals do arrive, `music.onEnd` will fire for a stolen track.
- **First fix:** clear `stopped` at the end of the FFI read, not at the start of `update` (or keep two
  lists: steals since the last read, and ends). In Dart, pass `stolen` to `AudioMusic._ended` and skip
  `onEnd`.
- **Test that fails today:** in crates/fynkel_audio_ffi/tests/abi.rs, fill 32 voices at priority 200, play
  one at 0, call `fa_audio_update`, and expect `stopped == 1` with `stolen == 1`. Today it's 0. A Dart
  variant: 40 `playOneShot`s from one source with every voice busy, then assert `source._shotCount <= 32`
  and that changing `volume` reaches the newest shot.

## AU4. `Audio.dispose` frees its scratch, but its public calls write to that scratch before they throw

- **Severity:** bug (use after free). **Size:** S.
- **Where:** packages/fynkel_audio/lib/src/audio.dart:752-765 (`calloc.free` of `_play`, `_settings`, `_fade`,
  `_ids`, `_transforms` and the rest inside `dispose`). This goes against the contract in
  packages/fynkel/lib/src/systems.dart:122-128, which says scratch must outlive dispose and be freed by a
  finaliser. Physics follows that contract (packages/fynkel_physics/lib/src/physics.dart:414-415, which uses
  OwnedMemory). Writes before `_native` throws: `playOneShot` (audio.dart:336-344) and `playAt` (:375-385);
  `AudioSource.play`/`playOneShot` (source.dart:273-277, 330-334, plus `_writeSpatial`, which has
  `readWorldTransforms` write into the freed `_ids`/`_transforms` at audio.dart:443-444);
  `AudioGroup.volume`/`fade` (mix.dart:54, then audio.dart:648); `AudioMusic.volume` (mix.dart:110).
- **How it shows:** a HUD or Flutter button keeps `world.audio` (or a group) and calls it after the world
  is disposed. That is the case systems.dart names. The call writes floats into freed malloc memory, which
  may already belong to something else, and only then throws `StateError`. The result is silent heap
  corruption.
- **First fix:** do what physics does. Hold the scratch in one owned block freed by a `NativeFinalizer`,
  and check `_disposed` at the top of each public entry point.
- **Test that fails today:** this is hard to observe from Dart. A practical version: after `world.dispose()`,
  `calloc` a block of `sizeOf<FaFade>()` (the allocator usually hands back the freed block), set
  `audio.group('m').volume = 0.5` inside `expect(.., throwsStateError)`, and assert the new block still
  reads 0. A structural test is more reliable: assert `Audio` uses the same owned-memory helper as physics.

## AU5. One bad transform turns off the world's sound for good

- **Severity:** bug. **Size:** S.
- **Where:** crates/fynkel_audio_ffi/src/lib.rs:769-772 (`fa_audio_place` fails the whole call on the first
  bad position or listener); crates/fynkel_audio/src/audio.rs:466-467 and spatial.rs:84-89 (reject
  non-finite values); packages/fynkel_audio/lib/src/audio.dart:533 (`_check` throws in `afterFrame` before
  `fa_audio_update` runs at :707); systems.dart:12-13 (a system that throws 5 frames in a row is turned off).
- **How it shows:** a playing 3D source on an object whose position becomes NaN (a physics blow-up or a
  divide-by-zero in game code), or a hearer with a degenerate rotation. Each frame `_place` throws, so
  `update` doesn't run: no end notices, no reopen of a lost device, and no rendering without a device.
  After 5 frames the system is turned off and never recovers, even once the object is fixed or destroyed.
  Uncertain: I found no finiteness check on the engine's transform setters (motion.dart `_setPosition`,
  fynkel_ffi). If NaN is rejected there, the trigger is narrower (a degenerate world rotation).
- **First fix:** in `fa_audio_place`, skip and count bad places, keeping the last good listener. In Dart,
  always call `fa_audio_update` even if `_place` failed, and report bad positions once.
- **Test that fails today:** Dart: a playing 3D `AudioSource` whose object is moved to `Vec3(double.nan,
  0, 0)`. Run 6 frames, then move it back and check that `onEnd` still fires when the clip ends and that
  `audio` isn't `isTurnedOff`.

## AU6. On Android, `AudioSession.voiceChat` goes silent while the game's own voice chat runs (likely)

- **Severity:** bug (likely; not seen on a device). **Size:** S.
- **Where:** crates/fynkel_audio/src/android.rs:50-51 and :90 (`MODE_IN_COMMUNICATION` counts as a call);
  crates/fynkel_audio/src/audio.rs:577-588 (when in a call, the device is suspended);
  session.rs:107-116 (on Android, voiceChat is treated as playback).
- **How it shows:** the VoIP stacks a game would use (flutter_webrtc, LiveKit, Agora) put `AudioManager`
  into `MODE_IN_COMMUNICATION` while they talk. Within half a second `in_call()` returns true and the
  game's output is paused for the whole voice chat, which is the feature voiceChat exists for (PR #142).
- **First fix:** when the session is `VoiceChat`, count only `MODE_RINGTONE`/`MODE_IN_CALL` (a cellular
  call) as a call. Better, use `TelephonyManager`/`AudioManager.getMode` together with whether this app
  owns the communication mode.
- **Test that fails today:** a unit test on a function that maps (session, mode) to suspend, if
  `in_call`'s decision is factored out: `(VoiceChat, MODE_IN_COMMUNICATION)` should not suspend. Today
  it does.

## AU7. Each streamed voice copies its whole file on every start and every loop

- **Severity:** slow. **Size:** S.
- **Where:** crates/fynkel_audio/src/clip.rs:77 (`Cursor::new(file.to_vec())`), reached from
  `Reader::open` (clip.rs:318-320) once per voice start and again on every loop (clip.rs:378-383 sets
  `opened = None`).
- **How it shows:** a 3-5 MB music OGG is copied in full on every `music.play`/`crossFade` and every time
  it loops, along with a fresh probe and a fresh decoder. With a device this runs on the stream thread,
  so it causes no glitch, but it is garbage that grows with track size. Without a device, `pump()` runs it
  on the game thread (audio.rs:605, 646), where it costs frame time.
- **First fix:** `MediaSourceStream::new(Box::new(Cursor::new(ArcBytes(file.clone()))))` with a newtype
  `AsRef<[u8]>` over `Arc<[u8]>`. symphonia implements `MediaSource` for `Cursor<T: AsRef<[u8]>>`. To
  loop, seek to 0 instead of reopening.
- **Test that fails today:** in tests/allocation.rs or tests/stream.rs, count bytes allocated by
  `decode_streams()` across 3 loops of a 1 MB streamed clip. Expect well under 1 MB; today it's 3 MB or
  more.

## AU8. iOS: the 2 s silence watchdog can't tell an interruption from a lost stream (uncertain)

- **Severity:** bug? (uncertain). **Size:** M.
- **Where:** crates/fynkel_audio/src/audio.rs:551-564 (2 s without callbacks counts as lost), :570-576 (a
  reopen every second); packages/fynkel_flutter/lib/src/fynkel_view.dart:532 (`inactive` doesn't
  suspend); cpal-0.18.2 coreaudio/ios/session_event_manager.rs:109-122 (cpal itself stops the stream on an
  interruption and resumes it after).
- **How it shows:** during Siri, an alarm, or a call shown while the app stays `inactive`, cpal stops the
  stream. The app isn't suspended, so after 2 s Fynkel drops the stream (which also discards cpal's own
  resume) and calls `try_device` every second. Each call applies the session, spawns and joins the stream
  thread, and tries to start an IO unit during the interruption. Whether the start fails or (for the
  mixable Ambient category) plays over Siri is not something I could settle from the code.
  `Stats.reopened` will rise during a call. PR #144 was tested for route changes, not interruptions.
- **First fix:** observe `AVAudioSessionInterruptionNotification` (or have cpal's error callback report an
  `Interrupted` state) and treat it like `in_call`: suspend the watchdog and don't reopen.
- **Test that fails today:** none headless. On the device, start a Siri query for over 3 s in the lab and
  check that `reopened` stays the same.

## AU9. World-level one-shots and `playAt` sounds survive scene loads

- **Severity:** awkward. **Size:** S.
- **Where:** packages/fynkel_audio/lib/src/audio.dart:345 and :388 (`owner: 0`); the audio package has no
  scene hook (grep for "scene" in packages/fynkel_audio/lib finds only a comment).
- **How it shows:** Unity's `PlayClipAtPoint` makes a temporary object in the active scene, so a single
  scene load cuts it off. Here an explosion `playAt` (pausable by default) keeps playing into the next
  level, from a position in the old scene. The mapping table in docs/writing-a-game.md:4041 presents the
  two as equivalent.
- **First fix:** give world-level shots a "scene" owner and stop it on a single scene load. A
  `persistent:` flag on `playOneShot` would cover menu clicks.
- **Test that fails today:** `audio.playAt(longClip, Vec3.zero)`, load another scene in single mode, then
  render. Today it's still audible.

## AU10. With no output, every retry spawns and joins a thread and redoes session/JNI work

- **Severity:** slow. **Size:** S.
- **Where:** crates/fynkel_audio/src/audio.rs:244-255 (`try_device`: `session::apply`, then
  `streamer.start()` spawns a thread, `Device::open` fails, then `streamer.stop()` joins it), retried
  every second from :570-576.
- **How it shows:** on a Mac mini with no speakers, or an Android device whose output is held elsewhere,
  the game thread spawns and joins an OS thread every second, indefinitely. On Android it also runs
  `prepare()` and `requestAudioFocus` through JNI each time. The cost is small, but it lands on the game
  thread and gets more frequent as `last_try` loops.
- **First fix:** open the device first and start the stream thread only on success. Back off the retry
  (1 s, 2 s, 4 s, up to 30 s) and reset the backoff on a route or default-device change.
- **Test that fails today:** none without a fake host. With a `Device` seam, count `streamer.start()`
  calls over 10 s of failing opens: today 10, with backoff 4 or fewer.

---

## Looked at and fine

- Mixer audio-thread discipline: no allocation, no free and no blocking in `render`. Clips and stream rings
  go back through the return ring and are dropped on the game thread (audio.rs:611-627). Overflow is held,
  not dropped (mixer.rs:375-386). The device callback only `try_lock`s, and the game thread locks the
  mixer only when no stream exists (device.rs:69 runs before `build_output_stream`; `drop_device` drops
  the stream first).
- The `Place` seqlock (mixer.rs:232-248) is sound for the single writer, and the reader rejects torn
  generations.
- Start, stop, pause, master, group and per-voice volume changes are all ramped (the one-pole gain, the
  `Fader` spans, the per-callback mix ramp). Group and voice fades wait while a voice is held.
- Streaming reads: window and refill logic for mono and stereo, pitch above 1, end of file (`drained`
  repeats the last frame), turning looping off mid-play (ends at the end of the current pass), starting
  part way with an early seek and pts-based skip, and the `starved` count only after the first two frames.
- Sample-rate mismatch: per-voice `step = clip.rate / out.rate * pitch` with linear interpolation, and
  `set_output` recomputes the ramp and filter coefficients when the device rate changes.
- Gapless MP3: symphonia 0.6 trims encoder delay and padding in the decoder (bundle-mp3 decoder.rs:131),
  so no `enable_gapless` option is needed.
- 3D: positions are read in `afterFrame` after the cameras move, so they are not a frame late. The
  listener falls back to the camera, then to the origin. Rolloff and equal-power pan match their docs.
- Pause seams: `world.paused` reaches the mixer in the same frame's `beforeFrame`. `followTimeScale`
  clamps pitch and holds below 0.05, never sending an invalid pitch. `applicationPause` suspends the
  device and the watchdog is reset.
- Destroyed and pooled objects: `AudioSource.onDisable` stops every voice of its owner (clip and shots)
  and unregisters it. Re-enabling gets a new owner, so old ends can't reach it.
- Hot reload: `AudioClip._reload` swaps the native clip, and playing voices keep their `Arc`.
  Finalisers are attached and detached correctly.
- The FFI never lets a panic cross (`with_audio` plus `guard`) and poisons the audio after one. Doubles
  go in through memory, as documented.
