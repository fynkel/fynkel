// Unity parity: Lock View to Selected (docs/unity-parity.md § Tier 3). ⇧F,
// or F twice, frames the selection and the view then follows it as it
// moves. Orbiting keeps the lock; panning or selecting something else
// lets go. Frame All keeps its View menu item without a key, as in Unity.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';

import 'harness.dart';

const crateB = 'c0000000-0000-4000-8000-0000000000e5';

void main() {
  testWidgets('⇧F follows the selection as it moves; orbit keeps it, a pan or another selection lets go', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final sv = e.shell.scene!;
    final tab = e.shell.tabs!.focused!;
    await e.click(e.row('Crate B'));
    final picture = find.byWidgetPredicate((w) => w.key is GlobalKey && '${w.key}'.contains('scene.picture'));
    final middle = tester.getCenter(picture);
    Vec3 crate() => matTranslation(sv.current!.preview!.worldMatrices([crateB])[crateB]!);
    void near(Vec3 a, Vec3 b, String reason) {
      for (var i = 0; i < 3; i++) {
        expect(a[i], closeTo(b[i], 1e-3), reason: reason);
      }
    }

    await e.press(LogicalKeyboardKey.keyF, shift: true);
    expect(sv.isFollowing, isTrue);
    near(sv.camera!.pivot, crate(), 'framed');
    expect(find.textContaining('following the selection'), findsOneWidget);

    // Drag it 2 m along X with the move gizmo: the view comes along.
    await e.press(LogicalKeyboardKey.keyW);
    final grab = sv.grabPoint('move.x')!;
    expect(sv.beginDrag('move.x', grab), isTrue);
    final axis = sv.viewProjection!.project(vAdd(crate(), [2, 0, 0]))!;
    sv.updateDrag(grab + (Offset(axis[0], axis[1]) - sv.gizmoPivotPx!));
    await settleCooks(tester);
    near(sv.camera!.pivot, crate(), 'follows while dragging');
    sv.endDrag();
    await settleCooks(tester);
    expect(crate()[0], greaterThan(-1), reason: 'moved from -2 towards +X');
    near(sv.camera!.pivot, crate(), 'and after');

    // ⌥-drag orbits around it, still locked.
    final distance = sv.camera!.distance;
    await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
    var g = await tester.startGesture(middle, kind: PointerDeviceKind.mouse, buttons: kPrimaryMouseButton);
    await g.moveBy(const Offset(40, 0));
    await g.moveBy(const Offset(40, 10));
    await g.up();
    await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
    await settleCooks(tester);
    expect(sv.isFollowing, isTrue, reason: 'an orbit keeps the lock');
    near(sv.camera!.pivot, crate(), 'orbiting the crate');
    expect(sv.camera!.distance, closeTo(distance, 1e-6));

    // A middle-drag pans: the view is free again.
    g = await tester.startGesture(middle, kind: PointerDeviceKind.mouse, buttons: kMiddleMouseButton);
    await g.moveBy(const Offset(30, 0));
    await g.moveBy(const Offset(30, 0));
    await g.up();
    await settleCooks(tester);
    expect(sv.isFollowing, isFalse, reason: 'a pan lets go');
    expect(find.textContaining('following the selection'), findsNothing);

    // F twice locks too; selecting something else lets go.
    await e.press(LogicalKeyboardKey.keyF);
    expect(sv.isFollowing, isFalse, reason: 'one F only frames');
    await e.press(LogicalKeyboardKey.keyF);
    expect(sv.isFollowing, isTrue, reason: 'F twice locks');
    await e.click(e.row('Floor'));
    await settleCooks(tester);
    expect(sv.isFollowing, isFalse);
    expect(tab.selection, ['c0000000-0000-4000-8000-0000000000e4']);

    // Frame All is still in the View menu, with no key.
    expect(e.shell.registry.byId('view.frameAll')!.shortcut, isNull);
    expect(e.shell.registry.byId('view.lockToSelected')!.label, 'Lock View to Selected');
  });
}
