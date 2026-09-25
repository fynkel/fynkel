// Code first, Tier 2 (docs/code-first.md, ADR-0026 §4): a material made
// in code — `world.createMaterial(PbrMaterial(...))`, Unity's
// `new Material(shader)` — taken by a `MeshRenderer` like a cooked one,
// changed with `setMaterial`, kept across scene loads. Over the real
// native library (the store's side: crates/fynkel_native/tests/load.rs).

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-code-first-materials-').path;
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

  test('a material made in code, on a shape, changed, kept across a scene load, destroyed', () {
    final w = fresh()..loadProject(fixture, cacheDir: cache);
    final red = w.createMaterial(const PbrMaterial(baseColor: Vec4(1, 0, 0, 1), roughness: 0.3));
    expect(red.isValid, isTrue);
    expect(red.asset[14], '4', reason: 'a v4 UUID');
    expect(w.createMaterial(), isNot(red), reason: 'a new id each time; PbrMaterial() is the default material');

    final cube = w.createPrimitive(Primitive.cube);
    cube.set(cube.get<MeshRenderer>()!.copyWith(material: red));
    w.frame(1 / 60);
    expect(cube.get<MeshRenderer>()!.material, red);

    // Unity's `material.color = …`: every renderer using it changes.
    w.setMaterial(red, w.runtimeMaterial(red)!.copyWith(baseColor: const Vec4(0, 1, 0, 1)));
    expect(w.runtimeMaterial(red)!.baseColor, const Vec4(0, 1, 0, 1));
    expect(w.runtimeMaterial(red)!.roughness, closeTo(0.3, 1e-6), reason: 'copyWith keeps the rest');

    w.scenes.transition(w.assets.find('scenes/playground')!);
    w.frame(1 / 60);
    w.setMaterial(red, const PbrMaterial(metallic: 1)); // still there: no throw
    expect(w.runtimeMaterial(red)!.metallic, 1);

    w.destroyMaterial(red);
    expect(w.runtimeMaterial(red), isNull);
  });

  test('a cooked material replaced for the run, then back', () {
    final w = fresh()..loadProject(fixture, cacheDir: cache);
    final rusted = w.assets.find('materials/rusted')!;
    expect(w.runtimeMaterial(rusted), isNull);
    w.setMaterial(rusted, const PbrMaterial(roughness: 1));
    expect(w.runtimeMaterial(rusted)!.roughness, 1);
    w.destroyMaterial(rusted);
    expect(w.runtimeMaterial(rusted), isNull);
  });

  test('before any load there is no store to hold it', () {
    final w = fresh();
    expect(() => w.createMaterial(), throwsStateError);
    expect(() => w.setMaterial(const AssetRef('not-a-uuid'), const PbrMaterial()), throwsArgumentError);
  });
}
