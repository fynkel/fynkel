# F8 — offscreen render capacity on the Mac (2026-10-05)

Machine: MacBook Pro M3 Max, 120 Hz panel. Tool:
`cargo run --release -p fynkel_host_headless --example render_throughput -- CACHE [--scene UUID] [--size WxH] [--quality NAME] [--frames N] [--live]`.
Frames are submitted back to back, three in flight, offscreen: no window, no
vsync, no readback. `--live` runs the 60 Hz sim on every other frame and
re-extracts every frame. GPU numbers are Metal timestamps (p50, spans overlap).

| scene | size | mode | fps | CPU (ms) | GPU frame (ms) |
| --- | --- | --- | --- | --- | --- |
| reference game | 1600x1200 | live | 599 | prepare 0.10, encode 0.32 | 1.64 (AO 0.98) |
| reference game | 3456x2234 (native) | live | 141 | prepare 0.13, encode 0.34 | 7.07 (AO 4.76) |
| reference game | 3456x2234 | low profile | 354 | | 2.86 |
| stress (~5–7k instances) | 1600x1200 | static | 431 | prepare 1.95 | 1.85 |
| stress | 1600x1200 | live | 174 | sim 6.05 (tick frames), extract 0.62, prepare 1.66 | 2.98 |
| stress | 3456x2234 | live | 167 | sim 5.81, extract 0.64, prepare 1.70 | 5.93 |

## What the numbers say

* The first table is the state *before* the changes listed below. Rendering
  itself cleared 120 fps everywhere, including native retina resolution (141
  fps reference game, 167 fps stress) — but at native size the GPU was the
  limit for light scenes: ~7 ms/frame, of which the screen-space AO + blur was
  ~4.8 ms (estimate 3.5, blur 1.3, depth copy 0.2). The `low` profile (no
  AO/shadows) was 2.9 ms.
* For the heavy scene the limit is CPU on the single thread: a 60 Hz tick
  frame costs sim 6.0 + extract 0.6 + prepare 1.7 + encode 0.4 ≈ 8.7 ms, over
  the 8.33 ms budget before any Dart work. Per tick (`systems ms/tick`):
  physics 3.7 (sync 1.4, propagation 0.8, rapier 0.8), animation 0.9, three
  mover systems 1.5 together. Everything scales with the number of *moving*
  entities (≈1.3 µs each per tick through system → journal → commit →
  propagate → physics sync, plus ≈0.5 µs per frame through extract and
  prepare); no single hot spot is left to remove, so dense-5000 needs
  batching of that path or a worker thread, not another micro-fix.
* In the Flutter app the fresh-frame rate follows GPU time: dense-2000 with AO
  off reaches 111.5 fresh frames/s against 92.5 with it on (acquire-wait and
  buffer count made no difference), and at half resolution 114.

## Changes made in this pass

1. `FastHasher`/`FastMap` (fynkel_contracts) replace SipHash in the render
   and ECS change-tracking hot maps.
2. `World::resolve_extract_view`: one lookup per entity in `extract_into`,
   no lerp when the pose did not move.
3. IOSurface ring mailbox: a finished frame nobody has shown is recycled for
   the new one instead of dropping the new frame (`ring.rs`, tests updated).
4. `prepare` resolves each `(mesh, material)` pair once per call instead of per
   instance (no per-instance `AssetRef` clones, hashes or batch-key lookups):
   stress instance construction 0.80 → 0.37 ms, prepare 1.70 → 1.21 ms.
   Light-overlap queries compare squared distances (no square root).
5. **AO at half resolution.** The estimate pass draws at half the internal
   size and the compute blur upsamples it depth-aware (2×2 taps judged by the
   full-size depth each stands for). Reference game at 3456×2234: AO stage 4.85
   → 1.80 ms (estimate 3.48 → 1.19, blur 1.30 → 0.56), GPU frame 7.1 → 4.1 ms,
   141 → 242 fps. A 1920×1080 frame differs from the full-size estimate by
   0.04/255 on average (p99.9 = 2). The blur's scalar-reference test was
   rewritten for half-size input; the shading tests pass unchanged.
