# Stage 16's start line

Phase 1 of [the speed plan](stage-16.md): every number the plan holds
Fynkel to, measured before any fix, on 2026-11-01 and 2026-11-02. Phase
5's report puts the end line beside it.

The devices, all by cable this time: a Galaxy Note10+ (Mali-G76, 60
Hz), an iPad (6th generation, A10, 2018) and an iPhone 16 Pro Max (A18
Pro, 120 Hz, here held to 60 unless said). Profile builds. Unity is
6000.3.9f1 with URP, built from `tools/unity_field` and
`tools/unity_tumble` to match Fynkel's scenes and levels.

## The field

Sheepdog's field (`examples/sheepdog/tool/field.dart`, `--define
FIELD=true`), each level 20 s, the first 5 to settle. Frames a second;
Fynkel's Note10+ row is the median of three runs.

| | low | medium | high |
|---|---|---|---|
| Note10+, Fynkel | 59.4 | 45.1 | 33.1 |
| Note10+, Unity (stage 14) | 59.5 | 52.1 | 53.3 |
| iPad, Fynkel | 40.0 | 30.3 | 19.0 |
| iPad, Unity | 59.4 | 39.7 | 28.5 |
| iPhone, Fynkel | 60 | 60 | 60 |
| iPhone, Unity | 60 | 60 | 60 |

Against the bar (within 10% of Unity): the Note10+ meets it at low and
misses at medium (−13%) and high (−38%); the iPad misses at every level
(−33%, −24%, −33%).

The GPU's time a frame, in ms (`FIELD_GPU=true`; Unity's from its
`FrameTimingManager`):

| | low | medium | high |
|---|---|---|---|
| iPad, Fynkel | 16.3 | 32.1 | 52.8 |
| iPad, Unity | 14.7 | 36.4 | 58.1 |
| iPhone, Fynkel | 2.1 | 11.7 | 12.5 |
| iPhone, Unity | 4.7 | 7.0 | 9.0 |

- **The iPad at low** draws at 40, not 60, because its GPU frame (16.3
  ms) is just over what 60 allows once the frame's own overheads are
  added: frames miss every other refresh. Unity's is 14.7 and holds 59.4.
  Of Fynkel's 16.3, the scene pass is 14.8 and shadows 1.5.
- **The iPad at medium**: scene 20.5, the finish 5.9, shadows 3.6, bloom
  2.1. **At high**: scene 29.9, ambient occlusion 10.2, the finish 5.6,
  shadows 5.0, bloom 2.1. Unity's GPU frame is longer at both, yet it
  draws more frames: its CPU and GPU overlap where Fynkel's wait.
- **Anti-aliasing** costs nothing on the A10 now (16.3 with 4x, 16.3
  with 2x, 16.4 without).
- **The iPhone's** per-pass times are rough: its GPU reports the passes
  of a frame as one, so the split moves between them (low's total read
  2.1, 3.1 and 5.9 across three runs of the same frame). Its totals at
  medium and high are steadier and above Unity's.

## The crowd

The lab's crowd (`examples/lab/tool/measured.dart`, `CROWD=100`): 100
KayKit characters, each on its own clip, a sun with shadows. Unity's
(`Crowd.cs`) is the same scene with a different mix of the same clips.
Frames a second; the Note10+ the median of three runs (Unity's of four).

| | low | medium | high |
|---|---|---|---|
| Note10+, Fynkel | 58.9 | 40.9 | 31.9 |
| Note10+, Unity | 57.3 | 59.8 | 39.6 |
| iPad, Fynkel | 40 | 31 | 22 |
| iPad, Unity | 30.1 | 28.8 | 23.5 |
| iPhone, Fynkel | 60 | 60 | 60 |
| iPhone, Unity | 60 | 60 | 60 |

- **Animation is cheaper in Fynkel**: 100 characters take 5.8–6.5 ms a
  frame on the Note10+, 8 on the iPad and 2.8 on the iPhone. Unity's
  main thread is 12.7–17.5 ms on the Note10+ and 18–23 on the iPad.
  That's why the iPad's crowd is faster in Fynkel at low and medium.
- **Drawing is dearer**: the Note10+ at medium and high, the same GPU
  story as the field's.

## The pile

1,000 cubes of 0.5 m falling, tumbling and settling, dropped again
every 8 s, fixed steps of 1/50 s (`packages/fynkel_physics/example`,
`tools/unity_tumble`).

| | Fynkel | Unity |
|---|---|---|
| Note10+, frames a second | 51.9 | 59–60 |
| Note10+, a step (median) | 4.61 ms | 10.8 ms |
| iPhone, a step (median) | 1.12 ms | 4.50 ms (stage 5) |

Fynkel's step is less than half Unity's on both phones, but slower
than stage 5's (3.94 ms on the Note10+, 0.86 on the iPhone): 17% and
30% more, to find in phase 4. The frames a second are lower than
Unity's on the Note10+ though the step is cheaper: the scene
allocates 293 KB of Dart garbage a frame on the Note10+, and 274 of its
3,128 frames in that run were slow to build.

`measure bench` had hung on every device since stage 6: its ray scene
built its chains with `parent =`, which keeps the world position, so its
rays missed and it threw. Fixed (`fynkel_measure`, with a test that runs
the whole bench on the Mac).

## Ten minutes

Sheepdog at each device's own level, 600 s (`measure rate --seconds
600`). Frames a second, the median of each minute:

| | minutes 1–10 | the bar (the end within 10% of the start) |
|---|---|---|
| Note10+ (low) | 59, 60, 52, 53, 49, 49, 49, 49, 49, 48 | missed (−19%) |
| iPhone (its pick) | 60, 47, 47, 47, 47, 45, 47, 46, 41, 45 | missed (−25%) |
| iPad (its pick: high) | 11 every minute | held, at 11 |

- **The Note10+** slows as it warms: its GPU's clock never goes over 572
  MHz of its 754, and its render thread is busy 76% of the time. Memory
  is flat (205 → 202 MB).
- **The iPhone** warms within a minute (thermal state "fair" at 44 s)
  and drops to about 47. Memory nearly flat (568 → 594 MB).
- **The iPad** picks high, as every Apple GPU does
  (`QualityLevel::for_adapter_on`, `(_, APPLE) => High`), and draws
  Sheepdog at 11. Stage 14 found the A10 can't draw high (RN10); the
  pick was never changed. Its memory went 180 → 422 MB in the first
  quarter and 449 by the end: most of it the first minutes at high,
  the rest a slow climb to look into.

Sheepdog on the iPhone at high and 120 (the plan's bar): **75**, down
from 80 when last measured; "fair" at 23 s and "serious" at 43 s.

## The bars owed by earlier stages

| | Note10+ | iPad | iPhone | bar |
|---|---|---|---|---|
| 100 `hideBehind` tags among 3,000 (stage 15), the UI thread's extra a frame | 1.27 ms | lost in noise (4.9 vs 5.0 ms) | 0.39 ms | 1 ms |
| A mesh changed every frame (stage 13), setting + sending | 0.39 + 0.83 = 1.22 ms | | 0.16 + 0.35 = 0.51 ms | 1 ms |
| "Particles and the world" (stage 12) | 59.9 fps | | 60 fps, step 0.03 ms | 60 |

The Note10+ misses both CPU bars; the iPhone meets them. The tags page
holds 60 on all three.

## The iPhone's budgets

Stage 14's budgets (what each part of a frame may take at 60), against
the lab's busiest scene, "All of it together", in ms (medians over 15
s). The iPhone's were owed; the others are stage 14's.

| Part | Budget | iPhone (low) | iPhone (medium) | iPhone (high) | Note10+ (low) | iPad (high) |
|---|---|---|---|---|---|---|
| The game's code | 3 | 0.63 | 0.64 | 0.78 | 1.58 | 2.11 |
| Animators | 1.5 | 0.38 | 0.43 | 0.53 | 1.21 | 1.32 |
| Meshes sent | 1 | 0.47 | 0.51 | 0.49 | 0.85 | 1.81 ✗ |
| Physics | 1 | 0.07 | 0.07 | 0.08 | 0.14 | 0.12 |
| Particles | 0.5 | 0.03 | 0.03 | 0.04 | 0.06 | 0.07 |
| Sound | 0.25 | 0.03 | 0.02 | 0.01 | 0.05 | 0.03 |
| `world.update` | 7 | 1.60 | 1.73 | 1.95 | 4.17 | 5.52 |
| Flutter's UI thread | 10 | 0.86 | 1.03 | 0.83 | 5.20 | 6.56 |
| The render thread | 14 | 2.63 | 3.00 | 2.56 | 4.67 | 37.27 ✗ |
| Frames a second | 60 | 59.9 | 60.0 | 60.0 | 59.7 | 26.8 ✗ |

The iPhone is within every budget at every level, at about a fifth of
each.

## What phase 3 and 4 start from

1. **The iPad's pick**: high on an A10 is 11 fps in Sheepdog. The pick
   by GPU family is the first fix (phase 3).
2. **The GPU at medium and high** on the Note10+ and the iPad: the scene
   pass, the finish, ambient occlusion, shadows, bloom (phase 3).
3. **Heat**: the Note10+ and the iPhone lose a fifth to a quarter of
   their frames after a few minutes. Less GPU work a frame is the fix;
   the ten-minute runs say whether it was enough.
4. **The CPU on the Note10+**: the tags (1.27 ms) and the mesh change
   (1.22 ms) (phase 4).
5. **Memory**: the iPad's climb at high, to look into (phase 4).
6. **The pile**: 293 KB of garbage a frame, and a step slower than stage 5's (phase 4).
