// Code first, Tier 2 (docs/code-first.md, ADR-0026 §4): the built-in
// shapes from code — `world.createPrimitive(Primitive.cube)`, Unity's
// `GameObject.CreatePrimitive` — in a project that never installed them
// (every cook ships `Fynkel/Primitives.glb`), with Unity's colliders; a
// plane's mesh collider, added from code, holds a falling cube. Over the
// real native library.

import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-code-first-primitives-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.loadProject(fixture, cacheDir: cache);
    // An empty stage: the fixture scene's floor would hold the cube too.
    for (final o in w.findObjectsWith<Collider>()) {
      o.destroy();
    }
    w.frame(1 / 60);
    return w;
  }

  test('the cook ships the shapes to a project that never installed them', () {
    final w = loaded();
    expect(Directory('$fixture/assets/Fynkel').existsSync(), isFalse);
    expect(w.assets.find('Fynkel/Primitives.glb'), const AssetRef(Primitive.id));
  });

  test('a cube falls onto a plane: the mesh, the material and Unity\'s colliders', () {
    final diagnostics = <FynkelDiagnostic>[];
    final w = loaded()..onDiagnostics = diagnostics.addAll;
    final plane = w.createPrimitive(Primitive.plane);
    final cube = w.createPrimitive(Primitive.cube, transform: const Pose(position: Vec3(1, 3, -2)))
      ..add(const RigidBody());
    expect(cube.name, 'Cube');
    expect(cube.get<MeshRenderer>()!.mesh, const AssetRef(Primitive.id, sub: 'mesh/Cube'));
    expect(cube.get<MeshRenderer>()!.material, Primitive.defaultMaterial);
    expect(cube.get<Collider>()!.shape, ColliderShape.box);
    expect(plane.get<Collider>()!.shape, ColliderShape.mesh);

    for (var i = 0; i < 240; i++) {
      w.frame(1 / 60);
    }
    expect(diagnostics, isEmpty, reason: 'the plane\'s mesh collider was registered for it');
    expect(cube.localTransform.position.y, closeTo(0.5, 0.05), reason: 'resting on the plane');
    expect(cube.bodyState()!.sleeping, isTrue);
  });

  test('every shape; a spec replaces the look, keeps the collider', () {
    final w = loaded();
    for (final p in Primitive.values) {
      final o = w.createPrimitive(p, name: 'My ${p.label}');
      expect(o.name, 'My ${p.label}');
      expect(o.get<MeshRenderer>()!.mesh.sub, 'mesh/${p.label}');
    }
    const red = AssetRef('c0000000-0000-4000-8000-000000000020'); // materials/rusted
    final ball = w.build(
      ObjectSpec.primitive(
        Primitive.sphere,
        components: [
          MeshRenderer(mesh: Primitive.sphere.mesh, material: red),
          const RigidBody(mass: 2),
        ],
      ),
    );
    w.frame(1 / 60);
    expect(ball.get<MeshRenderer>()!.material, red);
    expect(ball.get<Collider>()!.shape, ColliderShape.sphere);
    expect(ball.get<RigidBody>()!.mass, 2);
  });
}
