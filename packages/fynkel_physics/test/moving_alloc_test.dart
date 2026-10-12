// Following colliders that moved allocates nothing: 1,000 colliders, a
// hundred of them moving each frame, some under a moving parent, and a
// query each frame (docs/stage-6-architecture.md, items 7 and 9), counted
// by the VM service's allocation profile (see allocations.dart).

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

import 'allocations.dart';

final _upward = Ray(Vec3.zero, const Vec3(0, 1, 0));

/// Moves its object between two places, a frame at each.
class _Mover extends Behaviour {
  _Mover(this.here, this.there);

  final Transform here, there;
  var flip = false;

  @override
  void onUpdate() {
    super.onUpdate();
    flip = !flip;
    gameObject.transform = flip ? there : here;
  }
}

/// Asks a query where nothing is, each frame.
class _Asker extends Behaviour {
  var found = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    if (physics.raycast(_upward) != null) found++;
  }
}

void main() {
  final madeEachFrame = allocations();

  test('following a hundred moved colliders among 1,000 allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final parent = world.add(GameObject())
      ..addComponent(_Mover(const Transform(position: Vec3(0, 0, 50)), const Transform(position: Vec3(0, 1, 50))));
    for (var i = 0; i < 1000; i++) {
      final here = Transform(position: Vec3(i * 3.0, 0, 20));
      final object = world.add(GameObject(transform: here, components: [SphereCollider()]));
      if (i % 50 == 0) {
        object.setParent(parent);
      } else if (i % 10 == 0) {
        object.addComponent(_Mover(here, Transform(position: here.position + const Vec3(0, 0, 1))));
      }
    }
    final asker = _Asker();
    world.add(GameObject(components: [asker]));

    // 4,000 frames: the count's own noise, a burst of about 28,000 lists
    // now and then, must not pass for one a frame.
    expect(await madeEachFrame(world, 4000), isEmpty);
    expect(asker.found, 0);
  });
}
