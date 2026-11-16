import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

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
        SignalSubscription,
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

/// Whether views on screen ([FynkelView.directToScreen]) talk to the plugin the
/// iOS way: a layer of their own under Flutter's view, which the plugin
/// makes, with a size and a place. Null for this platform's way. A test
/// sets it to check iOS's way on a Mac, where the view is then a
/// [HeadlessView] standing in for the [AppleLayerView].
@visibleForTesting
bool? debugIos;

bool get _ios => debugIos ?? Platform.isIOS;

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

/// Sets [world]'s [World.applicationPaused] for the app's [state]: away
/// when hidden or paused, back when resumed. Inactive (on the way) and
/// detached (gone) leave it as it was. Not exported.
void tellLifecycle(World world, AppLifecycleState? state) {
  if (world.isDisposed) return;
  switch (state) {
    case AppLifecycleState.hidden || AppLifecycleState.paused:
      world.applicationPaused = true;
    case AppLifecycleState.resumed:
      world.applicationPaused = false;
    case AppLifecycleState.inactive || AppLifecycleState.detached || null:
      break;
  }
}

/// The views in the tree, for the plugin's calls about them all.
final _live = <FynkelViewState>{};

/// Listens for the plugin's calls: `failed` when the Android plugin
/// couldn't hand a view its new surface, and `lowMemory` when Android
/// says it's short of memory (see [FynkelViewState.didHaveMemoryPressure]).
void _listenToPlugin() {
  if (_listening) return;
  _listening = true;
  channel.setMethodCallHandler((call) async {
    if (call.method == 'lowMemory') {
      for (final view in _live.toList()) {
        view._lowMemory();
      }
      return;
    }
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
/// view's: a minimap beside it). Any other root, the view adds to a world
/// of its own, which wears the theme around the view (`world.theme`) from
/// before the root is added. A world a view made goes when the last view
/// showing it goes, so a view Flutter makes again for the same root (one
/// wrapped in a `Stack` now) goes on with the same game.
///
/// The view keeps the root it was first built with
/// ([FynkelViewState.root]), and a debug build says so when it's given
/// another. To change level, destroy the old root and add the next to its
/// world (`root.world`) first; to start over, make the root in a
/// [FynkelGame] (as [FynkelApp] does) and restart it, or give the view a
/// new `key`. The world is [FynkelViewState.world] too, for code outside
/// the game.
///
/// **The bare view**, `FynkelView(root:)` with no [FynkelGame] around it,
/// is for a second view of a game or a quick tool: a game starts with
/// [FynkelApp], or with a [FynkelGame] and its view inside a bigger app.
/// Alone, the view can't start the game over (no `restart`, no F5), and
/// the game's world is there only once the view is built, so a widget
/// beside it can't take `game.world.input` in the same build, as one in a
/// [FynkelGame]'s builder can.
///
/// `FynkelView.world(world)` shows a world made and driven by hand
/// instead, for tests and tools; the world is the caller's to dispose.
/// [onFrame] is called each frame with the seconds since the last one,
/// before the world's loop.
///
/// A hot reload reads the world's shaders and sounds again
/// ([World.reloadAssets]), whichever way it was made; a [FynkelGame] does
/// it for its own.
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
/// [renderScale] and scaled to fit. Below 1 trades sharpness for
/// speed; the view never lowers it by itself.
///
/// **Several views of one world** (a minimap over the game, split screen)
/// each draw it from their own [camera], or from the world's main one
/// ([World.camera]) when it's null, and the world is updated once a frame
/// however many show it. Each draws its frames at its own size and
/// cost; the world's meshes and textures are on the GPU once. One view
/// takes the input: the first of them with [input] on (a debug build says
/// so when two have it). It aims what's under a finger from its camera
/// ([Input.camera]); the others keep the taps on them from the game and
/// from the views under them. The game's errors show in one view too
/// ([showErrors]).
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
    this.renderScale = 1.0,
    this.onStats,
    this.onViewStats,
    this.focusNode,
    this.autofocus = true,
    this.showErrors = kDebugMode,
    this.pauseWhenCovered = true,
    this.camera,
    this.input = true,
    this.worldWidgets = true,
    this.directToScreen = true,
    this.onError,
    this.errorBuilder,
  }) : world = null,
       assert(
         renderScale > 0 && renderScale <= 4,
         'FynkelView.renderScale is $renderScale: give one above 0 and at most 4 (1 draws every pixel)',
       );

  /// Shows [world], made and driven by hand: the caller disposes of it.
  const FynkelView.world(
    World this.world, {
    super.key,
    this.onFrame,
    this.renderScale = 1.0,
    this.onStats,
    this.onViewStats,
    this.focusNode,
    this.autofocus = true,
    this.showErrors = kDebugMode,
    this.pauseWhenCovered = true,
    this.camera,
    this.input = true,
    this.worldWidgets = true,
    this.directToScreen = true,
    this.onError,
    this.errorBuilder,
  }) : root = null,
       source = null,
       assert(
         renderScale > 0 && renderScale <= 4,
         'FynkelView.renderScale is $renderScale: give one above 0 and at most 4 (1 draws every pixel)',
       );

  /// The game: shown in its [FynkelGame]'s world, or in the world it's in
  /// already, or else added to the view's own world when the view is first
  /// built. The view keeps the first it's given ([FynkelViewState.root]).
  final GameObject? root;

  /// Where the view's own world loads assets from; the app's bundle
  /// ([bundleSource]) when null. A root in a world already loads from
  /// that world's: a [FynkelGame]'s, say, is given its source there.
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
  final double renderScale;

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

  /// Called once when the view fails for good and can't show the game: a
  /// GPU device lost or a surface refused more than three times in a
  /// minute, or no view at all. The world runs on (a HUD over it still
  /// works). For a crash reporter; the error is reported to Flutter too.
  final void Function(Object error, StackTrace stack)? onError;

  /// What shows in the view's place once it has failed ([onError]): an
  /// [ErrorWidget] unless given. `retry` starts again with a new view.
  final Widget Function(BuildContext context, Object error, VoidCallback retry)? errorBuilder;

  /// Whether the errors the game's code throws ([World.errors]) are shown
  /// over the game and reported to Flutter (the console, and a failing
  /// widget test), rather than left to the game. On in debug builds.
  /// Of several views of one world, one shows them: the one taking
  /// [input] if it shows errors, else the first that does.
  final bool showErrors;

  /// Whether a page or dialog over the view pauses the game
  /// ([World.paused]): a pause menu, a settings page, a pop-up. Closing
  /// the last one goes on, unless the game was paused already, or set
  /// [World.paused] itself meanwhile (a game over under the settings page),
  /// which it keeps. Covered, the view gets no taps or keys either, and,
  /// as nothing in it moves, it's drawn only 10 times a second, which keeps
  /// a phone cool behind a menu.
  ///
  /// The cover's pause is [World.paused] itself, so a game that sets it to
  /// true while the cover holds it (true already) changes nothing the view
  /// can see, and goes on when the page closes: to stay paused then, set
  /// it once the page has closed.
  final bool pauseWhenCovered;

  /// The camera the view draws from, read each frame; the world's main
  /// one ([World.camera]) when null. While it isn't a shown object's in
  /// the world (not added yet, destroyed, inactive) the view is black: a
  /// minimap whose camera goes doesn't show the main picture instead.
  final Camera? camera;

  /// Whether the view gives the world the player's taps and keys (and
  /// takes focus for them). Off for a minimap or the second half of a
  /// split screen: of the views of one world, only the first with it on
  /// takes input, and a debug build says so when there are two.
  final bool input;

  /// Whether the world's [WorldWidget]s show over the view, placed by its
  /// camera: health bars on each half of a split screen. Off for a
  /// minimap.
  final bool worldWidgets;

  /// Phones: whether the view draws straight to the screen, on a layer of
  /// its own behind Flutter's view, rather than into a texture Flutter
  /// draws (false). On by default. On Android it needs the app's activity to be a
  /// `FynkelActivity` (the guide, §1); on iOS nothing. Flutter then never
  /// waits for the game's frames, which on a slower Android phone is the
  /// difference between 40 and 60 fps (docs/android-surface-report.md),
  /// and never copies them, which keeps an iPhone cool
  /// (docs/ios-surface-report.md), while its own UI goes on at the
  /// display's rate.
  ///
  /// Widgets over the view draw over it; whatever is painted under it
  /// doesn't show. What it can't do: follow a transform, a clip or an
  /// opacity from the widgets above it (a page that fades or zooms in
  /// shows the game unfaded; one that slides is followed), and stack more
  /// than two views on one another (a minimap over the game is two). Off,
  /// or with another activity, and on the Mac, the view is a texture, with
  /// no such limits.
  final bool directToScreen;

  @override
  State<FynkelView> createState() => FynkelViewState();
}

