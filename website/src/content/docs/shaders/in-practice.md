---
title: "Shaders in practice"
description: "Mistakes, hot reload, the first frame, costs and limits."
---

## Mistakes, changes and the first frame

### A shader that doesn't compile

A shader that doesn't compile throws a `ShaderError` with its file, line
and column (`lava.wgsl:5:12: ...`), its own lines, not the engine's around
them.

### Hot reload takes a shader's new code

Hot reload takes a shader's new code: change the file, press hot reload,
and every material using it draws with it from the next frame. Code that
doesn't compile keeps the old and says why on `world.errors`.

### The first time a shader is drawn

The first time a shader is drawn on a device, the GPU's driver compiles
it: a third of a second on a Mac, more on a phone, then kept by the system
for every run after. A view on screen doesn't wait for it: it draws without
those objects for those frames.

Behind a loading screen, let the view draw the level and `await
world.shadersReady()`, so nothing pops in when the game shows. The loading
screen is your widget: a `ValueNotifier<bool> ready` the game sets after
`await world.shadersReady(timeout: const Duration(seconds: 3))` in a routine
started in `onStart`, and a panel over the view until it's true.

## What shaders cost

Measured: the lab's shader
scenes hold 60 fps on a Mac and an iPhone at every level, and 120 when
asked; a Note10+ holds 60 at low in each.

### A surface or a vertex shader

A surface or a vertex shader costs about what the engine's own material
does: a few lines more of the same work. 2,000 blades of grass swaying in
the wind add about 0.1 ms a frame on a Mac.

### Reading what's behind

Reading what's behind (`depth_behind`, `scene_color`) costs most: the
frame is drawn in three passes instead of one while such a surface is in
view. The lab's pond and glass ball hold 60 at low on a Note10+, but 48 at
medium and 41 at high.

On a slower phone, give low quality a material that doesn't read it:
Sheepdog's pond is still water at low, which took it from 40 back to 50 fps
on the Note10+.

### A shader's first frame

A shader's first frame on a device: on a Note10+ its pipelines take up
to a second to be ready, then the phone keeps them for every run after. On
an iPhone, up to about half a second. Wait for them behind a loading screen,
as under [mistakes and changes](#mistakes-changes-and-the-first-frame).

## Limits

Four textures and four `PerObject` numbers to a shader, 512 bytes of
`Values`: numbers, vectors and arrays of them (no matrices yet, [What works today](/start/what-works/)).
A shader can't use another's code.
