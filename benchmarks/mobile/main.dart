// Installed as lib/main.dart by prepare.py --app. This is a bounded diagnostic,
// not a game template and not a sustained-performance qualification.
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui' show FramePhase;

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:path_provider/path_provider.dart';

import 'game/game.dart';
import 'capture_gate.dart';

void main() => runApp(const GameApp());

class GameApp extends StatelessWidget {
  const GameApp({super.key, this.launch = const FynkelLaunch()});
  final FynkelLaunch launch;
  @override
  Widget build(BuildContext context) => const MaterialApp(home: _Capture());
}

class _Capture extends StatefulWidget {
  const _Capture();
  @override
  State<_Capture> createState() => _CaptureState();
}

class _CaptureState extends State<_Capture> with WidgetsBindingObserver {
  final _view = GlobalKey<FynkelGameViewState>();
  final _clock = Stopwatch();
  bool _timeline = bool.fromEnvironment('FYNKEL_CAPTURE_TIMELINE');
  final _flutterFrames = <Map<String, Object?>>[];
  final _hostFrames = <Map<String, Object?>>[];
  bool _captureTicks = bool.fromEnvironment('FYNKEL_CAPTURE_CPU_TICKS');
  bool _captureCpu = bool.fromEnvironment('FYNKEL_CAPTURE_CPU_TICKS') || bool.fromEnvironment('FYNKEL_CAPTURE_CPU');
  bool _freezeScene = bool.fromEnvironment('FYNKEL_CAPTURE_FREEZE_SCENE');
  final _cpuFrames = <Map<String, Object?>>[];
  final _cpuTicks = <Map<String, Object?>>[];
  int _acquireWaitUs = int.fromEnvironment('FYNKEL_CAPTURE_ACQUIRE_WAIT_US', defaultValue: 3000);
  String _quality = 'medium';
  bool _qualityVerified = false;
  int _bufferCount = 4;
  double _resolutionScale = 1;
  bool _pipelined = false;
  bool _ignoreInactive = false;
  int _frameQueue = 1;
  int _limitSeconds = 8;
  late final FynkelViewportController _controller;
  bool _controllerCreated = false;
  Game? _game;
  IOSink? _log;
  Timer? _deadline;
  String _status = 'Loading dense workload';
  bool _finished = false;
  Future<void>? _closing;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _load();
  }

  Future<void> _load() async {
    try {
      final dir = await getApplicationDocumentsDirectory();
      if (!mounted) return;
      // This Flutter iOS host does not expose devicectl variables to Dart.
      // Consume the runner's request once; manual launches have no capture ID.
      final request = File('${dir.path}/capture-request.json');
      String? captureId;
      var awaitProfiler = false;
      if (await request.exists()) {
        final value = jsonDecode(await request.readAsString());
        await request.delete();
        final id = value is Map<String, dynamic> ? value['captureId'] : null;
        if (id is! String || !RegExp(r'^[0-9a-f]{32}$').hasMatch(id)) {
          throw StateError('Invalid capture request');
        }
        captureId = id;
        final waiting = value['awaitProfiler'] ?? false;
        if (waiting is! bool) throw StateError('Invalid profiler wait option');
        awaitProfiler = waiting;
        final quality = value['quality'] ?? 'medium';
        if (!['medium', 'medium-no-ao', 'medium-no-contact', 'medium-no-screen-space', 'medium-no-bloom',
          'medium-msaa2', 'medium-msaa1', 'medium-bare'].contains(quality)) {
          throw StateError('Invalid diagnostic quality');
        }
        _quality = quality as String;
        final timeline = value['timeline'] ?? _timeline;
        final frozen = value['freezeScene'] ?? _freezeScene;
        final ticks = value['cpuTicks'] ?? _captureTicks;
        final wait = value['acquireWaitUs'] ?? _acquireWaitUs;
        final buffers = value['bufferCount'] ?? 4;
        final scale = value['resolutionScale'] ?? 1.0;
        final pipelinedValue = value['pipelined'] ?? false;
        if (pipelinedValue is! bool) throw StateError('Invalid pipelined option');
        _pipelined = pipelinedValue;
        _ignoreInactive = value['ignoreInactive'] == true;
        final queue = value['frameQueue'] ?? 1;
        if (queue is! int || queue < 1 || queue > 3) throw StateError('Invalid frameQueue option');
        _frameQueue = queue;
        final limit = value['limitSeconds'] ?? 8;
        if (limit is! int || limit < 1 || limit > 600) throw StateError('Invalid limitSeconds option');
        _limitSeconds = limit;
        if (timeline is! bool || frozen is! bool || ticks is! bool || ![0, 3000].contains(wait) ||
            ![4, 5, 6, 8].contains(buffers) || ![0.5, 0.75, 1.0].contains(scale)) {
          throw StateError('Invalid diagnostic options');
        }
        _timeline = timeline; _freezeScene = frozen;
        _captureTicks = ticks; _captureCpu = _captureCpu || ticks;
        _acquireWaitUs = wait as int; _bufferCount = buffers as int;
        _resolutionScale = (scale as num).toDouble();
      }
      if (!mounted || _finished) return;
      _controller = FynkelViewportController(placeholders: false,
        acquireWaitUs: _acquireWaitUs, bufferCount: _bufferCount, pipelined: _pipelined, frameQueue: _frameQueue)..quality = _quality;
      _controllerCreated = true;
      if (_timeline) SchedulerBinding.instance.addTimingsCallback(_flutterTimings);
      _log = File('${dir.path}/mobile-capture.jsonl').openWrite();
      // Attach an error handler before the first asynchronous file write.
      unawaited(
        _log!.done.catchError((Object error) {
          if (mounted) _stop('log-error: $error');
        }),
      );
      _write({
        'event': 'start',
        'captureId': captureId,
        'awaitProfiler': awaitProfiler,
        'quality': _quality,
        'timeline': _timeline,
        'limitSeconds': _limitSeconds,
        'sustainedQualification': false,
        'cpuReadback': _captureCpu,
        'cpuTickReadback': _captureTicks,
        'acquireWaitUs': _acquireWaitUs,
        'bufferCount': _bufferCount, 'resolutionScale': _resolutionScale, 'pipelined': _pipelined, 'frameQueue': _frameQueue,
        'frozenScene': _freezeScene,
        'settleTicks': _freezeScene ? 120 : 0,
      });
      if (awaitProfiler) {
        setState(() => _status = 'Waiting for profiler — scene is not running');
        final released = await waitForCaptureRelease(
          dir, captureId!, isActive: () => mounted && !_finished,
        );
        if (!released) return;
        _write({'event': 'profiler-ready', 'captureId': captureId});
      }
      final cache = await unpackBundledCache();
      if (!mounted || _finished) return;
      if (cache == null) throw StateError('Bundled cache missing');
      if (_captureTicks && _freezeScene) throw StateError('Tick readback requires a live scene');
      _game = Game.load('(bundled)', cacheDir: cache);
      if (_captureTicks) {
        _game!.world.frameTimes.collectTickTimes = true;
      }
      if (_freezeScene) {
        // Diagnostic only: identical settled camera/physics/animation state
        // for every GPU sample. The normal dense gameplay capture stays live.
        for (var i = 0; i < 120; i++) {
          _game!.world.stepFrame();
        }
        _game!.world.timeScale = 0;
      }
      setState(
        () => _status = _freezeScene
            ? 'FROZEN GPU TEST — movement intentionally paused\nRendering for 8 seconds · $_quality'
            : 'LIVE TEST — objects should move\nRuns for 8 seconds, then pauses · $_quality',
      );
    } catch (error) {
      _stop('load-error: $error');
    }
  }

  void _write(Map<String, Object?> data) {
    _log?.writeln(
      jsonEncode({'time': DateTime.now().toUtc().toIso8601String(), 'elapsedMs': _clock.elapsedMilliseconds, ...data}),
    );
  }

  void _stats(FynkelFrameStats stats) {
    if (_finished) return;
    if (!_qualityVerified) {
      // Revalidate against the world's attached asset store, not the empty
      // viewport that existed before its first render.
      _controller.quality = _quality;
      if (!_controller.qualityOk) { _stop('unknown-quality-profile'); return; }
      _qualityVerified = true;
    }
    final thermal = stats.platform?.thermalState ?? ViewportThermalState.unknown;
    _write({
      'event': 'stats',
      'quality': _quality, 'qualityVerified': _qualityVerified,
      'windowSeconds': stats.windowSeconds,
      'loopFps': stats.fps,
      'acquiredFps': stats.textureAcquisitionFps,
      'width': stats.physicalWidth,
      'height': stats.physicalHeight,
      'thermal': thermal.name,
      'summary': stats.summary(),
      'simulationTick': _game!.world.tick,
      'frozenScene': _freezeScene,
    });
    if (thermal == ViewportThermalState.serious || thermal == ViewportThermalState.critical) {
      _stop('thermal-${thermal.name}');
    }
  }

  void _captureFrame(double dt, int ticks) {
    if (!_captureCpu || _finished) return;
    if (_cpuFrames.length >= 4096) {
      _stop('sample-limit');
      return;
    }
    final readback = Stopwatch()..start();
    final world = _game!.world;
    final times = world.frameTimes;
    if (_captureTicks) {
      for (final tick in times.tickTimes) {
        _cpuTicks.add({
          'frameIndex': _cpuFrames.length,
          'tick': tick.tick,
          'systems': [
            for (final time in tick.systems)
              {'system': time.system.name, 'micros': time.micros, 'entities': time.entities},
          ],
          'readbackUs': tick.readbackUs,
          'stagesUs': tick.stagesUs,
        });
      }
    }
    // Per-system data describes only the LAST native tick of a host frame.
    // Do not reuse the preceding frame's data when this frame has no ticks.
    final sample = <String, Object?>{
      'elapsedUs': _clock.elapsedMicroseconds,
      'frameDeltaUs': (dt * 1000000).round(),
      'ticks': ticks,
      'totalUs': times.totalUs,
      'nativeUs': times.nativeUs,
      'behavioursUs': times.behavioursUs,
      if (ticks > 0)
        'lastTickSystems': [
          for (final time in world.systemTimes())
            {'system': time.system.name, 'micros': time.micros, 'entities': time.entities},
        ],
    };
    sample['readbackUs'] = readback.elapsedMicroseconds;
    _cpuFrames.add(sample);
  }

  void _flutterTimings(List<FrameTiming> timings) {
    for (final timing in timings) {
      if (_flutterFrames.length >= 4096) break;
      final wall = timing.timestampInMicroseconds(FramePhase.rasterFinishWallTime);
      final offset = wall - timing.timestampInMicroseconds(FramePhase.rasterFinish);
      _flutterFrames.add({'frameNumber': timing.frameNumber,
        for (final phase in FramePhase.values)
          phase.name: phase == FramePhase.rasterFinishWallTime ? wall :
            timing.timestampInMicroseconds(phase) + offset});
    }
  }

  void _stop(String reason) {
    if (_finished) return;
    _finished = true;
    _deadline?.cancel();
    _view.currentState?.setPaused(true);
    if (_timeline) {
      SchedulerBinding.instance.removeTimingsCallback(_flutterTimings);
      _write({'event': 'hostFrames', 'samples': _hostFrames});
      _write({'event': 'flutterFrames', 'samples': _flutterFrames});
    }
    if (_captureCpu) _write({'event': 'cpuFrames', 'samples': _cpuFrames});
    if (_captureTicks) _write({'event': 'cpuTicks', 'samples': _cpuTicks});
    _write({'event': 'stop', 'reason': reason});
    _closing = _closeLog();
    if (mounted) {
      setState(() => _status = reason == 'deadline'
          ? 'TEST COMPLETE — intentionally paused\nRelaunch the app to run again'
          : 'TEST STOPPED: $reason');
    }
  }

  Future<void> _closeLog() async {
    try {
      await _log?.flush();
      await _log?.close();
    } catch (error) {
      if (mounted) setState(() => _status = 'Capture log failed: $error');
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state != AppLifecycleState.resumed && !_ignoreInactive) _stop('lifecycle-${state.name}');
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    if (_timeline) SchedulerBinding.instance.removeTimingsCallback(_flutterTimings);
    _deadline?.cancel();
    if (_closing == null) unawaited(_closeLog());
    if (_controllerCreated) _controller.dispose();
    _game?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    body: _game == null
        ? Center(child: Text(_status))
        : FynkelGameView(
            key: _view,
            resolutionScale: _resolutionScale,
            world: _game!.world,
            controller: _controller,
            onFrame: (dt, plan) {
              if (_finished) {
                _view.currentState?.setPaused(true);
              } else if (!_clock.isRunning) {
                _clock.start();
                // Wall-clock deadline, independent of simulated time/frame rate.
                _deadline = Timer(Duration(seconds: _limitSeconds), () => _stop('deadline'));
              }
              _captureFrame(dt, plan.steps);
            },
            onFrameTrace: !_timeline ? null : (trace) {
              if (_finished) return;
              if (_hostFrames.length >= 4096) { _stop('trace-limit'); return; }
              _hostFrames.add({'frame': _hostFrames.length,
                'startUs': trace.startUs, 'simulationEndUs': trace.simulationEndUs,
                'renderStartUs': trace.renderStartUs, 'endUs': trace.endUs,
                'steps': trace.steps, 'tick': trace.tick, 'slot': trace.slot});
            },
            onStats: _stats,
            overlay: SafeArea(
              child: Align(
                alignment: Alignment.topLeft,
                child: ColoredBox(
                  color: Colors.black87,
                  child: Padding(
                    padding: const EdgeInsets.all(12),
                    child: Text(_status, style: const TextStyle(
                      color: Colors.white, fontSize: 18, fontWeight: FontWeight.bold)),
                  ),
                ),
              ),
            ),
          ),
  );
}
