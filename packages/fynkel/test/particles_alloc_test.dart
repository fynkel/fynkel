// A frame where particle systems play and burst allocates nothing in
// Dart: the settings went to the engine when they were set, and a frame
// is one call with its times in native memory (docs/stage-12.md phase 1).
// Counted by the VM service's allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

void main() {
  final madeEachFrame = allocations();

  test('a frame where 20 systems play and burst allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final glow = Material(color: Color(0xFFFFAA33), blending: Blending.additive);
    for (var i = 0; i < 20; i++) {
      world.add(
        GameObject(
          transform: Transform(position: Vec3(i.toDouble(), 0, 0)),
          components: [
            ParticleSystem(
              duration: 0.5,
              startLifetime: Range(0.4, 0.8),
              startColor: ColorRange(Color(0xFFFFFFFF), Color(0xFFFF8800)),
              gravityModifier: 0.5,
              simulationSpace: i.isEven ? ParticleSpace.world : ParticleSpace.local,
              emission: Emission(rateOverTime: 50, bursts: [Burst(count: Range(10, 20))]),
              sizeOverLifetime: (t) => 1 - t,
              colorOverLifetime: ColorGradient.fadeOut(),
              material: glow,
            ),
          ],
        ),
      );
    }

    expect(await madeEachFrame(world, 2000), isEmpty);
  });
}
