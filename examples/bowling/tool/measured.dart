// bowling for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart. Its lifecycle check also rolls a ball.

import 'package:bowling/bowling.dart';
import 'package:bowling/hud.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

void main() => runApp(
  MeasuredApp(
    BowlingGame.new,
    overlay: (game) => Hud(game as BowlingGame),
    change: (game) async => (game as BowlingGame)
      ..aimX = 0.05
      ..roll(speed: 7.5, angle: 0),
  ),
);
