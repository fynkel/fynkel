---
title: "Fighting"
description: "Health, shots, a blast, waves and floating numbers."
---

## Health, and a flash when hit

```dart
// lib/recipes/hearts.dart
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';

class Hearts extends Behaviour {
  Hearts(this.max) : left = ValueNotifier(max);

  final int max;
  final ValueNotifier<int> left;
  final died = Signal<void>();

  void hit(int damage) {
    if (left.value == 0) return;
    left.value = (left.value - damage).clamp(0, max);
    if (left.value == 0) {
      died.emit();
      gameObject.destroy();
      return;
    }
    startRoutine(() async {
      final renderer = gameObject.get<MeshRenderer>();
      renderer?.color = Color.white;
      await wait(0.08);
      renderer?.color = null; // the material's own again
    });
  }
}
```

## A health bar over an enemy

```dart
// lib/recipes/health_bar.dart
import 'package:flutter/material.dart' show Alignment, Colors, LinearProgressIndicator, SizedBox, ValueListenableBuilder;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'hearts.dart';

WorldWidget healthBar(Hearts hearts) => WorldWidget(
  (context) => ValueListenableBuilder(
    valueListenable: hearts.left,
    builder: (context, left, _) => SizedBox(
      width: 48,
      child: LinearProgressIndicator(value: left / hearts.max, color: Colors.green, backgroundColor: Colors.black54),
    ),
  ),
  offset: Vec3(0, 1.2, 0),
  alignment: Alignment.bottomCenter,
);

GameObject enemy() {
  final hearts = Hearts(5);
  return GameObject(
    components: [MeshRenderer(Mesh.capsule(), Material(color: Color(0xFFB04040))), hearts, healthBar(hearts)],
  );
}
```

## Shoot, and hit at once

A ray from the gun, and what it meets loses health.

```dart
// lib/recipes/shoot.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import 'hearts.dart';

class Gun extends Behaviour {
  var _ready = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    _ready -= time.delta;
    if (_ready > 0 || !input.isPressed(KeyCode.f)) return;
    _ready = 0.15; // seconds between shots
    final hit = physics.raycast(Ray(position, forward), maxDistance: 50);
    hit?.collider.gameObject.getInParent<Hearts>()?.hit(1);
    debug.line(position, hit?.point ?? position + forward * 50, seconds: 0.05);
  }
}
```

## A blast

```dart
// lib/recipes/blast.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import 'hearts.dart';

void blast(GameObject from, {double radius = 4}) {
  for (final collider in from.physics.overlapSphere(from.position, radius)) {
    collider.gameObject.getInParent<Rigidbody>()?.addExplosionForce(
      8,
      from.position,
      radius,
      upwardsModifier: 0.5,
      mode: ForceMode.impulse,
    );
    collider.gameObject.getInParent<Hearts>()?.hit(2);
  }
  from.world.cameras.shake(0.4);
}
```

## Waves

```dart
// lib/recipes/waves.dart
import 'dart:math';

import 'package:fynkel/fynkel.dart';

import 'health_bar.dart';

class Waves extends GameObject {
  final _random = Random(1);

  @override
  void onStart() {
    super.onStart();
    startRoutine(() async {
      for (var wave = 1; ; wave++) {
        final group = add(GameObject(name: 'wave $wave'));
        for (var i = 0; i < wave * 3; i++) {
          final angle = _random.nextDouble() * 2 * pi;
          group.add(enemy()).position = Vec3(cos(angle) * 10, 1, sin(angle) * 10);
          await wait(0.3);
        }
        await waitUntil(() => group.children.isEmpty); // all gone
        group.destroy();
        await wait(3);
      }
    });
  }
}
```

## A damage number

```dart
// lib/recipes/damage_number.dart
import 'package:flutter/material.dart' show Colors, FontWeight, Text, TextStyle;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void damageNumber(World world, Vec3 at, int damage) {
  final number = world.add(
    GameObject(
      transform: Transform(position: at),
      components: [
        WorldWidget(
          (context) => Text(
            '$damage',
            style: const TextStyle(color: Colors.amber, fontSize: 24, fontWeight: FontWeight.bold),
          ),
        ),
      ],
    ),
  );
  number.moveBy(Vec3(0, 1, 0), seconds: 0.8);
  number.destroy(after: 0.8);
}
```
