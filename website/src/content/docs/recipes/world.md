---
title: "The world"
description: "Spawning, pick-ups, doors, day and night, and slow motion."
---

## Scatter trees

One mesh and one material, shared: hundreds are cheap.

```dart
// lib/recipes/scatter.dart
import 'dart:math';

import 'package:fynkel/fynkel.dart';

class Forest extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    final random = Random(7); // the same forest every time
    final green = Material(color: Color(0xFF3C7A3A)), brown = Material(color: Color(0xFF6B4A2E));
    for (var i = 0; i < 60; i++) {
      final at = Vec3(random.nextDouble() * 40 - 20, 0, random.nextDouble() * 40 - 20);
      final tree = add(GameObject(transform: Transform(position: at, scale: Vec3.all(0.7 + random.nextDouble() * 0.6))));
      tree.add(
        GameObject(
          transform: Transform(position: Vec3(0, 1, 0), scale: Vec3(0.3, 1, 0.3)),
          components: [MeshRenderer(Mesh.cylinder(), brown)],
        ),
      );
      tree.add(
        GameObject(
          transform: Transform(position: Vec3(0, 2.5, 0), scale: Vec3.all(2)),
          components: [MeshRenderer(Mesh.sphere(), green)],
        ),
      );
    }
  }
}
```

## Pick something up

```dart
// lib/recipes/pick_up.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

class Gem extends GameObject with Triggers {
  Gem({required this.onTaken})
    : super(
        components: [
          MeshRenderer(Mesh.sphere(), Material(color: Color(0xFF40C0E0), roughness: 0.2)),
          SphereCollider(isTrigger: true),
        ],
      );

  final void Function() onTaken;
  var _taken = false;

  @override
  void onTriggerEnter(Collider other) {
    if (_taken || !other.gameObject.has<CharacterController>()) return;
    _taken = true;
    onTaken();
    startRoutine(() async {
      await scaleTo(Vec3.all(0.01), seconds: 0.2);
      destroy();
    });
  }
}
```

## A door that opens when someone is near

```dart
// lib/recipes/sliding_door.dart
import 'package:fynkel/fynkel.dart';

class SlidingDoor extends GameObject {
  SlidingDoor(this.player)
    : super(
        transform: Transform(scale: Vec3(1.2, 2.2, 0.15)),
        components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFF8A6A4A)))],
      );

  final GameObject player;
  late final Vec3 _shut = localPosition;
  var _open = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    final near = (player.position - _shut).length < 3;
    _open = moveTowards(_open, near ? 1 : 0, 2 * time.delta);
    localPosition = _shut + Vec3(1.2 * _open, 0, 0);
  }
}
```

## Day and night

```dart
// lib/recipes/day.dart
import 'dart:math';

import 'package:fynkel/fynkel.dart';

class Day extends GameObject {
  Day({this.seconds = 60}) : super(components: [Light.sun()]);

  final double seconds;

  @override
  void onUpdate() {
    super.onUpdate();
    final angle = time.sinceStart / seconds * 2 * pi; // 0 is sunrise
    final height = sin(angle);
    lookAt(position - Vec3(cos(angle), height, 0.3));
    get<Light>()!
      ..intensity = max(0, height) * 2
      ..color = Color.lerp(Color.rgb(1, 0.6, 0.35), Color.white, max(0, height));
  }
}
```

## Slow motion for a moment

```dart
// lib/recipes/slow_motion.dart
import 'package:fynkel/fynkel.dart';

class SlowMotion extends Behaviour {
  Future<void> play() => startRoutine(() async {
    time.scale = 0.2;
    await wait(0.6, unscaled: true); // in real seconds
    time.scale = 1;
  });
}
```

## Keep the best score

```dart
// lib/recipes/best_score.dart
import 'dart:io';

import 'package:path_provider/path_provider.dart';

/// `final best = await BestScore.open();` in main, before runApp.
class BestScore {
  BestScore._(this._file, this.value);

  final File _file;
  int value;

  static Future<BestScore> open() async {
    final file = File('${(await getApplicationSupportDirectory()).path}/best.txt');
    return BestScore._(file, file.existsSync() ? int.tryParse(file.readAsStringSync()) ?? 0 : 0);
  }

  void offer(int score) {
    if (score <= value) return;
    value = score;
    _file.parent.createSync(recursive: true);
    _file.writeAsStringSync('$score');
  }
}
```
