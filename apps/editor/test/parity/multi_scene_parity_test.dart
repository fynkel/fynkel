// Unity parity: multi-scene editing (docs/unity-parity.md § Tier 3). Every
// open scene is a section of the Hierarchy under its header, the focused
// one the active scene (bold). Open Scene Additive (or a scene dropped on
// the Hierarchy) adds one; a double-click in the Project opens a scene
// alone, the others closing (asking about unsaved changes). A click in
// another scene activates it and selects there; objects dragged onto
// another scene's header move to it. A scene unloads to its header and
// loads back; the Scene view camera stays put between loaded scenes.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/documents/open_document.dart';

import 'harness.dart';

const results = 'c0000000-0000-4000-8000-000000000033';
const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const floor = 'c0000000-0000-4000-8000-0000000000e4';
const trophy = 'c0000000-0000-4000-8000-0000000003e1';

void main() {
  Finder header(OpenDocument s) => find.byKey(Key('hierarchy.scene.${s.document.id}'));
  FontWeight? weight(Editor e, OpenDocument s) => e.tester
      .widget<Text>(find.byKey(Key('hierarchy.scene.name.${s.document.id}')))
      .style
      ?.fontWeight;

  Future<(Editor, OpenDocument, OpenDocument)> twoScenes(WidgetTester tester) async {
    final e = await Editor.open(tester);
    final main = e.shell.tabs!.focused!;
    // Open Scene Additive from the Project's menu.
    e.shell.browser!.setFolder('scenes');
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('project.asset.$results')), buttons: kSecondaryButton);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('project.menu.openAdditive')));
    await tester.pumpAndSettle();
    final added = e.shell.tabs!.scenes.firstWhere((s) => s.document.id == results);
    return (e, main, added);
  }

  testWidgets('Open Scene Additive: both scenes under headers, the active one bold; a click in the other activates it', (
    tester,
  ) async {
    final (e, main, added) = await twoScenes(tester);
    expect(e.shell.tabs!.focused, same(main), reason: 'the active scene stays active');
    expect(header(main), findsOneWidget);
    expect(header(added), findsOneWidget);
    expect(weight(e, main), FontWeight.w700);
    expect(weight(e, added), FontWeight.w500);
    expect(find.byKey(const ValueKey('row.$trophy')), findsOneWidget, reason: "the other scene's objects are listed");
    expect(
      tester.getTopLeft(header(added)).dy,
      greaterThan(tester.getTopLeft(find.byKey(const ValueKey('row.$floor'))).dy),
      reason: 'each scene under its own header, in tab order',
    );

    await e.click(e.row('Crate A'));
    expect(main.selection, [crateA]);
    final camera = e.shell.scene!.camera!.toJson();

    // A click on the other scene's object: that scene is active, the object selected, the old selection gone.
    await e.click(find.byKey(const ValueKey('row.$trophy')));
    expect(e.shell.tabs!.focused, same(added));
    expect(added.selection, [trophy]);
    expect(main.selection, isEmpty);
    expect(weight(e, added), FontWeight.w700);
    expect(e.shell.scene!.camera!.toJson(), camera, reason: 'one Scene view camera across the loaded scenes');

    // The header's Set Active Scene.
    await tester.tap(find.byKey(Key('hierarchy.scene.menu.${main.document.id}')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(Key('hierarchy.scene.${main.document.id}.setActive')));
    await tester.pumpAndSettle();
    expect(e.shell.tabs!.focused, same(main));
  });

  testWidgets("rows dragged onto another scene's header move there; each scene's undo has its step", (tester) async {
    final (e, main, added) = await twoScenes(tester);
    await e.click(e.row('Crate A'));
    await e.drag(e.row('Crate A'), tester.getCenter(header(added)));
    expect(main.tree[crateA], isNull, reason: 'gone from Main');
    expect(e.shell.tabs!.focused, same(added), reason: 'the target scene is active, the moved object selected');
    final moved = added.selection.single;
    expect(added.tree[moved]?.name, 'Crate A');
    expect(added.tree[moved]?.isPrefabInstance, isTrue, reason: 'still an instance of its prefab');
    expect(main.dirty, isTrue);
    expect(added.dirty, isTrue);

    // Onto one of the other scene's objects: under it.
    await e.click(e.row('Floor'));
    expect(e.shell.tabs!.focused, same(main));
    await e.drag(e.row('Floor'), tester.getCenter(find.byKey(ValueKey('row.$moved'))));
    expect(main.tree[floor], isNull);
    final floorNow = added.selection.single;
    expect(added.tree[floorNow]?.parent, moved);

    // Main's history undoes its half (Floor back), Results' its own.
    main.undo();
    expect(main.tree[floor]?.name, 'Floor');
  });

  testWidgets('Unload / Load from the header; the Project double-click opens a scene alone, asking first', (
    tester,
  ) async {
    final (e, main, added) = await twoScenes(tester);
    await tester.tap(find.byKey(Key('hierarchy.scene.menu.$results')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('hierarchy.scene.$results.unload')));
    await tester.pumpAndSettle();
    expect(e.shell.tabs!.isLoaded(added), isFalse);
    expect(find.byKey(const ValueKey('row.$trophy')), findsNothing, reason: 'an unloaded scene is its header alone');
    expect(find.textContaining('(not loaded)'), findsOneWidget);

    // The last loaded scene cannot unload.
    await tester.tap(find.byKey(Key('hierarchy.scene.menu.${main.document.id}')));
    await tester.pumpAndSettle();
    expect(
      tester.widget<MenuItemButton>(find.byKey(Key('hierarchy.scene.${main.document.id}.unload'))).onPressed,
      isNull,
    );
    await tester.tapAt(Offset.zero);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(Key('hierarchy.scene.menu.$results')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('hierarchy.scene.$results.load')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('row.$trophy')), findsOneWidget);

    // A change in Main, then Results double-clicked: Main asks; Cancel keeps both, Discard leaves Results alone.
    e.shell.hierarchy!.rename(crateA, 'Crate Z');
    await tester.pumpAndSettle();
    Future<void> doubleClick() async {
      await tester.tap(find.byKey(const Key('project.asset.$results')));
      await tester.pump(const Duration(milliseconds: 50));
      await tester.tap(find.byKey(const Key('project.asset.$results')));
      await tester.pumpAndSettle();
    }

    await doubleClick();
    await tester.tap(find.byKey(const Key('closePrompt.cancel')));
    await tester.pumpAndSettle();
    expect(e.shell.tabs!.scenes, hasLength(2));
    await doubleClick();
    await tester.tap(find.byKey(const Key('closePrompt.discard')));
    await tester.pumpAndSettle();
    expect(e.shell.tabs!.scenes.map((s) => s.document.id), [results]);
    expect(e.shell.tabs!.focused, same(added));
  });
}
