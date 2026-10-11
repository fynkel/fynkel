import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'bowling.dart';
import 'hud.dart';

void main() => runApp(FynkelApp(BowlingGame.new, overlay: (context, game) => Hud(game.score, game.message)));
