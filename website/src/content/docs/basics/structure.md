---
title: "Structuring a bigger game"
description: "The root, managers, data in place of subclasses, and starting over."
---

**A bigger game** keeps the same shape. The root object holds what the whole
game shares (the score, the lives: `ValueNotifier`s the HUD shows) and the
managers, each a child. Pass each object what it needs when you make it, the
root or a manager, rather than searching for it. And let data, not subclasses,
tell kinds apart: a runner and a tank are one `Enemy` class given a different
`EnemyKind`.

Put everything that belongs to one level under one object, so starting over is
destroying it and adding a new one ([Levels](/making/levels/)):

```dart
// lib/defense.dart
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';

/// What makes one kind of enemy differ from another: data, not a subclass.
class EnemyKind {
  const EnemyKind({required this.name, required this.speed, required this.health, required this.color});

  final String name;
  final double speed;
  final int health;
  final Color color;

  static const runner = EnemyKind(name: 'runner', speed: 4, health: 1, color: Color(0xFFE0A030));
  static const tank = EnemyKind(name: 'tank', speed: 1, health: 10, color: Color(0xFF708090));
}

/// The root: what the whole game shares, and the level it plays.
class Defense extends GameObject {
  final gold = ValueNotifier(100);
  final lives = ValueNotifier(10);
  late Level level;

  @override
  void onLoad() {
    super.onLoad();
    level = add(Level(this));
  }

  /// Starts over in place: the same root and HUD, a new level.
  void restart() {
    level.destroy(); // its waves, enemies and routines go with it
    level = add(Level(this));
    gold.value = 100;
    lives.value = 10;
  }
}

/// One level: the ground, and the waves that cross it.
class Level extends GameObject {
  Level(this.game)
    : super(
        name: 'level',
        components: [MeshRenderer(Mesh.plane(), Material())],
        transform: Transform(scale: Vec3(20, 1, 20)),
      );

  final Defense game;

  @override
  void onStart() {
    super.onStart();
    startRoutine(() async {
      for (var wave = 1; ; wave++) {
        for (var i = 0; i < 5 + wave; i++) {
          add(Enemy(i.isEven ? EnemyKind.runner : EnemyKind.tank, game));
          await wait(0.8);
        }
        await wait(5);
      }
    });
  }
}

class Enemy extends GameObject {
  Enemy(this.kind, this.game)
    : super(
        name: kind.name,
        transform: Transform(position: Vec3(-10, 0.5, 0)),
        components: [MeshRenderer(Mesh.cube(), Material(color: kind.color))],
      );

  final EnemyKind kind;
  final Defense game;
  late int health = kind.health;

  @override
  void onUpdate() {
    super.onUpdate();
    translate(Vec3(kind.speed * time.delta, 0, 0), relativeTo: Space.world);
    if (position.x > 10) {
      game.lives.value--;
      destroy();
    }
  }
}
```
