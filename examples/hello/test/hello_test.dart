// The smallest game, run without a window: the camera, the sun and the
// cube are there, and the cube turns a radian a second.

import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:hello/main.dart';

void main() {
  test('a camera, a sun and a cube that turns a radian a second', () {
    final world = World();
    addTearDown(world.dispose);
    world.add(Hello());
    expect(world.camera, isNotNull);
    expect(world.all<Light>(), hasLength(1));
    final cube = world.first<Cube>()!;
    world.run(1);
    final forward = cube.rotation.rotate(const Vec3(0, 0, -1));
    final turned = math.atan2(-forward.x, -forward.z);
    expect(turned, closeTo(1, 0.02));
  });
}
