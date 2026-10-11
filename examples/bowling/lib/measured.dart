// bowling for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart. Its lifecycle check also rolls a ball.

import 'package:flutter/widgets.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

import 'bowling.dart';
import 'hud.dart';

void main() => runApp(
  MeasuredApp(
    BowlingGame.new,
    overlay: (game) => Hud((game as BowlingGame).score, game.message),
    change: (game) async => (game as BowlingGame).roll(x: 0.05, speed: 7.5, angle: 0),
  ),
);
