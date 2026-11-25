---
title: "Measuring"
description: "Frame times, budgets, and where the time goes."
---

**How long the frame took**, part by part, is `world.profile`: what Unity's
Profiler shows in its CPU view, for the frame on the game's thread.
`profile.updates`, `lateUpdates`, `fixedUpdates`, `starts` and `input` are
your code in each call; `motions` the tweens and routines; `meshes` sending
the meshes you changed; and `profile.systems` each system's own time, by name:
`Animators`, `Particles`, `Cameras`, and `Physics` and `Audio` from their
packages.

Each part is its own (a system's work isn't counted in the part it runs in),
and they add up to `profile.total`. It's the last finished frame's, always on,
and costs nothing you'd see; `print(world.profile)` lists it all.

One frame's numbers are uneven, so take the median of a second's worth: the
lab's "The frame, part by part" does, for every scene. The parts are
`Duration`s. A pause in Dart itself (a garbage collection, or in debug code
being compiled) lands in whichever part was running, so one frame's spike in
`input` or `Cameras` says only that something paused: the median says where
the time goes.

What the engine does to draw the frame runs on the view's own thread:
`ViewStats.busy`, from a `FynkelView`'s `onStats` (`stats.view.busy`, for
about a second).

## On real devices

### Measure

Measure on the slowest phone you support. Each example has a
`tool/measured.dart` for package:fynkel_measure's tool (a dev_dependency, so
the game itself doesn't carry it), which gives the frame rate and memory:
`dart run fynkel_measure:measure rate`, at the game's own frame rate
(`--define FYNKEL_FRAME_RATE=0` for as fast as it goes).

On an iPhone or a Mac it gives the thermal state too, and each minute's
frame rate with `--seconds 600`. `lifecycle` times restarts, new roots and
new views; `bench` the engine's own scenes; `--ios <device>` or `--android
<device>` measures on a phone, `--baseline` Flutter alone; `--help` lists
them.

### A frame's budget

A frame's budget at 60 fps is 16.7 ms. The engine's own are, on the
level a device draws at: your code 3 ms, the animators 1.5, meshes you
change 1, physics 1, particles 0.5, sound 0.25, so `world.update` 7 in all;
the render thread 14 a frame. The lab's 'All of it together', everything at
once, takes this, in ms ([the
report](https://github.com/fynkel/fynkel/blob/main/docs/stage-16-report.md)
has each part):

| | Galaxy Note10+ (low) | iPad, 6th gen (low) | iPhone 16 Pro Max (high) | Mac (high) |
|---|---|---|---|---|
| `world.update` | 3.2 | 1.3 | 1.4 | 0.3 |
| Flutter's UI thread | 4.2 | 1.7 | 1.2 | 0.3 |
| The render thread | 4.4 | 33.5, waiting for its GPU | 3.0 | 0.9 |
| Frames a second | 59.9 | 39.3 | 60 | 60 |

So a 2018 iPad holds 40 here, not 60: on an A10, plan on the GPU being
what limits you. The lab's `tool/measured.dart` breaks a scene's
frame down so on a device: `--define 'TOUR=<title>'`.

### Each level on each phone

Each level on each phone: Sheepdog's field, frames a second, a
sense of what the levels cost (the same report has more scenes):

| | low | medium | high |
|---|---|---|---|
| Galaxy Note10+ | 60 | 48 | 34 |
| iPad, 6th gen | 40 | 34 | 23 |
| iPhone 16 Pro Max | 60 | 60 | 60 |

### Measure a change on every GPU you support

The same shader can be
quicker on one and far slower on another: reading shadows with gathers
helped an A10 and cost a Mali-G76 a third of its frames.

### Where the GPU's time goes

Where the GPU's time goes: `WorldView.timeGpu = true` times each part of
every view's frames on the GPU (shadows, ambient occlusion, the scene,
bloom, the last pass, screen effects), and `view.stats.gpu` adds them up;
subtract an earlier one and divide by its `frames` for a frame's. It's for
measuring: off, it costs nothing; on, a little GPU time.

The parts never overlap, and say where the time goes rather than to the
tenth of a millisecond. **On a phone they can't compare two looks**: its GPU
slows its clock until it's nearly full, so a frame takes about as long
whatever it draws, and the cost shows as clock, power and heat.

One game's plain look and its full one both read about 13.5 ms a frame on
the Note10+, at 179 and 571 MHz. Compare the clock times how busy it is (on
the Note10+, `adb shell cat /sys/kernel/gpu/gpu_clock
/sys/kernel/gpu/gpu_busy`), or the phone's temperature over ten minutes.
