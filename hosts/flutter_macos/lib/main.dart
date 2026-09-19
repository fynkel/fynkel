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

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

import 'm0_ffi.dart';

const _bufferCount = 4;

void main() {
  runApp(const M0App());
}

class M0App extends StatelessWidget {
  const M0App({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Fynkel — Flutter host (M0 Path A)',
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
  bool _showViewport = true;
  double _inset = 0;
  int _generation = 0;

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
              child: _showViewport
                  ? ViewportPage(key: _viewportKey, generation: _generation)
                  : const Center(child: Text('viewport destroyed')),
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(8),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.center,
              spacing: 8,
              children: [
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
  final bool _blocking = const bool.fromEnvironment("FYNKEL_M0_BLOCKING");

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
    _log('init generation=${widget.generation}');
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
      final ctx = native.create(width: width, height: height, bufferCount: _bufferCount, blocking: _blocking);
      if (ctx == nullptr) throw StateError('fynkel_m0_create returned null (see native log)');
      _ctx = ctx;
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
        'viewport ${_width}x$_height  ring=$_bufferCount  mode=${_blocking ? "blocking" : "async"}  ticker.muted=${_ticker?.muted}',
        'dart tick interval ms  p50=${_p(intervals, .5)} p95=${_p(intervals, .95)} p99=${_p(intervals, .99)}  (n=${intervals.length}, skipped while resizing=$skipped)',
        'ffi render() call ms   p50=${_p(calls, .5)} p95=${_p(calls, .95)} p99=${_p(calls, .99)}',
        'rust cpu encode+submit p50=${s.cpuP50Ms.toStringAsFixed(3)} p95=${s.cpuP95Ms.toStringAsFixed(3)} p99=${s.cpuP99Ms.toStringAsFixed(3)}',
        'submit→gpu complete    p50=${s.gpuP50Ms.toStringAsFixed(2)} p95=${s.gpuP95Ms.toStringAsFixed(2)} p99=${s.gpuP99Ms.toStringAsFixed(2)}',
        'rendered=${s.framesRendered} dropped(ring full)=${s.framesDropped}/$dropped presented=${s.framesPresented} stalePresent=${s.presentsWithoutNewFrame}',
        'resizes=${s.resizes} rust resize ms p50=${s.resizeP50Ms.toStringAsFixed(2)} max=${s.resizeMaxMs.toStringAsFixed(2)}',
        if (swift != null)
          'swift copyPixelBuffer=${swift["copyCalls"]} newFrame=${swift["copyWithNewFrame"]} noFrame=${swift["copyNoFrame"]} readySignals=${swift["frameReadySignals"]} rebinds=${swift["rebinds"]}',
        '--- events',
        ..._events,
      ].join('\n');
    });
    // Also print for scripted captures (benchmarks/m0).
    debugPrint('[m0] ${_statsText.split('\n').take(8).join(' | ')}');
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
              Texture(textureId: _textureId!, filterQuality: FilterQuality.none)
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
                  Text('clicks: $_clicks   ', style: const TextStyle(fontFamily: 'Menlo')),
                  FilledButton.icon(
                    onPressed: () => setState(() => _clicks++),
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
