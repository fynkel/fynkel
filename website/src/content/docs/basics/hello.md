---
title: "How a game is made"
description: "The smallest game, line by line: objects, components, and the calls every object gets."
---

The whole of `lib/main.dart` for a lit, turning cube:

```dart
// lib/main.dart
import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() => runApp(FynkelApp(Hello.new));

class Hello extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(transform: Transform.lookAt(Vec3(0, 1.5, 4), Vec3.zero), components: [Camera()]));
    add(GameObject(components: [Light.sun(direction: Vec3(-0.4, -1, -0.3))]));
    add(Cube());
  }
}

class Cube extends GameObject {
  Cube() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A)))]);

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, time.delta);
  }
}
```

- **Everything in the game is a `GameObject`**, in a tree. `Hello` is the
  root; `FynkelApp(Hello.new)` makes one and shows it full screen. A root that
  takes arguments is made by a function of your own: `FynkelApp(() =>
  Level(number: 3))`.
- **An object has a transform** (where it is, relative to its parent) and
  **components**: a `Camera` to see from, a `Light`, a `MeshRenderer` to be
  drawn.
- **A new world is a clear day**: a sky that lights everything. `Light.sun` adds the sun, which
  casts shadows. [Making
  it look good](/graphics/looks/) goes on from there.
- **`onLoad`** is where an object adds what it's made of. **`onUpdate`** runs
  every frame. `time.delta` is the frame's time in seconds, so the cube turns
  one radian a second whatever the frame rate.
- **Always call `super`** in the `on…` methods. Fynkel's own do nothing today,
  but a class you extend may do something in its own, and the analyzer warns
  when a call is missing.

Y is up, and a camera looks down its −Z. Lengths are metres and angles
radians everywhere, the camera's and the joints' too: `90.deg` writes
one in degrees, and `angle.inDegrees` reads one back.
