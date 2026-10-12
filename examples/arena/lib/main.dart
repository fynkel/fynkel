import 'package:flutter/services.dart' show LogicalKeyboardKey;
import 'package:flutter/widgets.dart' show SingleActivator, runApp;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'arena.dart';
import 'hud.dart';

void main() => runApp(
  FynkelApp(
    ArenaGame.new,
    overlay: (context, game) => Hud(game),
    // The app's keys, not the game's: they work while it's paused.
    shortcuts: (game) => {
      const SingleActivator(LogicalKeyboardKey.escape): game.togglePause,
      const SingleActivator(LogicalKeyboardKey.keyP): game.togglePause,
    },
  ),
);
