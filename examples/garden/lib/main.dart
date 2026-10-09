import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' show Input;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

import 'garden.dart';

void main() => runApp(const GardenApp());

class GardenApp extends StatelessWidget {
  const GardenApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
    debugShowCheckedModeBanner: false,
    theme: ThemeData.dark(),
    home: const Scaffold(body: GardenPage()),
  );
}

class GardenPage extends StatefulWidget {
  const GardenPage({super.key});

  @override
  State<GardenPage> createState() => _GardenPageState();
}

class _GardenPageState extends State<GardenPage> {
  final _garden = Garden((asset) async => (await rootBundle.load('assets/$asset')).buffer.asUint8List());
  final _measurement = Measurement();
  final _onScreen = OnScreenClock();

  /// What the loads took, shown under the stats line.
  final _loads = ValueNotifier('loading…');

  @override
  void initState() {
    super.initState();
    // The baseline loads nothing: it's Flutter alone.
    if (!MeasureSettings.baseline) unawaited(_load());
    if (MeasureSettings.lifecycleCheck) unawaited(_runLifecycleCheck());
    if (MeasureSettings.benchCheck) unawaited(_runBench());
  }

  @override
  void dispose() {
    _measurement.dispose();
    _loads.dispose();
    _garden.world.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final clock = Stopwatch()..start();
    final times = await _garden.load();
    final spawned = clock.elapsed;
    final shown = await _onScreen.next(clock);
    for (final t in times) {
      Measurement.report('load ${_line(t)}');
    }
    Measurement.report('load scene: all loaded and spawned ${_ms(spawned)}, on screen ${_ms(shown)}');
    _loads.value = 'scene ${_ms(shown)} to the screen; character ${_line(times.last)}';
  }

  /// The character unloaded and loaded again, [MeasureSettings.cycles]
  /// times, with a mark after each: memory after each unload should match,
  /// and after each load.
  Future<void> _runLifecycleCheck() async {
    const settle = Duration(seconds: 2);
    // The first load and its upload are over by then.
    await Future<void>.delayed(const Duration(seconds: 8));
    await Measurement.mark('start');
    for (var i = 1; i <= MeasureSettings.cycles; i++) {
      if (!MeasureSettings.baseline) _garden.unloadCharacter();
      await Future<void>.delayed(settle);
      await Measurement.mark('unloaded $i');
      if (!MeasureSettings.baseline) {
        final clock = Stopwatch()..start();
        final time = await _garden.loadCharacter();
        final shown = await _onScreen.next(clock);
        Measurement.report('load ${_line(time)}, on screen ${_ms(shown)}');
      }
      await Future<void>.delayed(settle);
      await Measurement.mark('loaded $i');
    }
    await Measurement.mark('done');
  }

  /// The timings steps 1 and 5 left for the phones (package:fynkel_measure's bench), and
  /// the garden's own tap cast, once the scene has loaded and settled.
  Future<void> _runBench() async {
    await Future<void>.delayed(const Duration(seconds: 8));
    for (final line in bench()) {
      Measurement.report(line);
    }
    Measurement.report('raycast, the garden, at the character: ${castTimes(_garden.world, _garden.centreRay)}');
    await Measurement.mark('done');
  }

  // The view makes its engine view, and with it a GPU device, on the UI
  // isolate in its first tick that has a size: the gap from that tick to
  // the next is how long that held the UI isolate, and with it anything
  // else waiting there, such as a load's last steps.
  final _startClock = Stopwatch()..start();
  Duration? _firstTick;
  var _ticks = 0;

  void _onFrame(double dt, Input input) {
    _ticks++;
    if (_ticks == 1) _firstTick = _startClock.elapsed;
    if (_ticks == 2) {
      Measurement.report(
        'view: first tick ${_ms(_firstTick!)} after start, the next ${_ms(_startClock.elapsed - _firstTick!)} later',
      );
    }
    _garden.onFrame(dt, input);
  }

  static String _ms(Duration d) => '${(d.inMicroseconds / 1000).toStringAsFixed(1)} ms';

  static String _line(LoadTime t) => '${t.asset}: read ${_ms(t.read)}, load ${_ms(t.load)}, spawn ${_ms(t.spawn)}';

  @override
  Widget build(BuildContext context) {
    return Stack(
      children: [
        Positioned.fill(
          child: MeasureSettings.baseline
              ? const Center(child: SizedBox.square(dimension: 200, child: BaselineSpinner()))
              : FynkelView(
                  world: _garden.world,
                  onFrame: _onFrame,
                  onStats: _measurement.onStats,
                  onViewStats: _onScreen.onViewStats,
                ),
        ),
        StatsLine(_measurement),
        Positioned(
          left: 12,
          right: 12,
          bottom: 12,
          child: SafeArea(
            child: ValueListenableBuilder(
              valueListenable: _loads,
              builder: (context, text, _) => Text(
                text,
                style: const TextStyle(fontFamily: 'Menlo', fontSize: 11, color: Colors.black87),
              ),
            ),
          ),
        ),
      ],
    );
  }
}
