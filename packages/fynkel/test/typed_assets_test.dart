// Typed assets (ADR-0036 §3): `Assets.materials.gold` is a
// `MaterialAsset`, an Object that is also the `AssetRef` every API takes,
// so a field of that type only accepts that kind of asset. One made in
// code is destroyed through its own `destroy()`, as Unity's
// `Destroy(material)`. Over the real native library for the made ones.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const gold = '1a765874-6dba-512c-84ec-74c6ac87d1b6';
const kit = 'c0000000-0000-4000-8000-000000000011';

void main() {
  test('a typed asset is an Asset and the AssetRef it names', () {
    const m = MaterialAsset(gold, name: 'gold');
    expect(m, isA<Asset>());
    expect(m, isA<AssetRef>());
    expect((m.name, m.ref, m.isAlive), ('gold', const AssetRef(gold), true));
    expect(
      m == const AssetRef(gold) && const AssetRef(gold) == m,
      isTrue,
      reason: 'the same asset either way',
    );
    expect(m.kind, 'material');
    expect(
      () => m.destroy(),
      throwsUnsupportedError,
      reason: 'a project asset cannot be destroyed from a script',
    );

    const cube = MeshAsset(kit, sub: 'mesh/cube');
    expect((cube.name, cube.kind), ('cube', 'mesh'));
    expect(MeshAsset.fromRef(const AssetRef(kit, sub: 'mesh/cube')), cube);
    expect(
      [
        const TextureAsset(kit),
        const PrefabAsset(kit),
        const SceneAsset(kit),
        const AudioClipAsset(kit),
        const AnimationClipAsset(kit),
        const SkinAsset(kit),
        const ShaderAsset(kit),
        const AnimatorControllerAsset(kit),
        const VolumeProfileAsset(kit),
        const PhysicsMaterialAsset(kit),
        const RigAsset(kit),
      ].map((a) => a.kind),
      [
        'texture',
        'prefab',
        'scene',
        'audio',
        'clip',
        'skin',
        'shader',
        'controller',
        'volume',
        'physics_material',
        'rig',
      ],
    );
  });

  group('made in code', () {
    final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
    late String cache;
    late NativeLibrary lib;
    setUpAll(() {
      cache = Directory.systemTemp.createTempSync('fynkel-typed-assets-').path;
      cookProject(fixture, cacheDir: cache);
      lib = NativeLibrary.open();
    });
    tearDownAll(() {
      Directory(cache).deleteSync(recursive: true);
      lib.dispose();
    });

    test('a made material, mesh and clip are typed, alive until destroyed', () {
      final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)
        ..loadProject(fixture, cacheDir: cache);
      addTearDown(w.dispose);
      final MaterialAsset red = w.createMaterial(const PbrMaterial(baseColor: Vec4(1, 0, 0, 1)));
      final MeshAsset tri = w.createMesh(
        MeshData(positions: const [Vec3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 1, 0)], triangles: const [0, 1, 2]),
      );
      final AnimationClipAsset clip = w.createClip(
        (ClipBuilder(length: 1)..position('Box', {0: Vec3.zero, 1: Vec3.one})).build(),
      );
      for (final a in <Asset>[red, tri, clip]) {
        expect(a.isAlive, isTrue);
        a.destroy();
        expect(a.isAlive, isFalse);
      }
      expect(w.runtimeMaterial(red), isNull, reason: 'destroy() is destroyMaterial');
      expect(w.runtimeMesh(tri), isNull);
    });
  });
}
