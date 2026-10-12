import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart'
    show AssetSource, GameObject, HeadlessView, Input, ObjectError, ViewStats, WorldView, World;
import 'package:fynkel/platform.dart';

import 'error_panel.dart';
import 'fynkel_game.dart';
import 'input_feed.dart';

/// The plugin's channel (darwin/…/FynkelFlutterPlugin.swift and
/// android/…/FynkelFlutterPlugin.kt).
@visibleForTesting
const channel = MethodChannel('fynkel_flutter');

/// Whether views talk to the plugin the Android way: a size with
/// `register` and a `resize` for each new one, because there the plugin
/// makes a new surface for every size. Null for this platform's way. A test
/// sets it to check Android's way on a Mac, where the view is then a
/// [HeadlessView] standing in for the `AndroidSurfaceView`.
@visibleForTesting
bool? debugAndroid;

bool get _android => debugAndroid ?? Platform.isAndroid;

/// Whether this isolate has told the plugin to free the views an earlier
/// isolate left (a hot restart disposes nothing). It does so before its
/// first view registers. The plugin handles a channel's calls in the order
/// they were sent, so a `register` sent after `reset` always comes after
/// it, even while the first is still waiting for the answer.
bool _resetSent = false;

/// Forgets that this isolate reset the plugin, so a test can check that it
/// does.
@visibleForTesting
void forgetReset() => _resetSent = false;

/// What a [FynkelView] did in one stats window (about a second).
@immutable
class FynkelViewStats {
  const FynkelViewStats({required this.window, required this.ticks, required this.view, required this.size});

  /// How long the window was, by the frame clock.
  final Duration window;

  /// Frames of the frame clock: [FynkelView.onFrame] calls.
  final int ticks;

  /// The engine's counts in the window. `view.presented` is the frames
  /// Flutter actually showed new.
  final ViewStats view;

  /// The size the frames are drawn at, in pixels.
  final Size size;

  double get _seconds => window.inMicroseconds / 1e6;

  /// New frames shown per second: the rate the player sees.
  double get freshFps => view.presented / _seconds;

  /// Frame clock ticks per second: the display rate when nothing stalls.
  double get tickFps => ticks / _seconds;

  @override
  String toString() =>
      'fresh=${freshFps.toStringAsFixed(1)} ticks=${tickFps.toStringAsFixed(1)} '
      'size=${size.width.toInt()}x${size.height.toInt()} $view';
}

/// The bytes of asset [name] from the app's bundle, under `assets/`:
/// `character.glb` is `assets/character.glb`, listed in the app's
/// pubspec.yaml. A [FynkelView] with a [FynkelView.root] uses it unless
/// given another [FynkelView.source].
AssetSource bundleSource([AssetBundle? bundle]) => (name) async {
  final data = await (bundle ?? rootBundle).load('assets/$name');
  return data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes);
};

/// Shows a game: `FynkelView(root: garden)` makes a world, adds the
/// root, runs it, and frees it all when the view goes. Each display frame
/// it runs the world's loop ([World.update]: taps, starts, updates) with
/// what the player did ([Input]), then hands the world to the engine to
/// draw from its active camera. It fills the space its parent gives it,
/// which must be bounded, so menus and HUDs are Flutter widgets around or
/// on top of it.
///
/// The root is used once, when the view is first built: to change level,
/// destroy the old root and add another to its world (`root.world`); to
/// start over, make the root in a [FynkelGame] (as [FynkelApp] does) and
/// restart it. The world is [FynkelViewState.world] too, for code outside
/// the game.
///
/// `FynkelView(world:)` shows a world made and driven by hand instead,
/// for tests and tools: [onFrame] is called each frame with the seconds
/// since the last one, before the world's loop, and the world is the
/// caller's to dispose.
///
/// It gives the world's [World.input] Flutter's events as they come. It
/// reads pointers with a `Listener`, which takes part in no gesture
/// arena, so gesture widgets around it keep working, and a widget over it
/// (a HUD's button) keeps the pointers it takes from the game. While it
/// has focus it takes every key, so keys like the arrows and space reach
/// the game rather than moving focus or pressing buttons.
///
/// In a debug build ([showErrors]) it shows what the game's code throws
/// over the game, with the object and the line it came from, as well as
/// in the console: [World.errors], which it listens to.
///
/// The frames never stop: what throws in one (the view's own [onFrame],
/// say) is reported to Flutter and the next frame comes. Without the
/// plugin, as in a widget test of the game, the world runs and nothing is
/// drawn; if the view itself fails, its error shows in its place and the
/// world runs on.
///
/// The frames are drawn at the widget's size in physical pixels times
/// [resolutionScale] and scaled to fit. Below 1 trades sharpness for
/// speed; the view never lowers it by itself.
///
/// macOS, iOS and Android.
class FynkelView extends StatefulWidget {
  const FynkelView({
    super.key,
    this.root,
    this.source,
    this.world,
    this.onFrame,
    this.resolutionScale = 1.0,
    this.onStats,
    this.onViewStats,
    this.focusNode,
    this.autofocus = true,
    this.showErrors = kDebugMode,
  }) : assert(resolutionScale > 0 && resolutionScale <= 4),
       assert((root == null) != (world == null), 'give FynkelView a root or a world, not both');

