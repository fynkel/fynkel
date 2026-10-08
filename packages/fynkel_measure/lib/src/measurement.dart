import 'dart:async';
import 'dart:convert';
import 'dart:developer';
import 'dart:io';
import 'dart:isolate';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'footprint.dart';

/// What bin/measure.dart asked the app for. Set by the tool, never by
/// hand: as environment variables on the Mac, and as `--dart-define`s on a
/// phone, where an app gets no environment.
abstract final class MeasureSettings {
  static String _setting(String name, String defined) => Platform.environment[name] ?? defined;

  /// Print a `[stats]` line every second.
  static final printStats = _setting('FYNKEL_STATS', const String.fromEnvironment('FYNKEL_STATS')) == '1';

  /// Run the memory check, printing a `[check]` line at each point where
  /// memory should be back where it started.
  static final lifecycleCheck = _check == 'lifecycle';

  /// Run the app's timings, report each, then mark `done`.
  static final benchCheck = _check == 'bench';

  static final _check = _setting('FYNKEL_CHECK', const String.fromEnvironment('FYNKEL_CHECK'));

  /// How many times the check repeats each thing it does.
  static final cycles = int.tryParse(_setting('FYNKEL_CYCLES', const String.fromEnvironment('FYNKEL_CYCLES'))) ?? 10;

  /// For comparison: a spinning Flutter box ([BaselineSpinner]) where the
  /// engine's view would be, so the same checks show what Flutter alone
  /// does.
  static final baseline = _setting('FYNKEL_BASELINE', const String.fromEnvironment('FYNKEL_BASELINE')) == '1';
}

/// The app's side of a measurement: the stats line, once a second, and
/// the memory marks of the lifecycle check. Make one in a `State`'s
/// `initState`, pass [onStats] to the `FynkelView`, show [line], and
/// [dispose] it with the state.
class Measurement {
  Measurement() {
    _registerExtensions();
    SchedulerBinding.instance.addTimingsCallback(_onTimings);
    _footprint();
    // The baseline has no view to report, but its stats line changes every
    // second all the same: laying out new text costs Flutter memory, and
    // the comparison must not miss that.
    if (MeasureSettings.baseline) {
      _baselineStats = Timer.periodic(const Duration(seconds: 1), (_) => _onSecond('baseline=1'));
    }
  }

  /// The latest stats line.
  final line = ValueNotifier('');

  Timer? _baselineStats;

  // Flutter's own frames in the current stats window: how many, how many
  // had a build or raster phase longer than one frame of the display
  // (8.33 ms at 120 Hz, 16.7 ms at 60 Hz), which of the two it was, and
  // the longest raster phase (a slow frame's cause is usually there). The
  // line also says whether a mouse is over the view: while one is, Flutter
  // hit-tests its position after every frame, which roughly doubles the
  // Dart allocation per frame (docs/stage-1.md §7).
  int _flutterFrames = 0, _slowFrames = 0, _slowBuilds = 0, _slowRasters = 0;
  Duration _longestRaster = Duration.zero;

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

  void dispose() {
    _baselineStats?.cancel();
    SchedulerBinding.instance.removeTimingsCallback(_onTimings);
    line.dispose();
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

  /// For `FynkelView.onStats`.
  void onStats(FynkelViewStats stats) => _onSecond('$stats', seconds: stats.window.inMicroseconds / 1e6);

  void _onSecond(String view, {double seconds = 1}) {
    final text =
        '$view flutter=${(_flutterFrames / seconds).toStringAsFixed(1)} flutterFrames=$_flutterFrames '
        'slow=$_slowFrames slowBuild=$_slowBuilds slowRaster=$_slowRasters '
        'longestRaster=${_longestRaster.inMicroseconds} budget=${_budget.inMicroseconds} footprint=${_footprint()} mouse=${RendererBinding.instance.mouseTracker.mouseIsConnected ? 1 : 0}';
    _flutterFrames = 0;
    _slowFrames = _slowBuilds = _slowRasters = 0;
    _longestRaster = Duration.zero;
    line.value = text;
    if (MeasureSettings.printStats) stdout.writeln('[stats] $text');
  }

  /// A line for the tool to pass on as it is, such as a load time. Kept
  /// for the tool, which asks for them all at the end (`ext.fynkel.reports`):
  /// on a phone it reads the app's output only once it's connected, so
  /// lines printed at startup would be lost. Printed as well.
  static void report(String text) {
    _registerExtensions();
    _reports.add(text);
    stdout.writeln('[report] $text');
  }

  static final _reports = <String>[];
  static Completer<void>? _read;
  static var _registered = false;

  static void _registerExtensions() {
    if (_registered) return;
    _registered = true;
    registerExtension('ext.fynkel.read', (method, parameters) async {
      _read?.complete();
      _read = null;
      return ServiceExtensionResponse.result('{}');
    });
    registerExtension(
      'ext.fynkel.reports',
      (method, parameters) async => ServiceExtensionResponse.result(jsonEncode({'reports': _reports})),
    );
  }

  /// A point where memory should be back where it was at the other points
  /// of its kind (`created 3` and `created 4` are of a kind: the name
  /// without its number). Prints `[check] <point>` and returns once the
  /// tool has read the memory.
  ///
  /// Nothing should change after a mark until the tool has read the memory
  /// (several times, which over adb takes seconds) and says so through
  /// `ext.fynkel.read`. A fixed pause was too short on a phone: the
  /// readings fell into the next cycle. On the Mac and Android the tool
  /// reads the memory from outside; on iOS nothing can, so the app reads
  /// its own footprint as well and prints the lowest reading as a
  /// `[memory]` line.
  static Future<void> mark(String point) async {
    _registerExtensions();
    final done = _read = Completer<void>();
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
}

/// The stats line, top left, clear of the iPhone's status bar and Dynamic
/// Island, and wrapped to the screen's width. For a `Stack` over the view.
class StatsLine extends StatelessWidget {
  const StatsLine(this.measurement, {super.key});

  final Measurement measurement;

  @override
  Widget build(BuildContext context) => Positioned(
    left: 12,
    right: 12,
    top: 12,
    child: SafeArea(
      child: RepaintBoundary(
        child: ValueListenableBuilder(
          valueListenable: measurement.line,
          builder: (context, line, _) => Text(
            line,
            style: const TextStyle(fontFamily: 'Menlo', fontSize: 11, color: Colors.white70),
          ),
        ),
      ),
    ),
  );
}

/// A box turning once every 4 seconds: a new Flutter frame every vsync, as
/// with the engine's view, but nothing of the engine's. The baseline
/// ([MeasureSettings.baseline]) shows it in place of the view.
class BaselineSpinner extends StatefulWidget {
  const BaselineSpinner({super.key});

  @override
  State<BaselineSpinner> createState() => _BaselineSpinnerState();
}

class _BaselineSpinnerState extends State<BaselineSpinner> with SingleTickerProviderStateMixin {
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
