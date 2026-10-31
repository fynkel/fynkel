# Stage 14 public API pass: package:fynkel_audio

Area: `packages/fynkel_audio/lib` (fynkel_audio.dart, src/audio.dart,
clip.dart, listener.dart, mix.dart, source.dart, owned_memory.dart), at
main `bf27879a`. Known rows in docs/stage-14-findings.md and guide §21
are not repeated. Throwaway probes: `/tmp/s14api/scratch-audio/test/probe_test.dart`
and `probe2_test.dart` (headless worlds, `audio.render`, `audio.stats`).

---

## A1: A disabled or never-enabled source plays, and outlives its object (looping forever)

- **Where:** `src/source.dart:256-264` (`_system`), `:269` (`play`), `:323` (`playOneShot`), `:443-453` (`onDisable`); class doc `:24-25`.
- **Now:** `play()` and `playOneShot()` reach the world's sound through `_system`, which works whether or not the source is enabled. Only `onDisable` stops a source's sounds, and a source that was never enabled (added to an inactive object, or `enabled = false` before `play()`) never gets an `onDisable`. The source isn't in `_enabled` either, so a 3D one doesn't follow its object.
- **Why it matters:** the class doc says "Disabling it, or destroying its object, stops all its sounds". A pooled or prefab-style object that's built inactive, set up, and then destroyed leaves its sound playing. With `loop: true` it plays until the world is disposed, with nothing left to call `stop()` on. Unity refuses: "Can not play a disabled audio source".
- **Verified (test):** inactive object + `AudioSource(loop: true)` + `play()` gives `isPlaying true`. After `o.destroy()` and 600 frames (10 s), `stats.voices` is 1 and the render peak is still 0.1. A source with `enabled = false` then `play()` also plays (`isPlaying true`), and its voice survives the object's destroy.
- **Fix:** when the source isn't `isActiveAndEnabled`, `play`/`playOneShot` should do nothing (Unity's way, perhaps with an assert or a `world.report` warning), or throw `StateError('$this is disabled: enable it (or its object) before play()')`. Either way, a source's sounds must not outlive it: also call `stop()` from destroy when `_audio != null`, even if `onDisable` never ran.
- **Breaks code?** Only code that relies on a disabled source playing. That is undocumented, and the docs say otherwise.
- **Rating:** **bug**

## A2: Setting `music.volume` applies `loop` and `group` to the playing track, though both say "From the next track"

- **Where:** `src/mix.dart:105-115` (`volume` setter); docs at `:93-99`.
- **Now:** `_audio._set(_voice, looping: loop, group: group._index)` sends the current `loop` and `group` along with the volume.
- **Why it matters:** suppose a game sets `music.loop = false` or `music.group = cutsceneGroup` for the next track, then moves a volume slider. The playing track then stops looping (and fires `onEnd`) or jumps into another group. Without the volume change, the same settings correctly wait for the next track. So the result depends on an unrelated call.
- **Verified (test):** play a 0.2 s track, set `loop = false`, then `volume = 0.9`. It ends, `onEnd` fires, and `isPlaying` is false. The control case (`loop = false` alone) keeps looping. With `group = quiet` (volume 0) and then `volume = 1`, the playing track's peak goes 0.1 → 0.0.
- **Fix:** remember the looping and group the track started with (`_trackLoop`, `_trackGroup`) and send those from the `volume` setter. Or, alternatively, make `loop`/`group` apply at once and say so (the mixer command exists: `_set`).
- **Breaks code?** No.
- **Rating:** **bug**

## A3: `music.play(clip, fadeIn: <bad>)` stops the current track, then throws

- **Where:** `src/mix.dart:125-129` (`play` calls `stop()` before `_start`, which runs `_checkFade` at `:157`).
- **Now:** the value is checked only after the playing track has been stopped. `crossFade` checks first (`:135`); `play` doesn't.
- **Why it matters:** a bad value (say `fadeIn: -1` from a computed duration) silences the music as well as throwing. Code that catches the error is left with no music.
- **Verified (test):** play track A, then `play(B, fadeIn: -1)`. It throws `Invalid argument (fadeIn): is not 0 to 3600: -1.0`, and afterwards `isPlaying false`, `clip null`, peak 0.
- **Fix:** call `_checkFade(fadeIn, 'fadeIn')` at the top of `play`.
- **Breaks code?** No.
- **Rating:** **bug** (small)

