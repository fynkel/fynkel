# Stage 14 findings: what the audit found

Phase 1 of [stage 14](stage-14.md), done 2026-10-26 at `44ea4697`.
**Accepted** 2026-10-26 with the recommended answer to each of §6's
four decisions (the user: "let's go with them").

## 1. How it was done

- **Eight readers**, one per area, each reading its code whole: the Dart
  core, the Dart features, the bridge and the view, the renderer, the
  world/mesh/load crates, physics, sound, and the guide, examples, tests
  and CI.
- **Two fresh developers**, given only the guide and the packages'
  public code, each wrote a game that uses nearly everything:
  *Courtyard* (waves from a pool, physics, particles, 3D sound, music
  crossfades, lamps, fog, bloom, a swept path, a waving banner, a pause
  menu, settings) and *Marble Run* (a track dragged into shape and swept
  along a spline with its collider following, marbles, trails, a custom
  shader, cups that score, save and load).
- **Five checkers**, each in its own copy of the repository, went back
  over every claim: they read the code again and wrote a throwaway test
  for each one they could. What's below survived that.

Everything is kept: the readers' and checkers' notes and the two
developers' logs in [stage-14-audit/](stage-14-audit/), with each
finding's file and line; the throwaway tests (30 files, nearly all
failing today) in `~/fynkel-runs/s14/tests/`, to become each row's
failing-first test.

**The verdict.** About 95 claims; about 80 hold, most with a failing
test. **Both games were written from the guide alone**, built clean
and passed every test on their first run, and every guide sample they
used compiled as shown. The hardening pass's fixes held, apart from one
that holds only in part (BR2). What's wrong is in three places:

1. **A few things can take the whole app down**: a panic on the audio
   thread, a malformed model file, a particle shader, an oversized view.
2. **Lifecycle edges in code added since hardening**: physics colliders
   after a change, objects reparented while loading, the live camera
   destroyed, sound after dispose.
3. **Costs that grow with the scene**, mostly from stages 12 and 13
   (sky, sweeps, raycasts, animators), and **nothing in CI that would
   catch them**: no number is checked automatically.

Ids are the readers' (DC Dart core, DF Dart features, BR bridge, RN
renderer, WO world/mesh/load, PH physics, AU sound, GT guide/tests/CI,
GD the developers' logs). ✅ = a test fails today; 📖 = confirmed by
reading; ½ = partly true, as stated.

## 2. Phase 2: bugs, worst first

### 2a. The app goes down

| Row | What | How it shows | Size |
|---|---|---|---|
| AU1 ✅ | Starting a clip at its end panics the mixer | `playTime = clip.length` on a playing source; clips not a whole number of seconds too. On a device the app aborts (cpal catches nothing) | S |
| WO1 ✅ | A malformed .glb aborts the process | a 436-byte file with a sparse animation count of 2^44 asks for 70 TB; skin and animation data skip the size check meshes have | S |
| RN6 ✅ | A particle shader that passes the check fails at draw time | a helper named like the engine's (`fy_soft`) → "redefinition" when drawn, which stops every view on the device | S |
| BR1 ✅ | A view bigger than the GPU's 8192 isn't refused | made: every view loses the device; resized: that view fails for good. Dart allows 16384 | S |
| AU4 ✅ | Sound writes into freed memory after dispose | `playOneShot` after `world.dispose()` writes volume and pitch into freed scratch memory, then throws | S |
| WO6 ✅ | No cap on emitting or simulating | `Emit(u32::MAX)` ~14 s, `simulate(1e7)` ~26 s, holding the world lock; a 20,000 s prewarm accepted | S |
| BR3 ✅ | Long native calls not marked blocking | sky decode 8 ms, mesh shapes at 1024 segments 35–49 ms, `set_positions` with a new count 31 ms, simulate, shader_new, sweeps: run as leaf calls, which hold up garbage collection in every isolate | S |
| BR2 📖 | The shader thread has no error scope | a pipeline that passes naga but fails to build loses the device for every view; latent | S |

### 2b. Wrong behaviour

**Physics**

| Row | What | How it shows | Size |
|---|---|---|---|
| PH1 ✅ | A collider changed after the body moved goes back to where it was added | a resting ball given `radius = 0.6`: its collider stays at spawn height and the ball falls through the floor (y −3.97). Also adding Triggers/Collisions, toggling kinematic, a mesh changing | M |
| PH2 ✅ | With interpolation, a collider added mid-frame gets the drawn/simulated gap as offset | 0.18 m behind a body at 10 m/s, for good. One fix with PH1: offsets from the hierarchy, not the native pose | (with PH1) |
| PH4 ✅ | `movePosition`/`moveRotation` use a stale target | on a moving body: `moveRotation` sends it to x = 0; `movePosition` resets its rotation | S |
| PH3 ✅ | Off and on before a step gives enter then exit | the set pattern the guide recommends ends up empty | S |
| PH10 ✅ | A joint whose other body is destroyed waits forever | no message, keeps the dead `Rigidbody` | S |

