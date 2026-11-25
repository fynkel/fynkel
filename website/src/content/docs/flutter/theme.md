---
title: "One look for app and game"
description: "A theme shared by the widgets and the scene, light and dark."
---

**One look for the app and the game.** Write the palette once, as a
Flutter theme:

```dart
// lib/palette.dart
import 'package:flutter/material.dart';

/// The colour everything is grown from.
const seed = Color(0xFF4CA6E0);

ThemeData gameTheme(Brightness brightness) =>
    ThemeData(colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: brightness));
```

and give it to the app: `FynkelApp(Score.new, theme:
gameTheme(Brightness.light), darkTheme: gameTheme(Brightness.dark))`,
light or dark as the phone is (`themeMode:` to choose). The game wears
it too: `world.theme` is the app's `ThemeData` (from `Theme.of` where the
game is shown), there before the root is added, and
`world.onThemeChanged` tells of a new one. `.toFynkel()` turns a Flutter
colour into a Fynkel one, `.toFlutter()` the other way, both from
`package:fynkel_flutter`:

```dart
// lib/sky.dart
import 'package:flutter/material.dart' show ThemeData;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// On the camera: its sky is the theme's surface, and follows it.
class Sky extends Behaviour {
  @override
  void onStart() {
    super.onStart();
    wear(world.theme);
    on(world.onThemeChanged, wear);
  }

  void wear(ThemeData theme) => get<Camera>()!.clearColor = theme.colorScheme.surface.toFynkel();
}
```

When the phone goes dark, the app fades to its dark theme, and
`onThemeChanged` hears each step of the fade, so the sky fades with it.
`world.isDark` says which it is. A widget that shows a game's colour
imports Fynkel with a prefix:

```dart
// lib/team_chip.dart
import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' as fynkel;
import 'package:fynkel_flutter/fynkel_flutter.dart';

class TeamChip extends StatelessWidget {
  const TeamChip(this.team, {super.key});

  final fynkel.Color team;

  @override
  Widget build(BuildContext context) => Chip(label: const Text('team'), backgroundColor: team.toFlutter());
}
```

A test sets the theme itself: `world.theme = gameTheme(Brightness.dark)`.
`world.theme` is package:fynkel_flutter's (an extension, `WorldTheme`),
so a test of a plain `World` imports it:
`import 'package:fynkel_flutter/fynkel_flutter.dart' show WorldTheme;`.
