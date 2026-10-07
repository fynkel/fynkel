import 'dart:async';
import 'dart:io';

import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show ViewStats, World;
import 'package:fynkel/platform.dart';

/// The plugin's channel (darwin/…/FynkelFlutterPlugin.swift).
@visibleForTesting
const channel = MethodChannel('fynkel_flutter');

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

/// Shows [world] and runs its frame clock: each display frame it calls
/// [onFrame] with the seconds since the last one, then hands the world to
/// the engine to draw. It fills the space its parent gives it, which must
/// be bounded.
///
/// The frames are drawn at the widget's size in physical pixels times
/// [resolutionScale] and scaled to fit. Below 1 trades sharpness for
/// speed; the view never lowers it by itself.
///
/// macOS and iOS for now; Android comes in stage 1, step 7.
class FynkelView extends StatefulWidget {
  const FynkelView({super.key, required this.world, this.onFrame, this.resolutionScale = 1.0, this.onStats})
    : assert(resolutionScale > 0 && resolutionScale <= 4);

  final World world;

  /// Called once per display frame, before the world is drawn, with the
  /// seconds since the previous call (0 the first time). Game code goes
  /// here.
  final void Function(double dt)? onFrame;

  final double resolutionScale;

  /// Called about once a second with what the view did since the last call.
  final void Function(FynkelViewStats stats)? onStats;

  @override
  State<FynkelView> createState() => _FynkelViewState();
}

class _FynkelViewState extends State<FynkelView> with SingleTickerProviderStateMixin {
  late final Ticker _ticker = createTicker(_tick)..start();
  int? _lastMicros;

  /// The size the next frame is drawn at, from the last layout.
  int _width = 0, _height = 0;

  AppleView? _view;
  Future<int?>? _registration;
  int? _texture;
  Object? _error;

  // The current stats window.
  int _windowStartMicros = 0, _windowTicks = 0;
  ViewStats _windowStart = const ViewStats();

  @override
  void initState() {
    super.initState();
    _ticker; // Starts it.
  }

  @override
  void didUpdateWidget(FynkelView old) {
    super.didUpdateWidget(old);
    if (old.world != widget.world) _release();
  }

  @override
  void dispose() {
    _ticker.dispose();
    _release();
    super.dispose();
  }

  void _tick(Duration elapsed) {
    final micros = elapsed.inMicroseconds;
    final last = _lastMicros;
    _lastMicros = micros;
    widget.onFrame?.call(last == null ? 0 : (micros - last) / 1e6);
    if (_width == 0) return;

    final view = _view ?? _open();
    if (view == null) return;
    view.resize(_width, _height);
    if (_texture == null) return;
    view.frame();

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
  AppleView? _open() {
    final AppleView view;
    try {
      if (!Platform.isMacOS && !Platform.isIOS) {
        throw UnsupportedError('FynkelView runs on macOS and iOS for now (Android: stage 1, step 7).');
      }
      view = AppleView(widget.world, width: _width, height: _height);
    } catch (error, stack) {
      _fail(error, stack);
      return null;
    }
    _view = view;
    _registration = _register(view);
    return view;
  }

  Future<int?> _register(AppleView view) async {
    try {
      final texture = await channel.invokeMethod<int>('register', {
        'view': view.address,
        'acquire': AppleView.acquireAddress,
        'setFrameReady': AppleView.setFrameReadyAddress,
      });
      if (mounted && _view == view) setState(() => _texture = texture);
      return texture;
    } catch (error, stack) {
      if (mounted && _view == view) _fail(error, stack);
      return null;
    }
  }

  void _fail(Object error, StackTrace stack) {
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: error,
        stack: stack,
        library: 'fynkel_flutter',
        context: ErrorDescription('in FynkelView'),
      ),
    );
    _ticker.stop();
    setState(() => _error = error);
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
    final ratio = MediaQuery.devicePixelRatioOf(context) * widget.resolutionScale;
    return LayoutBuilder(
      builder: (context, constraints) {
        if (!constraints.hasBoundedWidth || !constraints.hasBoundedHeight) {
          throw FlutterError(
            'FynkelView needs a bounded size, but got $constraints.\n'
            'Put it in a SizedBox, an Expanded or anything else that gives it a size.',
          );
        }
        final size = constraints.biggest;
        _width = (size.width * ratio).round().clamp(1, 16384);
        _height = (size.height * ratio).round().clamp(1, 16384);
        final texture = _texture;
        return texture == null ? const SizedBox.expand() : Texture(textureId: texture);
      },
    );
  }
}
