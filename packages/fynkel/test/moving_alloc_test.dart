// A frame where 1,000 objects move allocates only their new transforms:
// the per-frame lists keep their storage (docs/stage-6-architecture.md,
// item 9), counted by the VM service's allocation profile (see
// allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

/// Moves its object each frame, on and off.
class _Mover extends Behaviour {
  var flip = false;

  @override
  void onUpdate() {
    super.onUpdate();
    flip = !flip;
    gameObject.transform = flip ? _there : _here;
  }
}

const _here = Transform();
const _there = Transform(position: Vec3(1, 2, 3));

void main() {
  final madeEachFrame = allocations();

  test('a frame where 1,000 objects move allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final parent = world.add(GameObject());
    for (var i = 0; i < 1000; i++) {
      final object = world.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material())])..addComponent(_Mover()));
      if (i % 10 == 0) object.setParent(parent);
    }

    expect(await madeEachFrame(world, 2000), isEmpty);
  });
}
