import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart' show Theme;
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show AssetSource, GameObject, ObjectError, World;

import 'fynkel_view.dart' show bundleSource, resetPlugin, tellLifecycle;
import 'world_listenables.dart' show WorldTheme;

/// Makes a game's root with [make] and builds what shows it, so the game
/// can start over with a new root:
///
/// ```dart
/// FynkelGame(
///   ArenaGame.new,
///   builder: (context, game) => Stack(children: [FynkelView(root: game), Hud(game)]),
/// )
/// ```
///
/// It's a widget, not the class a game extends: the game is its root, a
/// [GameObject].
///
/// It makes the game's world and adds the root to it before [builder]
/// runs, so `game.world` is there from the start: a widget beside the
/// view can take `game.world.input` while it's built. The [FynkelView] of
/// the root shows that world. The world wears the app's theme
/// (`world.theme`, from `Theme.of(context)` here) from before the root is
/// added, and hears when it changes (`world.onThemeChanged`).
///
/// [restart] destroys everything [builder] built, and the world, and
/// builds it again around a new root in a new world. That runs the game's code as
/// it is now, so after a hot reload it's how changed field initializers,
/// `onLoad` and `onStart` take effect (a hot reload alone changes only the
/// methods of objects already made). In debug builds F5 restarts, and
/// with [restartOnReload] every hot reload does too: for phones, which
/// have no F5. Without it, a hot reload still gives the game's shaders
/// and sounds their files' new contents (`World.reloadAssets`).
/// [FynkelApp] uses one.
///
/// A widget below it, or a widget test, reaches the game with
/// `FynkelGame.of<ArenaGame>(context).game`; calling the game from a
/// Flutter callback is safe, as it runs between frames. A page pushed with
/// `Navigator` isn't below it, but beside its page: give the game to the
/// page, unless the game is a [FynkelApp]'s, whose pages all reach it.
///
/// It keeps the game it started with when it's built again with another
/// [make]: give it a new `key` to start another (`key:
/// ValueKey(product.id)`), or [FynkelGameState.restart] it. A debug build
/// says so only for a [make] of another root type (`Level2.new` for
/// `Level1.new`): a closure made again in a build (`() =>
/// Level(number: 3)`) is a new function each time, so a change of what it
/// makes can't be told from no change.
class FynkelGame<T extends GameObject> extends StatefulWidget {
  /// A game whose root [make] makes, shown by what [builder] builds.
  const FynkelGame(this.make, {super.key, required this.builder, this.source, this.restartOnReload = false});

  /// Makes a new root, as `ArenaGame.new` does: when the game starts, and
  /// again each time it starts over.
  final T Function() make;

  /// What shows [make]'s root: a [FynkelView] of it, and anything around
  /// or over it that uses the game, such as a HUD.
  final Widget Function(BuildContext context, T game) builder;

  /// Where the world loads assets from: the app's bundle under `assets/`
  /// ([bundleSource]) unless given.
  final AssetSource? source;

  /// Whether a hot reload restarts the game. Off, a hot reload keeps the
  /// moment being played.
  final bool restartOnReload;

  /// The [FynkelGame] around [context], or null: `FynkelGame.maybeOf<Arena>(context)?.game`
  /// is an `Arena`.
  /// Under a [FynkelApp], a page pushed over the game finds the app's.
  static FynkelGameState<T>? maybeOf<T extends GameObject>(BuildContext context) {
    final around = context.findAncestorStateOfType<FynkelGameState<T>>();
    if (around != null) return around;
    final app = context.getInheritedWidgetOfExactType<AppGameScope>()?.holder.state;
    return app is FynkelGameState<T> ? app : null;
  }

  /// The [FynkelGame] around [context] (`FynkelGame.of<Arena>(context).restart()`).
  /// [T] is the type the `FynkelGame` was made for (what its `make` gives):
  /// for a `FynkelGame<Level>` making a `Level1`, ask for `Level` and cast
  /// its game. Throws a [FlutterError] if there is none, or if it isn't
  /// one for [T].
  static FynkelGameState<T> of<T extends GameObject>(BuildContext context) {
    final state = maybeOf<T>(context);
    if (state != null) return state;
    final other = maybeOf(context);
    throw FlutterError(
      other == null
          ? 'FynkelGame.of found no FynkelGame around this context: make it in the builder of one. A page pushed '
                "with Navigator (or a dialog) isn't below the FynkelGame of the page under it: give the game to the "
                "page (Settings(game)), or start the app with FynkelApp, whose pages all find its game."
          : 'FynkelGame.of<$T> found a FynkelGame made for ${other._made}, which is not a $T: '
                'ask for ${other._made}, and cast its game.',
    );
  }

  @override
  State<FynkelGame<T>> createState() => FynkelGameState<T>();
}

/// A [FynkelGame]'s state: its [game] and [world], and [restart].
class FynkelGameState<T extends GameObject> extends State<FynkelGame<T>> with WidgetsBindingObserver {
  // What it was made for, as asked for in FynkelGame.of.
  Type get _made => T;

  late T _game;
  late World _world;
  var _round = 0;

  // The world's errors, heard from the start (its stream tells only those
  // listening when an error comes): given to the view showing them, kept
  // until it's there, and reported after the first frame if none is.
  StreamSubscription<ObjectError>? _errorsHeard;
  void Function(ObjectError error)? _errorsTo;
  final _early = <ObjectError>[];

