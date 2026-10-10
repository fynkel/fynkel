// A frame without input allocates nothing (docs/stage-4.md §3.6, step 5),
// counted by the VM service's allocation profile (see allocations.dart).

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

class _Still extends GameObject {
  _Still() : super(components: [MeshRenderer(Mesh.cube(), Material())]);

  @override
  void onUpdate() {
    super.onUpdate();
  }
}

class _Idle extends Behaviour {}

void main() {
  final madeEachFrame = allocations();

  test('a frame without input allocates nothing, with the mouse resting over the view', () async {
    final world = World();
    addTearDown(world.dispose);
    world.input.resize(100, 100);
    world.add(GameObject(components: [Camera()]));
    for (var i = 0; i < 50; i++) {
      world.add(_Still()..addComponent(_Idle()));
    }
    world.input.hover(-1, 50, 50);
    world.input.onFingerHeld.listen((_) {});
    world.input.onHover.listen((_) {});

    expect(await madeEachFrame(world, 50000), isEmpty);
  });
}
