---
title: "Shadows"
description: "Sun and spotlight shadows: reach, sharpness and cost."
---

![Trees, columns and a fence casting shadows](/shots/shadows.webp)

A sun casts shadows unless it's made with `shadows: false`, as a new
scene's sun does in Unity; a spotlight casts them when asked. Every
object casts them and has them fall on it, unless its renderer says not:

```dart
/// A low evening sun, casting long shadows.
GameObject eveningSun() => GameObject(components: [Light.sun(direction: Vec3(-0.6, -0.5, -0.4), intensity: 2)]);

/// A spotlight 4 m up, pointing straight down, casting shadows.
GameObject stallLight() => GameObject(
  transform: Transform(position: Vec3(0, 4, 0), rotation: Quat.lookRotation(Vec3(0, -1, 0), up: Vec3(0, 0, -1))),
  components: [Light.spot(range: 8, angle: 60.deg, shadows: true)],
);

/// A puddle: it shadows nothing, but shadows fall on it.
MeshRenderer puddle(Material water) => MeshRenderer(Mesh.cylinder(), water)..castShadows = false;
```

`light.shadows`, `renderer.castShadows` and `renderer.receiveShadows` can
change at any time. The lab's 'Look: shadows' scene shows the sun's and a
spotlight's, at each level.

### The sun's shadows

The sun's shadows reach the quality's `shadowDistance` from the camera
and fade out there (less if the camera's `far` is nearer). They're drawn
into `shadowCascades` maps seen from the sun, with `shadowSoftness` edges:
at low one map with soft edges, at medium two, at high three with softer
ones. With more than one, the nearest covers least, so shadows near the
camera are sharpest.

### How sharp they are

How sharp they are is the map's texels spread over the distance it
covers: halve `shadowDistance` and they're twice as sharp. Keep it to what
the game needs shadowed. A table-top game seen from 2–3 m wants 10–20 m; an
open field, 60 or more with cascades.

Low's single 1024 map over 20 m keeps a wall's shadow smooth on a phone;
over 40 m it's saw-toothed. A bigger map is sharper too, at four times the
memory each step: `shadowResolution: 2048` is 8 MB a map, 1024 2 MB. Soft
edges cost the Note10+ about 1 ms of GPU a frame over hard ones, and a 2018
iPad about 2.

A quality with `shadows: false` draws none, whatever the lights ask. The
maps move with the camera a whole texel at a time, so a still shadow's edge
doesn't crawl as the camera moves. Only the first sun with `shadows` casts
them.

### A spotlight's shadows

A spotlight's shadows reach as far as its `range`. Each frame gives a
few spotlights shadows, the ones nearest the camera: 1 at low, 2 at medium,
4 at high. The others still light, without shadows. A spotlight with shadows
is at most 150° across: a wider one is refused with an `ArgumentError`,
since its shadow map can't cover the rim.

### Point lights don't cast shadows

Point lights don't cast shadows yet. A torch that should throw shadows
is a point light for its glow and a spotlight with shadows above it, aimed
down.

### castShadows: false

`castShadows: false` is for what shouldn't darken the ground (a glow,
water, a ghost) and for what can't shadow anything (a flat floor), which
also saves drawing it into the maps. **`receiveShadows: false`** keeps
shadows off an object.
