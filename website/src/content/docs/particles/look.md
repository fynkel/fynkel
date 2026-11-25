---
title: "Particles: the look"
description: "Fire, texture sheets, curves, lit and soft particles."
---

Unity's other modules, by the same names:

```dart
/// A fire: [flames] a texture of 4 × 4 frames of a flame.
ParticleSystem fire(Texture2D flames) => ParticleSystem(
  startSize: const Range(0.8, 1.2),
  startLifetime: const Range(0.7, 1),
  emission: const Emission(rateOverTime: 40),
  forceOverLifetime: const ParticleVector(y: ParticleCurve(2)),
  noise: const ParticleNoise(strength: ParticleCurve(0.4), frequency: 1.5, scrollSpeed: 1),
  textureSheet: TextureSheet(columns: 4, rows: 4),
  softness: 0.3,
  material: Material(baseColorTexture: flames, blending: Blending.additive),
);
```

## In detail

- **Curves.** A module's numbers are `ParticleCurve`s, Unity's `MinMaxCurve`:
  `ParticleCurve(2)` a constant, `ParticleCurve.between(1, 3)` random between
  two constants, `ParticleCurve.curve((t) => …, scale:)` a curve over each
  particle's life, `ParticleCurve.curves(a, b)` random between two curves.
  Each particle keeps its own place between the two for its whole life.
- **Movement.** `velocityOverLifetime` (a `ParticleVector`, x, y and z curves,
  in local or world space) is added to how it moves; `forceOverLifetime`
  speeds it up; `limitVelocity` (a `LimitVelocity`, no `Particle` in its name)
  holds its speed to a `limit` (by `dampen`, 0 to 1, a second) and slows it by
  `drag`; `rotationOverLifetime` turns it, radians a second; `noise` pushes it
  about in a field that moves with `scrollSpeed` (`octaves` up to 4, each
  costing about as much again).
- **By speed.** `sizeBySpeed` and `colorBySpeed` scale its size and colour
  from `minSpeed` to `maxSpeed`: sparks white when fast, red as they slow,
  `colorBySpeed: ColorBySpeed(ColorGradient([...]), maxSpeed: 6)` (the curve
  or the gradient comes first, without a name).
- **Texture sheets.** `textureSheet` splits the base colour texture into
  `columns × rows` frames, played over each life (`frameOverTime`, `cycles`),
  from a `startFrame`, or one row each (`randomRow`): flames, explosions.
- **How they're drawn.** `renderMode`: `ParticleRenderMode.billboard` facing
  the camera (the default); `.stretched(lengthScale:, speedScale:)` long the
  way they move (sparks, rain); `ParticleRenderMode.horizontal` lying flat
  (ripples, splashes); `ParticleRenderMode.vertical` standing up and turning
  to the camera round up; `.mesh(mesh)` a mesh each, turned round up by its
  rotation (leaves, rocks). A mesh in use can't be disposed.
- **Soft particles.** `softness`, in metres: they fade out as they come that
  close to what's behind, so smoke doesn't cut a hard line where it meets the
  floor. At `Quality.low`, which has no depth to read, they're hard.
- **Lit particles.** `lit: true` lights them as a surface facing the camera:
  smoke the sun falls on. Each square (or a trail's point) is lit once, at its
  middle, by the ambient light, the sky, the suns with their shadows and the
  lamps in reach, as a matte surface, so it's the same colour across and has
  no highlights; a cloud of overlapping ones costs little more than unlit
  ones. Mesh particles are lit pixel by pixel, with the material's `metallic`
  and `roughness`.
- **A material's own shader** ([A first shader](/shaders/first/)) shades them,
  its `unlit` or `surface` function, with `particle_age()` (0 to 1 through its
  life) and `particle_random()` (its own number) to tell them apart.
