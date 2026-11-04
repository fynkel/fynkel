# Stage 16 report: speed

[The stage 16 plan](stage-16.md), [its start line](stage-16-start.md).
Phases 1 to 4 are merged (#193 to #196), with #197 and #198 from checking
them. The report and the budgets are this PR.

## The result

The goal was Unity's frame on every device: the Sheepdog field as fast as
Unity draws it, or the gap measured, small and explained. It's met on the
iPhone at every level and on the Note10+ at low and medium. The Note10+ at
high and the iPad at every level are still short, and the gap is measured
but only partly explained.

What a player sees has changed most in the ten-minute runs:
- **The iPad** plays Sheepdog at 42 frames a second, not 11.
- **The iPhone** holds 60 for ten minutes, where it fell to about 45.
- **The Note10+** holds 52 to 60, where it fell to 48.

| Phase | What | PRs |
|---|---|---|
| 1 | Every number measured by cable on three devices, with Unity beside it | #193 |
| 2 | `renderer.rs` split into passes, every golden the same | #194 |
| 3 | Older Apple GPUs pick a lower level; a cheaper finish, bloom and ambient occlusion; a quicker shadow filter on Apple's GPUs | #195, #198 |
| 4 | A tree over the objects for casts, world matrices, the mesh change, nine small rows | #196 |
| 5 | The pile's garbage, the budgets, the guide's §20, this report | #197, this PR |

## The end line

Profile builds, by cable. The start is 2026-11-01 and 02; the end is
2026-11-03 and 04, on main at d70b9b14 or later. The fix in #198 changes
nothing on Apple's GPUs, so the iPad's and the iPhone's runs from before it
stand. The Note10+'s are all from after it, each with its GPU's clock and
the app on screen logged. The start's Note10+ rows are medians of three
runs; the end's field agrees with the two runs of the fix, and its crowd is
one run.

### The field

Frames a second, Fynkel at the start → the end, then Unity.

| | low | medium | high |
|---|---|---|---|
| Note10+ | 59.4 → **59.7** (59.5) | 45.1 → **47.7** (52.1) | 33.1 → **34.3** (53.3) |
| iPad | 40.0 → **40.0** (59.4) | 30.3 → **33.5** (39.7) | 19.0 → **22.5** (28.5) |
| iPhone | 60 → **59.8** (60) | 60 → **60** (60) | 60 → **60** (60) |

Against the bar, within 10% of Unity:

| | low | medium | high |
|---|---|---|---|
| Note10+ | met | met (−8%) | **missed (−36%)** |
| iPad | **missed (−33%)** | **missed (−16%)** | **missed (−21%)** |
| iPhone | met | met | met |

The iPad's GPU, in ms a frame, start → end: low 16.3 → 16.2, medium 32.1 →
27.5, high 52.8 → 44.6. Unity's: 14.7, 36.4, 58.1. So at medium and high
its GPU now takes less time than Unity's, and it still draws fewer frames.
The start line says why: Unity's CPU and GPU overlap where Fynkel's wait.
That part wasn't worked on.

### The crowd

100 characters, frames a second, as above.

| | low | medium | high |
|---|---|---|---|
| Note10+ | 58.9 → **57.9** (57.3) | 40.9 → **40.9** (59.8) | 31.9 → **31.9** (39.6) |
| iPad | 40 → **40** (30.1) | 31 → **31** (28.8) | 22 → **24** (23.5) |
| iPhone | 60 → **60** (60) | 60 → **60** (60) | 60 → **60** (60) |

The stage's GPU work did little for it. Animation is still cheaper in
Fynkel, and that's why the iPad is ahead. The Note10+ is well behind at
medium (−32%) and high (−19%).

### The pile

1,000 tumbling cubes.

| | start | end | Unity |
|---|---|---|---|
| Note10+, frames a second | 51.9 | **52** | 59–60 |
| Note10+, Dart garbage a frame | 293 KB | **161 KB** (207 with Rigidbodies) | |
| Mac, Dart garbage a frame | 297 KB (596 with Rigidbodies) | **143 KB** (165) | |

Half the garbage did nothing for the frame rate. The start line blamed the
garbage for the gap to Unity; that was wrong. The step wasn't measured
again.

### Ten minutes

Sheepdog at each device's own level, frames a second by minute.

| | start | end | bar: the end within 10% of the start |
|---|---|---|---|
| Note10+ | 59, 60, 52, 53, 49, 49, 49, 49, 49, 48 | 58, 60, 60, 60, 52, 52, 60, 57, 60, 52 | **just missed (−10.3%)**, was −19% |
| iPad | 11 every minute | 42 every minute, 41 the last | met |
| iPhone | 60, 47, 47, 47, 47, 45, 47, 46, 41, 45 | 60, 60, 55, 59, 60, 60, 60, 60, 60, 60 | met; thermal state "serious" at 558 s |

### The bars owed

| | start | end | bar |
|---|---|---|---|
| 100 tags among 3,000, Note10+ | 1.93 ms | **0.72 ms** | under 1: met |
| The same, iPad | 3.05 ms | **0.42 ms** | |
| The same, iPhone | 0.70 ms | **0.27 ms** | |
| A mesh changed every frame, Note10+ | 1.4 ms | **0.78 ms** | under 1: met |
| Sheepdog at high and 120, iPhone | 75 | **63, not comparable** | 120: **missed** |

The mesh's normals moved to the render thread (2.5 → 3.9 ms there), off the
frame's critical path at 60. The 63 was run warm, straight after the ten
minutes ("serious" at 12 s). It was to be run again from cold, but the
iPhone was no longer connected.

### Nothing lost

No golden file changed in the stage. CI's benchmarks
and soak are green on main. One soak on #197 failed, with about 190 MB more
outside Dart and malloc than usual, and the same again on its green rerun;
main's run after the merge read as usual. The Mac's state is the likely
cause, not proven.

## The budgets

What each part of a frame may take at 60 fps, on the level each device
picks, in the lab's busiest scene, 'All of it together'. Medians in ms over
15 s. The iPad's high is shown beside its pick, as stage 14's table had it.

| Part | Budget | Note10+ (low) | iPad (low) | iPad (high) | iPhone (high) | Mac (high) |
|---|---|---|---|---|---|---|
| The game's code | 3 | 1.50 | 0.59 | 2.30 | 0.74 | 0.19 |
| Animators | 1.5 | 1.13 | 0.35 | 1.38 | 0.45 | 0.09 |
| Meshes sent | 1 | 0.18 | 0.14 | 0.31 | 0.07 | 0.02 |
| Physics | 1 | 0.13 | 0.05 | 0.10 | 0.08 | 0.02 |
| Particles | 0.5 | 0.05 | 0.02 | 0.07 | 0.02 | 0.01 |
| Sound | 0.25 | 0.04 | 0.02 | 0.03 | 0.01 | 0.01 |
| The world's own | 0.25 | 0.02 | 0.01 | 0.01 | 0.01 | 0.00 |
| `world.update` | 7 | 3.17 | 1.34 | 4.46 | 1.43 | 0.34 |
| Flutter's UI thread | 10 | 4.24 | 1.71 | 5.48 | 1.24 | 0.34 |
| The render thread | 14 | 4.40 | 33.52 ✗ | 33.33 ✗ | 2.99 | 0.91 |
| Frames a second | 60 | 59.9 | 39.3 ✗ | 29.9 ✗ | 60.0 | 60.0 |

- **The Note10+, the iPhone and the Mac** are within every budget at the
  level they pick.
- **The iPad** is within every budget but the render thread, whose time
  includes waiting for the GPU. An A10 can't draw this scene at 60 at any
  level. Its meshes at high were over budget in stage 14 (1.81 ms).
- **The Note10+'s other levels**: medium 53.5 fps, high 46.3. Its render
  thread at medium reads about 4 ms in some runs and about 18 in others.

## Found on the way

- **Phase 3 made the Note10+ a third slower, and it merged.** I added
  gathers to high's shadow filter for the A10 and measured them on the iPad
  only. A shader holding them took the Note10+ from 46.5 to 30.1 fps at
  medium, a level that never runs them. The budgets found it. Building the
  stage at each point and alternating before and after on the phone traced
  it to the one commit. Fixed in #198: only Apple's GPUs have them. A change
  made for one GPU now gets run on the others before it's merged.
- **The memory tests didn't see the pile's garbage.** The compiled app
  keeps objects that the tests' JIT optimizes away. #197 has the tests for
  it, and the numbers above are from profile builds.
- **`measure bench` had hung on every device since stage 6.** Fixed, with a
  test.

## Left open

- **The Note10+ at high** (−36%) and **the iPad at every level** against
  Unity's field. On the iPad the GPU's frame is no longer the gap at medium
  and high: the CPU and GPU waiting on each other is what to look at. At
  low it's the scene pass, about 11 ms of it the ground and the sky.
- **The crowd on the Note10+** at medium and high.
- **Sheepdog at high and 120 on the iPhone**: missed, and owed a run from
  cold.
- **The Note10+'s GPU clock** was held at 377 MHz of 754 throughout one run,
  with half the frames, the phone cool and not in power saving. Why isn't
  known. Runs on this phone need their clock logged.
- **Flutter's raster thread on the Note10+** reads 15 ms in the busiest
  scene, against about 1 or under on Apple's devices. It read 10 to 16 at
  the stage's start too. Not looked into.
- **The pile's step**, 17% to 30% slower than stage 5's (the start line),
  was never explained, and the gap to Unity's frames isn't the garbage.
- **Memory over ten minutes.** The Note10+ read 188 → 225 MB, but its first
  reading moves a lot between runs (170, 188, 205 MB) and its last much less
  (202 at the start of the stage; 209 and 225 now), so most of that climb is
  the app warming up. Followed for 22 minutes (the phone then dropped off
  USB): graphics and native memory are flat, and only what Android calls
  "Unknown" grows, 34 → 46 MB, levelling off after about 18 minutes (46.1,
  46.2, 46.2). No leak on the Note10+ in that time. What "Unknown" holds
  isn't confirmed, and the run didn't reach the 30 minutes planned. The iPad
  (182 → 209 at low, 264 → 425 at high) wasn't followed this way.
- **BR6 and RN9** were each half done, as phase 4's table says. The rest is
  still open.

## Next

As agreed: each system made deeper and steadier, judged against Unity's same
system and against what the games use. New things come when a game asks.
