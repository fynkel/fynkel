# Stage 2 report: a small scene you can play with

Step 7 of [the stage 2 plan](stage-2.md). Measured on 2026-10-08 on the
step 6 branch (merged as 2a4566c0), with package:fynkel_measure's tool on
the profile build (AOT). The details of each step, and how each number is
taken, are in [stage-2.md §6](stage-2.md#6-measurements).

## The result

`examples/garden` is the snippet at the top of the plan, grown into a
scene: a textured ground, Kenney's character among 39 of the Nature Kit's
props, a tap on the character turns its head, a drag goes round it, a
scroll (or two fingers on a trackpad) zooms. Everything is loaded at run time
from bytes, decoded off the UI thread, and freed again on request. It runs
at the display's full rate on all three platforms with no dropped frames.

| 60 s of the garden | Mac (M3 Max) | iPhone 16 Pro Max | Galaxy Note10+ |
|---|---|---|---|
| Display | 120 Hz, window 1600 × 1200 | 120 Hz, 1320 × 2868 | 60 Hz, 1080 × 2241 |
| Fresh frames per second (median, min) | **120**, 119 | **120**, 117.7 | **59.9**, 57.9 |
| Frames drawn / shown | 7,257 / 7,256 | 7,319 / 7,319 | 3,573 / 3,573 |
| Dropped | 0 | 0 | 0 |
| Flutter frames over one frame | 0 | 0 | 275, 271 of them rasters (cost no frames) |
| Dart allocation per frame | 19.1 KB | 12.7 KB | 19.5 KB |

As in stage 1, the Note10+'s slow rasters are Flutter's raster thread
compositing the engine's texture on OpenGL ES, a frame behind; they cost
no frames.

**Loading** (the grass, twelve prop models and the character at once;
"on screen" is from the start of the load to the first frame shown with
it in, read from the engine's frame counts):

| | Mac (warm) | iPhone | Galaxy Note10+ |
|---|---|---|---|
| Whole scene, loaded and spawned | 45 – 73 ms | 23 ms | – |
| Whole scene, on screen | 65 – 153 ms | 148 ms | 0.44 – 1.3 s |
| The character again: loaded / on screen | 1 – 3 / 18 – 35 ms | 6 – 9 / 24 – 33 ms | – |

The Note10+'s scene waits at startup for the view, which makes its GPU
device on the UI isolate in 0.84 – 0.91 s (4 – 61 ms on the Mac, 7 ms on
the iPhone). The first launch on the Mac after a build takes about 1 s.

**Memory over the character's unload and reload cycles**, against Flutter
alone (`--baseline`, a spinning box in place of the view):

| Growth after each unload | Mac, 20 cycles | iPhone, 30 cycles | Galaxy Note10+, 30 cycles |
|---|---|---|---|
| The garden | 3 MB (after +7 MB once) | 20.1 MB | 30.6 MB |
| Flutter alone | 5.0 MB | 17.5 MB | 17.3 MB |
| GPU part | −5.33 MB each unload, exactly | flat (170 – 172 MB) | flat (123.3 MB) |

On the Note10+ the extra is the native heap: about 16 MB over the first
14 cycles, then flat to cycle 30, after which the garden grows as Flutter
does (0.44 against 0.34 MB a cycle). Bounded, so freed memory kept by the
allocator or the driver; you accepted it as a pass.

**The phone numbers stage 2's first steps owed** (`measure bench`,
through package:fynkel on the UI isolate):

| | Mac | iPhone | Galaxy Note10+ |
|---|---|---|---|
| Extract, 10,000 objects in trees (median) | 0.30 ms | 0.27 – 0.30 ms | 2.0 – 2.6 ms |
| Raycast, 10,000 cubes, flat / in chains | 83 / 193 µs | 80 / 191 µs | 560 / 1,668 µs |
| The garden's tap | 3 µs | 4 µs | 22 µs |

The iPhone is as fast as the M3 Max; the Note10+ is 6 – 8 times slower,
so 10,000 objects in trees take 2.5 ms of its 16.7 ms frame.

## What stage 2 built

- `fynkel_mesh` (the 48-byte vertex, shapes, MikkTSpace tangents) and
  `fynkel_load` (PNG and JPEG decoding, `.glb` parsing, no GPU), both new.
- In `fynkel_world`: parents, children and names, world transforms in one
  pass, recursive despawn, textures and models, raycasts. In
  `fynkel_render`: UVs and tangents, custom meshes, textures with GPU
  mipmaps, textured PBR, cut-outs.
- In `package:fynkel`: `Texture2D`, `Model`, `Mesh.custom`,
  `parent`/`children`/`find`, `Ray` and `raycast`, `Material.dispose`. In
  `package:fynkel_flutter`: `FrameInput` and `onViewStats`.
- `package:fynkel_measure` (stage 1's measuring, shared by both examples,
  now with a bench mode), `examples/garden`, `tools/garden_assets.py`, 12
  goldens, and the Khronos sample models in the tests.

## What was found on the way

- The hierarchy costs 0.13 ms a frame for 10,000 objects in trees and
  nothing for objects in none; the order is rebuilt only when a parent
  changes.
- Built-in shapes with generated tangents were 100 times slower than with
  exact ones (the sphere: 3 → 0.03 ms), so they aren't generated.
- On the Android emulator, the model tests found a race from step 3: the
  render worker published a frame before its GPU byte counts.
- Raycasts were twice as fast once each object's box was tested in world
  space (10,000 cubes: 210 → 85 µs).
- A model couldn't be unloaded: the world kept every material ever used,
  which kept their textures. `Material.dispose` is new.
- Kenney's Nature Kit stores sRGB colours where glTF wants linear ones and
  calls every colour metal; the character needs an extension it doesn't
  use. The asset tool fixes the files; our reader follows the spec.
- The measuring needed fixing again: an iPhone app's output reaches the
  tool only after it connects, so the reports are now read at the end; and
  10 iPhone cycles were too noisy to decide, so it took 30.

## Still open

- The view makes its GPU device on the UI isolate: 0.84 s on the Note10+
  at startup. It could be made on another thread, or shared by all views
  (stage 1's open point).
- A model disposed without its parts keeps their native memory until the
  garbage collector finalizes them, and the collector doesn't know their
  size (`externalSize` on the finalizers).
- The Note10+'s bounded native warm-up (about 16 MB) isn't explained
  further than "kept by the allocator or the driver".
- From stage 1: a surface Flutter closes under the engine; the Flutter
  leak report; 64-bit Android only.

## Seen on each device

Step 7 is done when you have played the garden on each device yourself.
Confirmed on 2026-10-08:

- [x] Mac
- [x] iPhone 16 Pro Max
- [x] Galaxy Note10+

**Stage 2 is done.**
