// A query that misses allocates nothing (docs/stage-5.md, step 3), nor
// does following colliders when something changed, counted by the VM
// service's allocation profile (see allocations.dart).

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

import 'allocations.dart';

const _up = Vec3(0, 1, 0);
final _upward = Ray(Vec3.zero, _up);
final _ahead = Ray(Vec3.zero, const Vec3(0, 0, 1));

/// Asks one query where nothing is, each frame. One query each, as a game
/// would: a function asking all of them is too big for the JIT to compile
/// without boxing (AOT, on the devices, has no such limit).
class _Asker extends Behaviour {
  _Asker(this.ask);

  final bool Function(Physics physics) ask;
  var asked = 0;
  var found = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    if (ask(physics)) found++;
    asked++;
  }
}

/// Sets its object's layer each frame, so the colliders are followed
/// before the first query (moving an object would allocate a transform).
class _Changer extends Behaviour {
  var flip = false;

  @override
  void onUpdate() {
    super.onUpdate();
    flip = !flip;
    gameObject.layer = flip ? 1 : 2;
  }
}

void main() {
  final madeEachFrame = allocations();

  test('queries that miss allocate nothing, after a layer was set', () async {
    final world = World();
    addTearDown(world.dispose);
    for (var i = 0; i < 20; i++) {
      world.add(
        GameObject(
          transform: Transform(position: Vec3(i * 3.0, 0, 20)),
          components: [
            switch (i % 3) {
              0 => BoxCollider(),
              1 => SphereCollider(),
              _ => CapsuleCollider(),
            },
          ],
        ),
      );
    }
    final askers = [
      _Asker((p) => p.raycast(_upward) != null),
      _Asker((p) => p.raycastAll(_upward).isNotEmpty),
      _Asker((p) => p.sphereCast(_upward, 0.5) != null),
      _Asker((p) => p.boxCast(Vec3.zero, const Vec3(0.5, 0.5, 0.5), _up) != null),
      _Asker((p) => p.capsuleCastAll(Vec3.zero, const Vec3(0, 0, 1), 0.5, _up).isNotEmpty),
      _Asker((p) => p.overlapSphere(const Vec3(0, 50, 0), 1).isNotEmpty),
      _Asker((p) => p.checkBox(const Vec3(0, 50, 0), const Vec3(1, 1, 1))),
      _Asker((p) => p.raycast(_ahead, layers: LayerMask.none) != null),
    ];
    world.add(GameObject(components: [_Changer(), ...askers]));

    expect(await madeEachFrame(world, 20000), isEmpty);
    for (final asker in askers) {
      expect(asker.asked, greaterThan(100000));
      expect(asker.found, 0);
    }
  });
}
