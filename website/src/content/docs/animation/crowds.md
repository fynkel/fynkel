---
title: "Many characters"
description: "What a crowd costs on real devices."
---

![A crowd of animated knights and archers](/shots/crowd.webp)

Each character's clips are sampled and blended in the engine, and its
mesh bent on the GPU, so a crowd costs little. The lab's "Animation: a
crowd" shows 1 to 100 KayKit characters, each on its own clip, and says
what the animation costs a frame. Measured with a hundred:

| Device | The animation a frame | Frames a second |
|---|---|---|
| Mac | 0.9–1.3 ms | 120 of 120 asked, at any quality |
| iPhone (ProMotion) | 2.1–2.8 ms | 120 of 120 at low and medium; at high, 80 (high's own cost: one character gives 80 too) |
| Galaxy Note10+ | 6–7 ms | 42 of 60 at low; 30 characters hold 60 |

Each character also takes about 160 KB of the engine's memory (its objects and
its animator); copies of one model share its meshes and textures. For a crowd
on an older Android phone, keep it to a few dozen or turn the far ones'
animators off (`stop()` holds their pose and costs nothing).

A hidden character (`active = false`, waiting in a pool) keeps its clip's time
but isn't posed, and costs nothing either: 200 of them take the animators 4 µs
a frame on a Mac.
