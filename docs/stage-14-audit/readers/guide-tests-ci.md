# Stage 14 audit: the guide, examples, tests, CI, package surface

Reader: guide-tests-ci. Read-only; nothing was built or run here except `gh run list/view`
(read-only) for CI history. Line numbers are on branch stage-14 at edbe57aa.

---

## GT1. No performance number is ever checked automatically
- **Severity**: gap. **Size**: M.
- **Where**: `.github/workflows/ci.yml` (no bench or measure step anywhere);
  `crates/fynkel_mesh/tests/made.rs:333-334` (`#[test] #[ignore] fn what_making_meshes_costs`),
  `crates/fynkel_mesh/tests/mesh.rs:~540` ("ignored, a measurement"), `crates/fynkel_mesh/tests/sweep.rs:~237`
  (`what_sweeping_costs`, ignored), `crates/fynkel_physics/tests/meshes.rs:257` (prints only),
  `examples/scale/test/scale_test.dart:19-30` (times 600 frames of 10,000 objects and only `print`s the median),
  `packages/fynkel_measure/bin/measure.dart` (rate/bench/lifecycle, run by hand only).
- **How it shows**: the CI log of run 40200810608 lists each of these as `ignored` or prints numbers
  without an assertion. Every bar in the stage reports (Note10+ mesh change 1.3 ms vs 1 ms, Sheepdog 60 fps,
  10k particles 0.03 ms) is measured once by hand; a 2x slowdown passes CI. The only automatic checks near
  speed are complexity ratios (`component_test.dart:221`, `routine_test.dart:84`, `signal_test.dart:149`) and
  the allocation gates, which are good but don't catch a constant-factor regression.
