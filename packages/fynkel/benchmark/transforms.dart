// Stage 1 plan, step 4: how long setting 10,000 transforms through direct
// FFI calls takes in one frame. Decision rule: under 1 ms on the Mac keeps
// direct calls; over that, a batched path gets designed.
//
//   dart run benchmark/transforms.dart          (JIT)
//   dart build cli -t benchmark/transforms.dart (AOT, as a release app runs)

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

const objects = 10000;
const frames = 300;

void main() {
  final world = World();
  final cubes = [for (var i = 0; i < objects; i++) world.spawn(Transform())];
  final positions = [for (var i = 0; i < objects; i++) Vec3(i % 100 * 1.0, 0, i ~/ 100 * 1.0)];

  void measure(String name, void Function(int frame) body) {
    for (var f = 0; f < 50; f++) {
      body(f); // warm up (and let the JIT optimise)
    }
    final times = <double>[];
    final watch = Stopwatch();
    for (var f = 0; f < frames; f++) {
      watch
        ..reset()
        ..start();
      body(f);
      watch.stop();
      times.add(watch.elapsedMicroseconds / 1000);
    }
    times.sort();
    String ms(double q) => times[((times.length - 1) * q).round()].toStringAsFixed(3);
    print('$name: median ${ms(0.5)} ms, p95 ${ms(0.95)} ms, worst ${ms(1)} ms per frame of $objects');
  }

  measure('set transform', (f) {
    final rotation = Quat.axisAngle(Vec3.up, f * 0.01);
    for (var i = 0; i < objects; i++) {
      cubes[i].transform = Transform(position: positions[i], rotation: rotation);
    }
  });
  measure('get transform', (f) {
    var sum = 0.0;
    for (var i = 0; i < objects; i++) {
      sum += cubes[i].transform.position.x;
    }
    if (sum < 0) print(sum);
  });
  measure('rotate (get + set)', (f) {
    for (var i = 0; i < objects; i++) {
      cubes[i].rotate(Vec3.up, math.pi / 360);
    }
  });
  world.dispose();
}
