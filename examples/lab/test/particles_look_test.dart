// 'Particles: the look': each of its five effects alone, close up, seeded,
// after two seconds, matches its golden (tests/golden/particles_<name>.png,
// shared with the Rust tests' rule: FYNKEL_BLESS=1 writes them).

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/scenes/particles_look.dart';

import 'golden.dart';

void main() {
  for (final name in particleEffects) {
    test('Particles: the look: the $name matches its golden', () {
      final world = World()..quality = Quality.high;
      addTearDown(world.dispose);
      final scene = world.add(ParticlesLookScene(only: name));
      for (var i = 0; i < 120; i++) {
        world.update(1 / 60);
      }
      expect(scene.systems.keys, [name]);
      expect(scene.systems[name]!.every((s) => s.particleCount > 0), isTrue);
      final view = HeadlessView(world, width: 200, height: 200);
      addTearDown(view.dispose);
      expectGolden('particles_$name', view.read(view.frame()).rgba, 200, 200);
    });
  }
}
