---
title: "What works today"
description: "What Fynkel can do now, what it runs on, and what is not there yet."
---

Fynkel is a 0.x preview. Whole games have been made with it, and its tests
run on every change, but the API may still change between versions: each
change is in the [changelog](/start/changelog/).

## What it runs on

| Platform | Works | Needs |
| --- | --- | --- |
| iPhone and iPad | Yes | iOS 15 or later |
| Android | Yes | Android 11 or later, 64-bit, with Vulkan |
| Mac | Yes | macOS 12 or later |
| Windows, Linux, web | No | |

Games are made on a Mac with Flutter 3.47 or later. Windows and Linux can't
build them yet.

## What is there

| | |
| --- | --- |
| [Objects](/basics/objects/) | A tree of game objects and behaviours, routines, tweens, signals, pooling |
| [Drawing](/graphics/meshes-and-materials/) | Physically based materials, shadows, skies, fog, bloom, ambient occlusion, up to 64 lamps, quality levels by device |
| [Shaders](/shaders/first/) | Surfaces, vertices, whole styles and screen effects in WGSL, hot reloaded |
| [Models](/assets/loading/) | glTF, with skins and clips; meshes, shapes, text and splines in code |
| [Animation](/animation/clips/) | Clips, cross fades, layers, masks, 1D blends, events, crowds |
| [Physics](/physics/colliders/) | Rigidbodies, joints, triggers, queries, character controllers |
| [Sound](/sound/playing/) | Files, 3D sound, music that crossfades and loops on the beat, groups |
| [Particles](/particles/basics/) | Emitters, trails, forces, collisions with planes |
| [Flutter](/flutter/hud/) | HUDs, menus, widgets pinned to objects, several views, games inside apps |
| [Testing](/making/testing/) | Whole games run and checked without a window, pictures too |

## What is not there yet

### The game

- An editor: the game is written in code
- Navmeshes and path finding
- Scene files and saving: a game does these itself for now
  ([Levels](/making/levels/) and [Saving](/making/saving/) show how)

### Input

- Gamepads

### Drawing

- Reflections of the scene itself (a mirror shows the sky alone)
- A 2D game's own pieces: a pixel-perfect camera, sprite sheets cut by the
  engine, tilemaps ([sprites](/assets/sprites/) and [flat
  text](/assets/flat-text/) are there)
- Live widgets laid out in the scene and tapped through it ([a widget drawn
  once into a texture](/assets/widget-pictures/) is there)
- A model's own vertex colours (a mesh made in code has them)
- Shadows from point lights (a spotlight's are there), and from see-through
  objects

### Shaders

- Matrices in `Values`, and shaders that use one another's code
- A node graph: shaders are written in WGSL

### Animation

- Morph targets (faces, blend shapes)
- 2D blends, by direction and speed
- Root motion and IK (feet on slopes)
- Clips moved from one skeleton to another (Unity's Humanoid), and clips made
  in code

### Physics

- 2D physics
- `ConfigurableJoint`, and chains of joints that don't stretch
- Stepping physics by hand (Unity's `Physics.Simulate`), for a throw's arc
  worked out ahead or a replay

### Sound

- A limiter on the mix: sounds together past full scale are cut flat, so
  [leave room](/sound/music/#loud-together)
- Reverb, echo and low-pass effects, Doppler, HRTF, a custom rolloff curve,
  and sound blocked by walls
- Ducking by itself (a group's `fade` does it by hand)
- The microphone, and sound made in code as it plays
- A handle on a one-shot, to stop it or hear of its end (a source of its own,
  with `onEnd`, has both)
- Fading a source (set its `volume` each frame, or put it in a group and
  `fade` that), and pausing music (keep its `playTime`, `stop` it, and set
  `playTime` again before the next `play`)
- On Android, pausing when another app takes the sound

### Particles

- Particles on the GPU (Unity's VFX Graph)
- Collision with physics' colliders (only planes), triggers and wind zones
- Lights on particles, shadows from them, a mesh's surface as a shape
- A `LineRenderer`

### Packages

- Packages with Rust code of their own: `package:fynkel/hooks.dart` builds
  only the engine's crates
