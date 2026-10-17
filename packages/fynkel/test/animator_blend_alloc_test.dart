// A frame where 100 model copies change their minds allocates nothing in
// Dart: each blends by a number that moves, and fades a layer in and out
// (docs/stage-9.md phase 3). Counted by the VM service's allocation
// profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';
import 'model_test.dart' show glb;

const _clips = ['Wheels', 'Wheels'];
const _thresholds = [0.0, 1.0];

/// The number the blend goes round, made once: a double worked out in the
/// game's own update is the game's to box.
final _ats = List<double>.generate(30, (i) => i / 30);

/// A truck whose game blends its wheels by a number going round, and
/// cheers on a layer every other few frames.
final class Restless extends Behaviour {
  var _frame = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    final animator = gameObject.animator!;
    _frame++;
    animator.blend('roll', _clips, thresholds: _thresholds, at: _ats[_frame % 30], duration: 0.2);
    final layer = animator.layer(1);
    switch (_frame % 3) {
      case 0:
        layer.crossFade('Wheels', duration: 0.2);
      case 1:
        layer.stop(duration: 0.2);
    }
  }
}

void main() {
  final madeEachFrame = allocations();

  test('a frame where 100 copies blend and fade layers each frame allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final truck = await Model.load(glb('CesiumMilkTruck.glb'));
    addTearDown(truck.dispose);
    for (var i = 0; i < 100; i++) {
      final copy = world.add(truck.instantiate());
      copy.animator!.layer(1, mask: 'Wheels', strength: 0.5);
      copy.addComponent(Restless());
    }

    expect(await madeEachFrame(world, 2000), isEmpty);
  });
}