- **First fix idea**: phase 4's plan: on the self-hosted runner, run the release-mode measurements
  (`--ignored` Rust benches, scale's frame times, `fynkel_measure bench` headless) against a committed baseline
  file with a tolerance (e.g. median +25 %), failing the PR. Start with the Mac; the devices stay manual.

## GT2. The lab soak is a flaky gate: it has failed on its own thresholds three times in three days, once on main
- **Severity**: bug (in the test). **Size**: S–M.
- **Where**: `examples/lab/test/soak_test.dart:95,117,118`; tuned in `a203294e` (2026-10-22) and
  `15b6d004` (2026-10-23).
- **How it shows** (from `gh run view --log-failed`):
  - run 39684829147 (stage-11, 10-21): malloc growth 20,925 B/round vs limit 20,000;
  - run 39768497043 (stage-11-mix, 10-22): footprint growth 296,141 vs 250,000;
  - run 40025387249 (**main**, after the #142 merge, 10-23): footprint growth **1,206,272** vs 250,000
    (process 218 → 289 MB over 120 rounds). The next main run passed with no soak change, so either the gate
    is noise at that level or a real 1.2 MB/round growth comes and goes. Neither can be told apart today. The
    two retunings changed only the malloc comparison; the footprint check that failed on main is unchanged.
- **First fix idea**: make the gate tell a leak from noise: count engine-owned memory directly (the engine's
  own live-object/byte counters from the C ABI, plus GPU memory counters) rather than process footprint; keep
  the footprint as a printed trend. Rerun the 10-23 main commit's soak a few times to find out whether the
  1.2 MB/round was real.

## GT3. The soak never draws: the renderer, the render worker and GPU uploads are outside it
- **Severity**: gap. **Size**: S.
- **Where**: `examples/lab/test/soak_test.dart:43` (`World(source: files)`, no `HeadlessView` anywhere in the
  file).
- **How it shows**: 120 rounds of every scene with no view means no extract, no mesh/texture upload, no
  pipeline creation for custom shaders or particle materials. A GPU-side leak per scene (a texture or
  pipeline never freed when a scene's world is disposed) can't show in it. `crates/fynkel_render/tests/gpu_memory.rs`
  and `texture_memory.rs` check steady frames of a fixed scene only, not scenes coming and going.
- **First fix idea**: give the soak a small `HeadlessView` (e.g. 160x120) and draw every 10th frame; hold
  the renderer's GPU byte counter flat as well.

## GT4. The platform code a phone runs is never compiled in CI
- **Severity**: gap. **Size**: S.
- **Where**: `.github/workflows/ci.yml` (only `flutter build macos --debug` per example);
  `tools/test-mobile.sh:23` (`crates=(fynkel_present fynkel_render fynkel_ffi)`).
- **How it shows**: not built for a phone target by anything automatic:
  - `packages/fynkel_flutter/android/src/main/kotlin/.../FynkelFlutterPlugin.kt` (355 lines) and
    `FynkelActivity.kt`: Kotlin is only compiled by a `flutter build apk/run`;
  - the iOS-only branches of the shared Swift plugin (`FynkelFlutterPlugin.swift` has 8 `#if`/`#else`,
    `FynkelTexture.swift` 2, `FynkelLayer.swift` 1): macOS builds compile only the AppKit side;
  - `crates/fynkel_audio/src/session.rs:29-129` (the iOS AVAudioSession and Android focus code behind
    `cfg(target_os)`) and `fynkel_physics_ffi`/`fynkel_audio_ffi` for `aarch64-apple-ios`/`aarch64-linux-android`
    (test-mobile.sh cross-compiles only the three render crates);
  - the build hook's triple mapping for iOS device/simulator and Android (`packages/fynkel/lib/hooks.dart`).
  A broken Kotlin import or an iOS-only Swift type error merges green and is found at the next device run,
  which for iPhone has been "owed" for stages 12 and 13 (MEMORY).
- **First fix idea**: one CI step: `cargo build --release -p fynkel_audio_ffi -p fynkel_physics_ffi` for the
  iOS and Android triples, plus `flutter build apk --debug` and `flutter build ios --debug --no-codesign`
  for one example (hello), which compiles Kotlin, iOS Swift and the hooks for both.

## GT5. Pocket Hole is outside CI, and doesn't follow the guide's Android setup
- **Severity**: gap. **Size**: S.
- **Where**: `.github/workflows/ci.yml` (the examples list: hello, garden, coins, arena, bowling, sheepdog,
  lab, scale; no pocket_hole); `examples/pocket_hole/android/.../MainActivity.kt`
  (`class MainActivity : FlutterActivity()`); guide `docs/writing-a-game.md:71-82`.
- **How it shows**: it has tests (`examples/pocket_hole/test/game_test.dart`, 4 tests) that no one runs, isn't
  analyzed or formatted, and has had ABIs 41–44 land since it merged (dbb02d3e, 2026-10-22) with no check that
  it still compiles (not verified either way here). It's the one example that is a shippable game (store icon,
  PRIVACY.md), yet it's the only one on `FlutterActivity`, the texture path the guide says costs 51 → 59 fps
  on the Note10+. Also `examples/lab/tool/` isn't format-checked (ci.yml's lab line is `lib test`).
- **First fix idea**: add pocket_hole to the CI examples block (analyze, test, build macos); switch it to
  `FynkelActivity`; add `tool` to the lab's format line.

## GT6. Moving a static collider leaves what rests on it asleep; Pocket Hole wakes blocks by hand
- **Severity**: awkward (gap in the engine or the guide, to be confirmed by a checker). **Size**: S.
- **Where**: `examples/pocket_hole/lib/game.dart` `onFixedUpdate` (moves the static `MeshCollider` floor via
  `floor.position = target`, then loops all blocks in an AABB around the old/new hole and calls
  `get<Rigidbody>()!.wakeUp()`); engine path `crates/fynkel_physics/src/colliders.rs:314-337` (`follow`) →
  `put` at `:433-447` (sets the collider pose, pushes to `dirty`, wakes nothing); guide
  `docs/writing-a-game.md:2708-2709` ("A collider with no Rigidbody is static; a game moves it by moving its
  object").
- **How it shows**: the only waking in the physics crate is on gravity change (`lib.rs:161-164`), forces,
  `set_sleeping`, teleports of bodies, and joint drives; nothing wakes bodies resting on a static collider
  that moved. The example's hand-written AABB wake loop is the workaround. (I didn't run Rapier to confirm
  it doesn't wake contacts of a moved parentless collider itself; the example's loop suggests it doesn't.)
- **First fix idea**: either wake the bodies in contact with a static collider when `follow` moves it
  (cheap: its contact pairs), or say in §16 that a collider that moves wants a kinematic `Rigidbody` and
  `movePosition`, and change Pocket Hole to that.

## GT7. No way to see what a frame costs per system; the guide points at a Profiler that doesn't exist
- **Severity**: gap (and a wrong guide claim). **Size**: M.
- **Where**: `docs/writing-a-game.md:1442-1443` ("tens of thousands want a look at the Profiler
  (`world.meshChangeTime` ...)"); `packages/fynkel/lib/src/world.dart:364` (`particleStepTime`), `:372`
  (`meshChangeTime`): the only two per-system timings. Nothing for the physics step, animation/posing,
  audio, routines/updates, extraction or the GPU frame.
- **How it shows**: the lab rebuilds the same rolling-window median timer six times:
  `examples/lab/lib/scenes/crowd.dart:154-184` (a private `WorldSystem` from `package:fynkel/extend.dart`,
  wrapping `beforeUpdate`/`afterUpdate`, to time animation), `particles_basics.dart:196`,
  `particles_look.dart:327`, `particles_world.dart:315`, `meshes_change.dart:132`, `splines.dart:454`.
  There is no "Profiler" in Fynkel 2 (grep finds none in packages/; the word is used only for Unity's).
  Phase 4's "frame broken down against a budget per device" has no API to read the breakdown from.
- **First fix idea**: one `world.frameTimes` (or `FrameStats`) with fixed fields per system (updates,
  physics step, animation, particles, meshes, audio mix, extract, render CPU, GPU), filled without
  allocation, shown by an optional overlay and read by fynkel_measure; fix the guide's sentence.

## GT8. The guide's code is compiled but almost none of it runs
- **Severity**: gap. **Size**: S–M.
- **Where**: `tools/check_guide.py:27-44,64-69`.
- **How it shows**: all 70 `dart` blocks go into one app and pass `flutter analyze --fatal-infos`; only the
  four `test/` blocks run (CI log of run 40200810608: errors_test, walker_test, app_test, bullets_test).
  The other samples (`Waves`, `shed`, `town`, `railway`, `Fountain`, `Dog`, the shape list at
  `writing-a-game.md:1458-1465` ...) are never executed, so a sample that throws at runtime (an
  `ArgumentError` for a setting, a wrong default, a mesh the sample's own claim gets wrong) passes. Also,
  since all blocks share one file, a sample can lean on another's declarations (§19's `TapToScore extends
  Score`), so "compiles as it stands" (`writing-a-game.md:4`) is true only of the whole.
  WGSL is covered separately and well (`crates/fynkel_render/tests/reference.rs:79-90`); kotlin/yaml blocks
  aren't checked (see GT4).
- **First fix idea**: have check_guide.py add a generated test that, for each `GameObject`/`Behaviour`
  subclass and top-level mesh/shape function in lib/game.dart that needs no arguments or assets, adds it to a
  `World` with a `HeadlessView` for 60 frames and expects no `world.errors`.

## GT9. Systems are tested one by one; sound is in no game
- **Severity**: gap. **Size**: M (phase 3 covers it).
- **Where**: example pubspecs: `fynkel_audio` only in `examples/lab/pubspec.yaml:18`; `fynkel_physics` in
  coins, bowling, lab, pocket_hole; Sheepdog (the reference game, particles + animation + shaders) has
  neither physics nor sound.
- **How it shows**: grepping every `*_test.dart` for physics/audio/particles/animation/splines/runtime
  meshes/shaders, only `examples/lab/test/lab_test.dart` names three or more, and it drives each lab scene
  alone. Never tested together: sound with physics (contact sounds, many 3D sources moving with bodies),
  sound + `time.scale`/pause next to particles and animation, skinned characters + physics, swept spline
  meshes + `MeshCollider` (`packages/fynkel_physics/test` has no `Mesh.sweep`/`setSweep`), shaders on
  particles + soft particles + shadows at each quality. Stage 11's in-game bars moved to shipping, so no
  game measures sound.
- **First fix idea**: phase 3's everything-scene as a test as well as a lab page (headless, a few hundred
  frames, no errors, no garbage after warm-up, memory flat), and give Sheepdog sound.

## GT10. fynkel_measure, which every number comes from, is almost untested
- **Severity**: gap. **Size**: S.
- **Where**: `packages/fynkel_measure` (lib 990 lines + `bin/measure.dart` 858) vs
  `test/on_screen_test.dart` (59 lines, `OnScreenClock` only).
- **How it shows**: the stats-line format (`lib/src/measurement.dart`) and the tool that parses it
  (`bin/measure.dart`), `MeasuredApp`, `bench`, footprint and thermal reading have no tests; the examples'
  `tool/measured.dart` are analyzed but never run. A change to the stats line breaks every device run, found
  only when a measurement is next taken by hand.
- **First fix idea**: a round-trip test (Measurement prints → measure.dart's parser reads the same numbers),
  and a widget test that pumps `MeasuredApp(Hello.new)` for a second.

## GT11. Widget-test and golden helpers are copied instead of provided
- **Severity**: awkward. **Size**: S.
- **Where**: the same `Future<void> wait(WidgetTester tester, [int frames = 10])` pumping 16,667 µs at a time
  in 11 lab test files (`examples/lab/test/shadows_test.dart:15`, lamps, see_through, look, several_views,
  movement, behind, sky, shaders, effects, toon) and `examples/arena/test/menus_test.dart:11`; the golden
  comparator duplicated (`examples/lab/test/golden.dart` copies `packages/fynkel/test/golden.dart`, its
  header says so).
- **How it shows**: hardening G8 gave `package:fynkel/testing.dart` `run`/`tap`/`viewPoint` for a world, but
  nothing for widget tests, where the guide (§19, `writing-a-game.md:3876-3880`) tells each game to pump
  durations by hand because `pumpAndSettle` never settles; and nothing for comparing a `HeadlessView` frame
  with a reference image, which every game with a look wants.
- **First fix idea**: `package:fynkel_flutter/testing.dart` with `tester.pumpFrames(n)` and
  `FynkelGame.of` sugar; an `expectFrame(view, 'name.png')` golden helper in `package:fynkel/testing.dart`
  (same 2/255, 0.1 % rule, `FYNKEL_BLESS`).

## GT12. CI: one serial job on the developer's Mac, and main runs can be cancelled
- **Severity**: awkward. **Size**: S.
- **Where**: `.github/workflows/ci.yml:14-16` (`concurrency: group: ci-${{ github.ref }}`,
  `cancel-in-progress: true`, applies to `main` too), one job `rust` with every step.
- **How it shows**: two merges to main in quick succession cancel the first's run, so that commit is never
  fully tested on main. All steps are serial in one job (recent runs 16–19 min; the stage-13-splines PR took
  37 min, run 40197512421); the first failing step (fmt, say) hides everything after it, including the long
  runs and mobile GPU tests. Per-step times from run 40200810608: cargo test 1.2, package:fynkel 1.9,
  physics 1.7, audio 0.8, flutter+examples 5.0, guide 0.2, long runs 5.6, mobile 1.7 min.
- **First fix idea**: `cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}`; `continue-on-error`
  is not the answer, but splitting into a few jobs (Rust, Dart packages, examples, long runs, mobile)
  on the one runner with `needs:` only where a build is shared would report every failure in one run.

## Minor, for the record
- `docs/stage-14.md:33` (and `docs/hardening.md:41`) say "§21 What changed"; in the guide §21 is "What
  isn't there yet" and "What changed" is §23 (`writing-a-game.md:3937,4109`).
- The guide's list of examples (`writing-a-game.md:8-13`) leaves out `scale` and `pocket_hole`.
- `examples/hello` has no test; CI only builds it.

---

## Looked at and fine
- **Guide code is gated**: all 70 `dart` blocks analyzed with `--fatal-infos` and its 4 test blocks run
  in CI (`tools/check_guide.py`, "the guide" step); every `wgsl` block compiled as a custom shader
  (`crates/fynkel_render/tests/reference.rs:79-90`). Every opening fence has a language (84 open, 84 close).
- **Guide references resolve**: every `examples/.../*.dart` path the guide names exists; every lab scene
  title it quotes exists (one is "Minimap: the basics" vs "Minimap", fine); identifiers in prose not in
  packages/ are all Unity names, WGSL helpers (present in `crates/fynkel_render/src/library.wgsl`), or §23's
  old names.
- **Spot-checked claims hold**: sound defaults (`source.dart:43-52`: playOnAwake, priority 128, minDistance 1,
  maxDistance 500, logarithmic; `audio.dart:180` 32 voices); spline `segmentsPerMetre` 2 / `maxSegments`
  (`resources.dart:244-245`); extrude's 40° (`fynkel_mesh/src/outline.rs:182,208`); `closeTo` on Vec3/Quat;
  `worldToScreenPoint(width:, height:)`; `fynkel_measure` is a dev_dependency in every example; the measure
  tool's `rate`, `--seconds`, `--define` and `FYNKEL_FRAME_RATE`; ABI numbers in §23 match the bindings (44,
  14, 5).
- **Packages plug in the same way**: `Physics` and `Audio` are both `WorldSystem`s made on first use via
  `world.system(...)`, each with `GameObject`/`Behaviour`/`World` extensions
  (`fynkel_physics/lib/src/physics.dart:1281-1298`, `fynkel_audio/lib/src/audio.dart:785-800`); each checks its
  own ABI at load with the same message shape (`fynkel/lib/src/native.dart:26`, `fynkel_physics/lib/src/raw.dart:89`,
  `fynkel_audio/lib/src/audio.dart:59`), and header-drift tests exist per C library. `PhysicsStatus` living only
  in `raw.dart` is deliberate and documented (§23).
- **Goldens**: strict (2/255, 0.1 % of pixels, `crates/fynkel_golden/src/lib.rs:58-104`, mirrored in Dart);
  all 80 files in `tests/golden` are referenced by a test (many through `format!`); missing goldens fail
  rather than auto-bless.
- **Garbage is gated**: `alloc`-tagged tests in all three Dart packages run serially in CI, plus Rust
  `alloc.rs` in render and ffi, also on the simulator/emulator.
- **Wall-clock tests** compare ratios with slack (`component_test.dart:221-235`, `routine_test.dart:84-96`,
  `texture_test.dart:253-266`); `signal_test.dart:153`'s absolute 250 ms for 50k cancels is generous. None
  has failed in the CI history I read.
- **Hot reload** is tested at widget level (`fynkel_view_test.dart:208-211,1284`).
- The arena/sheepdog pause menus and settings pages are alike on purpose (hardening G7: menus stay the
  game's); not raised again.

## Coverage

| System | Tested alone | Tested with others | Measured in CI |
|---|---|---|---|
| Objects, world loop, components, signals, routines, tweens | yes (packages/fynkel/test, ~60 files) | yes, through every example's tests | no (complexity ratios only) |
| Input | yes (`input_test`, flutter `input_test`) | yes (examples' tap/key tests) | no |
| Rendering, lights, shadows, sky, bloom/AO, styles | yes (fynkel_render tests + goldens, on Mac, iOS sim, Android emu) | lab pages' goldens; never all at once | no (alloc only) |
| Custom shaders | yes (`shaders.rs`, `shader_test`, guide WGSL) | with particles in lab only | no |
| Animation / skinning / crowd | yes (`animator*_test`, `skin.rs`, `knight_*` goldens) | Sheepdog (with particles) | no |
| Particles (+ trails, sub-emitters, planes) | yes (`particles_test`, `particles.rs`, lab goldens) | Sheepdog; lab | no (`particleStepTime` read by hand) |
| Runtime meshes, builder, combine | yes (`mesh_test`, `mesh_building_test`, `fynkel_mesh`) | with MeshCollider (`mesh_collider_test`) | no (benches `#[ignore]`) |
| Splines, sweep, SplineInstantiate/Animate | yes (`splines_test`, `spline_objects_test`, `sweep.rs`) | not with physics | no (`#[ignore]`) |
| Physics | yes (5 Rust + 9 Dart test files) | bowling, coins, pocket_hole (not in CI), lab | no |
| Sound | yes (Rust incl. goldens; Dart audio/mix/spatial, no device) | lab only; no game | no; device paths never compiled in CI (GT4) |
| View / presenting / plugin | Rust present tests, flutter `fynkel_view_test` with a stand-in plugin | examples' widget tests | no; Kotlin and iOS Swift never compiled (GT4) |
| Loading (glTF, images, HDR) | yes (+ damaged-file fuzz, 1000 iterations) | garden, lab | no |
| Memory over time | lab soak (no view, flaky: GT2/GT3) | all lab scenes in turn, one world each | yes, but noisy |
| fynkel_measure | OnScreenClock only (GT10) | never run in CI | n/a |
