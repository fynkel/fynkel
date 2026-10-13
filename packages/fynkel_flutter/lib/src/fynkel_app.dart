import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show AssetSource, GameObject;

import 'fynkel_game.dart';
import 'fynkel_view.dart';

/// The whole app for a game that is its view, and a HUD over it, if any:
/// `void main() => runApp(FynkelApp(Garden.new));`, or
/// `FynkelApp(ArenaGame.new, overlay: (context, game) => Hud(game))`. It
/// shows the root [make] makes in a [FynkelView] full screen, with assets
/// from the app's bundle under `assets/` unless [source] says otherwise.
///
/// It's a [MaterialApp], with the game as its first page: the overlay can
/// hold any Material widget (a `Slider`, a `TextField`, a dialog from
/// `showDialog`), in the app's [theme], and push pages over the game with
/// `Navigator.of(context)`. It's a [FynkelGame] too, so F5 starts the game
/// over in a debug build. A game with menus around it puts a [FynkelGame]
/// in its own app instead.
class FynkelApp<T extends GameObject> extends StatelessWidget {
  /// An app that shows the root [make] makes, with [overlay] over it.
  const FynkelApp(
    this.make, {
    super.key,
    this.overlay,
    this.source,
    this.title = '',
    this.restartOnReload = false,
    this.theme,
    this.darkTheme,
    this.themeMode = ThemeMode.system,
    this.navigatorKey,
    this.shortcuts,
  });

  /// Makes the game's root, as `Garden.new` does: when the app starts,
  /// and again each time it starts over.
  final T Function() make;

  /// What shows over the game, such as a HUD, given its root. It's on a
  /// transparent [Material], so Material widgets work in it.
  final Widget Function(BuildContext context, T game)? overlay;

  /// Where assets load from: the app's bundle under `assets/`
  /// ([bundleSource]) unless given.
  final AssetSource? source;

  /// The app's name for the system, such as the task switcher's.
  final String title;

  /// See [FynkelGame.restartOnReload].
  final bool restartOnReload;

  /// The overlay's and the app's themes, as [MaterialApp]'s. Without
  /// one, Material's dark theme, whose light text reads over most games.
  final ThemeData? theme, darkTheme;

  /// Which of [theme] and [darkTheme] is used, as [MaterialApp]'s.
  final ThemeMode themeMode;

  /// The app's navigator, to show pages or dialogs from outside a widget,
  /// such as from the game's code.
  final GlobalKey<NavigatorState>? navigatorKey;

  /// Keys for the app rather than the game, given its root, as
  /// [CallbackShortcuts]' bindings: `(game) => {const
  /// SingleActivator(LogicalKeyboardKey.escape): game.togglePause}`. A key
  /// the game uses goes to the game instead, unless the world is paused.
  final Map<ShortcutActivator, VoidCallback> Function(T game)? shortcuts;

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: title,
    color: const Color(0xFF000000),
    theme: theme ?? ThemeData.dark(),
    darkTheme: darkTheme,
    themeMode: themeMode,
    navigatorKey: navigatorKey,
    debugShowCheckedModeBanner: false,
    home: FynkelGame(
      make: make,
      source: source,
      restartOnReload: restartOnReload,
      builder: (context, game) {
        final view = FynkelView(root: game);
        final over = overlay;
        final page = over == null
            ? view
            : Stack(
                fit: StackFit.expand,
                children: [
                  view,
                  Material(
                    type: MaterialType.transparency,
                    // Loose, from the top-left corner: a HUD's text covers
                    // only itself, not the screen, so taps around it reach
                    // the game. Layouts that fill (Center, Column, Align)
                    // still do.
                    child: Align(
                      alignment: AlignmentDirectional.topStart,
                      child: Builder(builder: (context) => over(context, game)),
                    ),
                  ),
                ],
              );
        final keys = shortcuts;
        return keys == null ? page : CallbackShortcuts(bindings: keys(game), child: page);
      },
    ),
  );
}
