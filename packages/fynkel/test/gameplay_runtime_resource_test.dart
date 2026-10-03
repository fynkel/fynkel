import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

MeshData ground(double height) =>
    MeshData.grid(width: 10, depth: 10, xSegments: 1, zSegments: 1, height: (_, _) => height);

void main() {
  late Directory cache;
  late NativeLibrary library;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-runtime-resources-');
    cookProject(Directory('../../crates/fynkel_document/fixtures/project').absolute.path, cacheDir: cache.path);
    library = NativeLibrary.open();
  });
  tearDownAll(() {
    library.dispose();
    cache.deleteSync(recursive: true);
  });
  World fresh() {
    final world = World.create(library: library);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    world.loadScene('c0000000-0000-4000-8000-000000000034', cacheDir: cache.path);
    return world;
  }

  test('world disposal invalidates runtime resources and releases SDK descriptors', () {
    final world = fresh();
    final material = world.createMaterial();
    final mesh = world.createMesh(ground(2));
    final clip = world.createClip((ClipBuilder(length: 1)..position('joint', {0: Vec3.zero, 1: Vec3.one})).build());
    world.dispose();
    expect(material.isAlive, isFalse);
    expect(mesh.isAlive, isFalse);
    expect(clip.isAlive, isFalse);
    expect(world.runtimeMaterial(material), isNull);
    expect(world.runtimeMesh(mesh), isNull);
    world.dispose();
  });

  test('native host disposal also invalidates runtime asset handles', () {
    final world = fresh();
    final material = world.createMaterial();
    final mesh = world.createMesh(ground(2));
    final clip = world.createClip((ClipBuilder(length: 1)..position('joint', {0: Vec3.zero, 1: Vec3.one})).build());
    world.native.dispose();
    expect(material.isAlive, isFalse);
    expect(mesh.isAlive, isFalse);
    expect(clip.isAlive, isFalse);
  });

  test('callback disposal invalidates assets only after successful commit', () {
    final world = fresh();
    final material = world.createMaterial();
    expect(
      () => world.frame(
        0,
        update: () {
          world.dispose();
          expect(material.isAlive, isTrue);
          throw StateError('abort');
        },
      ),
      throwsStateError,
    );
    expect(world.isDisposed, isFalse);
    expect(material.isAlive, isTrue);
    world.frame(
      0,
      update: () {
        world.dispose();
        expect(material.isAlive, isTrue);
      },
    );
    expect(world.isDisposed, isTrue);
    expect(material.isAlive, isFalse);
  });

  test('material and mesh liveness, values and objects roll back together', () {
    final world = fresh();
    final before = world.spawn(name: 'before');
    late MaterialAsset material;
    late MeshAsset mesh;
    expect(
      () => world.edit(() {
        material = world.createMaterial();
        mesh = world.createMesh(ground(2));
        expect(material.isAlive, isTrue);
        expect(mesh.isAlive, isTrue);
        expect(world.runtimeMesh(mesh)!.positions.first.y, 2);
        before.name = 'discard';
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(material.isAlive, isFalse);
    expect(mesh.isAlive, isFalse);
    expect(before.name, 'before');
    expect(world.runtimeMaterial(material), isNull);
    expect(world.runtimeMesh(mesh), isNull);
  });
  test('new mesh and collider in one callback reach the next tick at four frame rates', () {
    for (final hz in [30, 60, 120, 240]) {
      final world = fresh();
      late GameObject terrain;
      world.frame(
        0,
        update: () {
          final mesh = world.createMesh(ground(2));
          final material = world.createMaterial();
          terrain = world.spawn(name: 'terrain')
            ..set(MeshRenderer(mesh: mesh, material: material))
            ..set(Collider(shape: ColliderShape.mesh, mesh: mesh));
          expect(world.raycast(const Vec3(0, 10, 0), const Vec3(0, -1, 0)), isNull);
        },
      );

      for (var i = 0; i < hz ~/ 10; i++) {
        world.frame(1 / hz);
      }
      final hit = world.raycast(const Vec3(0, 10, 0), const Vec3(0, -1, 0))!;
      expect(hit.entity, terrain.entity);
      expect(hit.point.y, closeTo(2, 1e-5));
    }
  });
  test('mesh replacement rollback preserves collision geometry and final replacement wins', () {
    final world = fresh();
    final mesh = world.createMesh(ground(2));
    world.spawn().set(Collider(shape: ColliderShape.mesh, mesh: mesh));
    world.frame(1 / 60);
    expect(
      () => world.edit(() {
        world.setMesh(mesh, ground(7));
        throw StateError('abort');
      }),
      throwsStateError,
    );
    world.frame(1 / 60);
    expect(world.raycast(const Vec3(0, 10, 0), const Vec3(0, -1, 0))!.point.y, closeTo(2, 1e-5));
    world.edit(() {
      world.setMesh(mesh, ground(4));
      world.setMesh(mesh, ground(6));
    });
    world.frame(1 / 60);
    expect(world.raycast(const Vec3(0, 10, 0), const Vec3(0, -1, 0))!.point.y, closeTo(6, 1e-5));
  });
  test('caught invalid material is sticky and preserves committed resources', () {
    final world = fresh();
    final material = world.createMaterial();
    final mesh = world.createMesh(ground(2));
    expect(
      () => world.edit(() {
        world.destroyMesh(mesh);
        try {
          world.setMaterial(material, const PbrMaterial(roughness: double.nan));
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(material.isAlive, isTrue);
    expect(mesh.isAlive, isTrue);
    expect(world.runtimeMaterial(material)!.roughness.isFinite, isTrue);
  });
  test('create then destroy and aborted removal preserve resource ownership', () {
    final world = fresh();
    final material = world.createMaterial();
    final mesh = world.createMesh(ground(2));
    expect(
      () => world.edit(() {
        material.destroy();
        mesh.destroy();
        expect(mesh.isAlive, isFalse);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(material.isAlive, isTrue);
    expect(mesh.isAlive, isTrue);
    world.edit(() {
      final gone = world.createMesh(ground(1));
      gone.destroy();
      expect(gone.isAlive, isFalse);
      material.destroy();
      mesh.destroy();
    });
    expect(material.isAlive, isFalse);
    expect(mesh.isAlive, isFalse);
  });
}
