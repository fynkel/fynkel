// M0 Flutter host: Path A viewport experiment (blueprint §8.2, ADR-0002).
//
// The host owns the loop (ADR-0007): a Ticker drives `render()` once per
// Flutter frame. Rust renders into an IOSurface ring; Swift hands the newest
// GPU-complete buffer to Flutter's Texture widget; Flutter widgets are
// composited on top. Frame-timing stats are shown in the overlay.
//
// M0-06 harness: resize (window / DPI), lifecycle (hidden, paused, resumed)
// and dispose/recreate are exercised from the parent page.

import 'dart:async';
import 'dart:ffi';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

import 'm0_ffi.dart';

const _bufferCount = 4;

/// Host mode: `a` (default) = Path A Texture viewport, `c` = Path C
/// (engine-owned CAMetalLayer under a transparent Flutter view; the Swift
/// side reads the same variable), `none` = no viewport (control).
/// `FYNKEL_M0_MODE` env var at runtime, else the dart-define.
final String hostMode = () {
  final env = Platform.environment['FYNKEL_M0_MODE'];
  if (env != null && env.isNotEmpty) return env;
  if (const bool.fromEnvironment('FYNKEL_M0_NO_VIEWPORT')) return 'none';
  return const String.fromEnvironment('FYNKEL_M0_MODE', defaultValue: 'a');
}();

void main() {
  runApp(const M0App());
}

class M0App extends StatelessWidget {
  const M0App({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Fynkel — Flutter host (M0 Path ${hostMode.toUpperCase()})',
      theme: ThemeData.dark(useMaterial3: true).copyWith(
        // Path C: Flutter is a transparent layer over the engine's surface.
        scaffoldBackgroundColor: hostMode == 'c' ? Colors.transparent : null,
      ),
      home: const HostPage(),
    );
  }
}

/// Owns the viewport widget so it can be destroyed and recreated (dispose test)
/// and padded to change its size without resizing the window.
class HostPage extends StatefulWidget {
  const HostPage({super.key});

  @override
  State<HostPage> createState() => _HostPageState();
}

class _HostPageState extends State<HostPage> {
  final _viewportKey = GlobalKey<_ViewportPageState>();
  final _pathCKey = GlobalKey<_PathCPageState>();
  bool _showViewport = hostMode != 'none';
  double _inset = 0;
  int _generation = 0;

  // Flutter's own frame timings (UI build + raster), to measure what the
  // Texture layer costs Impeller: compare with the viewport destroyed.
  final List<double> _buildMs = [];
  final List<double> _rasterMs = [];
  Timer? _timingsTimer;
  String _timingsText = '';

  @override
  void initState() {
    super.initState();
    SchedulerBinding.instance.addTimingsCallback(_onTimings);
    _timingsTimer = Timer.periodic(const Duration(seconds: 1), (_) => _reportTimings());
  }

  @override
  void dispose() {
    SchedulerBinding.instance.removeTimingsCallback(_onTimings);
    _timingsTimer?.cancel();
    super.dispose();
  }

  void _onTimings(List<FrameTiming> timings) {
    for (final t in timings) {
      _buildMs.add(t.buildDuration.inMicroseconds / 1000);
      _rasterMs.add(t.rasterDuration.inMicroseconds / 1000);
    }
  }

  void _reportTimings() {
    final b = List<double>.from(_buildMs)..sort();
    final r = List<double>.from(_rasterMs)..sort();
    _buildMs.clear();
    _rasterMs.clear();
    if (!mounted) return;
    setState(() {
      _timingsText =
          'flutter frames n=${r.length} viewport=${_showViewport ? hostMode : "off"}  ui build ms p50=${_ViewportPageState._p(b, .5)} p95=${_ViewportPageState._p(b, .95)}  raster ms p50=${_ViewportPageState._p(r, .5)} p95=${_ViewportPageState._p(r, .95)} p99=${_ViewportPageState._p(r, .99)}';
    });
    debugPrint('[m0] $_timingsText');
  }

