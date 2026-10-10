// The arena for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart. Its lifecycle check also starts a new round again and
// again.

import 'package:flutter/widgets.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

import 'arena.dart';
import 'hud.dart';

void main() => runApp(
  MeasuredApp(
    ArenaGame.new,
    overlay: (game) => Hud(game as ArenaGame),
    change: (game) => (game as ArenaGame).restart(),
  ),
);
