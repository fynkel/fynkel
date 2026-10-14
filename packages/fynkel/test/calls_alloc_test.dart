// Calls a game makes every frame allocate nothing: finding a component
// below an object (docs/hardening.md D10), counted by the VM service's
// allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

class _Marker extends Behaviour {}

/// Looks for a [_Marker] below it each frame, at the bottom of a tree.
class _Finder extends GameObject {
  var found = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    if (getInChildren<_Marker>() != null) found++;
  }
}

void main() {
  final madeEachFrame = allocations();

  test('a frame of common calls allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final finder = world.add(_Finder());
    var parent = finder as GameObject;
    for (var depth = 0; depth < 6; depth++) {
      for (var i = 0; i < 3; i++) {
        parent.add(GameObject());
      }
      parent = parent.add(GameObject());
    }
    parent.addComponent(_Marker());
    world.update(1 / 60);

    expect(await madeEachFrame(world, 50000), isEmpty);
    expect(finder.found, greaterThan(50000));
  });
}
