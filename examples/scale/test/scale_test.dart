import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:scale/main.dart';

void main() {
  test('10,000 objects of 50 kinds run, and their frames are timed and held to a bar', () {
    final world = World();
    addTearDown(world.dispose);
    final scale = world.add(Scale(objects: 10000));
    world.update(1 / 60);
    expect(scale.groups, 1000);
    expect(world.all<Part>().length, 9000);
    expect({for (final g in world.all<Group>()) g.kind.index}.length, kinds);

    // Past the first beats and blinks, then timed.
    for (var i = 0; i < 180; i++) {
      world.update(1 / 60);
    }
    final clock = Stopwatch();
    final times = <int>[];
    for (var i = 0; i < 600; i++) {
      clock
        ..reset()
        ..start();
      world.update(1 / 60);
      times.add(clock.elapsedMicroseconds);
    }
    times.sort();
    // ignore: avoid_print
    print('world.update, 10,000 objects: median ${times[300]} µs, 95th ${times[570]} µs, max ${times.last} µs');
    // Held to a bar (stage 14's GT1): the Mac's median when it was set,
    // 0.5 ms, doubled, as the crates' benchmarks are (fynkel_bench).
    expect(times[300], lessThan(1000), reason: 'world.update for 10,000 objects, median µs');
  });

  test('churn destroys and makes groups, and the count holds', () {
    final world = World();
    addTearDown(world.dispose);
    final scale = world.add(Scale(objects: 1000, churn: 60));
    for (var i = 0; i < 120; i++) {
      world.update(1 / 60);
    }
    expect(scale.replaced, greaterThanOrEqualTo(115));
    expect(world.all<Group>().length, 100);
    expect(world.all<Part>().length, 900);
  });
}
