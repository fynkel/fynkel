---
title: "Layers and debug drawing"
description: "Keep things apart, and draw lines to see what the code is doing."
---

**Layers** keep things apart. Each object has a `layer`, 0 to 31; a
camera's `cullingMask` leaves layers undrawn, their shadows too in that
camera's picture, `world.raycast(ray,
layers:)` goes through some, and `input.layers` makes taps go through
some (sparks and other effects):

```dart
const effects = 1;

class Sparkle extends GameObject {
  Sparkle() : super(layer: effects, components: [MeshRenderer(Mesh.sphere(), Material(emissive: Color.white))]);

  @override
  void onStart() {
    super.onStart();
    input.layers = LayerMask.except([effects]); // taps go through sparkles
  }
}
```

## Debug drawing

**Debug drawing**, for a frame or for `seconds:`, on top of everything:
`debug.line(a, b)`, `debug.ray(origin, direction)`, `debug.box`,
`debug.sphere`, `debug.path`, each with a `color`. It's off in release
builds. `debug.drawShapes = true` draws each particle system's shape and
the planes its particles hit ([Particle basics](/particles/basics/)).
