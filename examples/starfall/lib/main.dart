import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'game.dart';
import 'screens.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // One number to keep: shared_preferences is enough (https://fynkel.com/making/saving/).
  final prefs = await SharedPreferences.getInstance();
  runApp(
    FynkelApp(
      () {
        final game = StarfallGame(best: prefs.getInt('best') ?? 0);
        game.best.addListener(() => prefs.setInt('best', game.best.value));
        return game;
      },
      title: 'Starfall',
      overlay: (context, game) => Screens(game),
    ),
  );
}
