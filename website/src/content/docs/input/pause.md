---
title: "Keys and pausing"
description: "Which keys the game takes from Flutter, and pausing the game."
---

The view hears every key and gives each to the game
(`onKeyDown` hears them all), but it keeps from Flutter only the keys the game
uses: those bound to an axis or an action, and those it has asked `isPressed`,
`wasPressed` or `wasReleased` about (`input.usesKey`).

The others go on to Flutter's shortcuts and focus, so Tab and a dialog's
Escape work. Keys for the app rather than the game go in `FynkelApp`'s
`shortcuts:`, or a `CallbackShortcuts` around the view.

## Pausing

**Pause** with `world.paused = true`: the game's time stops (`delta` is 0, no
fixed steps; updates still run, with `unscaledDelta`), held keys, fingers and
actions let go, and input that comes while paused is dropped. The view claims
no keys then, so the pause menu's shortcuts get every one.

When it ends, the keys still down on the keyboard are held again (and
`wasPressed` that frame). The first frame after it counts no time, however
long the pause was. Sounds hold too, all but the music, which plays on for the
pause menu unless `audio.music.ignorePause = false` is set before its first
track ([Playing sounds](/sound/playing/)):

```dart
// lib/paused_app.dart
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';

void main() => runApp(
  FynkelApp(
    Score.new, // the HUD page's
    overlay: (context, game) => ScoreHud(game),
    shortcuts: (game) => {
      const SingleActivator(LogicalKeyboardKey.escape): () => game.world.paused = !game.world.paused,
    },
  ),
);
```

[How a game is made](/basics/hello/)'s `show runApp` is too narrow for this: import all of `widgets.dart`,
and `services.dart` for the keys, as here. A pause panel shows itself
when the game pauses, from wherever it's paused, with
`ValueListenableBuilder(valueListenable: game.world.pausedListenable,
...)` (`pausedListenable` comes from `package:fynkel_flutter`'s
`WorldListenables`: import it in the widget's file); game code hears it
with `on(world.onPausedChanged, ...)`.