6. `FYNKEL_PRESENT` and `FYNKEL_SYSTEMS` diagnostics in the standalone host,
   and the `render_throughput` example (this note's tool; `--live` adds the
   per-system tick times and the prepare phases).

## In the Flutter app (macOS, foreground launch, release/profile, 1600×1200)

| scene | loop fps | fresh frames/s | tick-frame work |
| --- | --- | --- | --- |
| dense-500 | 120.0 | 119 (was ~94 % before the ring mailbox) | 2.7 ms |
| dense-2000 | 120.0 | 92.5 → 104 with half-res AO | 5.2 ms |
| dense-5000 | 88 (before) / 78–88 (runs vary) | 60–66 | 11.5–12.6 ms |
| reference game (window 1600×1000) | 120 | 120 (0 dropped) | 1.3 ms |

dense-5000 is CPU-bound on the UI thread (native 5.9–6.5 ms + Dart behaviours
2.4–2.7 ms + extract/prepare 2.7–2.9 ms per tick frame) and still misses 120.

## Pipelined viewport and the frame queue (the 60 fresh-frames/s cap)

Two changes in `platform/apple` (ABI 15), both on by default for the viewport
that `FynkelGameView` owns (`pipelined: true, frameQueue: 2, bufferCount: 5`):

1. `fynkel_apple_viewport_set_pipelined`: admission, `prepare`, encode and
   submit run on a render worker while the host simulates the next frame; the
   host thread still extracts, and `render` answers the previous job. Every
   other renderer-touching call waits for the job in flight first.
2. `fynkel_apple_viewport_set_frame_queue(depth)`: with depth 2 the presenter
   shows two frames that finished between rasters one after the other instead
   of dropping the older (`acquire_presented` takes the oldest of ≤ depth
   waiting frames; more than that and it jumps to the newest). When another
   frame is queued behind the one returned, bit 8 of the answer is set and the
   Swift texture signals again, because the engine copies the texture once per
   signal-then-paint.

**What capped dense-5000 at 60 fresh frames/s.** The Flutter timeline of a
pipelined run (`timeline` capture) shows the loop at 120 Hz, but every cycle is
one long tick frame (build 9.3–10.5 ms, over the 8.33 ms vsync) followed at
once by a short one (2.4 ms): the two rasters land ~2 ms apart, then nothing for
~14 ms. A frame needs prepare (≈ 2 ms) + GPU (≈ 4 ms) ≈ 6 ms after it is
handed over, so neither raster of its own cycle finds it done and both frames
of a cycle become ready before the next cycle's first raster. With four
buffers (one Presented, one Retiring) the second admission then recycled the
first, unseen, frame — the tick frame's slot was always the same slot — so the
compositor copied 60 of 120 ready frames (`texture_ready=120`,
`texture_copies=60`, `copyQueued=0`). Raising the queue depth alone did
nothing for the same reason; it needs the ring to keep the second frame:
Presented + Retiring + two queued + one rendering = **5 buffers**.

**Measured** (Mac M3 Max, 1600×1200, release builds, foreground captures with
frames, `ignoreInactive`; 6 one-second windows each):

| scene | before (sync, mailbox, 4 buffers) | pipelined + queue 2 + 5 buffers |
| --- | --- | --- |
| dense-500 | loop 120, fresh 110 | loop 120, **fresh 120** |
| dense-2000 | loop 120, fresh 104 | loop 120, **fresh 120** |
| dense-5000 | loop 80.5, fresh 64.5 | loop 120, **fresh 120** (2 of 2 with 5 or 8 buffers; `texture_more=60`) |
| reference game (profile, 1200×820 window) | 120 / 120 | 120 / 120, 0 dropped |

