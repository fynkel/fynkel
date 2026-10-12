// A frame without input allocates nothing (docs/stage-4.md §3.6, step 5),
// counted by the VM service's allocation profile (see allocations.dart);
// with its fixed steps and a world system (docs/stage-5.md, step 2).

import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

class _Still extends GameObject {
  _Still() : super(components: [MeshRenderer(Mesh.cube(), Material())]);

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
  }

  @override
  void onUpdate() {
    super.onUpdate();
  }
}

class _Idle extends Behaviour {
  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
  }
}

class _System extends WorldSystem {
  _System(super.world);

  var steps = 0;
  var frames = 0;

  @override
  void afterFixedUpdate() {
    super.afterFixedUpdate();
    steps++;
  }

  @override
  void afterLateUpdate() {
    super.afterLateUpdate();
    frames++;
  }
}

void main() {
  final madeEachFrame = allocations();

  test('a frame without input allocates nothing, with the mouse resting over the view', () async {
    final world = World();
    addTearDown(world.dispose);
    world.input.simulate.resize(100, 100);
    world.add(GameObject(components: [Camera()]));
    for (var i = 0; i < 50; i++) {
      world.add(_Still()..addComponent(_Idle()));
    }
    world.input.simulate.hover(-1, 50, 50);
    world.input.onFingerHeld.listen((_) {});
    world.input.onHover.listen((_) {});
    final system = world.system(_System.new);

    expect(await madeEachFrame(world, 50000), isEmpty);
    expect(system.steps, greaterThan(40000), reason: 'the frames ran fixed steps');
    expect(system.frames, greaterThanOrEqualTo(50000));
  });
}
