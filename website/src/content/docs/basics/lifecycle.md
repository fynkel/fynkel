---
title: "The frame and the lifecycle"
description: "Which calls an object gets, and in what order a frame runs."
---

Each object and behaviour gets these calls, in the order they were added:

| Call | When |
|---|---|
| `onLoad` | Once, before it appears. It may be `async` (to load assets, [Loading assets](/assets/loading/)); the object appears when everything in it has loaded. Children added in it load alongside it, not before the rest of it: `await child.loaded` before reading what the child's `onLoad` sets (`await (a.loaded, b.loaded).wait` for two). |
| `onEnable` | Each time it starts being shown: when it appears, and when it or a parent is turned back on. |
| `onStart` | Once, at the start of its first frame. |
| `onFixedUpdate` | Every fixed step: 50 times a second, whatever the frame rate. For forces and what physics needs. |
| `onUpdate` | Every frame. |
| `onLateUpdate` | Every frame, after every `onUpdate`, the routines and the tweens: for what follows where things ended up. |
| `onDisable` | Each time it stops being shown, and before `onDestroy`. |
| `onDestroy` | Once, when it's destroyed. |
| `onApplicationPause` | When the app goes to the background (`true`) and comes back (`false`): save here, as the app may be closed while it's away. `world.applicationPaused` says which. A `FynkelApp`'s or `FynkelGame`'s world hears it even while none of its views is showing; a `World` you make yourself hears it only while a `FynkelView` shows it. |

## The order of a frame

A frame goes: input → starts → fixed steps → updates → routines and
tweens → late updates → cameras → drawing. This order is fixed and
tested.

## What objects cost each frame

**What objects cost each frame**: only those with something of yours to call.
A plain `GameObject`, with only the engine's components (a renderer, a light,
a collider, a particle system), costs the frame nothing on the game's thread
however many there are: 32,000 still cubes add 0.002 ms on a Mac.

A subclass, or one with a behaviour, has its updates called whether it
overrides them or not: tens of nanoseconds each on a Mac before your code,
which adds up in the tens of thousands. So make scenery plain objects, and
give the class of your own to what thinks.
