---
title: "Behaviours"
description: "What an object does, written once and put on any object."
---

**A behaviour is what an object does**, written once and put on any
object: health, a spin, a flash when hit. It's a `Behaviour` subclass,
added in `components:` or with `addComponent`, and it gets every call an
object gets (`onStart`, `onUpdate`, …):

```dart
class Spin extends Behaviour {
  Spin({this.speed = 1});

  double speed;

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, speed * time.delta); // its object's transform
  }
}

class Coin extends GameObject {
  Coin() : super(components: [MeshRenderer(Mesh.sphere(), Material(color: Color(0xFFFFC030))), Spin(speed: 3)]);

  void stop() => get<Spin>()?.enabled = false; // still there, but not called
}
```

From a behaviour, its object is near: `gameObject`, and the same
shortcuts an object has (`transform`, `position`, `get<T>()`, `world`,
`time`, `input`, `assets`, `startRoutine`, `on`).

## Adding and finding components

- **`addComponent(b)`** puts a behaviour on at any time and returns it;
  **`removeComponent(b)`** takes it off for good.
- **`get<T>()`** gives the first component of type `T` (or a subtype, or an
  interface it implements), or null; `getAll<T>()` all of them; `has<T>()`.
  `getInParent<T>()` and `getInChildren<T>()` look up or down the tree too,
  and `getAllInChildren<T>()` gives every one below; as Unity's, they skip
  objects turned off unless `includeInactive: true`. They find components:
  `getInParent<Level>()` for a `Level` object throws and points at
  `ancestor<Level>()`.
- **`enabled = false`** stops its calls, its routines and its tweens until
  it's turned back on.
- A destroyed object still has its components, to read: a dead enemy's
  `get<Health>()` still says how much it took.
- **To keep a behaviour in a typed field**, make it in a constructor of your
  own and pass it on, so the field and the components share it: `Enemy() :
  this._(Health(3));` then `Enemy._(this.health) : super(components:
  [health]);` with `final Health health;`.

## A subclass or a behaviour?

**Which one?** If a subclass's `onUpdate` would make sense on another kind
of object, it belongs in a behaviour. Keep a class for what an object *is*
(its state, its parts, its typed fields), and behaviours for what it
*does* that others could do too, or that comes and goes while it plays.