/// A [FynkelView]'s state: its [world], for code outside the game, such
/// as a HUD (`GlobalKey<FynkelViewState>().currentState!.world`).
class FynkelViewState extends State<FynkelView> with SingleTickerProviderStateMixin, WidgetsBindingObserver {
  // The root's world, and the root shown in it.
  World? _rootWorld;
  GameObject? _root;

  // The FynkelGame that made the world shown, if one did; and it again
  // while this view takes the game's errors from it.
  FynkelGameState? _maker, _game;

  /// The world shown: the root's (the view's own, which it added the root
  /// to, or a [FynkelGame]'s), or the one it was given.
  World get world => widget.world ?? _rootWorld!;

  /// The root shown: the one the view was first built with, or the next
  /// one added to its world in its place (a new level); null for a world
  /// given by hand ([FynkelView.world]).
  GameObject? get root => _root;

  late final Ticker _ticker = createTicker(_tick)..start();
  // Moves the world widgets, after each update.
  final _ticks = FrameTicks();
  int? _lastMicros;
  late final _input = InputFeed(() => world.input);
  FocusNode? _ownFocus;
  FocusNode get _focus => widget.focusNode ?? (_ownFocus ??= FocusNode(debugLabel: 'FynkelView'));

  /// The size the last layout gave, in logical pixels, and the physical
  /// pixels per logical one times [FynkelView.renderScale].
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

