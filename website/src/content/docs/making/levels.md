---
title: "Levels"
description: "Swap levels, fade between them, start over."
---

**Starting a level over** inside the game (a "try again" button) needs
no new world: destroy the object the level hangs from and add a new one,
as `Defense.restart` does in [Structuring a bigger game](/basics/structure/). Everything below it goes at once, with
its `onDestroy`, its routines, tweens and listeners; what the root keeps
(the score, the HUD's notifiers, the settings) stays. `FynkelGame`'s
`restart()` is the bigger hammer: a new root in a new world.

## Levels as objects

**Levels.** There are no scene files: a level is an object, and what it
adds hangs from it. The root holds one at a time, and what outlives them:

```dart
// lib/levels.dart
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';

/// One level, made in Blender. Destroying it clears it: its
/// objects, its routines, and the assets only it held.
class Level extends GameObject {
  Level(this.number) : super(name: 'level $number');

  static const last = 3;
  final int number;

  /// Where the player starts: the empty named `start` in Blender.
  var start = Vec3.zero;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final scene = add(await assets.instantiate('levels/level_$number.glb'));
    start = scene.find('start')?.worldTransform.position ?? Vec3.zero;
    // The next one loads while this one is played, so it comes at once.
    if (number < last) assets.preload(['levels/level_${number + 1}.glb']);
  }
}

/// The game's root.
class Levels extends GameObject {
  Levels({this.first = 1});

  final int first;
  final level = ValueNotifier(0);
  Level? _current;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await go(first);
  }

  /// Swaps the level there is for level [number]. The new one comes
  /// whole before the old one goes: no empty frame, and what both use
  /// stays loaded.
  Future<void> go(int number) async {
    final old = _current;
    final next = _current = add(Level(number));
    await next.loaded;
    old?.destroy();
    level.value = number;
  }
}
```

## A fade between levels

A fade between levels waits, and `wait` works only in a routine
([Routines](/basics/routines/)): a plain `async` method that calls it throws "wait(...) works only
inside a routine". Make `go` start one, and let the game's Flutter side
show `fading` as a black panel over the view ([A HUD over the game](/flutter/hud/)):

```dart
// lib/fading_levels.dart
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';

import 'levels.dart' show Level;

/// [Levels] with a fade: dark, swap, light again.
class FadingLevels extends GameObject {
  final fading = ValueNotifier(false);
  Level? _current;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await go(1);
  }

  Future<void> go(int number) => startRoutine(() async {
    if (_current != null) {
      fading.value = true;
      await wait(0.4); // the panel fades in meanwhile
    }
    final old = _current;
    final next = _current = add(Level(number));
    await next.loaded;
    old?.destroy();
    fading.value = false;
  });
}
```

A routine goes on only in a frame, so an `onLoad` that awaits one is
ready only after frames have run. The app runs them; a test with a bare
`await game.loaded` runs none and waits for ever. In a test, `await
world.untilLoaded(game)` ([Testing a game](/making/testing/)) runs frames until it's loaded.
