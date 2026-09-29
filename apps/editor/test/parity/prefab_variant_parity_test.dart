// Unity parity: prefab variants, nested prefabs and Unpack Completely
// (docs/unity-parity.md § Tier 3). A prefab instance dragged to the
// Project asks: a new original prefab or a variant of its prefab. The
// variant takes the instance's overrides; the Inspector names it a
// Prefab Variant, and Apply offers the variant and its base. An object
// holding an instance makes a prefab that keeps it nested; Unpack opens
// one level (the nested instance stays one), Unpack Completely all.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const cratePrefab = 'c0000000-0000-4000-8000-000000000040';
const meshRenderer = 'c0000000-0000-4000-8000-0000000000c3';
const receiveShadows = 'field.fynkel.MeshRenderer#0.receive_shadows';

void main() {
  Map<String, dynamic> read(Editor e, String rel) =>
      jsonDecode(File('${e.dir.path}/assets/$rel').readAsStringSync()) as Map<String, dynamic>;
  Map<String, dynamic> rootOf(Map<String, dynamic> prefab) =>
      (prefab['entities'] as Map)[prefab['root']] as Map<String, dynamic>;

  Future<void> dragToProject(Editor e, String row) async {
    e.shell.browser!.setFolder('prefabs');
    await e.tester.pumpAndSettle();
    await e.drag(e.row(row), e.tester.getCenter(find.byKey(const Key('project.prefabDrop'))));
  }

  testWidgets('an instance dragged to the Project: Prefab Variant; Apply to the variant or to its base', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final tab = e.shell.tabs!.focused!;

    // Cancel leaves everything as it was.
    await dragToProject(e, 'Crate A');
    expect(find.byKey(const Key('dialog.createPrefab')), findsOneWidget);
    await tester.tap(find.byKey(const Key('createPrefab.cancel')));
    await settleCooks(tester);
    expect(File('${e.dir.path}/assets/prefabs/Crate A.prefab.json').existsSync(), isFalse);
    expect(tab.expansionOf(crateA)!.overrides, hasLength(2));

    await dragToProject(e, 'Crate A');
    await tester.tap(find.byKey(const Key('createPrefab.variant')));
    await settleCooks(tester);
    final variant = read(e, 'prefabs/Crate A.prefab.json');
    final root = rootOf(variant)['prefab'] as Map;
    expect((root['asset'] as Map)['asset'], cratePrefab, reason: "the variant's root is an instance of its base");
    expect(root['overrides'], hasLength(2), reason: "the instance's overrides are the variant's");
    final x = tab.expansionOf(crateA)!;
    expect(x.prefab?.isVariant, isTrue);
    expect(x.overrides, isEmpty, reason: 'the instance looks the same, with nothing of its own');

    await e.click(e.row('Crate A'));
    expect(tester.widget<Text>(find.byKey(const ValueKey('prefab.title'))).data, 'Prefab Variant  Crate A');

    // An override on the instance: Apply offers the variant and its base.
    await tester.tap(find.descendant(of: find.byKey(const ValueKey(receiveShadows)), matching: find.byType(Checkbox)));
    await settleCooks(tester);
    expect(tab.expansionOf(crateA)!.overrides, hasLength(1));
    await tester.tap(
      find.descendant(of: find.byKey(const ValueKey(receiveShadows)), matching: find.text('Receive Shadows')),
      buttons: kSecondaryMouseButton,
    );
    await settleCooks(tester);
    expect(find.text("Apply to Prefab Variant 'Crate A'"), findsOneWidget);
    expect(find.text("Apply to Prefab 'Crate'"), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('field.apply.1')));
    await settleCooks(tester);
    final base = rootOf(read(e, 'prefabs/crate.prefab.json'));
    expect((base['components'] as Map)[meshRenderer]['data']['receive_shadows'], isFalse, reason: 'into the base');
    expect(
      ((rootOf(read(e, 'prefabs/Crate A.prefab.json'))['prefab'] as Map)['overrides'] as List).where(
        (o) => (o as Map)['field'] == 'receive_shadows',
      ),
      isEmpty,
      reason: 'the variant keeps no override of it',
    );
    expect(tab.expansionOf(crateA)!.overrides, isEmpty);

    // Another: the Overrides dropdown's Apply All ▾ into the variant.
    await tester.tap(find.descendant(of: find.byKey(const ValueKey(receiveShadows)), matching: find.byType(Checkbox)));
    await settleCooks(tester);
    await tester.tap(find.byKey(const ValueKey('prefab.overrides')));
    await settleCooks(tester);
    await tester.tap(find.byKey(const ValueKey('prefab.applyAll')));
    await settleCooks(tester);
    await tester.tap(find.byKey(const ValueKey('prefab.applyAll.target.0')));
    await settleCooks(tester);
    expect(
      ((rootOf(read(e, 'prefabs/Crate A.prefab.json'))['prefab'] as Map)['overrides'] as List).where(
        (o) => (o as Map)['field'] == 'receive_shadows' && o['value'] == true,
      ),
      hasLength(1),
      reason: "the variant's override now",
    );
    expect(tab.expansionOf(crateA)!.overrides, isEmpty);
  });

  testWidgets('Original Prefab from an instance unpacks it one level; the Project makes a variant too', (tester) async {
    final e = await Editor.open(tester);
    final tab = e.shell.tabs!.focused!;
    await dragToProject(e, 'Crate A');
    await tester.tap(find.byKey(const Key('createPrefab.original')));
    await settleCooks(tester);
    final original = rootOf(read(e, 'prefabs/Crate A.prefab.json'));
    expect(original['prefab'], isNull, reason: 'an original prefab: the base unpacked into it');
    expect(tab.expansionOf(crateA)!.prefab?.isVariant, isFalse);
    expect(tab.tree[crateA]?.isPrefabInstance, isTrue);

    // Create ▸ Prefab Variant on a prefab in the Project.
    await tester.tap(find.byKey(const Key('project.asset.$cratePrefab')), buttons: kSecondaryButton);
    await settleCooks(tester);
    await tester.tap(find.byKey(const Key('project.menu.createVariant')));
    await settleCooks(tester);
    final made = read(e, 'prefabs/crate Variant.prefab.json');
    expect(((rootOf(made)['prefab'] as Map)['asset'] as Map)['asset'], cratePrefab);
  });

  testWidgets('a prefab from an object holding an instance keeps it nested; Unpack one level, Unpack Completely all', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final tab = e.shell.tabs!.focused!;
    expect(e.shell.hierarchy!.createEmpty(underSelection: false), isTrue);
    await settleCooks(tester);
    final holder = tab.selection.single;
    e.shell.hierarchy!.rename(holder, 'Holder');
    await settleCooks(tester);
    await e.drag(e.row('Crate A'), tester.getCenter(find.byKey(ValueKey('row.$holder'))));
    expect(tab.tree[crateA]?.parent, holder);

    await dragToProject(e, 'Holder');
    expect(find.byKey(const Key('dialog.createPrefab')), findsNothing, reason: 'not an instance: no question');
    final prefab = read(e, 'prefabs/Holder.prefab.json');
    final nested = (prefab['entities'] as Map).values.where((n) => (n as Map)['prefab'] != null).toList();
    expect(nested, hasLength(1), reason: 'Crate A stays an instance inside the prefab');
    expect((nested.single as Map)['prefab']['overrides'], hasLength(2), reason: 'its overrides kept');
    expect(tab.tree[holder]?.isPrefabInstance, isTrue);

    // The Hierarchy's context menu on the Holder.
    Future<void> run(String command) async {
      await tester.tap(find.byKey(ValueKey('row.$holder')), buttons: kSecondaryButton);
      await settleCooks(tester);
      await tester.tap(find.byKey(Key('hierarchy.menu.$command')));
      await settleCooks(tester);
    }

    // The scene's own instances, by name.
    List<String> instances() => [
      for (final n in (tab.document.root()['entities'] as Map).values)
        if ((n as Map)['prefab'] != null) n['name'] as String,
    ];

    await run('prefab.unpack');
    expect(tab.tree[holder]?.isPrefabInstance, isFalse);
    expect(instances(), contains('Crate A'), reason: 'one level: the nested instance stays one');
    tab.undo();
    await settleCooks(tester);
    expect(tab.tree[holder]?.isPrefabInstance, isTrue);

    await run('prefab.unpackCompletely');
    expect(tab.tree[holder]?.isPrefabInstance, isFalse);
    expect(instances(), isNot(contains('Crate A')), reason: 'completely: nothing is an instance');
    expect(tab.document.history, greaterThan(0));
  });
}
