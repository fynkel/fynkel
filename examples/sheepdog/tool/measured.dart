// Sheepdog for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart: the game with its HUD and minimap at level 3, with
// wolves, and nobody playing, so the wolves take some of the flock. Its
// lifecycle check starts a new round again and again.

import 'package:flutter/widgets.dart';
import 'package:fynkel_measure/fynkel_measure.dart';
import 'package:sheepdog/game.dart';
import 'package:sheepdog/hud.dart';

void main() => runApp(
  MeasuredApp(
    () => SheepdogGame(level: 3),
    overlay: (game) {
      game as SheepdogGame;
      return Stack(
        fit: StackFit.expand,
        children: [
          Minimap(game),
          Hud(game, onPause: () {}),
        ],
      );
    },
    change: (game) => (game as SheepdogGame).restart(),
  ),
);
