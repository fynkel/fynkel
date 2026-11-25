import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';

void main() => runApp(FynkelApp(RollGame.new, overlay: (context, game) => Hud(game)));
