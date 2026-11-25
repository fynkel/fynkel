---
title: "Screens and controls"
description: "A score, a stick and a button, a countdown, pause and game over."
---

## A score in the corner

```dart
// lib/recipes/score.dart
import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show GameObject;
import 'package:fynkel_flutter/fynkel_flutter.dart';

class CoinGame extends GameObject {
  final coins = ValueNotifier(0);
}

void main() => runApp(
  FynkelApp(
    CoinGame.new,
    overlay: (context, game) => SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: ValueListenableBuilder(
          valueListenable: game.coins,
          builder: (context, coins, _) => Text('$coins', style: const TextStyle(fontSize: 32, color: Colors.white)),
        ),
      ),
    ),
  ),
);
```

## A stick and a jump button

They feed the same `move` axis and `jump` action as the keys.

```dart
// lib/recipes/touch_controls.dart
import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show Input;
import 'package:fynkel_flutter/fynkel_flutter.dart';

class TouchControls extends StatelessWidget {
  const TouchControls(this.input, {super.key});

  final Input input;

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Stack(
      children: [
        Positioned(left: 24, bottom: 24, child: Stick(input)),
        Positioned(
          right: 24,
          bottom: 48,
          child: Listener(
            onPointerDown: (_) => input.action('jump').press(),
            onPointerUp: (_) => input.action('jump').release(),
            onPointerCancel: (_) => input.action('jump').release(),
            child: const CircleAvatar(radius: 36, child: Icon(Icons.arrow_upward)),
          ),
        ),
      ],
    ),
  );
}
```

## Three, two, one, go

```dart
// lib/recipes/countdown.dart
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';

class Countdown extends GameObject {
  final text = ValueNotifier(''); // the HUD shows it
  final go = Signal<void>();

  @override
  void onStart() {
    super.onStart();
    startRoutine(() async {
      for (final word in ['3', '2', '1']) {
        text.value = word;
        await wait(1);
      }
      text.value = '';
      go.emit();
    });
  }
}
```

## A pause button

The menu is Flutter's dialog; covering the game pauses it.

```dart
// lib/recipes/pause_button.dart
import 'package:flutter/material.dart';

class PauseButton extends StatelessWidget {
  const PauseButton({super.key});

  @override
  Widget build(BuildContext context) => IconButton(
    icon: const Icon(Icons.pause, color: Colors.white),
    onPressed: () => showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Paused'),
        actions: [TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Resume'))],
      ),
    ),
  );
}
```

## Game over, and again

Everything of one round is under one object: starting over destroys it and
makes another.

```dart
// lib/recipes/game_over.dart
import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show GameObject;

class Round extends GameObject {}

class RoundGame extends GameObject {
  final over = ValueNotifier(false);
  late Round round;

  @override
  void onLoad() {
    super.onLoad();
    round = add(Round());
  }

  void again() {
    round.destroy();
    round = add(Round());
    over.value = false;
  }
}

class GameOver extends StatelessWidget {
  const GameOver(this.game, {super.key});

  final RoundGame game;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: game.over,
    builder: (context, over, _) =>
        over ? Center(child: FilledButton(onPressed: game.again, child: const Text('Play again'))) : const SizedBox(),
  );
}
```
