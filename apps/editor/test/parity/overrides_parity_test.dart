// Unity parity: the Overrides dropdown and the blue margin bar
// (docs/unity-parity.md § Tier 3). A prefab instance's Inspector opens with
// the Prefab bar — Open, Select, Overrides ▾. The dropdown lists the
// overrides by object; a row opens the prefab's value beside the
// instance's, with Revert and Apply, and Revert All / Apply All close it.
// An overridden field is bold with a blue bar in the margin and Apply /
// Revert on a right-click; an added component carries a '+' and Apply /
// Revert Added Component in its menu.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const castShadows = 'field.fynkel.MeshRenderer#0.cast_shadows';
const receiveShadows = 'field.fynkel.MeshRenderer#0.receive_shadows';

void main() {
  testWidgets('the Prefab bar, the Overrides dropdown, the blue bar and the right-click Apply / Revert', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final tab = e.shell.tabs!.focused!;
    await e.click(e.row('Crate A'));
    List<Object?> overrides() => tab.expansionOf(crateA)!.overrides;
    // The prefab's root on disk, by component id. Apply writes into the
    // prefab's tab, opened behind the scene's and unsaved; it is saved here.
    Map<String, dynamic> prefabRoot() {
      final prefab = e.shell.tabs!.byId('c0000000-0000-4000-8000-000000000040')!;
      expect(e.shell.tabs!.focused, same(tab));
      expect(prefab.dirty, isTrue);
      expect(prefab.save(), isTrue);
      final json = jsonDecode(File('${e.dir.path}/assets/prefabs/crate.prefab.json').readAsStringSync()) as Map;
      return (json['entities'] as Map)[json['root']]['components'] as Map<String, dynamic>;
    }

    Finder bar(String row) =>
        find.descendant(of: find.byKey(ValueKey(row)), matching: find.byKey(const ValueKey('field.overrideBar')));

    // The bar: the prefab's name, Open, Select, Overrides (2).
    expect(tester.widget<Text>(find.byKey(const ValueKey('prefab.title'))).data, 'Prefab  Crate');
    expect(find.byKey(const ValueKey('prefab.open')), findsOneWidget);
    expect(find.byKey(const ValueKey('prefab.selectAsset')), findsOneWidget);
    expect(find.text('Overrides (2)'), findsOneWidget);

    // Cast Shadows is overridden: the blue bar; Receive Shadows is not.
    expect(bar(castShadows), findsOneWidget);
    expect(bar(receiveShadows), findsNothing);
    expect(find.byKey(const ValueKey('card.overrideBar.fynkel.MeshRenderer#0')), findsOneWidget);

    // The dropdown: by object, the comparison, Revert, one undo step.
    await tester.tap(find.byKey(const ValueKey('prefab.overrides')));
    await tester.pumpAndSettle();
    expect(find.text("Review, Revert or Apply Overrides on 'Crate A'"), findsOneWidget);
    expect(find.byKey(const ValueKey('prefab.override.0')), findsOneWidget);
    expect(find.byKey(const ValueKey('prefab.override.1')), findsOneWidget);
    expect(find.byKey(const ValueKey('prefab.revert.0')), findsNothing, reason: 'a row opens on a click');
    await tester.tap(find.byKey(const ValueKey('prefab.override.0')));
    await tester.pumpAndSettle();
    expect(tester.widget<SelectableText>(find.byKey(const ValueKey('prefab.compare.Prefab'))).data, 'true');
    expect(tester.widget<SelectableText>(find.byKey(const ValueKey('prefab.compare.Override'))).data, 'false');
    final before = tab.document.history;
    await tester.tap(find.byKey(const ValueKey('prefab.revert.0')));
    await tester.pumpAndSettle();
    expect(overrides(), hasLength(1));
    expect(tab.document.history, before + 1);
    expect(find.byKey(const ValueKey('prefab.override.1')), findsNothing, reason: 'the dropdown follows');
    await tester.tapAt(const Offset(2, 2));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('prefab.overridesPanel')), findsNothing);
    expect(bar(castShadows), findsNothing);
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(overrides(), hasLength(2));
    expect(bar(castShadows), findsOneWidget);

    // A right-click on the overridden field: Apply to Prefab 'Crate'.
    await tester.tap(
      find.descendant(of: find.byKey(const ValueKey(castShadows)), matching: find.text('Cast Shadows')),
      buttons: kSecondaryMouseButton,
    );
    await tester.pumpAndSettle();
    expect(find.text("Apply to Prefab 'Crate'"), findsOneWidget);
    expect(find.byKey(const ValueKey('field.revert')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('field.apply')));
    await tester.pumpAndSettle();
    expect(
      prefabRoot()['c0000000-0000-4000-8000-0000000000c3']['data']['cast_shadows'],
      false,
      reason: 'the prefab has the value',
    );
    expect(overrides(), hasLength(1), reason: 'and the instance no longer overrides it');
    expect(bar(castShadows), findsNothing);

    // An added component: '+', the bar, Revert Added Component; again, Apply.
    expect(e.shell.inspector!.addComponent('fynkel.PointLight'), isTrue);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('card.added.fynkel.PointLight#0')), findsOneWidget);
    expect(find.byKey(const ValueKey('card.overrideBar.fynkel.PointLight#0')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('card.menu.fynkel.PointLight#0')));
    await tester.pumpAndSettle();
    expect(find.text("Apply Added Component to Prefab 'Crate'"), findsOneWidget);
    await tester.tap(find.byKey(const Key('card.menu.revertAdded')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('card.fynkel.PointLight#0')), findsNothing);
    expect(overrides(), hasLength(1));

    expect(e.shell.inspector!.addComponent('fynkel.PointLight'), isTrue);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('card.menu.fynkel.PointLight#0')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('card.menu.applyAdded')));
    await tester.pumpAndSettle();
    expect(prefabRoot().values.map((c) => c['type']), contains('fynkel.PointLight'));
    expect(find.byKey(const ValueKey('card.added.fynkel.PointLight#0')), findsNothing, reason: "now the prefab's");
    expect(find.byKey(const ValueKey('card.fynkel.PointLight#0')), findsOneWidget);

    // Apply All: the last override (the position) goes into the prefab too.
    expect(find.text('Overrides (1)'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('prefab.overrides')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('prefab.applyAll')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('prefab.overridesPanel')), findsNothing, reason: 'Apply All closes it');
    expect(overrides(), isEmpty);
    expect(find.text('Overrides'), findsOneWidget);
  });
}
