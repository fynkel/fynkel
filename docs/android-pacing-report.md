# Android frame pacing: the report

[The plan](android-pacing.md), time-boxed to four days, done in one. All
the runs were on the one Android phone here, a Galaxy Note10+ (Android
12, Mali-G76, 60 Hz), 60 s each, under `caffeinate -du`. The logs are in
`~/fynkel-runs/pacing/`.

## The result

**Frames are much faster, but the goal isn't met.**

| Note10+ | before | now | goal |
|---|---|---|---|
| Sheepdog, low | 34–36 (min 27–28) | 51 (min 35–39) | 60 (min ≥ 50) |
| Sheepdog, medium | 13–26 | 40 | ≥ 55 |
| The look scene, medium, by day | 34.4 | 43–50 | ≥ 55 |
| The look scene, medium, by night | 27.2 | 42 | — |
| The look scene, low, by day | 59.5 | 58.3 | — |
| The GPU's clock while drawing | 200 MHz, at its minimum (156) a fifth of the time | 325 MHz at low, 572 at medium, at its minimum almost never | above its minimum |

The before row for Sheepdog was measured twice on this branch, before
the change. The before row for the look scene is stage 8's.

So, as the plan says for this case, **`Quality.device` now picks low on
Android phones** (Mali, Adreno and the rest), and the guide (§5) says
why. A game can still offer medium on its settings page, as Sheepdog
does.

## What changed

**Frames are drawn into an image of their own, and a second thread
presents them.** Before, the render thread drew each frame straight
into the window's image, waited for the GPU to finish it, presented it,
and only then started the next frame. Each frame's wait left the GPU
idle, and an idle GPU is what kept its governor at the lowest clock.

Now (`crates/fynkel_present/src/android.rs`):

- A frame is drawn into one of two images (slots) the size of the
  window, and its commands are submitted.
- A "fynkel present" thread takes it from there: it gets the window's
  image, copies the slot into it, waits for the GPU, and presents. The
  render thread has already started the next frame by then.
- The render thread waits only if both slots are taken.
- The surface handoff (`handoff.rs`) can now lend the surface to two
  frames at once, one being drawn and one being presented. A resize, or
  the app going to the background, still waits for both and lends to no
  new frame while it waits (a new test).

What it costs: two images the size of the view, about 19 MB of GPU
memory for a full-screen view on the Note10+ (1080×2241), and one copy
per frame, which the runs include.

**Measuring.** `fynkel_measure rate --android` now also prints the
GPU's clock and how busy it was, from `/sys/kernel/gpu/` (once a second,
through adb), and the render thread's busy share. The share comes from
a new count in the view's stats, `busyMicros` (ABI 29, `FyViewStats::busy_micros`,
`ViewStats.busy` in Dart). A phone without those files prints nothing
for them.

## The plan's steps

| Step | What happened | Kept |
|---|---|---|
| 1. Measure | The GPU's clock and busy share, Dart's ticks, the render thread's busy share | Yes |
| 2. Unmerge Flutter's threads | **Not possible.** Flutter 3.47 stops at start with `DisableMergedPlatformUIThread` "is disabled and no longer allowed". Dart runs on Android's main thread, and there's no way out. | — |
| 3. Let the GPU work ahead | The slots and the presenter thread above | Yes |
| 4. The performance hint API | **Not on this phone**: the Note10+ has no performance hint HAL, so no session can be opened. It wasn't built. | — |
| 5. Draw straight to the screen | Written up below, not built | — |

What else was tried on the way, and dropped:

| Tried | Sheepdog low / medium | Why dropped |
|---|---|---|
| The presenter thread, drawing into the window's image | crashed | wgpu lets only one window image be taken at a time |
| Three slots, not two | 50.9 / 39.6 | No better than two, and 10 MB more |
| The render thread waiting for the presenter before each submit, so a frame's copy is never queued behind the next frame | 36 / 12.6 | Worse: the render thread waits again, as before |
| The presenter submitting the frame's commands too, after presenting the one before | crashed | The renderer's per-frame buffer is mapped again before a frame submitted later is done |
| Presenting without waiting for the GPU first | 19.6 / 20.0 | Much worse: Flutter's wait for the frame falls on the main thread, where Dart runs |

## Why it's still short

A trace of Sheepdog at low with the slots (Perfetto, 10 s) shows where
the time goes now:

- Dart ticks about as often as frames are shown (51–53 a second), not
  60. Dart runs on Android's main thread, and that thread spends about
  42 % of its time blocked (`waitForever`), where Flutter waits for the
  frame we hand it.
- Flutter's own frames are often slow on the raster thread (252 of 3158
  over 16.7 ms at low, 437 of 2414 at medium). Flutter composites our
  image, and its small job waits on the GPU behind ours.
- The GPU is 80 % busy at 325 MHz (low) or 572 MHz (medium), so the
  governor now clocks it up, but not to its top (754 MHz).

So the frames still pass through Flutter, on one thread Dart shares,
and on one GPU queue. Pacing within that can't take Flutter's wait out
of Dart's frame. Step 5 can.

## Step 5: drawing straight to the screen (not built)

What it would be: on Android, the game's view becomes a `SurfaceView`
of its own, under or over Flutter's, and the render thread presents to
it directly. Android's compositor puts it on screen; Flutter never
waits for our frames, and Dart's frame no longer includes ours. That's
how game engines usually embed on Android.

What it takes:

- **The plugin**: a platform view (Flutter's `AndroidView`, in its
  hybrid composition or `SurfaceView` mode), with the surface handed to
  the engine as today's texture surface is. The engine side changes
  little: it already presents to an Android surface it's given.
- **Widgets over the game**: they must still draw over the view. Flutter
  supports that for platform views, at a cost per frame of its own, and
  the cost on this phone is what would have to be measured first.
- **The lifecycle**: the surface comes and goes with the view, as it
  already does with the texture.
- **Several views** (Sheepdog's minimap): each its own `SurfaceView`, or
  the small ones stay as today.
- **Tests**: the plugin's Android side, and the measure tool's runs.

It's a stage-sized piece of work, and its outcome on this phone is not
known in advance: platform views have costs of their own in Flutter.
The first day of it would be a spike that measures one `SurfaceView`
under a transparent Flutter view in Sheepdog. **It isn't built until
you decide.**

## The Mac and the iPhone

The change is Android's only (`android.rs`, and the handoff it alone
uses); the busy count runs on every platform and costs one clock read
per frame. Re-measured once at the end, Sheepdog at high, 60 s: the Mac
120 (slowest second 120), the iPhone 120 (slowest second 75, as in stage
8's 76). Unchanged.

## Tests

- New: the time spent drawing is counted (`fynkel_ffi`), and the
  surface handoff waits for every frame and lends no more meanwhile
  (`fynkel_present`). The device's quality test now wants low on
  Android.
- The phone tests (`tools/test-mobile.sh android`, on the Note10+): the
  presenting test now waits for the presenter to catch up, a frame or
  two behind. It passes. Eight golden-image tests of textured models
  and materials (`fynkel_render:textures`, `fynkel_ffi:models`) fail on
  this phone, and fail the same with this work's quality change taken
  out. They aren't from this work. Why the phone's textures differ from
  the goldens isn't known yet; they need a look of their own.

## Not covered

- Other Android phones: only the Note10+ is here. A newer phone may hold
  60 at medium now; `Quality.device` picks low for all of them, which a
  game can override.
