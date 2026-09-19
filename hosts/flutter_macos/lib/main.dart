// Fynkel Flutter macOS host (blueprint §5.2, ADR-0007/0008, ADR-0002 Path A).
//
// The host owns the loop: a Ticker runs once per Flutter frame on the main
// isolate and (1) hands the frame dt to the world, (2) runs Dart gameplay
// and `step()` for every planned fixed tick, (3) asks the platform viewport
// to render the world's extract (interpolated by alpha) into the IOSurface
// ring. Swift hands the newest GPU-complete slot to Flutter's Texture
// widget; Flutter widgets are composited on top in the same frame.
//
// Harness kept from M0: resize (window / DPI), lifecycle (hidden, paused,
// resumed), dispose/recreate, HUD overlay with pointer/keyboard focus.

import 'dart:async';
import 'dart:ffi';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart';

import 'demo_scene.dart';
import 'viewport_ffi.dart';

const _bufferCount = 4;

/// `viewport` (default) or `none` (no viewport: raster control).
/// `FYNKEL_HOST_MODE` env var at runtime, else the dart-define.
final String hostMode = () {
  final env = Platform.environment['FYNKEL_HOST_MODE'];
  if (env != null && env.isNotEmpty) return env;
  return const String.fromEnvironment('FYNKEL_HOST_MODE', defaultValue: 'viewport');
}();

void main() {
  runApp(const HostApp());
}

