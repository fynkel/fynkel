import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';

/// Set by tool/measure.dart, never by hand.
final _env = Platform.environment;

/// Prints a `[stats]` line every second.
final _printStats = _env['FYNKEL_STATS'] == '1';

/// Runs the create/destroy and resize check, printing a `[check]` line at
/// each point where memory should be back where it started.
final _lifecycleCheck = _env['FYNKEL_CHECK'] == 'lifecycle';
final _cycles = int.tryParse(_env['FYNKEL_CYCLES'] ?? '') ?? 10;

void main() => runApp(const HelloApp());

class HelloApp extends StatelessWidget {
  const HelloApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
    debugShowCheckedModeBanner: false,
    theme: ThemeData.dark(),
    home: const Scaffold(body: HelloPage()),
  );
}

class HelloPage extends StatefulWidget {
  const HelloPage({super.key});

  @override
  State<HelloPage> createState() => _HelloPageState();
}

class _HelloPageState extends State<HelloPage> {
  final _game = Hello();
  final _stats = ValueNotifier('');

  // Flutter's own frames in the current stats window: how many, and how
  // many had a build or raster phase longer than one 120 Hz frame. The
  // line also says whether a mouse is over the view: while one is, Flutter
  // hit-tests its position after every frame, which roughly doubles the
  // Dart allocation per frame (docs/stage-1.md §7).
  int _flutterFrames = 0, _slowFrames = 0;

  // The lifecycle check: whether the view is there, and its size.
  bool _shown = true;
  double _scale = 1;

  @override
  void initState() {
    super.initState();
    SchedulerBinding.instance.addTimingsCallback(_onTimings);
    if (_lifecycleCheck) unawaited(_runLifecycleCheck());
  }

  @override
  void dispose() {
    SchedulerBinding.instance.removeTimingsCallback(_onTimings);
    _game.world.dispose();
    super.dispose();
  }

  void _onTimings(List<FrameTiming> timings) {
    const budget = Duration(microseconds: 8333);
    _flutterFrames += timings.length;
    _slowFrames += timings.where((t) => t.buildDuration > budget || t.rasterDuration > budget).length;
  }

  void _onStats(FynkelViewStats stats) {
    final seconds = stats.window.inMicroseconds / 1e6;
    final line =
        '$stats flutter=${(_flutterFrames / seconds).toStringAsFixed(1)} flutterFrames=$_flutterFrames '
        'slow=$_slowFrames mouse=${RendererBinding.instance.mouseTracker.mouseIsConnected ? 1 : 0}';
    _flutterFrames = 0;
    _slowFrames = 0;
    _stats.value = line;
    if (_printStats) stdout.writeln('[stats] $line');
  }

  Future<void> _runLifecycleCheck() async {
    const settle = Duration(seconds: 2);
    // Nothing changes for a second after a mark, while the tool reads the
    // memory.
    Future<void> mark(String point) async {
      stdout.writeln('[check] $point');
      await Future<void>.delayed(const Duration(seconds: 1));
    }

    await Future<void>.delayed(const Duration(seconds: 5));
    await mark('start');
    for (var i = 1; i <= _cycles; i++) {
      setState(() => _shown = false);
      await Future<void>.delayed(settle);
      setState(() => _shown = true);
      await Future<void>.delayed(settle);
      await mark('created $i');
    }
    for (var i = 1; i <= _cycles; i++) {
      setState(() => _scale = i.isOdd ? 0.6 : 1);
      await Future<void>.delayed(settle);
      await mark('resized $i');
    }
    await mark('done');
  }

  @override
  Widget build(BuildContext context) {
    return Stack(
      children: [
        if (_shown)
          Center(
            child: FractionallySizedBox(
              widthFactor: _scale,
              heightFactor: _scale,
              child: FynkelView(world: _game.world, onFrame: _game.onFrame, onStats: _onStats),
            ),
          ),
        Positioned(
          left: 12,
          top: 12,
          child: RepaintBoundary(
            child: ValueListenableBuilder(
              valueListenable: _stats,
              builder: (context, line, _) => Text(
                line,
                style: const TextStyle(fontFamily: 'Menlo', fontSize: 11, color: Colors.white70),
              ),
            ),
          ),
        ),
      ],
    );
  }
}
