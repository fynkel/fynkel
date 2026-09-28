// Runtime prefab instantiate (M3-08b, docs/design/dart-sdk.md §21):
// `World.instantiate` builds a cooked prefab now — its root, with the
// prefab's other entities under it, the same shape a scene instance expands
// to (ADR-0022).

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const cratePrefab = 'c0000000-0000-4000-8000-000000000040';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-prefab-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 4);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(fixture, cacheDir: cache);
    w.frame(1 / 60);
    return w;
  }

  test('instantiate builds the prefab\'s root at the given transform', () {
    final w = loaded();
    final before = w.allEntities().length;
    final crate = w.instantiate(
      cratePrefab,
      transform: const Pose(position: Vec3(2, 0, -3), rotation: Quat.identity, scale: Vec3.one),
    );
    final report = w.lastInstantiate!;
    expect(report.ok, isTrue);
    expect(report.prefabId, cratePrefab);
    expect(report.prefabPath, 'prefabs/crate.prefab.json');
    expect(report.name, 'Crate');
    expect(report.entityCount, 1, reason: 'the prefab\'s entities, its root included');
    expect(report.rejectedComponents, 0);
    expect(report.warnings, isEmpty);
    expect(crate.name, 'Crate');
    expect(crate.entity, report.root);
    expect(w.allEntities().length, before + 1, reason: 'the root is the whole crate');

    // The root is live now, at the given transform (it replaces the
    // prefab's); its components land at the next tick.
    expect(crate.localTransform.position, const Vec3(2, 0, -3));
    expect(crate.parent, isNull);
    w.frame(1 / 60);
    expect(crate.get<MeshRenderer>(), isNotNull);
    expect(crate.worldTransform!.position, const Vec3(2, 0, -3));
    expect(w.findAll('Crate').length, 1, reason: 'the scene\'s instance is named Crate A');
  });

  test('instantiate accepts an AssetRef, a name and a parent', () {
    final w = loaded();
    final robot = w.find('Robot')!;
    final go = w.instantiate(const AssetRef(cratePrefab), name: 'Cargo', parent: robot);
    expect(go.name, 'Cargo');
    expect(go.parent, robot);
    expect(robot.findChild('Cargo'), isNotNull);
    w.frame(1 / 60);
    expect(w.componentsOf(go.entity).whereType<MeshRenderer>(), hasLength(1));
  });

  test('instantiate fails for an unknown prefab, a pending parent and before a project is loaded', () {
    final w = loaded();
    expect(() => w.instantiate('c0000000-0000-4000-8000-0000000000ff'), throwsA(isA<FynkelInstantiateError>()));
    final pending = w.spawn(name: 'Pending');
    expect(() => w.instantiate(cratePrefab, parent: pending), throwsStateError);
    expect(() => w.instantiate(42), throwsArgumentError);

    final fresh = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 4);
    addTearDown(fresh.dispose);
    expect(() => fresh.instantiate(cratePrefab), throwsA(isA<FynkelInstantiateError>()));
  });
}
