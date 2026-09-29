// Unity parity: Prefab mode (docs/unity-parity.md § Tier 3). An
// instance's › in the Hierarchy (or Open in its Inspector) opens its prefab
// in context: the Scene view's bar shows the breadcrumbs — the scene, then
// the prefab — with ‹ back, the Context menu and Auto Save; the Hierarchy
// shows the prefab under a ‹. With Auto Save each change is saved as it is
// made; without it Save saves and leaving asks. From the Project a prefab
// opens in isolation.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart';
import 'package:fynkel_editor/src/scene_view/editor_state.dart';

import 'harness.dart';

const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const cratePrefab = 'c0000000-0000-4000-8000-000000000040';
const crateRoot = 'c0000000-0000-4000-8000-0000000000b1';

void main() {
  String rootName(Editor e) {
    final json = jsonDecode(File('${e.dir.path}/assets/prefabs/crate.prefab.json').readAsStringSync()) as Map;
    return (json['entities'] as Map)[crateRoot]['name'] as String;
  }

  Finder crumb(int i) => find.byKey(Key('scene.crumb.$i'));
  String crumbText(Editor e, int i) =>
      e.tester.widget<Text>(find.descendant(of: crumb(i), matching: find.byType(Text))).data!;

  testWidgets('› opens the prefab in context: breadcrumbs, ‹ back; Auto Save saves each change and its undo', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final scene = e.shell.tabs!.focused!;
    await tester.tap(find.byKey(const Key('hierarchy.openPrefab.$crateA')));
    await settleCooks(tester);
    final prefab = e.shell.tabs!.focused!;
    expect(prefab.document.id, cratePrefab);
    expect(e.shell.tabs!.contextOf(prefab)?.instance, crateA);
    expect(e.shell.tabs!.contextOf(prefab)?.tab, same(scene));

    // The bar: Main › Crate, the Context menu, Auto Save ticked; the Hierarchy's ‹ Crate.
    expect(find.byKey(const Key('scene.prefabBar')), findsOneWidget);
    expect(crumbText(e, 0), 'Main');
    expect(crumbText(e, 1), 'Crate');
    expect(find.byKey(const Key('scene.prefab.context')), findsOneWidget);
    expect(find.byKey(const Key('scene.prefab.save')), findsNothing, reason: 'Auto Save needs no Save');
    expect(tester.widget<Checkbox>(find.byType(Checkbox).last).value, isTrue);
    expect(find.byKey(const Key('hierarchy.prefabHeader')), findsOneWidget);

    // A change is on disk at once; so is its undo.
    expect(e.shell.hierarchy!.rename(crateRoot, 'Big Crate'), isTrue);
    await settleCooks(tester);
    expect(prefab.dirty, isFalse);
    expect(rootName(e), 'Big Crate');
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(prefab.dirty, isFalse);
    expect(rootName(e), 'Crate');

    // ‹ back to the scene: the prefab's tab closes, the scene is focused.
    await tester.tap(find.byKey(const Key('scene.prefab.back')));
    await settleCooks(tester);
    expect(e.shell.tabs!.focused, same(scene));
    expect(e.shell.tabs!.open, isNot(contains(prefab)));
    expect(find.byKey(const Key('scene.prefabBar')), findsNothing);
    expect(find.byKey(const Key('hierarchy.prefabHeader')), findsNothing);
  });

  testWidgets('without Auto Save: Save, and leaving with changes asks (Cancel stays, Discard leaves)', (tester) async {
    final e = await Editor.open(tester);
    final scene = e.shell.tabs!.focused!;
    await e.click(e.row('Crate A'));
    await tester.tap(find.byKey(const Key('prefab.open')));
    await settleCooks(tester);
    final prefab = e.shell.tabs!.focused!;
    expect(e.shell.tabs!.contextOf(prefab)?.instance, crateA, reason: "the Inspector's Open is in context too");

    await tester.tap(find.byKey(const Key('scene.prefab.autoSave')));
    await settleCooks(tester);
    expect(e.shell.tabs!.autoSavePrefabs, isFalse);
    expect(jsonDecode(File('${e.dir.path}/.fynkel/editor.json').readAsStringSync())['prefabAutoSave'], isFalse);
    expect(tester.widget<TextButton>(find.byKey(const Key('scene.prefab.save'))).onPressed, isNull);

    e.shell.hierarchy!.rename(crateRoot, 'Big Crate');
    await settleCooks(tester);
    expect(prefab.dirty, isTrue);
    expect(rootName(e), 'Crate');
    await tester.tap(find.byKey(const Key('scene.prefab.save')));
    await settleCooks(tester);
    expect(prefab.dirty, isFalse);
    expect(rootName(e), 'Big Crate');

    e.shell.hierarchy!.rename(crateRoot, 'Huge Crate');
    await settleCooks(tester);
    await tester.tap(find.byKey(const Key('scene.crumb.0')));
    await settleCooks(tester);
    await tester.tap(find.byKey(const Key('closePrompt.cancel')));
    await settleCooks(tester);
    expect(e.shell.tabs!.focused, same(prefab), reason: 'Cancel stays in Prefab mode');
    await tester.tap(find.byKey(const Key('hierarchy.prefab.back')));
    await settleCooks(tester);
    await tester.tap(find.byKey(const Key('closePrompt.discard')));
    await settleCooks(tester);
    expect(e.shell.tabs!.focused, same(scene));
    expect(rootName(e), 'Big Crate', reason: 'the discarded change never reached the file');

    // Turned back on, a prefab with unsaved changes is saved at once.
    await e.click(e.row('Crate A'));
    await tester.tap(find.byKey(const Key('hierarchy.openPrefab.$crateA')));
    await settleCooks(tester);
    e.shell.hierarchy!.rename(crateRoot, 'Crate');
    await settleCooks(tester);
    expect(e.shell.tabs!.focused!.dirty, isTrue);
    await tester.tap(find.byKey(const Key('scene.prefab.autoSave')));
    await settleCooks(tester);
    expect(e.shell.tabs!.focused!.dirty, isFalse);
    expect(rootName(e), 'Crate');
  });

  testWidgets('Context: Normal / Gray / Hidden, remembered; from the Project, isolation', (tester) async {
    final e = await Editor.open(tester);
    await tester.tap(find.byKey(const Key('hierarchy.openPrefab.$crateA')));
    await settleCooks(tester);
    final sv = e.shell.scene!;
    expect(sv.editorState.prefabContext, PrefabContextMode.gray, reason: "Unity's default");
    expect(sv.drawnContext?.instance, crateA);

    await tester.tap(find.byKey(const Key('scene.prefab.context')));
    await settleCooks(tester);
    await tester.tap(find.byKey(const Key('scene.prefab.context.hidden')).last);
    await settleCooks(tester);
    expect(sv.editorState.prefabContext, PrefabContextMode.hidden);
    expect(sv.drawnContext, isNull, reason: 'the prefab alone');
    expect(jsonDecode(File('${e.dir.path}/.fynkel/editor.json').readAsStringSync())['prefabContext'], 'hidden');
    sv.setPrefabContext(PrefabContextMode.normal);
    expect(sv.drawnContext?.instance, crateA);

    // The scene is drawn around the prefab: the instance's place is the prefab root's, greyed in Gray.
    final prefabTab = sv.current!;
    final scene = e.shell.tabs!.contextOf(prefabTab.tab)!.tab;
    final scenePreview = sv.previewOf(scene)!;
    var o = sv.overlay(prefabTab);
    expect(o.extraWorlds, hasLength(1));
    expect(o.extraWorlds.single.worldAddress, scenePreview.worldAddress);
    expect(o.extraWorlds.single.gray, isFalse, reason: 'Normal');
    final instance = scenePreview.worldMatrices([crateA])[crateA]!;
    final root = prefabTab.preview!.worldMatrices([crateRoot])[crateRoot]!;
    final placed = matMul(o.extraWorlds.single.transform, instance);
    for (var i = 0; i < 16; i++) {
      expect(placed[i], closeTo(root[i], 1e-4));
    }
    expect(scenePreview.entities([crateA]), isEmpty, reason: 'the instance itself is not drawn twice');
    sv.setPrefabContext(PrefabContextMode.gray);
    o = sv.overlay(prefabTab);
    expect(o.extraWorlds.single.gray, isTrue);
    sv.setPrefabContext(PrefabContextMode.hidden);
    expect(sv.overlay(prefabTab).extraWorlds, isEmpty);
    sv.setPrefabContext(PrefabContextMode.normal);

    // Double-clicked in the Project: in isolation — no scene crumb, no Context.
    await tester.tap(find.byKey(const Key('scene.prefab.back')));
    await settleCooks(tester);
    e.shell.browser!.setFolder('prefabs');
    await settleCooks(tester);
    await tester.tap(find.byKey(const Key('project.asset.$cratePrefab')));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.byKey(const Key('project.asset.$cratePrefab')));
    await settleCooks(tester);
    final prefab = e.shell.tabs!.focused!;
    expect(prefab.document.id, cratePrefab);
    expect(e.shell.tabs!.contextOf(prefab), isNull);
    expect(crumbText(e, 0), 'Crate');
    expect(crumb(1), findsNothing);
    expect(find.byKey(const Key('scene.prefab.context')), findsNothing);
    expect(sv.drawnContext, isNull);
    await tester.tap(find.byKey(const Key('scene.prefab.back')));
    await settleCooks(tester);
    expect(e.shell.tabs!.open, isNot(contains(prefab)), reason: '‹ closes it');
  });
}
