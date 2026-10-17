import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart' show Material, MaterialType, Theme;
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart'
    show
        AssetSource,
        Camera,
        FynkelException,
        FynkelStatus,
        GameObject,
        HeadlessView,
        Input,
        ObjectError,
        Quality,
        ViewStats,
        WorldView,
        World;
import 'package:fynkel/platform.dart';

import 'error_panel.dart';
import 'fynkel_game.dart';
import 'input_feed.dart';
import 'world_listenables.dart' show WorldTheme;
import 'world_widget.dart' show FrameTicks, WorldWidgetLayer;

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

/// iOS: the game draws straight to a layer of its own under Flutter's view,
/// as Android's surface (docs/ios-surface.md); a spike behind
/// `--define FYNKEL_IOS_LAYER=true` for now.
bool get _iosLayer => Platform.isIOS && const bool.fromEnvironment('FYNKEL_IOS_LAYER');

/// Whether the plugin gives the view where it draws, and with it the size
/// (Android's surface, iOS's layer), rather than the engine making it.
bool get _surfaced => _android || _iosLayer;

/// Whether this isolate has told the plugin to free the views an earlier
/// isolate left (a hot restart disposes nothing). It does so before its
/// first view registers. The plugin handles a channel's calls in the order
/// they were sent, so a `register` sent after `reset` always comes after
/// it, even while the first is still waiting for the answer.
bool _resetSent = false;

/// Tells the plugin to free the views an earlier isolate left, once per
/// isolate: [FynkelGame] does it as it starts, so a hot restart into a game
/// that shows no view yet leaks none, and the first view does it too. With
/// no plugin (a widget test) there's nothing to free.
Future<void> resetPlugin() async {
  if (_resetSent) return;
  _resetSent = true;
  try {
    await channel.invokeMethod<void>('reset');
  } on MissingPluginException {
    // Nothing to free.
  }
}

/// Forgets that this isolate reset the plugin, so a test can check that it
/// does.
@visibleForTesting
void forgetReset() => _resetSent = false;

/// The views showing each texture, for the plugin's calls about one.
final _byTexture = <int, FynkelViewState>{};
bool _listening = false;

/// Listens for the plugin's calls: `failed` when the Android plugin
/// couldn't hand a view its new surface.
void _listenToPlugin() {
  if (_listening) return;
  _listening = true;
  channel.setMethodCallHandler((call) async {
    if (call.method != 'failed') throw MissingPluginException();
    final args = call.arguments as Map;
    _byTexture[args['texture'] as int]?._replace(
      PlatformException(code: 'attach', message: args['message'] as String?),
      StackTrace.current,
    );
  });
}

/// How many engine views the [FynkelView]s made and haven't freed yet, so
/// a test can check none is left behind.
@visibleForTesting
int get debugViewsOpen => _viewsOpen;
int _viewsOpen = 0;

/// What a [FynkelView] did in one stats window (about a second).
@immutable
class FynkelViewStats {
  /// What a view did in a window of [window], as [FynkelView.onStats]
  /// gives it.
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
/// A [FynkelGame]'s root is shown in the game's world, which the game
/// makes and disposes, and so is a root already in a world (another
/// view's: a minimap beside it), which its maker disposes. Any other root,
/// the view adds to a world of its own, which wears the theme around the
/// view (`world.theme`) from before the root is added. The root is used once, when the view is first
/// built: to change level,
/// destroy the old root and add another to its world (`root.world`); to
/// start over, make the root in a [FynkelGame] (as [FynkelApp] does) and
/// restart it. The world is [FynkelViewState.world] too, for code outside
/// the game.
///
/// `FynkelView.world(world)` shows a world made and driven by hand
/// instead, for tests and tools; the world is the caller's to dispose.
/// [onFrame] is called each frame with the seconds since the last one,
/// before the world's loop.
///
/// It gives the world's [World.input] Flutter's events as they come. It
/// reads pointers with a `Listener`, which takes part in no gesture
/// arena, so gesture widgets around it keep working, and a widget over it
/// (a HUD's button) keeps the pointers it takes from the game. While it
/// has focus it gives the game every key, and keeps those the game uses
/// ([Input.usesKey]: bound to an axis or action, or asked about), so the
/// arrows and space move the player rather than focus; the others go on
/// to Flutter's shortcuts too.
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
/// **Several views of one world** (a minimap over the game, split screen)
/// each draw it from their own [camera], or from the world's main one
/// ([World.camera]) when it's null, and the world is updated once a frame
/// however many show it. Each draws its frames at its own size and
/// cost; the world's meshes and textures are on the GPU once. A view that
/// doesn't take [input] keeps the taps on it from the game and from the
/// views under it; one that does aims what's under a finger from its
/// camera ([Input.camera]).
///
/// macOS, iOS and Android.
class FynkelView extends StatefulWidget {
  /// Shows [root] in a world of the view's own, loading assets from
  /// [source].
  const FynkelView({
    super.key,
    required GameObject this.root,
    this.source,
    this.onFrame,
    this.resolutionScale = 1.0,
    this.onStats,
    this.onViewStats,
    this.focusNode,
    this.autofocus = true,
    this.showErrors = kDebugMode,
    this.pauseWhenCovered = true,
    this.camera,
    this.input = true,
    this.worldWidgets = true,
    this.androidSurface = true,
  }) : world = null,
       assert(resolutionScale > 0 && resolutionScale <= 4);

