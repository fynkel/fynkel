---
title: "What particles cost"
description: "Numbers from real phones, and how to keep smoke cheap."
---

Each system is one draw, however many particles it has. The
engine moves 10,000 particles in about 0.03 ms a frame on a Mac, 0.1 ms on an
iPhone 16 Pro Max and 0.3 ms on a Galaxy Note10+; `world.particleStepTime`
says how long the last frame's took (Unity's Profiler's
ParticleSystem.Update), and the `Particles` system in `world.profile`
([Measuring](/making/measuring/)) the whole of their frame.

A frame with systems playing and bursting makes no garbage. The modules cost
more: on a Mac, 10,000 particles take about 0.08 ms more for each of velocity
over lifetime, force and limit velocity, 0.04 ms for rotation, and 0.15 ms for
noise (0.4 ms with three octaves), as Unity's noise is its most expensive
module. Those 10,000, small, hold 60 fps at every quality level on all three.

## Drawing over and over

is what costs most on a phone: big see-through
particles covering the screen, each pixel drawn once for every puff in front
of it. The lab's "screen of smoke" fills the whole screen with puffs two to
three metres across, five metres from the camera (`LAB_SMOKE=200`, or any
count).

On a Galaxy Note10+ at low (a quarter fewer pixels, `renderScale` 0.75), 200
and 500 hold 60, 1,000 bring it to between 28 and 48, 2,000 to 27; at medium
and high, drawing every pixel, 200 bring it to 12.

An iPhone 16 Pro Max holds 60 with 200 at every level and with 500 at low, and
falls to 40 with 500 at high. A Mac holds 60 with 200. So:

- **Keep big puffs to a few hundred on screen at once.** A puff that covers a
  tenth of the screen costs as much as thousands of sparks. Sheepdog's dust is
  under half a metre across and gone in a second.
- **Make them smaller and fewer, and fade them sooner.** A puff that grows to
  twice its size costs four times as much at the end of its life; a shorter
  `startLifetime` means fewer on screen.
- **Fewer pixels help most**: low's `renderScale` and its soft particles off
  (`Quality.softParticles`, which reads the depth for every pixel) are why a
  Note10+ holds 60 with 200 at low and 12 at high.
- **Additive costs as much as transparent**: both draw every pixel; additive
  only saves the sort.
