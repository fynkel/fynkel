// Tumble for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart. Its timings are stage 5 step 1's (docs/stage-5.md): a
// fixed step of 1,000 cubes, falling and then resting, by itself and with
// the poses written to the objects and sent to the engine.

import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_measure/fynkel_measure.dart';
import 'package:tumble/main.dart';
import 'package:tumble/rigid.dart';

void main() => runApp(MeasuredApp(rigidbodies ? RigidTumble.new : Tumble.new, timings: (_) => timings()));

List<String> timings() {
  final lines = <String>[];
  final world = World();
  final tumble = world.add(Tumble(count: 1000));
  final first = tumble.cubes.first;
  String ms(int us) => '${(us / 1000).toStringAsFixed(2)} ms';
  void time(String name, int steps, void Function() body) {
    final times = <int>[];
    final clock = Stopwatch();
    for (var i = 0; i < steps; i++) {
      clock
        ..reset()
        ..start();
      body();
      times.add(clock.elapsedMicroseconds);
    }
    times.sort();
    lines.add('$name: median ${ms(times[times.length ~/ 2])}, slowest ${ms(times.last)}');
  }

  // A whole drop, as the scene runs it: 8 s of falling, tumbling and
  // settling, in 400 fixed steps, then the same split by the second.
  tumble.drop();
  time('step, 1,000 cubes, a whole 8 s drop', 400, () => tumble.physics.step());
  tumble.drop();
  final perStep = <int>[];
  final clock = Stopwatch();
  for (var i = 0; i < 400; i++) {
    clock
      ..reset()
      ..start();
    stepAndWrite(tumble.physics, tumble.cubes);
    first.worldTransform; // sends the moves to the engine, as a frame would
    perStep.add(clock.elapsedMicroseconds);
  }
  final sorted = [...perStep]..sort();
  lines.add(
    'step and poses, 1,000 cubes, a whole 8 s drop: median ${ms(sorted[200])}, '
    '90th ${ms(sorted[360])}, slowest ${ms(sorted.last)}',
  );
  lines.add(
    'step and poses by second: ${[for (var s = 0; s < 8; s++) ms(perStep.skip(s * 50).take(50).reduce((a, b) => a + b) ~/ 50)].join(', ')}',
  );
  // Resting: once they've settled.
  for (var i = 0; i < 1000; i++) {
    tumble.physics.step();
  }
  time('step and poses, 1,000 cubes resting', 150, () {
    stepAndWrite(tumble.physics, tumble.cubes);
    first.worldTransform;
  });
  world.dispose();
  return lines;
}
