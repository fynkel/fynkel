import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'footprint.dart';
import 'game.dart';

/// Set by tool/measure.dart, never by hand: as environment variables on
/// the Mac, and as `--dart-define`s on iOS, where an app gets no
/// environment.
String _setting(String name, String defined) => Platform.environment[name] ?? defined;

/// Prints a `[stats]` line every second.
final _printStats = _setting('FYNKEL_STATS', const String.fromEnvironment('FYNKEL_STATS')) == '1';

/// Runs the create/destroy and resize check, printing a `[check]` line at
/// each point where memory should be back where it started.
final _lifecycleCheck = _setting('FYNKEL_CHECK', const String.fromEnvironment('FYNKEL_CHECK')) == 'lifecycle';
final _cycles = int.tryParse(_setting('FYNKEL_CYCLES', const String.fromEnvironment('FYNKEL_CYCLES'))) ?? 10;

/// For comparison: a spinning Flutter box where the engine's view would be,
/// so the same checks show what Flutter alone does.
final _baseline = _setting('FYNKEL_BASELINE', const String.fromEnvironment('FYNKEL_BASELINE')) == '1';

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
    // The baseline has no view to report, but its stats line changes every
    // second all the same: laying out new text costs Flutter memory, and
    // the comparison must not miss that.
    if (_baseline) _baselineStats = Timer.periodic(const Duration(seconds: 1), (_) => _onSecond('baseline=1'));
  }

  Timer? _baselineStats;

  @override
  void dispose() {
    _baselineStats?.cancel();
    SchedulerBinding.instance.removeTimingsCallback(_onTimings);
    _game.world.dispose();
    super.dispose();
  }

  void _onTimings(List<FrameTiming> timings) {
    const budget = Duration(microseconds: 8333);
    _flutterFrames += timings.length;
    _slowFrames += timings.where((t) => t.buildDuration > budget || t.rasterDuration > budget).length;
  }

  void _onStats(FynkelViewStats stats) => _onSecond('$stats', seconds: stats.window.inMicroseconds / 1e6);

  void _onSecond(String view, {double seconds = 1}) {
    final line =
        '$view flutter=${(_flutterFrames / seconds).toStringAsFixed(1)} flutterFrames=$_flutterFrames '
        'slow=$_slowFrames footprint=${physicalFootprint()} mouse=${RendererBinding.instance.mouseTracker.mouseIsConnected ? 1 : 0}';
    _flutterFrames = 0;
    _slowFrames = 0;
    _stats.value = line;
    if (_printStats) stdout.writeln('[stats] $line');
  }

  Future<void> _runLifecycleCheck() async {
    const settle = Duration(seconds: 2);
    // Nothing changes for three seconds after a mark, while the memory is
    // read (several times: see tool/measure.dart). On the Mac the tool
    // reads it from outside; on iOS nothing can, so the app reads its own
    // footprint as well and prints the lowest reading as a `[memory]` line.
    Future<void> mark(String point) async {
      stdout.writeln('[check] $point');
      var lowest = footprint();
      for (var i = 0; i < 5; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 500));
        final reading = footprint();
        if (reading.total < lowest.total) lowest = reading;
      }
      stdout.writeln('[memory] $point ${lowest.total} ${lowest.graphics}');
      await Future<void>.delayed(const Duration(milliseconds: 500));
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
              child: _baseline
                  ? const _Spinner()
                  : FynkelView(world: _game.world, onFrame: _game.onFrame, onStats: _onStats),
            ),
          ),
        // Clear of the iPhone's status bar and Dynamic Island, and wrapped
        // to the screen's width.
        Positioned(
          left: 12,
          right: 12,
          top: 12,
          child: SafeArea(
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
        ),
      ],
    );
  }
}

/// A box turning once every 4 seconds: a new Flutter frame every vsync, as
/// with the engine's view, but nothing of the engine's.
class _Spinner extends StatefulWidget {
  const _Spinner();

  @override
  State<_Spinner> createState() => _SpinnerState();
}

class _SpinnerState extends State<_Spinner> with SingleTickerProviderStateMixin {
  late final _turns = AnimationController(vsync: this, duration: const Duration(seconds: 4))..repeat();

  @override
  void dispose() {
    _turns.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => RotationTransition(
    turns: _turns,
    child: const ColoredBox(color: Colors.teal),
  );
}
