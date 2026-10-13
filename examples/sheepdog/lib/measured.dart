// Sheepdog for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart: the game with its HUD and minimap, and nobody playing, so
// the wolves take the flock. Its lifecycle check starts a new round again
// and again.

import 'package:flutter/widgets.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

import 'game.dart';
import 'hud.dart';

void main() => runApp(
  MeasuredApp(
    SheepdogGame.new,
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
