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
          transform: Transform(position: Vec3(x, 0, 0), lookAt: Vec3(x + 1, 0, 1)),
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
    expect((Mesh.sphere().vertexCount, Mesh.sphere().indexCount), (49 * 25, 48 * 23 * 2 * 3));
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
