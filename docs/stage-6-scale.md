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
