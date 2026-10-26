# Marble Run — newcomer log

Read only docs/writing-a-game.md, packages/*/lib, their pubspecs, and
packages/fynkel/wgsl/fynkel.wgsl (the guide points there). Nothing from
examples/, crates/ or tests.

## Timeline (wall clock, 2026-10-26)
- 22:49 Started. Read the guide's TOC, then §1–§19 (skimmed §13
  animation). The guide is 4,389 lines, so reading took most of the time.
- 22:50 `flutter create --empty`, a pubspec with fynkel, fynkel_flutter, fynkel_physics,
  fynkel_audio (by path) + path_provider, fynkel_measure as a dev dependency. `pub get` fine.
- 22:50 A warm-up `flutter test` (World + BoxCollider + AudioSource) passed in seconds. The engine
  was already built in the repo's target/.
- 22:51–22:53 Read the public API: Spline, Mesh.sweep/setSweep, MeshCollider, Rigidbody,
  TrailRenderer, ParticleSystem, AudioClip.fromSamples, playAt, Pool, FynkelApp, testing.dart.
- 22:53 Wrote the whole game in one go (track, marble, cup, game, hud, main, stripes.wgsl).
  First `flutter analyze`: 2 errors, both my own JSON-cast bug, plus 1 lint. Clean after one fix.
- 22:54 Wrote 6 headless tests. **All passed on the first run.**
- 22:54 Traced a marble frame by frame and rendered PNGs with HeadlessView. The marble rolls the
  whole track in 3.5 s, hits the board and lands in the middle cup. Raised the camera (marbles were
  hidden in the channel). Found and fixed the trail bloom halo (below).
- 22:55 Widget test (FynkelApp + HUD, tap Go, tap Reset) passed first run.
- 22:56 Probed degenerate tracks; added a guard and a test. `flutter build macos --debug` ✓ (21 s).
- 22:57 Final: analyze clean, 8/8 tests pass. lib 612 lines (+12 WGSL), tests 194 lines.

## Findings (worst first)

### Confusing
1. **There's no way to check "makes no garbage" in a test.** The guide says 'no garbage' for
   `setPositions`, `setSweep`, MeshBuilder, pools, particles and queries. It never says how a game
   can check its own frame. fynkel_measure measures Dart allocation per frame only inside a running
   app (the stats line), and has no test helper. Also, the public types make a garbage-free drag
   frame impossible: `Vec3`, `Ray`, `Plane` and `ViewPoint` are `final class`es, and
   `finger.position`, `camera.screenPointToRay` and `screenPointToPlane` each return new ones. My
   drag path uses `Spline.setPositions` with one `Float32List` and `setSweep`, and moveKnotXyz
   takes doubles. Getting the finger's point still allocates. **I wrote no garbage test.** The
   brief made it conditional on the guide saying how, and it doesn't.
2. **Headless tests of a game that loads assets need `World(source: ...)`, which §19 never
   shows.** Every test in the guide uses `World()`. My root loads its shader in `onLoad`, so a test
   needs `World(source: (name) => File('assets/$name').readAsBytes())`. I found this in
   world.dart/assets.dart before hitting it. The error is good: "this World has no asset source to
   load X from: make it with World(source: ...)". But §19 should have one line on it.
3. **An additive trail plus `Camera(bloom:)` gives a halo about 2 m wide.** I copied §13c's
   habit (`blending: Blending.additive` for glows and trails) with a near-white colour. Five
   marbles' trails overlap, go over 1 and bloom into a big blue glow over the top of the track.
   I only found it by rendering a PNG. Fix: a transparent trail (`opacity: 0.55`). Worth a line
   in §13c where trails and bloom meet: "additive near-white overlaps go over 1 and bloom".
4. **`setSweep` throws after `Spline.setPositions` has already changed the spline.** Drag every
   knot onto one spot and setSweep throws `ArgumentError`, but the spline has already changed, so
   spline and mesh disagree. The caller has to put the knots back (my moveKnotXyz does, with a
   test). The message is slightly odd: "a spline needs at least two knots apart to sweep along;
   this has 4". It has 4 knots, none of them apart.
5. **Concurrent builds share the repo's target/ dir.** I saw this in a log file another session
   was also writing to (/tmp/s14/warm.log): a second game (`courtyard`) building at the same moment
   failed with `PathNotFoundException: Cannot copy file to …/libfynkel_physics_ffi.dylib`, the
   dylib missing from target/aarch64-apple-darwin/release mid-build. Not my build, but two apps
   building at once can break each other. The error doesn't say "another build is running".

### Minor
6. **Both sides hear a marble-on-marble collision**, so the click played twice. The guide implies
   this ("they come to a collider's object and its rigidbody's object") but never says "two dynamic
   bodies both get onCollisionEnter". I de-duplicated with `other.id < id`. I first used
   `identityHashCode`, then found `GameObject.id` in the source. The guide never mentions `id`.
7. **`world.run` is an extension in `package:fynkel/testing.dart`.** Without that import the error
   is "The method 'run' isn't defined for the type 'World'", with no hint. §19 does show the import.
8. **Waiting for an async root in a test.** The guide's `await add(level).loaded` is about levels.
   I assumed a test would have to pump `world.update` for it to finish and wrote a loop. Then I
   checked: `await game.loaded` alone works headless. One line in §19 would help.
9. **A pooled object with a TrailRenderer: I `clear()` it in `get`'s function as a precaution**
   (the guide's "a ball put back on the tee"). I didn't check whether a reused marble would
   otherwise streak from where it was released. §3's pooling section could say whether a release
   clears trails and particles.
10. **Making a root with arguments** (the save file): `FynkelApp(Hello.new)` is all the guide shows.
    `make` is a `T Function()`, so `FynkelApp(() => MarbleRun(saveFile: f))` works. Easy, but an
    example would save a look at the source.
11. Lint `prefer_initializing_formals` pushed me to a private field via an initializing formal. I
    renamed it public instead. Not Fynkel's fault.

### Guide wrong?
- Nothing I used was wrong. Every snippet I leaned on compiled as shown: the lava shader shape,
  `ParticleSystem` + `Burst`, `TrailRenderer`, `Mesh.sweep`/`setSweep`, `Mesh.lathe`, `Pool`,
  `with Triggers`/`with Collisions`, `onDragStart`/`onDrag`, `screenPointToPlane`, `FynkelApp`
  overlay, the widget test pattern, the JSON save.

## Pleasant
- **The API guesses were right the first time.** The whole game compiled with only my own JSON bug,
  and the tests passed on their first run. Unity names (`TrailRenderer`, `ParticleSystem`,
  `onCollisionEnter`, `playAt`) did what Unity users expect.
- **The MeshCollider follows a re-swept mesh with no code.** `collider.raycast(ray)` made "the
  collider changed" a 5-line test, and `physics.raycast` agreed.
- **The custom shader (scrolling stripes) worked on the first try.** It's the lava example's shape,
  with `uv.y` in metres along the sweep (Mesh.sweep's doc says so).
- **`AudioClip.fromSamples` let me synthesize the click** with no asset file. Audio runs headless,
  so `audio.stats.started` proves clicks reached the mixer.
- **`HeadlessView.png()` showed real problems** (hidden marbles, the bloom halo) that tests can't.
- **Saving per §18 was copy-paste.** Writing beside the file and renaming is a good habit to teach.
- **Error messages I hit were specific** (no asset source, the spline with no length).
