// Unity parity: a script's object fields in the Inspector
// (docs/unity-parity.md § Tier 2, Inspector fields). A `GameObject?` field
// and a field of another script type show `None (Type)`, take a Hierarchy
// row dropped on them — a script field only from an object that has that
// script — or a pick from ⊙, save as `{entity}` / `{entity, component}`,
// clear with ×, and show `Missing` once the object is deleted. One field
// for every type (ADR-0036 §6): a click pings what it holds, ⊙ opens a
// picker with a Scene tab and an Assets tab, and an asset field reads
// `None (Audio Clip)` or `Missing (Audio Clip)` as an object field does.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const robot = 'c0000000-0000-4000-8000-0000000000e1';
const robotSpinner = 'c0000000-0000-4000-8000-0000000000c2';
const crateB = 'c0000000-0000-4000-8000-0000000000e5';
const floor = 'c0000000-0000-4000-8000-0000000000e4';
const sun = 'c0000000-0000-4000-8000-0000000000e3';

/// The schema `fynkel codegen` writes for `class Turret`, whose fields
/// are `GameObject? target`, `Spinner? spinner`,
/// `NativeComponent<DirectionalLight>? light`, `AudioClipAsset? sound` and
/// `Mode mode`.
Map<String, Object?> turretSchema() {
  Map<String, Object?> reference(String type, {String? behaviour, String? component}) => {
    'type': ['object', 'null'],
    'properties': {
      'entity': {'type': 'string'},
      if (behaviour != null) 'component': {'type': 'string'},
    },
    'required': ['entity', if (behaviour != null) 'component'],
    'additionalProperties': false,
    'x-fynkel': {'type': type, 'nullable': true, 'behaviour': ?behaviour, 'component': ?component},
  };
  return {
    r'$schema': 'https://json-schema.org/draft/2020-12/schema',
    r'$id': 'https://fynkel.dev/schemas/behaviours/game.Turret.schema.json',
    'title': 'game.Turret',
    'type': 'object',
    'properties': {
      'target': reference('gameObject'),
      'spinner': reference('behaviour', behaviour: 'game.Spinner'),
      // `NativeComponent<DirectionalLight>? light` (ADR-0036 §2).
      'light': reference('component', component: 'fynkel.DirectionalLight'),
      'sound': {
        'anyOf': [
          {r'$ref': 'https://fynkel.dev/schemas/envelope.schema.json#/\$defs/assetRef'},
          {'type': 'null'},
        ],
        'x-fynkel': {'type': 'assetRef', 'nullable': true, 'kind': 'asset(audio)'},
      },
      'mode': {
        'type': 'string',
        'enum': ['idle', 'aim', 'fire'],
        'default': 'idle',
        'x-fynkel': {'type': 'enum'},
      },
    },
    'additionalProperties': false,
    'x-fynkel': {'kind': 'behaviour', 'id': 'game.Turret', 'version': 1, 'dart': 'package:game/turret.dart#Turret'},
  };
}

/// The fixture copy with the Turret script on the Sun, its fields [data].
void addTurret(Directory dir, {Map<String, Object?> data = const {'mode': 'idle'}}) {
  File('${dir.path}/schemas/behaviours/game.Turret.schema.json').writeAsStringSync(jsonEncode(turretSchema()));
  final file = File('${dir.path}/assets/scenes/main.scene.json');
  final scene = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
  (scene['entities'][sun]['components'] as Map<String, dynamic>)['c0000000-0000-4000-8000-0000000000f1'] = {
    'type': 'game.Turret',
    'version': 1,
    'data': data,
  };
  file.writeAsStringSync(jsonEncode(scene));
}

