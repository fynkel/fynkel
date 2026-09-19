// M0 Flutter host: Path A viewport experiment (blueprint §8.2, ADR-0002).
//
// The host owns the loop (ADR-0007): a Ticker drives `render()` once per
// Flutter frame. Rust renders into an IOSurface ring; Swift hands the newest
// GPU-complete buffer to Flutter's Texture widget; Flutter widgets are
// composited on top. Frame-timing stats are shown in the overlay.

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
      home: const ViewportPage(),
    );
  }
}

class ViewportPage extends StatefulWidget {
  const ViewportPage({super.key});

  @override
  State<ViewportPage> createState() => _ViewportPageState();
}

class _ViewportPageState extends State<ViewportPage> with SingleTickerProviderStateMixin {
  static const _channel = MethodChannel('fynkel/m0');

  M0Native? _native;
  Pointer<Void> _ctx = nullptr;
  int? _textureId;
  Ticker? _ticker;
  String? _error;

  // Viewport size in physical pixels; the ring is (re)created at this size.
  int _width = 0;
  int _height = 0;
  final bool _blocking = const bool.fromEnvironment("FYNKEL_M0_BLOCKING");

  // Frame timing on the Dart side.
  Duration? _lastTick;
  final List<double> _intervalsMs = [];
  final List<double> _renderCallMs = [];
  int _droppedThisWindow = 0;
  Timer? _statsTimer;
  String _statsText = 'starting…';
  int _clicks = 0;

  @override
  void initState() {
    super.initState();
    _statsTimer = Timer.periodic(const Duration(seconds: 1), (_) => _reportStats());
  }

  Future<void> _ensureViewport(int width, int height) async {
    if (_textureId != null || _error != null || width == 0 || height == 0) return;
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
        'width': width,
        'height': height,
      });
      if (!mounted) return;
      setState(() => _textureId = id);
      _ticker = createTicker(_onTick)..start();
    } catch (e) {
      setState(() => _error = '$e');
    }
  }

  void _onTick(Duration elapsed) {
    if (_ctx == nullptr) return;
    if (_lastTick != null) {
      _intervalsMs.add((elapsed - _lastTick!).inMicroseconds / 1000);
    }
    _lastTick = elapsed;
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
    _droppedThisWindow = 0;
    if (!mounted) return;
    setState(() {
      _statsText = [
        'viewport ${_width}x$_height  ring=$_bufferCount  mode=${_blocking ? "blocking" : "async"}',
        'dart tick interval ms  p50=${_p(intervals, .5)} p95=${_p(intervals, .95)} p99=${_p(intervals, .99)}  (n=${intervals.length})',
        'ffi render() call ms   p50=${_p(calls, .5)} p95=${_p(calls, .95)} p99=${_p(calls, .99)}',
        'rust cpu encode+submit p50=${s.cpuP50Ms.toStringAsFixed(3)} p95=${s.cpuP95Ms.toStringAsFixed(3)} p99=${s.cpuP99Ms.toStringAsFixed(3)}',
        'submit→gpu complete    p50=${s.gpuP50Ms.toStringAsFixed(2)} p95=${s.gpuP95Ms.toStringAsFixed(2)} p99=${s.gpuP99Ms.toStringAsFixed(2)}',
        'rendered=${s.framesRendered} dropped(ring full)=${s.framesDropped}/$dropped presented=${s.framesPresented} stalePresent=${s.presentsWithoutNewFrame}',
        if (swift != null)
          'swift copyPixelBuffer=${swift["copyCalls"]} newFrame=${swift["copyWithNewFrame"]} readySignals=${swift["frameReadySignals"]}',
      ].join('\n');
    });
    // Also print for scripted captures (benchmarks/m0).
    debugPrint('[m0] ${_statsText.replaceAll('\n', ' | ')}');
  }

  static String _p(List<double> sorted, double q) {
    if (sorted.isEmpty) return '-';
    final i = ((sorted.length - 1) * q).round();
    return sorted[i].toStringAsFixed(2);
  }

  @override
  void dispose() {
    _statsTimer?.cancel();
    _ticker?.dispose();
    if (_ctx != nullptr) {
      _channel.invokeMethod('unregisterTexture');
      _native?.destroy(_ctx);
      _ctx = nullptr;
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return Scaffold(
      body: LayoutBuilder(
        builder: (context, constraints) {
          final w = (constraints.maxWidth * dpr).round();
          final h = (constraints.maxHeight * dpr).round();
          // Create the ring for the first laid-out size. Resize handling is M0-06.
          WidgetsBinding.instance.addPostFrameCallback((_) => _ensureViewport(w, h));
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
      ),
    );
  }
}
