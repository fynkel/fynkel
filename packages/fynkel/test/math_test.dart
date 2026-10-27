import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  test('normalizedOrZero is zero for a zero vector, where normalized throws (hardening G1)', () {
    // The chase idiom, the frame the two meet.
    const position = Vec3(1, 2, 3), target = Vec3(1, 2, 3);
    expect((target - position).normalizedOrZero * 5, Vec3.zero);
    expect(() => (target - position).normalized, throwsArgumentError);
    expect(Vec3(3, 0, 4).normalizedOrZero, Vec3(0.6, 0, 0.8));
    expect(Vec3(double.nan, 0, 0).normalizedOrZero, Vec3.zero);
  });

  // Stage 14's DF14: a zero quaternion normalized gave NaNs, where a zero
  // vector throws.
  test('a quaternion of length 0 or not finite has no rotation to normalize, and says so', () {
    for (final q in [const Quat(0, 0, 0, 0), const Quat(double.nan, 0, 0, 1), const Quat(0, double.infinity, 0, 1)]) {
      expect(
        () => q.normalized,
        throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('no rotation'))),
        reason: '$q',
      );
    }
    expect(const Quat(0, 0, 0, 2).normalized, Quat.identity);
  });

  test('lookRotation matches the engine: the camera at (0, 1.5, 4) looking at the origin', () {
    final q = Quat.lookRotation(Vec3.zero - Vec3(0, 1.5, 4));
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
    expect(
      () => Quat.lookRotation(Vec3.zero),
      throwsA(isArgumentError.having((e) => '$e', 'message', contains('give it target - eye'))),
      reason: 'a point where a direction belongs, the usual slip, says what to give instead',
    );
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

  test('euler turns each way as axisAngle does: roll, then pitch, then yaw', () {
    const turn = math.pi / 2;
    expect(Quat.euler(0, 0, 0), Quat.identity);
    expect(Quat.euler(turn, 0, 0).rotate(Vec3.forward).closeTo(Vec3.up, 1e-12), isTrue, reason: 'pitch up');
    expect(Quat.euler(0, turn, 0).rotate(Vec3.forward).closeTo(Vec3.left, 1e-12), isTrue, reason: 'yaw left');
    expect(Quat.euler(0, 0, turn).rotate(Vec3.up).closeTo(Vec3.left, 1e-12), isTrue, reason: 'roll');
    // All three: Unity's order, roll first and yaw last.
    final q = Quat.euler(0.3, 0.8, 0.5);
    final expected = Quat.axisAngle(Vec3.up, 0.8) * Quat.axisAngle(Vec3.right, 0.3) * Quat.axisAngle(Vec3.back, 0.5);
    expect(q.closeTo(expected), isTrue, reason: '$q');
    expect(Quat.euler(0, 0.8, 0.5).closeTo(Quat.euler(0, 0.8, 0) * Quat.euler(0, 0, 0.5)), isTrue);
  });

  test('Transform is a position, a rotation and a scale', () {
    const t = Transform(position: Vec3(1, 2, 3), scale: Vec3(2, 2, 2));
    expect(t.rotation, Quat.identity);
    expect(Quat.lookRotation(Vec3.back).rotate(Vec3.forward).closeTo(Vec3.back, 1e-12), isTrue);
    expect(Transform().rotated(Vec3.up, 0).rotation, Quat.identity);
  });
}
