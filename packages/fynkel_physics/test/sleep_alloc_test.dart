// A frame whose bodies all sleep allocates nothing (docs/stage-5.md, step
// 4): the step still runs between frames each fixed step, and is collected
// at the next frame's start, counted as alloc_test.dart counts.

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

import 'allocations.dart';

void main() {
  final madeEachFrame = allocations();

  test('frames whose bodies sleep allocate nothing, stepping between frames', () async {
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(position: const Vec3(0, -0.5, 0)),
        components: [BoxCollider(size: const Vec3(100, 1, 100))],
      ),
    );
    final bodies = [
      for (var i = 0; i < 20; i++)
        world.add(
          GameObject(
            transform: Transform(position: Vec3(i * 2.0, 0.5, 0)),
            components: [
              BoxCollider(),
              Rigidbody(interpolation: RigidbodyInterpolation.interpolate),
            ],
          ),
        ),
    ];

    expect(await madeEachFrame(world, 20000), isEmpty);
    expect(world.physics.stepBetweenFrames, isTrue);
    for (final body in bodies) {
      expect(body.get<Rigidbody>()!.isSleeping, isTrue);
    }
  });
}
