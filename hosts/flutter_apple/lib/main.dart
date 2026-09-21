// Fynkel Flutter macOS/iOS host (blueprint §5.2, ADR-0007/0008, ADR-0002
// Path A) — the measurement harness, now a thin app on
// `package:fynkel_flutter`: `FynkelGameView` owns the Ticker-driven loop
// (simulate, then render the extract into the IOSurface ring) and the
// viewport texture; this file keeps the M0 harness around it: resize
// (window / DPI / inset), lifecycle, dispose/recreate, HUD overlay with
// pointer/keyboard focus, and the `[host]` stat lines `benchmarks/m0`
// parses.

import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'demo_scene.dart';

const _bufferCount = 4;

/// `viewport` (default) or `none` (no viewport: raster control).
/// `FYNKEL_HOST_MODE` env var at runtime, else the dart-define.
final String hostMode = () {
  final env = Platform.environment['FYNKEL_HOST_MODE'];
  if (env != null && env.isNotEmpty) return env;
  return const String.fromEnvironment('FYNKEL_HOST_MODE', defaultValue: 'viewport');
}();

/// A project directory to load instead of the Dart demo scene: its startup
/// scene goes into the world and its cooked cache (`fynkel cook`) feeds the
/// renderer. `FYNKEL_PROJECT` env var at runtime, else the dart-define.
final String? projectDir = () {
  final env = Platform.environment['FYNKEL_PROJECT'];
  if (env != null && env.isNotEmpty) return env;
  const define = String.fromEnvironment('FYNKEL_PROJECT');
  return define.isEmpty ? null : define;
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
    final b = MsPercentiles.of(_buildMs);
    final r = MsPercentiles.of(_rasterMs);
    _buildMs.clear();
    _rasterMs.clear();
    if (!mounted) return;
    setState(() {
      _timingsText =
          'flutter frames n=${r.count} viewport=${_showViewport ? hostMode : "off"}  ui build ms p50=${b.p50.toStringAsFixed(2)} p95=${b.p95.toStringAsFixed(2)}  raster ms ${r.format()}';
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
          // Wrap: the controls fold onto several lines on a phone (iOS host).
          Padding(
            padding: const EdgeInsets.all(8),
            child: Wrap(
              alignment: WrapAlignment.center,
              crossAxisAlignment: WrapCrossAlignment.center,
              spacing: 8,
              runSpacing: 4,
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

class _ViewportPageState extends State<ViewportPage> with WidgetsBindingObserver {
  static const _syncMode = int.fromEnvironment('FYNKEL_SYNC', defaultValue: 2);
  static const _syncModeNames = ['async-poll', 'blocking', 'metal-completion'];
  static const _acquireWaitUs = int.fromEnvironment('FYNKEL_ACQUIRE_WAIT_US', defaultValue: 3000);
  static const _satellites = int.fromEnvironment('FYNKEL_SATELLITES', defaultValue: 24);

  // Engine: one dylib serves the engine ABI (package:fynkel) and the viewport ABI.
  late final World _world;
  DemoScene? _scene;
  LoadReport? _project;
  final _gameKey = GlobalKey<FynkelGameViewState>();
  late final FynkelViewportController _controller;
  String? _error;
  bool _tornDown = false;
  bool _announcedCreate = false;
  int _lastResizes = 0;

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
    _viewportFocus.addListener(() => _log('viewport focus=${_viewportFocus.hasFocus}'));
    _hudTextFocus.addListener(() => _log('hud text focus=${_hudTextFocus.hasFocus}'));
    _log('init generation=${widget.generation}');
    _world = World.create();
    // Release-mode hosts log diagnostics; debug throws (FynkelError).
    _world.onDiagnostics = (d) => _log('diagnostics: ${d.join("; ")}');
    _world.onLoadDiagnostics = (d) => _log('load: ${d.join("; ")}');
    try {
      final dir = projectDir;
      if (dir == null) {
        _scene = DemoScene(satellites: _satellites);
      } else {
        // The cooked scene, not Dart, decides what exists (blueprint §7):
        // the world reads `<dir>/.fynkel/cache` (`fynkel run` cooks first)
        // and the viewport draws from the same asset store.
        final r = _project = _world.loadProject(dir);
        _log('loaded ${r.projectName}: scene "${r.sceneName}" ${r.entityCount} entities, ${r.assetCount} assets from ${r.cacheDir} (${r.scenePath})');
      }
    } on FynkelLoadError catch (e) {
      _error = '$e\nrun `fynkel cook` in the project (or `fynkel run`, which cooks first)';
    } catch (e) {
      _error = '$e';
    }
    _controller = FynkelViewportController(
      bufferCount: _bufferCount,
      syncMode: ViewportSyncMode.values[_syncMode],
      acquireWaitUs: _acquireWaitUs,
      placeholders: _project == null,
      camera: const ViewportCameraSpec(eye: Vec3(0, 2.5, 7.5), target: Vec3(0, 0, 0)),
    );
    _controller.addListener(_onViewportChanged);
  }

  void _onViewportChanged() {
    final c = _controller;
    switch (c.state) {
      case ViewportState.attached when !_announcedCreate:
        _announcedCreate = true;
        _log('created ${c.physicalWidth}x${c.physicalHeight} texture=${c.textureId} world tick=${_world.tick}');
        // A game host wants the world to own keyboard focus until a HUD widget takes it.
        _viewportFocus.requestFocus();
      case ViewportState.attached when c.resizes != _lastResizes:
        _lastResizes = c.resizes;
        _log('resized ${c.physicalWidth}x${c.physicalHeight}');
      case ViewportState.failed:
        setState(() => _error = '${c.error}');
      default:
        break;
    }
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

  void _onPointer(PointerEvent e, Offset physical) {
    // Viewport physical pixels: the engine's coordinate space.
    final px = physical.dx.round();
    final py = physical.dy.round();
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
    // (keep rendering). FynkelGameView mutes its ticker for hidden/paused,
    // which pauses the simulation clock (§5.2: no backlog while paused).
    final mute = state == AppLifecycleState.hidden || state == AppLifecycleState.paused;
    _log('lifecycle $state ticker.muted=$mute');
  }

  @override
  void didChangeMetrics() {
    // Fires on window resize and display/DPI change; the viewport's
    // LayoutBuilder sees the new constraints on the next frame.
    final v = View.of(context);
    _log('metrics dpr=${v.devicePixelRatio} physical=${v.physicalSize.width.round()}x${v.physicalSize.height.round()}');
  }

  /// Ordered teardown (the package's `detach`: clear the Rust→Swift callback,
  /// unregister the texture, free the viewport), then dispose the world.
  /// Safe to call more than once.
  Future<void> teardown() async {
    if (_tornDown) return;
    _tornDown = true;
    _gameKey.currentState?.setPaused(true);
    if (_controller.isAttached) {
      await _controller.detach();
      _log('viewport destroyed');
    }
    final s = _world.stats();
    _world.dispose();
    _log('world disposed after tick ${s.tick} (${s.liveEntities} entities)');
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _controller.removeListener(_onViewportChanged);
    _viewportFocus.dispose();
    _hudTextFocus.dispose();
    // If the parent did not call teardown() first (e.g. navigation), run it
    // now; the async tail (unregister → destroy) completes after dispose.
    teardown();
    _controller.dispose();
    super.dispose();
  }

  // ---- loop ----------------------------------------------------------------

  void _fixedUpdate(int tick) => _scene?.fixedUpdate(_world, tick);

  void _onStats(FynkelFrameStats f) {
    final s = f.viewport;
    final b = f.world;
    final swift = f.platform;
    if (!mounted || _tornDown) return;
    setState(() {
      _statsText = [
        'viewport ${f.physicalWidth}x${f.physicalHeight}  ring=$_bufferCount  mode=${_syncModeNames[_syncMode]} acquireWait=${_acquireWaitUs}us  ticker.muted=${f.muted}',
        if (_project != null)
          'project "${_project!.projectName}" scene "${_project!.sceneName}" (${_project!.entityCount} entities, ${_project!.rejectedComponents} rejected)  cache=${_project!.cacheDir}',
        if (s != null)
          'world tick=${b.tick} entities=${b.liveEntities} instances=${s.instances} draws=${s.draws} culled=${s.culled} shadow=${s.shadowCasters}/${s.shadowDraws} meshes=${s.meshes} placeholders=${s.placeholders} missing=${s.missing} assetErrors=${s.assetErrors} sceneCamera=${s.sceneCamera}  fixed ticks/s=${f.fixedTicks} missedDeadlines=${f.missedDeadlines}  bridge calls=${b.ffiCalls} bytes in/out=${b.bytesIn}/${b.bytesOut}',
        'dart tick interval ms  ${f.tickInterval.format()}  (n=${f.tickInterval.count}, skipped while resizing=${f.skippedWhileResizing})',
        'dart simulate ms       ${f.simulate.format()}  (gameplay + submit + step + snapshot)',
        'ffi render() call ms   ${f.renderCall.format()}  (extract + encode + submit)',
        'dart behaviours ms     ${f.behaviours.format(3)}  native step ms ${f.nativeStep.format(3)}  total work ms ${f.totalWork.format()}',
        if (s != null) ...[
          'rust extract ms        ${s.extract.format(3)}  prepare ms ${s.prepare.format(3)}  encode ms ${s.encode.format(3)}',
          'gpu pass ms            shadow p95=${s.gpuShadow.p95.toStringAsFixed(3)} scene p95=${s.gpuScene.p95.toStringAsFixed(3)} post p95=${s.gpuPost.p95.toStringAsFixed(3)}  frame ${s.gpuFrame.format(3)}  (n=${s.gpuFrame.count}, untimed=${s.gpuFramesUntimed}, timestamps=${s.gpuTimestamps})',
          'memory MB              native=${(b.nativeBytes / 1048576).toStringAsFixed(1)} resident=${(b.residentBytes / 1048576).toStringAsFixed(1)} assets=${(b.residencyBytes / 1048576).toStringAsFixed(1)} gpu=${(s.gpuBytes / 1048576).toStringAsFixed(1)}  snapshot B=${b.snapshotBytes} (peak ${b.snapshotPeakBytes}, budget ${b.snapshotBudget})',
          'rust cpu encode+submit p50=${s.cpuP50Ms.toStringAsFixed(3)} p95=${s.cpuP95Ms.toStringAsFixed(3)} p99=${s.cpuP99Ms.toStringAsFixed(3)}',
          'submit→gpu complete    p50=${s.gpuP50Ms.toStringAsFixed(2)} p95=${s.gpuP95Ms.toStringAsFixed(2)} p99=${s.gpuP99Ms.toStringAsFixed(2)}',
          'rendered=${s.framesRendered} dropped(ring full)=${s.framesDropped}/${f.dropped} presented=${s.framesPresented} stalePresent=${s.presentsWithoutNewFrame}',
          'present lag (frames behind newest submit) p50=${s.presentLagP50} p95=${s.presentLagP95} max=${s.presentLagMax}  raster acquire wait ms p50=${s.acquireWaitP50Ms.toStringAsFixed(2)} p95=${s.acquireWaitP95Ms.toStringAsFixed(2)} max=${s.acquireWaitMaxMs.toStringAsFixed(2)}',
          'resizes=${s.resizes} rust resize ms p50=${s.resizeP50Ms.toStringAsFixed(2)} max=${s.resizeMaxMs.toStringAsFixed(2)}',
        ],
        if (f.systems.isNotEmpty)
          'systems µs (last tick) ${f.systems.where((t) => t.micros > 0).map((t) => '${t.system.name}=${t.micros}').join(' ')}',
        if (swift != null)
          'swift copyPixelBuffer=${swift.copyCalls} newFrame=${swift.copyWithNewFrame} noFrame=${swift.copyNoFrame} readySignals=${swift.frameReadySignals} rebinds=${swift.rebinds}',
        'input: pointer=$_pointer events/s=$_pointerEventsThisWindow clicks=$_viewportClicks  keysDown=${_keysDown.toList()} keyEvents=$_keyEventsToViewport  focus=${FocusManager.instance.primaryFocus?.debugLabel ?? "?"}',
        '--- events',
        ..._events,
      ].join('\n');
    });
    _pointerEventsThisWindow = 0;
    // Also print for scripted captures (benchmarks/).
    debugPrint('[host] ${_statsText.split('\n').take(17).join(' | ')}');
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null) {
      return Center(child: Text(_error!, textAlign: TextAlign.center));
    }
    return FynkelGameView(
      key: _gameKey,
      world: _world,
      controller: _controller,
      fixedUpdate: _fixedUpdate,
      onStats: _onStats,
      focusNode: _viewportFocus,
      autofocus: false,
      onKeyEvent: _onViewportKey,
      onPointerEvent: _onPointer,
      placeholder: const Center(child: Text('creating viewport…', textAlign: TextAlign.center)),
      // Flutter overlay: the "HUD over the 3D world".
      overlay: Stack(
        fit: StackFit.expand,
        children: [
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
      ),
    );
  }
}
