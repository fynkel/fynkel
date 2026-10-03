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
  final _controller = FynkelViewportController(placeholders: false)..quality = 'medium';
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
      _log = File('${dir.path}/mobile-capture.jsonl').openWrite();
      // Attach an error handler before the first asynchronous file write.
      unawaited(
        _log!.done.catchError((Object error) {
          if (mounted) _stop('log-error: $error');
        }),
      );
      _write({'event': 'start', 'quality': 'medium', 'limitSeconds': 8, 'sustainedQualification': false});
      final cache = await unpackBundledCache();
      if (!mounted || _finished) return;
      if (cache == null) throw StateError('Bundled cache missing');
      _game = Game.load('(bundled)', cacheDir: cache);
      setState(() => _status = 'Capturing for at most 8 seconds');
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
    });
    if (thermal == ViewportThermalState.serious || thermal == ViewportThermalState.critical) {
      _stop('thermal-${thermal.name}');
    }
  }

  void _stop(String reason) {
    if (_finished) return;
    _finished = true;
    _deadline?.cancel();
    _view.currentState?.setPaused(true);
    _write({'event': 'stop', 'reason': reason});
    _closing = _closeLog();
    if (mounted) setState(() => _status = 'Stopped: $reason');
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
            onFrame: (_, _) {
              if (_finished) {
                _view.currentState?.setPaused(true);
              } else if (!_clock.isRunning) {
                _clock.start();
                // Wall-clock deadline, independent of simulated time/frame rate.
                _deadline = Timer(const Duration(seconds: 8), () => _stop('deadline'));
              }
            },
            onStats: _stats,
            overlay: SafeArea(
              child: Align(
                alignment: Alignment.topLeft,
                child: ColoredBox(
                  color: Colors.black87,
                  child: Text(_status, style: const TextStyle(color: Colors.white)),
                ),
              ),
            ),
          ),
  );
}
