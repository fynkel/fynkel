import 'dart:async';
import 'dart:developer';
import 'dart:io';
import 'dart:isolate';

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

  // Flutter's own frames in the current stats window: how many, how many
  // had a build or raster phase longer than one frame of the display
  // (8.33 ms at 120 Hz, 16.7 ms at 60 Hz), which of the two it was, and
  // the longest raster phase (a slow frame's cause is usually there). The
  // line also says whether a mouse is over the view: while one is, Flutter
  // hit-tests its position after every frame, which roughly doubles the
  // Dart allocation per frame (docs/stage-1.md §7).
  int _flutterFrames = 0, _slowFrames = 0, _slowBuilds = 0, _slowRasters = 0;
  Duration _longestRaster = Duration.zero;

  // The lifecycle check: whether the view is there, and its size.
  bool _shown = true;
  double _scale = 1;

  @override
  void initState() {
    super.initState();
    SchedulerBinding.instance.addTimingsCallback(_onTimings);
    _footprint();
    if (_lifecycleCheck) unawaited(_runLifecycleCheck());
    // The baseline has no view to report, but its stats line changes every
    // second all the same: laying out new text costs Flutter memory, and
    // the comparison must not miss that.
    if (_baseline) _baselineStats = Timer.periodic(const Duration(seconds: 1), (_) => _onSecond('baseline=1'));
  }

  Timer? _baselineStats;

  // On Android the footprint is the PSS from /proc/self/smaps_rollup, which
  // the kernel adds up over every mapping: up to 20 ms with the GPU mapped.
  // Read on the UI thread, inside the view's tick, it made one frame a
  // second late (59 fps on a 60 Hz phone). So there it's read on another
  // isolate, and the stats line shows the latest reading.
  int? _lastFootprint;
  bool _readingFootprint = false;

  int _footprint() {
    if (!Platform.isAndroid) return physicalFootprint();
    if (!_readingFootprint) {
      _readingFootprint = true;
      unawaited(
        Isolate.run(physicalFootprint).then((bytes) {
          _lastFootprint = bytes;
          _readingFootprint = false;
        }),
      );
    }
    return _lastFootprint ?? 0;
  }

  @override
  void dispose() {
    _baselineStats?.cancel();
    SchedulerBinding.instance.removeTimingsCallback(_onTimings);
    _game.world.dispose();
    super.dispose();
  }

  void _onTimings(List<FrameTiming> timings) {
    final budget = _budget;
    _flutterFrames += timings.length;
    for (final t in timings) {
      final build = t.buildDuration > budget, raster = t.rasterDuration > budget;
      if (build || raster) _slowFrames++;
      if (build) _slowBuilds++;
      if (raster) _slowRasters++;
      if (t.rasterDuration > _longestRaster) _longestRaster = t.rasterDuration;
    }
  }

  /// One frame of the display; 120 Hz when Flutter doesn't know its rate.
  Duration get _budget {
    final hz = WidgetsBinding.instance.platformDispatcher.views.firstOrNull?.display.refreshRate ?? 0;
    return Duration(microseconds: (1e6 / (hz > 0 ? hz : 120)).round());
  }

  void _onStats(FynkelViewStats stats) => _onSecond('$stats', seconds: stats.window.inMicroseconds / 1e6);

  void _onSecond(String view, {double seconds = 1}) {
    final line =
        '$view flutter=${(_flutterFrames / seconds).toStringAsFixed(1)} flutterFrames=$_flutterFrames '
        'slow=$_slowFrames slowBuild=$_slowBuilds slowRaster=$_slowRasters '
        'longestRaster=${_longestRaster.inMicroseconds} budget=${_budget.inMicroseconds} footprint=${_footprint()} mouse=${RendererBinding.instance.mouseTracker.mouseIsConnected ? 1 : 0}';
    _flutterFrames = 0;
    _slowFrames = _slowBuilds = _slowRasters = 0;
    _longestRaster = Duration.zero;
    _stats.value = line;
    if (_printStats) stdout.writeln('[stats] $line');
  }

  Future<void> _runLifecycleCheck() async {
    const settle = Duration(seconds: 2);
    // Nothing changes after a mark until tool/measure.dart has read the
    // memory (several times, which over adb takes seconds) and says so
    // through `ext.hello.read`. A fixed pause was too short on a phone:
    // the readings fell into the next cycle, with the view hidden. On the
    // Mac and Android the tool reads the memory from outside; on iOS
    // nothing can, so the app reads its own footprint as well and prints
    // the lowest reading as a `[memory]` line.
    Completer<void>? read;
    registerExtension('ext.hello.read', (method, parameters) async {
      read?.complete();
      read = null;
      return ServiceExtensionResponse.result('{}');
    });
    Future<void> mark(String point) async {
      final done = read = Completer<void>();
      stdout.writeln('[check] $point');
      var lowest = footprint();
      for (var i = 0; i < 5; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 500));
        final reading = footprint();
        if (reading.total < lowest.total) lowest = reading;
      }
      stdout.writeln('[memory] $point ${lowest.total} ${lowest.graphics}');
      await done.future;
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