  /// Shows [world], made and driven by hand: the caller disposes of it.
  const FynkelView.world(
    World this.world, {
    super.key,
    this.onFrame,
    this.resolutionScale = 1.0,
    this.onStats,
    this.onViewStats,
    this.focusNode,
    this.autofocus = true,
    this.showErrors = kDebugMode,
    this.pauseWhenCovered = true,
    this.camera,
    this.input = true,
    this.worldWidgets = true,
    this.androidSurface = true,
  }) : root = null,
       source = null,
       assert(resolutionScale > 0 && resolutionScale <= 4);

  /// The game: shown in its [FynkelGame]'s world, or in the world it's in
  /// already, or else added to the view's own world when the view is first
  /// built.
  final GameObject? root;

  /// Where the view's own world loads assets from; the app's bundle
  /// ([bundleSource]) when null. A [FynkelGame]'s root loads from the
  /// game's: give it there.
  final AssetSource? source;

  /// A world made by hand ([FynkelView.world]), shown in place of a
  /// [root]'s own.
  final World? world;

  /// Called once per frame of the world's ([World.targetFrameRate]),
  /// before its [World.update], with the seconds since the previous call
  /// (0 the first time). For
  /// code that drives the world from outside; a game's own code goes in
  /// its objects.
  final void Function(double dt)? onFrame;

  /// Pixels drawn per physical pixel: above 0 and at most 4, 1 unless
  /// given. Below 1 is faster and softer. It multiplies the world's
  /// [Quality.renderScale], which a game sets for all its views (a
  /// settings page); this is for one view of its own, a minimap say.
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

  /// Whether a page or dialog over the view pauses the game
  /// ([World.paused]): a pause menu, a settings page, a pop-up. Closing
  /// the last one goes on, unless the game was paused already, or paused
  /// itself meanwhile. Covered, the view gets no taps or keys either, and,
  /// as nothing in it moves, it's drawn only 10 times a second, which keeps
  /// a phone cool behind a menu.
  final bool pauseWhenCovered;

  /// The camera the view draws from, read each frame; the world's main
  /// one ([World.camera]) when null. While it isn't a shown object's in
  /// the world (not added yet, destroyed, inactive) the view is black: a
  /// minimap whose camera goes doesn't show the main picture instead.
  final Camera? camera;

  /// Whether the view gives the world the player's taps and keys (and
  /// takes focus for them), and shows what the game's code throws
  /// ([showErrors]). Off for a minimap or the second half of a split
  /// screen: one view takes input, and the errors show once.
  final bool input;

  /// Whether the world's [WorldWidget]s show over the view, placed by its
  /// camera: health bars on each half of a split screen. Off for a
  /// minimap.
  final bool worldWidgets;

