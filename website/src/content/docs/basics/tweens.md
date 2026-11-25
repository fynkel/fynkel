---
title: "Tweens"
description: "Move, turn, scale, colour or change any value over time."
---

A tween moves something over time, with an ease, and stops by itself when
its object is destroyed:

```dart
class Door extends GameObject {
  Door() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFF806040)))]);

  Motion open() => moveBy(Vec3(0, 2, 0), seconds: 0.6, ease: Ease.outBack);

  void wobble() {
    punchScale(Vec3.one * 0.2, seconds: 0.3);
    sequence([colorTo(Color.white, seconds: 0.05), colorTo(Color(0xFF806040), seconds: 0.3)]); // this door alone
  }

  void hover() => moveBy(Vec3(0, 0.2, 0), seconds: 1, ease: Ease.inOutSine, loops: -1, yoyo: true);

  void fadeLight() => tween(1.0, 0.0, seconds: 2, onUpdate: (v) => get<Light>()?.intensity = v);
}
```

Each returns a **`Motion`**: `stop()`, `complete()` (to where its last loop
ends; one that loops for ever ends the loop it's in), and it's a future, so
`await open();` waits for it to end.

### Helpers

`moveTo`, `moveBy`, `rotateTo`, `rotateBy`, `lookAtOver`,
`scaleTo`, `colorTo`, `punchScale`, `shake`. Each takes `seconds:`; all but
`colorTo`, `punchScale` and `shake` take `speed:` instead. `moveTo`,
`moveBy` and `rotateTo` are in the world, as `position`; `localMoveTo` and
`localRotateTo` relative to the parent; `scaleTo` sets `localScale`, and
`rotateBy` turns about the object's own axis.

`colorTo` changes the renderer's own `color` ([Meshes and
materials](/graphics/meshes-and-materials/)), so other objects sharing its
material don't flash too; give it `material:` to change a material. A
`punchScale` or `shake` given while one is still going on the object ends
where the first began: a recoil every shot never grows the barrel.

### tween(from, to, …, onUpdate:)

`tween(from, to, …, onUpdate:)` moves any value: `double`, `Vec2`, `Vec3`,
`Quat`, `Color`, or anything with a `lerp:` you give.

### Options

`ease:` (`Ease.outQuad` by default; any `double
Function(double)`, so Flutter's `Curves.x.transform` too), `delay:`,
`loops:` (-1 for ever), `yoyo:`, `unscaled:` (goes on while paused),
`onComplete:`.

### sequence([...])

`sequence([...])` plays one after another, **`together([...])`** all at
once.