**Objects, cameras, motion (Dart)**

| Row | What | How it shows | Size |
|---|---|---|---|
| DF1 ✅ | The live camera's object destroyed → the next update throws | then cuts with no blend and no `onLiveChanged` | S |
| DC2 ✅ | Reparented while loading → drawn, but never started or updated | `_setParent` doesn't move the wait | S/M |
| DC1 ✅ | `onLoad` runs twice | a behaviour added by an earlier `onLoad` to a constructor child | S |
| DC6 ✅ | One child failing in `world.add` leaves the rest stuck | later siblings "already added" for good, lights stay counted, and the root stays in the world half built | S |
| DF5 ✅ | `Motion.complete()` on a finite loop stops at the wrong place | a yoyo ends at 1 not 0; a 3-loop sequence moves 1 not 3 | S |
| DF6 ✅ | Splines on a scaled container aren't in metres | speed 1 travels 2 m/s at scale 2; `rotationAt` and `directionAt` disagree | S/M |
| DF8 ✅ | `Follow` with look-ahead jumps 1 m on pause | and back on resume | S |
| DF9 ✅ | `Animator.at` marks can't be removed | a pooled object adds one more each reuse | S |

**Meshes, particles, animation (Rust and Dart)**

| Row | What | How it shows | Size |
|---|---|---|---|
| WO9 ✅ | Filling ends of extrusions and sweeps is wrong with holes | 2+ holes (a B, an 8) often overlap and cover the holes; a hole to the left puts triangles outside the cap; a self-touching outline fills outside; quadratic (10k points 120 ms). **Rewrite the fill** | M |
| WO7 ✅ | Prewarm runs before collision planes are found | prewarmed particles thrown up at 19 m/s; a sub-emitter fires 30 on frame 1 | S |
| WO5 ✅ | Collision planes on a non-uniformly scaled emitter | normal by the inverse, not the inverse transpose: wrong bounce, wrong side | S |
| DF4 ✅ | A destroyed collision plane breaks the particle system | every setter throws; rewritten in full on every spawn | S |
| WO2 ✅ | A destroyed leaf keeps its cached matrix | a destroyed skin joint drawn at its parent | S |
| WO3 ✅ | Raycasts hit skinned meshes in their bind pose | and undrawn submeshes; sidedness from the first material only | M |
| WO13 ✅ | Crossfades ignore speed; backwards loops aren't counted | a paused animator finishes its fade; events give the wrong range | S |
| WO14 ✅ | A sweep flips 180° where the path is vertical | | S |
| DF10 ✅ | `MeshBuilder` drops given normals | once any vertex is added without one | S |
| DF12 ✅ | `Mesh.inBackground` loses dynamic and UV flags | and `Mesh.custom` without UVs is marked as having them | S |
| DF11 ✅ | `SplineInstantiate(count:)` on an empty spline throws every frame | | S |
| DF7b ✅ | A shader reload turning a value into a texture throws midway | the old native shader is never freed | S |
| WO11 ✅ | The trail clock is f32 | runs 1.8× fast as it grows, stops after 3–6 days | S |
| WO12, DF14 ✅ | NaN let through | a NaN particle colour accepted; `Quat.normalized` of zero gives NaN where `Vec3` throws | S |

**Sound**

| Row | What | How it shows | Size |
|---|---|---|---|
| AU2 ✅ | A stolen voice clicks; fades are stolen first at any priority | a 3 s music fade cut to 0 by a priority-256 sound | M |
| AU3 ✅ | Stolen-voice notices never reach Dart | after 32 one-shots, a source's volume stops reaching new ones; `music.onEnd` would fire for a stolen track | S |
| AU-new 📖 | Music can be stolen | it plays at priority 0, though "never given up" | S |
| AU5 ½ | An overflowing position turns sound off for good | NaN is refused when set; a world position of 6e38 gets through | S |
| AU6 📖 | Android: voice chat counts as a phone call | a WebRTC library's `MODE_IN_COMMUNICATION` pauses the game's sound. **Checked on the Note10+ 2026-10-27**: voice chat plays through the mode, `ambient` waits, both wait while ringing | S |
| AU8 📖 | iOS: Siri, an alarm or a call is treated as a lost device | drops cpal's resume, retries each second; it does recover. **Checked on the iPhone 2026-10-27**: Siri, a timer and calls; the output never reopened | M |

