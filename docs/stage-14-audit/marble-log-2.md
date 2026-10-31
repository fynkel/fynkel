# Marble Run — newcomer's log

Read only docs/writing-a-game.md and packages/*/lib + pubspecs.
Clock: started 17:45, app created 17:45, game written 17:48, all tests
green 17:48 (first run), extra tests/screenshot/log by 17:51. About 6 min
wall-clock in all. The native build was already warm on this machine
(cargo "Finished in 0.09s"), so I can't judge the "few minutes" first build.

## Findings (worst first)

1. **confusing — no way to check "no garbage" in a test.** The brief (and
   the guide's many "makes no garbage" claims, §12b, §13c, §20) made me look
   for a check. §20 says only: run `flutter run --profile` and open
   DevTools' Memory view. testing.dart has nothing for it. Workaround: a test
   that a drag frame sweeps into the *same* `Mesh`/`Spline` once a frame
   (`identical`, `mesh.shapeVersion + 1`), which is a proxy, not an
   allocation count. A sentence saying "a test can't count allocations; check
   with DevTools" would have saved the search.

2. **minor — guide says `mesh.shapeVersion` comes "from
   `package:fynkel/extend.dart`"** (§12b). It's also exported by
   `package:fynkel/fynkel.dart`, so importing extend.dart as told gives the
   analyzer's `unnecessary_import` info. Fix: drop the import.

3. **minor — what a shader can read isn't in the guide.** §14 sends you to
   `packages/fynkel/wgsl/fynkel.wgsl` and `examples/lab/assets/shaders/`
   (outside what a package user's lib/ shows, and examples aren't shipped).
   I guessed `in.uv` from the lava example. Which UV axis runs *along* a
   sweep is only in `Mesh.sweep`'s doc comment (u around, v along); the
   guide says just "UVs in metres". It worked first time (stripes scroll
   down the track), but by luck of the guess.

4. **minor — `AudioClip.fromSamples` must be `dispose()`d, but when?** The
   guide says asset clips free themselves, made ones need `dispose()`. A
   clip owned by the root and used by `playAt` one-shots: dispose in
   `onDestroy`? Is it safe while a one-shot still plays? Not said. I don't
   dispose it (a leak per FynkelGame restart, probably).

5. **minor — sweeping into an empty `Mesh()`.** The guide shows
   `Mesh.sweep(...)` then `setSweep` on that mesh. I did `Mesh()` +
   `setSweep` from the start (one code path for first build and rebuilds).
   It works, but nothing says it's allowed.

6. **minor — where to set `time.fixedDelta`.** §16/§20 advise
   `time.fixedDelta = 1 / 100` for small fast balls but not where. I set it
   in the root's `onLoad`; it worked. A line ("in the root's onLoad") would
   remove the doubt.

7. **minor — `world.all<T>()` while destroying.** Reset loops over
   `world.all<Marble>()` and destroys each. Whether `all` is a copy (safe to
   destroy while iterating) isn't said; it worked.

8. **minor — the swept channel hid the marbles** from my first camera (side
   on). My mistake, not the engine's, but the headless `view.png()` (§19)
   found it in one shot — see "pleasant".

## Compile errors / wrong first guesses

None. `flutter analyze` was clean on the first write of lib/, and all six
first tests passed on their first run. Only analyzer nits later: an unused
import in a test, and #2 above.

## Workarounds written

- Drag-frame "garbage" test as identity/shapeVersion proxy (#1).
- `Hud(saveFile:)` so a widget test can save without path_provider.
- Knot handles sit 0.6 m toward the camera (`KnotHandle.lift`) so the
  track's MeshCollider (which also takes taps, §11) isn't hit first.

## Pleasant

- Everything I needed was in the guide, under the name Unity uses:
  `Spline.smooth`, `spline[i] = Knot(...)`, `setSweep`, `MeshCollider()`
  following its mesh, `with Collisions`/`with Triggers`,
  `collision.relativeVelocity`, `audio.playAt(volume:)`, `TrailRenderer`,
  burst `ParticleSystem` with `StopAction.destroy`, `rotateBy(loops: -1)`,
  `punchScale`, `Sky.gradient`, `Light.sun(shadows: true)`.
- The "lower `gameObject.id` acts once per pair" tip for collisions.
- `AudioClip.fromSamples` meant no sound asset was needed; sound runs in
  tests with no device.
- Headless tests: `World(source:)` loading the real .wgsl, `world.run`,
  `input.simulate.fingerDown/Move/Up` + `worldToScreenPoint` for a real
  drag test, `physics.raycast` to prove the collider moved.
- `HeadlessView` + `shadersReady` + `view.png()` gave a real picture from a
  test in ~10 s: the only way I could see the game without a device.
- `fynkel_flutter/testing.dart` (`tester.frames`, `tester.run`,
  `tester.game<T>()`) made the whole-app test (tap Go, see score, Reset)
  10 lines.
- Errors-fail-the-test by default: I'd have known at once if a callback threw.