  /// The root being played.
  T get game => _game;

  /// The world [game] is in.
  World get world => _world;

  var _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // The first time, after initState: the theme can be read from here on.
    if (!_started) {
      _started = true;
      // The first game under a FynkelApp is the app's.
      final app = context.getInheritedWidgetOfExactType<AppGameScope>()?.holder;
      if (app != null && app.state == null) _app = app..state = this;
      unawaited(resetPlugin());
      // Its views tell its world too, but a game may show none for a while
      // (a loading page, a page pushed in its place) and still has to hear
      // the app go: its music stops, and onApplicationPause saves.
      WidgetsBinding.instance.addObserver(this);
      _start();
    } else {
      _world.theme = Theme.of(context);
    }
  }

  AppGame? _app;

  // Whether a debug build said that the widget's make changed.
  bool _toldMake = false;

  @override
  void didUpdateWidget(FynkelGame<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    // A closure made again in a build (`() => Level(number: 3)`) is a new
    // function each time, but of the same type: only a make of another
    // type (`Level2.new` for `Level1.new`) surely makes another game.
    if (!kDebugMode || _toldMake || widget.make.runtimeType == oldWidget.make.runtimeType) return;
    _toldMake = true;
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: FlutterError(
          "FynkelGame's make changed from ${oldWidget.make.runtimeType} to ${widget.make.runtimeType}, but a "
          'FynkelGame keeps the game it started with: give it a Key to start another '
          '(FynkelGame(make, key: ValueKey(level), ...)), or restart it.',
        ),
        library: 'fynkel_flutter',
      ),
    );
  }

  /// Starts the game over with a new root and a new world.
  void restart() => setState(_again);

  @override
  void reassemble() {
    super.reassemble();
    // The new code is in, and everything builds again after this. The
    // shaders' and sounds' files are read again either way: a hot reload
    // sends the changed ones.
    if (widget.restartOnReload) {
      _again();
    } else {
      unawaited(_world.reloadAssets());
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) => tellLifecycle(_world, state);

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    // What was built of it, the view first, is gone already.
    if (_app case final app? when app.state == this) app.state = null;
    _errorsHeard?.cancel();
    _world.dispose();
    super.dispose();
  }

  void _start() {
    final world = _world = World(source: widget.source ?? bundleSource());
    _early.clear();
    _errorsTo = null;
    _errorsHeard = world.errors.listen(_heard);
    world.theme = Theme.of(context);
    _game = widget.make();
    world.add(_game);
    // After the first frame and what it reported, which comes in
    // microtasks queued before this one.
    SchedulerBinding.instance.addPostFrameCallback((_) => scheduleMicrotask(() => _reportEarly(world)));
  }

  void _again() {
    // The old world goes once what showed it is gone, after this frame.
    final old = _world;
    _errorsHeard?.cancel();
    SchedulerBinding.instance.addPostFrameCallback((_) => old.dispose());
    _start();
    _round++;
  }

  void _heard(ObjectError error) {
    final to = _errorsTo;
    to == null ? _early.add(error) : to(error);
  }

  /// Reports what no view took, as an error nothing listens to would be,
  /// and stops listening.
  void _reportEarly(World world) {
    final heard = _errorsHeard;
    if (!identical(world, _world) || heard == null || _errorsTo != null) return;
    heard.cancel();
    _errorsHeard = null;
    for (final error in _early) {
      FlutterError.reportError(
        FlutterErrorDetails(
          exception: error.error,
          stack: error.stackTrace,
          library: 'fynkel',
          context: ErrorDescription('in ${error.gameObject ?? 'the world'}'),
        ),
      );
    }
    _early.clear();
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

/// For [FynkelView]: sends what [game]'s world reports to [to], those
/// reported before first; with [to] null, the errors are the game's alone,
/// and nothing more is heard. False when the game hears nothing any more,
/// so the view must listen to the world itself. Not exported.
bool hearGameErrors(FynkelGameState game, void Function(ObjectError error)? to) {
  if (game._errorsHeard == null) return false;
  if (to == null) {
    game._errorsHeard!.cancel();
    game._errorsHeard = null;
  } else {
    game._errorsTo = to;
    game._early.forEach(to);
  }
  game._early.clear();
  return to != null;
}

/// For [FynkelView], when it goes: what [game]'s world reports is kept
/// again, unless another view took it since. Not exported.
void stopHearingGameErrors(FynkelGameState game, void Function(ObjectError error) to) {
  if (game._errorsTo == to) game._errorsTo = null;
}

/// For [FynkelApp]: the app's game, which the first [FynkelGame] under
/// [AppGameScope] makes itself. Not exported.
class AppGame {
  /// The app's game, while there is one.
  FynkelGameState? state;
}

/// For [FynkelApp]: above its `Navigator`, so the pages pushed over the
/// game find it ([FynkelGame.maybeOf]). Not exported.
class AppGameScope extends InheritedWidget {
  /// Holds [holder] above [child], the app's navigator.
  const AppGameScope({super.key, required this.holder, required super.child});

  /// The app's game.
  final AppGame holder;

  @override
  bool updateShouldNotify(AppGameScope oldWidget) => false;
}
