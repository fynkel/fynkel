# Android frame pacing

Status: proposed 2026-10-16. Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). This runs after stage
8 and before stage 9 (animation). It is time-boxed: **four working
days**, then a decision either way.

## Why

Stage 8 left one shortfall (docs/stage-8-report.md, "The Note10+"). The
Mac and the iPhone hold 120 fps at every level. The Galaxy Note10+
(Android 12, Mali-G76, 60 Hz) gives:

| Note10+ | low | medium | high |
|---|---|---|---|
| Sheepdog (two views, eight lamps) | 19–43 | 22 | 19 |
| The lab's look scene, by day | 59.5 | 34.4 | 27.9 |

Sheepdog held 60 in stage 7, before the look, and it falls short even at
low, with no bloom and no AO.

Phase 6 found the cause, and it isn't one effect being too slow:

- **The GPU stays at its lowest clock**: 156 of 754 MHz, 72–84 % busy.
  Its governor (`Interactive`) raises the clock only when the GPU is
  busier than that.
- **It isn't busier because our frames and Flutter's run one after
  another.** Flutter takes each of our frames on the main thread, the
  same thread Dart runs on (stage 6 scale §2), so Dart ticks only about
  37 times a second. Our render thread then idles about 20 ms of each
  27, waiting for the next scene. The GPU has gaps, the governor sees
  "not busy enough", and the clock stays low.
- At 156 MHz, each pass has a large fixed cost: bloom about 9 ms a
  frame, medium's last pass 3–4 ms.

So the work is in how frames are paced on Android, not in the renderer.

## The goal

On the Note10+, 60 s runs, by day and by night:

- **Sheepdog at low holds 60** (min ≥ 50), as it did in stage 7.
- **Sheepdog at medium ≥ 55**, and the look scene at medium ≥ 55.
- The GPU clock is seen to rise above its minimum while drawing.
- Nothing changes on the Mac or the iPhone (re-measured once at the end).

If that is met, medium stays `Quality.device`'s pick for Mali and Adreno.
If it isn't met at the end of the time box, `Quality.device` picks low
for them, the guide says why, and what was learned is written down for
later.

## Rules

- **Every experiment is a 60 s run on the Note10+**, under
  `caffeinate -du`, logs in `~/fynkel-runs/`. Each run reports fps (min),
  Dart's ticks a second, frames presented, and the GPU's clock and busy
  share over the run (step 1). A suspicious run is repeated: this phone
  has given 19 and 43 for the same build.
- **Only what helps is kept.** Each experiment is its own commit or is
  thrown away, and the table below says which.
- **No platform other than Android changes**, unless step 5 says so.
- Tests where code is kept: the Rust tests run on the Mac (the Android
  target has tests that make real surfaces through the NDK, run on the
  phone); `flutter test` in every touched package.
- One PR at the end, with a short report (`docs/android-pacing-report.md`).

## Steps

### 1. Measure what we're fixing (½ day)

- `fynkel_measure rate --android` samples `/sys/kernel/gpu/gpu_clock`
  and `gpu_busy` once a second through adb, and prints their mean and
  spread with the result. A phone without those files prints nothing
  for them.
- It already prints Dart's ticks a second; add the render thread's busy
  share (time drawing ÷ wall time), from the worker's stats.
- Baseline: Sheepdog low and medium, and the look scene medium, each
  run twice.

### 2. Unmerge Flutter's threads (½ day)

Flutter 3.47 runs Dart on Android's main (platform) thread, where it
also takes our frames and waits for them. Flutter can still run Dart on
a thread of its own (the `DisableMergedPlatformUIThread` manifest
setting). If Dart no longer shares a thread with the frame handover, it
can tick at 60 while the GPU works, which keeps the GPU fed.

- Try it in Sheepdog's manifest. If it helps, find out whether the
  plugin can ask for it or whether each game's manifest needs it, and
  put it where a new game gets it (the guide's steps for a new game,
  §1).
- Check that it doesn't break input, the lifecycle (background and
  back), or hot restart.

### 3. Let the GPU work ahead (1 day)

Today the render thread waits for each frame's GPU work before handing
it over, and draws only one frame at a time. Once Dart isn't waiting
behind the handover (step 2), that wait may no longer be needed:

- Hand the frame over without waiting, and allow two frames in flight
  (`desired_maximum_frame_latency`, and enough images in the producer).
  Phase 6 tried dropping the wait alone, on merged threads; this tries
  it on unmerged ones.
- Start drawing the next scene as soon as it's posted, rather than at
  the next tick, so the GPU has work queued while Flutter composites.

### 4. Tell Android what the frame needs (1 day)

Android 12 (API 31, the Note10+'s level) has a performance hint API:
the render thread says how long its frames should take (16.6 ms) and how
long they did, and the system raises clocks when they run long.

- Open a hint session for the render thread (and Dart's, if step 2 gives
  it a thread of its own), report each frame's time, loaded with `dlsym`
  so older Androids just skip it.
- It's known to raise CPU clocks; whether it moves this GPU's clock is
  what the run finds out.

### 5. If all of that falls short: draw straight to the screen (decide, don't build)

The largest change: our frames go to an Android `SurfaceView` behind a
transparent Flutter view, so the OS composites them and Flutter never
waits on them. That is how game engines usually embed, and how the
iPhone already works in spirit (IOSurfaces in a ring). It changes the
plugin, the lifecycle and how widgets layer over the game, so it's a
stage-sized piece of work. This step only writes down what it would
take, measured against what steps 2–4 got, and asks before any of it
is built.

### 6. Decide and report (½ day)

- Keep what helped; re-run the goal's table, and the Mac and the iPhone
  once.
- `Quality.device` for Mali and Adreno: medium if the goal is met, low
  if it isn't.
- The report, the guide (if a game's manifest or settings change), and
  the stage 8 report's Note10+ section pointed at it.

## Not in this

- Making passes cheaper on the GPU (bloom, the last pass). That would
  help at any clock, but it's renderer work, and the clock is the bigger
  lever: 156 → 754 MHz is nearly five times.
- Other Android phones. Only the Note10+ is here; the report says so.
- Frame pacing on the Mac's dense scenes (the 120 fps stream's open item).
