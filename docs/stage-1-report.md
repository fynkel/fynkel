# Stage 1 report: a lit cube, written in Dart

Step 8 of [the stage 1 plan](stage-1.md). Measured on 2026-10-08 on
`main` after step 7 (fadf1ca9), with `examples/hello/tool/measure.dart`
on the profile build (AOT). The details of each step, and how each number
is taken, are in [stage-1.md §7](stage-1.md#7-measurements).

## The result

The cube from the snippet at the top of the plan runs on all three
platforms at the display's full rate, every frame fresh, at native
resolution, with no slow Flutter frames on the UI thread. Its GPU memory
comes back after every create, resize, rotation and trip to the
background. What the total grows by is Flutter's own growth: a Flutter app
with a spinning box in place of the view grows as much.

| 60 s of the spinning cube | Mac (M3 Max) | iPhone 16 Pro Max | Galaxy Note10+ |
|---|---|---|---|
| Display | 120 Hz, window 1600 × 1200 | 120 Hz, 1320 × 2868 | 60 Hz, 1080 × 2241 |
| GPU API (engine / Flutter) | Metal / Metal | Metal / Metal | Vulkan / OpenGL ES |
| Fresh frames per second (median, min) | **120**, 119 | **120**, 119 | **59.9**, 58.9 |
| Frames drawn / shown | 7,257 / 7,256 | 7,197 / 7,197 | 3,597 / 3,597 |
| Coalesced, skipped, dropped | 0, 0, 1 | 4, 0, 0 | 0, 0, 0 |
| Flutter builds over one frame | 0 | 0 | 0 |
| Flutter rasters over one frame | 0 | 0 | 54 (cost no frames) |
| Dart allocation per frame | 8.1 KB (mouse in the view 13 of 60 s) | 12.2 KB | 18.2 KB (Flutter alone: 20.6 – 23.4) |

The iPhone ran over Wi-Fi. On the phone, slow rasters are Flutter's raster
thread compositing the engine's texture on OpenGL ES; that thread runs a
frame behind, so they cost no frames, and their number changes a lot from
run to run (28 to 247 in step 7).

**Memory over 10 create/destroy cycles and 10 resizes** (the total is
`footprint` on the Mac, the app's own `task_info` on the iPhone, PSS + swap
on Android; the GPU part is the driver's):

| Points | Mac | iPhone | Galaxy Note10+ |
|---|---|---|---|
| Created, full size | 433.6 – 434.5 MB (0.9) | 367.8 – 370.4 MB (2.7) | 249.0 – 254.6 MB (5.5) |
| …GPU part | 271.7 MB every time | 167.2 – 167.3 MB | 118.7 MB every time |
| Resized back to full size | 433.9 – 434.9 MB (1.0) | 371.5 – 373.8 MB (2.3) | 255.3 – 259.1 MB (3.9) |
| Resized to 0.6 | 371.8 – 373.0 MB (1.2) | 246.5 – 248.5 MB (2.1) | 194.6 – 197.8 MB (3.3) |
| 20 rotations / 10 trips to the background | – | – | +7.6 / +4.2 MB, GPU flat |

On the Mac, one of two runs had a 9.6 MB range, which is Flutter's
compositor keeping two more window-sized surfaces now and then (step 5).
The 30- and 40-cycle runs of steps 6 and 7 compare each platform with
Flutter alone; the engine's growth matched it everywhere, within about
90 KB a cycle.

## What stage 1 built

- `fynkel_world` (hecs), `fynkel_render` (wgpu, lit and instanced, golden
  images), `fynkel_present` (the Apple frame ring, the Android surface),
  `fynkel_ffi` (the C interface, with a render worker per view).
- `package:fynkel` (the Dart API, its build hook running cargo) and
  `package:fynkel_flutter` (`FynkelView`, the Swift and Kotlin plugins).
- `examples/hello` and its measuring tool, the iOS Simulator and Android
  emulator test runs (`tools/test-mobile.sh`), and CI.

## What was found on the way

These all come from measuring, not from the plan:
- Apple's Metal driver keeps a 128 MB pool while anything blits. The
  renderer blitted its per-frame data; it now writes it where the GPU
  reads it (hello: 415.7 → 271.7 MB of GPU memory).
- Hot restart leaked a render worker each time; the plugin is now reset
  once per isolate.
- Metal objects autoreleased on engine threads stayed until the thread
  ended; every pass now drains a pool.
- On Android: a surface closed under the engine panicked the render
  worker; Flutter 3.47's `resetInBackground` leaked 58 MB per trip to the
  background (a Flutter bug, avoided); the emulator's driver keeps 4 MB per
  thread, so render threads outlive views.
- The measuring itself was wrong three times, and was fixed each time: a
  covered window draws nothing; Auto-Lock ended unattended iPhone runs; and
  Android readings fell into the next cycle. hello's own once-a-second
  memory read also cost a frame on Android.

## Still open

From [stage-1.md §6](stage-1.md#6-open-questions):
- A surface Flutter closes under the engine freezes the view until the
  next resize (seen once on Android); `getForcedNewSurface` could recover
  at once.
- The Flutter leak could be reported to Flutter, with your yes.
- One shared wgpu device for all views would save a new view's start-up
  time and its first blit.
- Dart allocation per frame differs by platform (8 – 18 KB) and isn't
  looked into; all of it is below Flutter alone on Android.
- Android builds are 64-bit only.

## Seen on each device

Step 8 is done when you have seen the cube on each device yourself.
Confirmed on 2026-10-08:

- [x] Mac
- [x] iPhone 16 Pro Max
- [x] Galaxy Note10+

**Stage 1 is done.**
