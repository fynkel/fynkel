// Unity parity: the Hierarchy, driven through the whole shell over a real
// project (docs/unity-parity.md § Hierarchy).
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

void main() {
  testWidgets('the scene opens with its objects in file order', (tester) async {
    final e = await Editor.open(tester);
    expect(e.rows, ['Camera', 'Robot', 'Sun', 'Crate A', 'Floor', 'Crate B']);
  });

  testWidgets('⌘D duplicates right after the original and selects the copy', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Sun'));
    await e.press(LogicalKeyboardKey.keyD, command: true);
    expect(e.rows, ['Camera', 'Robot', 'Sun', 'Sun (1)', 'Crate A', 'Floor', 'Crate B']);
    expect(e.selectedNames, ['Sun (1)']);
  });

  testWidgets('F2 renames, Enter commits, ⌘Z undoes', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Floor'));
    await e.press(LogicalKeyboardKey.f2);
    await e.type('Ground');
    await e.submit();
    expect(e.rows, contains('Ground'));
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(e.rows, contains('Floor'));
    await e.press(LogicalKeyboardKey.keyZ, command: true, shift: true);
    expect(e.rows, contains('Ground'));
  });

  testWidgets('⌘⌫ deletes the selection; ⌘Z brings it back in its place', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Robot'));
    await e.press(LogicalKeyboardKey.backspace, command: true);
    expect(e.rows, ['Camera', 'Sun', 'Crate A', 'Floor', 'Crate B']);
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(e.rows, ['Camera', 'Robot', 'Sun', 'Crate A', 'Floor', 'Crate B']);
  });

  testWidgets('a double-click frames instead of renaming; Esc then F2 shows the real name', (tester) async {
    final e = await Editor.open(tester);
    await tester.tap(e.row('Sun'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(e.row('Sun'));
    await settleCooks(tester);
    expect(find.byType(TextField).evaluate().where((el) => '${el.widget.key}'.contains('hierarchy.rename')), isEmpty);
    await e.press(LogicalKeyboardKey.f2);
    await e.type('Moon');
    await e.press(LogicalKeyboardKey.escape);
    expect(e.rows, contains('Sun'));
    await e.press(LogicalKeyboardKey.f2);
    final id = e.shell.tabs!.focused!.selection.single;
    final field = tester.widget<TextField>(find.byKey(Key('hierarchy.rename.$id')));
    expect(field.controller!.text, 'Sun');
  });

  testWidgets('deleting leaves nothing selected', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Robot'));
    await e.press(LogicalKeyboardKey.backspace, command: true);
    expect(e.selectedNames, isEmpty);
  });

  testWidgets('⌘⇧N creates an empty object, selected and ready to rename', (tester) async {
    final e = await Editor.open(tester);
    await e.press(LogicalKeyboardKey.keyN, command: true, shift: true);
    expect(e.selectedNames, ['GameObject']);
    expect(e.shell.tabs!.focused!.tree.roots.last, e.shell.tabs!.focused!.selection.single, reason: 'at the root');
    final id = e.shell.tabs!.focused!.selection.single;
    expect(find.byKey(Key('hierarchy.rename.$id')), findsOneWidget, reason: 'Unity starts renaming the new object');
  });

  testWidgets('⌘S saves and the file reloads the same', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Floor'));
    await e.press(LogicalKeyboardKey.f2);
    await e.type('Ground');
    await e.submit();
    await e.press(LogicalKeyboardKey.keyS, command: true);
    final names = [for (final v in (e.savedScene()['entities'] as Map).values) (v as Map)['name']];
    expect(names, contains('Ground'));
  });

  testWidgets('⌘C / ⌘V copies an object', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Sun'));
    await e.press(LogicalKeyboardKey.keyC, command: true);
    await e.press(LogicalKeyboardKey.keyV, command: true);
    expect(e.rows, [
      'Camera',
      'Robot',
      'Sun',
      'Sun',
      'Crate A',
      'Floor',
      'Crate B',
    ], reason: 'right after the selection');
    expect(e.selectedNames, ['Sun']);
    expect(
      e.shell.tabs!.focused!.selection.single,
      isNot(e.shell.tabs!.focused!.tree.roots[2]),
      reason: 'the copy is selected',
    );
  });

  testWidgets('⌘X then ⌘⇧V moves an object under another; ⌘Z undoes each', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Crate B'));
    await e.press(LogicalKeyboardKey.keyX, command: true);
    expect(e.rows, isNot(contains('Crate B')));
    await e.click(e.row('Floor'));
    await e.press(LogicalKeyboardKey.keyV, command: true, shift: true);
    final tab = e.shell.tabs!.focused!;
    final pasted = tab.tree[tab.selection.single]!;
    expect(pasted.name, 'Crate B');
    expect(tab.tree[pasted.parent!]!.name, 'Floor');
    expect(
      pasted.componentTypes,
      containsAll(['fynkel.RigidBody', 'fynkel.Collider']),
      reason: 'components come along',
    );
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(e.rows, ['Camera', 'Robot', 'Sun', 'Crate A', 'Floor', 'Crate B']);
  });

  testWidgets('a copied object pastes into another scene', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Sun'));
    await e.press(LogicalKeyboardKey.keyC, command: true);
    e.shell.tabs!.openDocument('assets/scenes/results.scene.json');
    await settleCooks(tester);
    final before = e.rows.length;
    await e.press(LogicalKeyboardKey.keyV, command: true);
    expect(e.rows, hasLength(before + 1));
    expect(e.selectedNames, ['Sun']);
  });

  testWidgets('dragging a root between two others reorders the roots', (tester) async {
    final e = await Editor.open(tester);
    // The top quarter of a row is "above it".
    await e.drag(e.row('Crate B'), e.above('Floor'));
    expect(e.rows, ['Camera', 'Robot', 'Sun', 'Crate A', 'Crate B', 'Floor']);
    await e.drag(e.row('Sun'), e.above('Camera'));
    expect(e.rows, ['Sun', 'Camera', 'Robot', 'Crate A', 'Crate B', 'Floor']);
    await e.press(LogicalKeyboardKey.keyS, command: true);
    final saved = e.savedScene();
    final roots = [
      for (final v in (saved['entities'] as Map).values)
        if ((v as Map)['parent'] == null) v['name'],
    ];
    expect(roots, ['Sun', 'Camera', 'Robot', 'Crate A', 'Crate B', 'Floor'], reason: 'the order is saved');
  });

  testWidgets('several rows dragged together keep their order', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Camera'));
    await e.click(e.row('Robot'), command: true);
    await e.drag(e.row('Robot'), e.above('Crate B'));
    expect(e.rows, ['Sun', 'Crate A', 'Floor', 'Camera', 'Robot', 'Crate B']);
  });

  testWidgets('⌥⇧N creates an empty child of the selection', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Sun'));
    await e.press(LogicalKeyboardKey.keyN, shift: true, alt: true);
    final tab = e.shell.tabs!.focused!;
    final made = tab.tree[tab.selection.single]!;
    expect(made.name, 'GameObject');
    expect(tab.tree[made.parent!]!.name, 'Sun');
  });

  testWidgets('dragging a row onto another makes it a child', (tester) async {
    final e = await Editor.open(tester);
    await e.drag(e.row('Crate B'), tester.getCenter(e.row('Floor')));
    final tree = e.shell.tabs!.focused!.tree;
    final floor = tree.nodes.values.firstWhere((n) => n.name == 'Floor');
    final crate = tree.nodes.values.firstWhere((n) => n.name == 'Crate B');
    expect(crate.parent, floor.id);
  });

  testWidgets('undo and redo bring back the selection the step was made with', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Sun'));
    await e.press(LogicalKeyboardKey.keyD, command: true);
    expect(e.selectedNames, ['Sun (1)']);
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(e.selectedNames, ['Sun']);
    await e.press(LogicalKeyboardKey.keyZ, command: true, shift: true);
    expect(e.selectedNames, ['Sun (1)']);

    await e.click(e.row('Robot'));
    await e.click(e.row('Floor'), command: true);
    await e.press(LogicalKeyboardKey.backspace, command: true);
    expect(e.selectedNames, isEmpty);
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(e.selectedNames, ['Robot', 'Floor']);
  });

  testWidgets('⌥-click on the arrow opens or closes everything below', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Floor'));
    await e.press(LogicalKeyboardKey.keyN, shift: true, alt: true);
    await e.submit();
    await e.press(LogicalKeyboardKey.keyN, shift: true, alt: true);
    await e.press(LogicalKeyboardKey.escape);
    int made() => e.rows.where((r) => r == 'GameObject').length;
    expect(made(), 2);
    final floor = e.shell.tabs!.focused!.tree.nodes.values.firstWhere((n) => n.name == 'Floor').id;
    final arrow = find.byKey(Key('hierarchy.expander.$floor'));

    Future<void> altClick() async {
      await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
      await tester.tap(arrow);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
      await settleCooks(tester);
    }

    await altClick();
    expect(made(), 0);
    await e.click(arrow);
    expect(made(), 1, reason: 'the child was closed too');
    await e.click(arrow);
    await altClick();
    expect(made(), 2);
  });

  testWidgets('right-click on empty space offers Create Empty at the root, and Paste', (tester) async {
    final e = await Editor.open(tester);
    final list = tester.getRect(find.byKey(const Key('hierarchy.list')));
    final below = Offset(list.center.dx, e.rowRect('Crate B').bottom + 40);
    expect(below.dy, lessThan(list.bottom));
    await tester.tapAt(below, buttons: kSecondaryMouseButton);
    await settleCooks(tester);
    expect(find.byKey(const Key('hierarchy.empty.paste')), findsOneWidget);
    await e.click(find.byKey(const Key('hierarchy.empty.createEmpty')));
    final tab = e.shell.tabs!.focused!;
    expect(e.selectedNames, ['GameObject']);
    expect(tab.tree.roots.last, tab.selection.single, reason: 'the last root, renaming');
    expect(tab.tree[tab.selection.single]!.parent, isNull);
  });

  testWidgets('+ → Create Empty with an object selected: the new child takes typing at once', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Floor'));
    await e.click(find.byKey(const Key('hierarchy.create')));
    await e.click(find.byKey(const Key('hierarchy.create.empty')));
    await e.type('Tile');
    await e.submit();
    final tab = e.shell.tabs!.focused!;
    expect(e.selectedNames, ['Tile']);
    expect(tab.tree[tab.tree[tab.selection.single]!.parent!]!.name, 'Floor');
  });
}
