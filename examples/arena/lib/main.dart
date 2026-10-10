import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'arena.dart';
import 'hud.dart';

void main() => runApp(FynkelApp(ArenaGame.new, overlay: (context, game) => Hud(game)));
