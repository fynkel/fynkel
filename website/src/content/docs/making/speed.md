---
title: "Keeping it fast"
description: "What keeps a game fast on a phone."
---

What keeps a game fast:

- **A little garbage is fine; a lot is not.** `Vec3`, `Ray`, `ViewPoint` and
  `Transform` are small objects that never change, so working out a new one
  makes one: a drag frame that reads `finger.position` and asks
  `camera.screenPointToPlane` makes about 9, some 300 bytes, which Dart frees
  for almost nothing.

  What would cost is thousands a frame, and that's where the engine says "no
  garbage": `setPositions` and the other mesh channels, `setSweep`,
  `MeshBuilder`, pools, particles, queries that miss. Keep a list and change
  it in place for those.

  To see what a game allocates, run it with `flutter run --profile` and open
  DevTools' Memory view.
- **What the camera can't see costs the GPU nothing.** An object wholly out of
  view isn't drawn; it still costs its update and its copy to the renderer, so
  hide or destroy what's far away for good.

### Every speed times time.delta

Then the game is the same at 60 and 120
fps.

### 60 frames a second is plenty

60 frames a second is plenty for most games, and keeps a phone cool
([Time and frame rate](/basics/time/)). Ask for 120 only where it shows, a
fast game, and measure it for 10 minutes, not one: a phone slows itself down
when it's hot.

### Many alike things, one manager

coins moves its 300 motes from one
object's `onUpdate` (`Swarm`): one call a frame, not 300.

### When memory runs short, the system says so

`world.onLowMemory` is
Unity's `Application.lowMemory`: let go of what the game can get back, a
cache or a pool, or drop `world.quality`: Sheepdog on an iPad holds 232 MB
of GPU memory at high and 43 MB at low.

A phone that stays short can close the game. While the app is away its views
also let go of the images they draw through, and make them again with the
next frame.

### Share materials and meshes

`Mesh.cube()` is always the same mesh; give
many objects one `Material`.

### Let bodies sleep

A body at rest costs nothing a step; one woken every
step (a force each `onFixedUpdate`) costs as one moving.

### Listen only where you need to

Contacts are worked out only for
colliders whose object or rigidbody is `with Triggers` or `with Collisions`.

### Move and query in any order

A query first brings physics up to what
moved since the last one, and looks only at those objects and what's below
them: moving one bullet among 5,000 colliders and then casting costs about 2
µs on a Mac.

### Query from the updates

The step runs between frames, so a query in
`onUpdate` never waits for it; one from a Flutter handler between frames
may.

### More steps for fast spins and chains

Physics turns a body at most a
quarter turn a step, and a chain of joints pulls apart more the longer a
step is: `time.fixedDelta = 1 / 100` (or 1 / 120) where a game needs it, in
its root's `onLoad`. Keep a joint's two bodies within 10 times each other's
mass, as Unity advises.

### More solver iterations for tall stacks

Each step, physics goes round
the touching bodies `physics.defaultSolverIterations` times (6, Unity's
default), each time pushing every pair a little closer to right. A tower of
many boxes, or a heavy one landing on a light one, needs more rounds for the
push to get from top to bottom: 10 to 16 for a stacking game, with
`time.fixedDelta = 1 / 100` if pieces still sink into each other. More costs
about as much more physics time.

### Short-reaching lamps

A lamp costs something for each object it reaches,
and the pixels it lights; one that reaches no further than it needs to costs
least. Turn a lamp off with `enabled = false`: it's left out altogether.

### Shaders that read what's behind cost most

Shaders that read what's behind cost most ([A first
shader](/shaders/first/)): the frame takes three passes while one is in
view. On an older phone give low quality a material that doesn't read it.

### Views on screen

Views on screen (`directToScreen`, on unless turned off; on Android with
a `FynkelActivity`, [Adding Fynkel to an app](/start/existing-app/)), so
Flutter neither waits for the game's frames nor copies them.

### A cheaper quality for an older phone

`world.quality = Quality.low`
draws in one pass, with no image between the scene and the screen; or take
from a level only what costs most there, bloom first, or draw fewer pixels
(`renderScale`) ([Quality levels](/graphics/quality/)).
