---
title: "Menus and pages"
description: "Pause menus, settings pages and the back button."
---

Menus, pop-ups and pages are Flutter's own: `showDialog`, `Navigator.push`,
`PopScope`. Whatever covers the view's page pauses the game
(`world.paused`), so it hears no taps or keys, and closing the last one goes
on.

The game stays alive behind it: its objects, routines and score wait where
they were. As nothing in it moves, it's drawn only 10 times a second, which
keeps a phone cool behind a menu.

```dart
// lib/menus.dart
import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';

/// The game's page: back (Android's button) opens the pause menu.
class GamePage extends StatelessWidget {
  const GamePage({super.key});

  @override
  Widget build(BuildContext context) => FynkelGame(
    Score.new,
    builder: (context, game) => PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) showPauseMenu(context);
      },
      child: Stack(fit: StackFit.expand, children: [FynkelView(root: game), ScoreHud(game)]),
    ),
  );
}

/// The pause menu, over the game's page at [context].
Future<void> showPauseMenu(BuildContext context) {
  final navigator = Navigator.of(context);
  return showDialog<void>(
    context: context,
    builder: (context) => SimpleDialog(
      title: const Text('Paused'),
      children: [
        SimpleDialogOption(onPressed: navigator.pop, child: const Text('Resume')),
        SimpleDialogOption(
          onPressed: () => navigator.push(MaterialPageRoute<void>(builder: (_) => const SettingsPage())),
          child: const Text('Settings'),
        ),
        SimpleDialogOption(
          onPressed: () => navigator
            ..pop() // the menu
            ..pop(), // and the game's page
          child: const Text('Leave'),
        ),
      ],
    ),
  );
}

class SettingsPage extends StatelessWidget {
  const SettingsPage({super.key});

  @override
  Widget build(BuildContext context) => Scaffold(appBar: AppBar(title: const Text('Settings')));
}
```

The settings page goes over the menu, and the game stays paused under
both until the menu closes. `PopScope(canPop: false)` turns Android's
back button into "open the menu". In a dialog, Escape is a back too: a
menu that leaves the game on back, as the arena's does, maps Escape to
closing itself with a `CallbackShortcuts` (`examples/arena/lib/menus.dart`,
which has a title screen, this menu and a settings page, and tests that
click through them).

## When it pauses

- A page pauses the game once it has slid in all the way, and as it starts to
  slide back off the game goes on. While it slides, the game still shows and
  runs at its rate.
- A game that was paused already stays paused. A pause button can set
  `world.paused = true` to stop the game the moment it's tapped and then push
  its page; on Resume, close the page and unpause it.
- Unpaused by the game while the page is over it, it runs there: the game's
  word goes.
- For a game that runs on under a see-through pop-up, give
  `FynkelView(pauseWhenCovered: false)`.
