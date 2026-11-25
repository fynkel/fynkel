---
title: "A controls page"
description: "Let the player change the keys."
---

**A controls page** rebinds with a "press a key" dialog. Its `Focus`
takes every key before the app's shortcuts do, and Escape closes it
keeping the key there was:

```dart
// lib/controls.dart
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' show Input, KeyCode;
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// Waits for the player to press a key for [what], and gives it; null for
/// Escape.
Future<KeyCode?> askForKey(BuildContext context, String what) => showDialog<KeyCode>(
  context: context,
  builder: (context) => Focus(
    autofocus: true,
    onKeyEvent: (node, event) {
      if (event is KeyDownEvent) {
        Navigator.of(context).pop(event.logicalKey == LogicalKeyboardKey.escape ? null : keyCodeOf(event));
      }
      return KeyEventResult.handled;
    },
    child: AlertDialog(title: Text('Press a key for $what')),
  ),
);

/// The controls page's row for jumping: the key, and a tap to change it.
class JumpKey extends StatefulWidget {
  const JumpKey(this.input, {super.key});

  final Input input;

  @override
  State<JumpKey> createState() => _JumpKeyState();
}

class _JumpKeyState extends State<JumpKey> {
  var _key = KeyCode.space;

  @override
  Widget build(BuildContext context) => ListTile(
    title: const Text('Jump'),
    trailing: OutlinedButton(
      onPressed: () async {
        final key = await askForKey(context, 'Jump');
        if (key == null) return;
        widget.input.bindAction('jump', [key]);
        setState(() => _key = key);
      },
      child: Text(_key.label),
    ),
  );
}
```

A real game keeps its keys in a `ChangeNotifier` the settings page and
the game share, and the game binds them again when it changes:
`on(controls.changed, (_) => controls.bindTo(input))`
([examples/sheepdog/lib/game.dart](https://github.com/fynkel/fynkel/blob/main/examples/sheepdog/lib/game.dart)). `Controls` keeps one key for each
name, keys only, and the arrow keys always move too. `bindTo` binds every
action again from it, so a mouse button or a second key for an action is
bound after it, each time: `input.bindAction('fire', [controls['fire'],
MouseButton.left])`.
