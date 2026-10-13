// The example runs (stage 6, PR 13): both piles, the raw world's and the
// Rigidbodies', fall, land on the ground without going through it, and
// are dropped again after 8 seconds, headless as CI runs them.

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:tumble/main.dart';
import 'package:tumble/rigid.dart';

/// The cubes' mean height, and the lowest and furthest out.
(double mean, double lowest, double furthest) heights(List<GameObject> cubes) {
  var sum = 0.0;
  var lowest = double.infinity;
  var furthest = 0.0;
  for (final cube in cubes) {
    final p = cube.position;
    sum += p.y;
    if (p.y < lowest) lowest = p.y;
    final out = p.x.abs() > p.z.abs() ? p.x.abs() : p.z.abs();
    if (out > furthest) furthest = out;
  }
  return (sum / cubes.length, lowest, furthest);
}

void main() {
  for (final (name, make) in [
    ('the raw world', () => Tumble(count: 100)),
    ('Rigidbodies', () => RigidTumble(count: 100)),
  ]) {
    test('$name: the pile falls, lands on the ground and is dropped again', () {
      final world = World();
      addTearDown(world.dispose);
      final tumble = world.add(make());
      final cubes = switch (tumble) {
        Tumble(:final cubes) || RigidTumble(:final cubes) => cubes,
        _ => throw StateError('$tumble'),
      };
      world.update(1 / 60);
      final (dropped, _, _) = heights(cubes);
      expect(dropped, greaterThan(4));
      // Seven seconds: down, and resting on the ground, inside its edges.
      for (var i = 0; i < 7 * 60; i++) {
        world.update(1 / 60);
      }
      final (landed, lowest, furthest) = heights(cubes);
      expect(landed, lessThan(1.5));
      expect(lowest, greaterThan(0.2), reason: 'none went through the ground');
      expect(furthest, lessThan(20), reason: 'none fell off its edge');
      // Past eight seconds: up in the air again.
      for (var i = 0; i < 70; i++) {
        world.update(1 / 60);
      }
      expect(heights(cubes).$1, greaterThan(3));
    });
  }
}
