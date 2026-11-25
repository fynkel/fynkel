---
title: "A HUD over the game"
description: "Scores, buttons and dialogs: plain Flutter over the game."
---

The game is a Flutter widget, so menus, HUDs and settings are plain
Flutter. Keep what the HUD shows in `ValueNotifier`s on the game, and give
the HUD to `FynkelApp`'s `overlay`, which gets the game's root:

```dart
class Score extends GameObject {
  final points = ValueNotifier(0);
}
```

```dart
// lib/hud.dart
import 'package:flutter/widgets.dart';

import 'game.dart';

class ScoreHud extends StatelessWidget {
  const ScoreHud(this.game, {super.key});

  final Score game;

  @override
  Widget build(BuildContext context) => SafeArea(
    child: ValueListenableBuilder(
      valueListenable: game.points,
      builder: (context, points, _) => Text('★ $points', style: const TextStyle(fontSize: 24)),
    ),
  );
}
```

and in `main`: `runApp(FynkelApp(Score.new, overlay: (context, game) =>
ScoreHud(game)))`. The overlay lies in a `Stack` over the view: a HUD
that isn't `Positioned` sits loose at the top-left, and a `Positioned`
one goes where it says (`Positioned(right: 16, bottom: 16, child:
...)`).

## On a phone

**On a phone the view is under Flutter, not in it** (on Android with a
`FynkelActivity`, [Adding Fynkel to an app](/start/existing-app/)). Widgets over the view draw over the game, as
everywhere: the HUD, a pause menu, a dialog's dimmed background. Three
things differ from the Mac:

- Whatever is painted under the view doesn't show: the view clears its place,
  so the game shows through. That's what you want.
- A transform, clip or opacity from the widgets above the view doesn't reach
  the game. A page sliding in is followed (the view moves to where its widget
  is, each frame); one that fades or zooms in shows the game unfaded and at
  full size, and a view in a scrolling list isn't clipped at the list's edges.
- Views stack two deep, no more. A view lying within another goes over it (a
  minimap over the game); any others lie side by side, at the bottom (a split
  screen, a compare page's two viewers). A third view within the minimap
  wouldn't show over it. Views on pages under the page shown, or in hidden
  tabs, don't count: their surfaces are hidden.

Where one of those matters, give that view `directToScreen: false`: it's a
texture Flutter draws, as on the Mac, slower on some Android phones and
warmer on an iPhone.

## From the game to the UI and back

A `Signal` has a `stream`, for
`StreamBuilder` or a `listen` that opens a dialog when the game is over
(it hears each value a moment after `emit`). The other way, any
`Listenable` (a `ValueNotifier` the settings page changes) has a
`changed` signal, from `package:fynkel_flutter`, so an object listens with
`on(settings.volume.changed, (_) => ...)` and stops when it's destroyed.

**Calling the game from a Flutter callback is safe**: a button's
`onPressed` can call `game.restart()`, add an object or cast a ray. It
runs between frames, on the same thread as the game; a physics call made
then waits for the step running between frames to finish first.

## What FynkelApp is

`FynkelApp` is a `MaterialApp` with the game as its page, so the overlay
can hold any Material widget (a `Slider`, a `TextField`, a button), open a
dialog with `showDialog`, or push a page over the game with
`Navigator.of(context)`: a page pushed there finds the game with
`FynkelGame.of(context)`, as the overlay does. Give it a `theme:` (Material's dark one if not),
and a `navigatorKey:` to open pages from the game's code.

## Keep the HUD in its own file

Flutter and Fynkel both have a `Color`
(and a `Material` and a `Transform`), so a game's files import
`package:fynkel/fynkel.dart` (and `package:flutter/foundation.dart` for
`ValueNotifier`), and its widgets' files import Flutter, as the examples
do. A file that needs both imports one with a prefix, or only the names
it uses from the other (`show ThemeData`).
