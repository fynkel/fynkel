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

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

class Weapon extends DataAsset {
  int damage = 1;
  Weapon? upgrade;
}

class Wielder extends Behaviour {
  Weapon? weapon;
  List<Weapon> spares = [];
}

/// What the generator emits for the two classes.
final weaponType = DataType(
  id: 'test.Weapon',
  create: Weapon.new,
  fields: [
    DataField(
      name: 'damage',
      type: 'integer',
      get: (d) => (d as Weapon).damage,
      set: (d, v) => (d as Weapon).damage = v as int,
    ),
    DataField(
      name: 'upgrade',
      type: 'data',
      dataType: 'test.Weapon',
      get: (d) => dataJson((d as Weapon).upgrade),
      target: (d) => (d as Weapon).upgrade,
      set: (d, v) => (d as Weapon).upgrade = v as Weapon?,
    ),
  ],
);
final wielderType = BehaviourType(
  id: 'test.Wielder',
  create: Wielder.new,
  fields: [
    BehaviourField(
      name: 'weapon',
      type: 'data',
      dataType: 'test.Weapon',
      get: (b) => dataJson((b as Wielder).weapon),
      target: (b) => (b as Wielder).weapon,
      set: (b, v) => (b as Wielder).weapon = v as Weapon?,
    ),
    BehaviourField(
      name: 'spares',
      type: 'list',
      dataType: 'test.Weapon',
      get: (b) => [for (final x in (b as Wielder).spares) dataJson(x)],
      target: (b) => (b as Wielder).spares,
      set: (b, v) => (b as Wielder).spares = [for (final x in v as List) x as Weapon],
    ),
  ],
);

const sword = 'c0000000-0000-4000-8000-000000000d01';
const axe = 'c0000000-0000-4000-8000-000000000d02';
const great = 'c0000000-0000-4000-8000-000000000d03';
const arena = 'c0000000-0000-4000-8000-000000000d10';

const assetRef = r'https://fynkel.dev/schemas/envelope.schema.json#/$defs/assetRef';
Map<String, Object?> dataRef(bool nullable) => {
      if (nullable)
        'anyOf': [
          {r'$ref': assetRef},
          {'type': 'null'},
        ]
      else
        r'$ref': assetRef,
      'x-fynkel': {'type': 'data', if (nullable) 'nullable': true, 'data': 'test.Weapon'},
    };

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
    write('schemas/data/test.Weapon.schema.json', {
      r'$schema': 'https://json-schema.org/draft/2020-12/schema',
      r'$id': 'https://fynkel.dev/schemas/data/test.Weapon.schema.json',
      'title': 'test.Weapon',
      'type': 'object',
      'properties': {
        'damage': {
          'type': 'integer',
          'x-fynkel': {'type': 'integer'},
        },
        'upgrade': dataRef(true),
      },
      'additionalProperties': false,
      'x-fynkel': {'kind': 'data', 'id': 'test.Weapon', 'version': 1},
    });
    write('schemas/behaviours/test.Wielder.schema.json', {
      r'$schema': 'https://json-schema.org/draft/2020-12/schema',
      r'$id': 'https://fynkel.dev/schemas/behaviours/test.Wielder.schema.json',
      'title': 'test.Wielder',
      'type': 'object',
      'properties': {
        'weapon': dataRef(true),
        'spares': {
          'type': 'array',
          'items': dataRef(false),
          'x-fynkel': {'type': 'list', 'items': 'data', 'data': 'test.Weapon'},
        },
      },
      'additionalProperties': false,
      'x-fynkel': {'kind': 'behaviour', 'id': 'test.Wielder', 'version': 1},
    });
    // The sword upgrades to the great sword, which upgrades back: a cycle.
    write('assets/data/sword.data.json', weapon(sword, 'Sword', {
      'damage': 3,
      'upgrade': {'asset': great},
    }));
    write('assets/data/great.data.json', weapon(great, 'Great Sword', {
      'damage': 8,
      'upgrade': {'asset': sword},
    }));
    write('assets/data/axe.data.json', weapon(axe, 'Axe', {}));
    write('assets/scenes/arena.scene.json', {
      r'$schema': 'https://fynkel.dev/schemas/scene.schema.json',
      'formatVersion': 5,
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

  test('world.data from code; what cannot be made', () {
    final w = world();
    expect(() => w.data<Weapon>(const AssetRef(sword)), throwsStateError, reason: 'no project loaded');
    w.loadProject(project);
    final axeWeapon = w.data<Weapon>(const AssetRef(axe));
    expect((axeWeapon.damage, axeWeapon.dataType?.id), (1, 'test.Weapon'));
    expect(() => w.data<Weapon>(const AssetRef('c0000000-0000-4000-8000-0000000000ff')), throwsArgumentError);
    expect(() => w.data<Weapon>(const AssetRef('c0000000-0000-4000-8000-000000000020')), throwsArgumentError,
        reason: 'a material, not a data asset');
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
}
