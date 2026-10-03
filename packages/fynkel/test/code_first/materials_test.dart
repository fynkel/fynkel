// Code first, Tier 2 (docs/code-first.md, ADR-0026 §4): a material made
// in code — `world.createMaterial(PbrMaterial(...))`, Unity's
// `new Material(shader)` — taken by a `MeshRenderer` like a cooked one,
// changed with `setMaterial`, kept across scene loads. Over the real
// native library (the store's side: crates/fynkel_native/tests/load.rs).

import 'dart:io';

import 'package:fynkel/advanced.dart';
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

  group('a material of a .shader (ADR-0031)', () {
    late String project;
    late String shaderCache;
    setUpAll(() {
      project = Directory.systemTemp.createTempSync('fynkel-code-first-shader-').path;
      _copy(Directory(fixture), Directory(project));
      Directory('$project/assets/shaders').createSync(recursive: true);
      File('$project/assets/shaders/tint.shader').writeAsStringSync(_tintShader);
      File('$project/assets/shaders/tint.shader.meta.json').writeAsStringSync(
        '{"\$schema":"https://fynkel.dev/schemas/meta.schema.json","formatVersion":2,"id":"$_tintId",'
        '"name":"tint.shader","importer":"fynkel.shader","importerVersion":1,"settings":{},"subAssets":{}}',
      );
      File('$project/assets/materials/tinted.material.json').writeAsStringSync(
        '{"\$schema":"https://fynkel.dev/schemas/material.schema.json","formatVersion":2,'
        '"id":"c0000000-0000-4000-8000-000000000602","name":"Tinted","shader":{"asset":"$_tintId"},'
        '"parameters":{"tint":[0,0,1]}}',
      );
      shaderCache = '$project/.fynkel/cache';
      cookProject(project, cacheDir: shaderCache);
    });
    tearDownAll(() => Directory(project).deleteSync(recursive: true));

    test('made with values, changed one at a time, checked as the cook checks, destroyed', () {
      final w = fresh()..loadProject(project, cacheDir: shaderCache);
      final glow = w.createShaderMaterial('shaders/tint.shader', values: {'tint': const Vec3(1, 0, 0), 'power': 2});
      expect(glow.isValid, isTrue);
      expect(w.createShaderMaterial(const AssetRef(_tintId)), isNot(glow));

      final cube = w.createPrimitive(Primitive.cube);
      cube.set(cube.get<MeshRenderer>()!.copyWith(material: glow));
      w.frame(1 / 60);

      // Unity's `material.SetFloat`: every frame is fine.
      for (var i = 0; i < 3; i++) {
        w.setMaterialValue(glow, 'power', i.toDouble());
        w.frame(1 / 60);
      }
      expect(w.materialValue(glow, 'power'), 2.0);
      expect(w.materialValue(glow, 'tint'), [1.0, 0.0, 0.0]);
      w.setMaterialValue(glow, 'glow', w.assets.find('textures/swatch')!);
      expect(w.materialValue(glow, 'glow'), isA<AssetRef>());
      w.setMaterialValue(glow, 'power', null);
      expect(w.materialValue(glow, 'power'), isNull, reason: 'the default again');

      // Checked at the call, the material unchanged.
      expect(
        () => w.setMaterialValue(glow, 'power', 9),
        throwsA(isA<ArgumentError>().having((e) => '$e', 'message', contains('FYN-D0303'))),
      );
      expect(
        () => w.setMaterialValue(glow, 'powr', 1),
        throwsA(isA<ArgumentError>().having((e) => '$e', 'message', contains('FYN-D0301'))),
      );
      expect(() => w.setMaterialValue(glow, 'tint', 'red'), throwsArgumentError);
      expect(w.materialValue(glow, 'power'), isNull);
      expect(() => w.createShaderMaterial('shaders/nope.shader'), throwsArgumentError);
      expect(() => w.createShaderMaterial(42), throwsArgumentError);

      w.destroyMaterial(glow);
      expect(w.materialValue(glow, 'tint'), isNull);
    });

    test('shader edits, representation changes and Inspector overrides share rollback', () {
      final w = fresh()..loadProject(project, cacheDir: shaderCache);
      final material = w.createShaderMaterial(const AssetRef(_tintId), values: {'power': 1});
      expect(
        () => w.edit(() {
          w.setMaterialValue(material, 'power', 2);
          expect(w.materialValue(material, 'power'), 2);
          w.setMaterial(material, const PbrMaterial());
          expect(w.materialValue(material, 'power'), isNull);
          expect(w.runtimeMaterial(material), isNotNull);
          throw StateError('abort');
        }),
        throwsStateError,
      );
      expect(w.materialValue(material, 'power'), 1);
      expect(w.runtimeMaterial(material), isNull);
      expect(
        () => w.edit(() {
          w.applyMaterialEdit(material.asset, {'shader': 'fynkel.pbr@1'});
          expect(material.isAlive, isFalse);
          throw StateError('abort');
        }),
        throwsStateError,
      );
      expect(material.isAlive, isTrue);
      expect(w.materialValue(material, 'power'), 1);
      expect(
        () => w.edit(() {
          w.setMaterialValue(material, 'power', 2);
          try {
            w.setMaterialValue(material, 'power', 9);
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(w.materialValue(material, 'power'), 1);
      final tint = <num>[1, 0, 0];
      w.setMaterialValue(material, 'tint', tint);
      tint[0] = 0;
      expect(w.materialValue(material, 'tint'), [1, 0, 0]);
      expect(() => (w.materialValue(material, 'tint') as List)[0] = 0, throwsUnsupportedError);
      w.edit(() {
        w.setMaterial(material, const PbrMaterial());
      });
      expect(w.materialValue(material, 'power'), isNull);
      expect(w.runtimeMaterial(material), isNotNull);
      material.destroy();
      expect(material.isAlive, isFalse);
    });

    test('shader resource ownership ends with its world', () {
      final w = fresh()..loadProject(project, cacheDir: shaderCache);
      final material = w.createShaderMaterial(const AssetRef(_tintId), values: {'power': 1});
      w.dispose();
      expect(material.isAlive, isFalse);
      expect(w.materialValue(material, 'power'), isNull);
    });

    test('a cooked shader material changed for the run, then back', () {
      final w = fresh()..loadProject(project, cacheDir: shaderCache);
      final tinted = w.assets.find('materials/tinted')!;
      expect(w.materialValue(tinted, 'power'), isNull);
      w.setMaterialValue(tinted, 'power', 3);
      expect(w.materialValue(tinted, 'power'), 3);
      expect(w.materialValue(tinted, 'tint'), [0, 0, 1], reason: 'its cooked values kept');
      w.destroyMaterial(tinted);
      expect(w.materialValue(tinted, 'power'), isNull);
    });

    test('an Inspector edit during Play: the saved fields, with what code set kept on top', () {
      final w = fresh()..loadProject(project, cacheDir: shaderCache);
      final tinted = w.assets.find('materials/tinted')!;
      Map<String, Object?> saved(List<num> tint, num power) => {
        r'$schema': 'https://fynkel.dev/schemas/material.schema.json',
        'formatVersion': 2,
        'id': tinted.asset,
        'name': 'Tinted',
        'shader': {'asset': _tintId},
        'parameters': {'tint': tint, 'power': power},
      };
      expect(
        w.applyMaterialEdit(tinted.asset, saved([1, 0, 0], 1)),
        isFalse,
        reason: 'code has not touched it: the editor draws it over the cooked one',
      );

      w.setMaterialValue(tinted, 'power', 3);
      expect(w.applyMaterialEdit(tinted.asset, saved([1, 0, 0], 1)), isTrue);
      expect(w.materialValue(tinted, 'tint'), [1, 0, 0], reason: 'the edit shows');
      expect(w.materialValue(tinted, 'power'), 3, reason: 'the code drives power');
      w.setMaterialValue(tinted, 'power', 2);
      expect(w.materialValue(tinted, 'tint'), [1, 0, 0], reason: 'the next SetFloat keeps the edit');

      // Switched to Lit in the Inspector: the code's copy goes.
      expect(w.applyMaterialEdit(tinted.asset, {'shader': 'fynkel.pbr@1'}), isFalse);
      expect(w.materialValue(tinted, 'power'), isNull);
    });

    test('a Lit material is not a shader material', () {
      final w = fresh()..loadProject(project, cacheDir: shaderCache);
      expect(() => w.setMaterialValue(w.assets.find('materials/rusted')!, 'tint', 1), throwsArgumentError);
    });
  });

  test('before any load there is no store to hold it', () {
    final w = fresh();
    expect(() => w.createMaterial(), throwsStateError);
    expect(() => w.setMaterial(const AssetRef('not-a-uuid'), const PbrMaterial()), throwsArgumentError);
  });
}

const _tintId = 'c0000000-0000-4000-8000-000000000601';

const _tintShader = r'''shader "Tests/Tint" {
    lighting unlit
    properties {
        tint color3 = (1, 1, 1)
        power float = 1 range(0, 4)
        glow texture = black
    }
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    s.base_color = material.tint * material.power + sample_glow(in.uv0).rgb;
    return s;
}
''';

void _copy(Directory from, Directory to) {
  to.createSync(recursive: true);
  for (final e in from.listSync()) {
    final name = e.uri.pathSegments.lastWhere((s) => s.isNotEmpty);
    if (e is Directory) {
      if (name == '.fynkel') continue;
      _copy(e, Directory('${to.path}/$name'));
    } else if (e is File) {
      e.copySync('${to.path}/$name');
    }
  }
}
