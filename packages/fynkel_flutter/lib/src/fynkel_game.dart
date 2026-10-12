import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show GameObject;

/// Makes a game's root with [make] and builds what shows it, so the game
/// can start over with a new root:
///
/// ```dart
/// FynkelGame(
///   make: ArenaGame.new,
///   builder: (context, game) => Stack(children: [FynkelView(root: game), Hud(game)]),
/// )
/// ```
///
/// [restart] destroys everything [builder] built, the view's world with
/// it, and builds it again around a new root. That runs the game's code as
/// it is now, so after a hot reload it's how changed field initializers,
/// `onLoad` and `onStart` take effect (a hot reload alone changes only the
/// methods of objects already made). In debug builds F5 restarts, and
/// with [restartOnReload] every hot reload does too: for phones, which
/// have no F5. [FynkelApp] uses one.
class FynkelGame<T extends GameObject> extends StatefulWidget {
  const FynkelGame({super.key, required this.make, required this.builder, this.restartOnReload = false});

  /// Makes a new root, as `ArenaGame.new` does.
  final T Function() make;

  /// What shows [make]'s root: a [FynkelView] of it, and anything around
  /// or over it that uses the game, such as a HUD.
  final Widget Function(BuildContext context, T game) builder;

  /// Whether a hot reload restarts the game. Off, a hot reload keeps the
  /// moment being played.
  final bool restartOnReload;

  /// The [FynkelGame] around [context], or null: `FynkelGame.maybeOf<Arena>(context)?.game`
  /// is an `Arena`.
  static FynkelGameState<T>? maybeOf<T extends GameObject>(BuildContext context) =>
      context.findAncestorStateOfType<FynkelGameState<T>>();

  /// The [FynkelGame] around [context] (`FynkelGame.of<Arena>(context).restart()`).
  /// Throws a [FlutterError] if there is none, or if its game isn't a [T].
  static FynkelGameState<T> of<T extends GameObject>(BuildContext context) {
    final state = maybeOf<T>(context);
    if (state != null) return state;
    throw FlutterError(
      maybeOf(context) == null
          ? 'FynkelGame.of found no FynkelGame around this context: make it in the builder of one.'
          : 'FynkelGame.of<$T> found a FynkelGame whose game is not a $T.',
    );
  }

  @override
  State<FynkelGame<T>> createState() => FynkelGameState<T>();
}

/// A [FynkelGame]'s state: its [game], and [restart].
class FynkelGameState<T extends GameObject> extends State<FynkelGame<T>> {
  late T _game = widget.make();
  var _round = 0;

  /// The root being played.
  T get game => _game;

  /// Starts the game over with a new root and a new world.
  void restart() => setState(() {
    _game = widget.make();
    _round++;
  });

  @override
  void reassemble() {
    super.reassemble();
    // The new code is in, and everything builds again after this.
    if (widget.restartOnReload) {
      _game = widget.make();
      _round++;
    }
  }

  @override
  Widget build(BuildContext context) {
    final built = KeyedSubtree(
      key: ValueKey(_round),
      child: Builder(builder: (context) => widget.builder(context, _game)),
    );
    if (!kDebugMode) return built;
    return CallbackShortcuts(bindings: {const SingleActivator(LogicalKeyboardKey.f5): restart}, child: built);
  }
}
