import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';

void expectVec(List<double> a, List<double> b, {double eps = 1e-6}) {
  expect(a.length, b.length);
  for (var i = 0; i < a.length; i++) {
    expect(a[i], closeTo(b[i], eps), reason: 'component $i of $a vs $b');
  }
}

void main() {
  group('projection matrices', () {
    test('perspective matches the runtime (column-major, Metal 0..1 depth)', () {
      final m = perspectiveMatrix(math.pi / 2, 2, 0.1, 100);
      expect(m[0], closeTo(0.5, 1e-9)); // f / aspect with f = 1
      expect(m[5], closeTo(1, 1e-9));
      expect(m[10], closeTo(100 / (0.1 - 100), 1e-9));
      expect(m[11], -1);
      expect(m[14], closeTo(0.1 * 100 / (0.1 - 100), 1e-9));
      expect(m[15], 0);
    });

    test('orthographic maps the height to ±1', () {
      final m = orthographicMatrix(4, 2, 0.1, 100);
      expect(m[0], closeTo(1 / 4, 1e-9)); // half height 2 × aspect 2 = 4
      expect(m[5], closeTo(1 / 2, 1e-9));
      expect(m[15], 1);
    });
  });

  group('EditorCamera', () {
    test('the default looks at its pivot from its distance', () {
      final c = EditorCamera.initial();
      expect(vDistance(c.position, c.pivot), closeTo(c.distance, 1e-9));
      final toPivot = vNormalize(vSub(c.pivot, c.position));
      expectVec(c.forward, toPivot);
    });

    test('orbit keeps the distance; pitch is clamped short of the poles', () {
      final c = EditorCamera.initial();
      c.orbit(300, -2000);
      expect(vDistance(c.position, c.pivot), closeTo(c.distance, 1e-9));
      expect(c.pitch.abs(), lessThanOrEqualTo(EditorCamera.maxPitch));
    });

    test('dolly never goes through the pivot; pan moves the pivot in the view plane', () {
      final c = EditorCamera.initial();
      for (var i = 0; i < 100; i++) {
        c.dolly(5);
      }
      expect(c.distance, greaterThanOrEqualTo(EditorCamera.minDistance));
      final before = List.of(c.pivot);
      final f = c.forward;
      c.pan(100, 0, 500);
      final moved = vSub(c.pivot, before);
      expect(vDot(vNormalize(moved), f).abs(), lessThan(1e-6));
    });

    test('frame puts the box in view and focus centres it', () {
      final c = EditorCamera.initial();
      c.frame([10, 0, 10], [12, 2, 12]);
      expectVec(c.pivot, [11, 1, 11]);
      final vp = ViewProjection(c, 800, 600);
      for (final corner in boxCorners([10, 0, 10], [12, 2, 12])) {
        final px = vp.project(corner)!;
        expect(px[0], inInclusiveRange(0, 800));
        expect(px[1], inInclusiveRange(0, 600));
      }
    });

    test('alignTo takes the exact matrix and the lens; a move releases it', () {
      final c = EditorCamera.initial();
      final world = Trs(
        position: [0, 3.557387, 5.636236],
        rotation: [-0.174108, 0, 0, 0.984727],
        scale: [1, 1, 1],
      ).toMatrix();
      c.alignTo(world, orthographic: false, fovY: 0.95, near: 0.1, far: 120, manualExposure: 12.5);
      expectVec(c.world, world, eps: 1e-9);
      expect(c.fovY, 0.95);
      expect(c.near, 0.1);
      expect(c.far, 120);
      expect(c.manualExposure, 12.5);
      final preview = c.toPreview();
      expectVec(preview.world, world, eps: 1e-9);
      expect(preview.toJson()['exposureMode'], 'manual');
      expect(preview.toJson()['exposure'], 12.5);
      // The rig approximates the aligned view once released.
      c.orbit(0, 0);
      expect(c.alignedWorld, isNull);
      expectVec(matTranslation(c.world), [0, 3.557387, 5.636236], eps: 1e-3);
      expect(c.manualExposure, 12.5, reason: 'a lens setting survives movements');
    });

    test('toJson / fromJson round-trip, unknown json is null', () {
      final c = EditorCamera(
        pivot: [1, 2, 3],
        distance: 4,
        yaw: 0.3,
        pitch: -0.2,
        orthographic: true,
        fovY: 1.1,
        near: 0.5,
        far: 50,
      );
      final back = EditorCamera.fromJson(c.toJson())!;
      expectVec(back.pivot, c.pivot);
      expect(back.distance, 4);
      expect(back.yaw, 0.3);
      expect(back.pitch, -0.2);
      expect(back.orthographic, isTrue);
      expect(back.fovY, 1.1);
      expect(back.near, 0.5);
      expect(back.far, 50);
      expect(EditorCamera.fromJson('nonsense'), isNull);
      // A bad field falls back to its default; the rest of the camera survives.
      final lenient = EditorCamera.fromJson({'distance': 'x', 'yaw': 0.3, 'near': -1})!;
      expect(lenient.distance, 10);
      expect(lenient.yaw, 0.3);
      expect(lenient.near, 0.1);
    });
  });

  group('ViewProjection', () {
    test('a projected point casts back along the ray through its pixel', () {
      final c = EditorCamera.initial();
      final vp = ViewProjection(c, 1200, 800);
      const p = [1.5, 0.7, -2.0];
      final px = vp.project(p)!;
      final ray = vp.rayAt(px[0], px[1]);
      // The point lies on the ray: distance from the ray is ~0.
      final t = vDot(vSub(p, ray.origin), ray.dir);
      expectVec(ray.at(t), p, eps: 1e-6);
      expectVec(vp.ndcOf(0, 0), [-1, 1]);
      expectVec(vp.ndcOf(1200, 800), [1, -1]);
    });

    test('orthographic rays are parallel to the forward axis', () {
      final c = EditorCamera.initial()..orthographic = true;
      final vp = ViewProjection(c, 1000, 500);
      final a = vp.rayAt(10, 10), b = vp.rayAt(900, 400);
      expectVec(a.dir, b.dir, eps: 1e-9);
      expectVec(a.dir, c.forward, eps: 1e-9);
    });

    test('metresPerPixel scales with distance in perspective and is constant in orthographic', () {
      final c = EditorCamera.initial();
      final vp = ViewProjection(c, 1000, 1000);
      final near = vp.metresPerPixel(vAdd(c.position, vScale(c.forward, 2)));
      final far = vp.metresPerPixel(vAdd(c.position, vScale(c.forward, 8)));
      expect(far / near, closeTo(4, 1e-6));
      c.orthographic = true;
      final o = ViewProjection(c, 1000, 1000);
      expect(o.metresPerPixel([0, 0, 0]), closeTo(o.metresPerPixel([5, 5, 5]), 1e-9));
    });
  });

  group('Ray', () {
    test('hitPlane and closestOnLine', () {
      const r = Ray([0, 5, 0], [0, -1, 0]);
      expectVec(r.hitPlane([0, 0, 0], [0, 1, 0])!, [0, 0, 0]);
      expect(r.hitPlane([0, 0, 0], [1, 0, 0]), isNull, reason: 'parallel');
      expect(const Ray([0, 5, 0], [0, 1, 0]).hitPlane([0, 0, 0], [0, 1, 0]), isNull, reason: 'behind');
      expect(
        const Ray([0, 0, 0], [0, -1, 0]).hitPlane([0, 0, 0], [0, 1, 0]),
        isNull,
        reason: 'the origin on the plane is no hit',
      );
      // A ray straight down at x = 3 is closest to the X axis at s = 3.
      const r2 = Ray([3, 5, 0], [0, -1, 0]);
      expect(r2.closestOnLine([0, 0, 0], [1, 0, 0]), closeTo(3, 1e-9));
      expect(r2.closestOnLine([0, 0, 0], [0, 1, 0]), isNull, reason: 'parallel to the axis');
    });
  });

  test('snapTo rounds to the step; a zero step is off', () {
    expect(snapTo(1.24, 0.5), 1.0);
    expect(snapTo(1.26, 0.5), 1.5);
    expect(snapTo(-0.74, 0.5), -0.5);
    expect(snapTo(1.24, 0), 1.24);
  });

  test('quaternion helpers agree with the matrix path', () {
    final q = quatFromAxisAngle([0, 1, 0], math.pi / 2);
    expectVec(quatRotate(q, [1, 0, 0]), [0, 0, -1]);
    final m = quatToMatrix(q);
    expectVec(transformDirection(m, [1, 0, 0]), [0, 0, -1]);
    expectVec(quatNormalize(quatMul(q, quatConjugate(q))), quatIdentity);
  });

  test('item 9e: looking from an axis side, the view names, and a second click from the opposite side', () {
    final c = EditorCamera.initial();
    c.pivot = [1, 2, 3];
    c.distance = 8;
    expect(c.viewName, isNull, reason: 'the default view is between the axes');
    for (final (axis, positive, name) in [
      (0, true, 'Right'),
      (0, false, 'Left'),
      (1, true, 'Top'),
      (1, false, 'Bottom'),
      (2, true, 'Front'),
      (2, false, 'Back'),
    ]) {
      final v = EditorCamera.initial()..pivot = [1, 2, 3];
      v.distance = 8;
      v.lookFrom(axis, positive: positive);
      expect(v.viewName, name);
      expect(v.viewFrom, (axis, positive));
      expect(v.forward[axis], closeTo(positive ? -1 : 1, 1e-4), reason: '$name looks back at the pivot');
      expect(v.pivot, [1, 2, 3], reason: 'the pivot stays');
      expect(vDistance(v.position, v.pivot), closeTo(8, 1e-9), reason: 'so does the distance');
      if (axis == 1) {
        expect(
          v.up[2],
          closeTo(positive ? -1 : 1, 1e-2),
          reason: 'the front at the bottom looking down, at the top looking up',
        );
      }
      v.lookFrom(axis, positive: positive);
      expect(v.viewFrom, (axis, !positive), reason: 'again: from the other side');
    }
    // From an aligned scene camera: the exact matrix is dropped.
    c.alignTo(Trs(position: const [0, 0, 5], rotation: const [0, 0, 0, 1], scale: const [1, 1, 1]).toMatrix());
    expect(c.viewName, 'Front');
    c.lookFrom(0, positive: true);
    expect(c.alignedWorld, isNull);
    expect(c.viewName, 'Right');
    // A small orbit leaves the named view.
    c.orbit(10, 0);
    expect(c.viewName, isNull);
  });
}
