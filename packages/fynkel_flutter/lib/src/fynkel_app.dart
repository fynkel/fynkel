import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show AssetSource, GameObject;

import 'fynkel_game.dart';
import 'fynkel_view.dart';

/// The whole app for a game that is its view, and a HUD over it, if any:
/// `void main() => runApp(FynkelApp(Garden.new));`, or
/// `FynkelApp(ArenaGame.new, overlay: (context, game) => Hud(game))`. It
/// shows the root [make] makes in a [FynkelView] full screen, with assets
/// from the app's bundle under `assets/` unless [source] says otherwise.
/// It's a [FynkelGame], so F5 starts the game over in a debug build. A
/// game with menus around it puts a [FynkelGame] in its own app instead.
class FynkelApp<T extends GameObject> extends StatelessWidget {
  const FynkelApp(this.make, {super.key, this.overlay, this.source, this.title = '', this.restartOnReload = false});

  /// Makes the game's root, as `Garden.new` does: when the app starts,
  /// and again each time it starts over.
  final T Function() make;

  /// What shows over the game, such as a HUD, given its root.
  final Widget Function(BuildContext context, T game)? overlay;

  final AssetSource? source;

  /// The app's name for the system, such as the task switcher's.
  final String title;

  /// See [FynkelGame.restartOnReload].
  final bool restartOnReload;

  @override
  Widget build(BuildContext context) => WidgetsApp(
    title: title,
    color: const Color(0xFF000000),
    debugShowCheckedModeBanner: false,
    builder: (context, _) => FynkelGame(
      make: make,
      restartOnReload: restartOnReload,
      builder: (context, game) {
        final view = FynkelView(root: game, source: source);
        final over = overlay;
        if (over == null) return view;
        return Stack(fit: StackFit.expand, children: [view, over(context, game)]);
      },
    ),
  );
}
