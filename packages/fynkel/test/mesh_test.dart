import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';
import 'render_test.dart' show stage1;

Matcher throwsFynkel(FynkelStatus status, [Pattern? message]) => throwsA(
  isA<FynkelException>()
      .having((e) => e.status, 'status', status)
      .having((e) => e.message, 'message', message == null ? anything : contains(message)),
);

/// A pyramid with a vertex per face corner, so its faces are flat.
Mesh pyramid() {
  const apex = Vec3(0, 0.6, 0);
  const base = [Vec3(-0.5, -0.4, 0.5), Vec3(0.5, -0.4, 0.5), Vec3(0.5, -0.4, -0.5), Vec3(-0.5, -0.4, -0.5)];
  final positions = [
    for (var i = 0; i < 4; i++) ...[base[i], base[(i + 1) % 4], apex],
    ...[base[0], base[3], base[2], base[0], base[2], base[1]],
  ];
  return Mesh.custom(positions: positions, indices: [for (var i = 0; i < positions.length; i++) i]);
}

/// An octahedron whose six vertices are shared, so it's shaded smoothly.
Mesh octahedron() => Mesh.custom(
  positions: const [
    Vec3(0.6, 0, 0),
    Vec3(-0.6, 0, 0),
    Vec3(0, 0.6, 0),
    Vec3(0, -0.6, 0),
    Vec3(0, 0, 0.6),
    Vec3(0, 0, -0.6),
  ],
  indices: const [0, 2, 4, 4, 2, 1, 1, 2, 5, 5, 2, 0, 4, 3, 0, 1, 3, 4, 5, 3, 1, 0, 3, 5],
);

