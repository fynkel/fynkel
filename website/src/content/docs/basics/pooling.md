---
title: "Pooling"
description: "Reuse shots, sparks and enemies instead of making them anew."
---

A game that makes and destroys many short-lived objects
(shots, sparks, enemies of a wave) can reuse them instead, with a
`Pool`, as Unity's `ObjectPool`: `get` hands out a hidden object or makes
a new one, and `release` hides it again for the next `get`.

```dart
class Shot extends GameObject {
  Shot(Material material) : super(components: [MeshRenderer(Mesh.sphere(), material)]);

  var speed = 0.0;
  var flown = 0.0;

  @override
  void onEnable() {
    super.onEnable();
    flown = 0; // each time it's handed out
  }

  @override
  void onUpdate() {
    super.onUpdate();
    translate(Vec3.forward * (speed * time.delta));
    flown += time.delta;
    if (flown > 2) release(); // back to its pool
  }
}

class Turret extends GameObject {
  final shotMaterial = Material(color: const Color(0xFFFFD040));
  late final shots = Pool(this, () => Shot(shotMaterial));
  ObjectHandle<Shot>? lastShot;

  void fire() {
    final shot = shots.get((shot) {
      shot.position = position;
      shot.rotation = rotation;
      shot.speed = 12;
    });
    lastShot = ObjectHandle(shot);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (lastShot?.object case final shot?) debug.line(position, shot.position);
  }
}
```

`countActive` and `countInactive` say how many are out and waiting;
`prewarm(n)` makes them ahead, so play doesn't make (and load) them: a new
one `get` makes shows from the next frame, once it's loaded, where a waiting
one shows at once; `activeAt(i)` walks the ones out without making a list
(walk from the end to release as you go), and `active` gives a copy;
`clear()` destroys the waiting ones; with `maxSize`, a release beyond that
many waiting destroys.

Hidden objects aren't in `world.all`, so a wave's "are they all gone" works
the same with a pool. Getting and releasing allocates nothing.

### Reset in get's function, or in onEnable

The function runs first,
then the object is shown, so its `onEnable` sees what the function set. A
new object is made by the pool's function and added below the pool's parent.

### A released object's particles and trail go

Hidden, its particle systems
stop and clear, and its trail is cleared, at the next particles step; shown
again, they start afresh. One released and got again in the same frame never
sees that step: call `trail.clear()` (and the particles' `clear()`) in
`get`'s function, so it doesn't streak from where it was.

### A released object starts clean

It's hidden, the routines and motions it
and everything below it ran are stopped (a hidden object's would only pause,
and resume stale), what they listened to with `on` while shown (from
`onEnable` on) is cancelled, and it's back below the pool's parent. So
listen in `onEnable`, and each `get` listens once again; what it listened to
in `onLoad` stays, through every reuse. Its fields are the game's.

### release() on the object

`release()` on the object gives it back to its pool, or destroys it if
it didn't come from one: write it where you'd write `destroy()`. As
`destroy()`, releasing again does nothing (a shot that hits two enemies in
one step), and `release(after: 2)` releases it later, unless it's released
sooner: its next use isn't cut short.

### Hold pooled objects with an ObjectHandle

A field pointing at a shot
still points at it once it's released and handed out again as a new shot;
`ObjectHandle(shot).object` is null from the release on (and once it's
destroyed).
