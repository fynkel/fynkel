// Unity parity: the Physics Material (ADR-0025; docs/unity-parity.md
// § Tier 3). Create ▸ Physics Material in the Project makes a
// `.physicsmaterial.json` whose Inspector edits Friction, Bounciness and
// the two combine rules, each change saved at once and undone by ⌘Z. A
// Collider's Material field picks one; while it is set, the collider's
// own Friction and Bounciness are hidden — the material's replace them.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const floor = 'c0000000-0000-4000-8000-0000000000e4';
const material = 'field.fynkel.Collider#0.material';
const friction = 'field.fynkel.Collider#0.friction';
const restitution = 'field.fynkel.Collider#0.restitution';

void main() {
  Future<void> reveal(WidgetTester tester, Finder f) async {
    await Scrollable.ensureVisible(tester.element(f), alignment: 0.5);
    await settleCooks(tester);
  }

  testWidgets('New Physics Material: the Inspector edits friction, bounciness and the combine rules; ⌘Z', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final scene = e.shell.tabs!.focused!;
    e.shell.browser!.setFolder('');
    await settleCooks(tester);
    await e.click(find.byKey(const Key('project.new')));
    await e.click(find.byKey(const Key('project.new.physics_material')));
    final file = File('${e.dir.path}/assets/New Physics Material.physicsmaterial.json');
    expect(file.existsSync(), isTrue);
    Map<String, Object?> saved() => jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
    await e.press(LogicalKeyboardKey.escape);

    Finder inRow(String field, Finder f) => find.descendant(of: find.byKey(Key('physicsMaterial.$field')), matching: f);
    expect(find.byKey(const Key('physicsMaterial.friction')), findsOneWidget);
    expect(inRow('friction', find.text('Friction')), findsOneWidget);

    // Bounciness typed: saved at once.
    final box = inRow('bounciness', find.byKey(const ValueKey('number.text')));
    await reveal(tester, box);
    await e.click(box);
    await e.type('1');
    await e.submit();
    expect(saved()['bounciness'], 1);

    // Friction Combine: Unity's four rules.
    await reveal(tester, find.byKey(const Key('physicsMaterial.friction_combine')));
    await e.click(inRow('friction_combine', find.text('Average')));
    for (final rule in ['Minimum', 'Multiply', 'Maximum']) {
      expect(find.text(rule), findsWidgets);
    }
    await e.click(find.text('Maximum').last);
    expect(saved()['friction_combine'], 'maximum');

    // ⌘Z in the Inspector undoes the material, step by step; the scene is untouched.
    expect(e.shell.registry.byId('document.undo')!.label, 'Undo Physics Material Change');
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(saved()['friction_combine'], isNot('maximum'));
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(saved()['bounciness'] ?? 0, 0);
    expect(scene.document.history, 0);
  });

  testWidgets("a Collider's Material picks one; its own friction and bounciness give way to the material's", (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final tab = e.shell.tabs!.focused!;
    final bouncy = e.shell.project!
        .newDocument('physics_material', 'physics/Bouncy.physicsmaterial.json', name: 'Bouncy')
        .id!;
    await settleCooks(tester);
    await e.click(e.row('Floor'));
    Map<String, Object?> collider() {
      final components = (tab.document.root()['entities'] as Map)[floor]['components'] as Map;
      return (components.values.firstWhere((c) => (c as Map)['type'] == 'fynkel.Collider') as Map)['data']
          as Map<String, Object?>;
    }

    await reveal(tester, find.byKey(const ValueKey(material)));
    expect(find.descendant(of: find.byKey(const ValueKey(material)), matching: find.text('None (Physics Material)')), findsOneWidget);
    expect(find.byKey(const ValueKey(friction)), findsOneWidget);
    expect(
      find.descendant(of: find.byKey(const ValueKey(restitution)), matching: find.text('Bounciness')),
      findsOneWidget,
      reason: "Unity's name",
    );

    // ⊙: only physics materials are offered.
    await e.click(
      find.descendant(of: find.byKey(const ValueKey(material)), matching: find.byKey(const ValueKey('asset.pick'))),
    );
    expect(find.textContaining('Choose a physics material'), findsOneWidget);
    expect(find.byKey(const ValueKey('assetPicker.row.0')), findsOneWidget);
    expect(find.byKey(const ValueKey('assetPicker.row.1')), findsNothing);
    await e.click(find.byKey(const ValueKey('assetPicker.row.0')));
    expect(collider()['material'], {'asset': bouncy});
    expect(find.descendant(of: find.byKey(const ValueKey(material)), matching: find.text('Bouncy')), findsOneWidget);
    expect(find.byKey(const ValueKey(friction)), findsNothing, reason: "the material's friction replaces it");
    expect(find.byKey(const ValueKey(restitution)), findsNothing);

    // ⌘Z: the collider's own values again.
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(collider()['material'], isNull);
    expect(find.byKey(const ValueKey(friction)), findsOneWidget);
  });
}
