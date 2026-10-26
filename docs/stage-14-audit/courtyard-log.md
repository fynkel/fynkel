# Courtyard — a newcomer's log

Started 22:49 (2026-10-26). Read only docs/writing-a-game.md and packages/*/lib.

## Timeline
- 22:49 start. Read the guide end to end (4,389 lines), then a few public files (pool, cameras, audio mix/source, physics ctors, input).
- 22:50 `flutter create`, pubspec, FynkelActivity, minSdk 30, WAVs from a Python script (tool/make_sounds.py).
- 22:50 first `flutter test` (warm-up): engine already built by cargo, 23 s — but it FAILED (see F1). Retry passed in 7 s.

## Findings
Ranked worst first. Ratings: blocker / confusing / minor.

### F1 — confusing (near-blocker on a first run): the build hook races when two games build at once
First `flutter test` died with an unhandled exception from the hook:
`PathNotFoundException: Cannot copy file to '.../hooks_runner/shared/fynkel_physics/build/.../libfynkel_physics_ffi.dylib',
path = '<engine>/target/aarch64-apple-darwin/release/libfynkel_physics_ffi.dylib'` + a Dart stack trace through
`buildRustCrate (package:fynkel/hooks.dart:86)`. The log was interleaved with ANOTHER app's test run
(`/tmp/s14/games/marble/...`): every game shares the engine repo's `target/` dir, so one game's cargo build replaced
the dylib while mine copied it. Re-running fixed it. Nothing in the message says "retry" or "another build is running";
the guide (§1) only says the first build is slow. Workaround: run again.

### F2 — confusing: §19 never says how a headless test loads assets
My first test with music failed: `Bad state: this World has no asset source to load hit.wav from: make it with
World(source: ...)`. The message is good, but §19 (and §13b, which says sound works in tests) never shows a test that
loads a file, and `AssetSource` isn't described anywhere in the guide. I found the typedef in assets.dart
(`Future<Uint8List> Function(String name)`). Workaround: `World(source: (name) => File('assets/$name').readAsBytes())`.
A one-line example in §19 would do.

### F3 — confusing: `world.pausedListenable` is an extension from fynkel_flutter, and §11 doesn't say
§11 tells you to use `game.world.pausedListenable` in a pause panel. In my HUD file (which, per §17's advice, imports
Flutter and only `show Vec2` from fynkel) it failed: "The getter 'pausedListenable' isn't defined for the type 'World'".
It's `WorldListenables` in package:fynkel_flutter. Fix: `import 'package:fynkel_flutter/fynkel_flutter.dart' show
WorldListenables;`. The guide should name the import (the analyzer's "try importing the library" doesn't say which).

### F4 — confusing: §17 mentions `Stick(game.world.input)` as if it were a widget
"a widget beside the view can take `game.world.input` while it's built, as an on-screen stick does
(`Stick(game.world.input)`)". There's no `Stick` in any package (fynkel_flutter exports no joystick). I wrote my own
(GestureDetector -> `input.setAxis2('move', ...)`, ~45 lines). Fine, but the text reads like a shipped widget; say it's
your own, or ship one (Unity has OnScreenStick).

### F5 — minor: the brief's "point lamps with shadows" can't be done
§5 says plainly "Point lights don't cast shadows yet". Workaround: each torch is a point lamp (light) plus a spotlight
above it aimed down and in (shadows). Clear in the guide, so not confusing — just a gap vs. Unity.

### F6 — minor: music plays through a pause by default; `ignorePause` applies from the next track
The brief wants pause to stop sound. §13b says music plays on through a pause (sensible default), and
`AudioMusic.ignorePause`'s doc says "From the next track", so it must be set before `music.play`. I set it before the
first track; a test checks `music.time` holds while paused. The guide could mention `audio.music.ignorePause` by name in
the music paragraph (I found it in mix.dart).

### F7 — minor: noise in test output
Every test that picks `Quality.low` prints "Fynkel: a camera asks for bloom, but the quality drawn (low) doesn't draw
it..." — expected per §5, but it shows in otherwise clean test runs; there's no way to say "I know" per camera.

### F8 — minor: tests of a pooled wave depend on the routine's timing
Nothing wrong in the engine; my own wave test first failed because I released enemies while the routine was still
spawning (my bug). Worth a sentence in §19: when testing a routine, run past its waits before acting.

## What surprised me (no change needed)
- `Light.ambient(intensity:)` guessed right; `Follow` + `LookAt` work on a plain `Camera` without a `VirtualCamera`.
- `physics.overlapSphere` returns the player's `CharacterController` too (enemy bites use it) — I wasn't sure it would.
- `ParticleSystem(playOnAwake: false, looping: false)` + `emit(40)` after moving the object: works, no restart needed.

## What was pleasant
- ~580 lines of game written straight from the guide: `flutter analyze` clean on the FIRST run of lib/game.dart.
  Only one compile error in the whole project (F3).
- All 10 headless game tests passed on their first run; the only failures were my own test mistakes.
- Pool / ObjectHandle / `release(after:)` / `onEnable` reset exactly as §3 says; `world.all<Enemy>()` skips released ones.
- `world.paused` really stops time, physics, input and (with ignorePause off) music: one flag.
- `audio.render(frames)` let a test prove the hit sound is heard from the right — lovely.
- `HeadlessView(...).png()` gave a picture of the scene in a test (torches, fog, path, banner, shadows all visible),
  without a device.
- Widget tests of the real `FynkelApp` (pause button, settings radio + slider, Escape) worked with `tester.pump(17 ms)`
  as §19 says.
- Error messages from the engine itself (asset source) told me exactly what to do.

## Time
- 22:49 start → 22:50 project set up → ~22:52 game.dart + hud.dart analyzed clean → 22:53 headless tests green →
  22:54 sound + widget tests green, headless screenshot → 22:55 done. Reading the guide was most of the effort;
  the native build was already cached (23 s), so no multi-minute cold build here.