void main() {
  test('meshes made in Dart render to the same golden image as the Rust test', () async {
    final scene = stage1();
    addTearDown(scene.world.dispose);
    scene.cube.destroy();
    // One made here, one on a helper isolate: the same result.
    final meshes = [(pyramid(), -0.8), (await Mesh.inBackground(octahedron), 0.8)];
    for (final (mesh, x) in meshes) {
      scene.world.add(
        GameObject(
          transform: Transform(position: Vec3(x, 0, 0), rotation: Quat.lookRotation(Vec3(1, 0, 1))),
          components: [MeshRenderer(mesh, scene.orange)],
        ),
      );
    }
    final view = HeadlessView(scene.world, width: 256, height: 160);
    addTearDown(view.dispose);
    expectGolden('custom_meshes', view.read(view.frame()).rgba, 256, 160);
  });

  test('a broken mesh throws where it is made, saying what is wrong', () {
    const triangle = [Vec3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 1, 0)];
    expect(
      () => Mesh.custom(positions: triangle, indices: [0, 1, 9]),
      throwsFynkel(FynkelStatus.invalidValue, 'index 2 is 9, but there are only 3 vertices'),
    );
    expect(
      () => Mesh.custom(positions: triangle, indices: [0, 1]),
      throwsFynkel(FynkelStatus.invalidValue, 'in threes'),
    );
    expect(
      () => Mesh.custom(positions: [...triangle.take(2), Vec3(double.nan, 0, 0)], indices: [0, 1, 2]),
      throwsFynkel(FynkelStatus.invalidValue, "position 2 isn't finite"),
    );
    expect(() => Mesh.raw(Float32List(4), indices: Uint32List(3)), throwsArgumentError);
    expect(() => Mesh.raw(Float32List(9), uvs: Float32List(4), indices: Uint32List(3)), throwsArgumentError);
  });

  test('a mirrored texture adds vertices where the tangents split', () {
    final mesh = Mesh.custom(
      positions: const [Vec3(-1, 1, 0), Vec3(0, 1, 0), Vec3(1, 1, 0), Vec3(-1, 0, 0), Vec3(0, 0, 0), Vec3(1, 0, 0)],
      uvs: const [Vec2(0, 0), Vec2(1, 0), Vec2(0, 0), Vec2(0, 1), Vec2(1, 1), Vec2(0, 1)],
      indices: const [3, 4, 1, 3, 1, 0, 4, 5, 2, 4, 2, 1],
    );
    expect((mesh.vertexCount, mesh.indexCount), (8, 12));
    expect((Mesh.sphere().vertexCount, Mesh.sphere().indexCount), (25 * 17, 24 * 15 * 2 * 3));
  });

  test('the built-in shapes: the same mesh each time, of Unity\'s sizes', () {
    for (final make in [Mesh.cube, Mesh.sphere, Mesh.capsule, Mesh.cylinder, Mesh.plane, Mesh.quad]) {
      expect(make(), same(make()));
    }
    (Vec3, Vec3) bounds(Mesh mesh) {
      final p = mesh.positions;
      var (lo, hi) = (Vec3.one * 1e9, Vec3.one * -1e9);
      for (var i = 0; i < p.length; i += 3) {
        final v = Vec3(p[i], p[i + 1], p[i + 2]);
        lo = Vec3(math.min(lo.x, v.x), math.min(lo.y, v.y), math.min(lo.z, v.z));
        hi = Vec3(math.max(hi.x, v.x), math.max(hi.y, v.y), math.max(hi.z, v.z));
      }
      return (lo, hi);
    }

    Matcher near(Vec3 v) => predicate<Vec3>((got) => (got - v).length < 1e-5, '$v');
    for (final (mesh, half) in [
      (Mesh.capsule(), const Vec3(0.5, 1, 0.5)),
      (Mesh.cylinder(), const Vec3(0.5, 1, 0.5)),
      (Mesh.quad(), const Vec3(0.5, 0.5, 0)),
    ]) {
      final (lo, hi) = bounds(mesh);
      expect(lo, near(-half), reason: '$mesh');
      expect(hi, near(half), reason: '$mesh');
    }
    expect(Mesh.sphere().indexCount ~/ 3, 720, reason: 'as light as Unity\'s sphere');
  });

  test('a mesh reads back its positions and triangles, built-in shapes too', () {
    const given = [Vec3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 1, 0), Vec3(0, 0, 1)];
    const triangles = [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
    final mesh = Mesh.custom(positions: given, indices: triangles);
    final (positions, indices) = (mesh.positions, mesh.indices);
    expect(positions.length, mesh.vertexCount * 3);
    expect(indices.length, triangles.length);
    for (var i = 0; i < triangles.length; i++) {
      final at = indices[i] * 3;
      expect(Vec3(positions[at], positions[at + 1], positions[at + 2]), given[triangles[i]]);
    }
    expect(Mesh.cube().indices.length, 36);
    expect(Mesh.cube().positions.every((p) => p.abs() == 0.5), isTrue);
    mesh.dispose();
    expect(() => mesh.positions, throwsStateError);
  });

  test('one mesh in two worlds, and a big one built in the background', () async {
    // A 128 × 128 terrain: about 33,000 vertices with UVs, so tangents are
    // generated, off the test's own isolate.
    const n = 128;
    final terrain = await Mesh.inBackground(() {
      final positions = Float32List((n + 1) * (n + 1) * 3);
      final uvs = Float32List((n + 1) * (n + 1) * 2);
      for (var z = 0; z <= n; z++) {
        for (var x = 0; x <= n; x++) {
          final i = z * (n + 1) + x;
          positions.setAll(i * 3, [x.toDouble(), math.sin(x / 7) * math.cos(z / 5), z.toDouble()]);
          uvs.setAll(i * 2, [x / n, z / n]);
        }
      }
      final indices = Uint32List(n * n * 6);
      for (var z = 0, k = 0; z < n; z++) {
        for (var x = 0; x < n; x++, k += 6) {
          final a = z * (n + 1) + x, b = a + n + 1;
          indices.setAll(k, [a, b, a + 1, a + 1, b, b + 1]);
        }
      }
      return Mesh.raw(positions, uvs: uvs, indices: indices);
    });
    expect(terrain.vertexCount, (n + 1) * (n + 1));

    final a = World(), b = World();
    addTearDown(a.dispose);
    addTearDown(b.dispose);
    final material = Material();
    final inA = a.add(GameObject(components: [MeshRenderer(terrain, material)]));
    final inB = b.add(GameObject(components: [MeshRenderer(terrain, material)]));
    expect(inA.get<MeshRenderer>()!.mesh, same(terrain));
    expect(inB.get<MeshRenderer>()!.mesh, same(terrain));
  });
}
