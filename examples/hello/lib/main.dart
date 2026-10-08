import 'dart:async';

import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

import 'game.dart';

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
  final _measurement = Measurement();

  // The lifecycle check: whether the view is there, and its size.
  bool _shown = true;
  double _scale = 1;

  @override
  void initState() {
    super.initState();
    if (MeasureSettings.lifecycleCheck) unawaited(_runLifecycleCheck());
  }

  @override
  void dispose() {
    _measurement.dispose();
    _game.world.dispose();
    super.dispose();
  }

  /// The view created and destroyed, then resized between the full window
  /// and 0.6 of it, with a mark after each.
  Future<void> _runLifecycleCheck() async {
    const settle = Duration(seconds: 2);
    final cycles = MeasureSettings.cycles;
    await Future<void>.delayed(const Duration(seconds: 5));
    await Measurement.mark('start');
    for (var i = 1; i <= cycles; i++) {
      setState(() => _shown = false);
      await Future<void>.delayed(settle);
      setState(() => _shown = true);
      await Future<void>.delayed(settle);
      await Measurement.mark('created $i');
    }
    for (var i = 1; i <= cycles; i++) {
      setState(() => _scale = i.isOdd ? 0.6 : 1);
      await Future<void>.delayed(settle);
      await Measurement.mark(i.isOdd ? 'resized to 0.6 ${(i + 1) ~/ 2}' : 'resized to full ${i ~/ 2}');
    }
    await Measurement.mark('done');
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
              child: MeasureSettings.baseline
                  ? const BaselineSpinner()
                  : FynkelView(
                      world: _game.world,
                      onFrame: (dt, _) => _game.onFrame(dt),
                      onStats: _measurement.onStats,
                    ),
            ),
          ),
        StatsLine(_measurement),
      ],
    );
  }
}
