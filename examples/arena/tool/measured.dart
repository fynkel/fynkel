// The arena for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart. Its lifecycle check also starts a new round again and
// again.

import 'package:arena/arena.dart';
import 'package:arena/hud.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

void main() =>
    runApp(MeasuredApp(ArenaGame.new, overlay: (context, game) => Hud(game), change: (game) => game.restart()));
