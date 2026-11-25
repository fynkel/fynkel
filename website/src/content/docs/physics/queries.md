---
title: "Raycasts and overlaps"
description: "Ask what a ray, a sphere or a box meets."
---

Queries are on `physics`, which every object and behaviour has. They take
what Unity's take, in its order:

| Query | Arguments |
| --- | --- |
| `raycast`, `raycastAll` | `ray, maxDistance:` |
| `sphereCast`, `sphereCastAll` | `ray, radius, maxDistance:` |
| `boxCast`, `boxCastAll` | `center, halfExtents, direction, orientation:, maxDistance:` |
| `capsuleCast`, `capsuleCastAll` | `point1, point2, radius, direction, maxDistance:` (the points are the centres of its ends) |
| `overlapSphere`, `overlapBox`, `overlapCapsule` | The shape: every collider in it |
| `checkSphere`, `checkBox`, `checkCapsule` | The shape: whether anything is in it |

The `…All` casts give their hits nearest first. Each query takes `layers:`
and `triggers:` (`QueryTriggers.ignore` goes through triggers), and sees
colliders where their objects are now, moved this frame or not. A `Ray`'s
direction is made unit length for you.

```dart
// lib/sight.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

const walls = 2;

/// Sees the player when nothing on the walls layer is in between.
class Sight extends Behaviour {
  Sight(this.player);

  final GameObject player;
  var seesPlayer = false;

  @override
  void onUpdate() {
    super.onUpdate();
    final to = player.position - gameObject.position;
    final hit = physics.raycast(
      Ray(gameObject.position, to),
      maxDistance: to.length,
      layers: LayerMask.of([walls]),
      triggers: QueryTriggers.ignore,
    );
    seesPlayer = hit == null;
  }
}

/// A wall: a cube with a box collider, on the walls layer.
GameObject wall(Vec3 at) => GameObject(
  layer: walls,
  transform: Transform(position: at, scale: Vec3(4, 2, 0.2)),
  components: [MeshRenderer(Mesh.cube(), Material()), BoxCollider()],
);
```

## What a hit tells

A hit is a `RaycastHit`, as Unity's: `point`, `normal`, `distance`,
`collider`, `gameObject` and `rigidbody`. A ray doesn't hit what it starts
inside, and a query that misses allocates nothing.

A cast's `distance` is how far its shape's centre went before it touched (so
a ball's centre is at `ray.origin + direction * distance` then), and `point`
is on the surface it touched. A shape that starts touching or inside a
collider doesn't hit it, as in Unity: start a ball's cast a hair above the
ground it sits on.

The lists the `…All` casts and the overlaps give are read-only, hit or miss:
`.toList()` to sort or change one.

## Other ways to ask

- `collider.raycast(ray)` and `collider.closestPoint(point)` ask one collider.
- `world.raycast` finds what's drawn, with no colliders. Taps find both
  ([Keys, mouse and touch](/input/reading/)).
- `physics.drawColliders = true` draws every collider with the debug lines.