Pipelined alone (4 buffers, no queue): dense-5000 loop 120, fresh 60;
dense-2000 loop 107–120, fresh 75–86 — worse than the synchronous 104, which is
why the queue and the fifth buffer ship together.

Costs: one more frame of latency (`present_lag=1`), one more IOSurface per
viewport, and a tick frame at dense-5000 that is still 8.6–9.5 ms of UI-thread
work (native 6 ms, Dart behaviours 2.7 ms, extract 0.8 ms): the loop holds
120 because the short frame after it absorbs the overrun, so there is no
headroom left at 5000 moving entities. A second capture session was needed
because a full-screen app on the Mac occludes the benchmark window and
produces all-zero windows (discarded).

## Full screen (3456×2096 pixels, the Mac's whole screen minus the menu bar)

The benchmark's window was resized to 1728×1080 points before each capture
(`WIN=1728x1080 /tmp/runapp.py`, the window is clamped to the screen). Same
build, six 1-second windows, foreground:

| scene | sync, mailbox, 4 buffers | pipelined + queue 2 + 5 buffers | GPU frame |
| --- | --- | --- | --- |
| dense-500 | loop 120, fresh 106 | loop 120, **fresh 120** | 4.9 ms |
| dense-2000 | loop 120, fresh 89.5 | loop 120, **fresh 120** | 3.9–4.1 ms |
| dense-5000 | loop 80.5, fresh 60 | loop 120.3, **fresh 120.3** | 4.1 ms |

This is a large window, not macOS's full-screen mode (which may composite
differently).

**Is 120 a cap?** In the app, yes: the game loop is driven by Flutter's
vsync ticker, so it runs at the display's refresh rate (120 Hz here; a 240 Hz
monitor would be driven at 240). The engine itself is faster: the offscreen
benchmark (`render_throughput`, pipelined, not tied to a display) at
3456×2234 runs 183 fps (stress scene), 167 fps (dense-5000, with its 6 ms
simulation) and 243 fps (reference game).

## iPhone 16 Pro Max (120 Hz ProMotion, 1320×2868), 2026-10-06

dense-500 release build on the device, full effects (AO, contact shadows,
MSAA 4), nominal thermal state, a cooled phone, six 1-second windows
(`benchmarks/mobile/capture.py`).

| configuration | loop | fresh frames/s | dropped/s | GPU frame |
| --- | --- | --- | --- | --- |
| old: sync, mailbox, 4 buffers, native | 120 | 38–43 | 30 | ~10.7 ms |
| pipelined + queue 2 + 5 buffers, native, **before** the in-flight cap | 120 | 29–35 | 19–23 | ~11.5 ms |
| same, **with the in-flight cap**, native | 120 | **89–90** | 30 | 9.6–11 ms |
| same, `resolutionScale: 0.75` (990×2151) | 120 | **120** | 0 | 5.6–6.9 ms |

What was wrong: at native resolution the GPU needs ~10 ms a frame, so it
cannot make 120 fps. The render worker kept starting frames anyway; the ring
filled with finished frames the presenter never saw, and recycled them, so
the GPU spent its time on pictures nobody displayed (rendered 98, shown 29,
repeats 65 in a second). `set_frame_queue(depth > 1)` now also allows at most
two frames in flight and drops a third before any work is done for it, like
the full-ring drop. The GPU then renders only what can be shown: 90 fresh
frames/s at native resolution, 3× the previous default and more than the
synchronous path, with no throttling in these captures.

The honest limit: at native resolution with every effect on, this scene takes
~10 ms on the phone's GPU, so 120 is out of reach (≈100 is the ceiling). Two
ways to 120 are legitimate: render at a fraction of native resolution
(`FynkelGameView(resolutionScale: 0.75)`, the new option, which holds 120 with
0 dropped here), or make the effects cheaper. The option is explicit and off
by default; nothing is lowered silently.

### What each effect costs on the iPhone (native 1320×2868, dense-500)

