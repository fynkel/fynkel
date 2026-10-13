# Stage 6, step 5: scale and stability

The four checks of [stage-6.md](stage-6.md) §1, with their numbers.
They ran on 2026-10-12 on the Mac (M-series, 120 Hz), the Note10+
(Android 12, Mali-G76) and the iPhone.

- The scale world is [examples/scale](../examples/scale): groups of ten
  objects in 50 kinds that are still, spin, bob, beat to a signal or
  blink.
- The error check is `packages/fynkel/test/errors_everywhere_test.dart`.
- The hot reload check ran on package:fynkel_physics' tumble example.

## Results

| Check | Result |
| --- | --- |
| A. 10,000 objects of 50 kinds | Mac **120 fps**. Note10+ **34.9 fps**, its UI thread the limit. iPhone **44 fps**, its GPU the limit. See §A |
| B. 30 minutes on each phone | Note10+ **58.9 fps** (min 54.9) for 30 minutes, memory flat at 279–280 MB from minute 8. iPhone **120 fps** (min 110), memory 404 → 398 → 400 MB |
| C. Errors in every lifecycle method | **The game goes on.** 7,499 errors from 25 places over 600 frames, each one reported |
| D. Hot reload in the middle of play, physics running | **Works.** 30 reloads with an edit, 10 hot restarts, 0 errors; memory 234 → 239 MB |

## A. 10,000 objects of 50 kinds

### The Mac

The world runs at 120 fps, with no slow Flutter frames over a minute.

- **Dart allocation:** 310 KB a frame. It comes from the immutable
  `Vec3`/`Transform` that every move makes, and from the tweens and
  routines. It costs 223 collections a minute and none of them shows.
- **Footprint:** 791 MB at the start, 643 after a quarter, 668 at the end.

The lifecycle check (`measure lifecycle --cycles 10`) destroys and remakes
the world 10 times, shows a new view 10 times and resizes 5 times. Memory
stays between 645 and 660 MB throughout, so nothing leaks.

**An open question for step 6: the GPU part of the footprint.** It grows
with the scene far more than the instances explain.

| Scene | GPU part of the footprint |
| --- | --- |
| hello (one cube) | 278 MB |
| scale, 1,000 objects | 332 MB |
| scale, 10,000 objects | 455 MB |

That is about 14 KB an object, where its instance is 164 bytes in a
3-buffer ring (5 MB in all). The 50 materials may be the cause: a
`queue.write_buffer` blit each frame keeps 128 MB of Metal driver memory
dirty (docs/stage-1.md §6 q6). Look with Xcode's memory tools before
architecture item 14.

### The Note10+

**10,000 objects at 34.9 fps** (min 23.6), 358 KB of Dart allocation a
frame. Each behaviour alone, all 50 kinds doing it:

| Behaviour | fps (median, min) | Dart a frame |
| --- | --- | --- |
| still | 34.9, 27.9 | 33 KB |
| spin | 31.9, 23.0 | 258 KB |
| bob | 35.9, 26.9 | 760 KB |
| beat | 35.9, 30.9 | 580 KB |
| blink | 32.9, **13.0** | **1.3 MB** |

The behaviour barely matters, so the cost is in the frame itself. The
mesh, measured back to back (`SHAPE=`, 30 s each):

| Objects | Built-in sphere (2,208 triangles) | `SHAPE=low` sphere (720) | Cubes only |
| --- | --- | --- | --- |
| 1,000 | 23.0 fps, slow frames raster | 33.0 fps, raster | 59.9 fps |
| 10,000 | 34.9 fps (33.9 again), slow frames **build** | 16.7 fps, build | 22.0 fps (22.0 again), build |

Two different limits:

1. **At 1,000 objects it's the GPU, and the built-in sphere is too heavy
   for it.**
   - The slow frames are raster frames.
   - The lighter sphere takes 23 fps to 33. Unity's sphere has 768
     triangles; ours has 2,208.
   - Half the resolution (19.6 fps) or the camera turned away (23.0 fps)
     doesn't help, so the cost is the vertices, not the pixels: every
     object is drawn, seen or not (architecture item 8, no culling).
   - Cubes still run twice as fast as the light spheres, so there is more
     to find on Mali than the triangle count.
