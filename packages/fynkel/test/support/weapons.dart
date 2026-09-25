// The `test.Weapon` data type and the `test.Wielder` script that holds
// weapons, as the generator emits them, and their schemas (ADR-0027).

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/fynkel.dart';

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

/// Writes both types' schemas into [project]'s `schemas/`.
void writeWeaponSchemas(String project) {
  void write(String rel, Object json) {
    final f = File('$project/$rel')..parent.createSync(recursive: true);
    f.writeAsStringSync(const JsonEncoder.withIndent('  ').convert(json));
  }

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
}

/// Registers both types.
void registerWeapons(BehaviourRegistry r) => r
  ..register(wielderType)
  ..registerData(weaponType);
