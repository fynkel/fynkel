// Hierarchy → Project through the whole shell (the gate finding of
// 2026-09-24): the Project panel asks the Hierarchy whether the dragged
// objects can become a prefab, and "no reason" must mean yes.

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';

import 'support/fakes.dart';

void main() {
  testWidgets('a Hierarchy row dragged onto the Project list is made into a prefab', (tester) async {
    final project = FakeProject(dir: '/fake/game');
    final doc = project.addDocument(
      FakeDocument(
        path: '/fake/game/assets/scenes/s.scene.json',
        id: 'scene-s',
        name: 'S',
        relativePath: 'scenes/s.scene.json',
        entities: {
          'crate': FakeDocument.entity('Crate', position: [4, 0, 0]),
        },
      ),
    );
    await pumpLarge(
      tester,
      shellApp(prefs: Preferences.inMemory(), projects: {'/fake/game': project}, initialProject: '/fake/game'),
    );
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    final browser = shell.browser!;
    expect(browser.canMakePrefab(['crate']), isFalse, reason: 'no scene is open yet');

    shell.tabs!.openDocument(doc.id);
    await tester.pumpAndSettle();
    expect(browser.canMakePrefab(['crate']), isTrue);
    final made = <(List<String>, String)>[];
    browser.makePrefab = (ids, folder) async {
      made.add((ids, folder));
      return null;
    };

    final row = find.descendant(of: find.byKey(const Key('hierarchy.list')), matching: find.text('Crate'));
    final g = await tester.startGesture(tester.getCenter(row));
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveBy(const Offset(20, 0));
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveTo(tester.getCenter(find.byKey(const Key('project.prefabDrop'))));
    await tester.pump(const Duration(milliseconds: 50));
    await g.up();
    await tester.pumpAndSettle();
    expect(made, hasLength(1));
    expect(made.single.$1, ['crate']);
  });
}