void main() {
  Finder field(String name) => find.byKey(ValueKey('field.game.Turret#0.$name'));
  String face(Editor e, String name) => e.tester
      .widget<Text>(find.descendant(of: field(name), matching: find.byKey(const ValueKey('object.name'))))
      .data!;
  String assetFace(Editor e, String name) => e.tester
      .widget<Text>(find.descendant(of: field(name), matching: find.byKey(const ValueKey('asset.name'))))
      .textSpan!
      .toPlainText();
  Map<String, dynamic> turretData(Editor e) {
    final components = e.savedScene()['entities'][sun]['components'] as Map<String, dynamic>;
    return components['c0000000-0000-4000-8000-0000000000f1']['data'] as Map<String, dynamic>;
  }

  testWidgets('drop a Hierarchy row on a GameObject field and on a script field; saved as references', (tester) async {
    final e = await Editor.open(tester, prepare: addTurret);
    await e.click(e.row('Sun'));
    expect(face(e, 'target'), 'None (GameObject)');
    expect(face(e, 'spinner'), 'None (Spinner)');

    await e.drag(e.row('Crate B'), tester.getCenter(field('target')));
    expect(face(e, 'target'), 'Crate B');
    // Crate B has no Spinner: the script field refuses it.
    await e.drag(e.row('Crate B'), tester.getCenter(field('spinner')));
    expect(face(e, 'spinner'), 'None (Spinner)');
    await e.drag(e.row('Robot'), tester.getCenter(field('spinner')));
    expect(face(e, 'spinner'), 'Robot (Spinner)');
    expect(e.selectedNames, ['Sun'], reason: 'a drop does not change the selection');

    await e.press(LogicalKeyboardKey.keyS, command: true);
    expect(turretData(e)['target'], {'entity': crateB});
    expect(turretData(e)['spinner'], {'entity': robot, 'component': robotSpinner});

    // One undo per assignment.
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(face(e, 'spinner'), 'None (Spinner)');
    expect(face(e, 'target'), 'Crate B');
  });

  testWidgets(
    '⊙ lists the objects (a script field only those with the script); × clears; a deleted target is Missing',
    (tester) async {
      final e = await Editor.open(tester, prepare: addTurret);
      await e.click(e.row('Sun'));

      await e.click(find.descendant(of: field('spinner'), matching: find.byKey(const ValueKey('object.pick'))));
      expect(find.byKey(const ValueKey('objectPicker.row.0')), findsOneWidget);
      expect(find.byKey(const ValueKey('objectPicker.row.1')), findsNothing, reason: 'only the Robot has a Spinner');
      await e.click(find.byKey(const ValueKey('objectPicker.row.0')));
      expect(face(e, 'spinner'), 'Robot (Spinner)');

      await e.click(find.descendant(of: field('target'), matching: find.byKey(const ValueKey('object.pick'))));
      await tester.enterText(find.byKey(const ValueKey('objectPicker.search')), 'Floor');
      await tester.pumpAndSettle();
      final rows = find.byWidgetPredicate(
        (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('objectPicker.row.'),
      );
      expect(rows, findsOneWidget, reason: 'the search leaves the Floor');
      await e.click(rows);
      expect(face(e, 'target'), 'Floor');

      await e.click(find.descendant(of: field('target'), matching: find.byTooltip('Clear')));
      expect(face(e, 'target'), 'None (GameObject)');
      await e.press(LogicalKeyboardKey.keyZ, command: true);
      expect(face(e, 'target'), 'Floor');

      // Delete the Floor, then look at the Sun again.
      await e.click(e.row('Floor'));
      await e.press(LogicalKeyboardKey.backspace, command: true);
      expect(e.rows, isNot(contains('Floor')));
      await e.click(e.row('Sun'));
      expect(face(e, 'target'), 'Missing (GameObject)');
      await e.press(LogicalKeyboardKey.keyS, command: true);
      expect(turretData(e)['target'], {'entity': floor}, reason: 'the reference is kept, as Unity keeps it');
    },
  );

  testWidgets('a built-in component field takes only objects with that component (ADR-0036 §2)', (tester) async {
    final e = await Editor.open(tester, prepare: addTurret);
    await e.click(e.row('Sun'));
    expect(face(e, 'light'), 'None (Directional Light)');

    // Crate B has no light: the field refuses it.
    await e.drag(e.row('Crate B'), tester.getCenter(field('light')));
    expect(face(e, 'light'), 'None (Directional Light)');

    await e.click(find.descendant(of: field('light'), matching: find.byKey(const ValueKey('object.pick'))));
    expect(find.byKey(const ValueKey('objectPicker.row.0')), findsOneWidget);
    expect(find.byKey(const ValueKey('objectPicker.row.1')), findsNothing, reason: 'only the Sun has a light');
    await e.click(find.byKey(const ValueKey('objectPicker.row.0')));
    expect(face(e, 'light'), 'Sun (Directional Light)');

    await e.press(LogicalKeyboardKey.keyS, command: true);
    expect(turretData(e)['light'], {'entity': sun}, reason: 'the object names its one light');
  });

  testWidgets('a click on a set field pings its object in the Hierarchy; an empty one opens the picker', (tester) async {
    final e = await Editor.open(tester, prepare: addTurret);
    await e.click(e.row('Sun'));
    await e.click(find.descendant(of: field('target'), matching: find.byKey(const ValueKey('object.name'))));
    expect(find.byKey(const ValueKey('objectPicker.search')), findsOneWidget, reason: 'None: the click picks');
    await e.press(LogicalKeyboardKey.escape);

    await e.drag(e.row('Crate B'), tester.getCenter(field('target')));
    await e.click(find.descendant(of: field('target'), matching: find.byKey(const ValueKey('object.name'))));
    expect(find.byKey(const ValueKey('objectPicker.search')), findsNothing, reason: 'a ping, not the picker');
    expect(find.byKey(const Key('hierarchy.pinged.$crateB')), findsOneWidget, reason: 'the row is outlined');
    expect(e.selectedNames, ['Sun'], reason: 'a ping selects nothing');

    await e.click(e.row('Robot'));
    expect(find.byKey(const Key('hierarchy.pinged.$crateB')), findsNothing, reason: 'the next selection clears it');
  });

  testWidgets('⊙ has a Scene tab and an Assets tab, filtered by the field\'s type', (tester) async {
    final e = await Editor.open(tester, prepare: addTurret);
    await e.click(e.row('Sun'));
    Finder tab(String prefix, String name) => find.byKey(ValueKey('$prefix.tab.$name'));
    bool on(String prefix, String name) => e.tester.widget<Semantics>(tab(prefix, name)).properties.selected ?? false;

    // An object field opens on the Scene; its Assets hold no scene object.
    await e.click(find.descendant(of: field('target'), matching: find.byKey(const ValueKey('object.pick'))));
    expect((on('objectPicker', 'Scene'), on('objectPicker', 'Assets')), (true, false));
    expect(find.byKey(const ValueKey('objectPicker.row.0')), findsOneWidget);
    await e.click(tab('objectPicker', 'Assets'));
    expect(on('objectPicker', 'Assets'), isTrue);
    expect(find.byKey(const ValueKey('objectPicker.row.0')), findsNothing);
    expect(find.byKey(const ValueKey('objectPicker.none')), findsOneWidget, reason: 'None is on every tab');
    await e.press(LogicalKeyboardKey.escape);

    // An asset field opens on the Assets: the one sound.
    await e.click(find.descendant(of: field('sound'), matching: find.byKey(const ValueKey('asset.pick'))));
    expect((on('assetPicker', 'Scene'), on('assetPicker', 'Assets')), (false, true));
    expect(find.byKey(const ValueKey('assetPicker.row.0')), findsOneWidget);
    expect(find.byKey(const ValueKey('assetPicker.row.1')), findsNothing, reason: 'only the sound, no mesh');
    await e.click(tab('assetPicker', 'Scene'));
    expect(find.byKey(const ValueKey('assetPicker.row.0')), findsNothing, reason: 'no scene object is a sound');
    await e.click(tab('assetPicker', 'Assets'));
    await e.click(find.byKey(const ValueKey('assetPicker.row.0')));
    expect(assetFace(e, 'sound'), 'tone.wav');
  });

  testWidgets('an asset field reads None (Type), and Missing (Type) when its asset is gone', (tester) async {
    final e = await Editor.open(
      tester,
      prepare: (dir) => addTurret(
        dir,
        data: {
          'mode': 'idle',
          'sound': {'asset': 'dead0000-0000-4000-8000-000000000000'},
        },
      ),
    );
    await e.click(e.row('Sun'));
    expect(assetFace(e, 'sound'), 'Missing (Audio Clip)');
    await e.click(find.descendant(of: field('sound'), matching: find.byTooltip('Clear')));
    expect(assetFace(e, 'sound'), 'None (Audio Clip)');
  });
}