## A4: Setting `minDistance` past `maxDistance` blames `maxDistance`

- **Where:** `src/source.dart:169-171`, `:467-470` (`_checkDistances`); the constructor at `:65`.
- **Now:** `source.minDistance = 600` (with the default max of 500) throws `Invalid argument (maxDistance): is not above minDistance (600.0): 500.0`. `AudioSource(minDistance: 600)` gives the same message.
- **Why it matters:** the game never touched `maxDistance`, yet the error names it. Setting both on a playing source to a range above 500 only works in one order (max first), and nothing says so. `maxDistance`'s doc (`:177-178`) doesn't state the constraint either; only `minDistance`'s does.
- **Verified (test):** quoted above.
- **Fix:** the setter for `minDistance` should throw on `'minDistance'` with "is not below maxDistance (500.0); set maxDistance first". The constructor should name whichever argument the caller gave. Add "above [minDistance]" to `maxDistance`'s doc. Optionally add `setDistances(min, max)`.
- **Breaks code?** No.
- **Rating:** **unhelpful error**

## A5: Groups are always passed as `AudioGroup`, and `AudioSource(...)` has no `group:` argument

- **Where:** `src/source.dart:38-52` (constructor), `:200-204` (`group`); `src/audio.dart:332` (`playOneShot(group:)`), `:369` (`playAt(group:)`); `src/mix.dart:95` (`music.group`).
- **Now:** everywhere a group goes in, it is an `AudioGroup` made by `audio.group('name')`, so the package is consistent. But the guide introduces groups by name ("`audio.group('effects')`… a one-shot with `playOneShot(clip, group:)`"), and the second fresh developer wrote `group: 'effects'` (their one compile error). Every other source setting is a constructor argument except `group`, so a grouped source takes two statements (`addComponent(AudioSource(...))..group = audio.group('effects')`).
- **Why it matters:** this is the one thing a fresh developer got wrong. It's also extra boilerplate for the most common settings-screen setup (every effect source in "effects").
- **Fix:** keep `AudioGroup` as the one type, and add `AudioGroup? group` to the `AudioSource` constructor. In the guide, write the named argument as `group: audio.group('effects')` wherever it's introduced. A `String` form isn't worth a second parameter: Dart has no overloads, and `groupName:` next to `group:` would be two ways to say one thing.
- **Breaks code?** No (additive).
- **Rating:** **awkward**

## A6: A group from another world is silently used as that world's group with the same number

- **Where:** `src/audio.dart:783` (`_indexOf`); used by `AudioSource.group`, `playOneShot`, `playAt` and `music.group`.
- **Now:** only the group's index is sent, with no check that `group` belongs to this `Audio`.
- **Why it matters:** an app with two worlds (two `FynkelView`s, a menu world and a game world, or tests that keep a group in a field) can play under the wrong group's volume, or no group's, with no error.
- **Verified (test):** world A's `effects` (volume 0) passed to world B's `playOneShot` plays at full level. Once B has its own group #1 (`loud`), the sound plays under `loud`.
- **Fix:** in `_indexOf`, check `identical(group._audio, this)`, and if not, throw `ArgumentError.value(group, 'group', 'belongs to another world\'s audio: use this world\'s audio.group(\'${group.name}\')')`.
- **Breaks code?** Only code that is already wrong.
- **Rating:** **bug** (minor)

## A7: Music calls its position `time`, the source calls it `playTime`, and music's can't be set

- **Where:** `src/mix.dart:90-91` (`AudioMusic.time`) vs `src/source.dart:237-254` (`AudioSource.playTime`, documented as named so "`time` is the game's here").
- **Now:** the same idea has two names. Music's is read-only, so a game can't resume a track where it left off after a scene change, or start one partway in. The guide's streaming paragraph also says "from any `time`".
- **Fix:** rename it to `AudioMusic.playTime` with a setter (seek). Keep `time` as a `@Deprecated('use playTime')` getter for a stage. Allow a start position too: `play(clip, fadeIn:, from:)`.
- **Breaks code?** Not with the deprecated alias.
- **Rating:** **misleading** (two names for one thing)

## A8: One-shots give nothing back: no way to know one ended, or to stop just that one

