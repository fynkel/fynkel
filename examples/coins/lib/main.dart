import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'coins.dart';
import 'hud.dart';

void main() => runApp(FynkelApp(CoinsGame.new, overlay: (context, game) => Hud(game.player.score)));
