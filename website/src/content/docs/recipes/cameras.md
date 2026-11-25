---
title: "Cameras"
description: "Third person, top down, a shake, and tapping the ground."
---

## Behind the player

The player turns it with a drag and zooms with a pinch or the wheel, and it
keeps out of walls.

```dart
// lib/recipes/third_person.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

GameObject thirdPerson(GameObject player) =>
    GameObject(components: [Camera(), Orbit(player, distance: 6, offset: Vec3(0, 1.5, 0)), CameraCollision()]);
```

## From above

```dart
// lib/recipes/top_down.dart
import 'package:fynkel/fynkel.dart';

GameObject topDown(GameObject player) =>
    GameObject(components: [Camera(), Follow(player, offset: Vec3(0, 12, 7), damping: 0.2), LookAt(player)]);
```

## Through the player's eyes

Drag to look around.

```dart
// lib/recipes/first_person.dart
import 'package:fynkel/fynkel.dart';

class Eyes extends Behaviour {
  Eyes(this.player);

  final GameObject player;
  var _yaw = 0.0, _pitch = 0.0;

  @override
  void onStart() {
    super.onStart();
    on(input.onDrag, (finger) {
      _yaw -= finger.delta.x * 0.005;
      _pitch = (_pitch - finger.delta.y * 0.005).clamp(-1.4, 1.4);
    });
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    position = player.position + Vec3(0, 0.7, 0);
    rotation = Quat.euler(_pitch, _yaw, 0);
  }
}

GameObject firstPerson(GameObject player) => GameObject(components: [Camera(), Eyes(player)]);
```

## Shake when hit

```dart
// lib/recipes/shake.dart
import 'package:fynkel/fynkel.dart';

void bump(World world) => world.cameras.shake(0.3, seconds: 0.25);
```

## Go where the player taps

```dart
// lib/recipes/tap_to_move.dart
import 'package:fynkel/fynkel.dart';

class TapToMove extends Behaviour {
  Vec3? _goal;

  @override
  void onStart() {
    super.onStart();
    on(input.onTap, (finger) => _goal = world.camera?.screenPointToPlane(finger.position, Plane.ground));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final goal = _goal?.withY(position.y);
    if (goal != null) position = Vec3.moveTowards(position, goal, 4 * time.delta);
  }
}
```
