// Unity parity: data assets, the ScriptableObject (ADR-0027 §7;
// docs/unity-parity.md). Create ▸ Game › Weapon in the Project makes a
// `.data.json` of that type; its Inspector page draws the type's fields
// from the committed schema with the script Inspector's editors, each
// change saved at once and undone by ⌘Z; a data-typed field's picker
// offers only data assets of its type; a record (`@FynkelRecord`) is a
// foldout, alone or as a list's items.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const sword = 'c0000000-0000-4000-8000-000000000d01';
const axe = 'c0000000-0000-4000-8000-000000000d02';
const shield = 'c0000000-0000-4000-8000-000000000d03';
const loot = 'c0000000-0000-4000-8000-000000000d04';
const assetRef = r'https://fynkel.dev/schemas/envelope.schema.json#/$defs/assetRef';

void writeJson(File f, Object json) {
  f.parent.createSync(recursive: true);
  f.writeAsStringSync(const JsonEncoder.withIndent('  ').convert(json));
}

Map<String, Object?> typeSchema(String id, Map<String, Object?> properties, {String? menu, String? icon}) => {
  r'$schema': 'https://json-schema.org/draft/2020-12/schema',
  r'$id': 'https://fynkel.dev/schemas/data/$id.schema.json',
  'title': id,
  'type': 'object',
  'properties': properties,
  'additionalProperties': false,
  'x-fynkel': {'kind': 'data', 'id': id, 'version': 1, 'menu': ?menu, 'icon': ?icon},
};

/// The fixture with `game.Weapon` (damage, upgrade) and `game.Shield`
/// types, two weapons and a shield.
void weapons(Directory dir) {
  writeJson(
    File('${dir.path}/schemas/data/game.Weapon.schema.json'),
    typeSchema('game.Weapon', {
      'damage': {
        'type': 'integer',
        'minimum': 0,
        'default': 1,
        'x-fynkel': {'type': 'integer'},
      },
      'upgrade': {
        'anyOf': [
          {r'$ref': assetRef},
          {'type': 'null'},
        ],
        'x-fynkel': {'type': 'data', 'nullable': true, 'data': 'game.Weapon'},
      },
    }, menu: 'Game/Weapon', icon: 'swords'),
  );
  writeJson(File('${dir.path}/schemas/data/game.Shield.schema.json'), typeSchema('game.Shield', {}));
  Map<String, Object?> doc(String id, String name, String type) => {
    r'$schema': 'https://fynkel.dev/schemas/data.schema.json',
    'formatVersion': 1,
    'id': id,
    'name': name,
    'type': type,
    'version': 1,
    'data': <String, Object?>{},
  };
  writeJson(File('${dir.path}/assets/data/sword.data.json'), doc(sword, 'Sword', 'game.Weapon'));
  writeJson(File('${dir.path}/assets/data/axe.data.json'), doc(axe, 'Axe', 'game.Weapon'));
  writeJson(File('${dir.path}/assets/data/shield.data.json'), doc(shield, 'Shield', 'game.Shield'));
  // `game.Loot`: a `Drop` record alone (`best`) and in a list (`drops`), as
  // the generator writes them.
  Map<String, Object?> drop() => {
    'type': 'object',
    'properties': {
      'weight': {
        'type': 'integer',
        'minimum': 0,
        'default': 1,
        'x-fynkel': {'type': 'integer', 'min': 0},
      },
      'rare': {
        'type': 'boolean',
        'default': false,
        'x-fynkel': {'type': 'boolean'},
      },
    },
    'additionalProperties': false,
  };
  writeJson(
    File('${dir.path}/schemas/data/game.Loot.schema.json'),
    typeSchema('game.Loot', {
      'best': {
        ...drop(),
        'default': <String, Object?>{},
        'x-fynkel': {'type': 'record', 'record': 'Drop'},
      },
      'drops': {
        'type': 'array',
        'items': {
          ...drop(),
          'x-fynkel': {'type': 'record', 'record': 'Drop'},
        },
        'default': <Object?>[],
        'x-fynkel': {'type': 'list', 'items': 'record'},
      },
    }),
  );
  writeJson(File('${dir.path}/assets/data/loot.data.json'), doc(loot, 'Loot', 'game.Loot'));
}