  /// How the engine view reaches the screen, with what that alone
  /// needs: made on the first frame with a size, null once let go of.
  _Host? _host;

  /// iOS: the plugin couldn't give the view a layer (no Flutter view of its
  /// own on screen: an app that adds Flutter to its own view controllers),
  /// so the view is a texture from then on, whatever host comes next.
  bool _noLayer = false;
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
    _live.add(this);
    // Android's memory warnings come from the plugin (didHaveMemoryPressure).
    if (_android) _listenToPlugin();
  }

  // The world whose clock counts this view.
  World? _clocked;

  // Whether the view is in the tree: one taken out (to be put back
  // elsewhere, or freed at the end of the frame) takes no input and shows
  // no errors.
  bool _active = true;

  /// Counts the view on [world]'s clock, and off the last one's.
  void _clock(World world) {
    if (identical(world, _clocked)) return;
    _unclock();
    _WorldClock.of(world).views.add(this);
    _clocked = world;
  }

  /// Takes the view off its world's clock. A world that views made goes
  /// with the last view showing it, whichever that is: one Flutter made
  /// again for the same root goes on with it.
  void _unclock() {
    final world = _clocked;
    if (world == null) return;
    _clocked = null;
    final clock = _WorldClock.of(world)..leave(this);
    if (clock.views.isEmpty && clock.made && !world.isDisposed) world.dispose();
  }

  var _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // The first time, after initState: the theme can be read from here on.
    if (!_started) {
      _started = true;
      _show();
    }
    _wearTheme();
    _cover();
  }

  @override
  void activate() {
    super.activate();
    _active = true;
    _hearErrors();
  }

  @override
  void deactivate() {
    _active = false;
    _hearErrors();
    super.deactivate();
  }

  // Whether a route is over the view's, and whether that paused the world.
  bool _covered = false, _pausedIt = false;

  // While the view's pause holds: hears the game set [World.paused]
  // itself, which makes the pause the game's.
  SignalSubscription<bool>? _pauseWatch;

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
          _pauseWatch = world.onPausedChanged.listen((_) => _gameTookPause());
        }
      } else {
        _letGo(world);
      }
    });
  }

  /// The game set [World.paused] while the view's pause held: from then on
  /// the pause, or the lack of one, is the game's, and stays when the view
  /// is uncovered.
  void _gameTookPause() {
    _pausedIt = false;
    _pauseWatch?.cancel();
    _pauseWatch = null;
  }

  /// Goes on, if covering paused [world].
  void _letGo(World world) {
    _pauseWatch?.cancel();
    _pauseWatch = null;
    if (!_pausedIt) return;
    _pausedIt = false;
    if (world.paused && !world.isDisposed) world.paused = false;
  }

  /// Gives the world the app's theme, unless a [FynkelGame] does: a view
  /// made it, or this one was given it.
  void _wearTheme() {
    final world = _clocked;
    if (world == null || _maker != null) return;
    if (widget.world != null || _WorldClock.of(world).made) world.theme = Theme.of(context);
  }

  /// Tells the world when the app goes away and comes back
  /// ([World.applicationPaused]).
  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final world = widget.world ?? _rootWorld;
    if (world != null) tellLifecycle(world, state);
  }

  /// The system is short of memory: the world's told
  /// ([World.onLowMemory]), once for all its views. While the app is away
  /// they let go of the images they draw through too. While it's on screen
  /// they'd only make them again at once, with a slow frame.
  ///
  /// On Android this comes from the plugin instead, which hears the
  /// system's level: Flutter's own warning comes for any level from
  /// "running low" up, and so for `TRIM_MEMORY_UI_HIDDEN`, which Android
  /// sends whenever the app goes away (Home), short of memory or not
  /// (playtest audit).
  @override
  void didHaveMemoryPressure() {
    if (!_android) _lowMemory();
  }

  void _lowMemory() {
    final world = widget.world ?? _rootWorld;
    if (world == null || world.isDisposed) return;
    final clock = _WorldClock.of(world);
    if (clock.warned) return;
    clock.warned = true;
    // Every view hears of it before this runs.
    scheduleMicrotask(() => clock.warned = false);
    world.lowMemory();
    if (!world.applicationPaused) return;
    for (final view in clock.views) {
      view._host?.view.releaseImages();
    }
  }

  /// A hot reload: the world's shaders and sounds are read again
  /// ([World.reloadAssets]), once for all its views. A [FynkelGame] does
  /// it for its own world.
  @override
  void reassemble() {
    super.reassemble();
    final world = _clocked;
    if (world == null || world.isDisposed || _maker != null) return;
    final clock = _WorldClock.of(world);
    if (clock.reloading) return;
    clock.reloading = true;
    // Every view is reassembled before this runs.
    scheduleMicrotask(() => clock.reloading = false);
    unawaited(world.reloadAssets());
  }

  /// Shows the world the widget gives, or its root's.
  void _show() {
    final game = FynkelGame.maybeOf(context);
    _root = null;
    _rootWorld = null;
    if (widget.world case final world?) {
      _maker = identical(game?.world, world) ? game : null;
      _clock(world);
      _hearErrors();
    } else {
      _rootWorld = _worldOfRoot(widget.root!, game);
    }
  }

  /// The world of [root]: its [FynkelGame]'s, the world it's in already,
  /// or else one the view makes and adds it to. The errors are heard before
  /// the root is added, which may throw at once.
  World _worldOfRoot(GameObject root, FynkelGameState? game) {
    _root = root;
    _maker = null;
    if (game != null && identical(game.game, root)) {
      if (widget.source != null) {
        _reportSource("$root is a FynkelGame's, whose world loads its assets: give source: to the FynkelGame");
      }
      _maker = game;
      final world = game.world;
      _clock(world);
      // The game hears its world's errors from the start (what onLoad
      // threw, before this view was made), and passes them on to the view
      // that shows them; a view that takes input but shows no errors,
      // with no other view showing them, leaves them to the game.
      if (!_hearErrors() && widget.input && _WorldClock.of(world).errorView == null) hearGameErrors(game, null);
      return world;
    }
    // In a world already, as another view's root (a minimap beside the
    // game): shown there, and freed with it.
    if (root.isAlive) {
      final world = root.world;
      // The view that made it for this root was given its source.
      if (widget.source != null && !_WorldClock.of(world).made) {
        _reportSource('$root is in a World already, which loads its assets: give source: to the World it was added to');
      }
      _clock(world);
      _hearErrors();
      return world;
    }
    final world = World(source: widget.source ?? bundleSource())..theme = Theme.of(context);
    _WorldClock.of(world).made = true;
    _clock(world);
    _hearErrors();
    return world..add(root);
  }

  /// Reports, rather than throws, a [FynkelView.source] that isn't used:
  /// the view goes on with the root's world.
  void _reportSource(String message) {
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: ArgumentError('FynkelView.source is given, but $message, not to its view.'),
        library: 'fynkel_flutter',
      ),
    );
  }

  // Whether the view's widget had another root than the one it shows,
  // which a debug build said.
  bool _toldRoot = false;

  /// The widget gives a root other than the one shown: the next level,
  /// added to the same world, is shown from now on; any other is said
  /// once, in a debug build.
  void _newRoot(GameObject next) {
    if (next.isAlive && identical(next.world, _rootWorld)) {
      _root = next;
      return;
    }
    if (!kDebugMode || _toldRoot) return;
    _toldRoot = true;
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: FlutterError(
          "FynkelView's root changed from $_root to $next, but a FynkelView keeps the game it started with: "
          'give it a Key to start another (FynkelView(key: ValueKey(level), root: ...)), or make the root in a '
          'FynkelGame and restart it. For the next level in the same world, destroy the old root and add the next '
          'to its world (root.world.add) first.',
        ),
        library: 'fynkel_flutter',
      ),
    );
  }

  // Whether this view shows its world's errors: one view of a world does.
  bool _showsErrors = false;

  /// Starts or stops showing the world's errors as this view's turn to
  /// show them ([_WorldClock.errorView]) comes or goes; true while it
  /// shows them.
  bool _hearErrors() {
    final world = _clocked;
    final shows = _active && world != null && !world.isDisposed && identical(_WorldClock.of(world).errorView, this);
    if (shows == _showsErrors) return shows;
    _stopHearing();
    if (!shows) return false;
    _showsErrors = true;
    final game = _maker;
    if (game != null && hearGameErrors(game, _heard)) {
      _game = game;
    } else {
      _errorsHeard = world.errors.listen(_heard);
    }
    return true;
  }

  void _stopHearing() {
    _showsErrors = false;
    _errorsHeard?.cancel();
    _errorsHeard = null;
    _gameErrors.clear();
    if (_game case final game?) stopHearingGameErrors(game, _heard);
    _game = null;
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

  /// Whether the view takes the player's input: of the views of its world,
  /// the first in the tree with [FynkelView.input] on.
  bool get _takesInput {
    final world = _clocked;
    return widget.input && _active && world != null && identical(_WorldClock.of(world).inputView, this);
  }

  // Whether the view took input when it was last built.
  bool _tookInput = false;

  /// Picks up, each frame, a change in which of the world's views takes
  /// its input and shows its errors, as views come and go; and says once
  /// for the world, in a debug build, that two views take input.
  void _roles() {
    final world = _clocked;
    if (world == null) return;
    final showed = _showsErrors;
    _hearErrors();
    final takes = _takesInput;
    if (showed != _showsErrors || takes != _tookInput) setState(() {});
    if (!kDebugMode || takes || !widget.input || !_active) return;
    final clock = _WorldClock.of(world);
    if (clock.toldInput) return;
    clock.toldInput = true;
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: FlutterError(
          'Two FynkelViews of one World take input, and only the first of them does: give input: false to all but '
          'one (a minimap, the other half of a split screen). The one that takes input aims the taps from its camera.',
        ),
        library: 'fynkel_flutter',
      ),
    );
  }

  @override
  void didUpdateWidget(FynkelView oldWidget) {
    super.didUpdateWidget(oldWidget);
    // A new world, or from a root to a world or back, starts the view
    // over; a new root alone is the next level, or a mistake (_newRoot).
    if (oldWidget.world != widget.world) {
      _letGo(oldWidget.world ?? _rootWorld!);
      _covered = false;
      _release();
      _stopHearing();
      _unclock();
      _show();
      _wearTheme();
    } else if (widget.root case final next? when !identical(next, _root)) {
      _newRoot(next);
    }
    _hearErrors();
    _cover();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _live.remove(this);
    _letGo(world);
    _stopHearing();
    _ticker.dispose();
    _ticks.dispose();
    _ownFocus?.dispose();
    _release();
    // A view still drawing keeps what it last had until it's freed.
    _unclock();
    super.dispose();
  }

  /// Runs a frame. Whatever throws is reported and the next frame comes
  /// all the same: a ticker whose callback throws would stop for good.
  void _tick(Duration elapsed) {
    final micros = elapsed.inMicroseconds;
    try {
      _roles();
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

    final host = _host ?? _open();
    if (host == null) return;
    final view = host.view;
    try {
      view.camera = widget.camera;
      if (host is! _Surfaced) view.resize(_width, _height);
      final texture = host.texture;
      if (texture == null) return;
      if (host is _Surfaced) _sendSize(host, texture);
      if (host.onScreen) _sendPlace(host, texture);
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
    // No device here draws a texture past 8192 on a side; the engine
    // refuses a view past what its device draws.
    final width = (_logical.width * _ratio * scale).round().clamp(0, 8192);
    final height = (_logical.height * _ratio * scale).round().clamp(0, 8192);
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

  /// Makes the engine view, in the host this platform shows it with, and
  /// starts registering it.
  _Host? _open() {
    final _Host host;
    try {
      if (_android) {
        final view = Platform.isAndroid
            ? AndroidSurfaceView(world, width: _width, height: _height)
            : HeadlessView(world, width: _width, height: _height);
        host = _AndroidSurface(view, asksOnScreen: widget.directToScreen);
      } else if (_ios && widget.directToScreen && !_noLayer) {
        host = _AppleLayer(
          Platform.isIOS
              ? AppleLayerView(world, width: _width, height: _height)
              : HeadlessView(world, width: _width, height: _height),
        );
      } else if (Platform.isMacOS || Platform.isIOS) {
        host = _AppleTexture(AppleView(world, width: _width, height: _height));
      } else {
        throw UnsupportedError('FynkelView runs on macOS, iOS and Android.');
      }
    } catch (error, stack) {
      _fail(error, stack);
      return null;
    }
    _viewsOpen++;
    _host = host;
    host.registration = _register(host);
    return host;
  }

  Future<int?> _register(_Host host) async {
    try {
      if (!_resetSent) {
        _resetSent = true;
        await channel.invokeMethod<void>('reset');
      }
      final place = host.asksOnScreen ? _place() : null;
      final arguments = host.arguments(place);
      if (host is _Surfaced) _listenToPlugin();
      // A texture's id, or for a surface on screen {id, onScreen}.
      final answer = await channel.invokeMethod<Object>('register', arguments);
      final onScreen = answer is Map && answer['onScreen'] == true;
      final texture = answer is Map ? answer['id'] as int? : answer as int?;
      if (mounted && _host == host) {
        if (texture != null) _byTexture[texture] = this;
        if (onScreen) {
          host
            ..sentPlace = place
            ..sentOver = false;
        }
        setState(() {
          host
            ..texture = texture
            ..onScreen = onScreen;
        });
      }
      return texture;
    } on MissingPluginException {
      // No plugin: a widget test, most likely. The game runs on unseen.
      if (mounted && _host == host) _withoutPicture();
      return null;
    } catch (error, stack) {
      if (host is _AppleLayer && error is PlatformException && error.code == 'nolayer') {
        // No layer here: a texture from the next frame.
        if (mounted && _host == host) {
          _noLayer = true;
          setState(_release);
        }
        return null;
      }
      if (mounted && _host == host) _fail(error, stack);
      return null;
    }
  }

  /// Android and iOS's layer: gives the plugin the new size (a new surface
  /// on Android, the layer set up again on iOS).
  /// The plugin handles the calls in order, so the last size sent wins.
  void _sendSize(_Surfaced host, int texture) {
    if (_width == host.sentWidth && _height == host.sentHeight) return;
    host
      ..sentWidth = _width
      ..sentHeight = _height;
    unawaited(
      channel.invokeMethod<void>('resize', {'texture': texture, 'width': _width, 'height': _height}).catchError((
        Object error,
        StackTrace stack,
      ) {
        if (mounted && _host == host) _replace(error, stack);
      }),
    );
  }

  /// On screen: where the view is, in physical pixels from the
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

  /// On screen: moves the surface or layer to where the view now is (a
  /// page sliding in, a resize), and over the other views when it lies
  /// within one of them (a minimap over the game).
  void _sendPlace(_Host host, int texture) {
    final place = _place();
    if (place != host.sentPlace) {
      host.sentPlace = place;
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
    final over = _byTexture.values.any((other) {
      final at = other._host?.sentPlace;
      return other != this && other._host!.onScreen && at != null && at != place && at.expandToInclude(place) == at;
    });
    if (over != host.sentOver) {
      host.sentOver = over;
      unawaited(channel.invokeMethod<void>('over', {'texture': texture, 'over': over}));
    }
  }

  /// The view can't go on (the plugin couldn't give it a new surface): a
  /// new one takes its place, as for a lost device.
  void _replace(Object error, StackTrace stack) {
    if (!mounted || _host == null) return;
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
    widget.onError?.call(error, stack);
  }

  /// After a failure: a new view from the next frame, with no losses held
  /// against it.
  void _retry() {
    if (!mounted || _error == null) return;
    _losses.clear();
    setState(() {
      _error = null;
      _noPicture = false;
    });
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
    final host = _host;
    _host = null;
    _windowStartMicros = 0;
    if (host == null) return;
    _byTexture.remove(host.texture);
    final (view, registration) = (host.view, host.registration);
    if (registration == null) return;
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
    if (error != null) return widget.errorBuilder?.call(context, error, _retry) ?? ErrorWidget(error);
    _built = true;
    // A release build doesn't check the scale: one that isn't a number
    // above 0 draws every pixel, and one past 4 draws 4.
    final scale = widget.renderScale;
    final ratio = MediaQuery.devicePixelRatioOf(context) * (scale > 0 && scale.isFinite ? math.min(scale, 4) : 1);
    final takesInput = _tookInput = _takesInput;
    final view = LayoutBuilder(
      builder: (context, constraints) {
        if (!constraints.hasBoundedWidth || !constraints.hasBoundedHeight) {
          throw FlutterError(
            'FynkelView needs a bounded size, but got $constraints.\n'
            'Put it in a SizedBox, an Expanded or anything else that gives it a size.',
          );
        }
        final size = constraints.biggest;
        if (takesInput && !world.isDisposed) {
          _input.resize(size);
          world.input.camera = widget.camera;
        }
        _logical = size;
        _ratio = ratio;
        _sizeFromLayout();
        final host = _host, texture = host?.texture;
        if (texture == null) return const SizedBox.expand();
        // On screen: the surface is behind Flutter's view, so the view
        // clears whatever was painted under it, and the game shows through.
        if (host!.onScreen) return const SizedBox.expand(child: CustomPaint(painter: _Window()));
        return Texture(textureId: texture);
      },
    );
    // Without input it keeps the taps on it from what's under it.
    final game = !takesInput
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
              child: WorldWidgetLayer(
                world: world,
                frames: _ticks,
                camera: widget.camera,
                framesBehind: () => _host?.framesBehind ?? 0,
              ),
            ),
          ),
        if (_gameErrors.isNotEmpty)
          ErrorPanel(errors: List.of(_gameErrors), onDismiss: () => setState(_gameErrors.clear)),
      ],
    );
  }
}

