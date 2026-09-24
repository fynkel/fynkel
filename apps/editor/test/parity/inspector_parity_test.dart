// Unity parity: the Inspector, driven through the whole shell over a real
// project (docs/unity-parity.md § Inspector).
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/inspector/inspector_math.dart';

import 'harness.dart';

void main() {
  /// Types [degrees] into the rotation's [axis] (0 X, 1 Y, 2 Z) and presses Return.
  Future<void> setRotation(Editor e, int axis, String degrees) async {
    final field = find.descendant(of: find.byKey(ValueKey('euler.$axis')).first, matching: find.byType(EditableText));
    await e.tester.tap(field);
    await e.tester.pumpAndSettle();
    await e.type(degrees);
    await e.submit();
  }

  Euler rotationOf(Editor e, String name) {
    final tab = e.shell.tabs!.focused!;
    final n = tab.tree.nodes.values.firstWhere((n) => n.name == name);
    return Euler.fromQuaternion(n.transform!.rotation).clean;
  }

  testWidgets('editing one rotation axis of two objects keeps each one\'s other axes', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Floor'));
    await setRotation(e, 0, '30');
    expect(rotationOf(e, 'Floor').x, closeTo(30, 1e-3));
    await e.click(e.row('Crate B'));
    await setRotation(e, 2, '10');
    await e.click(e.row('Floor'), command: true);
    expect(e.selectedNames, ['Crate B', 'Floor']);
    await setRotation(e, 1, '45');
    final floor = rotationOf(e, 'Floor');
    final crate = rotationOf(e, 'Crate B');
    expect([floor.x, floor.y, floor.z], [closeTo(30, 1e-3), closeTo(45, 1e-3), closeTo(0, 1e-3)]);
    expect([crate.x, crate.y, crate.z], [closeTo(0, 1e-3), closeTo(45, 1e-3), closeTo(10, 1e-3)]);
    // One undo step for both.
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(rotationOf(e, 'Floor').y, closeTo(0, 1e-3));
    expect(rotationOf(e, 'Crate B').y, closeTo(0, 1e-3));
  });

  testWidgets('dragging a position label moves the object in the Scene view before the release', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Crate B'));
    final tab = e.shell.tabs!.focused!;
    final id = tab.selection.single;
    final scene = e.shell.scene!;
    final world = scene.current?.preview;
    expect(world, isNotNull, reason: 'the Scene view has the scene open');
    final x0 = world!.worldMatrices([id])[id]![12];
    // The X letter of Position is the drag handle.
    final handle = find.descendant(
      of: find.byKey(const ValueKey('vec.0')).first,
      matching: find.byKey(const ValueKey('number.axis')),
    );
    final g = await tester.startGesture(tester.getCenter(handle));
    await tester.pump(const Duration(milliseconds: 50));
    for (var k = 0; k < 10; k++) {
      await g.moveBy(const Offset(10, 0));
      await tester.pump(const Duration(milliseconds: 16));
    }
    final during = world.worldMatrices([id])[id]![12];
    expect(during, greaterThan(x0 + 1e-6), reason: 'the world follows the drag');
    expect(tab.document.history, 0, reason: 'nothing written yet');
    await g.up();
    await tester.pumpAndSettle();
    expect(tab.document.history, 1, reason: 'one undo step on release');
    expect(world.worldMatrices([id])[id]![12], closeTo(during, 1e-6));
  });

  testWidgets('a typed rotation reads the same after a reselect and after undo/redo', (tester) async {
    final e = await Editor.open(tester);
    // Y beyond 90°: the quaternion alone would read back as X 180, Y 60, Z 180.
    await e.click(e.row('Floor'));
    await setRotation(e, 1, '120');
    String shown(int axis) => tester
        .widget<EditableText>(
          find.descendant(of: find.byKey(ValueKey('euler.$axis')).first, matching: find.byType(EditableText)),
        )
        .controller
        .text;
    List<String> angles() => [shown(0), shown(1), shown(2)];
    expect(angles(), ['0', '120', '0']);
    await e.click(e.row('Crate B'));
    await e.click(e.row('Floor'));
    expect(angles(), ['0', '120', '0'], reason: 'reselected');
    await setRotation(e, 0, '10');
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(angles(), ['0', '120', '0'], reason: 'undone');
    await e.press(LogicalKeyboardKey.keyZ, command: true, shift: true);
    expect(angles(), ['10', '120', '0'], reason: 'redone');
  });

  testWidgets('Copy Component on one object, Paste Component Values on another; one undo', (tester) async {
    final e = await Editor.open(tester);
    final tab = e.shell.tabs!.focused!;
    List<double> positionOf(String name) => [
      for (final v in tab.tree.nodes.values.firstWhere((n) => n.name == name).transform!.position) v.toDouble(),
    ];
    await e.click(e.row('Floor'));
    final menu = find.byWidgetPredicate(
      (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('card.menu.fynkel.Transform'),
    );
    await e.click(menu.first);
    await e.click(find.byKey(const Key('card.menu.copyValues')));
    await e.click(e.row('Crate B'));
    expect(positionOf('Crate B'), isNot(positionOf('Floor')));
    await e.click(menu.first);
    await e.click(find.byKey(const Key('card.menu.pasteValues')));
    expect(positionOf('Crate B'), positionOf('Floor'));
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(positionOf('Crate B'), isNot(positionOf('Floor')));
  });

  testWidgets('a component\'s enabled is the checkbox in its header', (tester) async {
    final e = await Editor.open(tester, scene: 'c0000000-0000-4000-8000-000000000032');
    await e.click(e.row('Fountain'));
    final box = find.byWidgetPredicate(
      (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('card.enabled.fynkel.Emitter'),
    );
    expect(box, findsOneWidget);
    final before = tester.widget<Checkbox>(box).value;
    await e.click(box);
    expect(tester.widget<Checkbox>(box).value, before == false);
    final tab = e.shell.tabs!.focused!;
    expect(tab.document.history, 1, reason: 'one undo step');
  });

  testWidgets('clicking an asset field pings the asset in the Project; the selection stays', (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Robot'));
    const rusted = 'c0000000-0000-4000-8000-000000000020';
    final material = find.byWidgetPredicate(
      (w) =>
          w is Text &&
          w.key == const ValueKey('asset.name') &&
          (w.textSpan?.toPlainText() ?? '').toLowerCase().contains('rusted'),
    );
    expect(material, findsOneWidget);
    await e.click(material);
    final browser = e.shell.browser!;
    expect(browser.pinged, rusted);
    expect(browser.selection, isEmpty, reason: 'a ping selects nothing');
    expect(find.byKey(const Key('project.asset.$rusted')), findsOneWidget, reason: 'its folder is shown');
    expect(e.selectedNames, ['Robot'], reason: 'the Inspector keeps the object');
    expect(find.byKey(const ValueKey('assetPicker.search')), findsNothing, reason: 'no picker');
  });

  testWidgets('Move Up / Move Down reorder the cards and the saved file; the Transform stays first; one undo', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Crate B'));
    // The cards top to bottom, as their menus are laid out.
    List<String> cards() {
      final menus = find.byWidgetPredicate(
        (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('card.menu.fynkel.'),
      );
      final found = menus.evaluate().map((el) => (el.widget.key! as ValueKey<String>).value).toList()
        ..sort((a, b) => tester.getTopLeft(find.byKey(ValueKey(a))).dy.compareTo(tester.getTopLeft(find.byKey(ValueKey(b))).dy));
      return [for (final k in found) k.substring('card.menu.'.length).split('#').first];
    }

    List<String> saved() {
      final crate = (e.savedScene()['entities'] as Map<String, dynamic>).values.cast<Map<String, dynamic>>().singleWhere(
        (x) => x['name'] == 'Crate B',
      );
      return [for (final c in (crate['components'] as Map<String, dynamic>).values) (c as Map)['type'] as String];
    }

    Future<void> menuItem(String card, String item) async {
      await e.click(find.byKey(ValueKey('card.menu.$card#0')));
      await e.click(find.byKey(Key('card.menu.$item')));
    }

    expect(cards(), ['fynkel.Transform', 'fynkel.RigidBody', 'fynkel.Collider']);
    await menuItem('fynkel.Collider', 'moveUp');
    expect(cards(), ['fynkel.Transform', 'fynkel.Collider', 'fynkel.RigidBody']);

    // The Collider is right under the Transform now: it cannot go above it,
    // and the Transform itself does not move.
    await e.click(find.byKey(const ValueKey('card.menu.fynkel.Collider#0')));
    expect(tester.widget<PopupMenuItem<String>>(find.byKey(const Key('card.menu.moveUp'))).enabled, isFalse);
    await e.press(LogicalKeyboardKey.escape);
    await e.click(find.byKey(const ValueKey('card.menu.fynkel.Transform#0')));
    expect(tester.widget<PopupMenuItem<String>>(find.byKey(const Key('card.menu.moveDown'))).enabled, isFalse);
    await e.press(LogicalKeyboardKey.escape);

    await e.press(LogicalKeyboardKey.keyS, command: true);
    expect(saved(), ['fynkel.Transform', 'fynkel.Collider', 'fynkel.RigidBody', 'fynkel.Layer']);

    await menuItem('fynkel.Collider', 'moveDown');
    expect(cards(), ['fynkel.Transform', 'fynkel.RigidBody', 'fynkel.Collider']);
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(cards(), ['fynkel.Transform', 'fynkel.Collider', 'fynkel.RigidBody']);
  });

  testWidgets('the colour picker lights the Scene view while picking; OK is one undo step, Cancel puts it back', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Sun'));
    final tab = e.shell.tabs!.focused!;
    final sun = tab.selection.single;
    final world = e.shell.scene!.current!.preview!;
    List<double> colour() {
      final entities = (world.dump()['entities'] as List).cast<Map<String, Object?>>();
      final components = (entities.singleWhere((x) => x['id'] == sun)['components'] as List)
          .cast<Map<String, Object?>>();
      final light = components.singleWhere((c) => c['type'] == 'fynkel.DirectionalLight');
      return [for (final v in (light['data'] as Map)['color'] as List) (v as num).toDouble()];
    }

    final before = colour();
    Future<void> pickDarkRed() async {
      await tester.ensureVisible(find.byKey(const ValueKey('color.swatch')));
      await e.click(find.byKey(const ValueKey('color.swatch')));
      final sv = find.byKey(const ValueKey('picker.sv'));
      final g = await tester.startGesture(tester.getTopLeft(sv) + const Offset(4, 4));
      await tester.pump();
      await g.moveTo(tester.getBottomRight(sv) - const Offset(4, 60));
      await tester.pump();
      await g.up();
      await tester.pump();
    }

    await pickDarkRed();
    final during = colour();
    expect(during, isNot(before), reason: 'the world follows the picker');
    expect(tab.document.history, 0, reason: 'nothing written while picking');

    // Cancel: the world back to the document, nothing written.
    await e.click(find.text('Cancel'));
    expect(colour(), before);
    expect(tab.document.history, 0);

    // OK: one transaction with what was shown.
    await pickDarkRed();
    await e.click(find.text('OK'));
    expect(tab.document.history, 1, reason: 'one undo step');
    final after = colour();
    for (var i = 0; i < 3; i++) {
      expect(after[i], closeTo(during[i], 1e-4));
    }
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(colour(), before);
  });
}