One quality profile at a time, otherwise identical (live scene, pipelined +
queue 2 + 5 buffers, nominal thermal state, 90 s between captures; the
profiles are `prepare.py`'s diagnostic copies of `medium`, selected with
`capture.py --quality`). Loop was 120 throughout.

| profile (what is off) | fresh frames/s | dropped/s | GPU frame p50 |
| --- | --- | --- | --- |
| medium (nothing) | 90 | 30 | 9.8 ms |
| no contact shadows | 90 | 30 | 9.4 ms |
| no AO | 99 | 20 | 9.1 ms |
| no bloom | 100 | 19 | 9.1 ms |
| MSAA 2 instead of 4 | 95 | 24 | 9.1 ms |
| MSAA off (1) | 100 | 20 | 9.1 ms |
| **no AO and no contact shadows** | **120** | 0 | 5.4 ms |
| no AO, no contact shadows, no bloom, MSAA off | 120 | 0 | 4.6 ms |

Turning off AO *or* contact shadows alone saves 0.4–0.7 ms, but turning off
both saves 4.4 ms. The two share the screen-space pipeline (depth prepass, MSAA
depth resolve, estimation and the depth-aware blur), and that fixed cost only
disappears when neither is on. Practical consequences: dropping one of them is
nearly useless for performance; dropping both is what reaches 120 at native
resolution on this scene. Bloom and MSAA each cost about 0.7 ms.

Not yet measured on the phone: dense-5000 (its 8+ ms tick frame plus the GPU
put it near 60 and the phone throttles after ~2 s), and sustained runs longer
than the 6-second capture.

### Texture compression on the iPhone (ADR-0061), 2026-10-06

The dense-500 app with `--textures`: every one of its 8 materials has a
1024² colour, normal and metallic-roughness map (24 textures). Built twice,
the maps cooked uncompressed (`none`) or with the ADR-0061 defaults (ASTC 6×6
colour, 4×4 data); same scene, same native resolution, pipelined + queue 2 +
5 buffers, nominal thermal state, captures alternated with 90 s between.

| | uncompressed | ASTC | |
| --- | --- | --- | --- |
| cooked textures (`fynkel_cache`) | 130 MB | 28 MB | 4.6× smaller |
| app (`Runner.app`) | 163 MB | 56 MB | 2.9× smaller |
| medium (every effect): fresh frames/s | 89–92 | 89–90 | same |
| medium: GPU frame p50 | 10.2–11.9 ms | 10.9–12.2 ms | same (noise) |
| no AO, no contact shadows: fresh frames/s | 120 | 120 | same |
| no AO, no contact shadows: GPU frame p50 | 6.30–6.54 ms | 6.27–6.54 ms | same |

So on this scene compression buys memory, disk and download size, not frame
time: the GPU's time is the screen-space passes and the resolve, not texture
reads (the textured surfaces are small on screen and sample low mips). Where
compression should show in frame time is a scene whose screen is covered by
close, high-resolution textures; none of ours is that yet. The ADR's claim
stands as written ("to be measured, not assumed"): measured, no change here.

## Next

1. (Done on the desktop stress scene 2026-10-06, `6a58294f`: movers,
   propagation and the physics sync batched, tick p50 5.38 → 3.83 ms; the
   phone's dense-5000 not yet re-measured.) dense-5000 tick frame (8.6–9.5 ms): batch the movers' path (system →
   journal → commit → propagate → physics sync, ≈ 1.3 µs per moving entity),
   trim `Physics::sync` (1.0 ms, a `BTreeSet` of 8.9 k candidates) and
   `propagate` (0.7 ms), and parallelise `extract_into`. Native profile of one
   tick: physics 2.4 ms of 5.9, movers + commit 1.9 ms, animation 0.8 ms.
2. Latency: `present_lag=1`; a game that wants the lowest latency can pass
   `FynkelViewportController(pipelined: false)` and the default mailbox.
