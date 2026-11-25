---
title: "Routines"
description: "Code that waits in game time: coroutines, as plain Dart async."
---

A routine is `async` code that waits in game time, Unity's coroutine in
plain Dart:

```dart
class Spawner extends GameObject {
  final score = ValueNotifier(0);
  final clock = ValueNotifier(false);

  @override
  void onStart() {
    super.onStart();
    startRoutine(() async {
      for (var wave = 1; ; wave++) {
        for (var i = 0; i < wave * 3; i++) {
          add(Crate(Vec3(i.toDouble(), 0.5, -5)));
          await wait(0.4);
        }
        await waitUntil(() => world.all<Crate>().isEmpty);
        await wait(2);
      }
    });
  }

  /// Ten points in ten seconds, or not.
  Future<bool> beatTheClock() async {
    clock.value = true;
    try {
      return await race([
        startRoutine(() async {
          await waitUntil(() => score.value >= 10);
          return true;
        }),
        startRoutine(() async {
          await wait(10);
          return false;
        }),
      ]);
    } finally {
      clock.value = false; // runs even when the routine is stopped
    }
  }

  Future<void> bonusRound() async {
    final won = await startRoutine(beatTheClock); // a routine returns a value
    if (won) score.value += 100;
  }
}
```

- `startRoutine` returns a **`Routine`**: `stop()`, `isRunning`, and it's a
  future of its result. When it's stopped, its `finally` blocks run, and
  routines started inside it stop too. Nothing else of it runs again: a tween
  it started on another object goes on (it's that object's), but a `.then` the
  routine gave it never runs.

  What must happen when a tween ends, whatever became of the routine (a crate
  back to its pool), goes in the tween's `onComplete:`.
- An error a routine doesn't catch goes to `world.errors`.

### startRoutine(() async {...})

`startRoutine(() async {...})` runs one, on an object or behaviour
(Unity's `StartCoroutine`); `stopAllRoutines()` stops the ones it started.
It **stops by itself** when its object is destroyed (or its behaviour
removed), and **pauses** while its object is hidden or the behaviour isn't
enabled, except during their own load: an `onLoad` may await a routine (or a
tween) of its own.

### Waits

`await nextFrame`, `await fixedStep` (right after the next fixed
step's `onFixedUpdate` calls), `await wait(seconds)` (`unscaled: true` to go
on while paused), `await waitUntil(() => test)`, `await waitWhile(() =>
test)`, a tween, a signal's `next`, or any `Future`.

### race([...])

`race([...])` gives the first to end and stops the rest (routines, waits
and tweens); **`waitAll([...])`** waits for every one. `race([x, wait(3)])`
is a timeout.
