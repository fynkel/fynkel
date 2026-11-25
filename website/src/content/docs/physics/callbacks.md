---
title: "Triggers and collisions"
description: "Hear when things touch, and which layers meet."
---

A mixin asks for the callbacks, on an object or a behaviour:

| Mixin | Callbacks |
| --- | --- |
| `with Triggers` | `onTriggerEnter`, `onTriggerStay`, `onTriggerExit(Collider other)` |
| `with Collisions` | `onCollisionEnter`, `onCollisionStay`, `onCollisionExit(Collision collision)` |

A **trigger** is a collider with `isTrigger: true`: things pass through it
and it says so. It needs a rigidbody (kinematic will do) or a character
controller on one side. A **collision** needs a dynamic rigidbody on one
side. As in Unity, callbacks come to a collider's object and to its
rigidbody's object.

```dart
// lib/pickups.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A coin the player picks up by touching it.
class Coin extends GameObject with Triggers {
  Coin(Vec3 at)
    : super(
        transform: Transform(position: at),
        components: [MeshRenderer(Mesh.sphere(), Material()), SphereCollider(isTrigger: true)],
      );

  @override
  void onTriggerEnter(Collider other) {
    if (other.gameObject.has<CharacterController>()) destroy(); // the player walked into it
  }
}

/// Plays a thud when its object lands hard.
class Thud extends Behaviour with Collisions {
  @override
  void onCollisionEnter(Collision collision) {
    if (collision.relativeVelocity.length > 3) print('thud on ${collision.gameObject.name}');
  }
}
```

## When they come

- **Enter** comes the step two colliders start to touch.
- **Stay** comes every step after, while either body is awake. A character
  controller counts as awake, standing still too.
- **Exit** comes the step they part, or after one is disabled, removed,
  destroyed, hidden, put on a layer that doesn't meet, or switched between
  trigger and solid (Unity skips those; here the other side always hears).

They come at the start of the next frame, before `onFixedUpdate`. So a body
read in `onCollisionEnter` is where the step left it, after the touch: a box
landing at 6 m/s is a few centimetres past where it first met the other, as
in Unity. `collision`'s contacts say where they met.

A behaviour gets callbacks while it's disabled too, as in Unity. What a
callback throws goes to `world.errors`.

## Once per pair

Both objects in a collision get it, each once: a ball and the wall it hits
each hear `onCollisionEnter`, with the other in `collision`. To act once per
pair, let one side do it: say the one with the lower `gameObject.id` (every
object's own number, unique in its world).

## What a collision tells

A `Collision` has the other side's `collider`, `gameObject` and `rigidbody`,
the `impulse` that pushed this side, the `relativeVelocity`, and the points
they touch at (`contactCount`, `getContact(i)`, `contacts`), each with a
`point`, a `normal` out of the other side, and a `separation`. It's the same
object every time: read it in the callback.

## Layers that don't meet

Layers that don't meet pass through each other:
`physics.ignoreLayerCollision(ghosts, 0)` lets everything on the
`ghosts` layer fall through layer 0, from the next step, and
`physics.ignoreCollision(a, b)` does the same for two colliders until one
leaves the world. Queries don't read the matrix; their `layers:` say what
they hit.
