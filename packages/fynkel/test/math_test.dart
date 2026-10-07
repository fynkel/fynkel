import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  test('lookRotation matches the engine: the camera at (0, 1.5, 4) looking at the origin', () {
    final q = Transform(position: Vec3(0, 1.5, 4), lookAt: Vec3.zero).rotation;
    // A pure pitch down by atan(1.5 / 4), the value glam's look_at gives.
    final half = -math.atan2(1.5, 4) / 2;
    expect(q.closeTo(Quat(math.sin(half), 0, 0, math.cos(half))), isTrue, reason: '$q');
  });

  test('lookRotation points -Z along the direction and keeps +Y up', () {
    for (final direction in [Vec3(-0.4, -1, -0.3), Vec3(1, 0, 0), Vec3(0, 0, 1), Vec3(0.2, 0.9, -0.1)]) {
      final q = Quat.lookRotation(direction);
      expect(q.rotate(Vec3.forward).closeTo(direction.normalized, 1e-9), isTrue, reason: '$direction');
      expect(q.rotate(Vec3.right).y.abs(), lessThan(1e-9), reason: 'no roll');
      expect(q.rotate(Vec3.up).y, greaterThan(0), reason: 'upright');
      expect((q.length - 1).abs(), lessThan(1e-12));
    }
  });

  test('looking straight down or up still gives a rotation', () {
    for (final direction in [Vec3.down, Vec3.up]) {
      expect(Quat.lookRotation(direction).rotate(Vec3.forward).closeTo(direction, 1e-9), isTrue);
    }
    expect(() => Quat.lookRotation(Vec3.zero), throwsArgumentError);
  });

  test('axisAngle turns counter-clockwise, and a * b turns by b first', () {
    final quarter = Quat.axisAngle(Vec3.up, math.pi / 2);
    expect(quarter.rotate(Vec3.forward).closeTo(Vec3.left, 1e-12), isTrue);
    final pitch = Quat.axisAngle(Vec3.right, math.pi / 2);
    // forward → up (pitch), then up stays up (yaw around up).
    expect((quarter * pitch).rotate(Vec3.forward).closeTo(Vec3.up, 1e-12), isTrue);
    expect((quarter * quarter.inverse).closeTo(Quat.identity, 1e-12), isTrue);
  });

  test('Color reads 0xAARRGGBB like Flutter does', () {
    const c = Color(0x80E0613A);
    expect((c.r, c.g, c.b, c.a), (0xE0 / 255, 0x61 / 255, 0x3A / 255, 0x80 / 255));
    expect(Color(0xFFFFFFFF), Color.white);
  });

  test('Transform takes at most one way to say its rotation', () {
    expect(() => Transform(rotation: Quat.identity, lookAt: Vec3.zero), throwsArgumentError);
    expect(Transform(forward: Vec3.back).forward.closeTo(Vec3.back, 1e-12), isTrue);
    expect(Transform().rotated(Vec3.up, 0).rotation, Quat.identity);
  });
}