  /// Android: whether the view draws straight to the screen, behind
  /// Flutter's own view, when the app's activity is a `FynkelActivity`
  /// (the guide, §1). There Flutter never waits for the game's frames,
  /// which on a slower phone is the difference between 40 and 60 fps
  /// (docs/android-surface-report.md). Widgets over the view draw over it;
  /// whatever is painted under it doesn't show. What it can't do: follow a
  /// transform, a clip or an opacity from the widgets above it (a page
  /// that fades or zooms in shows the game unfaded; one that slides is
  /// followed), and stack more than two views on one another (a minimap
  /// over the game is two). Off, or with another activity, the view is a
  /// texture Flutter draws, as on the other platforms, with no such limits.
  final bool androidSurface;

  @override
  State<FynkelView> createState() => FynkelViewState();
}

/// A [FynkelView]'s state: its [world], for code outside the game, such
/// as a HUD (`GlobalKey<FynkelViewState>().currentState!.world`).
class FynkelViewState extends State<FynkelView> with SingleTickerProviderStateMixin, WidgetsBindingObserver {
  // The root's world, and whether the view made it (and so disposes it);
  // the FynkelGame that made it, if one did.
  World? _rootWorld;
  bool _owns = false;
  FynkelGameState? _game;

  /// The world shown: the root's (the view's own, which it added the root
  /// to, or a [FynkelGame]'s), or the one it was given.
  World get world => widget.world ?? _rootWorld!;

  late final Ticker _ticker = createTicker(_tick)..start();
  // Moves the world widgets, after each update.
  final _ticks = FrameTicks();
  int? _lastMicros;
  late final _input = InputFeed(() => world.input);
  FocusNode? _ownFocus;
  FocusNode get _focus => widget.focusNode ?? (_ownFocus ??= FocusNode(debugLabel: 'FynkelView'));

  /// The size the last layout gave, in logical pixels, and the physical
  /// pixels per logical one times [FynkelView.resolutionScale].
  Size _logical = Size.zero;
  double _ratio = 1;

  /// The size to draw at, in pixels, from the layout's and the quality's
  /// [Quality.renderScale]; 0 while it is empty.
  int _layoutWidth = 0, _layoutHeight = 0;

  /// The size the next frame is drawn at: the layout's once it has held
  /// for [_settleFrames] frames (at once for the first), so a window drag
  /// or a split-view animation makes one new set of surfaces, not one a
  /// frame. Meanwhile the last picture is stretched to fit.
  int _width = 0, _height = 0;
  int _steadyFrames = 0;
  static const _settleFrames = 3;

  WorldView? _view;
  Future<int?>? _registration;
  int? _texture;

  /// Android: the size the plugin was last given.
  int _sentWidth = 0, _sentHeight = 0;

  /// Android: the view draws straight to the screen, to a surface behind
  /// Flutter's view ([FynkelView.androidSurface]), not to a texture. The
  /// plugin says so when the view registers.
  bool _onScreen = false;

  /// Android, on screen: where the surface was last put, in physical
  /// pixels from the top left of Flutter's view, and whether it was put
  /// over the other views.
  Rect? _sentPlace;
  bool _sentOver = false;
  Object? _error;

  /// Whether the frames go on without a picture: there is no plugin (a
  /// widget test), or the view failed ([_error]). The world still runs.
  bool _noPicture = false;

  // Whether the world was paused at the last frame.
  bool _wasPaused = false;

  // When the view's own frame last reported an error, so one that comes
  // every frame is reported once a second.
  int? _reportedMicros;

  // When the GPU device was lost in the last minute: a loss now and then (a
  // driver reset) brings a new view, one that keeps coming fails the view.
  final _losses = <int>[];

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
    WidgetsBinding.instance.addObserver(this);
  }

  // The world whose clock counts this view.
  World? _clocked;

  /// Counts the view on its world's clock, and off the last one's.
  void _clock() {
    final world = this.world;
    if (identical(world, _clocked)) return;
    if (_clocked case final old?) _WorldClock.of(old).views--;
    _WorldClock.of(world).views++;
    _clocked = world;
  }

