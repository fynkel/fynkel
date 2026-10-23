import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';
import 'render_test.dart' show stage1;

/// What the engine refusing with [status] throws in Dart: an
/// [ArgumentError] for a bad value, a [StateError] for something gone or in
/// use, a [FynkelException] for the engine's own failures.
Matcher throwsRefusal(FynkelStatus status, [Pattern? message]) => throwsA(
  switch (status) {
    FynkelStatus.invalidValue => isA<ArgumentError>(),
    FynkelStatus.noObject ||
    FynkelStatus.noComponent ||
    FynkelStatus.noMesh ||
    FynkelStatus.noMaterial ||
    FynkelStatus.noTexture ||
    FynkelStatus.inUse => isA<StateError>(),
    _ => isA<FynkelException>().having((e) => e.status, 'status', status),
  }.having((e) => '$e', 'text', message == null ? anything : contains(message)),
);

/// A file the engine can't read: a [FormatException] saying why.
Matcher throwsFormat(String message) =>
    throwsA(isA<FormatException>().having((e) => e.message, 'message', contains(message)));

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
      throwsRefusal(FynkelStatus.invalidValue, 'index 2 is 9, but there are only 3 vertices'),
    );
    expect(
      () => Mesh.custom(positions: triangle, indices: [0, 1]),
      throwsRefusal(FynkelStatus.invalidValue, 'in threes'),
    );
    expect(
      () => Mesh.custom(positions: [...triangle.take(2), Vec3(double.nan, 0, 0)], indices: [0, 1, 2]),
      throwsRefusal(FynkelStatus.invalidValue, "position 2 isn't finite"),
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

  group('a mesh that changes', () {
    // As the Rust test's: a grid of 16 × 16 squares a metre across, its
    // triangles the far half's then the near half's.
    (Float32List, Uint32List) grid(double Function(double x, double z) height) {
      const n = 16;
      final positions = Float32List((n + 1) * (n + 1) * 3);
      for (var z = 0, i = 0; z <= n; z++) {
        for (var x = 0; x <= n; x++, i += 3) {
          final (px, pz) = (x / n - 0.5, z / n - 0.5);
          positions
            ..[i] = px
            ..[i + 1] = height(px, pz)
            ..[i + 2] = pz;
        }
      }
      final indices = Uint32List(n * n * 6);
      for (var z = 0, k = 0; z < n; z++) {
        for (var x = 0; x < n; x++, k += 6) {
          final i = z * (n + 1) + x;
          indices.setAll(k, [i, i + n + 1, i + 1, i + 1, i + n + 1, i + n + 2]);
        }
      }
      return (positions, indices);
    }

    double bump(double x, double z) => 0.4 * math.exp(-(x * x + z * z) * 12);

    test('made from nothing in Dart, it draws as the Rust test\'s, and each frame\'s changes go up together', () {
      final scene = stage1();
      addTearDown(scene.world.dispose);
      scene.cube.destroy();
      final (positions, indices) = grid(bump);
      final count = positions.length ~/ 3;
      // White low down, orange at the top.
      final colors = Uint8List(count * 4);
      for (var v = 0; v < count; v++) {
        final h = positions[v * 3 + 1] / 0.4;
        colors.setAll(v * 4, [255, (255 - h * 135).toInt(), (255 - h * 255).toInt(), 255]);
      }
      final mesh = Mesh()
        ..markDynamic()
        ..setPositions(positions)
        ..setColors(colors)
        ..setTriangles(indices, submeshStarts: Uint32List.fromList([indices.length ~/ 2]));
      expect((mesh.vertexCount, mesh.indexCount, mesh.submeshCount), (count, indices.length, 2));
      expect(mesh.submesh(1), (start: indices.length ~/ 2, count: indices.length ~/ 2));
      final white = Material(color: Color.white, roughness: 0.7);
      final blue = Material(color: Color.rgb(0.4, 0.6, 1), roughness: 0.7);
      final renderer = MeshRenderer(mesh)..materials = [white, blue];
      expect(renderer.materials, [white, blue]);
      expect(renderer.material, same(white));
      scene.world.add(
        GameObject(
          transform: Transform(position: Vec3(0, -0.3, 0), scale: Vec3.one * 2.2),
          components: [renderer],
        ),
      );
      final view = HeadlessView(scene.world, width: 256, height: 160);
      addTearDown(view.dispose);
      scene.world.update(1 / 60);
      expectGolden('mesh_changed_before', view.read(view.frame()).rgba, 256, 160);

      // The same list, filled again: a dent where the bump was. Not drawn
      // until the frame's end.
      final (dented, _) = grid((x, z) => -bump(x, z) * 0.6);
      positions.setAll(0, dented);
      mesh.setPositions(positions);
      expect(mesh.positions, positions, reason: 'reading back sees the change at once');
      expectGolden('mesh_changed_before', view.read(view.frame()).rgba, 256, 160);
      scene.world.update(1 / 60);
      expectGolden('mesh_changed_after', view.read(view.frame()).rgba, 256, 160);
      expect(mesh.colors, colors, reason: 'kept');
      expect(() => scene.world.update(0), returnsNormally);
    });

    test('normals and tangents are worked out until the game sets its own', () {
      final (positions, indices) = grid((x, z) => x);
      final mesh = Mesh()
        ..setPositions(positions)
        ..setTriangles(indices);
      // A slope rising along +x: its normal leans back along -x.
      final n = mesh.normals;
      expect(n.length, positions.length);
      expect(n[0], closeTo(-math.sqrt1_2, 1e-5));
      expect(n[1], closeTo(math.sqrt1_2, 1e-5));
      // Its own from now on: moving it doesn't change them.
      final up = Float32List(positions.length);
      for (var i = 1; i < up.length; i += 3) {
        up[i] = 1;
      }
      mesh.setNormals(up);
      final (flat, _) = grid((x, z) => 0);
      mesh.setPositions(flat);
      expect(mesh.normals, up);
      mesh.recalculateNormals();
      expect(mesh.normals, up, reason: 'flat now, so up anyway');
      mesh.setPositions(positions);
      mesh.recalculateNormals();
      expect(mesh.normals[0], closeTo(-math.sqrt1_2, 1e-5));
    });

    test('a refused change throws and changes nothing; a built-in shape can\'t change', () {
      final (positions, indices) = grid(bump);
      final mesh = Mesh()
        ..setPositions(positions)
        ..setTriangles(indices);
      expect(
        () => mesh.setTriangles(Uint32List.fromList([0, 1, 9999])),
        throwsRefusal(FynkelStatus.invalidValue, 'index 2 is 9999'),
      );
      expect(() => mesh.setPositions(Float32List(4)), throwsArgumentError);
      expect(() => mesh.setColors(Uint8List(8)), throwsRefusal(FynkelStatus.invalidValue, 'colors has 2 items'));
      expect(
        () => mesh.setTriangles(indices, submeshStarts: Uint32List.fromList([7])),
        throwsRefusal(FynkelStatus.invalidValue, 'submesh 1 starts at index 7'),
      );
      expect(mesh.indices, indices);
      expect(() => Mesh.cube().setPositions(positions), throwsStateError);
      mesh.clear();
      expect((mesh.vertexCount, mesh.indexCount), (0, 0));
    });
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