  /// The game: added to the view's own world when the view is first
  /// built.
  final GameObject? root;

  /// Where the view's own world loads assets from; the app's bundle
  /// ([bundleSource]) when null.
  final AssetSource? source;

  /// A world made by hand, shown in place of a [root]'s own.
  final World? world;

  /// Called once per display frame, before the world's [World.update],
  /// with the seconds since the previous call (0 the first time). For
  /// code that drives the world from outside; a game's own code goes in
  /// its objects.
  final void Function(double dt)? onFrame;

  final double resolutionScale;

  /// Called about once a second with what the view did since the last call.
  final void Function(FynkelViewStats stats)? onStats;

  /// Called after each frame the view hands to the engine with the
  /// engine's counts since the view was made, for measuring: the frame
  /// that first draws a new model is the first whose `textureBytes` or
  /// `meshBytes` hold its data, and it's on screen once `presented` goes
  /// up after that.
  final void Function(ViewStats stats)? onViewStats;

  /// The view's focus, for keys. One of its own when null.
  final FocusNode? focusNode;

  /// Whether the view takes focus when it first appears, so keys work
  /// before it's tapped.
  final bool autofocus;

  /// Whether the errors the game's code throws ([World.errors]) are shown
  /// over the game and reported to Flutter (the console, and a failing
  /// widget test), rather than left to the game. On in debug builds.
  final bool showErrors;

  @override
  State<FynkelView> createState() => FynkelViewState();
}

/// A [FynkelView]'s state: its [world], for code outside the game, such
/// as a HUD (`GlobalKey<FynkelViewState>().currentState!.world`).
class FynkelViewState extends State<FynkelView> with SingleTickerProviderStateMixin {
  World? _own;

  /// The world shown: the view's own, which its root was added to, or the
  /// one it was given.
  World get world => widget.world ?? _own!;

  late final Ticker _ticker = createTicker(_tick)..start();
  int? _lastMicros;
  late final _input = InputFeed(() => world.input);
  FocusNode? _ownFocus;
  FocusNode get _focus => widget.focusNode ?? (_ownFocus ??= FocusNode(debugLabel: 'FynkelView'));

  /// The size the next frame is drawn at, from the last layout; 0 while it
  /// is empty.
  int _width = 0, _height = 0;

  WorldView? _view;
  Future<int?>? _registration;
  int? _texture;

  /// Android: the size the plugin was last given.
  int _sentWidth = 0, _sentHeight = 0;
  Object? _error;

  /// Whether the frames go on without a picture: there is no plugin (a
  /// widget test), or the view failed ([_error]). The world still runs.
  bool _noPicture = false;

  // When the view's own frame last reported an error, so one that comes
  // every frame is reported once a second.
  int? _reportedMicros;

  // What the game's code threw, shown over it (showErrors).
  StreamSubscription<ObjectError>? _errorsHeard;
  final _gameErrors = <ObjectError>[];
  bool _built = false;

  // The current stats window.
  int _windowStartMicros = 0, _windowTicks = 0;
  ViewStats _windowStart = const ViewStats();

  @override
  void initState() {
    super.initState();
    _ticker; // Starts it.
    _own = _makeOwn();
    if (widget.world case final world?) _listen(world);
  }

  /// A world with the widget's root in it, if it has one. It's heard
  /// before the root is added, which may throw at once.
  World? _makeOwn() {
    final root = widget.root;
    if (root == null) return null;
    final world = World(source: widget.source ?? bundleSource());
    _listen(world);
    return world..add(root);
  }

  void _listen(World world) {
    _errorsHeard?.cancel();
    _errorsHeard = null;
    _gameErrors.clear();
    if (widget.showErrors) _errorsHeard = world.errors.listen(_heard);
  }