/// How a [FynkelView]'s engine [view] reaches the screen, one kind for
/// each platform's way, each keeping what it alone needs.
sealed class _Host {
  _Host(this.view);

  final WorldView view;

  /// Its registration with the plugin, on its way or done.
  Future<int?>? registration;

  /// The id the plugin gave it, once registered.
  int? texture;

  /// Whether it asks to be drawn straight to the screen, behind Flutter's
  /// view ([FynkelView.directToScreen]), and so is registered with its place.
  bool get asksOnScreen;

  /// It draws straight to the screen, not to a texture: the plugin says so
  /// as it registers.
  bool onScreen = false;

  /// On screen: where it was last put, in physical pixels from the top
  /// left of Flutter's view, and whether over the other views.
  Rect? sentPlace;
  bool sentOver = false;

  /// What the plugin is sent to register it, at [place] when on screen.
  Map<String, Object> arguments(Rect? place);

  /// How many frames after Flutter's picture of an update the engine's
  /// picture of it is on screen, so world widgets are placed as that many
  /// updates ago (lab: "UI in the world", its sync check, filmed).
  int get framesBehind;
}

/// macOS, and iOS when not on screen or with no layer to have: a texture
/// the engine makes at the size the view asks for, which Flutter shows.
final class _AppleTexture extends _Host {
  _AppleTexture(super.view);

