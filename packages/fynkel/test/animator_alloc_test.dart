// A frame where 100 model copies play their clips allocates nothing in
// Dart: the engine poses the parts, and Dart only forgets the transforms
// it kept (docs/stage-9.md §2, no hidden cost). Counted by the VM
// service's allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';
import 'model_test.dart' show glb;

void main() {
  final madeEachFrame = allocations();

  test('a frame where 100 copies play their clips allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final truck = await Model.load(glb('CesiumMilkTruck.glb'));
    addTearDown(truck.dispose);
    for (var i = 0; i < 100; i++) {
      world.add(truck.instantiate()).animator!.play('Wheels');
    }

    expect(await madeEachFrame(world, 2000), isEmpty);
  });
}
