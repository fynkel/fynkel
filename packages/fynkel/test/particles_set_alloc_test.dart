// A frame that changes a particle system's settings and a trail's, as an
// engine's exhaust follows its speed, allocates nothing in Dart but the
// numbers it boxes (stage 14's DF2: each set made about 9 objects).
// Counted by the VM service's allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

void main() {
  final madeEachFrame = allocations();

  test('a frame that changes a system\'s settings and a trail\'s allocates nothing but boxed numbers', () async {
    final world = World();
    addTearDown(world.dispose);
    world.add(Exhaust());
    final made = await madeEachFrame(world, 2000);
    made.remove('_Double');
    expect(made, isEmpty);
  });
}

/// Smoke and a trail that follow a speed going up and down.
class Exhaust extends GameObject {
  final smoke = ParticleSystem(
    startLifetime: const Range(0.5, 1),
    sizeOverLifetime: (t) => 1 - t,
    colorOverLifetime: ColorGradient.fadeOut(),
  );
  final trail = TrailRenderer(time: 0.5);
  static const _rates = [Emission(rateOverTime: 5), Emission(rateOverTime: 40)];
  static const _speeds = [Range(1, 2), Range(3, 5)];
  static const _colors = [Color(0xFF888888), Color(0xFF333333)];
  var _speed = 0.0;
  var _frame = 0;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(smoke);
    addComponent(trail);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _frame++;
    _speed = (_speed + 0.01) % 1;
    final fast = _frame.isEven ? 1 : 0;
    smoke
      ..emission = _rates[fast]
      ..startSpeed = _speeds[fast]
      ..startColor = _colors[fast]
      ..simulationSpeed = 1 + _speed
      ..gravityModifier = _speed;
    trail
      ..widthMultiplier = 1 + _speed
      ..time = 0.5 + _speed
      ..emitting = _frame % 100 != 0;
  }
}