2. **At 10,000 objects it's the UI thread, whatever the mesh.**
   - The slow frames are build frames.
   - The same Dart code is slower with cubes: `world.update` takes 14 ms
     there, 8 ms with the built-in spheres. Both results repeat.
   - The bench's tight loop times `world.update` at a 1.5 ms median and
     the copy into the renderer (extract) at 1.9–2.3 ms. The live app is
     several times slower.
   - The CPU governor is part of it: the fast cores can sit at 520 of
     2,730 MHz, and two busy loops beside the app took update to about
     11.5 ms.
   - It isn't heat (31–34 °C), low-power mode (off) or the frame clock:
     arena still runs at 59.9 fps.
   - **What a 5-second profile shows (simpleperf, 10,000 cubes):**
     - The UI thread has 66% of all samples: 62% of that is Dart code,
       26% the engine (Rust).
     - The engine's top function is `World::material`, about 7% of the
       UI thread: a hash-map lookup and a copy of the material for every
       object, every frame, in the extract.
     - The raster and render threads have 13% each.
   - **Why cubes make the UI thread slower is not known.** One guess:
     faster GPU frames make the render thread take the world's lock more
     often. The next profile needs Dart symbols
     (`--split-debug-info`).

**The iPhone at 10,000 objects runs at 44 fps** (min 14.5), and its limit
is the other side:

- Flutter ran 6,937 frames in the minute, none slow to build, so the UI
  thread keeps up.
- Of the 6,931 frames posted to the engine, the render side drew 2,519;
  4,412 were skipped.
- 5,000 built-in spheres are 11 million triangles a frame, so the lighter
  sphere and culling are the fix there too.
- Memory: 547 MB at the start, 556 at the end.

Blink is the pooling pattern (hide, show again). It allocates 1.3 MB a
frame and dips to 13 fps, because every toggle rebuilds the hidden set.
That confirms architecture item 6.

**What step 6 does with it:**

- **Lighter built-in sphere** (and capsule) meshes.
- **Culling** (architecture item 8).
- **The extract looks a material up once per batch**, not once per
  object.
- **A profile of the UI thread with Dart symbols** at 10,000 objects on
  the Note10+, to find where the live frame's time goes and why the mesh
  changes it.
- **Android performance hints (ADPF)**, so the governor lifts the clocks
  for the frame's threads.
