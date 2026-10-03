// Installed as lib/main.dart by prepare.py --app. This is a bounded diagnostic,
// not a game template and not a sustained-performance qualification.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:path_provider/path_provider.dart';

import 'game/game.dart';

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
  static const _captureTicks = bool.fromEnvironment('FYNKEL_CAPTURE_CPU_TICKS');
  static const _captureCpu = _captureTicks || bool.fromEnvironment('FYNKEL_CAPTURE_CPU');
  static const _freezeScene = bool.fromEnvironment('FYNKEL_CAPTURE_FREEZE_SCENE');
  final _cpuFrames = <Map<String, Object?>>[];
  final _cpuTicks = <Map<String, Object?>>[];
  static const _acquireWaitUs = int.fromEnvironment('FYNKEL_CAPTURE_ACQUIRE_WAIT_US', defaultValue: 3000);
  final _controller = FynkelViewportController(placeholders: false, acquireWaitUs: _acquireWaitUs)..quality = 'medium';
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
      if (await request.exists()) {
        final value = jsonDecode(await request.readAsString());
        await request.delete();
        final id = value is Map<String, dynamic> ? value['captureId'] : null;
        if (id is! String || !RegExp(r'^[0-9a-f]{32}$').hasMatch(id)) {
          throw StateError('Invalid capture request');
        }
        captureId = id;
      }
      if (!mounted || _finished) return;
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
        'quality': 'medium',
        'limitSeconds': 8,
        'sustainedQualification': false,
        'cpuReadback': _captureCpu,
        'cpuTickReadback': _captureTicks,
        'acquireWaitUs': _acquireWaitUs,
        'frozenScene': _freezeScene,
        'settleTicks': _freezeScene ? 120 : 0,
      });
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
            ? 'FROZEN GPU TEST — movement intentionally paused\nRendering for 8 seconds'
            : 'LIVE TEST — objects should move\nRuns for 8 seconds, then pauses',
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
    final thermal = stats.platform?.thermalState ?? ViewportThermalState.unknown;
    _write({
      'event': 'stats',
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

  void _stop(String reason) {
    if (_finished) return;
    _finished = true;
    _deadline?.cancel();
    _view.currentState?.setPaused(true);
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
    if (state != AppLifecycleState.resumed) _stop('lifecycle-${state.name}');
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _deadline?.cancel();
    if (_closing == null) unawaited(_closeLog());
    _controller.dispose();
    _game?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    body: _game == null
        ? Center(child: Text(_status))
        : FynkelGameView(
            key: _view,
            world: _game!.world,
            controller: _controller,
            onFrame: (dt, plan) {
              if (_finished) {
                _view.currentState?.setPaused(true);
              } else if (!_clock.isRunning) {
                _clock.start();
                // Wall-clock deadline, independent of simulated time/frame rate.
                _deadline = Timer(const Duration(seconds: 8), () => _stop('deadline'));
              }
              _captureFrame(dt, plan.steps);
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