  void _heard(ObjectError error) {
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: error.error,
        stack: error.stackTrace,
        library: 'fynkel',
        context: ErrorDescription('in ${error.object ?? 'the world'}'),
      ),
    );
    if (_gameErrors.length == 100) _gameErrors.removeAt(0);
    _gameErrors.add(error);
    // Before the first build (a root's onLoad throwing in initState) the
    // build reads them; during one, the next frame does.
    if (!mounted || !_built) return;
    if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
      SchedulerBinding.instance.addPostFrameCallback((_) {
        if (mounted) setState(() {});
      });
    } else {
      setState(() {});
    }
  }

  @override
  void didUpdateWidget(FynkelView oldWidget) {
    super.didUpdateWidget(oldWidget);
    // A new root alone changes nothing (see FynkelView); a new world, or
    // from a root to a world or back, starts the view over.
    if (oldWidget.world != widget.world) {
      _release();
      _own?.dispose();
      _own = _makeOwn();
      if (widget.world case final world?) _listen(world);
    }
  }

  @override
  void dispose() {
    _errorsHeard?.cancel();
    _ticker.dispose();
    _ownFocus?.dispose();
    _release();
    // A view still drawing keeps what it last had until it's freed.
    _own?.dispose();
    super.dispose();
  }

  /// Runs a frame. Whatever throws is reported and the next frame comes
  /// all the same: a ticker whose callback throws would stop for good.
  void _tick(Duration elapsed) {
    final micros = elapsed.inMicroseconds;
    try {
      _frame(micros);
    } catch (error, stack) {
      final reported = _reportedMicros;
      if (reported != null && micros - reported < 1000000) return;
      _reportedMicros = micros;
      FlutterError.reportError(
        FlutterErrorDetails(
          exception: error,
          stack: stack,
          library: 'fynkel_flutter',
          context: ErrorDescription("in FynkelView's frame (the next frame comes all the same)"),
        ),
      );
    }
  }

  void _frame(int micros) {
    final last = _lastMicros;
    _lastMicros = micros;
    final dt = last == null ? 0.0 : (micros - last) / 1e6;
    widget.onFrame?.call(dt);
    final world = this.world;
    if (!world.isDisposed) world.update(dt);
    // Nothing to draw before there is a size (on Android, before the
    // window has one): no view yet, and no surface.
    if (_noPicture || _width == 0 || _height == 0) return;

    final view = _view ?? _open();
    if (view == null) return;
    if (!_android) view.resize(_width, _height);
    final texture = _texture;
    if (texture == null) return;
    if (_android) _sendSize(view, texture);
    view.frame();
    widget.onViewStats?.call(view.stats);

    _windowTicks++;
    final onStats = widget.onStats;
    if (onStats != null && micros - _windowStartMicros >= 1000000) {
      final now = view.stats;
      if (_windowStartMicros != 0) {
        onStats(
          FynkelViewStats(
            window: Duration(microseconds: micros - _windowStartMicros),
            ticks: _windowTicks,
            view: now - _windowStart,
            size: Size(_width.toDouble(), _height.toDouble()),
          ),
        );
      }
      _windowStartMicros = micros;
      _windowTicks = 0;
      _windowStart = now;
    }
  }

  /// Makes the engine view and starts registering its texture.
  WorldView? _open() {
    final WorldView view;
    try {
      if (_android) {
        view = Platform.isAndroid
            ? AndroidSurfaceView(world, width: _width, height: _height)
            : HeadlessView(world, width: _width, height: _height);
      } else if (Platform.isMacOS || Platform.isIOS) {
        view = AppleView(world, width: _width, height: _height);
      } else {
        throw UnsupportedError('FynkelView runs on macOS, iOS and Android.');
      }
    } catch (error, stack) {
      _fail(error, stack);
      return null;
    }
    _view = view;
    _registration = _register(view);
    return view;
  }

  Future<int?> _register(WorldView view) async {
    try {
      if (!_resetSent) {
        _resetSent = true;
        await channel.invokeMethod<void>('reset');
      }
      final Map<String, Object> arguments;
      if (_android) {
        _sentWidth = view.width;
        _sentHeight = view.height;
        arguments = {'view': view.address, 'width': view.width, 'height': view.height};
      } else {
        arguments = {
          'view': view.address,
          'acquire': AppleView.acquireAddress,
          'setFrameReady': AppleView.setFrameReadyAddress,
          'free': AppleView.freeAddress,
        };
      }
      final texture = await channel.invokeMethod<int>('register', arguments);
      if (mounted && _view == view) setState(() => _texture = texture);
      return texture;
    } on MissingPluginException {
      // No plugin: a widget test, most likely. The game runs on unseen.
      if (mounted && _view == view) _withoutPicture();
      return null;
    } catch (error, stack) {
      if (mounted && _view == view) _fail(error, stack);
      return null;
    }
  }

  /// Android: gives the plugin the new size, which brings a new surface.
  /// The plugin handles the calls in order, so the last size sent wins.
  void _sendSize(WorldView view, int texture) {
    if (_width == _sentWidth && _height == _sentHeight) return;
    _sentWidth = _width;
    _sentHeight = _height;
    unawaited(
      channel.invokeMethod<void>('resize', {'texture': texture, 'width': _width, 'height': _height}).catchError((
        Object error,
        StackTrace stack,
      ) {
        if (mounted && _view == view) _fail(error, stack);
      }),
    );
  }

  /// The view can't show the game: the error shows in its place, and the
  /// world runs on, so a HUD over it still works.
  void _fail(Object error, StackTrace stack) {
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: error,
        stack: stack,
        library: 'fynkel_flutter',
        context: ErrorDescription('in FynkelView'),
      ),
    );
    _withoutPicture();
    setState(() => _error = error);
  }

  /// Lets go of the engine view; the world's frames go on without it.
  void _withoutPicture() {
    _noPicture = true;
    _release();
  }

  /// Lets go of the engine view: the plugin stops using it first, then it
  /// is freed. Safe while the registration is still on its way.
  void _release() {
    final view = _view, registration = _registration;
    _view = null;
    _registration = null;
    _texture = null;
    _windowStartMicros = 0;
    if (view == null || registration == null) return;
    unawaited(() async {
      final texture = await registration;
      if (texture != null) await channel.invokeMethod<void>('unregister', {'texture': texture});
      view.dispose();
    }());
  }

  @override
  Widget build(BuildContext context) {
    final error = _error;
    if (error != null) return ErrorWidget(error);
    _built = true;
    final ratio = MediaQuery.devicePixelRatioOf(context) * widget.resolutionScale;
    final view = LayoutBuilder(
      builder: (context, constraints) {
        if (!constraints.hasBoundedWidth || !constraints.hasBoundedHeight) {
          throw FlutterError(
            'FynkelView needs a bounded size, but got $constraints.\n'
            'Put it in a SizedBox, an Expanded or anything else that gives it a size.',
          );
        }
        final size = constraints.biggest;
        if (!world.isDisposed) _input.resize(size);
        _width = (size.width * ratio).round().clamp(0, 16384);
        _height = (size.height * ratio).round().clamp(0, 16384);
        final texture = _texture;
        return texture == null ? const SizedBox.expand() : Texture(textureId: texture);
      },
    );
    final game = Focus(
      focusNode: _focus,
      autofocus: widget.autofocus,
      onKeyEvent: (_, event) {
        // A debug build's restart key goes up to the game's FynkelGame.
        if (kDebugMode && event.logicalKey == LogicalKeyboardKey.f5 && FynkelGame.maybeOf(context) != null) {
          return KeyEventResult.ignored;
        }
        return _input.key(event);
      },
      onFocusChange: (focused) {
        if (!focused) _input.releaseKeys();
      },
      child: Listener(
        behavior: HitTestBehavior.opaque,
        onPointerDown: (event) {
          _focus.requestFocus();
          _input.pointerDown(event);
        },
        onPointerMove: _input.pointerMove,
        onPointerUp: _input.pointerUp,
        onPointerCancel: _input.pointerCancel,
        onPointerSignal: (event) {
          if (event is PointerScrollEvent) {
            GestureBinding.instance.pointerSignalResolver.register(event, (_) => _input.scrolled(event.scrollDelta));
          }
        },
        onPointerPanZoomStart: _input.panZoomStart,
        onPointerPanZoomUpdate: _input.panZoomUpdate,
        child: MouseRegion(onEnter: _input.hover, onHover: _input.hover, onExit: _input.exit, child: view),
      ),
    );
    // Passthrough: an unbounded size still reaches the view's own check.
    return Stack(
      fit: StackFit.passthrough,
      children: [
        game,
        if (_gameErrors.isNotEmpty)
          ErrorPanel(errors: List.of(_gameErrors), onDismiss: () => setState(_gameErrors.clear)),
      ],
    );
  }
}