- **Where:** `src/audio.dart:355` (`playOneShot` returns `void`), `:388` (`playAt`), `src/source.dart:323` (`AudioSource.playOneShot`).
- **Now:** `onEnd` covers only a source's `clip`. A world-level one-shot can only be stopped with every other one (`stopOneShots`).
- **Why it matters:** "play the line, then open the door", or chaining a jingle after a win sound, needs a timer guessed from `clip.length / pitch`. That guess is wrong if the game pauses, `followTimeScale` changes, or the voice is stolen. Unity's `PlayClipAtPoint` returns nothing either, but in Unity you can use a temporary AudioSource object. Here, the equivalent needs a throwaway `GameObject` + `AudioSource` + `onEnd` + destroy.
- **Fix:** return a small `AudioVoice`/handle (or `int`) from the three calls, with `isPlaying` and `stop()`, or accept an `onEnd:` callback. The mixer already reports each handle that stops (`_frame`, `fa_stopped_read`). Returning a value doesn't break callers that ignore it. Watch allocations: these calls are allocation-tested, so an `int` handle or a callback is cheaper than an object.
- **Breaks code?** No.
- **Rating:** **awkward**

## A9: Sources can't fade, and music can't pause

- **Where:** `src/source.dart:308-317` (`stop()`: a few ms fade only), no `fade`; `src/mix.dart` (`AudioMusic`: no `pause`/`unPause`).
- **Now:** groups have `fade(to, seconds)`, and music has `play(fadeIn:)`, `crossFade` and `stop(fadeOut:)`. An `AudioSource` (an ambient loop, a campfire, an engine hum) can only be faded by setting `volume` every frame by hand. The only ways to stop and resume music are `world.paused` (with `ignorePause = false`, which applies from the next track) and `group.mute`, which keeps it running.
- **Why it matters:** fading out an ambient loop when you leave an area, or pausing the music for a cutscene and resuming it, are common, and each needs boilerplate. The mixer already has per-voice fades (`fa_voice_fade`, used by music).
- **Fix:** add `AudioSource.stop({double fadeOut = 0})` and `AudioSource.fade(double to, double seconds)`, mirroring music and groups, and `AudioMusic.pause()`/`unPause()`, mirroring the source.
- **Breaks code?** No (an optional named argument plus new members).
- **Rating:** **awkward**

## A10: `AudioClip.fromSamples` takes a negative rate as 4 GHz, and its errors don't name the argument

- **Where:** `src/clip.dart:83-95` (no Dart-side check; `int` → `u32` in FFI).
- **Now:** `fromSamples(samples, rate: -1)` succeeds with `rate=4294967295`, `length=2.3e-9`, and the clip plays as nothing. A negative `channels` turns into "a clip has 1 or 2 channels, not 4294967295". Other errors come from Rust as bare `ArgumentError(message)` with no argument name or value: "a clip needs a sample rate above 0", "3 samples aren't whole frames of 2 channels", "sample 0 isn't a number". These read well enough, but the rate case is silent.
- **Fix:** check in Dart with `if (rate <= 0) throw ArgumentError.value(rate, 'rate', 'is not above 0')` and `if (channels != 1 && channels != 2) throw ArgumentError.value(channels, 'channels', 'is not 1 or 2')`.
- **Breaks code?** No.
- **Rating:** **unhelpful error** (silent for a negative rate)

## A11: Playing a freed clip: "the clip  was freed"

- **Where:** `src/clip.dart:157`.
- **Now:** `StateError('the clip $name was freed')`. For a clip from `fromSamples` without a name, the message has a double space and says nothing about which clip. Nor does it say why: "freed by `assets` when the last object that asked for it was destroyed" is the likely cause, and the doc of `isDisposed` explains it.
- **Verified (test):** prints `Bad state: the clip  was freed`.
- **Fix:** use `'$this was freed (by dispose(), or by assets when the last object that loaded it was destroyed): load it again, or load it from an object that stays'`. `toString` already handles an empty name.
- **Breaks code?** No.
- **Rating:** **unhelpful error**

## A12: `AudioSource.playOneShot` accepts and clamps a pitch that `Audio.playOneShot` refuses

