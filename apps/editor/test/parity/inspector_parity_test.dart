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
}
