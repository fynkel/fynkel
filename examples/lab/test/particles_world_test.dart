// 'Particles and the world': each of its three effects alone, close up,
// seeded, after two seconds, matches its golden
// (tests/golden/particles_world_<name>.png; FYNKEL_BLESS=1 writes them).

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/scenes/particles_world.dart';

import 'golden.dart';

void main() {
  for (final name in worldEffects) {
    test('Particles and the world: the $name matches its golden', () {
      final world = World()..quality = Quality.high;
      addTearDown(world.dispose);
      final scene = world.add(ParticlesWorldScene(only: name));
      for (var i = 0; i < 120; i++) {
        world.update(1 / 60);
      }
      expect(scene.systems.keys, [name]);
      expect(scene.systems[name]!.every((s) => s.particleCount > 0), isTrue);
      final view = HeadlessView(world, width: 200, height: 200);
      addTearDown(view.dispose);
      expectGolden('particles_world_$name', view.read(view.frame()).rgba, 200, 200);
    });
  }
}
