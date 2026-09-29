// Unity parity: the Rect tool (T) and the rotate gizmo's trackball
// (docs/unity-parity.md § Tier 3). Rect: a rectangle around the selection
// in the plane of its two axes most across the view; a corner resizes
// about the opposite one (⌥ about the centre), an edge one way, the inside
// moves, just outside a corner turns; a click inside that never drags
// selects what is under it. Rotate: a drag inside the rings turns freely.
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart' show matInverseAffine, matMul;
import 'package:fynkel_editor/src/scene_view/scene_math.dart';
import 'package:fynkel_editor/src/scene_view/scene_tools.dart';

import 'harness.dart';

const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const crateB = 'c0000000-0000-4000-8000-0000000000e5';

void near(Vec3 a, Vec3 b, {String? reason, double tolerance = 1e-3}) {
  for (var i = 0; i < 3; i++) {
    expect(a[i], closeTo(b[i], tolerance), reason: reason);
  }
}

void main() {
  testWidgets('T: a corner resizes about the opposite one, ⌥ about the centre; inside moves; a corner zone turns', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final sv = e.shell.scene!;
    final tab = e.shell.tabs!.focused!;
    await e.click(e.row('Crate B'));
    sv.frameSelected();
    await settleCooks(tester);
    await e.press(LogicalKeyboardKey.keyT);
    expect(sv.tool, SceneTool.rect);
    expect(e.shell.registry.byId('scene.tool.rect')!.label, contains('Rect'));

    Vec3 grab(String id) => sv.handles().firstWhere((h) => h.id == id).grab;
    Offset px(Vec3 p) {
      final r = sv.viewProjection!.project(p)!;
      return Offset(r[0], r[1]);
    }

    void dragTo(String handle, Vec3 target) {
      expect(sv.beginDrag(handle, sv.grabPoint(handle)!), isTrue, reason: handle);
      sv.updateDrag(px(target));
      expect(sv.endDrag()!.ok, isTrue);
    }

    // A corner: the diagonal ×1.5, the opposite corner stays.
    var c00 = grab('rect.corner.00'), c11 = grab('rect.corner.11');
    expect(vDistance(c00, c11), greaterThan(0.5), reason: 'a rectangle around the crate');
    final history = tab.document.history;
    final target = vAdd(c00, vScale(vSub(c11, c00), 1.5));
    dragTo('rect.corner.11', target);
    await settleCooks(tester);
    expect(tab.document.history, history + 1, reason: 'one undo step');
    near(grab('rect.corner.00'), c00, reason: 'the opposite corner stays');
    near(grab('rect.corner.11'), target, reason: 'the dragged corner is where the pointer is');
    final scale = [...tab.tree[crateB]!.transform!.scale]..sort();
    expect(scale[0], closeTo(1, 1e-6), reason: 'the depth keeps its scale');
    expect(scale[1], closeTo(1.5, 1e-3));
    expect(scale[2], closeTo(1.5, 1e-3));

    // ⌥ on an edge: about the centre — the other side moves the other way.
    c00 = grab('rect.corner.00');
    final c10 = grab('rect.corner.10'), c01 = grab('rect.corner.01');
    final along = vScale(vSub(c10, c00), 0.25);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
    dragTo('rect.edge.a1', vAdd(grab('rect.edge.a1'), along));
    await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
    await settleCooks(tester);
    near(grab('rect.corner.00'), vSub(c00, along), reason: 'the far edge moved out as much');
    near(grab('rect.corner.01'), vSub(c01, along));

    // Inside: a move in the plane.
    final before = matTranslation(sv.current!.preview!.worldMatrices([crateB])[crateB]!);
    c00 = grab('rect.corner.00');
    final shift = vScale(vSub(grab('rect.corner.01'), c00), 0.5);
    dragTo('rect.move', vAdd(grab('rect.move'), shift));
    await settleCooks(tester);
    near(matTranslation(sv.current!.preview!.worldMatrices([crateB])[crateB]!), vAdd(before, shift));
    near(grab('rect.corner.00'), vAdd(c00, shift));

    // Just outside a corner: a quarter turn about the plane's normal, in place (Pivot).
    final rotation = List.of(tab.tree[crateB]!.transform!.rotation);
    final position = List.of(tab.tree[crateB]!.transform!.position);
    final pivotPx = sv.gizmoPivotPx!;
    final zone = sv.grabPoint('rect.rotate.11')!;
    expect(sv.handleAt(zone)?.id, 'rect.rotate.11');
    final rel = zone - pivotPx;
    expect(sv.beginDrag('rect.rotate.11', zone), isTrue);
    sv.updateDrag(pivotPx + Offset(-rel.dy, rel.dx));
    expect(sv.endDrag()!.ok, isTrue);
    await settleCooks(tester);
    expect(tab.tree[crateB]!.transform!.rotation, isNot(rotation));
    near(tab.tree[crateB]!.transform!.position, position, reason: 'turned about its own origin');

    // Both crates: a click inside their rectangle, on Crate A, selects it.
    await e.click(e.row('Crate A'), command: true);
    expect(tab.selection, unorderedEquals([crateA, crateB]));
    final crate = px(matTranslation(sv.current!.preview!.worldMatrices([crateA])[crateA]!));
    expect(sv.handleAt(crate)?.id, 'rect.move', reason: 'inside the rectangle');
    final picture = find.byWidgetPredicate((w) => w.key is GlobalKey && '${w.key}'.contains('scene.picture'));
    await tester.tapAt(tester.getTopLeft(picture) + crate / tester.view.devicePixelRatio);
    await settleCooks(tester);
    expect(tab.selection, [crateA]);
  });

  testWidgets('E: a drag inside the rings turns freely, right turns the front to the right', (tester) async {
    final e = await Editor.open(tester);
    final sv = e.shell.scene!;
    final tab = e.shell.tabs!.focused!;
    await e.click(e.row('Crate B'));
    sv.frameSelected();
    await settleCooks(tester);
    await e.press(LogicalKeyboardKey.keyE);
    final start = sv.current!.preview!.worldMatrices([crateB])[crateB]!;
    final frame = sv.gizmoFrame()!;
    final grab = sv.grabPoint('rotate.free')!;
    expect(sv.handleAt(grab)?.id, 'rotate.free', reason: 'the rings are not there');
    // The rings keep their priority over the sphere inside them.
    expect(sv.handleAt(sv.grabPoint('rotate.x')!)?.id, 'rotate.x');

    final axes = matAxes(sv.viewProjection!.world);
    final right = axes[0], up = axes[1];
    final toCamera = vNormalize(vSub(matTranslation(sv.viewProjection!.world), frame.pivot));
    expect(sv.beginDrag('rotate.free', grab), isTrue);
    sv.updateDrag(grab + Offset(90 * sv.pixelRatio, 0));
    expect(sv.drag!.readout, '90°');
    expect(sv.endDrag()!.ok, isTrue);
    await settleCooks(tester);
    expect(tab.document.history, 1);

    final now = sv.current!.preview!.worldMatrices([crateB])[crateB]!;
    final turn = matMul(now, matInverseAffine(start));
    final axis = vNormalize(vCross(toCamera, right));
    near(transformDirection(turn, axis), axis, reason: 'about the axis across the drag');
    final front = vNormalize(vSub(right, vScale(toCamera, vDot(toCamera, right))));
    near(transformDirection(turn, toCamera), front, reason: 'the front turned a quarter to the right');
    near(matTranslation(now), matTranslation(start), reason: 'in place');
    expect(vDot(up, axis).abs(), greaterThan(math.cos(math.pi / 4)), reason: 'about (nearly) the view\'s up');

    // The Transform tool keeps its move handles inside: no trackball there.
    await e.press(LogicalKeyboardKey.keyY);
    expect(sv.handles().where((h) => h.id == 'rotate.free'), isEmpty);
  });
}
