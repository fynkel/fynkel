// Gizmos drawn by game code (ADR-0019 §3, item 9h): the shapes' lines,
// the colour and matrix scopes, off unless the world turns them on, and
// the world clearing them every frame.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Draws a sphere around itself every update.
class Marker extends Behaviour {
  @override
  void update(UpdateContext context) {
    Gizmos.withColor(const Vec4(1, 0, 0, 1), () => Gizmos.wireSphere(transform.position, 2));
  }
}

List<List<double>> linesOf(GizmoBuffer b) {
  final d = b.lines;
  return [for (var i = 0; i < b.lineCount; i++) d.sublist(i * 10, i * 10 + 10).toList()];
}

void main() {
  group('without a world', () {
    late TestGameContext context;
    late GizmoBuffer buffer;
    setUp(() {
      context = TestGameContext();
      buffer = GizmoBuffer();
      context.install<GizmoBuffer>(buffer);
    });

    test('compiled in a test (not a product build)', () {
      expect(Gizmos.compiled, isTrue);
    });

    test('off: no buffer, every call is silent', () {
      final bare = TestGameContext();
      bare.run(() {
        expect(Gizmos.enabled, isFalse);
        Gizmos.wireSphere(Vec3.zero, 1);
        Gizmos.color = const Vec4(1, 0, 0, 1);
        expect(Gizmos.color, Gizmos.defaultColor);
      });
      Gizmos.line(Vec3.zero, Vec3.one); // outside any context too
    });

    test('a line and a ray in world space, in the current colour', () {
      context.run(() {
        Gizmos.line(Vec3.zero, const Vec3(1, 2, 3));
        Gizmos.color = const Vec4(0, 1, 0, 0.5);
        Gizmos.ray(const Vec3(1, 0, 0), const Vec3(0, 0, -2));
      });
      expect(linesOf(buffer), [
        [0, 0, 0, 1, 2, 3, 1, 1, 1, 1],
        [1, 0, 0, 1, 0, -2, 0, 1, 0, 0.5],
      ]);
    });

    test('the shapes\' line counts; a non-finite point is dropped', () {
      int count(void Function() draw) {
        buffer.clear();
        context.run(draw);
        return buffer.lineCount;
      }

      expect(count(() => Gizmos.wireCircle(Vec3.zero, 1)), 32);
      expect(count(() => Gizmos.wireSphere(Vec3.zero, 1)), 96);
      expect(count(() => Gizmos.wireBox(Vec3.zero, Vec3.one)), 12);
      expect(count(() => Gizmos.wireCapsule(Vec3.zero, 0.5, 2)), 64 + 4 + 64);
      expect(count(() => Gizmos.arrow(Vec3.zero, const Vec3(0, 0, -1))), 5);
      expect(count(() => Gizmos.frustum(Vec3.zero, Quat.identity, 1, 1.5, 0.1, 10)), 12);
      expect(count(() => Gizmos.line(Vec3.zero, const Vec3(double.nan, 0, 0))), 0);
    });

    test('a sphere\'s points are at its radius', () {
      context.run(() => Gizmos.wireSphere(const Vec3(1, 1, 1), 2));
      for (final l in linesOf(buffer)) {
        final a = Vec3(l[0], l[1], l[2]);
        expect((a - const Vec3(1, 1, 1)).length, closeTo(2, 1e-5));
      }
    });

    test('the matrix scope draws in a local space and restores the previous one', () {
      const local = Transform(position: Vec3(10, 0, 0), rotation: Quat.identity, scale: Vec3(2, 2, 2));
      context.run(() {
        Gizmos.withMatrix(local, () {
          Gizmos.withColor(const Vec4(0, 0, 1, 1), () => Gizmos.line(Vec3.zero, const Vec3(1, 0, 0)));
          expect(Gizmos.color, Gizmos.defaultColor);
        });
        expect(Gizmos.matrix, isNull);
        Gizmos.line(Vec3.zero, const Vec3(1, 0, 0));
      });
      expect(linesOf(buffer), [
        [10, 0, 0, 12, 0, 0, 0, 0, 1, 1],
        [0, 0, 0, 1, 0, 0, 1, 1, 1, 1],
      ]);
    });

    test('clear empties the lines and resets the colour and matrix', () {
      context.run(() {
        Gizmos.color = const Vec4(1, 0, 0, 1);
        Gizmos.matrix = const Transform(position: Vec3.one, rotation: Quat.identity, scale: Vec3.one);
        Gizmos.line(Vec3.zero, Vec3.one);
      });
      buffer.clear();
      expect(buffer.isEmpty, isTrue);
      expect(buffer.color, Gizmos.defaultColor);
      expect(buffer.matrix, isNull);
    });
  });

  group('in a world', () {
    late NativeLibrary lib;
    setUpAll(() => lib = NativeLibrary.open());
    tearDownAll(() => lib.dispose());

    test('off by default; on, a behaviour\'s gizmos are the frame\'s, cleared at the next', () {
      final w = World.create(library: lib)..throwOnDiagnostics = true;
      addTearDown(w.dispose);
      final go = w.spawn(transform: const Transform(position: Vec3(0, 1, 0), rotation: Quat.identity, scale: Vec3.one));
      go.addBehaviour(Marker());
      w.frame(1 / 60);
      expect(w.gizmos, isNull);
      expect(w.showGizmos, isFalse);

      w.showGizmos = true;
      w.frame(1 / 60);
      expect(w.gizmos!.lineCount, 96);
      expect(w.gizmos!.lines[6], 1, reason: 'red');
      expect(w.gizmos!.lines[7], 0);
      w.frame(1 / 60);
      expect(w.gizmos!.lineCount, 96, reason: 'cleared, then drawn again — not accumulated');

      w.showGizmos = false;
      w.frame(1 / 60);
      expect(w.gizmos, isNull);
    });
  });
}
