---
title: "Your own systems and packages"
description: "Work done once a frame over many objects, and sharing it."
---

A game adds to Fynkel in three ways:
- **a behaviour** ([Behaviours](/basics/behaviours/)), for what one object does;
- **a shader** ([A first shader](/shaders/first/)), for how something looks;
- **a world system**, for work done once a frame over many objects:
  physics, sound, a wind, a flock, a day's clock.

Physics and sound are packages built as systems. So are the engine's
own animators, particles and cameras. A system is where Unity would have
a manager object, or a system added to its player loop. It's in
`package:fynkel/extend.dart`, which a game itself doesn't need.

```dart
// lib/wind.dart
import 'dart:math' as math;

import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';

/// The world's wind: everything with a Sway leans with it.
final class Wind extends WorldSystem {
  Wind(super.world);

  /// How far it leans what sways, at most.
  double strength = 10.deg;

  final _swaying = <Sway>[];

  @override
  String get name => 'wind';

  @override
  void afterUpdate() {
    super.afterUpdate();
    final t = world.time.sinceStart;
    for (var i = 0; i < _swaying.length; i++) {
      _swaying[i]._lean(t, strength);
    }
  }
}

/// `world.wind`, `gameObject.wind` and a behaviour's `wind`, made the
/// first time they're asked for.
extension WorldWind on World {
  Wind get wind => system(Wind.new);
}

extension GameObjectWind on GameObject {
  Wind get wind => world.wind;
}

extension BehaviourWind on Behaviour {
  Wind get wind => world.wind;
}

/// Leans its object with the wind, around where it stood.
class Sway extends Behaviour {
  Sway({this.phase = 0});

  /// Where in the gust it is, so a field doesn't sway as one.
  final double phase;

  Quat _rest = Quat.identity;

  @override
  void onEnable() {
    super.onEnable();
    _rest = localRotation;
    wind._swaying.add(this);
  }

  @override
  void onDisable() {
    wind._swaying.remove(this);
    localRotation = _rest;
    super.onDisable();
  }

  void _lean(double t, double strength) {
    final angle = strength * (0.6 + 0.4 * math.sin(t * 1.7 + phase));
    localRotation = _rest * Quat.axisAngle(Vec3.back, angle);
  }
}
```

```dart
// test/wind_test.dart
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:my_game/wind.dart';

void main() {
  test('what sways leans with the wind, and stands up when it stops', () {
    final world = World();
    final grass = GameObject(components: [Sway()]);
    world.add(grass);
    world.run(1);
    expect(grass.localRotation, isNot(Quat.identity));
    expect(world.profile.systems.map((s) => s.name), contains('wind'));

    grass.get<Sway>()!.enabled = false;
    expect(grass.localRotation, Quat.identity);
    world.dispose();
  });
}
```

### One of each per world

`world.system(Wind.new)` makes it the first time
and gives the same one after. A getter in an extension, as above, is how
physics gives `world.physics` and sound `world.audio`.

### Its hooks run in the frame

Its hooks run in the frame ([The frame and the
lifecycle](/basics/lifecycle/)), whether paused or not. In order:
- `beforeFrame`;
- `beforeFixedUpdate` and `afterFixedUpdate` round each fixed step;
- `beforeUpdate`;
- `afterUpdate`, after the objects' updates, where the animators pose;
- `afterLateUpdate`, before the cameras move;
- `afterFrame`.

Override the ones you need, and call `super`. `order` puts it before
(lower) or after (higher) other systems. `name` is what `world.profile`
shows, with the time its hooks took: `system.frameTime` on the system,
and `time` in each of `world.profile.systems`' `(name:, time:)`.

### Its components are behaviours

One joins its system in `onEnable`
and leaves in `onDisable`, as a collider does. So one that's off, or
on an object that's off, costs the system nothing. To watch components
that aren't its own, a system overrides `componentAdded` and
`componentRemoved`. Physics hears of `with Triggers` that way.

### What it throws goes to world.errors

What it throws goes to `world.errors`, and the frame goes on. A
system whose hooks throw 5 frames in a row is turned off
(`isTurnedOff`). Code of the game's that a system calls (a callback)
reports with `world.report(object, error, stack)`, so one object's
bug doesn't stop the rest.

### Dispose

`dispose` is called once, when the world is. A game may keep the
system past it, so its public calls should throw a `StateError` after.

### Following objects without visiting them all

`world.movedReader()`
lists those whose transform was set since it last read, and
`world.transformVersion` says whether any was.
`readWorldTransforms` reads many in one call; `writeTransforms` sets
many from native code. `componentCount`/`componentAt` and
`childCount`/`childAt` walk an object without making lists.

### Files of its own

An `AssetKind` loads them through `assets`, shared
and freed as models are. `world.addAssetKind` lets `assets.preload`
load them by their ending.

### Taps

`pointerHit(ray, layers)` makes what the system holds
tappable ([Keys, mouse and touch](/input/reading/)) though it has no mesh. Physics answers with its
colliders.

## Making it a package

Move `wind.dart` into a Dart package of its own
that depends on `fynkel`, export it from the package's library, and the
game imports that. Its tests are the same.

A package whose code is in Rust, as physics and sound are, isn't covered
yet ([What works today](/start/what-works/)).