void main() {
  Future<void> reveal(WidgetTester tester, Finder f) async {
    await Scrollable.ensureVisible(tester.element(f), alignment: 0.5);
    await tester.pumpAndSettle();
  }

  testWidgets('Create ▸ Game › Weapon: the Inspector edits its fields from the schema; a refusal; ⌘Z', (tester) async {
    final e = await Editor.open(tester, prepare: weapons);
    final scene = e.shell.tabs!.focused!;
    e.shell.browser!.setFolder('');
    await tester.pumpAndSettle();
    await e.click(find.byKey(const Key('project.new')));
    expect(find.text('New Game › Weapon'), findsOneWidget, reason: "the type's menu path");
    expect(find.byKey(const Key('project.new.data.game.Shield')), findsOneWidget);
    await e.click(find.byKey(const Key('project.new.data.game.Weapon')));
    final file = File('${e.dir.path}/assets/New Weapon.data.json');
    expect(file.existsSync(), isTrue);
    Map<String, Object?> saved() => jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
    expect(saved()['type'], 'game.Weapon');
    final id = saved()['id'] as String;
    await e.press(LogicalKeyboardKey.escape);

    final damage = find.byKey(ValueKey('field.data.$id.damage'));
    expect(damage, findsOneWidget);
    expect(find.byKey(ValueKey('field.data.$id.upgrade')), findsOneWidget);
    final box = find.descendant(of: damage, matching: find.byKey(const ValueKey('number.text')));
    await reveal(tester, box);
    await e.click(box);
    await e.type('7');
    await e.submit();
    expect((saved()['data'] as Map)['damage'], 7);

    // Below the schema's minimum: refused, nothing written.
    await e.click(box);
    await e.type('-3');
    await e.submit();
    expect((saved()['data'] as Map)['damage'], 7);
    expect(find.descendant(of: damage, matching: find.textContaining('below the minimum')), findsOneWidget);

    // Back in the Inspector (the refusal took the keyboard away).
    await e.click(find.descendant(of: damage, matching: find.text('Damage')));
    expect(e.shell.registry.byId('document.undo')!.label, 'Undo Data Asset Change');
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect((saved()['data'] as Map).containsKey('damage'), isFalse);
    expect(scene.document.history, 0);
  });

  testWidgets("a data-typed field's picker offers only data assets of its type", (tester) async {
    final e = await Editor.open(tester, prepare: weapons);
    e.shell.browser!.select(sword);
    await tester.pumpAndSettle();
    final upgrade = find.byKey(const ValueKey('field.data.$sword.upgrade'));
    await reveal(tester, upgrade);
    expect(find.descendant(of: upgrade, matching: find.text('None')), findsOneWidget);
    await e.click(find.descendant(of: upgrade, matching: find.byKey(const ValueKey('asset.pick'))));
    // The two weapons (the sword itself too); not the shield, not a material.
    expect(find.byKey(const ValueKey('assetPicker.row.1')), findsOneWidget);
    expect(find.byKey(const ValueKey('assetPicker.row.2')), findsNothing);
    await e.click(find.text('Axe').last);
    final file = File('${e.dir.path}/assets/data/sword.data.json');
    expect((jsonDecode(file.readAsStringSync()) as Map)['data'], {
      'upgrade': {'asset': axe},
    });
    expect(find.descendant(of: upgrade, matching: find.text('Axe')), findsOneWidget);
  });

  testWidgets('a record is a foldout, alone and as the items of a list; its fields edit in place', (tester) async {
    final e = await Editor.open(tester, prepare: weapons);
    e.shell.browser!.select(loot);
    await tester.pumpAndSettle();
    final file = File('${e.dir.path}/assets/data/loot.data.json');
    Map<String, Object?> data() => (jsonDecode(file.readAsStringSync()) as Map)['data'] as Map<String, Object?>;

    final best = find.byKey(const ValueKey('field.data.$loot.best'));
    final weight = find.byKey(const ValueKey('field.data.$loot.best.weight'));
    expect(weight, findsOneWidget);
    final box = find.descendant(of: weight, matching: find.byKey(const ValueKey('number.text')));
    await reveal(tester, box);
    await e.click(box);
    await e.type('4');
    await e.submit();
    expect(data()['best'], {'weight': 4});

    // Folded: its fields are gone until it opens again.
    await e.click(find.descendant(of: best, matching: find.byKey(const ValueKey('foldout.toggle'))).first);
    expect(weight, findsNothing);
    await e.click(find.descendant(of: best, matching: find.byKey(const ValueKey('foldout.toggle'))).first);
    expect(weight, findsOneWidget);

    // A list of records: + adds one with the record's defaults.
    final add = find.byKey(const ValueKey('array.add.drops'));
    await reveal(tester, add);
    await e.click(add);
    expect(data()['drops'], [
      {'weight': 1, 'rare': false},
    ]);
    final item = find.byKey(const ValueKey('field.data.$loot.drops.0'));
    expect(find.descendant(of: item, matching: find.byKey(const ValueKey('foldout.toggle'))), findsOneWidget);
    final rare = find.descendant(of: item, matching: find.byType(Checkbox));
    await reveal(tester, rare);
    await e.click(rare);
    expect(data()['drops'], [
      {'weight': 1, 'rare': true},
    ]);
  });
}