**The rest**

| Row | What | Size |
|---|---|---|
| RN1 ½ | White and pure-colour outlines glow under bloom (alpha 1 read as bright) | S |
| BR4 ✅ | iOS layer handoff lets the waiter go before the render thread dropped its copy (107 in 100,000) | S |
| GD1 ✅ | **Two apps building at once break each other**: the build hook copies the dylib outside cargo's lock (both developers hit it) | S |

## 3. Phase 3: all of it together

As planned (the everything-on scene, its soak with drawing, Sheepdog's
spline fences and path), plus what the audit found at the seams:

- **DC5 + RN7: one way in for every system.** Particles, trails,
  animators, cameras and lights are wired into `GameObject` and `World`
  by hand in five or more places each, while physics and sound plug in
  through `WorldSystem`; per-object data reaches the GPU four ways.
  Moving the built-in ones onto one path is what makes the next system
  cheap to add. (L)
- **GT9**: no test runs three systems at once outside the lab; sound is
  in no game. The everything-on scene and its tests close this.
  **Done**: the lab's 'All of it together', tested at each level.
- **GT3**: the soak never draws, so GPU-side leaks can't show.
  **Done**: it draws every 30th frame, 64×64 (`FYNKEL_SOAK_DRAW=0` not).
- **AU9 ½**: a way to stop all world-level one-shots (no scene loads in
  Fynkel, so it's narrower than raised). **Done**: `audio.stopOneShots()`.

## 4. Phase 4: fast, and staying fast

**Measuring first** (nothing below is safe without these):

| Row | What | Size |
|---|---|---|
| GT1 | No number is checked in CI: Rust benches are `#[ignore]`, scale's test only prints, `fynkel_measure` never runs. Bench gates with a margin, on the runner | M |
| GT7 | Per-system frame timing (only particles and meshes have one; the lab rebuilds a median timer in six scenes; the guide points at a Profiler that doesn't exist) | M |
| — | A budget per device and level, written down; curves against object, lamp, particle, source and body counts | M |

**The costs found** (Mac numbers; a phone is slower):

| Row | What | Measured | Size |
|---|---|---|---|
| RN2 | A turning or dimming sky rebuilds all its light | 9.5 ms a frame turning (2K image), 0.79 still | M |
| WO8 | Sweeping and changing meshes allocate each frame | 87 allocations, 800 KB per road sweep | M |
| WO4 | Raycasts recompute every matrix and test every triangle | 50 casts into 3000 objects: 14.2 ms | M |
| DC3 + DF3 | Animators mark every part moved every frame, hidden or paused too | 50 hidden knights ~42 µs; physics then rescans | S |
| DC4 | Plain objects visited every update; one destroy or reparent is a pass over all | 0.1 ms at 5000, 0.5 ms at 20000 | S/M |
| DF2 | A particle or trail setter resends everything and relinks every system | 9 objects a set; 4.3 µs with 1000 systems | M |
| DF7a | `material.set` packs every value again | ~70 objects a set (the guide's way to tween) | S |
| WO10 | Each view recomputes every world matrix each frame | 75 µs for 3000 objects | M |
| RN4 | Transparent particles re-sorted with depth recomputed per compare | 0.73 ms for 10k, 0.13 keyed | S |
| PH5 | Any mesh change, colours too, rebuilds its collider; each rebuild scans all colliders | 10 colour sets = 10 rebuilds | S |
| RN5 ½ | Soft particles turn on the whole what's-behind path | bandwidth on the Note10+; the depth-only idea doesn't avoid MSAA's round trip | M |
| RN3 📖 | Vulkan pipeline cache rewritten after every pipeline | Android only | S |
| AU7 | A streamed voice copies its whole file each start and loop | 2.1 MB for 3 loops of 256 KB | S |
| BR5 📖 | iOS layer holds its lease across the drawable wait | main thread can wait a frame, up to 1 s | S/M |
| BR6 ½, BR7, PH6, PH7, RN9 ½, DF13 | smaller: extract re-walks resources, world widgets allocate, rig follow, per-step slot loops, the frame ring never shrinks, bloom kept when off | | S |

**The owed bars**: stage 13's mesh change on the Note10+ (1.3 ms vs 1),
Sheepdog on the iPhone at 120/high (80), the iPhone runs owed by stages
12 and 13.

## 5. Phase 5: easy to write a game with

**What the developers hit** (both games):

| Row | What | Size |
|---|---|---|
| GD2, GD14 | §19 never shows a test loading assets (`World(source: …)`) or `await game.loaded` without frames | S |
| GD9 | No way to check "no garbage" in a game's own tests, though the guide promises it; `finger.position`, `screenPointToRay`, `screenPointToPlane` make new objects each call | M |
| GD4 | The guide names `Stick(…)` as if it shipped; it's only in the lab. Ship it in `fynkel_flutter` | S |
| GD3 | `pausedListenable` needs `fynkel_flutter`'s import; §11 doesn't say | S |
| GD6 | `audio.music.ignorePause` isn't named where pausing is | S |
| GD7 | The "quality doesn't draw bloom" warning prints in every low test, with no way to quiet it | S |
| GD10–12, GD15, GD16 ½ | hints: additive trails + bloom halo, `setSweep`'s message, both bodies hear a collision (and `id`), pooled trails cleared, a root game with arguments | S |
| DC7 | `getInChildren` null before add, where `getInParent` throws | S |
| BR9 ½ | A failed view is a grey box for good: no retry, no hook beyond `FlutterError.onError` | S |
| BR8 ½ | ABI conventions drifted (`fy_world_time_set` breaks the ABI's own rule); a written rule and a check | S |

**CI and tests**:

| Row | What | Size |
|---|---|---|
| GT4 | What a phone runs is never compiled in CI: Kotlin, iOS Swift branches, sound's session code, mobile Rust targets | S |
| GT2 | The soak's memory check fails on noise (3 in 3 days, 1 on main) | S |
| GT12 | One serial job, 16–19 min; `cancel-in-progress` cancels runs on main | S |
| GT8 | 70 guide blocks analyzed, 4 run | S/M |
| GT5 | Pocket Hole outside CI; `lab/tool` not format-checked | S |
| GT10, GT11 | `fynkel_measure` barely tested; `wait(tester)` copied 15 times, the golden comparator twice: a `fynkel_flutter` testing library | S |
| — | hello has no test; the guide's example list misses scale and pocket_hole | S |

**Rewrites** (§6, decision 2):

| Row | What | Size |
|---|---|---|
| RN8 | `renderer.rs` 3516 lines, `encode` one 630-line function, pipeline making repeated ~10 times: passes as their own units | L |
| BR11 | `FynkelViewState`'s four platform hosts share one set of flags, where V1, V2, V4 and V12 lived: one class each | M |
| BR10 | `abi.rs` 5730 lines into modules; the header stays byte-identical | M |
| DC5 | (phase 3, above) | L |

## 6. Decisions

1. **The order: phase 2's 2a first** (the app going down), then 2b with
   GD1 and GT4 alongside (*recommended*: crashes before anything, and CI
   compiling phone code catches the next ones), rather than going area
   by area.
2. **The four rewrites: all four** (*recommended*: each is where bugs
   have been or where the next change hurts; BR10 is mechanical and its
   check is the header unchanged; RN8 and DC5 are the large ones and go
   after their area's bugs are fixed, so the failing tests guard them),
   rather than only DC5 and BR11, or none.
3. **New things the audit asked for**: ship `Stick` and per-system
   timing in this stage (*recommended*: the guide already names one and
   phase 4 can't work without the other); root motion and "animate
   physics" (PH9) to stage 15, animation 2; particles colliding with
   colliders (PH8), point-light shadows (GD5) and a stop-all for sound
   (AU9) to §21, until a game asks.
4. **On the phones**: AU6, AU8 and BR5 need a device. I'll write the
   test and say what to look for; the user runs it.

## 7. What didn't survive

So it isn't raised again:

- **GT6**: moving a static collider *does* wake what rests on it
  (Rapier 0.36.1; two tests pass). Pocket Hole's wake loop isn't needed.
- **AU9 as stated**: there are no scene loads; dispose frees every voice.
- **AU10**: retries are every second, not faster; a nit (on Android each
  asks for focus again).
- **AU5's NaN path**: NaN is refused when set.
- **PH8, PH11 as gaps in the guide**: both are already documented; PH8's
  particle half is a feature (§6.3).
- **RN9**: the "stale behind flags" (reset before the early return) and
  bind groups never evicted (freeing a texture evicts them).
- **BR8**: "optional float pointer" (they refuse null) and "a Vec per
  post" (empty Vecs don't allocate).
- **GD8, GD13, GD17**: the developer's own bug, a missing import with
  the right error, a lint not Fynkel's.
- **DF7's example name** `noise` (the prelude defines it; any other name
  shows the bug).