- **Where:** `src/source.dart:329` vs `src/audio.dart:339`, `:374`, `:778-779`.
- **Now:** the source's check is `speed > 0 && speed <= 8` ("is not above 0 and up to 8"), and then it clamps to 0.05. The world's `playOneShot`/`playAt`, the `pitch` setter and the constructor all refuse anything below 0.05. So `pitch: 0.01` plays at 0.05 on a source and throws on the world.
- **Why it matters:** this is a small inconsistency in what counts as out of range, and the guide promises "a pitch of 0 throws". The source multiplies by its own pitch, so a range for the product makes sense. If so, say it in the doc.
- **Verified (test):** `source.playOneShot(c, pitch: 0.01)` doesn't throw; `audio.playOneShot(c, pitch: 0.01)` throws.
- **Fix:** use the same check (0.05 to 8) and message, or document that the source clamps the product.
- **Breaks code?** Only calls that pass 0 < pitch < 0.05.
- **Rating:** **minor**

## A13: `playTime` past the clip's end plays nothing and calls `onEnd` at once

- **Where:** `src/source.dart:246-254`, `:277` (clamped to `clip.length`).
- **Now:** `playTime = 5` on a 1 s clip, then `play()`: nothing is heard, and on the next frame `isPlaying` is false and `onEnd` has fired. The setter's only check is `>= 0`.
- **Why it matters:** a saved position from a longer clip, or seconds mixed up with frames, fails silently, and the error shows up as a spurious `onEnd`.
- **Fix:** if a clip is set, refuse `value > clip.length` with `ArgumentError.value(value, 'playTime', 'is past the end of ${clip.name} (${clip.length} s)')`. Or document the clamp.
- **Breaks code?** Only code that relies on the silent clamp.
- **Rating:** **minor**

## A14: `AudioStats.toString` leaves out `reopened`

- **Where:** `src/audio.dart:136-140`.
- **Now:** every counter except `reopened` is printed. Yet that's the one a phone log needs (AU8/AU-voice work).
- **Verified (test):** prints `AudioStats(no device 48000 Hz × 2, 0 voices, started 0, stolen 0, refused 0, callbacks 0, underruns 0, starved 0, 0.000 ms a frame)`.
- **Fix:** add `reopened $reopened`.
- **Breaks code?** No.
- **Rating:** **minor**

---

## Checked and fine

- **Unity names:** `play`/`pause`/`unPause`/`stop`/`playOneShot`, `volume`, `pitch`, `loop`, `mute`, `priority`, `playOnAwake`, `spatialBlend`, `panStereo`, `minDistance`/`maxDistance`, `rolloffMode` (`AudioRolloffMode.logarithmic`/`linear`), `isPlaying` (false while paused, as Unity), `AudioListener`, `AudioClip.channels`/`length`. Where they differ, the doc says so: `playAt` (PlayClipAtPoint), `AudioGroup` (AudioMixerGroup), `group` (outputAudioMixerGroup), `ignorePause` (ignoreListenerPause), `playTime` (time), `rate` (frequency), `frames` (samples), `audio.volume` (AudioListener.volume).
- **Defaults match Unity:** volume 1, pitch 1, 2D, priority 128, min 1/max 500, logarithmic, playOnAwake true, loop false, ignorePause false. `followTimeScale` is off (Unity has none). Music's `loop = true` and `ignorePause = true` are deliberate and documented.
- **Range errors** for volume, pitch, priority, pan, fade seconds, `render(frames)`, the 16th group (it lists the groups there are), no clip to `play`, a source on no object yet, after dispose, the ABI mismatch, and junk bytes (a `FormatException` with the file's name). NaN is refused everywhere a double is checked.
- **Behaviour:** `music.volume` during a fade-in doesn't cancel the fade. `play()` on an inactive object and then activating it doesn't start a second voice. `music.isPlaying` stays true while held by the game's pause (as Unity under AudioListener.pause). `crossFade` checks its value before touching the track. `stopOneShots` leaves sources and music alone, as documented.
- **Docs:** the library, `Audio`, `AudioSource`, `AudioClip` (memory estimate, streaming), `AudioSession`, `AudioGroup` and `AudioListener` (last enabled hears) match the code. The only false doc sentence found is the "disabling or destroying stops all its sounds" one in A1.
- `OwnedMemory` is not exported (internal), so it is out of scope as public API.
