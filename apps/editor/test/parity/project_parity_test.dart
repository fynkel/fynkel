// Unity parity: the Project browser, driven through the whole shell over a
// real project (docs/unity-parity.md § Project).
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:fynkel_editor/src/theme/icons.dart';

import 'harness.dart';

const coin = 'c0000000-0000-4000-8000-000000000042';
const crate = 'c0000000-0000-4000-8000-000000000040';

void main() {
  Future<Editor> inPrefabs(WidgetTester tester) async {
    final e = await Editor.open(tester);
    e.shell.browser!.setFolder('prefabs');
    await tester.pumpAndSettle();
    await e.click(find.byKey(const Key('project.asset.$coin')));
    return e;
  }

  File file(Editor e, String name) => File('${e.dir.path}/assets/prefabs/$name');

  testWidgets('⌘D duplicates the asset beside it, selected; ⌥⌘Z takes it back', (tester) async {
    final e = await inPrefabs(tester);
    final browser = e.shell.browser!;
    await e.press(LogicalKeyboardKey.keyD, command: true);
    expect(file(e, 'coin 1.prefab.json').existsSync(), isTrue);
    final copy = browser.selection.single;
    expect(copy, isNot(coin));
    expect(browser.entry(copy)!.path, 'prefabs/coin 1.prefab.json');
    expect(find.byKey(Key('project.asset.$copy')), findsOneWidget);
    expect(e.rows, ['Camera', 'Robot', 'Sun', 'Crate A', 'Floor', 'Crate B'], reason: 'the Hierarchy kept its ⌘D');

    await e.press(LogicalKeyboardKey.keyZ, command: true, alt: true);
    expect(file(e, 'coin 1.prefab.json').existsSync(), isFalse);
  });

  testWidgets('Enter renames the asset; ⌘↓ opens it', (tester) async {
    final e = await inPrefabs(tester);
    await e.press(LogicalKeyboardKey.enter);
    expect(e.shell.browser!.renamingAsset, coin);
    await e.type('gold coin');
    await e.submit();
    expect(file(e, 'gold coin.prefab.json').existsSync(), isTrue);
    expect(file(e, 'coin.prefab.json').existsSync(), isFalse);

    await e.click(find.byKey(const Key('project.asset.$coin')));
    await e.press(LogicalKeyboardKey.arrowDown, command: true);
    expect(e.shell.tabs!.focused!.path, endsWith('gold coin.prefab.json'), reason: 'the prefab opened in its tab');
  });

  testWidgets('⌘⌫ asks first; Cancel keeps the asset, Delete moves it to the trash', (tester) async {
    final e = await inPrefabs(tester);
    await e.press(LogicalKeyboardKey.keyD, command: true);
    final copy = e.shell.browser!.selection.single;

    await e.press(LogicalKeyboardKey.backspace, command: true);
    expect(find.byKey(const Key('project.deleteConfirm')), findsOneWidget);
    expect(
      find.descendant(
        of: find.byKey(const Key('project.deleteConfirm')),
        matching: find.textContaining('coin 1.prefab.json'),
      ),
      findsOneWidget,
    );
    await e.press(LogicalKeyboardKey.escape);
    expect(find.byKey(const Key('project.deleteConfirm')), findsNothing);
    expect(file(e, 'coin 1.prefab.json').existsSync(), isTrue);

    await e.click(find.byKey(Key('project.asset.$copy')));
    await e.press(LogicalKeyboardKey.backspace, command: true);
    await e.press(LogicalKeyboardKey.enter);
    expect(find.byKey(const Key('project.deleteConfirm')), findsNothing);
    expect(file(e, 'coin 1.prefab.json').existsSync(), isFalse);
    expect(Directory('${e.dir.path}/.fynkel/trash/$copy').existsSync(), isTrue);
  });

  testWidgets('a referenced asset goes straight to the "still referenced" question', (tester) async {
    final e = await inPrefabs(tester);
    await e.click(find.byKey(const Key('project.asset.$crate')));
    await e.press(LogicalKeyboardKey.backspace, command: true);
    expect(find.byKey(const Key('project.deleteConfirm')), findsNothing);
    expect(find.byKey(const Key('project.deleteRefused')), findsOneWidget);
    await e.press(LogicalKeyboardKey.escape);
    expect(file(e, 'crate.prefab.json').existsSync(), isTrue);
  });

  testWidgets('+ → New Volume Profile makes a profile here, selected and renaming', (tester) async {
    final e = await inPrefabs(tester);
    final browser = e.shell.browser!;
    await e.click(find.byKey(const Key('project.new')));
    await e.click(find.byKey(const Key('project.new.volume')));
    final made = file(e, 'New Volume Profile.volume.json');
    expect(made.existsSync(), isTrue);
    final id = browser.selection.single;
    expect(browser.renamingAsset, id);
    expect(browser.entry(id)!.document, 'volume');
    await e.type('Night');
    await e.submit();
    final night = file(e, 'Night.volume.json');
    expect(night.existsSync(), isTrue);
    final json = night.readAsStringSync();
    expect(json, contains('volume.schema.json'));
    // The starting look: ambient occlusion on, bloom ready for an intensity.
    expect(json, contains('ambientOcclusion'));
  });

  testWidgets('Find References In Scene: the Hierarchy searches for the objects that use the asset', (tester) async {
    final e = await Editor.open(tester);
    const robot = 'c0000000-0000-4000-8000-000000000001';
    Future<void> findReferences(String folder, String id) async {
      e.shell.browser!.setFolder(folder);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(Key('project.asset.$id')), buttons: kSecondaryMouseButton);
      await tester.pumpAndSettle();
      await e.click(find.byKey(const Key('project.menu.findReferences')));
    }

    // The model: the Robot, and Crate A whose prefab draws it.
    await findReferences('models', robot);
    final search = tester.widget<TextField>(find.byKey(const Key('hierarchy.search')));
    expect(search.controller!.text, startsWith('ref:$robot:'));
    expect(e.rows, ['Robot', 'Crate A']);

    // The prefab: its one instance.
    await findReferences('prefabs', crate);
    expect(e.rows, ['Crate A']);

    // × clears the search: every object again.
    await e.click(find.descendant(of: find.byKey(const Key('hierarchy.search')), matching: find.byIcon(FIcons.close)));
    expect(e.rows, ['Camera', 'Robot', 'Sun', 'Crate A', 'Floor', 'Crate B']);
  });
}