  var _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // The first time, after initState: the theme can be read from here on.
    if (!_started) {
      _started = true;
      _rootWorld = _worldOfRoot();
      if (widget.world case final world?) _listen(world);
      _clock();
    }
    _wearTheme();
    _cover();
  }

  // Whether a route is over the view's, and whether that paused the world.
  bool _covered = false, _pausedIt = false;

  /// The frames a second of a world the view paused when a route covered
  /// it ([FynkelView.pauseWhenCovered]): enough for a resize to show.
  static const _coveredFrameRate = 10;

  /// Pauses the world when a route covers the view's, and goes on when
  /// it's uncovered.
  void _cover() {
    final covered = !(ModalRoute.of(context)?.isCurrent ?? true) && widget.pauseWhenCovered;
    if (covered == _covered) return;
    _covered = covered;
    // After this frame: pausing tells widgets, which can't build again in
    // the middle of this build.
    SchedulerBinding.instance.addPostFrameCallback((_) {
      if (!mounted || covered != _covered) return;
      final world = this.world;
      if (world.isDisposed) return;
      if (covered) {
        if (!world.paused) {
          world.paused = true;
          _pausedIt = true;
        }
      } else {
        _letGo(world);
      }
    });
  }

  /// Goes on, if covering paused [world].
  void _letGo(World world) {
    if (!_pausedIt) return;
    _pausedIt = false;
    if (world.paused && !world.isDisposed) world.paused = false;
  }

  /// Gives the world the app's theme, unless a [FynkelGame] does: the view
  /// made it, or was given it.
  void _wearTheme() {
    final theme = Theme.of(context);
    if (_owns) {
      _rootWorld!.theme = theme;
    } else if (widget.world case final world? when !identical(world, FynkelGame.maybeOf(context)?.world)) {
      world.theme = theme;
    }
  }

  /// Tells the world when the app goes away and comes back
  /// ([World.applicationPaused]).
  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final world = widget.world ?? _rootWorld;
    if (world == null || world.isDisposed) return;
    switch (state) {
      case AppLifecycleState.hidden || AppLifecycleState.paused:
        world.applicationPaused = true;
      case AppLifecycleState.resumed:
        world.applicationPaused = false;
      case AppLifecycleState.inactive || AppLifecycleState.detached:
        // On the way, or gone: what it was stays.
        break;
    }
  }

  /// The world of the widget's root, if it has one: its [FynkelGame]'s,
  /// or else one the view makes and adds it to. It's heard before the root is added, which may throw at once.
  World? _worldOfRoot() {
    final root = widget.root;
    _owns = false;
    if (root == null) return null;
    final game = FynkelGame.maybeOf(context);
    if (game != null && identical(game.game, root)) {
      if (widget.source != null) {
        // Reported, not thrown: the view goes on with the game's world.
        FlutterError.reportError(
          FlutterErrorDetails(
            exception: ArgumentError.value(
              widget.source,
              'source',
              "$root is a FynkelGame's, whose world loads its assets: give source: to the FynkelGame, not its view",
            ),
            library: 'fynkel_flutter',
          ),
        );
      }
      // The game hears its world's errors from the start (what onLoad
      // threw, before this view was made), and passes them on.
      _listen(null);
      // A view without input leaves the game's errors to the one with it.
      if (widget.input) {
        _game = game;
        hearGameErrors(game, widget.showErrors ? _heard : null);
      }
      return game.world;
    }
    // In a world already, as another view's root (a minimap beside the
    // game): shown there, and left to whoever made that world.
    if (root.isAlive) {
      _listen(root.world);
      return root.world;
    }
    final world = World(source: widget.source ?? bundleSource())..theme = Theme.of(context);
    _owns = true;
    _listen(world);
    return world..add(root);
  }

  void _listen(World? world) {
    _errorsHeard?.cancel();
    _errorsHeard = null;
    _gameErrors.clear();
    if (_game case final game?) stopHearingGameErrors(game, _heard);
    _game = null;
    if (widget.showErrors && widget.input && world != null) _errorsHeard = world.errors.listen(_heard);
  }

  void _heard(ObjectError error) {
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: error.error,
        stack: error.stackTrace,
        library: 'fynkel',
        context: ErrorDescription('in ${error.gameObject ?? 'the world'}'),
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
      _letGo(oldWidget.world ?? _rootWorld!);
      _covered = false;
      _release();
      if (_owns) _rootWorld?.dispose();
      _rootWorld = _worldOfRoot();
      if (widget.world case final world?) _listen(world);
      _clock();
      _wearTheme();
    }
    _cover();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    if (_clocked case final world?) _WorldClock.of(world).views--;
    _letGo(world);
    _listen(null);
    _ticker.dispose();
    _ticks.dispose();
    _ownFocus?.dispose();
    _release();
    // A view still drawing keeps what it last had until it's freed.
    if (_owns) _rootWorld?.dispose();
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
    final world = this.world;
    final clock = _WorldClock.of(world);
    // Not one of the world's frames ([World.targetFrameRate]): nothing runs
    // or is drawn, and the last picture stays. Paused under a menu, the
    // picture doesn't change, so it's drawn only [_coveredFrameRate] times
    // a second.
    if (!world.isDisposed) {
      final rate = world.targetFrameRate;
      final covered = _pausedIt && world.paused && (rate == 0 || rate > _coveredFrameRate);
      if (!clock.due(covered ? _coveredFrameRate : rate)) return;
    }
    final last = _lastMicros;
    _lastMicros = micros;
    final dt = last == null ? 0.0 : (micros - last) / 1e6;
    widget.onFrame?.call(dt);
    if (!world.isDisposed) {
      // A pause let go of every key: those still down are held again.
      final paused = world.paused;
      if (_wasPaused && !paused && _focus.hasFocus) _input.holdKeys();
      _wasPaused = paused;
      clock.update(world);
    }
    _ticks.tick();
    _sizeFromLayout();
    _settle();
    // Nothing to draw before there is a size (on Android, before the
    // window has one): no view yet, and no surface.
    if (_noPicture || _width == 0 || _height == 0) return;

    final view = _view ?? _open();
    if (view == null) return;
    try {
      view.camera = widget.camera;
      if (!_surfaced) view.resize(_width, _height);
      final texture = _texture;
      if (texture == null) return;
      if (_surfaced) _sendSize(view, texture);
      if (_onScreen) _sendPlace(texture);
      view.frame();
    } on FynkelException catch (error, stack) {
      if (error.status != FynkelStatus.deviceLost && error.status != FynkelStatus.workerFailed) rethrow;
      _lost(error, stack, micros);
      return;
    }
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

  /// Works out [_layoutWidth] and [_layoutHeight]: each frame, so a new
  /// [World.quality] draws at its own scale without a new layout.
  void _sizeFromLayout() {
    final world = this.world;
    final scale = world.isDisposed ? 1.0 : (world.quality ?? (_deviceQuality ??= Quality.device)).renderScale;
    final width = (_logical.width * _ratio * scale).round().clamp(0, 16384);
    final height = (_logical.height * _ratio * scale).round().clamp(0, 16384);
    if (width != _layoutWidth || height != _layoutHeight) _steadyFrames = 0;
    _layoutWidth = width;
    _layoutHeight = height;
  }

  void _settle() {
    if (_layoutWidth == _width && _layoutHeight == _height) {
      _steadyFrames = 0;
      return;
    }
    if (_width == 0 || _height == 0 || ++_steadyFrames >= _settleFrames) {
      _width = _layoutWidth;
      _height = _layoutHeight;
      _steadyFrames = 0;
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
      } else if (_iosLayer) {
        view = AppleLayerView(world, width: _width, height: _height);
      } else if (Platform.isMacOS || Platform.isIOS) {
        view = AppleView(world, width: _width, height: _height);
      } else {
        throw UnsupportedError('FynkelView runs on macOS, iOS and Android.');
      }
    } catch (error, stack) {
      _fail(error, stack);
      return null;
    }
    _viewsOpen++;
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
      if (_iosLayer) {
        _sentWidth = view.width;
        _sentHeight = view.height;
        final place = _place();
        arguments = {
          'view': view.address,
          'width': view.width,
          'height': view.height,
          'layer': true,
          'attach': AppleLayerView.attachAddress,
          'detach': AppleLayerView.detachAddress,
          'free': AppleView.freeAddress,
          'onScreen': true,
          'left': place.left.round(),
          'top': place.top.round(),
          'frameWidth': place.width.round(),
          'frameHeight': place.height.round(),
        };
      } else if (_android) {
        _sentWidth = view.width;
        _sentHeight = view.height;
        final place = widget.androidSurface ? _place() : null;
        arguments = {
          'view': view.address,
          'width': view.width,
          'height': view.height,
          if (place != null) ...{
            'onScreen': true,
            'left': place.left.round(),
            'top': place.top.round(),
            'frameWidth': place.width.round(),
            'frameHeight': place.height.round(),
          },
        };
      } else {
        arguments = {
          'view': view.address,
          'acquire': AppleView.acquireAddress,
          'setFrameReady': AppleView.setFrameReadyAddress,
          'free': AppleView.freeAddress,
        };
      }
      if (_surfaced) _listenToPlugin();
      // A texture's id, or for a surface on screen {id, onScreen}.
      final answer = await channel.invokeMethod<Object>('register', arguments);
      final onScreen = answer is Map && answer['onScreen'] == true;
      final texture = answer is Map ? answer['id'] as int? : answer as int?;
      if (mounted && _view == view) {
        if (texture != null) _byTexture[texture] = this;
        if (onScreen) {
          _sentPlace = arguments.containsKey('left')
              ? Rect.fromLTWH(
                  (arguments['left']! as int).toDouble(),
                  (arguments['top']! as int).toDouble(),
                  (arguments['frameWidth']! as int).toDouble(),
                  (arguments['frameHeight']! as int).toDouble(),
                )
              : null;
          _sentOver = false;
        }
        setState(() {
          _texture = texture;
          _onScreen = onScreen;
        });
      }
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
        if (mounted && _view == view) _replace(error, stack);
      }),
    );
  }

  /// Android, on screen: where the view is, in physical pixels from the
  /// top left of Flutter's view, rounded as the plugin places it.
  Rect _place() {
    final box = context.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize || !box.attached) return Rect.zero;
    final ratio = MediaQuery.devicePixelRatioOf(context);
    final topLeft = box.localToGlobal(Offset.zero) * ratio;
    final size = box.size * ratio;
    return Rect.fromLTWH(
      topLeft.dx.roundToDouble(),
      topLeft.dy.roundToDouble(),
      size.width.roundToDouble(),
      size.height.roundToDouble(),
    );
  }

  /// Android, on screen: moves the surface to where the view now is (a
  /// page sliding in, a resize), and over the other views when it lies
  /// within one of them (a minimap over the game).
  void _sendPlace(int texture) {
    final place = _place();
    if (place != _sentPlace) {
      _sentPlace = place;
      unawaited(
        channel.invokeMethod<void>('place', {
          'texture': texture,
          'left': place.left.round(),
          'top': place.top.round(),
          'width': place.width.round(),
          'height': place.height.round(),
        }),
      );
    }
    final over = _byTexture.values.any(
      (other) =>
          other != this &&
          other._onScreen &&
          other._sentPlace != null &&
          other._sentPlace != place &&
          other._sentPlace!.expandToInclude(place) == other._sentPlace,
    );
    if (over != _sentOver) {
      _sentOver = over;
      unawaited(channel.invokeMethod<void>('over', {'texture': texture, 'over': over}));
    }
  }

  /// The view can't go on (the plugin couldn't give it a new surface): a
  /// new one takes its place, as for a lost device.
  void _replace(Object error, StackTrace stack) {
    if (!mounted || _view == null) return;
    _lost(error, stack, _lastMicros ?? 0);
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

  /// The GPU device was lost, the render worker stopped, or the plugin
  /// couldn't give the view a new surface: a new view (with a new device,
  /// when it was lost) takes the old one's place from the next frame and
  /// uploads everything again. More than [_maxLosses] in
  /// a minute, and the view fails.
  void _lost(Object error, StackTrace stack, int micros) {
    _losses
      ..removeWhere((at) => micros - at > 60000000)
      ..add(micros);
    if (_losses.length > _maxLosses) {
      _fail(error, stack);
      return;
    }
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: error,
        stack: stack,
        library: 'fynkel_flutter',
        context: ErrorDescription('in FynkelView (a new view takes its place)'),
      ),
    );
    setState(_release);
  }

  static const _maxLosses = 3;

  /// Lets go of the engine view; the world's frames go on without it.
  void _withoutPicture() {
    _noPicture = true;
    _release();
  }

  /// Lets go of the engine view: the plugin stops using it first, then it
  /// is freed. Safe while the registration is still on its way.
  void _release() {
    final view = _view, registration = _registration;
    _byTexture.remove(_texture);
    _view = null;
    _registration = null;
    _texture = null;
    _onScreen = false;
    _sentPlace = null;
    _windowStartMicros = 0;
    if (view == null || registration == null) return;
    unawaited(() async {
      try {
        final texture = await registration;
        if (texture != null) await channel.invokeMethod<void>('unregister', {'texture': texture});
      } catch (error, stack) {
        // The plugin already let go of it (after a reset, or with the
        // engine detached): it's freed all the same.
        FlutterError.reportError(
          FlutterErrorDetails(
            exception: error,
            stack: stack,
            library: 'fynkel_flutter',
            context: ErrorDescription('while FynkelView let go of its view'),
          ),
        );
      } finally {
        view.dispose();
        _viewsOpen--;
      }
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
        if (widget.input && !world.isDisposed) {
          _input.resize(size);
          world.input.camera = widget.camera;
        }
        _logical = size;
        _ratio = ratio;
        _sizeFromLayout();
        final texture = _texture;
        if (texture == null) return const SizedBox.expand();
        // On screen: the surface is behind Flutter's view, so the view
        // clears whatever was painted under it, and the game shows through.
        if (_onScreen) return const SizedBox.expand(child: CustomPaint(painter: _Window()));
        return Texture(textureId: texture);
      },
    );
    // Without input it keeps the taps on it from what's under it.
    final game = !widget.input
        ? Listener(behavior: HitTestBehavior.opaque, child: view)
        : Focus(
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
              if (focused) {
                _input.holdKeys();
              } else {
                _input.releaseKeys();
              }
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
                  GestureBinding.instance.pointerSignalResolver.register(
                    event,
                    (_) => _input.scrolled(event.scrollDelta),
                  );
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
        // On a see-through Material, as FynkelApp's overlay: text has its
        // style, and buttons have what they paint on.
        if (widget.worldWidgets)
          Positioned.fill(
            child: Material(
              type: MaterialType.transparency,
              child: WorldWidgetLayer(world: world, frames: _ticks, camera: widget.camera),
            ),
          ),
        if (_gameErrors.isNotEmpty)
          ErrorPanel(errors: List.of(_gameErrors), onDismiss: () => setState(_gameErrors.clear)),
      ],
    );
  }
}

