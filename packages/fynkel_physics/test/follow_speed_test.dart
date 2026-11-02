// Stage 14's PH6: following the objects that moved walks what's below each
// once. A 300-deep chain whose every link moved was walked from each link
// down, 300 times over: 158 ms on the Mac, against 0.8 ms when only its
// top link moved (which moves the rest with it).

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

/// The time a query takes to follow [links] after each of them moved,
/// the best of several rounds, in microseconds.
double follow(World world, List<GameObject> links, {bool all = true}) {
  var best = double.infinity;
  final watch = Stopwatch();
  for (var round = 0; round < 30; round++) {
    for (var i = 0; i < (all ? links.length : 1); i++) {
      links[i].localPosition = Vec3(round.isEven ? 0.001 : 0, 0.01, 0);
    }
    watch
      ..reset()
      ..start();
    world.physics.overlapSphere(const Vec3(0, 1000, 0), 0.1);
    watch.stop();
    if (round > 4 && watch.elapsedMicroseconds < best) best = watch.elapsedMicroseconds.toDouble();
  }
  return best;
}

World withStatics() {
  final world = World();
  addTearDown(world.dispose);
  for (var i = 0; i < 2000; i++) {
    world.add(
      GameObject(
        transform: Transform(position: Vec3(i * 3.0, -50, 0)),
        components: [BoxCollider()],
      ),
    );
  }
  return world;
}

void main() {
  test('a chain whose every link moved is followed as fast as one moved by its top', () {
    final world = withStatics();
    final chain = <GameObject>[];
    GameObject? parent;
    for (var i = 0; i < 300; i++) {
      final link = GameObject(components: [BoxCollider()]);
      parent == null ? world.add(link) : parent.add(link);
      chain.add(link);
      parent = link;
    }
    final top = follow(world, chain, all: false);
    final every = follow(world, chain);
    expect(every, lessThan(top * 2 + 50), reason: 'every link moved $every µs, the top $top µs');
  });
}
