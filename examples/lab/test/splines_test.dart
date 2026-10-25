// 'Splines': the road with a knot dragged, the rope hanging, the fence's
// posts along their curve and the cart on its track match the golden
// (tests/golden/splines.png; FYNKEL_BLESS=1 writes it).

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/scenes/splines.dart';

import 'golden.dart';

void main() {
  test('Splines: a dragged road, a rope, a fence and a cart match their golden', () {
    final world = World()..quality = Quality.high;
    addTearDown(world.dispose);
    final scene = world.add(SplinesScene(still: true));
    world.update(1 / 60);
    final triangles = scene.road.mesh.indexCount;
    final version = scene.road.spline.version;

    // A knot dragged: the road follows in the same frame.
    world.all<KnotHandle>().firstWhere((h) => h.index == 2).placeKnot(const Vec3(-3.2, 0.3, 0.2));
    world.update(1 / 60);
    expect(scene.road.spline.version, greaterThan(version));
    expect(scene.road.mesh.indexCount, isNot(0));
    expect(scene.road.lastSweep, greaterThan(0));
    expect(triangles, greaterThan(0));

    final posts = scene.fence.get<SplineInstantiate>()!.copies;
    expect(posts.length, (scene.fence.get<SplineContainer>()!.spline.length).floor() + 1);
    expect(scene.rope.mesh.indexCount ~/ 3, greaterThan(0));
    expect(scene.cart.get<SplineAnimate>()!.distance, 2, reason: 'still: the cart waits at its start');

    final view = HeadlessView(world, width: 320, height: 200);
    addTearDown(view.dispose);
    expectGolden('splines', view.read(view.frame()).rgba, 320, 200);
  });
}