  Future<void> _toggleViewport() async {
    if (_showViewport) {
      // Ordered teardown *before* the widget goes away; see ViewportPage.teardown.
      await _viewportKey.currentState?.teardown();
      await _pathCKey.currentState?.teardown();
      setState(() => _showViewport = false);
    } else {
      setState(() {
        _showViewport = true;
        _generation++;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Column(
        children: [
          Expanded(
            child: Padding(
              padding: EdgeInsets.all(_inset),
              child: !_showViewport
                  ? const Center(child: Text('viewport destroyed'))
                  : hostMode == 'c'
                      ? PathCPage(key: _pathCKey, generation: _generation)
                      : ViewportPage(key: _viewportKey, generation: _generation),
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(8),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.center,
              spacing: 8,
              children: [
                // Keeps Flutter animating at display rate even without the viewport,
                // so raster timings are comparable.
                const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                Text(_timingsText, style: const TextStyle(fontFamily: 'Menlo', fontSize: 11)),
                FilledButton.tonal(
                  onPressed: _toggleViewport,
                  child: Text(_showViewport ? 'destroy viewport' : 'recreate viewport'),
                ),
                FilledButton.tonal(
                  onPressed: () => setState(() => _inset = _inset == 0 ? 120 : 0),
                  child: Text(_inset == 0 ? 'shrink viewport' : 'grow viewport'),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class ViewportPage extends StatefulWidget {
  const ViewportPage({super.key, required this.generation});

  final int generation;

  @override
  State<ViewportPage> createState() => _ViewportPageState();
}

class _ViewportPageState extends State<ViewportPage>
    with SingleTickerProviderStateMixin, WidgetsBindingObserver {
  static const _channel = MethodChannel('fynkel/m0');

  M0Native? _native;
  Pointer<Void> _ctx = nullptr;
  int? _textureId;
  Ticker? _ticker;
  String? _error;

  // Viewport size in physical pixels; the ring is (re)created at this size.
  int _width = 0;
  int _height = 0;
  (int, int)? _pendingSize;
  bool _resizing = false;
  bool _creating = false;
  bool _tornDown = false;
  static const _syncMode = int.fromEnvironment('FYNKEL_M0_SYNC', defaultValue: 2);
  static const _syncModeNames = ['async-poll', 'blocking', 'metal-completion'];
  static const _acquireWaitUs = int.fromEnvironment('FYNKEL_M0_ACQUIRE_WAIT_US', defaultValue: 3000);

  // Frame timing on the Dart side.
  Duration? _lastTick;
  final List<double> _intervalsMs = [];
  final List<double> _renderCallMs = [];
  int _droppedThisWindow = 0;
  int _skippedWhileResizing = 0;
  Timer? _statsTimer;
  String _statsText = 'starting…';
  int _clicks = 0;
  final List<String> _events = [];
  final _clock = Stopwatch()..start();

  // Input harness (M0 backlog #9): what the engine would receive.
  final _viewportFocus = FocusNode(debugLabel: 'viewport');
  final _hudTextFocus = FocusNode(debugLabel: 'hud-text');
  final Set<String> _keysDown = {};
  String _pointer = '-';
  int _pointerEventsThisWindow = 0;
  int _viewportClicks = 0;
  int _keyEventsToViewport = 0;

  void _log(String msg) {
    final line = '${(_clock.elapsedMilliseconds / 1000).toStringAsFixed(2)}s $msg';
    debugPrint('[m0] $line');
    _events.add(line);
    if (_events.length > 6) _events.removeAt(0);
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _statsTimer = Timer.periodic(const Duration(seconds: 1), (_) => _reportStats());
    _viewportFocus.addListener(() => _log('viewport focus=${_viewportFocus.hasFocus}'));
    _hudTextFocus.addListener(() => _log('hud text focus=${_hudTextFocus.hasFocus}'));
    _log('init generation=${widget.generation}');
  }

  // ---- input -----------------------------------------------------------------

  KeyEventResult _onViewportKey(FocusNode node, KeyEvent e) {
    final k = e.logicalKey.keyLabel.isEmpty ? e.logicalKey.debugName ?? '?' : e.logicalKey.keyLabel;
    if (e is KeyDownEvent) {
      _keysDown.add(k);
      _keyEventsToViewport++;
      _log('key down $k (viewport)');
    } else if (e is KeyUpEvent) {
      _keysDown.remove(k);
    }
    // The engine consumes game keys; everything else bubbles (menus, ⌘Q).
    return e.logicalKey.keyLabel.length == 1 ? KeyEventResult.handled : KeyEventResult.ignored;
  }

  void _onPointer(PointerEvent e) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    // Local logical → viewport physical pixels: the engine's coordinate space.
    final px = (e.localPosition.dx * dpr).round();
    final py = (e.localPosition.dy * dpr).round();
    _pointer = '$px,$py (${e.runtimeType})';
    _pointerEventsThisWindow++;
    if (e is PointerDownEvent) {
      _viewportClicks++;
      // Deferred: a focused TextField's tap-outside handler unfocuses *in the
      // same dispatch* and would win over a synchronous requestFocus here.
      Future.microtask(_viewportFocus.requestFocus);
      _log('pointer down on viewport at $px,$py → focus viewport');
    }
  }

  // ---- lifecycle -----------------------------------------------------------

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // `hidden` = window minimised/occluded on macOS; `inactive` = lost focus
    // (keep rendering). Muting the ticker stops render() without stopping the
    // Ticker's frame subscription bookkeeping.
    final mute = state == AppLifecycleState.hidden || state == AppLifecycleState.paused;
    _ticker?.muted = mute;
    _log('lifecycle $state ticker.muted=$mute');
  }

  @override
  void didChangeMetrics() {
    // Fires on window resize and display/DPI change; the LayoutBuilder below
    // sees the new constraints on the next frame.
    final v = View.of(context);
    _log('metrics dpr=${v.devicePixelRatio} physical=${v.physicalSize.width.round()}x${v.physicalSize.height.round()}');
  }

  // ---- create / resize / teardown ----------------------------------------

  void _requestSize(int width, int height) {
    if (_tornDown || _error != null || width == 0 || height == 0) return;
    if (_textureId == null) {
      if (!_creating) _create(width, height);
      return;
    }
    if (width == _width && height == _height) return;
    _pendingSize = (width, height);
    if (!_resizing) _drainResizes();
  }

  Future<void> _create(int width, int height) async {
    _creating = true;
    _width = width;
    _height = height;
    try {
      final native = _native ??= M0Native(M0Native.defaultLibraryPath());
      final ctx = native.create(width: width, height: height, bufferCount: _bufferCount, syncMode: _syncMode);
      if (ctx == nullptr) throw StateError('fynkel_m0_create returned null (see native log)');
      _ctx = ctx;
      native.setAcquireWaitUs(ctx, _acquireWaitUs);
      final id = await _channel.invokeMethod<int>('registerTexture', {
        'ctx': ctx.address,
        'acquireFn': native.acquireFnAddress,
        'setCallbackFn': native.setCallbackFnAddress,
        'iosurfaces': native.iosurfaceAddresses(ctx, _bufferCount),
      });
      if (!mounted || _tornDown) return;
      setState(() => _textureId = id);
      _ticker = createTicker(_onTick)..start();
      _log('created ${width}x$height texture=$id');
      // A game host wants the world to own keyboard focus until a HUD widget takes it.
      _viewportFocus.requestFocus();
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      _creating = false;
    }
  }

  /// Applies the latest requested size, coalescing bursts from live resize.
  /// Sequence per resize: stop rendering → Rust rebuilds the ring (GPU idle)
  /// → Swift re-wraps the new IOSurfaces → resume rendering.
  Future<void> _drainResizes() async {
    _resizing = true;
    try {
      while (_pendingSize != null && !_tornDown) {
        final (w, h) = _pendingSize!;
        _pendingSize = null;
        if (w == _width && h == _height) continue;
        final sw = Stopwatch()..start();
        if (!_native!.resize(_ctx, w, h)) {
          _log('resize FAILED ${w}x$h');
          continue;
        }
        await _channel.invokeMethod('rebindTexture', {
          'iosurfaces': _native!.iosurfaceAddresses(_ctx, _bufferCount),
        });
        _width = w;
        _height = h;
        _log('resized ${w}x$h in ${sw.elapsedMicroseconds / 1000} ms');
      }
    } finally {
      _resizing = false;
    }
  }

  /// Ordered teardown. Safe to call more than once. Order matters:
  /// 1. stop the host loop; 2. clear the Rust→Swift callback (synchronous);
  /// 3. Swift detaches + unregisters the texture (raster thread will not call
  /// into Rust afterwards); 4. free the Rust context.
  Future<void> teardown() async {
    if (_tornDown) return;
    _tornDown = true;
    _ticker?.dispose();
    _ticker = null;
    _statsTimer?.cancel();
    _viewportFocus.dispose();
    _hudTextFocus.dispose();
    if (_ctx != nullptr) {
      final ctx = _ctx;
      _ctx = nullptr;
      _native!.clearFrameReadyCallback(ctx);
      await _channel.invokeMethod('unregisterTexture');
      _native!.destroy(ctx);
      _log('destroyed');
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    // If the parent did not call teardown() first (e.g. navigation), run it
    // now; the async tail (unregister → destroy) completes after dispose.
    teardown();
    super.dispose();
  }

  // ---- loop ----------------------------------------------------------------

  void _onTick(Duration elapsed) {
    if (_ctx == nullptr) return;
    if (_lastTick != null) {
      _intervalsMs.add((elapsed - _lastTick!).inMicroseconds / 1000);
    }
    _lastTick = elapsed;
    if (_resizing) {
      // Between Rust resize and Swift rebind the presenter has no buffers yet.
      _skippedWhileResizing++;
      return;
    }
    final sw = Stopwatch()..start();
    final rendered = _native!.render(_ctx, elapsed.inMicroseconds / 1e6);
    sw.stop();
    _renderCallMs.add(sw.elapsedMicroseconds / 1000);
    if (rendered < 0) _droppedThisWindow++;
  }

  Future<void> _reportStats() async {
    if (_ctx == nullptr || _native == null) return;
    final s = _native!.takeStats(_ctx);
    final swift = await _channel.invokeMapMethod<String, dynamic>('stats');
    final intervals = List<double>.from(_intervalsMs)..sort();
    final calls = List<double>.from(_renderCallMs)..sort();
    _intervalsMs.clear();
    _renderCallMs.clear();
    final dropped = _droppedThisWindow;
    final skipped = _skippedWhileResizing;
    _droppedThisWindow = 0;
    _skippedWhileResizing = 0;
    if (!mounted) return;
    setState(() {
      _statsText = [
        'viewport ${_width}x$_height  ring=$_bufferCount  mode=${_syncModeNames[_syncMode]} acquireWait=${_acquireWaitUs}us  ticker.muted=${_ticker?.muted}',
        'dart tick interval ms  p50=${_p(intervals, .5)} p95=${_p(intervals, .95)} p99=${_p(intervals, .99)}  (n=${intervals.length}, skipped while resizing=$skipped)',
        'ffi render() call ms   p50=${_p(calls, .5)} p95=${_p(calls, .95)} p99=${_p(calls, .99)}',
        'rust cpu encode+submit p50=${s.cpuP50Ms.toStringAsFixed(3)} p95=${s.cpuP95Ms.toStringAsFixed(3)} p99=${s.cpuP99Ms.toStringAsFixed(3)}',
        'submit→gpu complete    p50=${s.gpuP50Ms.toStringAsFixed(2)} p95=${s.gpuP95Ms.toStringAsFixed(2)} p99=${s.gpuP99Ms.toStringAsFixed(2)}',
        'rendered=${s.framesRendered} dropped(ring full)=${s.framesDropped}/$dropped presented=${s.framesPresented} stalePresent=${s.presentsWithoutNewFrame}',
        'present lag (frames behind newest submit) p50=${s.presentLagP50} p95=${s.presentLagP95} max=${s.presentLagMax}  raster acquire wait ms p50=${s.acquireWaitP50Ms.toStringAsFixed(2)} p95=${s.acquireWaitP95Ms.toStringAsFixed(2)} max=${s.acquireWaitMaxMs.toStringAsFixed(2)}',
        'resizes=${s.resizes} rust resize ms p50=${s.resizeP50Ms.toStringAsFixed(2)} max=${s.resizeMaxMs.toStringAsFixed(2)}',
        if (swift != null)
          'swift copyPixelBuffer=${swift["copyCalls"]} newFrame=${swift["copyWithNewFrame"]} noFrame=${swift["copyNoFrame"]} readySignals=${swift["frameReadySignals"]} rebinds=${swift["rebinds"]}',
        'input: pointer=$_pointer events/s=$_pointerEventsThisWindow clicks=$_viewportClicks  keysDown=${_keysDown.toList()} keyEvents=$_keyEventsToViewport  focus=${FocusManager.instance.primaryFocus?.debugLabel ?? "?"}',
        '--- events',
        ..._events,
      ].join('\n');
    });
    _pointerEventsThisWindow = 0;
    // Also print for scripted captures (benchmarks/m0).
    debugPrint('[m0] ${_statsText.split('\n').take(10).join(' | ')}');
  }

  static String _p(List<double> sorted, double q) {
    if (sorted.isEmpty) return '-';
    final i = ((sorted.length - 1) * q).round();
    return sorted[i].toStringAsFixed(2);
  }

  @override
  Widget build(BuildContext context) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return LayoutBuilder(
      builder: (context, constraints) {
        final w = (constraints.maxWidth * dpr).round();
        final h = (constraints.maxHeight * dpr).round();
        WidgetsBinding.instance.addPostFrameCallback((_) => _requestSize(w, h));
        return Stack(
          fit: StackFit.expand,
          children: [
            if (_textureId != null)
              // The viewport is an ordinary Flutter layer: pointer events and
              // keyboard focus are handled by normal Flutter widgets around it.
              Focus(
                focusNode: _viewportFocus,
                onKeyEvent: _onViewportKey,
                child: Listener(
                  onPointerDown: _onPointer,
                  onPointerMove: _onPointer,
                  onPointerHover: _onPointer,
                  onPointerUp: _onPointer,
                  behavior: HitTestBehavior.opaque,
                  child: Texture(textureId: _textureId!, filterQuality: FilterQuality.none),
                ),
              )
            else
              Center(child: Text(_error ?? 'creating viewport…', textAlign: TextAlign.center)),
            // Flutter overlay: this is the "HUD over the 3D world" the gate requires.
            Positioned(
              left: 16,
              top: 16,
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: Colors.black.withValues(alpha: 0.55),
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Text(
                    _statsText,
                    style: const TextStyle(fontFamily: 'Menlo', fontSize: 12, height: 1.4),
                  ),
                ),
              ),
            ),
            Positioned(
              right: 16,
              bottom: 16,
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  SizedBox(
                    width: 180,
                    child: TextField(
                      focusNode: _hudTextFocus,
                      decoration: const InputDecoration(isDense: true, border: OutlineInputBorder(), hintText: 'HUD text field'),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Text('clicks: $_clicks   ', style: const TextStyle(fontFamily: 'Menlo')),
                  FilledButton.icon(
                    onPressed: () {
                      setState(() => _clicks++);
                      _log('HUD button click #$_clicks');
                    },
                    icon: const Icon(Icons.touch_app),
                    label: const Text('Flutter button over 3D'),
                  ),
                ],
              ),
            ),
            const Positioned(
              left: 0,
              right: 0,
              bottom: 16,
              child: Center(child: CircularProgressIndicator()),
            ),
          ],
        );
      },
    );
  }
}

/// Path C page (backlog #7): no Texture widget. The engine's CAMetalLayer is
/// under the whole window; this page is the transparent HUD on top and the
/// owner of the Rust surface context. Its size drives the engine's drawable
/// size only indirectly (the Swift host reports the engine view's frame).
class PathCPage extends StatefulWidget {
  const PathCPage({super.key, required this.generation});
  final int generation;

  @override
  State<PathCPage> createState() => _PathCPageState();
}

class _PathCPageState extends State<PathCPage> with WidgetsBindingObserver {
  static const _channel = MethodChannel('fynkel/m0/pathc');

  M0SurfaceNative? _native;
  Pointer<Void> _ctx = nullptr;
  String? _error;
  int _width = 0, _height = 0;
  double _scale = 0;
  bool _tornDown = false;
  bool _paused = false;

  Timer? _statsTimer;
  String _statsText = 'starting…';
  int _clicks = 0;
  final List<String> _events = [];
  final _clock = Stopwatch()..start();

  final _viewportFocus = FocusNode(debugLabel: 'viewport');
  final _hudTextFocus = FocusNode(debugLabel: 'hud-text');
  final Set<String> _keysDown = {};
  String _pointer = '-';
  int _pointerEventsThisWindow = 0;
  int _viewportClicks = 0;
  int _keyEventsToViewport = 0;

  void _log(String msg) {
    final line = '${(_clock.elapsedMilliseconds / 1000).toStringAsFixed(2)}s $msg';
    debugPrint('[m0] $line');
    _events.add(line);
    if (_events.length > 6) _events.removeAt(0);
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _channel.setMethodCallHandler(_onHostCall);
    _statsTimer = Timer.periodic(const Duration(seconds: 1), (_) => _reportStats());
    _viewportFocus.addListener(() => _log('viewport focus=${_viewportFocus.hasFocus}'));
    _hudTextFocus.addListener(() => _log('hud text focus=${_hudTextFocus.hasFocus}'));
    _log('init path C generation=${widget.generation}');
    _create();
  }

  Future<void> _create() async {
    try {
      _native ??= M0SurfaceNative(M0Native.defaultLibraryPath());
      final info = await _channel.invokeMapMethod<String, dynamic>('attach');
      if (info == null || _tornDown) return;
      _width = info['width'] as int;
      _height = info['height'] as int;
      _scale = (info['scale'] as num).toDouble();
      final ctx = _native!.create(layerAddress: info['layer'] as int, width: _width, height: _height);
      if (ctx == nullptr) throw StateError('fynkel_m0_surface_create failed');
      _ctx = ctx;
      _log('surface ${_width}x$_height @${_scale}x, engine render thread started');
      setState(() {});
      _viewportFocus.requestFocus();
    } catch (e) {
      setState(() => _error = '$e');
    }
  }

  Future<dynamic> _onHostCall(MethodCall call) async {
    switch (call.method) {
      case 'resized':
        _applySize((call.arguments as Map).cast<String, dynamic>(), 'host view resized');
      case 'visibility':
        _setPaused(!(call.arguments as bool), 'window ${call.arguments == true ? "deminiaturized" : "miniaturized"}');
    }
  }

  void _applySize(Map<String, dynamic> a, String why) {
    final w = a['width'] as int, h = a['height'] as int;
    final s = (a['scale'] as num).toDouble();
    if (_ctx != nullptr && (w != _width || h != _height || s != _scale)) {
      _width = w;
      _height = h;
      _scale = s;
      _native!.resize(_ctx, w, h);
      _log('$why → ${w}x$h @${s}x');
    }
  }

  /// Re-reads the engine view's size from the host; catches a frame change
  /// that raced the attach.
  Future<void> _syncSize(String why) async {
    if (_ctx == nullptr) return;
    final a = await _channel.invokeMapMethod<String, dynamic>('size');
    if (a != null && !_tornDown) _applySize(a, why);
  }

  void _setPaused(bool paused, String why) {
    if (_ctx == nullptr || paused == _paused) return;
    _paused = paused;
    _native!.setPaused(_ctx, paused);
    _log('${paused ? "paused" : "resumed"} engine loop: $why');
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _log('lifecycle $state');
    _setPaused(state == AppLifecycleState.hidden || state == AppLifecycleState.paused, '$state');
  }

  @override
  void didChangeMetrics() => _syncSize('metrics changed');

  KeyEventResult _onViewportKey(FocusNode node, KeyEvent e) {
    final k = e.logicalKey.keyLabel.isEmpty ? e.logicalKey.debugName ?? '?' : e.logicalKey.keyLabel;
    if (e is KeyDownEvent) {
      _keysDown.add(k);
      _keyEventsToViewport++;
      _log('key down $k (viewport)');
    } else if (e is KeyUpEvent) {
      _keysDown.remove(k);
    }
    return e.logicalKey.keyLabel.length == 1 ? KeyEventResult.handled : KeyEventResult.ignored;
  }

  void _onPointer(PointerEvent e) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    final px = (e.localPosition.dx * dpr).round();
    final py = (e.localPosition.dy * dpr).round();
    _pointer = '$px,$py (${e.runtimeType})';
    _pointerEventsThisWindow++;
    if (e is PointerDownEvent) {
      _viewportClicks++;
      Future.microtask(_viewportFocus.requestFocus);
      _log('pointer down on viewport at $px,$py → focus viewport');
    }
  }

  void _reportStats() {
    if (_ctx == nullptr) return;
    _syncSize('size poll');
    final s = _native!.takeStats(_ctx);
    if (!mounted) return;
    String t(List<double> v, [int d = 2]) =>
        'p50=${v[0].toStringAsFixed(d)} p95=${v[1].toStringAsFixed(d)} p99=${v[2].toStringAsFixed(d)}';
    setState(() {
      _statsText = [
        'path C surface ${_width}x$_height @${_scale}x  paused=$_paused  (engine thread, Fifo)',
        'engine present interval ms ${t(s.intervalMs)}  (n=${s.frames}, skipped=${s.skipped})',
        'engine acquire wait ms     ${t(s.acquireMs)}',
        'engine cpu encode+submit   ${t(s.cpuMs, 3)}',
        'resizes=${s.resizes} rust resize ms p50=${s.resizeP50Ms.toStringAsFixed(2)} max=${s.resizeMaxMs.toStringAsFixed(2)}',
        'input: pointer=$_pointer events/s=$_pointerEventsThisWindow clicks=$_viewportClicks  keysDown=${_keysDown.toList()} keyEvents=$_keyEventsToViewport  focus=${FocusManager.instance.primaryFocus?.debugLabel ?? "?"}',
        '--- events',
        ..._events,
      ].join('\n');
    });
    _pointerEventsThisWindow = 0;
    debugPrint('[m0] ${_statsText.split('\n').take(6).join(' | ')}');
  }

  /// Ordered teardown: stop stats → Swift stops reporting → join the engine
  /// thread and release the surface (the layer itself stays with the host).
  Future<void> teardown() async {
    if (_tornDown) return;
    _tornDown = true;
    _statsTimer?.cancel();
    _viewportFocus.dispose();
    _hudTextFocus.dispose();
    if (_ctx != nullptr) {
      final ctx = _ctx;
      _ctx = nullptr;
      await _channel.invokeMethod('detach');
      _native!.destroy(ctx);
      _log('destroyed');
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _channel.setMethodCallHandler(null);
    teardown();
    super.dispose();
  }

  Rect? _reportedRect;

  /// Tells the host where this widget is so the engine view follows Flutter
  /// layout (shrink/grow, host bar) instead of filling the window.
  void _reportRect() {
    if (_ctx == nullptr || !mounted) return;
    final box = context.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize) return;
    final rect = box.localToGlobal(Offset.zero) & box.size;
    if (rect == _reportedRect) return;
    _reportedRect = rect;
    _channel.invokeMethod('setFrame', {'x': rect.left, 'y': rect.top, 'w': rect.width, 'h': rect.height});
  }

  @override
  Widget build(BuildContext context) {
    WidgetsBinding.instance.addPostFrameCallback((_) => _reportRect());
    return Stack(
      fit: StackFit.expand,
      children: [
        if (_ctx != nullptr)
          // Transparent hit target: Flutter's view is on top of the engine
          // layer, so *all* input arrives here first and is forwarded.
          Focus(
            focusNode: _viewportFocus,
            onKeyEvent: _onViewportKey,
            child: Listener(
              onPointerDown: _onPointer,
              onPointerMove: _onPointer,
              onPointerHover: _onPointer,
              onPointerUp: _onPointer,
              behavior: HitTestBehavior.opaque,
              child: const SizedBox.expand(),
            ),
          )
        else
          Center(child: Text(_error ?? 'attaching engine surface…', textAlign: TextAlign.center)),
        Positioned(
          left: 16,
          top: 16,
          child: DecoratedBox(
            decoration: BoxDecoration(
              color: Colors.black.withValues(alpha: 0.55),
              borderRadius: BorderRadius.circular(8),
            ),
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Text(_statsText, style: const TextStyle(fontFamily: 'Menlo', fontSize: 12, height: 1.4)),
            ),
          ),
        ),
        Positioned(
          right: 16,
          bottom: 16,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              SizedBox(
                width: 180,
                child: TextField(
                  focusNode: _hudTextFocus,
                  decoration: const InputDecoration(isDense: true, border: OutlineInputBorder(), hintText: 'HUD text field'),
                ),
              ),
              const SizedBox(width: 12),
              Text('clicks: $_clicks   ', style: const TextStyle(fontFamily: 'Menlo')),
              FilledButton.icon(
                onPressed: () {
                  setState(() => _clicks++);
                  _log('HUD button click #$_clicks');
                },
                icon: const Icon(Icons.touch_app),
                label: const Text('Flutter button over 3D'),
              ),
            ],
          ),
        ),
        const Positioned(left: 0, right: 0, bottom: 16, child: Center(child: CircularProgressIndicator())),
      ],
    );
  }
}
