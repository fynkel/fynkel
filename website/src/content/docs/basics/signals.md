---
title: "Signals"
description: "Events any code can listen to, that end with their listener."
---

A `Signal` is an event any code can listen to. Listen with **`on`**, and
the listening ends by itself when the listener is destroyed, so nothing
ever calls into a dead object:

```dart
class Health extends Behaviour {
  Health(this.max) : left = max;

  final int max;
  int left;
  final died = Signal<GameObject>();

  void damage(int amount) {
    if (left == 0) return;
    left = (left - amount).clamp(0, max);
    if (left == 0) died.emit(gameObject);
  }
}

class Scoreboard extends GameObject {
  int kills = 0;

  void watch(GameObject enemy) {
    on(enemy.get<Health>()!.died, (_) => kills++);
  }

  Future<void> waitForBoss(GameObject boss) async {
    await boss.get<Health>()!.died.next; // in a routine: one of its waits
  }
}
```

- `emit(value)` calls every listener at once, in order; a `Signal<void>` is
  emitted with `emit()`. A handler that throws goes to `world.errors`, and the
  others still hear.
- `signal.listen(handler)` listens without an owner: cancel it yourself.
- `await signal.next` waits for the next value.