/// Clears its rectangle, whatever was painted under it, so the surface
/// behind Flutter's view shows through.
class _Window extends CustomPainter {
  const _Window();

  @override
  void paint(Canvas canvas, Size size) => canvas.drawRect(Offset.zero & size, Paint()..blendMode = BlendMode.clear);

  @override
  bool shouldRepaint(_Window oldDelegate) => false;
}

/// A world's frames, shared by the views that show it: the first of them
/// to tick in a display frame updates the world, the rest draw what that
/// made. The seconds between updates start from 0 when its first view
/// comes, as they did for a view alone.
// The device's quality level, for a world without its own: looked up
// once, the first time a view is laid out.
Quality? _deviceQuality;

class _WorldClock {
  static final _clocks = Expando<_WorldClock>('FynkelView clock');

  static _WorldClock of(World world) => _clocks[world] ??= _WorldClock();

  // The display frame the world was last updated in; null before the
  // first, and again when its last view goes.
  Duration? _updated;
  int _views = 0;

  int get views => _views;
  set views(int value) {
    _views = value;
    if (value == 0) _updated = _asked = null;
  }

  // The display frame [_due] was worked out in, so every view showing the
  // world gets the same answer in it.
  Duration? _asked;
  bool _due = true;

  /// Whether this display frame is one of the world's, at [rate] a second
  /// (0 for every one): its [World.targetFrameRate], or fewer. One that
  /// comes up to half a display frame early counts, as they come a little
  /// unevenly: at 60 on a 120 Hz screen, every other one.
  bool due(int rate) {
    final now = SchedulerBinding.instance.currentFrameTimeStamp;
    if (now != _asked) {
      final display = _asked == null ? Duration.zero : now - _asked!;
      _asked = now;
      final last = _updated;
      _due = last == null || rate == 0 || (now - last + display ~/ 2).inMicroseconds >= 1000000 / rate;
    }
    return _due;
  }

  void update(World world) {
    final now = SchedulerBinding.instance.currentFrameTimeStamp;
    final last = _updated;
    if (now == last) return;
    _updated = now;
    world.update(last == null ? 0 : (now - last).inMicroseconds / 1e6);
  }
}
