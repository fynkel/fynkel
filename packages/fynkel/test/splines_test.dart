// Splines (docs/stage-13.md phase 3): through their knots, by distance,
// up turned as little as it can be, and a shape swept along one.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

Matcher near(Vec3 v, [double within = 1e-6]) =>
    predicate<Vec3>((p) => (p - v).length <= within, 'within $within of $v');

Matcher about(double v, [double within = 1e-6]) => closeTo(v, within);

/// A circle of [radius] around Y in the XZ plane, [n] points.
List<Vec3> circle(double radius, int n) => [
  for (var i = 0; i < n; i++) Vec3(radius * math.cos(math.pi * 2 * i / n), 0, radius * math.sin(math.pi * 2 * i / n)),
];

void main() {
  group('along a spline', () {
    test('a linear spline goes straight between its knots, by distance', () {
      final path = Spline.linear([Vec3.zero, const Vec3(10, 0, 0), const Vec3(10, 0, 10)]);
      expect(path.length, about(20));
      expect(path.positionAt(5), near(const Vec3(5, 0, 0)));
      expect(path.positionAt(15), near(const Vec3(10, 0, 5)));
      expect(path.directionAt(5), near(Vec3.right));
      expect(path.directionAt(15), near(Vec3.back));
      expect(path.distanceOfKnot(1), about(10));
      // Clamped to its ends.
      expect(path.positionAt(-3), near(Vec3.zero));
      expect(path.positionAt(99), near(const Vec3(10, 0, 10)));
    });

    test('a smooth spline goes through every knot, at an even speed', () {
      final points = [Vec3.zero, const Vec3(4, 1, -2), const Vec3(9, 0, 3), const Vec3(15, 2, 1)];
      final path = Spline.smooth(points);
      for (var i = 0; i < points.length; i++) {
        expect(path.positionAt(path.distanceOfKnot(i)), near(points[i], 1e-6), reason: 'knot $i');
      }
      // A metre apart along it is about a metre apart in space.
      for (var d = 0.0; d + 1 < path.length; d += 1) {
        final gap = (path.positionAt(d + 1) - path.positionAt(d)).length;
        expect(gap, inInclusiveRange(0.97, 1.001), reason: 'at $d');
      }
    });

    test('a closed spline goes round, and its distances do', () {
      final ring = Spline.smooth(circle(5, 8), closed: true);
      expect(ring.length, about(math.pi * 10, math.pi * 10 * 0.01));
      expect(ring.positionAt(ring.length + 2), near(ring.positionAt(2), 1e-6));
      expect(ring.positionAt(-1), near(ring.positionAt(ring.length - 1), 1e-6));
      expect(ring.upAt(ring.length - 1e-9), near(ring.upAt(0), 1e-4));
      // Each point is close to the circle.
      for (var d = 0.0; d < ring.length; d += 0.5) {
        expect(ring.positionAt(d).length, about(5, 0.05));
      }
    });

    test('a bezier knot leaves along its own handle', () {
      final path = Spline([
        const Knot(Vec3.zero, mode: TangentMode.bezier, tangentOut: Vec3(0, 3, 0)),
        const Knot(Vec3(5, 0, 0), mode: TangentMode.bezier, tangentIn: Vec3(0, 3, 0)),
      ]);
      expect(path.directionAt(0), near(Vec3.up, 1e-6));
      expect(path.directionAt(path.length), near(Vec3.down, 1e-6));
      // An arch: its middle is up.
      expect(path.positionAt(path.length / 2).y, about(2.25, 1e-3));
    });

    test('up stays +Y along a flat curve, and turns smoothly along a helix', () {
      final flat = Spline.smooth([Vec3.zero, const Vec3(5, 0, -5), const Vec3(10, 0, 0), const Vec3(15, 0, -8)]);
      for (var d = 0.0; d < flat.length; d += 0.7) {
        expect(flat.upAt(d), near(Vec3.up, 1e-6), reason: 'at $d');
      }
      final helix = Spline.smooth([
        for (var i = 0; i <= 24; i++) Vec3(3 * math.cos(i * math.pi / 4), i * 0.5, 3 * math.sin(i * math.pi / 4)),
      ]);
      var before = helix.upAt(0);
      for (var d = 0.1; d < helix.length; d += 0.1) {
        final (up, along) = (helix.upAt(d), helix.directionAt(d));
        expect(up.length, about(1, 1e-6));
        expect(up.dot(along).abs(), lessThan(1e-6));
        expect(up.dot(before), greaterThan(0.99), reason: 'no sudden turn at $d');
        before = up;
      }
    });

    test('a knot\'s roll turns up around the curve, clockwise seen going along', () {
      final road = Spline([
        const Knot(Vec3.zero, mode: TangentMode.linear),
        Knot(const Vec3(10, 0, 0), mode: TangentMode.linear, roll: 90.deg),
      ]);
      expect(road.upAt(0), near(Vec3.up));
      // Going along +X, right is +Z.
      expect(road.upAt(10), near(Vec3.back, 1e-6));
      expect(road.upAt(5), near(Vec3(0, math.sqrt1_2, math.sqrt1_2), 1e-6));
      final turned = road.rotationAt(5);
      expect(turned.rotate(Vec3.forward), near(Vec3.right, 1e-6));
    });

    test('the nearest point is found along it', () {
      final path = Spline.linear([Vec3.zero, const Vec3(10, 0, 0), const Vec3(10, 0, 10)]);
      final near5 = path.nearest(const Vec3(5, 3, 0));
      expect(near5.distance, about(5, 1e-4));
      expect(near5.position, near(const Vec3(5, 0, 0), 1e-4));
      final ring = Spline.smooth(circle(5, 12), closed: true);
      final hit = ring.nearest(const Vec3(0, 0, 9));
      expect(hit.position, near(const Vec3(0, 0, 5), 0.02));
    });

    test('knots are added, changed, moved and taken away', () {
      final path = Spline([]);
      expect(path.length, 0);
      path
        ..add(const Knot(Vec3.zero))
        ..add(const Knot(Vec3(4, 0, 0)));
      expect(path.length, about(4));
      final v = path.version;
      path.insert(1, const Knot(Vec3(2, 2, 0), mode: TangentMode.linear));
      expect(path.version, greaterThan(v));
      expect(path.knots.map((k) => k.position), [Vec3.zero, const Vec3(2, 2, 0), const Vec3(4, 0, 0)]);
      expect(path[1].mode, TangentMode.linear);
      path[1] = const Knot(Vec3(2, 0, 0));
      expect(path.length, about(4));
      expect(path.removeAt(1).position, const Vec3(2, 0, 0));
      expect(path.knotCount, 2);
      path.setPositions(Float32List.fromList([0, 0, 0, 0, 3, 0]));
      expect(path.length, about(3));
      expect(path.directionAt(1), near(Vec3.up));
      expect(() => path.setPositions(Float32List(3)), throwsArgumentError);
      expect(() => path[2], throwsRangeError);
      expect(() => path.add(const Knot(Vec3(double.nan, 0, 0))), throwsArgumentError);
      path.clear();
      expect(path.knotCount, 0);
    });
  });

  group('sweeping', () {
    test('a shape swept along a spline is the length of it, with caps', () {
      final path = Spline.linear([Vec3.zero, const Vec3(10, 0, 0)]);
      final road = Mesh.sweep(path, Shape2D.rect(4, 0.5), segmentsPerMetre: 1);
      // Four sides of 10 quads, two caps of 2 triangles.
      expect(road.indexCount ~/ 3, 4 * 10 * 2 + 2 * 2);
      final p = road.positions;
      double most(int axis, double Function(double, double) pick) {
        var m = p[axis];
        for (var i = axis; i < p.length; i += 3) {
          m = pick(m, p[i]);
        }
        return m;
      }

      expect([most(0, math.min), most(0, math.max)], [about(0, 1e-5), about(10, 1e-5)]);
      expect([most(1, math.min), most(1, math.max)], [about(-0.25, 1e-5), about(0.25, 1e-5)]);
      // Going along +X, the shape's X is +Z.
      expect([most(2, math.min), most(2, math.max)], [about(-2, 1e-5), about(2, 1e-5)]);
      // More samples where it bends.
      final bent = Spline.smooth([Vec3.zero, const Vec3(5, 0, -5), const Vec3(10, 0, 0)]);
      final tube = Mesh.sweep(bent, Shape2D.circle(0.2, segments: 8), segmentsPerMetre: 0.1, caps: false);
      expect(tube.indexCount ~/ 3 ~/ (8 * 2), greaterThan(10));
    });

    test('a closed spline makes a ring with no caps, and maxSegments is kept', () {
      final ring = Spline.smooth(circle(5, 8), closed: true);
      final tyre = Mesh.sweep(ring, Shape2D.circle(1, segments: 12), segmentsPerMetre: 1, maxSegments: 20);
      expect(tyre.indexCount ~/ 3, 20 * 12 * 2);
    });

    test('a mesh swept again keeps its count, and broken settings are refused', () {
      final points = Float32List.fromList([0, 0, 0, 1, -0.3, 0, 2, 0, 0]);
      final rope = Spline.smooth([Vec3.zero, const Vec3(1, 0, 0), const Vec3(2, 0, 0)]);
      final mesh = Mesh()..markDynamic();
      final shape = Shape2D.circle(0.05, segments: 6);
      mesh.setSweep(rope, shape, segmentsPerMetre: 8, maxSegments: 16);
      final count = mesh.vertexCount;
      rope.setPositions(points);
      mesh.setSweep(rope, shape, segmentsPerMetre: 8, maxSegments: 16);
      expect(mesh.vertexCount, count);
      var lowest = 0.0;
      final p = mesh.positions;
      for (var i = 1; i < p.length; i += 3) {
        lowest = math.min(lowest, p[i]);
      }
      expect(lowest, lessThan(-0.3));
      expect(mesh.normals.length, mesh.vertexCount * 3);
      expect(
        () => Mesh.sweep(Spline.linear([Vec3.zero]), shape),
        throwsA(isA<ArgumentError>().having((e) => '$e', 'says', contains('1 knot'))),
      );
      // GD11: knots all at one point say so, not that there are too few.
      rope.setPositions(Float32List(9));
      expect(
        () => mesh.setSweep(rope, shape),
        throwsA(isA<ArgumentError>().having((e) => '$e', 'says', contains('all at one point (length 0)'))),
      );
      expect(() => Mesh.sweep(rope, shape, segmentsPerMetre: 0), throwsArgumentError);
      expect(() => Mesh.cube().setSweep(rope, shape), throwsStateError);
    });
  });
}
