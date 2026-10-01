// Code first, Tier 2 (docs/code-first.md, ADR-0026 §4): a mesh made in
// code — `world.createMesh(MeshData(...))`, Unity's `new Mesh()` — taken
// by a `MeshRenderer` and a mesh `Collider` like a cooked one, changed
// with `setMesh`, kept across scene loads. Over the real native library
// (the store's side: crates/fynkel_native/tests/load.rs; the pixels:
// crates/fynkel_render/tests/project.rs).

import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-code-first-meshes-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    return w;
  }

  /// A 10 m square of ground at [height] — terrain from a height field.
  MeshData ground(double height) =>
      MeshData.grid(width: 10, depth: 10, xSegments: 4, zSegments: 4, height: (x, z) => height + 0.1 * x);

  test('a made mesh drawn and stood on, changed, kept across a scene load, destroyed', () {
    final w = fresh()..loadProject(fixture, cacheDir: cache);
    final mesh = w.createMesh(ground(1));
    expect(mesh.asset[14], '4', reason: 'a v4 UUID');
    final terrain = w.build(
      ObjectSpec(
        'Terrain',
        components: [
          MeshRenderer(
            mesh: mesh,
            material: w.createMaterial(const PbrMaterial(baseColor: Vec4(0.3, 0.6, 0.2, 1))),
          ),
          Collider(shape: ColliderShape.mesh, mesh: mesh),
        ],
      ),
    );
    w.frame(1 / 60);
    expect(terrain.get<MeshRenderer>()!.mesh, mesh);

    // Standing on it: a ray down meets the height field (y = 1 + 0.1x).
    var hit = w.raycast(const Vec3(2, 10, 0), const Vec3(0, -1, 0))!;
    expect(hit.entity, terrain.entity);
    expect(hit.point.y, closeTo(1.2, 1e-4));
    expect(hit.normal.y, greaterThan(0.99), reason: 'the front faces up');

    // New geometry: the renderer and the collider follow.
    w.setMesh(mesh, ground(3));
    w.frame(1 / 60);
    hit = w.raycast(const Vec3(2, 10, 0), const Vec3(0, -1, 0))!;
    expect(hit.point.y, closeTo(3.2, 1e-4));
    expect(w.runtimeMesh(mesh)!.positions.first.y, closeTo(2.5, 1e-9));

    w.scenes.transition(w.assets.find('scenes/playground')!);
    w.frame(1 / 60);
    w.setMesh(mesh, w.runtimeMesh(mesh)!); // still there: no throw

    // Removed under a collider still naming it: nothing to stand on, and
    // the collider says why, as for any mesh that is not there.
    final again = w.createMesh(ground(0));
    final floor = w.build(
      ObjectSpec(
        'Floor',
        components: [Collider(shape: ColliderShape.mesh, mesh: again)],
      ),
    );
    w.frame(1 / 60);
    expect(w.raycast(const Vec3(2, 10, 0), const Vec3(0, -1, 0))?.entity, floor.entity);
    w.destroyMesh(again);
    expect(
      () => w.frame(1 / 60),
      throwsA(isA<FynkelError>().having((e) => '$e', 'message', contains('meshUnavailable'))),
    );
    expect(w.raycast(const Vec3(2, 10, 0), const Vec3(0, -1, 0))?.entity, isNot(floor.entity));
    expect(w.runtimeMesh(again), isNull);
  });

  test('normals and tangents are computed when not given', () {
    final grid = MeshData.grid(width: 2, depth: 2);
    final normals = grid.resolvedNormals();
    expect(normals, everyElement(const Vec3(0, 1, 0)));
    for (final t in grid.resolvedTangents(normals)) {
      expect((t.x, t.y, t.z), (1.0, 0.0, 0.0), reason: 'along +u, which runs along +x');
      expect(t.w.abs(), 1);
    }
    final noUv = MeshData(
      positions: const [Vec3.zero, Vec3(1, 0, 0), Vec3(0, 1, 0)],
      triangles: const [0, 1, 2],
    );
    expect(noUv.resolvedNormals(), everyElement(const Vec3(0, 0, 1)), reason: 'counter-clockwise from +z');
    expect(noUv.resolvedTangents(noUv.resolvedNormals()), everyElement(const Vec4(0, 0, 0, 0)));

    // The payload: 32-byte header, 72 bytes a vertex, 4 an index.
    final bytes = ByteData.sublistView(noUv.toPayload());
    expect(bytes.lengthInBytes, 32 + 3 * 72 + 3 * 4);
    expect((bytes.getUint32(0, Endian.little), bytes.getUint32(4, Endian.little)), (3, 3));
    expect(
      bytes.getFloat32(32 + 72 + 56, Endian.little),
      1,
      reason: "the second vertex's colour defaults to white",
    );
  });

  test('what cannot be a mesh throws', () {
    const tri = [Vec3.zero, Vec3(1, 0, 0), Vec3(0, 1, 0)];
    expect(() => MeshData(positions: tri, triangles: const []), throwsArgumentError);
    expect(() => MeshData(positions: tri, triangles: const [0, 1]), throwsArgumentError);
    expect(() => MeshData(positions: tri, triangles: const [0, 1, 3]), throwsArgumentError);
    expect(
      () => MeshData(positions: tri, triangles: const [0, 1, 2], uvs: const [Vec2.zero]),
      throwsArgumentError,
    );
    expect(
      () => MeshData(
        positions: const [Vec3(double.nan, 0, 0), Vec3.one, Vec3.zero],
        triangles: const [0, 1, 2],
      ),
      throwsArgumentError,
    );
    expect(() => MeshData.grid(xSegments: 0), throwsArgumentError);

    final w = fresh();
    expect(
      () => w.createMesh(MeshData(positions: tri, triangles: const [0, 1, 2])),
      throwsStateError,
      reason: 'no asset store before a load',
    );
  });
}
