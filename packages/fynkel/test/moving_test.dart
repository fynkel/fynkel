// Moving things by hand (docs/stage-4.md §3.7, step 6): position,
// rotation and scale on objects and behaviours, the world's directions and
// points, and Unity's Mathf, against hand-worked values.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

// The world's transforms come back from the engine in 32-bit floats.
void expectVec(Vec3 actual, Vec3 expected, [double tolerance = 1e-6]) =>
    expect(actual.closeTo(expected, tolerance), isTrue, reason: '$actual is not $expected');

class Steer extends Behaviour {}

void main() {
  late World world;

  setUp(() => world = World());
  tearDown(() => world.dispose());

  // A parent at (1, 0, 0), turned a quarter left (its -Z now looks along
  // -X) and twice the size: its (0, 0, -1) is the world's (-1, 0, 0).
  GameObject parent() => world.add(
    GameObject(
      transform: Transform(
        position: const Vec3(1, 0, 0),
        rotation: Quat.axisAngle(Vec3.up, math.pi / 2),
        scale: Vec3.one * 2,
      ),
    ),
  );

  group('on an object', () {
    test('position, rotation and scale read and set its transform', () {
      final o = world.add(GameObject());
      o.position = const Vec3(1, 2, 3);
      o.rotation = Quat.axisAngle(Vec3.up, 1);
      o.scale = const Vec3(2, 2, 2);
      expect(
        o.transform,
        Transform(position: const Vec3(1, 2, 3), rotation: Quat.axisAngle(Vec3.up, 1), scale: Vec3.one * 2),
      );
      expect((o.position, o.rotation, o.scale), (o.transform.position, o.transform.rotation, o.transform.scale));
    });

    test('before it is added too', () {
      final o = GameObject()..position = const Vec3(0, 1, 0);
      expect(world.add(o).position, const Vec3(0, 1, 0));
    });

    test('worldPosition is where it is in the world, and setting it puts it there, whatever its parents', () {
      final child = parent().add(GameObject(transform: Transform(position: const Vec3(0, 0, -1))));
      expectVec(child.worldPosition, const Vec3(-1, 0, 0));
      child.worldPosition = const Vec3(1, 4, -2);
      expectVec(child.worldPosition, const Vec3(1, 4, -2));
      expectVec(child.position, const Vec3(1, 2, 0));
    });

    test('forward, right and up are its -Z, +X and +Y in the world', () {
      final child = parent().add(GameObject(transform: Transform(rotation: Quat.axisAngle(Vec3.right, math.pi / 2))));
      // Tipped back a quarter (its -Z looks up), then turned with its parent.
      expectVec(child.forward, Vec3.up);
      expectVec(child.right, Vec3.forward);
      expectVec(child.up, Vec3.right);
    });

    test('lookAt turns it to look at a world point, under a turned parent too', () {
      final root = world.add(GameObject());
      root.lookAt(const Vec3(0, 0, 5));
      expectVec(root.forward, Vec3.back);
      final child = parent().add(GameObject());
      child.lookAt(const Vec3(1, 0, 3));
      expectVec(child.forward, Vec3.back);
      expect(child.up.y, closeTo(1, 1e-6));
      expect(() => root.lookAt(Vec3.zero), throwsArgumentError, reason: 'it is there');
    });

    test('transformPoint goes from its space to the world, and inverseTransformPoint back', () {
      final child = parent().add(GameObject(transform: Transform(position: const Vec3(0, 1, 0))));
      // Its (0, 0, -1): twice as far as the parent's scale, along -X.
      expectVec(child.transformPoint(const Vec3(0, 0, -1)), const Vec3(-1, 2, 0));
      expectVec(child.inverseTransformPoint(const Vec3(-1, 2, 0)), const Vec3(0, 0, -1));
      for (final p in [const Vec3(3, -2, 0.5), const Vec3(-7, 1, 4)]) {
        expectVec(child.inverseTransformPoint(child.transformPoint(p)), p, 1e-5);
      }
    });
  });

  test('a behaviour moves its object', () {
    final o = world.add(GameObject(components: [Steer()]));
    final steer = o.get<Steer>()!;
    steer.position = const Vec3(0, 0, -3);
    steer.lookAt(const Vec3(4, 0, -3));
    expect(o.position, const Vec3(0, 0, -3));
    expectVec(steer.forward, Vec3.right);
    expectVec(steer.worldPosition, const Vec3(0, 0, -3));
  });

  group("Unity's Mathf", () {
    test('lerp, inverseLerp and clamp01', () {
      expect(lerp(2, 6, 0.25), 3);
      expect(lerp(2, 6, 1.5), 8, reason: 'not clamped');
      expect(inverseLerp(2, 6, 3), 0.25);
      expect(inverseLerp(2, 2, 3), 0);
      expect([clamp01(-1), clamp01(0.5), clamp01(2)], [0, 0.5, 1]);
      expect(Vec3.lerp(Vec3.zero, const Vec3(2, 4, 8), 0.5), const Vec3(1, 2, 4));
      expect(Vec2.lerp(Vec2.zero, const Vec2(2, 4), 0.25), const Vec2(0.5, 1));
      expect(Color.lerp(Color.black, Color.white, 0.5), const Color.rgb(0.5, 0.5, 0.5));
    });

    test('slerp turns at an even speed, the short way', () {
      final a = Quat.identity, b = Quat.axisAngle(Vec3.up, 2);
      for (final t in [0.0, 0.25, 0.5, 1.0]) {
        expect(Quat.slerp(a, b, t).closeTo(Quat.axisAngle(Vec3.up, 2 * t)), isTrue, reason: '$t');
      }
      final far = Quat.axisAngle(Vec3.up, 3 * math.pi / 2);
      expect(Quat.slerp(a, far, 0.5).closeTo(Quat.axisAngle(Vec3.up, -math.pi / 4)), isTrue, reason: 'the short way');
      expect(Quat.slerp(a, Quat.axisAngle(Vec3.up, 1e-6), 0.5).closeTo(Quat.axisAngle(Vec3.up, 5e-7)), isTrue);
      expect(a.angleTo(b), closeTo(2, 1e-12));
    });

    test('moveTowards steps at most so far, never past', () {
      expect(moveTowards(0, 10, 3), 3);
      expect(moveTowards(0, -10, 3), -3);
      expect(moveTowards(9, 10, 3), 10);
      expectVec(Vec3.moveTowards(Vec3.zero, const Vec3(0, 3, 4), 1), const Vec3(0, 0.6, 0.8), 1e-12);
      expect(Vec3.moveTowards(Vec3.zero, const Vec3(0, 3, 4), 6), const Vec3(0, 3, 4));
      expect(Vec2.moveTowards(Vec2.zero, const Vec2(3, 4), 2.5), const Vec2(1.5, 2));
    });

    test('repeat wraps and pingPong goes back and forth', () {
      expect([0.0, 1.0, 2.5, 3.0, 7.0, -0.5].map((t) => repeat(t, 3)), [0, 1, 2.5, 0, 1, 2.5]);
      expect([0.0, 1.0, 2.0, 3.0, 4.0, 5.0].map((t) => pingPong(t, 2)), [0, 1, 2, 1, 0, 1]);
    });

    test('randomInSphere and randomOnCircle', () {
      final r = math.Random(1);
      for (var i = 0; i < 200; i++) {
        expect(randomInSphere(r).length, lessThanOrEqualTo(1));
        expect(randomOnCircle(r).length, closeTo(1, 1e-12));
      }
    });

    test('SmoothDamp closes in on the target without passing it', () {
      final damp = SmoothDamp(smoothTime: 0.3);
      var x = 0.0;
      final seen = <double>[];
      for (var i = 0; i < 120; i++) {
        x = damp.step(x, 10, 1 / 60);
        seen.add(x);
      }
      for (var i = 1; i < seen.length; i++) {
        expect(seen[i], greaterThanOrEqualTo(seen[i - 1]));
        expect(seen[i], lessThanOrEqualTo(10));
      }
      expect(seen[17], inInclusiveRange(4, 7), reason: 'about half way after its smooth time');
      expect(x, closeTo(10, 0.1));
      expect(damp.velocity, greaterThanOrEqualTo(0));

      final capped = SmoothDamp(smoothTime: 0.3, maxSpeed: 1);
      var y = 0.0;
      for (var i = 0; i < 60; i++) {
        y = capped.step(y, 10, 1 / 60);
      }
      expect(y, lessThanOrEqualTo(1.0001), reason: 'at most 1 a second');

      // Already rushing at it: it stops at the target, not past it.
      final fast = SmoothDamp(smoothTime: 0.3)..velocity = 100;
      expect(fast.step(0, 1, 0.1), 1);
      expect(fast.velocity, 0);

      final follow = SmoothDamp3(smoothTime: 0.2);
      var p = Vec3.zero;
      for (var i = 0; i < 240; i++) {
        p = follow.step(p, const Vec3(1, -2, 3), 1 / 60);
      }
      expectVec(p, const Vec3(1, -2, 3), 1e-3);
    });
  });
}
