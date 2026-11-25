---
title: "Rigidbodies"
description: "Objects that fall, roll and are pushed."
---

A `Rigidbody` makes its object move with physics, as Unity's: give it a
collider and it falls, tumbles and lands.

```dart
// lib/ball.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// Rolls its ball with W, A, S, D or the arrows, and jumps on space.
class Roll extends Behaviour {
  late final Rigidbody body = gameObject.get<Rigidbody>()!;

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    final move = input.axis2('move');
    body.addForce(Vec3(move.x, 0, -move.y) * 10);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (input.action('jump').wasPressed) body.addForce(Vec3(0, 5, 0), mode: ForceMode.impulse);
  }
}

GameObject ball() => GameObject(
  transform: Transform(position: Vec3(0, 3, 0)),
  components: [
    MeshRenderer(Mesh.sphere(), Material()),
    SphereCollider(material: PhysicsMaterial(bounciness: 0.5)),
    Rigidbody(mass: 2, interpolation: RigidbodyInterpolation.interpolate),
    Roll(),
  ],
);
```

Colliders on its children that have no rigidbody of their own are part of
the same body, and the body's mass is spread over them.

## Settings

| | |
| --- | --- |
| `mass` | In kilograms |
| `useGravity` | Whether it falls |
| `isKinematic` | Moved by the game alone: it pushes, and nothing pushes it |
| `linearDamping`, `angularDamping` | How fast it slows down by itself |
| `linearVelocity`, `angularVelocity` | How it's moving |
| `centerOfMass`, `maxAngularVelocity` | As Unity's |
| `constraints` | What it can't do: `RigidbodyConstraints.freezeRotation \| .freezePositionY` |
| `interpolation` | `RigidbodyInterpolation.interpolate` draws it smoothly between steps |
| `collisionDetectionMode` | `continuous` for small, fast bodies |

## Forces

Push it with `addForce`, `addTorque`, `addRelativeForce`,
`addForceAtPosition` and `addExplosionForce`, in `onFixedUpdate`. Each
takes a `ForceMode`:

| `ForceMode` | What it is |
| --- | --- |
| `force` | Newtons over the next step, the default |
| `acceleration` | The same, whatever its mass |
| `impulse` | Newton seconds, at once: a kick or a jump |
| `velocityChange` | The same, whatever its mass |

`addExplosionForce(force, position, radius, upwardsModifier:, mode:)` pushes
the body away from `position`: `force` strong there, less in a straight line
out to nothing at `radius` (the same everywhere with 0). `upwardsModifier`
puts the blast that far below, so it lifts too. Call it on each body in
reach (`physics.overlapSphere`).

## Moving bodies

As in Unity, **setting a body's transform teleports it**. To move it through
the world use forces, or `movePosition` and `moveRotation` for a kinematic
body, which pushes what it meets. A body's object can have a parent: physics
writes its pose back relative to it.

### Smooth between steps

With `RigidbodyInterpolation.interpolate`, what's drawn is between the
body's last two steps, so it moves smoothly at 120 fps on 50 steps a second;
`rigidbody.position` reads where physics has it.

Queries (raycasts, overlaps) and character controllers meet the body there
too, where the last step left it, up to a step ahead of what's drawn, as in
Unity. A controller riding an interpolated platform (`moveWithGround`) goes
with the platform's steps.

### Fast bodies

A small, fast body can pass through a thin one between steps. Every fast
body is swept against static colliders, and
`CollisionDetectionMode.continuous` sweeps it against bodies too.

### The same every time

**The same steps give the same world**: the same game, pushed the same way,
ends with its bodies in the same places to the bit, at 60 fps or 120 (the
fixed step hides the frame rate), so a test can count on it. Across devices
or engine versions there is no promise.

### The world's settings

`physics.gravity`, `defaultSolverIterations` and `sleepThreshold` are the
world's. A new `gravity` wakes every body, so sleeping ones fall the new
way. `physics.rigidbodyCount` and `colliderCount` count what's in the world,
for a HUD or a test.

`isSleeping` says whether a body sleeps. `sleep()` and `wakeUp()` do nothing
for a body not in the world yet, and what can't be done there says why (no
object yet, not enabled, not shown, destroyed).

## Platforms and doors

Move a kinematic body (a platform, a door, a bullet that's a trigger):
`movePosition` and `moveRotation`, in `onFixedUpdate` or from a finger's event
([dragging a body](/physics/bullets-and-dragging/#dragging-a-body)): called
more than once before a step, the last one counts. It goes there over the next
step, so it pushes what's in the way, carries what stands on it, and is drawn
smoothly when interpolated.

Setting `position` (or the transform) teleports it, as in Unity: right for a
respawn, wrong for a move. Either way a trigger sees only what it overlaps at
the end of each step, so a thing that moves further than its own size in a
step can pass something by; see [bullets](/physics/bullets-and-dragging/).

## When the step runs

**The step runs between frames**, on the physics' own thread: it starts when a
frame is done, and its results are written at the start of the next, before
`onFixedUpdate` and the updates. Queries in the updates never wait for it, and
see the world as the last step left it (an interpolated body's drawn pose
trails that by up to a step).

The costs: a force shows a frame later, and `await fixedStep` resumes before
that step has run. `physics.stepBetweenFrames = false` steps within the frame
instead, as Unity does.

A Flutter widget that reads a body as it builds (`body.linearVelocity` for a
speedometer) waits for the step running then, giving back the time the
thread saved. So read it in the game, in `onUpdate`, and hand the
widget the number through a `ValueNotifier`. The views and world widgets
never wait: they read the objects' transforms, which the step's results
were written to at the start of the frame.
