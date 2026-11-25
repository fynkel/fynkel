---
title: "Moving about"
description: "Walk, jump, dash, patrol and chase."
---

Each recipe is a file to copy: the code is compiled and run with every
change to Fynkel.

## Walk relative to the camera

W means "away from the camera", whichever way that is.

```dart
// lib/recipes/walk.dart
import 'package:fynkel/fynkel.dart';

class WalkByCamera extends Behaviour {
  WalkByCamera({this.speed = 4});

  final double speed;

  @override
  void onUpdate() {
    super.onUpdate();
    final camera = world.camera?.gameObject;
    if (camera == null) return;
    final move = input.axis2('move');
    final way = (camera.right.flat * move.x + camera.forward.flat.normalizedOrZero * move.y).normalizedOrZero;
    if (way == Vec3.zero) return;
    position += way * (speed * time.delta);
    rotation = Quat.rotateTowards(rotation, Quat.lookRotation(way), 10 * time.delta); // turn to face it
  }
}
```

## Jump, with a little kindness

A jump that counts if it's pressed just before landing, or just after
walking off an edge.

```dart
// lib/recipes/jump.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

class Jumper extends Behaviour {
  late final CharacterController controller = gameObject.get<CharacterController>()!;
  var _fall = 0.0, _sinceGround = 1.0, _sincePress = 1.0;

  @override
  void onUpdate() {
    super.onUpdate();
    _sinceGround = controller.isGrounded ? 0 : _sinceGround + time.delta;
    _sincePress = input.action('jump').wasPressed ? 0 : _sincePress + time.delta;
    if (controller.isGrounded && _fall < 0) _fall = -1;
    if (_sincePress < 0.1 && _sinceGround < 0.1) {
      _fall = 6;
      _sincePress = _sinceGround = 1;
    }
    _fall -= 15 * time.delta;
    final move = input.axis2('move');
    controller.move(Vec3(move.x * 4, _fall, -move.y * 4) * time.delta);
  }
}

GameObject jumper() => GameObject(
  transform: Transform(position: Vec3(0, 1.1, 0)),
  components: [MeshRenderer(Mesh.capsule(), Material()), CharacterController(), Jumper()],
);
```

## A third-person player

Both of those: it walks as the camera sees it, turns to face the way it goes,
and falls. Its camera is in [Cameras](/recipes/cameras/#behind-the-player).

```dart
// lib/recipes/third_person_walk.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

class ThirdPersonWalk extends Behaviour {
  late final CharacterController controller = gameObject.get<CharacterController>()!;
  var _fall = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    final camera = world.camera?.gameObject;
    if (camera == null) return;
    final move = input.axis2('move');
    final way = (camera.right.flat * move.x + camera.forward.flat.normalizedOrZero * move.y).normalizedOrZero;
    if (way != Vec3.zero) rotation = Quat.rotateTowards(rotation, Quat.lookRotation(way), 10 * time.delta);
    if (controller.isGrounded) _fall = input.action('jump').wasPressed ? 6 : -1;
    _fall -= 15 * time.delta;
    controller.move((way * 4 + Vec3(0, _fall, 0)) * time.delta);
  }
}

GameObject thirdPersonPlayer() => GameObject(
  transform: Transform(position: Vec3(0, 1.1, 0)),
  components: [MeshRenderer(Mesh.capsule(), Material()), CharacterController(), ThirdPersonWalk()],
);
```

## Keep it on the island

```dart
// lib/recipes/stay_near.dart
import 'package:fynkel/fynkel.dart';

class StayNear extends Behaviour {
  StayNear(this.centre, this.radius);

  final Vec3 centre;
  final double radius;

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    final from = (position - centre).flat;
    if (from.length > radius) position = (centre + from.normalized * radius).withY(position.y);
  }
}
```

## Patrol between points

```dart
// lib/recipes/patrol.dart
import 'package:fynkel/fynkel.dart';

class Patrol extends Behaviour {
  Patrol(this.points, {this.speed = 2});

  final List<Vec3> points;
  final double speed;
  var _next = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    final goal = points[_next];
    position = Vec3.moveTowards(position, goal, speed * time.delta);
    if ((goal - position).length < 0.01) {
      _next = (_next + 1) % points.length;
    } else {
      lookAt(goal);
    }
  }
}
```

## Chase when near

```dart
// lib/recipes/chase.dart
import 'package:fynkel/fynkel.dart';

class Chase extends Behaviour {
  Chase(this.target, {this.sees = 8, this.speed = 2.5});

  final GameObject target;
  final double sees, speed;

  @override
  void onUpdate() {
    super.onUpdate();
    if (target.isDestroyed) return;
    final to = (target.position - position).flat;
    if (to.length > sees || to.length < 1) return;
    lookAt(position + to);
    position += to.normalized * (speed * time.delta);
  }
}
```

## Bob and turn, as a coin does

```dart
// lib/recipes/bob.dart
import 'dart:math';

import 'package:fynkel/fynkel.dart';

class Bob extends Behaviour {
  late final double _y = localPosition.y;

  @override
  void onUpdate() {
    super.onUpdate();
    localPosition = localPosition.withY(_y + sin(time.sinceStart * 3) * 0.15);
    rotate(Vec3.up, 2 * time.delta);
  }
}
```
