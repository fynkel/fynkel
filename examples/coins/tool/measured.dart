// coins for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart. Its lifecycle check also changes the level back and forth.

import 'package:coins/coins.dart';
import 'package:coins/hud.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

void main() => runApp(
  MeasuredApp(
    CoinsGame.new,
    overlay: (context, game) => Hud(game.player.score, toon: game.toon),
    change: (game) => game.nextLevel(),
  ),
);
