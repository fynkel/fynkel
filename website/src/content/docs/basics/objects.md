---
title: "Objects"
description: "GameObjects: a tree of objects, subclasses, adding, destroying, hiding and finding them."
---

An object can be built in one expression, the way a Flutter widget tree
is, or made in a subclass:

```dart
GameObject lamp() => GameObject(
  name: 'lamp',
  transform: Transform(position: Vec3(2, 0, 0)),
  components: [MeshRenderer(Mesh.cube(), Material(color: Color.white))],
  children: [
    GameObject(
      name: 'bulb',
      transform: Transform(position: Vec3(0, 1, 0), scale: Vec3.one * 0.3),
      components: [MeshRenderer(Mesh.sphere(), Material(emissive: Color(0xFFFFE0A0), emissiveIntensity: 4))],
    ),
  ],
);
```

## Subclasses

**A subclass is what an object is**: a `Player`, an `Enemy`, a `Level`.
It holds its state in fields, builds its parts in its constructor or
`onLoad`, and comes back typed wherever the game finds it:

```dart
class Crate extends GameObject {
  Crate(Vec3 at)
    : super(
        transform: Transform(position: at),
        components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFB08040)))],
      );

  int hits = 0;
}

class Yard extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    for (var i = 0; i < 3; i++) {
      add(Crate(Vec3(i * 1.5, 0.5, 0)));
    }
    add(lamp());
  }

  @override
  void onStart() {
    super.onStart();
    final crates = children.whereType<Crate>().toList(); // typed: they're Crates
    crates.first.hits++;
    find('lamp')?.find('bulb')?.active = false; // by name, below this one
    crates.last.destroy(); // with everything below it
  }
}
```

**A subclass shares `GameObject`'s names.** A method or field of yours
named as one of its own (`rotate`, `find`, `add`, `on`, `wait`,
`tween`, `bounds`, `animator`, `name`, …) is the analyzer's error or
warning, which may name a part of `GameObject` you can't look up:
`isn't a valid override of '_Moving.rotate'` is [Moving things](/basics/moving/)'s `rotate`. Give
yours a name of the game's own (`turnTile`, `label`).

## Adding, destroying and finding

### add(child)

`add(child)` puts an object below this one and returns it, typed: `final
crate = add(Crate(at));`. The world's root is added with `world.add`;
`FynkelApp` does that for you. For a component in a field, `addWith` adds an
object holding it and returns the component: `_sun =
addWith(Light.sun(shadows: true))`.

### destroy()

`destroy()` takes an object and everything below it out of the game. A
field still pointing at it says `isDestroyed`; reading its transform then
throws a clear error instead of giving a stale value. `destroy(after: 0.5)`
destroys it that many seconds of game time later (Unity's `Destroy(go, t)`),
for sparks and shots, without a routine each; hidden or not, and slowed by
`time.scale` and pauses as the game is. Destroying again does nothing.

### active = false

`active = false` hides an object and everything below it: not drawn, not
touched, not updated. `activeInHierarchy` says whether it and all its
parents are on.

### Finding

Finding: `parent`, `children` (or `childCount` and `childAt(i)`, Unity's
`GetChild`, which make no list), `find(name)` below this one,
`ancestor<T>()` the nearest above of a type, and across the whole game
`world.all<T>()`, `world.first<T>()` and `world.only<T>()` (which throws
unless there's exactly one). These find by type, so a game rarely needs
names or tags (the `name` checks in [Triggers and
collisions](/physics/callbacks/) are there to keep the examples short; a
subclass or a component is the sturdier test).

As Unity's, they leave out objects that are turned off (and the components
on them) unless `includeInactive: true`. They do find an object still
loading ([The frame and the lifecycle](/basics/lifecycle/)), as Unity finds
one before its `Awake`: `await object.loaded` before reading what its
`onLoad` sets.

`all` gives a new list, so destroying or releasing what's in it while going
through it is safe.