  @override
  bool get asksOnScreen => false;

  // Measured on a Mac at 120 Hz, 2026-10-31: a whole frame, every frame.
  @override
  int get framesBehind => 1;

  @override
  Map<String, Object> arguments(Rect? place) => {
    'view': view.address,
    'acquire': AppleView.acquireAddress,
    'setFrameReady': AppleView.setFrameReadyAddress,
    'free': AppleView.freeAddress,
  };
}

/// The plugin gives the view where it draws, and with it the size, which
/// is sent it as it changes.
sealed class _Surfaced extends _Host {
  _Surfaced(super.view) : sentWidth = view.width, sentHeight = view.height;

  /// The size the plugin was last given.
  int sentWidth, sentHeight;
}

/// iOS on screen: a layer of its own under Flutter's view, which the
/// plugin makes.
final class _AppleLayer extends _Surfaced {
  _AppleLayer(super.view);

  @override
  bool get asksOnScreen => true;

  // Measured on an iPhone 16 Pro Max at 120 Hz, 2026-10-31: the layer
  // shows the engine's picture with Flutter's; placing a frame back left
  // the widgets a whole frame behind it.
  @override
  int get framesBehind => 0;

  @override
  Map<String, Object> arguments(Rect? place) => {
    'view': view.address,
    'width': view.width,
    'height': view.height,
    'layer': true,
    'attach': AppleLayerView.attachAddress,
    'detach': AppleLayerView.detachAddress,
    'free': AppleView.freeAddress,
    'onScreen': true,
    'left': place!.left.round(),
    'top': place.top.round(),
    'frameWidth': place.width.round(),
    'frameHeight': place.height.round(),
  };
}

