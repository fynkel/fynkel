import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'mesh_test.dart' show throwsRefusal;

/// The box around a mesh's positions: (min, max).
(Vec3, Vec3) boundsOf(Mesh mesh) {
  final p = mesh.positions;
  var (lo, hi) = (Vec3.all(double.infinity), Vec3.all(-double.infinity));
  for (var i = 0; i < p.length; i += 3) {
    lo = Vec3(math.min(lo.x, p[i]), math.min(lo.y, p[i + 1]), math.min(lo.z, p[i + 2]));
    hi = Vec3(math.max(hi.x, p[i]), math.max(hi.y, p[i + 1]), math.max(hi.z, p[i + 2]));
  }
  return (lo, hi);
}

Matcher near(Vec3 v) => isA<Vec3>().having((a) => (a - v).length, 'distance from $v', lessThan(1e-4));

void main() {
  group('shapes with settings', () {
    test('the shapes without settings are the shared ones; with any, new ones', () {
      expect(identical(Mesh.sphere(), Mesh.sphere()), isTrue);
      expect(identical(Mesh.cylinder(), Mesh.cylinder()), isTrue);
      expect(identical(Mesh.capsule(), Mesh.capsule()), isTrue);
      final big = Mesh.sphere(radius: 2);
      expect(identical(big, Mesh.sphere()), isFalse);
      expect(big.vertexCount, Mesh.sphere().vertexCount);
      final (lo, hi) = boundsOf(big);
      expect(lo, near(const Vec3.all(-2)));
      expect(hi, near(const Vec3.all(2)));
    });

    test('each shape has the size it was given', () {
      for (final (mesh, lo, hi) in [
        (Mesh.box(size: const Vec3(4, 2, 1), segmentsX: 4), const Vec3(-2, -1, -0.5), const Vec3(2, 1, 0.5)),
        (Mesh.grid(width: 6, depth: 4), const Vec3(-3, 0, -2), const Vec3(3, 0, 2)),
        (Mesh.capsule(radius: 0.25, height: 3), const Vec3(-0.25, -1.5, -0.25), const Vec3(0.25, 1.5, 0.25)),
        (Mesh.cylinder(radius: 1, height: 0.5), const Vec3(-1, -0.25, -1), const Vec3(1, 0.25, 1)),
        (Mesh.cone(radius: 1, height: 2), const Vec3(-1, -1, -1), const Vec3(1, 1, 1)),
        (Mesh.torus(radius: 1, tube: 0.25), const Vec3(-1.25, -0.25, -1.25), const Vec3(1.25, 0.25, 1.25)),
        (Mesh.disc(radius: 2), const Vec3(-2, 0, -2), const Vec3(2, 0, 2)),
      ]) {
        final (a, b) = boundsOf(mesh);
        expect(a, near(lo), reason: '$mesh');
        expect(b, near(hi), reason: '$mesh');
      }
    });

    test('segments cut a shape finer', () {
      expect(Mesh.grid(segmentsX: 4, segmentsZ: 2).indexCount, 4 * 2 * 6);
      expect(Mesh.grid(segmentsX: 8, segmentsZ: 2).indexCount, 8 * 2 * 6);
      expect(Mesh.cylinder(segments: 8, caps: false).indexCount, 8 * 6);
      expect(Mesh.disc(inner: 0.25, segments: 8).indexCount, 8 * 6);
    });

    test('a box textures by the metre', () {
      final uvs = Mesh.box(size: const Vec3(4, 1, 1)).uvs;
      final most = [for (var i = 0; i < uvs.length; i += 2) uvs[i]].reduce(math.max);
      expect(most, 4, reason: 'the 4 m long faces repeat the texture 4 times');
      final half = Mesh.box(size: const Vec3(4, 1, 1), uvPerMetre: 0.5).uvs;
      expect([for (var i = 0; i < half.length; i += 2) half[i]].reduce(math.max), 2);
    });

    test('a setting that makes no shape is refused, saying which', () {
      expect(() => Mesh.sphere(segments: 2), throwsRefusal(FynkelStatus.invalidValue, 'segments is 2'));
      expect(() => Mesh.box(size: const Vec3(1, 0, 1)), throwsRefusal(FynkelStatus.invalidValue, 'size.y is 0'));
      expect(() => Mesh.disc(radius: 1, inner: 2), throwsRefusal(FynkelStatus.invalidValue, 'inner is 2'));
    });
  });

  group('outlines', () {
    test('a shape with a hole is extruded into a closed slab', () {
      final frame = Shape2D.rect(2, 2).withHole(Shape2D.rect(1, 1));
      expect(frame.holes, hasLength(1));
      final slab = Mesh.extrude(frame, depth: 0.5);
      // 8 triangles on each end, 8 sides of 2.
      expect(slab.indexCount ~/ 3, 8 * 2 + 8 * 2);
      final (lo, hi) = boundsOf(slab);
      expect(lo, near(const Vec3(-1, -1, -0.25)));
      expect(hi, near(const Vec3(1, 1, 0.25)));
    });

    // Stage 14's WO9: a hole outside the outline was joined to it, its
    // triangles outside the slab, or quietly dropped.
    test('a hole outside the outline is refused', () {
      final apart = Shape2D.rect(2, 2).withHole(Shape2D.rect(1, 1, centre: const Vec2(-3, 0)));
      expect(() => Mesh.extrude(apart), throwsRefusal(FynkelStatus.invalidValue, "hole 1 isn't inside the outline"));
    });

    test('circles and rounded rectangles are outlines of the size asked for', () {
      final ring = Shape2D.circle(1, segments: 16);
      expect(ring.outline, hasLength(16));
      expect(ring.outline.every((p) => (p.length - 1).abs() < 1e-9), isTrue);
      final r = Shape2D.rounded(4, 2, 0.5, segments: 4);
      expect(r.outline, hasLength(4 * 5));
      final xs = r.outline.map((p) => p.x), ys = r.outline.map((p) => p.y);
      expect(xs.reduce(math.min), closeTo(-2, 1e-9));
      expect(xs.reduce(math.max), closeTo(2, 1e-9));
      expect(ys.reduce(math.min), closeTo(-1, 1e-9));
      expect(ys.reduce(math.max), closeTo(1, 1e-9));
      expect(() => Shape2D([Vec2.zero, Vec2.one]), throwsArgumentError);
    });

    test('a profile is lathed around Y', () {
      final cup = Mesh.lathe([const Vec2(0, 0), const Vec2(0.5, 0), const Vec2(0.5, 1)], sides: 12);
      // The base's 12 triangles to the middle, the side's 12 squares.
      expect(cup.indexCount ~/ 3, 12 + 24);
      final (lo, hi) = boundsOf(cup);
      expect(lo, near(const Vec3(-0.5, 0, -0.5)));
      expect(hi, near(const Vec3(0.5, 1, 0.5)));
      expect(
        () => Mesh.lathe([const Vec2(0, 0), const Vec2(-1, 1)]),
        throwsRefusal(FynkelStatus.invalidValue, 'left of the axis'),
      );
    });
  });

  group('combining', () {
    test('meshes are combined where they are placed, one part each across them', () {
      final town = Mesh.combine([
        for (var i = 0; i < 5; i++) CombineInstance(Mesh.cube(), position: Vec3(i * 2.0, 0, 0)),
      ]);
      expect(town.vertexCount, 5 * 24);
      expect(town.submeshCount, 1);
      final (lo, hi) = boundsOf(town);
      expect(lo, near(const Vec3(-0.5, -0.5, -0.5)));
      expect(hi, near(const Vec3(8.5, 0.5, 0.5)));
    });

    test('a turned and stretched part keeps its triangles facing out', () {
      final both = Mesh.combine([
        CombineInstance(Mesh.cube(), rotation: Quat.axisAngle(Vec3.up, 0.5), scale: const Vec3(2, 1, 1)),
        CombineInstance(Mesh.cube(), position: const Vec3(3, 0, 0), scale: const Vec3(-1, 1, 1)),
      ]);
      final (p, n, ix) = (both.positions, both.normals, both.indices);
      for (var t = 0; t < ix.length; t += 3) {
        Vec3 at(int k) => Vec3(p[ix[t + k] * 3], p[ix[t + k] * 3 + 1], p[ix[t + k] * 3 + 2]);
        final face = (at(1) - at(0)).cross(at(2) - at(0));
        final normal = Vec3(n[ix[t] * 3], n[ix[t] * 3 + 1], n[ix[t] * 3 + 2]);
        expect(face.dot(normal), greaterThan(0), reason: 'triangle ${t ~/ 3} faces its normal');
      }
    });

    test('nothing to combine is refused', () {
      expect(() => Mesh.combine([]), throwsArgumentError);
    });
  });

  group('a builder', () {
    test('builds flat faces with corners of their own, in the parts chosen', () {
      final b = MeshBuilder()
        ..addQuad(const Vec3(0, 0, 0), const Vec3(1, 0, 0), const Vec3(1, 1, 0), const Vec3(0, 1, 0))
        ..submesh(1)
        ..color = const Color.rgb(1, 0, 0)
        ..addTriangle(const Vec3(0, 1, 0), const Vec3(1, 1, 0), const Vec3(0.5, 2, 0));
      expect((b.vertexCount, b.triangleCount), (7, 3));
      final mesh = b.build();
      expect(mesh.submeshCount, 2);
      expect(mesh.submesh(0), (start: 0, count: 6));
      expect(mesh.submesh(1), (start: 6, count: 3));
      // Facing +Z, as wound; the triangle red.
      final n = mesh.normals;
      for (var i = 0; i < n.length; i += 3) {
        expect([n[i], n[i + 1], n[i + 2]], [0, 0, 1]);
      }
      final colors = mesh.colors;
      expect(colors.sublist(0, 4), [255, 255, 255, 255]);
      expect(colors.sublist(4 * 4, 4 * 5), [255, 0, 0, 255]);
    });

    test('shared vertices are smooth, unless the builder is flat', () {
      MeshBuilder tent({required bool flat}) {
        final b = MeshBuilder(flat: flat);
        final ridge = b.addVertex(const Vec3(0, 1, 0));
        final ridgeBack = b.addVertex(const Vec3(0, 1, -1));
        final left = b.addVertex(const Vec3(-1, 0, 0)), leftBack = b.addVertex(const Vec3(-1, 0, -1));
        final right = b.addVertex(const Vec3(1, 0, 0)), rightBack = b.addVertex(const Vec3(1, 0, -1));
        b
          ..addIndices(left, ridge, ridgeBack)
          ..addIndices(left, ridgeBack, leftBack)
          ..addIndices(ridge, right, rightBack)
          ..addIndices(ridge, rightBack, ridgeBack);
        return b;
      }

      final smooth = tent(flat: false).build();
      expect(smooth.vertexCount, 6);
      final n = smooth.normals;
      expect(n[1], greaterThan(0.9), reason: 'the ridge: between the two slopes, nearly straight up');

      final faceted = tent(flat: true).build();
      expect(faceted.vertexCount, 12, reason: 'three corners for each of 4 triangles');
      final f = faceted.normals;
      expect(f[1], closeTo(math.sqrt1_2, 1e-5), reason: 'each slope its own way');
    });

    test('a mesh added is placed, and a mirrored one stays facing out', () {
      final b = MeshBuilder()
        ..addMesh(Mesh.cube(), position: const Vec3(2, 0, 0))
        ..addMesh(Mesh.cube(), scale: const Vec3(-1, 1, 1));
      final mesh = b.build();
      expect(mesh.vertexCount, 48);
      final (lo, hi) = boundsOf(mesh);
      expect(lo, near(const Vec3(-0.5, -0.5, -0.5)));
      expect(hi, near(const Vec3(2.5, 0.5, 0.5)));
    });

    test('applying it again replaces what a mesh had, fewer vertices or more', () {
      final mesh = Mesh();
      final b = MeshBuilder();
      for (var n = 4; n >= 1; n--) {
        b.clear();
        for (var i = 0; i < n; i++) {
          final x = i.toDouble();
          b.addQuad(Vec3(x, 0, 0), Vec3(x + 1, 0, 0), Vec3(x + 1, 1, 0), Vec3(x, 1, 0));
        }
        b.applyTo(mesh);
        expect((mesh.vertexCount, mesh.indexCount), (n * 4, n * 6));
      }
      b
        ..clear()
        ..applyTo(mesh);
      expect(mesh.vertexCount, 0, reason: 'an empty builder empties it');
      expect(() => b.build(), throwsStateError);
    });

    test('a built mesh draws in a world as one object', () {
      final world = World();
      addTearDown(world.dispose);
      final b = MeshBuilder()
        ..addQuad(const Vec3(0, 0, 0), const Vec3(1, 0, 0), const Vec3(1, 1, 0), const Vec3(0, 1, 0));
      final house = GameObject(components: [MeshRenderer(b.build(), Material())]);
      world.add(house);
      world.update(1 / 60);
      expect(house.get<MeshRenderer>()!.mesh!.vertexCount, 4);
    });
  });
}