class HostApp extends StatelessWidget {
  const HostApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Fynkel — Flutter host',
      theme: ThemeData.dark(useMaterial3: true),
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
    debugPrint('[host] $_timingsText');
  }

  Future<void> _toggleViewport() async {
    if (_showViewport) {
      // Ordered teardown *before* the widget goes away; see ViewportPage.teardown.
      await _viewportKey.currentState?.teardown();
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
  static const _channel = MethodChannel('fynkel/viewport');

  // Engine: one dylib serves the engine ABI (package:fynkel) and the viewport ABI.
  World? _world;
  DemoScene? _scene;
  ViewportNative? _viewportNative;
  Pointer<Void> _viewport = nullptr;
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
  static const _syncMode = int.fromEnvironment('FYNKEL_SYNC', defaultValue: 2);
  static const _syncModeNames = ['async-poll', 'blocking', 'metal-completion'];
  static const _acquireWaitUs = int.fromEnvironment('FYNKEL_ACQUIRE_WAIT_US', defaultValue: 3000);
  static const _satellites = int.fromEnvironment('FYNKEL_SATELLITES', defaultValue: 24);

  // Frame timing on the Dart side.
  Duration? _lastTick;
  final List<double> _intervalsMs = [];
  final List<double> _simMs = [];
  final List<double> _renderCallMs = [];
  int _ticksThisWindow = 0;
  int _missedDeadlines = 0;
  int _droppedThisWindow = 0;
  int _skippedWhileResizing = 0;
  Timer? _statsTimer;
  String _statsText = 'starting…';
  int _clicks = 0;
  final List<String> _events = [];
  final _clock = Stopwatch()..start();

  // Input harness: what the engine would receive.
  final _viewportFocus = FocusNode(debugLabel: 'viewport');
  final _hudTextFocus = FocusNode(debugLabel: 'hud-text');
  final Set<String> _keysDown = {};
  String _pointer = '-';
  int _pointerEventsThisWindow = 0;
  int _viewportClicks = 0;
  int _keyEventsToViewport = 0;

  void _log(String msg) {
    final line = '${(_clock.elapsedMilliseconds / 1000).toStringAsFixed(2)}s $msg';
    debugPrint('[host] $line');
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
    // (keep rendering). Muting the ticker pauses the host loop — and with it
    // the simulation clock (§5.2: no unbounded backlog while paused).
    final mute = state == AppLifecycleState.hidden || state == AppLifecycleState.paused;
    _ticker?.muted = mute;
    if (!mute) _lastTick = null; // do not feed the pause as one giant dt
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
      final world = _world ??= World.create();
      // Release-mode hosts log diagnostics; debug throws (FynkelError).
      world.onDiagnostics = (d) => _log('diagnostics: ${d.join("; ")}');
      _scene ??= DemoScene(satellites: _satellites);
      final native = _viewportNative ??= ViewportNative();
      final vp = native.create(width: width, height: height, bufferCount: _bufferCount, syncMode: _syncMode);
      if (vp == nullptr) throw StateError('fynkel_macos_viewport_create returned null (see native log)');
      _viewport = vp;
      native.setAcquireWaitUs(vp, _acquireWaitUs);
      native.setCamera(vp, eye: [0, 2.5, 7.5], target: [0, 0, 0]);
      final id = await _channel.invokeMethod<int>('registerTexture', {
        'ctx': vp.address,
        'acquireFn': native.acquireFnAddress,
        'setCallbackFn': native.setCallbackFnAddress,
        'iosurfaces': native.iosurfaceAddresses(vp),
      });
      if (!mounted || _tornDown) return;
      setState(() => _textureId = id);
      _ticker = createTicker(_onTick)..start();
      _log('created ${width}x$height texture=$id world tick=${world.tick}');
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
        if (!_viewportNative!.resize(_viewport, w, h)) {
          _log('resize FAILED ${w}x$h');
          continue;
        }
        await _channel.invokeMethod('rebindTexture', {
          'iosurfaces': _viewportNative!.iosurfaceAddresses(_viewport),
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
  /// into Rust afterwards); 4. free the viewport; 5. dispose the world.
  Future<void> teardown() async {
    if (_tornDown) return;
    _tornDown = true;
    _ticker?.dispose();
    _ticker = null;
    _statsTimer?.cancel();
    _viewportFocus.dispose();
    _hudTextFocus.dispose();
    if (_viewport != nullptr) {
      final vp = _viewport;
      _viewport = nullptr;
      _viewportNative!.clearFrameReadyCallback(vp);
      await _channel.invokeMethod('unregisterTexture');
      _viewportNative!.destroy(vp);
      _log('viewport destroyed');
    }
    final world = _world;
    _world = null;
    if (world != null) {
      final s = world.stats();
      world.dispose();
      _log('world disposed after tick ${s.tick} (${s.liveEntities} entities)');
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

  /// One host frame (ADR-0008): simulate, then render the extract.
  void _onTick(Duration elapsed) {
    final world = _world;
    if (_viewport == nullptr || world == null) return;
    var dt = 0.0;
    if (_lastTick != null) {
      dt = (elapsed - _lastTick!).inMicroseconds / 1e6;
      _intervalsMs.add(dt * 1000);
    }
    _lastTick = elapsed;

    final sim = Stopwatch()..start();
    final plan = world.frame(dt, fixedUpdate: (tick) => _scene!.fixedUpdate(world, tick));
    sim.stop();
    _simMs.add(sim.elapsedMicroseconds / 1000);
    _ticksThisWindow += plan.steps;
    if (plan.missedDeadline) _missedDeadlines++;

    if (_resizing) {
      // Between Rust resize and Swift rebind the presenter has no buffers yet;
      // the simulation still advanced, so no time is lost.
      _skippedWhileResizing++;
      return;
    }
    final sw = Stopwatch()..start();
    final rendered = _viewportNative!.render(_viewport, world.native.handleAddress);
    sw.stop();
    _renderCallMs.add(sw.elapsedMicroseconds / 1000);
    if (rendered == -1) _droppedThisWindow++;
    if (rendered == renderNoWorld) _log('render: no world (poisoned?)');
  }

  Future<void> _reportStats() async {
    final world = _world;
    if (_viewport == nullptr || _viewportNative == null || world == null) return;
    final s = _viewportNative!.takeStats(_viewport);
    final b = world.stats();
    final swift = await _channel.invokeMapMethod<String, dynamic>('stats');
    final intervals = List<double>.from(_intervalsMs)..sort();
    final sim = List<double>.from(_simMs)..sort();
    final calls = List<double>.from(_renderCallMs)..sort();
    _intervalsMs.clear();
    _simMs.clear();
    _renderCallMs.clear();
    final dropped = _droppedThisWindow;
    final skipped = _skippedWhileResizing;
    final ticks = _ticksThisWindow;
    final missed = _missedDeadlines;
    _droppedThisWindow = 0;
    _skippedWhileResizing = 0;
    _ticksThisWindow = 0;
    _missedDeadlines = 0;
    if (!mounted) return;
    setState(() {
      _statsText = [
        'viewport ${_width}x$_height  ring=$_bufferCount  mode=${_syncModeNames[_syncMode]} acquireWait=${_acquireWaitUs}us  ticker.muted=${_ticker?.muted}',
        'world tick=${b.tick} entities=${b.liveEntities} instances=${s.instances}  fixed ticks/s=$ticks missedDeadlines=$missed  bridge calls=${b.ffiCalls} bytes in/out=${b.bytesIn}/${b.bytesOut}',
        'dart tick interval ms  p50=${_p(intervals, .5)} p95=${_p(intervals, .95)} p99=${_p(intervals, .99)}  (n=${intervals.length}, skipped while resizing=$skipped)',
        'dart simulate ms       p50=${_p(sim, .5)} p95=${_p(sim, .95)} p99=${_p(sim, .99)}  (gameplay + submit + step + snapshot)',
        'ffi render() call ms   p50=${_p(calls, .5)} p95=${_p(calls, .95)} p99=${_p(calls, .99)}  (extract + encode + submit)',
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
    // Also print for scripted captures (benchmarks/).
    debugPrint('[host] ${_statsText.split('\n').take(12).join(' | ')}');
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
            // Flutter overlay: the "HUD over the 3D world".
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
