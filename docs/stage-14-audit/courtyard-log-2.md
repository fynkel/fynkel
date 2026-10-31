# Courtyard — newcomer log

Started 17:44 (2026-10-31). Read only docs/writing-a-game.md and packages/*/lib + pubspecs
(lib sources only to check a constructor's parameters when the guide didn't show them).

## Timeline
- 17:44–17:45 read the whole guide (4,786 lines). Long, but nearly all of it was needed.
- 17:45 `flutter create --empty`, pubspec with four path deps, three WAVs from a Python sine script.
- 17:45 first `flutter test` (warm-up): 7 s. The engine libraries were already built on this Mac, so I
  never saw the "several minutes" first build; the hook printed `cargo build ... (the first build takes a few
  minutes)` and finished in 0.1 s each.
- 17:46–17:48 game.dart (468 lines) + banner.dart written; first `flutter analyze`: 1 error (playAt group).
- 17:48–17:49 game_test.dart (8 tests): 7 passed first run; the 1 failure was my test's timing.
- 17:49–17:50 hud.dart, settings.dart, main.dart: analyze clean first time. Widget test: 1 failure (my
  test didn't wait for a page transition), then green.
- 17:50 headless frame (HeadlessView → PNG) looked right first time: torches, spot pools, path ring,
  banner, shadows, fog.
- `flutter build macos --debug`: built in 18 s, no changes needed (only Xcode's usual 'Run Script' warning).
- Final: `flutter analyze` clean, `flutter test` 10/10 passing (8 headless World tests, 1 widget test, 1 HeadlessView picture test).

Total: about 10 minutes of wall time from empty folder to green tests. Far fewer surprises than expected.

## Findings, worst first

### confusing — the brief's "point lamps with shadows" can't be done
Guide §5 Shadows: "Point lights don't cast shadows yet." Clear and found on first read, but it changes the
design: the torches are unshadowed `Light.point`s and I added two shadowed `Light.spot`s over the yard plus a
dim moon `Light.sun(shadows: true)`. Suggest saying it in `Light.point`'s own doc and in §21 "What isn't there
yet" (it's not listed there under Drawing — I checked).

### confusing — music plays through a pause by default
A pause menu that should stop sound needs `audio.music.ignorePause = false`, and only "from the next track it
plays", so it has to be set before the first `play`. The guide says this (§13b) but only inside a long
sentence; easy to miss and a silent bug (nothing fails, the music just keeps playing). My pause test caught
it only because I set it first; a short "pausing everything" recipe in §11's Pause part would help, since
sounds/one-shots hold but music doesn't.

### minor — `playAt(..., group:)` takes an `AudioGroup`, not a name
Guide §13b: "a one-shot with `playOneShot(clip, group:)` or `playAt(..., group:)`", right after
`audio.group('effects')` is introduced by name. My first guess `group: 'effects'` failed: "The argument type
'String' can't be assigned to the parameter type 'AudioGroup?'". Right call: `group: audio.group('effects')`.
The compile error was clear; the guide could show the call.

### minor — widget tests: page transitions need a guessed wait
`pumpAndSettle` never settles with a view on screen (§19 says so). So after `Navigator.push`/`pageBack` I had to
guess a duration: `tester.frames(10)` was too short for the route's pop animation and the next tap hit the
fading barrier ("Resume" tap missed, world stayed paused). `tester.run(0.6)` fixed it. A helper like
`tester.settleUi()` (pump Flutter until no transitions, while the world steps) would remove the guess; at
least the guide could mention route transitions.

### minor — `world.run(t)` + routine waits round to frames
My "clear wave 1" test killed enemies at t = 0.3 s when the 4th had not spawned yet (spawns at 0.05 s gaps
land on 60 fps frame boundaries). My mistake, not the engine's, but `run` is described as "a second at 60
fps" only in a comment; a sentence that waits end on frame boundaries would help test writers.

### minor — not obvious which parameters exist without reading lib/
I opened lib/ sources for: `Follow(...)` named params, `Light.spot`/`Light.sun` params (is `color` there?),
`Pool` constructor, `Stick` params, `AudioMusic` members (`clip`, `time` for tests), `AudioStats.started`,
`addWith(name:)`, `Quat.identity`. All were where I'd expect; the doc comments are good. The guide can't list
everything — fine.

### unclear but turned out fine (worth a line in the guide)
- Setting a pooled dynamic body's `linearVelocity` / `position` in `Pool.get`'s reset function, before a new
  object is even added, worked without error. The guide says reset there, but I wasn't sure about rigidbodies.
- A trigger collider on a *child* of an enemy with the `Rigidbody` reported to the enemy's `with Triggers`
  when the player's `CharacterController` walked in. The guide's rules (callbacks go to "the collider's object
  and its rigidbody's object"; a controller "meets triggers as a kinematic rigidbody does") were enough, but I
  had to combine two paragraphs to be confident.
- `ParticleSystem.emit(30)` on a looping system with `rateOverTime: 0`, moved to the hit point first, works
  as a single reusable burst source. The guide's sparks example makes a new object per burst; the cheaper
  "one system, move it, emit" pattern could be shown.

### pleasant
- The guide's examples compile as written; nearly every API I used compiled on the first guess
  (`Pool`, `addWith`, `Follow`+`LookAt`, `Spline.smooth(closed:)`, `Mesh.sweep`, `SplineAnimate`,
  `CharacterController.move`, `physics.overlapSphere(triggers:)`, `audio.music.crossFade`, `Stick`,
  `pausedListenable`, `FynkelApp(shortcuts:)`).
- The §12b `Waves` example made the waving banner a few minutes' work.
- Headless tests are excellent: physics, triggers, particles (`particleCount`), 3D sound
  (`audio.stats.started`), music (`music.clip`, `music.time`) and pause all testable with no device, and fast
  (whole suite < 1 s after the build).
- `HeadlessView(...).png()` let me *see* the scene from a test; the first picture was right.
- `tester.game<T>()` / `tester.frames()` made the HUD → pause → settings → resume widget test short.
- Error messages I hit were all Dart analyzer ones and clear.