- **The incremental copy (item 8's question) is not needed yet.** The
  extract is about 2 ms of a 16.7 ms frame. Culling moves work off the UI
  thread first; measure again after it.

## B. 30 minutes on each phone

The run was `measure rate --seconds 1800` with 2,000 cubes, replacing 20
groups a second (200 objects destroyed and made anew every second, 36,000
groups in all). On the Note10+ memory was read with `dumpsys meminfo`
every minute.

**The Note10+.**

| Measure | Result |
| --- | --- |
| Frame rate | 58.9 fps median, min 54.9, over 1,797 one-second windows |
| Frames | 106,333 posted, every one shown |
| Slow Flutter frames | 2,715 (2.5%), nearly all raster |
| Longest raster | 62.6 ms |
| Dart allocation | 9.9 KB a frame, 341 collections in 30 minutes |

Memory (`dumpsys meminfo`'s KB, in thousands):

| Time | Total PSS | Native heap | Graphics |
| --- | --- | --- | --- |
| Start | 244 | 41.0 | 125.5 |
| 4 minutes | 276 | 43.7 | 126.3 |
| 10 minutes | 279 | 44.1 | 126.3 |
| 20 minutes | 280 | 44.4 | 126.3 |
| 29 minutes | 280 | 44.3 | 126.3 |

It grows in the first minutes, while Dart's heap and the allocators find
their size, then stays flat for 20 minutes with 200 objects made and
destroyed every second. Nothing leaks. Architecture item 14 (CPU copies
of textures and meshes) doesn't show here: the scale world has only
built-in meshes. Its urgency is a question of a level's textures, so it
stays where the architecture review put it.

**The iPhone.** The tool reads its footprint at the start, after a
quarter and at the end; there is no `dumpsys` to read in between.

| Measure | Result |
| --- | --- |
| Frame rate | 120 fps median, min 110, over 1,800 one-second windows |
| Frames | 216,012 posted; 41 coalesced, 6 skipped, none dropped |
| Slow Flutter frames | none; longest raster 6.85 ms |
| Dart allocation | 4.4 KB a frame, 340 collections in 30 minutes |
| Footprint | 404.0 MB at the start, 398.2 MB after a quarter, 399.8 MB at the end |

## C. Errors everywhere

`errors_everywhere_test.dart` throws, every frame for 600 frames, from
every place a game's code runs:

- the lifecycle methods of an object and of a behaviour (`onEnable`,
  `onStart`, `onFixedUpdate`, `onUpdate`, `onLateUpdate`, `onDisable`,
  `onDestroy`);
- `onLoad`;
- a signal's listener, routines, and a tween's `onComplete`;
- the pointer methods (`onPointerDown`, `onPointerUp`, `onClick`,
  `onDragStart`);
- a `WorldSystem`'s four hooks.

Every 10 frames the throwing object is also turned off and on again, and
an object that fails to load is added. A witness object counts what it
gets:

- 600 updates and 600 late updates;
- 495–505 fixed steps (50 a second);
- its routine's waits go on.

All 25 places are reported, 7,499 errors in all, and the game goes on.
Physics callbacks that throw are checked in fynkel_physics'
`contacts_test.dart`.

## D. Hot reload with physics running

Tumble with 500 rigidbodies, each with collision callbacks
(`TUMBLE_RIGIDBODIES=true TUMBLE_LISTEN=true`), ran under
`flutter run -d macos`. A script then:

- **hot reloaded 30 times, 4 s apart, each after an edit:** 84–123 ms
  each, the reassemble 12–21 ms;
- **hot restarted 10 times, 8 s apart:** 151–192 ms each.

Nothing was reported. The app's memory was 234 MB before and 239 MB
after, peaking at 260 MB during the restarts. Architecture item 3 (the
physics buffers a restart drops without freeing) and item 13 (a GPU
device made on every restart) stand: what they keep is too small to see
here, so 10 restarts don't show it.

## Also found

- **The docs say `=1` where the code needs `true`.** Tumble's comments
  (and the scale tools' first try) pass `--dart-define=TUMBLE_RIGIDBODIES=1`,
  but `bool.fromEnvironment` is true only for `true`. Fix the comments in
  step 6.

## Step 6: measured again

Stage 6 step 6's PR 15 ran check A again on 2026-10-13, after the fixes
(culling, the lighter sphere and the material looked up once per batch
came in PR 8). It found two slowdowns that PRs 4 to 8 had brought in, and
fixed both in the same PR.

| 10,000 objects | Step 5 | Step 6 |
| --- | --- | --- |
| Mac | 120 fps | **120 fps** |
| iPhone | 44 fps, its GPU the limit | **120 fps** (min 119); all 7,199 frames drawn but one |
| Note10+ | 34.9 fps, its UI thread the limit | **27.0 and 35.9 fps** shown, in two runs; the game updates 52–56 times a second; its GPU is now the limit |

### What had got slower, and the fixes

**1. World positions worked out again at every move.** Since PR 4 an
object keeps its world transform in Dart, worked out from its parents'.
It was kept "until anything in the world moves": one count for the whole
world. In the scale world 9,000 parts bob, each reading and setting its
`position` (the world's, since PR 5), so every part worked out its
parents' transforms again at each move. That made 11 MB of Dart garbage a
frame on the Mac with only bobbing (step 5's code: 0.84 MB).

- Now each object keeps its world transform for as long as its own
  transform and its parent's world transform are the same ones it was
  worked out from. One object moving leaves the others' alone.
- Setting `position` also keeps what it just learned: the world
  transform is the position set, turned and sized as before.
- `Quat.rotate` and `inverseTransformPoint` now work in plain numbers
  and make only their result: the old `rotate` made seven `Vec3`s.

With only bobbing: 11.3 MB a frame → 1.15 MB. That is still 37% more than
step 5's 0.84 MB, the price of turning a world position into the
parent's space. With all five behaviours: 2.45 MB → 0.37 MB (step 5:
0.31 MB).

**2. On Android the game's update waited for the GPU.** Bisecting the
PRs on the Note10+ put the drop at PR 8 (30.9 → 11.8 fps), though PR 8
draws less. A Perfetto trace showed why:

- Flutter takes each of our frames on Android's main thread, and waits
  there until the GPU has finished it. On Android 12 the `ImageReader`
  does it when the frame is acquired; from 13 on, Flutter itself waits on
  the frame's fence.
- Dart runs on that same thread, so the next update waited too:
  `waitForever` on the main thread took 44 ms a frame (6 ms on PR 7).
- It depended on timing, which explains step 5's puzzles. A quicker
  render thread (PR 8's culling, or cubes in step 5) handed the frame
  over sooner, before the next update had started, and the update then
  waited behind the GPU. A slower one handed it over while the update
  ran, and the two overlapped. A render thread made slower on purpose
  ran faster.
- **The fix:** the render thread now waits for its own frame's GPU work
  before handing the frame to Flutter
  (`crates/fynkel_present/src/android.rs`). It has nothing else to do in
  the meantime, and the main thread finds each frame ready. The main
  thread's wait dropped from 44 ms to 0.01 ms a frame.
- On the Note10+, 10,000 objects went from 12.7 fps to 27–36, and 1,000
  from 23 (step 5) to 54.9. Arena, bowling and garden hold 59.9.

### The Note10+

10,000 objects of all five behaviours: **27.0 fps** on screen over 60 s
(min 18.0), the update running 52 times a second; 35.9 fps over 30 s in
a second run, the update 56 times a second. The render side
skips the scenes it can't keep up with, so the UI thread is no longer the
limit: the GPU is. ~5,500 objects are in view, the rest culled.

Each behaviour alone (20 s; Dart per update, the allocation over every
update, not over the frames shown):

| Behaviour | Step 5 fps | Step 6 fps shown | Updates a second | Dart per update |
| --- | --- | --- | --- | --- |
| still | 34.9 | 30.0 | 58.7 | 30 KB |
| spin | 31.9 | 26.0 | 55.4 | 275 KB |
| bob | 35.9 | 28.0 | 54.8 | 1.19 MB |
| beat | 35.9 | 27.0 | 54.0 | 457 KB |
| blink | 32.9, min **13.0** | **37.9**, min 36.9 | 59.7 | 52 KB |

Blink's dip is gone: PR 9 made hiding and showing an object cheap. The
others show a few frames fewer than in step 5, but the game itself runs
about 1.6 times as often: the update no longer waits for the GPU, and the
GPU, now the limit, draws as often as it can. The mesh, back to back (30 s each):

| Objects | Built-in sphere (720 triangles) | `SHAPE=dense` (2,208, step 5's built-in) | Cubes only |
| --- | --- | --- | --- |
| 1,000 | **54.9 fps** (step 5: 23.0) | 53.9 fps (step 5: 23.0) | 59.9 fps (59.9) |
| 10,000 | 27.0 and 35.9 fps (16.7 with step 5's `SHAPE=low`) | 17.0 and 16.7 fps (34.9) | **40.9 fps** (22.0) |
| 10,000, camera turned away | **58.9 fps** (23.0) | | |

Step 5's dense sphere at 10,000 objects is the one case that shows fewer
frames than before: 17 fps, where step 5 showed 34.9. One run right after
the fix showed it at 35.9, and two later ones at 17.0 and 16.7. The game
keeps updating 40 times a second, so it's the GPU. On this Mali GPU, how
fast a frame draws swings with how the frames are paced (its clock follows
its load). The built-in sphere is the light one now, so this is left
open, to look at with a GPU profiler (Arm's Streamline) when the look
work starts.

**Where the UI thread's time goes now** (the Dart VM's own CPU samples,
which name the Dart functions, at 1 kHz over 8 s of the 10,000-object
world):

- `World.update`: 53%. Most of it is the loop that calls each object's
  `onUpdate` and the other lifecycle methods (`_callEach`: 20% on its
  own, as the loop, before any game code). `_runFixedSteps` takes 8.7%
  though the world has no physics: it calls every object's
  `onFixedUpdate` 50 times a second.
- The view's frame (`WorldView.frame`): 37%, of which the engine's
  extract (`fy_view_frame`, Rust) is 30% and sending the moved transforms
  7.5%.
- Getting the world's pointer (`World._engineWorld`, `GameObject._engine`,
  `world`, `pointer`): about 10% between them, mostly from `transform`,
  which fetches it even when it has the transform already.

Those are the next places to look for the UI thread: not calling
lifecycle methods an object doesn't use, and the getters. None of them is
a regression; they were there in step 5 too.

**Android performance hints (ADPF)** can't help on the Note10+:
`dumpsys performance_hint` says `HAL Support: false` (Android 12,
Exynos 9825), so a hint session would do nothing. They wait for an
Android 13+ phone that has them.

### The Mac's GPU memory

Step 5 saw the Mac's footprint grow about 14 KB an object and asked
whether the GPU's part did. It doesn't. `footprint` and `vmmap` split it
(the scale world, 12 s in):

| Objects | Graphics (owned, tagged graphics) | Owned, not tagged | Whole footprint |
| --- | --- | --- | --- |
| 10 | 266 MB | 6 MB | 415 MB |
| 1,000 | 266 MB | 31 MB | 436 MB |
| 10,000 | 266 MB | 111 MB | 555 MB |
| 10,000, camera turned away | 256 MB | 6 MB | |

- The graphics part doesn't grow with the objects.
- What grows (about 11 KB an object drawn) is owned memory that isn't
  mapped into the app and isn't tagged as graphics, in 128 KB pieces.
- It depends on what is **drawn**: with the camera turned away,
  everything culled, it stays at 6 MB. It doesn't grow over time either
  (114.2 → 114.4 MB over 30 s), so nothing leaks.
- It isn't the per-frame data: that ring is written in place by the CPU,
  with no copy (`frame_ring.rs`), and is 5 MB.
- It fits the GPU driver's own memory for the triangles of a frame (a
  tile-based GPU keeps the frame's geometry until it has drawn every
  tile), which grows with what is drawn. Xcode's Metal capture could
  name it for certain. With nothing leaking, it isn't worth more now.

### The other checks

The iPhone at 1,000 objects: 120 fps (min 116). The light games on the
Note10+ with the Android fix (30 s each): arena, bowling and garden all **59.9 fps**, every frame shown. The
Mac at 10,000: 120 fps (min 112), 13 slow frames of 7,237, 370 KB of Dart
a frame (step 5: 310 KB).
