// Unity parity: Pivot / Center (docs/unity-parity.md § Tier 3). Z toggles
// where the gizmo sits. Pivot: at the active object's origin, and with
// several objects selected each one rotates and scales about its own
// origin. Center: at the middle of the selection's bounds, and the group
// turns and scales about it.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/scene_view/editor_state.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';

import 'harness.dart';

const floor = 'c0000000-0000-4000-8000-0000000000e4';
const crateB = 'c0000000-0000-4000-8000-0000000000e5';

void main() {
  testWidgets(
    'Z moves the gizmo from the pivot to the centre; Pivot turns each object in place, Center turns the group',
    (tester) async {
      final e = await Editor.open(tester);
      final sv = e.shell.scene!;
      final tab = e.shell.tabs!.focused!;
      await e.click(e.row('Crate B'));
      await e.click(e.row('Floor'), command: true); // the Floor leads
      sv.frameSelected();
      await settleCooks(tester);

      Vec3 at(String id) => matTranslation(sv.current!.preview!.worldMatrices([id])[id]!);
      List<double> rotationOf(String id) => tab.tree[id]!.transform!.rotation;
      List<double> scaleOf(String id) => tab.tree[id]!.transform!.scale;
      final start = {floor: at(floor), crateB: at(crateB)};
      final middle = vScale(vAdd(start[floor]!, start[crateB]!), 0.5);
      void near(Vec3 a, Vec3 b, {String? reason}) {
        for (var i = 0; i < 3; i++) {
          expect(a[i], closeTo(b[i], 1e-3), reason: reason);
        }
      }

      // Drag a handle a quarter turn around the pivot's pixel (or, for the
      // scale cube, to the right).
      void turn(String handle) {
        final pivotPx = sv.gizmoPivotPx!;
        final grab = sv.grabPoint(handle)!;
        expect(sv.beginDrag(handle, grab), isTrue);
        final rel = grab - pivotPx;
        sv.updateDrag(handle == 'scale.uniform' ? grab + const Offset(60, 0) : pivotPx + Offset(-rel.dy, rel.dx));
        expect(sv.endDrag()!.ok, isTrue);
      }

      // Pivot (the default): the gizmo on the Floor.
      expect(sv.pivotMode, PivotMode.pivot);
      near(sv.gizmoFrame()!.pivot, start[floor]!);
      await e.press(LogicalKeyboardKey.keyE);
      final rotations = {floor: rotationOf(floor), crateB: rotationOf(crateB)};
      turn('rotate.y');
      near(at(floor), start[floor]!, reason: 'in place');
      near(at(crateB), start[crateB]!, reason: 'in place, not around the Floor');
      expect(rotationOf(crateB), isNot(rotations[crateB]));
      expect(tab.document.history, 1, reason: 'one undo step');
      await e.press(LogicalKeyboardKey.keyZ, command: true);

      await e.press(LogicalKeyboardKey.keyR);
      turn('scale.uniform');
      final factor = scaleOf(floor)[0];
      expect(factor, greaterThan(1));
      near(at(crateB), start[crateB]!, reason: 'each scales about its own origin');
      expect(scaleOf(crateB), [for (final _ in scaleOf(crateB)) closeTo(factor, 1e-6)]);
      await e.press(LogicalKeyboardKey.keyZ, command: true);

      // Z: Center, on the toolbar too, remembered in editor.json.
      await e.press(LogicalKeyboardKey.keyZ);
      expect(sv.pivotMode, PivotMode.center);
      expect(
        tester
            .widget<Tooltip>(
              find.ancestor(of: find.byKey(const Key('scene.pivot')), matching: find.byType(Tooltip)).first,
            )
            .message,
        startsWith('Center'),
      );
      expect(sv.editorState.toJson()['pivot'], 'center');
      near(sv.gizmoFrame()!.pivot, middle);

      await e.press(LogicalKeyboardKey.keyE);
      turn('rotate.y');
      expect(vLength(vSub(at(crateB), start[crateB]!)), greaterThan(0.1), reason: 'the group turns about the centre');
      for (final id in [floor, crateB]) {
        expect(vLength(vSub(at(id), middle)), closeTo(vLength(vSub(start[id]!, middle)), 1e-3));
      }
      await e.press(LogicalKeyboardKey.keyZ, command: true);

      await e.press(LogicalKeyboardKey.keyR);
      turn('scale.uniform');
      final f = scaleOf(floor)[0];
      for (final id in [floor, crateB]) {
        near(at(id), vAdd(middle, vScale(vSub(start[id]!, middle), f)), reason: 'away from the centre');
      }
      expect(scaleOf(crateB), [for (final _ in scaleOf(crateB)) closeTo(f, 1e-6)]);
      expect(f, greaterThan(1));
    },
  );
}
