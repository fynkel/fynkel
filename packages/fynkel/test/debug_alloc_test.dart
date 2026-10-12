// A frame that draws debug lines allocates nothing once its buffer has
// grown (docs/stage-4.md §3.9, step 8), counted as in alloc_test.dart.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

const _red = Color.rgb(1, 0, 0);

class _Drawing extends GameObject {
  _Drawing(this.at);

  final Vec3 at;

  @override
  void onUpdate() {
    super.onUpdate();
    debug
      ..line(at, Vec3.zero, color: _red)
      ..box(at, Vec3.one)
      ..sphere(at, 0.5);
  }
}

void main() {
  final madeEachFrame = allocations();

  test('a frame drawing lines, boxes and spheres allocates nothing but their points', () async {
    final world = World();
    addTearDown(world.dispose);
    // As in alloc_test.dart, which allocates nothing.
    world.input.simulate.resize(100, 100);
    world.add(GameObject(components: [Camera()]));
    for (var i = 0; i < 10; i++) {
      world.add(_Drawing(Vec3(i.toDouble(), 0, 0)));
    }
    world.input.simulate.hover(-1, 50, 50);
    // The frames without drawing first: on main (2026-10-10) a world of
    // plain objects boxes one double a frame in the JIT, drawing or not.
    world.debug.enabled = false;
    final without = await madeEachFrame(world, 20000, tries: 1);
    world.debug.enabled = true;
    final made = await madeEachFrame(world, 20000, tries: 1);
    // The box and the sphere work out their corners and points as Vec3s;
    // nothing else is made per frame that wasn't made without drawing.
    expect(made.keys.toSet().difference({'Vec3', ...without.keys}), isEmpty, reason: '$made, without: $without');
    for (final MapEntry(:key, :value) in without.entries) {
      expect(made[key] ?? 0, lessThan(value * 1.5 + 100), reason: '$key: $made, without: $without');
    }
  });
}
