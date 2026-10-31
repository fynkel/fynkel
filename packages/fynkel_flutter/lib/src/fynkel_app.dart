import 'package:flutter/foundation.dart' show kDebugMode;
import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show AssetSource, GameObject;

import 'fynkel_game.dart';
import 'fynkel_view.dart';

/// The whole app for a game that is its view, and a HUD over it, if any:
/// `void main() => runApp(FynkelApp(Garden.new));`, or
/// `FynkelApp(ArenaGame.new, overlay: (context, game) => Hud(game))`. It
/// shows the root [make] makes in a [FynkelView] full screen, with assets
/// from the app's bundle under `assets/` unless [source] says otherwise.
/// The view's own settings ([onStats], [onError], [errorBuilder],
/// [showErrors], [pauseWhenCovered], [directToScreen], [renderScale]) are
/// passed on to it.
///
/// It's a [MaterialApp], with the game as its first page: the overlay can
/// hold any Material widget (a `Slider`, a `TextField`, a dialog from
/// `showDialog`), in the app's [theme], and push pages over the game with
/// `Navigator.of(context)`, which find the game with [FynkelGame.of] too.
/// It's a [FynkelGame] too, so F5 starts the game over in a debug build.
/// A game with menus around it puts a [FynkelGame] in its own app instead.
class FynkelApp<T extends GameObject> extends StatefulWidget {
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
    this.onStats,
    this.onError,
    this.errorBuilder,
    this.showErrors = kDebugMode,
    this.pauseWhenCovered = true,
    this.directToScreen = true,
    this.renderScale = 1.0,
  });

  /// Makes the game's root, as `Garden.new` does: when the app starts,
  /// and again each time it starts over.
  final T Function() make;

  /// What shows over the game, such as a HUD, given its root. It's on a
  /// transparent [Material], so Material widgets work in it, and in a
  /// [Stack] the size of the screen: a [Positioned] overlay goes where it
  /// says (`Positioned(left: 24, bottom: 24, child: Stick(...))`), and any
  /// other is laid out loosely from the top-left corner, covering only
  /// itself, so taps around it reach the game. Layouts that fill
  /// ([Center], [Column], [Align]) still do.
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

  /// See [FynkelView.onStats].
  final void Function(FynkelViewStats stats)? onStats;

  /// See [FynkelView.onError].
  final void Function(Object error, StackTrace stack)? onError;

  /// See [FynkelView.errorBuilder].
  final Widget Function(BuildContext context, Object error, VoidCallback retry)? errorBuilder;

  /// See [FynkelView.showErrors].
  final bool showErrors;

  /// See [FynkelView.pauseWhenCovered].
  final bool pauseWhenCovered;

  /// See [FynkelView.directToScreen].
  final bool directToScreen;

  /// See [FynkelView.renderScale].
  final double renderScale;

  @override
  State<FynkelApp<T>> createState() => _FynkelAppState<T>();
}

class _FynkelAppState<T extends GameObject> extends State<FynkelApp<T>> {
  // The game, for the pages pushed over it.
  final _game = AppGame();

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: widget.title,
    color: const Color(0xFF000000),
    theme: widget.theme ?? ThemeData.dark(),
    darkTheme: widget.darkTheme,
    themeMode: widget.themeMode,
    navigatorKey: widget.navigatorKey,
    debugShowCheckedModeBanner: false,
    // Above the navigator, so every page finds the game.
    builder: (context, navigator) => AppGameScope(holder: _game, child: navigator!),
    home: FynkelGame(
      widget.make,
      source: widget.source,
      restartOnReload: widget.restartOnReload,
      builder: (context, game) {
        final view = FynkelView(
          root: game,
          onStats: widget.onStats,
          onError: widget.onError,
          errorBuilder: widget.errorBuilder,
          showErrors: widget.showErrors,
          pauseWhenCovered: widget.pauseWhenCovered,
          directToScreen: widget.directToScreen,
          renderScale: widget.renderScale,
        );
        final over = widget.overlay;
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
                    // the game. A Positioned overlay goes where it says.
                    child: Stack(children: [Builder(builder: (context) => over(context, game))]),
                  ),
                ],
              );
        final keys = widget.shortcuts;
        return keys == null ? page : CallbackShortcuts(bindings: keys(game), child: page);
      },
    ),
  );
}
