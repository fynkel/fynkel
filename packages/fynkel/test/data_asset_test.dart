// Data assets at runtime (ADR-0027 step 4): `world.data<T>(ref)` makes one
// shared instance per asset from its cooked JSON (`fynkel_world_asset_json`)
// and keeps it for the world's life; a behaviour's data-typed fields are
// set to those instances at load, a data asset's own ones too (a cycle
// shares the instance); a reference that cannot be made is null with
// `FYN-B0004`. Over a copy of the fixture project with a `test.Weapon`
// type, three weapons and a scene whose knight holds them, cooked by the
// `fynkel` CLI and loaded by the real native library.

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

import 'support/weapons.dart';

const sword = 'c0000000-0000-4000-8000-000000000d01';
const axe = 'c0000000-0000-4000-8000-000000000d02';
const great = 'c0000000-0000-4000-8000-000000000d03';
const arena = 'c0000000-0000-4000-8000-000000000d10';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String project;
  late NativeLibrary lib;

  void write(String rel, Object json) {
    final f = File('$project/$rel')..parent.createSync(recursive: true);
    f.writeAsStringSync(const JsonEncoder.withIndent('  ').convert(json));
  }

  Map<String, Object?> weapon(String id, String name, Map<String, Object?> data) => {
    r'$schema': 'https://fynkel.dev/schemas/data.schema.json',
    'formatVersion': 1,
    'id': id,
    'name': name,
    'type': 'test.Weapon',
    'version': 1,
    'data': data,
  };

  setUpAll(() {
    lib = NativeLibrary.open();
    project = Directory.systemTemp.createTempSync('fynkel-data-assets-').path;
    Process.runSync('cp', ['-R', '$fixture/.', project]);
    final stale = Directory('$project/.fynkel');
    if (stale.existsSync()) stale.deleteSync(recursive: true);
    writeWeaponSchemas(project);
    // The sword upgrades to the great sword, which upgrades back: a cycle.
    write(
      'assets/data/sword.data.json',
      weapon(sword, 'Sword', {
        'damage': 3,
        'upgrade': {'asset': great},
      }),
    );
    write(
      'assets/data/great.data.json',
      weapon(great, 'Great Sword', {
        'damage': 8,
        'upgrade': {'asset': sword},
      }),
    );
    write('assets/data/axe.data.json', weapon(axe, 'Axe', {}));
    write('assets/scenes/arena.scene.json', {
      r'$schema': 'https://fynkel.dev/schemas/scene.schema.json',
      'formatVersion': 7,
      'id': arena,
      'name': 'Arena',
      'entities': {
        'c0000000-0000-4000-8000-000000000d20': {
          'name': 'Knight',
          'parent': null,
          'children': [],
          'components': {
            'c0000000-0000-4000-8000-000000000d30': {
              'type': 'fynkel.Transform',
              'version': 1,
              'data': {
                'position': [0, 0, 0],
                'rotation': [0, 0, 0, 1],
                'scale': [1, 1, 1],
              },
            },
            'c0000000-0000-4000-8000-000000000d31': {
              'type': 'test.Wielder',
              'version': 1,
              'data': {
                'weapon': {'asset': sword},
                'spares': [
                  {'asset': axe},
                  {'asset': sword},
                ],
              },
            },
          },
        },
      },
    });
    final prefab = jsonDecode(File('$project/assets/scenes/arena.scene.json').readAsStringSync()) as Map;
    prefab[r'$schema'] = 'https://fynkel.dev/schemas/prefab.schema.json';
    prefab['id'] = 'd0000000-0000-4000-8000-000000000001';
    prefab['root'] = 'c0000000-0000-4000-8000-000000000d20';
    write('assets/prefabs/knight.prefab.json', prefab);
    final r = runFynkel(['validate', '--project', project, '--json']);
    final errors = [
      for (final d in (jsonDecode(r.stdout as String) as Map)['diagnostics'] as List)
        if ((d as Map)['severity'] == 'error' && d['code'] != 'FYN-P0005') d,
    ];
    expect(errors, isEmpty, reason: '${r.stdout}');
    cookProject(project);
  });
  tearDownAll(() {
    lib.dispose();
    Directory(project).deleteSync(recursive: true);
  });

  World world({bool registerData = true}) {
    final w = World.create(library: lib)..registry.register(wielderType);
    if (registerData) w.registry.registerData(weaponType);
    addTearDown(w.dispose);
    return w;
  }

  test('loaded data ownership ends with the world without destroying standalone data', () {
    for (final nativeDisposal in [false, true]) {
      final w = world()..loadProject(project);
      final loaded = w.data<Weapon>(const AssetRef(sword));
      final standalone = Weapon();
      if (nativeDisposal) {
        w.native.dispose();
      } else {
        w.dispose();
      }
      expect(loaded.isAlive, isFalse);
      expect(loaded.upgrade!.isAlive, isFalse);
      expect(standalone.isAlive, isTrue);
      expect(() => w.data<Weapon>(const AssetRef(sword)), throwsStateError);
      w.dispose();
      expect(w.loadedData, isEmpty);
    }
  });

  test('published entity bytes survive scene replacement and old handles retire', () {
    final w = world()..loadProject(project);
    final before = w.native.query(QueryKind.allEntities);
    final handles = before.entities();
    final bytes = before.bytes.toList();
    w.loadScene(arena);
    expect(before.bytes, bytes);
    expect(before.entities(), handles);
    for (final e in handles) { expect(w.native.contains(e), isFalse); }
  });

  test('a scene load sets data-typed fields to the shared instances', () {
    final w = world();
    final warnings = <LoadDiagnostic>[];
    w.loadProject(project); // its startup scene warns of game.Spinner
    w.onLoadDiagnostics = warnings.addAll;
    w.loadScene(arena);
    expect(warnings, isEmpty, reason: '$warnings');
    final knight = w.find('Knight')!.behaviour<Wielder>()!;
    final s = knight.weapon!;
    expect(s.damage, 3);
    expect(s.asset, const AssetRef(sword));
    expect(knight.spares.map((x) => x.damage), [1, 3], reason: 'the axe takes the class default');
    expect(knight.spares[1], same(s), reason: 'one instance per asset');
    expect(s.upgrade!.damage, 8);
    expect(s.upgrade!.upgrade, same(s), reason: 'a cycle shares the instance');

    expect(w.data<Weapon>(const AssetRef(sword)), same(s));
    expect(w.loadedData.keys, unorderedEquals([sword, great, axe]));

    // Changes last for the run: the next load shares the changed instance.
    s.damage = 99;
    w.loadScene(arena);
    expect(w.find('Knight')!.behaviour<Wielder>()!.weapon!.damage, 99);
    expect(wielderType.field('weapon')!.get(knight), {'asset': sword});
  });

  test('an editor edit during Play: the shared instance takes the saved data', () {
    final w = world();
    w.loadProject(project);
    w.loadScene(arena);
    final knight = w.find('Knight')!.behaviour<Wielder>()!;
    final s = knight.weapon!;
    expect(w.applyDataEdit(sword, {'damage': 5}), isTrue);
    expect(s.damage, 5);
    expect(s.upgrade, isNull, reason: 'no longer named: back to its default');
    expect(
      w.applyDataEdit(sword, {
        'upgrade': {'asset': axe},
      }),
      isTrue,
    );
    expect(s.damage, 1, reason: 'the class default');
    expect(s.upgrade, same(w.data<Weapon>(const AssetRef(axe))));
    expect(w.applyDataEdit(great, const {}), isTrue);
    expect(w.applyDataEdit('c0000000-0000-4000-8000-0000000000ff', const {}), isFalse);

    // A script's data-typed field set from its JSON, as the Inspector does:
    // a material is no weapon (left out, FYN-B0004).
    final warnings = <LoadDiagnostic>[];
    w.onLoadDiagnostics = warnings.addAll;
    final spares = w.resolveData([
      {'asset': great},
      {'asset': 'c0000000-0000-4000-8000-000000000020'},
    ], 'test.Weapon');
    expect((spares as List).single, same(w.loadedData[great]));
    expect(warnings.map((d) => d.code), ['FYN-B0004']);
  });

  test('world.data from code; what cannot be made', () {
    final w = world();
    expect(() => w.data<Weapon>(const AssetRef(sword)), throwsStateError, reason: 'no project loaded');
    w.loadProject(project);
    final axeWeapon = w.data<Weapon>(const AssetRef(axe));
    expect((axeWeapon.damage, axeWeapon.dataType?.id), (1, 'test.Weapon'));
    // A data asset is an Object (ADR-0036 §1): named after its document,
    // alive for the world's life, and not destroyable from a script.
    expect(axeWeapon, isA<Asset>());
    expect((axeWeapon.ref, axeWeapon.isAlive), (const AssetRef(axe), true));
    expect(axeWeapon.name, isNot('Weapon'));
    expect(() => axeWeapon.destroy(), throwsUnsupportedError);
    final made = Weapon();
    expect((made.name, made.ref), ('Weapon', null));
    made.destroy();
    expect(made.isAlive, isFalse);
    expect(() => w.data<Weapon>(const AssetRef('c0000000-0000-4000-8000-0000000000ff')), throwsArgumentError);
    expect(
      () => w.data<Weapon>(const AssetRef('c0000000-0000-4000-8000-000000000020')),
      throwsArgumentError,
      reason: 'a material, not a data asset',
    );
    expect(() => w.data<Weapon>(const AssetRef(sword, sub: 'x')), throwsArgumentError);
  });

  test('a data asset whose type is not registered: the field is None, with FYN-B0004', () {
    final w = world(registerData: false);
    final warnings = <LoadDiagnostic>[];
    w.loadProject(project); // its startup scene warns of game.Spinner
    w.onLoadDiagnostics = warnings.addAll;
    w.loadScene(arena);
    final knight = w.find('Knight')!.behaviour<Wielder>()!;
    expect(knight.weapon, isNull);
    expect(knight.spares, isEmpty);
    expect(warnings.map((d) => d.code).toSet(), {'FYN-B0004'});
    expect(warnings.first.message, contains('test.Wielder.weapon'));
    expect(warnings.first.message, contains('not registered'));
    expect(warnings.first.pointer, endsWith('/data/weapon'));
  });
  test('edit shares staged cyclic assets and publishes their cache once', () {
    final w = world()..loadProject(project);
    final s = w.edit(() {
      final s = w.data<Weapon>(const AssetRef(sword));
      expect(s.upgrade!.upgrade, same(s));
      expect(w.data<Weapon>(const AssetRef(sword)), same(s));
      expect(w.loadedData[sword], same(s));
      return s;
    });
    expect(w.loadedData[sword], same(s));
    expect(w.data<Weapon>(const AssetRef(great)), same(s.upgrade));
  });

  test('abort discards new cache entries and invalidates escaped new assets', () {
    final w = world()..loadProject(project);
    final existing = w.data<Weapon>(const AssetRef(axe))..damage = 77;
    late Weapon escaped;
    expect(
      () => w.edit(() {
        escaped = w.data<Weapon>(const AssetRef(sword));
        expect(w.data<Weapon>(const AssetRef(axe)), same(existing));
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(w.loadedData.keys, [axe]);
    expect(existing.damage, 77);
    expect(existing.isAlive, isTrue);
    expect(escaped.isAlive, isFalse);
    expect(escaped.upgrade!.isAlive, isFalse);
    final retry = w.edit(() => w.data<Weapon>(const AssetRef(sword)));
    expect(retry, isNot(same(escaped)));
    expect(retry.isAlive, isTrue);
  });

  test('caught malformed or mistyped data references poison the complete edit', () {
    final w = world()..loadProject(project);
    for (final operation in <void Function()>[
      () => w.resolveData({'asset': sword}, 'wrong.Type'),
      () => w.resolveData({'asset': sword, 'sub': 'bad'}, 'test.Weapon'),
      () => w.resolveData(42, 'test.Weapon'),
      () => w.data<Weapon>(const AssetRef('c0000000-0000-4000-8000-000000000020')),
    ]) {
      expect(
        () => w.edit(() {
          w.spawn(name: 'discard-data');
          try {
            operation();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(w.find('discard-data'), isNull);
      expect(w.loadedData, isEmpty);
    }
  });

  test('prefab data fields and lists share the same staged cache as world.data', () {
    final w = world()..loadProject(project);
    final root = w.edit(() {
      final root = w.instantiate('d0000000-0000-4000-8000-000000000001');
      final b = root.behaviour<Wielder>()!;
      expect(b.weapon, same(w.data<Weapon>(const AssetRef(sword))));
      expect(b.spares[1], same(b.weapon));
      expect(b.weapon!.upgrade!.upgrade, same(b.weapon));
      return root;
    });
    expect(root.behaviour<Wielder>()!.weapon, same(w.loadedData[sword]));
    expect(w.loadedData.keys, unorderedEquals([sword, great, axe]));
  });

  test('final native validation rolls back prefab data and attachments', () {
    final w = world()..loadProject(project);
    expect(
      () => w.edit(() {
        final root = w.instantiate('d0000000-0000-4000-8000-000000000001', name: 'discard-knight');
        final target = w.spawn();
        root.set(Follow(target: target));
        target.destroy();
      }),
      throwsStateError,
    );
    expect(w.loadedData, isEmpty);
    expect(w.find('discard-knight'), isNull);
  });

  test('data factory cannot overwrite a bound shared instance', () {
    final w = world()..loadProject(project);
    final existing = w.data<Weapon>(const AssetRef(axe))..damage = 77;
    w.registry.registerData(
      DataType(id: 'test.Weapon', create: () => existing, fields: weaponType.fields),
      replace: true,
    );
    expect(() => w.edit(() => w.data<Weapon>(const AssetRef(sword))), throwsStateError);
    expect(existing.damage, 77);
    expect(existing.asset, const AssetRef(axe));
    expect(existing.isAlive, isTrue);
    expect(w.loadedData.keys, [axe]);
  });

  test('cached assets reject sub-asset aliases inside edits', () {
    final w = world()..loadProject(project);
    final existing = w.data<Weapon>(const AssetRef(axe));
    expect(
      () => w.edit(() {
        try {
          w.data<Weapon>(const AssetRef(axe, sub: 'bad'));
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(w.loadedData[axe], same(existing));
  });

  test('reentrant data factory rejects before publishing an incomplete instance', () {
    final w = world()..loadProject(project);
    w.registry.registerData(
      DataType(id: 'test.Weapon', create: () => w.data<Weapon>(const AssetRef(sword)), fields: weaponType.fields),
      replace: true,
    );
    expect(() => w.edit(() => w.data<Weapon>(const AssetRef(sword))), throwsStateError);
    expect(w.loadedData, isEmpty);
  });
  test('data destruction and editor field mutation remain guarded inside edits', () {
    final w = world()..loadProject(project);
    final code = Weapon();
    final loaded = w.data<Weapon>(const AssetRef(axe));
    for (final operation in <void Function()>[
      code.destroy,
      () {
        w.applyDataEdit(axe, {'damage': 55});
      },
    ]) {
      expect(
        () => w.edit(() {
          w.spawn(name: 'discard-data-mutation');
          try {
            operation();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(w.find('discard-data-mutation'), isNull);
      expect(code.isAlive, isTrue);
      expect(loaded.damage, 1);
    }
  });
}
