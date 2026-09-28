// Unity parity: a script's object fields in the Inspector
// (docs/unity-parity.md § Tier 2, Inspector fields). A `GameObject?` field
// and a field of another script type show `None (Type)`, take a Hierarchy
// row dropped on them — a script field only from an object that has that
// script — or a pick from ⊙, save as `{entity}` / `{entity, component}`,
// clear with ×, and show `Missing` once the object is deleted.
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

/// The schema `fynkel codegen` writes for
/// `class Turret { GameObject? target; Spinner? spinner;
/// NativeComponent<DirectionalLight>? light; Mode mode; }`.
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

/// The fixture copy with the Turret script on the Sun.
void addTurret(Directory dir) {
  File('${dir.path}/schemas/behaviours/game.Turret.schema.json').writeAsStringSync(jsonEncode(turretSchema()));
  final file = File('${dir.path}/assets/scenes/main.scene.json');
  final scene = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
  (scene['entities'][sun]['components'] as Map<String, dynamic>)['c0000000-0000-4000-8000-0000000000f1'] = {
    'type': 'game.Turret',
    'version': 1,
    'data': {'mode': 'idle'},
  };
  file.writeAsStringSync(jsonEncode(scene));
}

void main() {
  Finder field(String name) => find.byKey(ValueKey('field.game.Turret#0.$name'));
  String face(Editor e, String name) => e.tester
      .widget<Text>(find.descendant(of: field(name), matching: find.byKey(const ValueKey('object.name'))))
      .data!;
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
}
