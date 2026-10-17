// The lab's crowd (stage 9, phase 4) for package:fynkel_measure's tool,
// which runs this in place of lib/main.dart. Its settings are
// `--define`s:
//
// - `CROWD`, how many characters (100);
// - `QUALITY=low|medium|high`, the quality drawn, the device's pick
//   otherwise;
// - `ANIMATED=false`, the crowd holding still, to compare with.
//
// Every 10 seconds it reports the animation's time a frame (the median of
// the last second), and its bench times world.update with the crowd.
//
//   caffeinate -du dart run fynkel_measure:measure rate --seconds 60 --define CROWD=100
//
// The tool's own `--define FYNKEL_FRAME_RATE=120` asks for 120 fps.

import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_measure/fynkel_measure.dart';
import 'package:lab/scenes/crowd.dart';

const _crowd = int.fromEnvironment('CROWD', defaultValue: 100);
const _quality = String.fromEnvironment('QUALITY');
const _animated = bool.fromEnvironment('ANIMATED', defaultValue: true);

void main() => runApp(
  MeasuredApp(() {
    final scene = CrowdScene(count: _crowd, animated: _animated);
    scene.loaded.then((_) {
      scene.world.quality = Quality.levels.where((q) => q.name == _quality).firstOrNull;
      Timer.periodic(const Duration(seconds: 10), (timer) {
        if (scene.isDestroyed) return timer.cancel();
        Measurement.report(
          '${scene.people.length} characters, animation ${scene.animationMs.toStringAsFixed(2)} ms a frame',
        );
      });
    });
    return scene;
  }, timings: (root) => [_update(root.world)]),
);

String _update(World world) {
  final clock = Stopwatch();
  final times = <int>[];
  for (var i = 0; i < 240; i++) {
    clock
      ..reset()
      ..start();
    world.update(1 / 60);
    times.add(clock.elapsedMicroseconds);
  }
  times.sort();
  return 'crowd world.update: median ${times[120]} µs, 95th ${times[228]} µs, max ${times.last} µs';
}