/// Android: a surface the plugin makes, shown as a texture, or on screen
/// behind Flutter's view when [asksOnScreen].
final class _AndroidSurface extends _Surfaced {
  _AndroidSurface(super.view, {required this.asksOnScreen});

  @override
  final bool asksOnScreen;

  // Measured on a Note10+ at 60 Hz, 2026-10-31: a whole frame.
  @override
  int get framesBehind => 1;

  @override
  Map<String, Object> arguments(Rect? place) => {
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

// The device's quality level, for a world without its own: looked up
// once, the first time a view is laid out.
Quality? _deviceQuality;

/// A world's frames, shared by the views that show it: the first of them
/// to tick in a display frame updates the world, the rest draw what that
/// made. The seconds between updates start from 0 when its first view
/// comes, as they did for a view alone. Which of the views takes the
/// input and shows the errors, and whether views made the world.
class _WorldClock {
  static final _clocks = Expando<_WorldClock>('FynkelView clock');

  static _WorldClock of(World world) => _clocks[world] ??= _WorldClock();

  // The display frame the world was last updated in; null before the
  // first, and again when its last view goes.
  Duration? _updated;

  /// The views showing the world, in the order they came.
  final views = <FynkelViewState>[];

  /// Whether a view made the world for its root: it goes when the last
  /// view showing it goes.
  bool made = false;

  /// Whether a debug build said that two views take input, whether a hot
  /// reload is reading the world's files again, and whether a memory
  /// warning is being told.
  bool toldInput = false, reloading = false, warned = false;

  void leave(FynkelViewState view) {
    views.remove(view);
    if (views.isEmpty) _updated = _asked = null;
  }

  /// The view that takes the input: the first in the tree with
  /// [FynkelView.input] on.
  FynkelViewState? get inputView {
    for (final view in views) {
      if (view._active && view.widget.input) return view;
    }
    return null;
  }

  /// The view that shows the errors: the one taking input, if it shows
  /// them, or else the first in the tree that does.
  FynkelViewState? get errorView {
    final input = inputView;
    if (input != null && input.widget.showErrors) return input;
    for (final view in views) {
      if (view._active && view.widget.showErrors) return view;
    }
    return null;
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
