---
title: "Character controllers"
description: "Walk a player: slopes, steps, jumps and moving platforms."
---

![A character controller at a course of steps, walls and a ramp](/shots/character.webp)

A `CharacterController` walks a player, as Unity's does. It's a capsule the
game moves with `move(motion)`, and it goes as far as the world lets it.

```dart
// lib/player.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// Walks with W, A, S, D or the arrows, and jumps on space.
class Walk extends Behaviour with ControllerHits {
  late final CharacterController controller = gameObject.get<CharacterController>()!;
  var fall = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    final move = input.axis2('move');
    if (controller.isGrounded) fall = input.action('jump').wasPressed ? 5 : -1;
    fall -= 9.81 * time.delta;
    controller.move(Vec3(move.x * 4, fall, -move.y * 4) * time.delta);
  }

  @override
  void onControllerColliderHit(ControllerColliderHit hit) {
    if (hit.gameObject.name == 'lava') gameObject.position = Vec3(0, 1.1, 0);
  }
}

GameObject player() => GameObject(
  name: 'player',
  transform: Transform(position: Vec3(0, 1.1, 0)),
  components: [MeshRenderer(Mesh.capsule(), Material()), CharacterController(), Walk()],
);
```

## Where it stands

The capsule is `height:` 2 m tall with a `radius:` of 0.5 m unless you say
otherwise. With `center:` zero, the default, the object's position is the
capsule's middle, so the player above starts at 1.1 m. For a model whose
origin is at its feet, give `center: Vec3(0, height / 2, 0)`.

It keeps `skinWidth` (8 cm, as Unity's) between itself and what it meets,
so it stands that far above the ground. Lower the model under it by as much
(a child at `Vec3(0, -0.08, 0)`), or its feet float.

## Walls, steps and slopes

It slides along walls, walks up steps up to `stepOffset` (0.3 m) and slopes
up to `slopeLimit` (`45.deg`), and stops at anything higher or steeper.

`move` returns `CollisionFlags`: `flags.has(CollisionFlags.below)` (the
ground), `.sides` and `.above`. `isGrounded` and `velocity` say how the last
move went.

## Gravity

Gravity is the game's to add, as in Unity and as `Walk` does above, or
`simpleMove(speed)` adds it. While it's grounded, keep a small downward
speed (the `-1` above), so it stays on the ground down a slope.

## What it meets

It pushes the rigidbodies it walks into, as if it weighed `mass` (70 kg; 0
pushes nothing), and meets triggers as a kinematic rigidbody does. `with
ControllerHits` hears each collider a move hits, in
`onControllerColliderHit`.

## Moving platforms

It rides what it stands on. `ground` is that collider, and when it moves or
turns (a platform moved by a tween, say) the controller goes along in the
same frame, unless `moveWithGround` is off. `groundVelocity` says how fast
the ground goes, for a jump off it.
