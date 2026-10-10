import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'arena.dart';
import 'hud.dart';

void main() {
  final game = ArenaGame();
  runApp(
    WidgetsApp(
      color: const Color(0xFF000000),
      debugShowCheckedModeBanner: false,
      builder: (context, _) => Stack(
        fit: StackFit.expand,
        children: [
          FynkelView(root: game),
          Hud(game),
        ],
      ),
    ),
  );
}
