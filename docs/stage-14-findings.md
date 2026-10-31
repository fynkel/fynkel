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
  **Done**: it draws every 30th frame, 64×64, in CI too, and the GPU's
  memory is held to a limit with the rest of what's outside the Dart
  heap and malloc (GT2). A 512×512 view kept each round on purpose adds
  9 MB a round there, against 0.1 allowed.
- **AU9 ½**: a way to stop all world-level one-shots (no scene loads in
  Fynkel, so it's narrower than raised). **Done**: `audio.stopOneShots()`.

## 4. Phase 4: fast, and staying fast

**Measuring first** (nothing below is safe without these):

| Row | What | Size |
|---|---|---|
| GT1 | No number is checked in CI: Rust benches are `#[ignore]`, scale's test only prints, `fynkel_measure` never runs. Bench gates with a margin, on the runner. **Done** for the crates and scale: `crates/fynkel_bench` (a median over 200 ms at least, held to a bar of the Mac's time doubled; over it, tried again up to three times, the best held, as other work on the runner once slowed one 2.4×); extracting 10,000 objects, making, changing and sweeping meshes, and setting a ground's collider again run in release in CI, and scale's `world.update` for 10,000 objects is held to 1 ms. Since 2026-10-29 a bar missed twice warns on the run's page and doesn't fail it (a busy runner put scale's 0.5 ms at 5.3 ms and held PRs for hours), as do Android emulator tests that hang or crash the emulator twice. `fynkel_measure` runs on devices; its tests are GT10 | M |
| GT7 | Per-system frame timing (only particles and meshes have one; the lab rebuilds a median timer in six scenes; the guide points at a Profiler that doesn't exist). **Done**: `world.profile`, the frame's parts and each system's own time (`WorldSystem.name`, `frameTime`), always on (scale's frame 0.49 → 0.51 ms, within its noise), no garbage; the lab shows its medians for every scene, and the guide (§6) points there. The six scenes' own timers stay: they time one thing each (a sweep, a step) for their readouts | M |
| — | A budget per device and level, written down; curves against object, lamp, particle, source and body counts. **Curves done on the Mac** (`examples/lab/test/curves_test.dart`, `FYNKEL_CURVES=1`): each count a fresh lab scene, the median of 120 frames, the best of three tries, on the game's thread (`world.profile.total`) and drawn (a 256×256 headless view, so CPU, not fill). Every curve grows in step with its count or slower; still objects on the game's thread didn't until DC4, below, was fixed. **Budgets written** (below) for the Mac, the Note10+ and an iPad (6th generation, 2018), from the lab's tour on the device (`examples/lab/tool/measured.dart`, which now breaks each level's frame down); the iPhone's are owed, as it was only reachable wirelessly | M |

**The curves** (Mac, 2026-10-27; ms over an empty world's, at the
second count and the last; growth 1 = in step):

| What | Counts | Game's thread | Drawn | Each, game |
|---|---|---|---|---|
| Moving objects | 4,000 → 32,000 | 0.47 → 4.8 (1.03–1.12) | 0.6 → 4.7 (0.91–0.98) | 0.15 µs |
| Still objects | 4,000 → 32,000 | 0.09 → 1.3 (1.23–1.35, DC4) | 0.5 → 3.4 (0.88–0.98) | 0.04 µs at 32,000 |
| Lamps over 1,000 objects | 16 → 64 | flat, 0.02 | +0.1 to +0.4 (0.5) | — |
| Particles, one system | 2,500 → 10,000 | 0.013 → 0.043 (0.8–0.97) | flat | 4 ns |
| Sounds looping in 3D, moved | 8 → 32 | 0.035 → 0.11 (0.8–0.9) | — | 3 µs |
| Bodies, awake, in piles | 200 → 800 | 0.19 → 0.80 (0.94–1.12) | 0.15 → 0.25 | 1 µs |

Moving objects cost more each past 4,000 or so (0.12 µs → 0.15), their
data no longer in the cache; above that they're in step. Still objects
should cost close to nothing, but the updates, late updates and fixed
updates each visited every one (DC4): 0.1 ms at 4,000, 1.1–1.4 at
32,000, three times as much for each doubling at the top, as the walks
fell out of the cache. With DC4 done they cost 0.002 ms at 32,000, and
the moving objects (moved by the scene, plain themselves) 3.1 ms.

**The budgets** (2026-10-28): what each part of a frame may take at 60
fps, 16.7 ms, on the level the device draws at, held against the lab's
'All of it together' (a skinned crowd, colliding particles, sounds in
3D, custom shaders, lamps and shadows, physics, meshes changed every
frame and swept), the busiest scene there is. Medians, in ms, over 15 s
of profile builds. The game's thread is `world.profile`; Flutter's UI
thread holds it, sending the frame on and Flutter's build; the render
thread's is its time a drawn frame, waiting for the GPU included, so a
device whose GPU can't keep up shows it there.

| Part | Budget | Note10+ (low) | iPad (high, its pick) | iPad (low) | Mac (high) |
|---|---|---|---|---|---|
| The game's code | 3 | 1.58 | 2.11 | 0.64 | 0.51 |
| Animators | 1.5 | 1.21 | 1.32 | 0.38 | 0.32 |
| Meshes sent | 1 | 0.85 | 1.81 ✗ | 0.58 | 0.28 |
| Physics | 1 | 0.14 | 0.12 | 0.06 | 0.06 |
| Particles | 0.5 | 0.06 | 0.07 | 0.02 | 0.03 |
| Sound | 0.25 | 0.05 | 0.03 | 0.02 | 0.02 |
| The world's own | 0.25 | 0.02 | 0.01 | 0.01 | 0.00 |
| `world.update` | 7 | 4.17 | 5.52 | 1.79 | 1.25 |
| Flutter's UI thread | 10 | 5.20 | 6.56 | 2.46 | 1.64 |
| The render thread | 14 | 4.67 | 37.27 ✗ | 35.75 ✗ | 1.74 |
| Frames a second | 60 | 59.7 | 26.8 ✗ | 35.1 ✗ | 60.0 |

The budgets leave the rest of the frame to what a game adds and to the
device's own slowdowns. The Note10+ is within all of them at low; at
medium and high its render thread takes 19–20 ms (50.7 and 48.1 fps),
which is why it draws at low. The Mac takes a tenth of its budgets at
every level. The iPad's GPU (an A10) can't draw this scene at 60 at any
level, and Sheepdog, a real game, shows it isn't only this scene (RN10).
Its game's thread at low is a third of what it is at medium and high,
with the same work: when its GPU is the limit, the CPU runs slower.

The bars owed from before: stage 13's mesh change on the Note10+ is
where it was, sending the flag and the water 0.82 ms a frame and setting
them about 0.45, against 1 ms together (the lab's 'Meshes that change',
59.9 fps). Sheepdog on the iPhone at 120 and high, and the iPhone runs
owed by stages 12 and 13, are still owed.

**Unity and Fynkel** (2026-10-28): is the engine slow, or the scene?
Sheepdog's field, built from the same numbers in both
(`examples/sheepdog/tool/field.dart`, `--define FIELD=true`, and
`tools/unity_field`, Unity 6.3 with URP): 60 skinned sheep and the dog,
120 posts, 8 lamps, a sun with shadows, 500 lit dust particles, fog.
Sheepdog's own shaders are left out, so each engine draws with its
standard surface. URP's levels are made to match Fynkel's: HDR, render
scale, anti-aliasing, shadow map, cascades, softness and distance,
ambient occlusion only at high, bloom and tone mapping from medium, soft
particles where Fynkel has them, Forward+ for Fynkel's clustered lamps.
Screenshots from both match. Frames a second over 15 s, profile and
release builds, 60 asked for; Unity's GPU time a frame, Fynkel's render
thread's (which waits for the GPU):

| Level | iPad, Unity | iPad, Fynkel | Note10+, Unity | Note10+, Fynkel |
|---|---|---|---|---|
| low | 59.7 (GPU 14.6 ms) | 39.0 (29.0 ms) | 59.5 | 59.6 (2.1 ms) |
| low, 2x AA | 59.5 (13.9) | 39.9 (25.0) | 59.7 | 59.7 (2.1) |
| low, no AA | 59.2 (13.3) | 55.9 (17.9) | 59.8 | 59.7 (2.2) |
| medium | 39.6 (37.0) | 15.3 (65.1) | 52.1 | 32.2 (28.8) |
| high | 28.3 (58.4) | 10.9 (91.4) | 53.3 | 23.9 (38.4) |

Unity draws the same scene faster on both. At low on the Note10+ the two
are even (Fynkel has room to spare there); everywhere else Unity's GPU
does the frame in about half Fynkel's time. Two things stand out (RN11,
RN12). The game's thread isn't where it's lost: `world.update` takes
1.5–4 ms on the iPad and 2.4–2.8 on the Note10+. (Unity's GPU times on
the Note10+ read 16.6 ms at every level, the frame's length, not the
GPU's work, and are left out.)

**Each part's GPU time** (2026-10-28): the engine now times every pass on
the GPU when asked (`WorldView.timeGpu`, `ViewStats.gpu`; the field's
`--define FIELD_GPU=true`). A tiled GPU's clocks round a pass are rough
(a small pass's start can be read after its end), so each pass counts
only the time it adds past the latest end, its frame's or the frame
before's: the parts never overlap, and on the iPad they add up to the
frame (medium 64.25 ms of GPU against 64.27 a frame, high 89.90 against
89.92). Medium, then medium with one thing taken away
(`--define FIELD_PARTS=true`), ms a frame:

| Medium, with… | iPad: scene | iPad: all | Note10+: scene | Note10+: all |
|---|---|---|---|---|
| everything | 51.2 | 64.7 | 21.0 | 29.4 |
| render scale 0.75 | 29.7 | 39.1 | 12.7 | 19.7 |
| no anti-aliasing | 25.3 | 38.8 | 22.1 | 33.5 |
| no HDR (so no bloom) | 48.8 | 52.9 | 18.7 | 21.0 |
| no lamp highlights | 48.6 | 62.1 | 20.5 | 29.7 |
| hard shadows | 49.3 | 62.8 | 20.2 | 29.4 |
| no soft particles | 36.9 | 50.4 | 17.0 | 26.2 |

The rest of the iPad's medium frame: shadows 4.8, bloom 2.9, finish 5.8;
at high ambient occlusion adds 11.2 and the scene 64.5. The scene pass
is most of the frame on both. Shadows, lamp highlights and soft shadows
are 2–3 ms each. What isn't small: anti-aliasing on the A10 (26 ms),
soft particles (14 on the iPad, 4 on the Note10+), and HDR on the Mali
(8.4: bloom 4, finish 2.2, the scene 2.3). Those are RN11 and RN12.

Soft particles now read a depth of their own (RN11, below): medium on the
iPad 64.7 → 57.3 ms of GPU a frame. What's left of anti-aliasing's cost
there, the field at medium with one kind of thing hidden
(`--define FIELD_CONTENT=true`; scene pass, ms a frame):

| Medium, with… | 4x anti-aliasing | none | Anti-aliasing costs |
|---|---|---|---|
| everything | 43.0 | 22.5 | 20.5 |
| the dust hidden | 17.5 | 16.3 | 1.2 |
| the lamps hidden | 34.3 | 17.6 | 16.7 |
| the animals hidden | 40.1 | 21.3 | 18.9 |
| the posts hidden | 42.7 | 22.5 | 20.2 |
| the dust unlit (`FIELD_UNLIT_DUST`) | 19.7 | 18.4 | 1.3 |

Without the dust anti-aliasing costs the A10 1.2 ms, as it costs Unity.
It's the lit dust: 500 particles, each pixel of each shaded with the
whole surface model (every lamp, the sun's shadow, fog) under the
particles in front of it, about 6 ms without anti-aliasing and 25 with
it. Unlit, the dust costs 2 ms either way. Solid surfaces don't pay it:
a tiled GPU shades only what's in front.

After soft particles' fix, frames a second (as the table above):

| Level | iPad, Unity | iPad, Fynkel | Note10+, Unity | Note10+, Fynkel |
|---|---|---|---|---|
| medium | 39.6 | 15.3 → 17.5 | 52.1 | 32.2 → 36.3 |
| high | 28.3 | 10.9 → 12.0 | 53.3 | 23.9 → 26.4 |

(The iPad's low, no anti-aliasing read 39.9 in this run against 55.9
before, with 16 ms of GPU work a frame: it had been drawing for an hour,
and something besides the GPU held it at 40. Its low rows are left as
they were till it's measured cool.)

Lit particles are now lit once each, at their middle (RN11, below):
the field on the iPad, cool (thermal nominal throughout), GPU ms a frame
and frames a second:

| Level | GPU before | GPU after | Scene pass | fps before | fps after | Unity |
|---|---|---|---|---|---|---|
| low, 4x AA | 20.9 | 16.4 | 19.6 → 15.0 | 39.7 | 40.0 | |
| low, no AA | 16.4 | 13.6 | 14.6 → 12.3 | 40.0 | 53.1 | |
| medium | 57.3 | 32.8 | 43.0 → 19.7 | 17.5 | 30.0 | 39.6 |
| high | 89.9 | 53.6 | 64.5 → 29.3 | 12.0 | 18.7 | 28.3 |

Medium's scene pass is now as it was with the dust hidden (17.5). Low
stays at 40 with 16.4 ms of GPU a frame. Fewer pixels
(`--define FIELD_SCALES=true`, low at render scales 0.75, 0.6, 0.5):

| Low, render scale | fps | GPU ms a frame | render thread ms a frame |
|---|---|---|---|
| 0.75 | 40.1 | 16.5 | 24.9 |
| 0.6 | 40.0 | 16.2 | 24.9 |
| 0.5 | 60.0 | 12.8 | 3.2 |

At 0.5 the render thread works 3.2 ms a frame: the rest of its 25 at 40
fps is waiting for a drawable, and the CPU isn't what holds it (the
render thread at Apple's user-interactive QoS changed nothing). It's the
GPU, by a little: a frame that misses 60 Hz's 16.7 ms (the engine's
passes and Flutter's own frame on the same GPU) is shown every other
vsync and a half, at 40; the A10, idle a third of the time, lowers its
clock, and the same work stretches to fill about 16 ms, at 0.6's pixels
as at 0.75's. It doesn't come back by itself: 'low, no AA' read 53 in one
run and 40 in the next, after 2x at 40. Unity's 14.6 ms fits and keeps
the clock up. So low on the A10 needs a little less GPU work a frame,
as RN12 is about. What's left between medium and Unity is the rest of the
frame: shadows 3.8, bloom 3.1, the finish 6.2 (RN12).

**The costs found** (Mac numbers; a phone is slower):

| Row | What | Measured | Size |
|---|---|---|---|
| RN2 | A turning or dimming sky rebuilds all its light | 9.5 ms a frame turning (2K image), 0.79 still. **Done**: an image's light is worked out unturned at intensity 1, and the forward pass turns and scales what it reads; a gradient's is worked out once for unit colours (each colour's share in a channel, as blending is linear in them) and coloured as it's read, so every gradient shares it. A 2K image turning and brightening every frame: 7.6 → 0.5 ms (Mac, a still sky's); a gradient changing colour every frame: 6.1 → 0.5. `FrameStats.sky_light_made` says when a frame works it out | M |
| WO8 | Sweeping and changing meshes allocate each frame | 87 allocations, 800 KB per road sweep. **Done**: a sweep builds into room kept on the thread and swaps it with the mesh's, the caps' fill is kept while the outline and holes stay the same, the ffi reads the path into room it keeps and lists up to 16 loops on the stack, and `recalculate_tangents` sums into room it keeps. A capped pipe swept again, and a grid changed with its tangents worked out again, allocate nothing once warm (`fynkel_mesh/tests/alloc.rs`), nor does a pipe swept, given to the world and drawn every frame (`fynkel_ffi/tests/alloc.rs`). A sweep makes the same bytes as before. The 100 m road: 0.100 → 0.058 ms (Mac), its bar 0.2 → 0.12 | M |
| WO4 | Raycasts recompute every matrix and test every triangle | 50 casts into 3000 objects: 14.2 ms. **Done**: the world matrices are kept until a transform, parent, active flag or pose changes (`World::transforms_version`); a missed box costs no material lookups; a mesh of 64 triangles or more cast at 16 times in one version gets a box tree over its triangles (split at the middle of its centres, a few a leaf), so a changed mesh cast at a few times a frame never pays for one. 50 casts into 3000 objects and a 50,000-triangle ground: 11.3 → 1.5 ms (Mac), a CI benchmark (`bench_fifty_raycasts`, bar 3 ms) | M |
| DC3 + DF3 | Animators mark every part moved every frame, hidden or paused too | 50 hidden knights ~42 µs; physics then rescans. **Done**: the engine lists the animators it posed (`World::posed`, `fy_world_posed_get`, ABI 45); only their parts are marked moved, and the version goes up only then. 200 hidden knights: the animators' 34 µs a frame → 4 (Mac), with nothing for physics to follow; a stopped or faded-out one marks nothing either. A paused one (`speed = 0`) or one holding a finished clip's last pose is still posed: Unity's animator writes its parts every frame while it plays, over the game's own changes | S |
| DC4 | Plain objects visited every update; one destroy or reparent is a pass over all | 0.1 ms at 5000, 0.5 ms at 20000; the curves: 1.3 ms at 32,000, growing faster than the count. **Done**: only objects with something to call (a subclass, a behaviour) are in the update order; a plain one joins in its place when given a behaviour. 32,000 still objects 1.3 → 0.002 ms, 32,000 moved by one object 4.8 → 3.1; destroying or moving plain ones is no pass at all, and others' a pass over those in the order only. A model's parts given behaviours now update after their parents (they went in the order the behaviours were put on) | S/M |
| DF2 ✅ | A particle or trail setter resends everything and relinks every system | 9 objects a set; 4.3 µs with 1000 systems. **Done**: the engine relinks sub emitters only for a new system or other sub emitters; the setters keep no closures, the checks make no lists, and the settings are written through views into the native settings made once, not a view for each field (hundreds for a system with its modules). Still written as each is set, not once at the frame's end: the engine checks the numbers, and a wrong one throws where it was set. A frame setting five of a system's settings and three of a trail's makes nothing but boxed numbers (`particles_set_alloc_test.dart`). A set with 1000 systems: 3.9 → 0.9 µs (Mac, JIT); the engine's part 3 → 0.06 µs, a CI benchmark (`bench_setting_one_of_a_thousand_particle_systems`, 1000 sets, bar 0.12 ms) | M |
| DF7a ✅ | `material.set` packs every value again | ~70 objects a set (the guide's way to tween). **Done**: a number, vector, colour or list for a value of the shader as it is now is checked and written in place, and only its bytes go to each world (`fy_material_values_set` with its offset), not the whole material; an array, a texture or a reloaded shader still packs them all. A frame setting four values, in two worlds, makes nothing but boxed numbers (`material_alloc_test.dart`); a set 0.62 → 0.12 µs (Mac, JIT) | S |
| RN10 ✅ | An older Apple GPU is given high | Every Apple GPU gets high (`QualityLevel::for_adapter`), as every one known when it was written was fast. An iPad (6th generation, A10, 1536×2048) draws Sheepdog at 13 fps at high and 17 at medium. At low it's 45, held back by 4× anti-aliasing: 59 without it, 58 with 2×, and 59 at half the pixels with it. The passes already discard the multisampled images and keep them on the chip, so it's the anti-aliasing's own cost. Proposed: Apple's A-series GPUs before the A12 pick low with 2× anti-aliasing, read from the adapter's name; the iPhones and Macs in use keep high. Where to cut is a guess: only an A10 and an A18 have been measured **Closed as Unity has it** (the user's word, 2026-10-28): the engine keeps one pick per kind of device, and a game that wants another for older ones chooses it, as a Unity game does: `SystemInfo.graphicsDeviceName`, `graphicsDeviceVendorId` and `graphicsApi` say what the GPU is (`fy_gpu_describe`, ABI 47), with an example in the guide's quality section. Since lit particles are lit once each, the iPad's high is 18.7 fps in the field and low without anti-aliasing 53 | S |
| RN11 ✅ | Anti-aliasing costs an Apple GPU ten times what it costs Unity | The field at low on the iPad (A10): 4x anti-aliasing adds 11 ms a frame for Fynkel (17.9 → 29.0), 1.4 for Unity (13.2 → 14.6). Pass by pass (above): at medium it's 26 of the scene pass's 51 ms, and soft particles 14 of them. Soft particles read the solid scene's depth, and get it the way a see-through surface gets what's behind it: the scene in two passes, the multisampled depth stored and copied to one sample, the colour resolved and drawn back into every sample for the second, the depth loaded again. Each of those moves the whole image through memory, which a tiled GPU otherwise never does; the A10 has least to spare. **Soft particles done**: unless a shader reads what's behind, they read the solid scene's depth from a pass of its own at one sample (ambient occlusion's draws, factored out; cut-outs aren't in it), and the scene stays one pass whose samples never leave the chip. Soft particles 14 → 6 ms on the iPad at medium; at 4x anti-aliasing only one sample's depth is kept (`particles.rs`). **Left**: lit particles shaded per pixel cost the A10 four times as much with anti-aliasing (above). **Lit particles done** (the user's word, 2026-10-28): the built-in material's squares and trails are lit once each, at a square's middle or a trail's point, as their vertices are placed: the ambient light, the sky, the suns with their shadows and every lamp in reach, as a matte surface (no highlights). A particle is the same colour across, which dust and smoke don't show; meshes and a material's own shader stay lit pixel by pixel. On the iPad at medium the scene pass 43 → 19.7 ms, 17.5 → 30.0 fps; at high 12.0 → 18.7 (`a_lit_particle_is_lit_once_at_its_middle`) | M |
| RN12 ½ | HDR, bloom and ambient occlusion cost twice Unity's | The field at medium and high: on the Note10+ Unity holds 52–53 fps, Fynkel 32 and 24 (render thread 29 and 38 ms); on the iPad 37 and 58 ms of GPU against 65 and 91. Pass by pass (above): on the iPad HDR is small (the scene 2.4 ms, bloom 2.9, finish 5.8) and the cost is RN11's; on the Note10+ HDR is 8.4 ms (bloom 4, finish 2.2, the scene 2.3). Candidates there: the scene image is half floats (8 bytes a pixel) where URP's mobile HDR uses 4 (R11G11B10); bloom's chain; the finish pass reading the whole image again. Ambient occlusion at high: 11.2 ms on the iPad. **The scene image done** (the user's word, 2026-10-28): it holds 10 bits a colour (4 bytes a pixel), not half floats. It holds colour already tone mapped, 0 to 1, so 10 bits are finer than the screen's 8 everywhere but the darkest tones (below about 0.023, linear), and there the surfaces and the sky add noise of up to half a step, as URP's camera dithering does (`fy_dither`, `a_dark_gradient_doesnt_band_in_the_scene_image`). Interleaved gradient noise from the pixel's position cost the Note10+ 1.5 ms of scene; a hash of the surface's point in the world costs a third of that. Keeping the image's light curved (gamma) to spend its bits on the darks can't work: blending adds the stored values, so glows and glass would come out wrong. What it costs the look: near black a pixel is up to one 10-bit step (4/255 on the screen) off, a grain seen only brightened (the goldens allow it there); additive glows piled on each other stop at white where half floats went past it, so the brightest core of a burst of sparks and its bloom are a little less (`particles_look`, `particles_ring`, `particles_fountain` blessed again); bloom of lit surfaces within about 10% up to 20 times white. The Note10+ in the field: medium 41.6 → 45.3 fps (bloom 4.07 → 2.65 ms), high 28.6 → 31.5; undithered it was 45–47. **Left**: bloom's chain and the finish pass | M |
| WO10 | Each view recomputes every world matrix each frame | 75 µs for 3000 objects. **Half done with WO4**: a frame where nothing moved skips them; one moved object still recomputes them all | M |
| RN4 | Transparent particles re-sorted with depth recomputed per compare | 0.73 ms for 10k, 0.13 keyed. **Done**: each particle's depth and index packed into one number, those sorted, and the instances made in that order; equal depths keep the system's order. Preparing a frame of 10,000 see-through particles: 0.600 → 0.101 ms (Mac), a CI benchmark (`bench_preparing_ten_thousand_see_through_particles`, bar 0.2 ms); fynkel_render joins the benchmarks step | S |
| PH5 | Any mesh change, colours too, rebuilds its collider; each rebuild scans all colliders | 10 colour sets = 10 rebuilds. **Done**: a mesh counts the changes to its positions or triangles apart (`shapeVersion`, in `extend.dart`), and physics follows only those; colours, UVs, normals and tangents changed every frame leave its colliders, and what sleeps on them, alone. The scan over all colliders stays: it runs only when the shape changes, and making the shape again costs far more (3 ms for 10,000 triangles against microseconds for the scan) | S |
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
| GD2, GD14 ✅ | §19 never shows a test loading assets (`World(source: …)`) or `await game.loaded` without frames. **Done**: §19 has one, run by `check_guide.py` | S |
| GD9 ½ | No way to check "no garbage" in a game's own tests, though the guide promises it; `finger.position`, `screenPointToRay`, `screenPointToPlane` make new objects each call. **Half done**: measured, a drag frame makes 9 objects (6 `Vec3`, 2 `Ray`, a `ViewPoint`), from the math types being immutable classes (inlining doesn't remove them; Dart has no structs); the guide's speed section now says which "no garbage" claims hold (the bulk paths) and that a few value objects a frame are fine. Open: a test helper. `flutter test` has no VM service unless run with `--enable-vmservice`, and can't spawn the counter's isolate (`Isolate.packageConfig`/`spawnUri` unsupported), so it needs a separate `dart` process counting through the service | M |
| GD4 ✅ | The guide names `Stick(…)` as if it shipped; it's only in the lab. Ship it in `fynkel_flutter`. **Done**: `Stick(input, axis:, floating:, size:)`, Ring Ring Deals' stick made general (`floating` appears under the thumb, taps go through); the lab's copy gone | S |
| GD3 ✅ | `pausedListenable` needs `fynkel_flutter`'s import; §11 doesn't say. **Done**: it says | S |
| GD6 ✅ | `audio.music.ignorePause` isn't named where pausing is. **Done**: named in the music paragraph | S |
| GD7 ✅ | The "quality doesn't draw bloom" warning prints in every low test, with no way to quiet it. **Done**: it says so only once a view draws the world; a test's world stays quiet | S |
| GD10–12, GD15, GD16 ✅ | hints: additive trails + bloom halo, `setSweep`'s message, both bodies hear a collision (and `id`), pooled trails cleared, a root game with arguments. **Done**: each in its section; a sweep along knots at one point says so | S |
| DC7 ✅ | `getInChildren` null before add, where `getInParent` throws. **Done**: both see the objects given before add | S |
| BR9 ✅ | A failed view is a grey box for good: no retry, no hook beyond `FlutterError.onError`. **Done**: `FynkelView(onError:, errorBuilder: (context, error, retry))` | S |
| BR8 ½ | ABI conventions drifted (`fy_world_time_set` breaks the ABI's own rule); a written rule and a check | S |

**CI and tests**:

| Row | What | Size |
|---|---|---|
| GT4 ✅ | What a phone runs is never compiled in CI: Kotlin, iOS Swift branches, sound's session code, mobile Rust targets. **Done**: CI builds the lab for iOS and Android (step 'phone builds') | S |
| GT2 | The soak's memory check fails on noise (3 in 3 days, 1 on main). **Done**: the process's total wanders because the Dart VM's pages and malloc's freed ones come and go (both counted on their own already); the total was flat by category (`footprint`). It's now held only for the rest: 5.2 MB undrawn, the same every round; 21–24 MB drawn | S |
| GT12 ½ | One serial job, 16–19 min; `cancel-in-progress` cancels runs on main. **Half done**: runs on main are never cancelled (a merge's run was, 2026-10-30). Still one job, ~56 min now: one runner, so parallel jobs wouldn't be faster | S |
| GT8 ✅ | 70 guide blocks analyzed, 4 run. **Done**: `tools/check_guide.py` also makes `test/guide_runs_test.dart`: every game object and behaviour the guide writes that's made with no arguments (37), and every function making a game object (`ball()`, `player()`, 6), each in a world of its own with a camera, run a second, no errors; its assets are the guide's own shaders and stand-ins from the repository. 48 tests run, from 5. Without assets 15 failed, so it catches what breaks | S/M |
| GT5 ✅ | Pocket Hole outside CI; `lab/tool` not format-checked. **Done**: both in CI | S |
| GT10, GT11 ✅ | `fynkel_measure` barely tested; `wait(tester)` copied 15 times, the golden comparator twice: a `fynkel_flutter` testing library. **Done**: `package:fynkel_flutter/testing.dart` (`tester.frames(n)`, `tester.run(seconds)`, `tester.game<T>()`) in every example's widget tests; the golden check is `package:fynkel/testing.dart`'s `expectImage` / `view.expectFrame(path)` for any game, with its own PNG reader (no `package:image`), and the repository's and the lab's goldens go through it; `fynkel_measure` tests its footprint, thermal state, stats line and `MeasuredApp` | S |
| — ✅ | hello has no test; the guide's example list misses scale and pocket_hole. **Done** | S |
| GT13 ✅ | (2026-10-31, from Ring Ring Deals) A character stopping for a frame on open ground got past every test: only the game found it. **Done**: `fynkel_physics/test/walking_test.dart` walks the game's capsule round for a minute, frames as a phone gives them, and loses none (the old controller: 114 of 3814) | S |

**Rewrites** (§6, decision 2):

| Row | What | Size |
|---|---|---|
| RN8 | `renderer.rs` 3516 lines, `encode` one 630-line function, pipeline making repeated ~10 times: passes as their own units | L |
| BR11 | `FynkelViewState`'s four platform hosts share one set of flags, where V1, V2, V4 and V12 lived: one class each. **Done**: a `_Host` each (Apple texture, Apple layer, Android surface, texture or on screen), the state holding one | M |
| BR10 | `abi.rs` 5730 lines into modules; the header stays byte-identical | M |
| DC5 | (phase 3, above) | L |

### Fresh developers again (2026-10-31, at `2a0ed245`)

The same two briefs, given to two new agents with the same rules (the
guide and the packages' public code only), into `/tmp/s14b`; their logs
are [courtyard-log-2.md](stage-14-audit/courtyard-log-2.md) and
[marble-log-2.md](stage-14-audit/marble-log-2.md).

**Both games built clean and passed every test**: Courtyard 785 lines
and 10 tests, Marble Run 463 lines and a shader, 8 tests, in about 10
and 6 minutes. Marble Run's first draft analyzed clean and passed its
first 6 tests with no wrong guess at the API; Courtyard's only compile
error was `playAt(group:)` taking an `AudioGroup`, not a name.

Of phase 1's findings, these weren't raised again: assets in tests
(GD2), `pausedListenable`'s import (GD3), `Stick` (GD4), the bloom
warning (GD7), the trail halo, `setSweep`'s message, both bodies hearing
a collision, pooled trails, a root with arguments, `await game.loaded`
(GD10–16), the build race (GD1; both built at once, from a warm cache).
Raised again:

| Row | What | Now |
|---|---|---|
| GD9 ½ | No way to check "no garbage" in a test | as before: the helper is left for shipping; §20 says what holds |
| GD5 | Point lights cast no shadows | decision 3 sent it to §21, which never listed it. **Done**: §21, §5 (a torch as a point light and a shadowed spotlight) and `Light.point`'s doc say so |
| GD6 | Music plays through a pause; `ignorePause` from the next track | §13b named it, §11's pause didn't. **Done**: §11 does. Applying it to the track already playing needs a mixer command, left |
| GD8 | A routine's `wait` ends on a frame | **Done**: §19 says `world.run` moves in whole frames |

New, all small, **done** in the guide unless said: `mesh.shapeVersion`
said to need `extend.dart` (`fynkel.dart` has it); a surface shader's
inputs only in `fynkel.wgsl`; a sweep's u and v; `setSweep` into an
empty mesh; when a clip from samples is freed; where `fixedDelta` is
set; `world.all` giving a copy; a page transition in a widget test
(`tester.run(0.5)`). Left as they are: a few parameters only in the
doc comments (`Follow`'s, `Light.spot`'s), and a reused burst (one
system moved and `emit`ted), which works and the guide doesn't show.

### The pass over the public API (2026-10-31, at `bf27879a`)

Five readers, one per area (the core, the features, physics, sound,
the Flutter widgets with `fynkel_measure`), went through every public
name, default, error message and doc. Each tested what it claimed about
behaviour with a throwaway test. Their notes are in
[stage-14-audit/api/](stage-14-audit/api/), each finding with its file,
line, fix, and whether the fix breaks a game's code. 78 findings: 13
bugs, the rest errors that don't say what to do, docs that say
something else, and gaps. Ids: C core, F features, P physics, A sound,
U Flutter.

**Bugs**

| Row | What | Size |
|---|---|---|
| U1 | A `FynkelView(root:)` that Flutter makes again (a view wrapped in a `Stack` only sometimes) destroys the game: the new view takes the world over, then the old one disposes it | S |
| U2 | A game that pauses itself while a dialog covers it is unpaused when the dialog closes, against `pauseWhenCovered`'s doc | S |
| U3 | `tester.game<T>()` gives the widget's `root`, not the one shown, for `build() => FynkelView(root: MyGame())`; a view given a new root or `make` keeps the old game, saying nothing | S |
| A1 | A source that's off, or on an object that's off, plays; a loop then outlives its object (10 s after `destroy()`) | S |
| A2 | Setting `music.volume` applies a waiting `loop` or `group` change to the playing track | S |
| A3 | `music.play(clip, fadeIn: -1)` stops the track, then throws: no music | S |
| A6 | Another world's `AudioGroup` is used as this world's group of the same number | S |
| P1 | A joint the engine refuses (to its own body, a NaN anchor) is lost for good, even after a good value; the bad value stays | S |
| P2 | Queries give a read-only list only when nothing was hit, so `.sort()` fails only on a miss | S |
| C3 | `world.all`, `first` and `only` give objects whose `onLoad` hasn't finished, not yet shown; their `late` fields throw | S |
| F1 | `Mesh.extrude(Shape2D.circle(0))` panics in the engine | S |
| F2 | A NaN shake or a `VirtualCamera` value out of range fails every frame, and after 5 the whole camera system is off for good, naming no camera | S |
| F3 | `SplineAnimate` throws every frame once its spline's object is destroyed | S |

**Errors that should say what to do** (no game that works changes):
- **The engine's own checks speak Rust** (C1, C2, P8): `Camera(fovY: 60)`
  gives "Camera.fov_y: 3437.7468 is outside 1..=179", in radians
  turned to degrees, at `world.add`, naming no object. Fix: check in
  Dart where it's set, with the Dart name, the value as given and
  what's allowed ("fovY is 60 radians; did you mean 60.deg?").
- **"Instance of 'Shot'"** where the value should be (C7, F5): about a
  dozen `ArgumentError.value` calls. Fix: one helper that names it.
- **One-offs**: C4, C6 (`start`, renamed `startRoutine`, still in an
  error and the guide), C8, C9, C10, C13, F6, F7, F8, F15–F18, P4, P13,
  A4, A10–A13, U14.

**Docs that say something else**: F4, F10, F11, P6, P7 (the guide's
`body.velocity`), P11, P12, C12, U6 (a bare view doesn't reload
shaders on hot reload; fix the view, not the doc), U7, U13, U17, A14.

**Gaps a game hits**: U4 (`FynkelApp` passes on none of the view's
settings: no stats, no error hook), U5 (a page pushed from the overlay
can't find the game), U10 (the overlay isn't a `Stack`, so `Stick`'s
`Positioned` fails), A5 (`AudioSource(group:)`), P3 (a joint with no
rigidbody waits silently), P5 (`Rigidbody.position` throws before it's
added), P10 (`copyWith` for joint motors, springs, limits and
materials), F12 (`Color.withAlpha`, `Vec2.normalized`, a quaternion's
angles), C11, P9, U9, U11, U12, U18, C14, C15.

**Changes a game's code would see** (for the user's word):
- **C3, C5, Unity's defaults**: `all`/`first`/`only` leave out objects
  still loading, and `getInParent`/`getInChildren` leave out what's off
  unless `includeInactive: true`, as Unity's do.
- **Names**: Animator's `duration:` → `seconds:` as everywhere else
  (F9); music's `time` → `playTime`, settable, as a source's (A7);
  `Mesh.lathe`'s `sides` → `segments` (F14); `FynkelView.onScreen`,
  read as a callback (U8); `Controls.fromJson` as a method (U15);
  `resolutionScale` against `renderScale` (U16); the two `shake`s (F13).
- **More sound**: one-shots that say when they end and stop alone (A8),
  a source's `fade`, music's `pause` (A9). New in the mixer, so bigger.

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
