// The handle framework and the tool registry (item 9g): shapes, hit
// tests, drag constraints, the built-in tools and a registered one.
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/services.dart' show LogicalKeyboardKey;
import 'package:flutter/widgets.dart' show IconData;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/scene_view/editor_state.dart';
import 'package:fynkel_editor/src/scene_view/gizmo.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';
import 'package:fynkel_editor/src/scene_view/scene_tools.dart';
import 'package:fynkel_editor/src/theme/icons.dart';

ToolContext contextAt(EditorCamera cam, Vec3 pivot, {List<Vec3> axes = GizmoFrame.worldAxes}) {
  final vp = ViewProjection(cam, 1000, 800);
  return ToolContext(
    frame: GizmoFrame(pivot: pivot, axes: axes, size: vp.metresPerPixel(pivot) * GizmoFrame.screenSize),
    vp: vp,
  );
}

Offset px(ToolContext c, Vec3 p) {
  final r = c.vp.project(p)!;
  return Offset(r[0], r[1]);
}

Handle? hit(ToolContext c, List<Handle> hs, Offset p) =>
    hitTestHandles([for (final h in hs) ProjectedHandle(h, c.vp)], p);

DragInput input(ToolContext c, Offset from, Offset to) => DragInput(
  startPx: from,
  px: to,
  startRay: c.vp.rayAt(from.dx, from.dy),
  ray: c.vp.rayAt(to.dx, to.dy),
  pivotPx: px(c, c.frame.pivot),
  cameraForward: c.camera[2],
);

/// A tool a project or a test adds: one handle that moves along world X.
final class NudgeTool extends SceneTool {
  const NudgeTool();
  @override
  String get id => 'nudge';
  @override
  String get label => 'Nudge';
  @override
  IconData get icon => FIcons.add;
  @override
  List<Handle> handles(ToolContext c) => [
    Handle(
      id: 'nudge.x',
      shape: HandleShape(triangles: cube(vAdd(c.frame.pivot, [c.frame.size, 0, 0]), c.frame.axes, c.frame.size * 0.1)),
      color: const [1, 1, 1, 1],
      constraint: AxisConstraint(c.frame.pivot, const [1, 0, 0]),
      grab: vAdd(c.frame.pivot, [c.frame.size, 0, 0]),
    ),
  ];
  @override
  TransformEdit? edit(Handle handle, DragValue value, ToolContext c) => TransformEdit(
    delta: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, value.scalar, 0, 0, 1],
    pivot: vAdd(c.frame.pivot, [value.scalar, 0, 0]),
    readout: 'nudged',
    channel: TransformChannel.move,
  );
}

void main() {
  group('the move tool', () {
    test('three arrows, three planes and the view square, at a constant screen size', () {
      final cam = EditorCamera.initial();
      final c = contextAt(cam, const [0, 0.5, 0]);
      final hs = SceneTool.move.handles(c);
      expect(hs.map((h) => h.id).toSet(), {'move.view', 'move.xy', 'move.yz', 'move.xz', 'move.x', 'move.y', 'move.z'});
      final pivot = px(c, c.frame.pivot);
      for (final id in ['move.x', 'move.y', 'move.z']) {
        final grab = px(c, hs.singleWhere((h) => h.id == id).grab);
        expect((grab - pivot).distance, closeTo(GizmoFrame.screenSize * 0.88, GizmoFrame.screenSize * 0.3));
        expect(hit(c, hs, grab)?.id, id, reason: 'the arrow\'s grab point is on its cone');
      }
      expect(hit(c, hs, pivot)?.id, 'move.view', reason: 'the centre square wins over the arrows under it');
      expect(hit(c, hs, pivot + const Offset(400, 300)), isNull);
      for (final id in ['move.xy', 'move.yz', 'move.xz']) {
        expect(hit(c, hs, px(c, hs.singleWhere((h) => h.id == id).grab))?.id, id);
      }
      // Twice as far: the same pixels, twice the metres.
      cam.distance *= 2;
      final far = contextAt(cam, const [0, 0.5, 0]);
      expect(far.frame.size, closeTo(c.frame.size * 2, c.frame.size * 0.05));
    });

    test('the planes sit on the camera\'s side of the pivot', () {
      final cam = EditorCamera.initial();
      final c = contextAt(cam, const [0, 0, 0]);
      for (final h in SceneTool.move.handles(c).where((h) => h.id.length == 7)) {
        expect(vDot(vSub(h.grab, c.frame.pivot), c.toCamera), greaterThan(0), reason: h.id);
      }
    });

    test('a drag along X reads metres along X and snaps', () {
      final c = contextAt(EditorCamera.initial(), const [0, 0, 0]);
      final x = SceneTool.move.handles(c).singleWhere((h) => h.id == 'move.x');
      final from = px(c, x.grab), to = px(c, vAdd(x.grab, const [1.3, 0, 0]));
      final v = x.constraint.solve(input(c, from, to))!;
      expect(v.scalar, closeTo(1.3, 1e-6));
      final e = SceneTool.move.edit(x, v, c)!;
      expect(e.delta.sublist(12, 15), [closeTo(1.3, 1e-6), closeTo(0, 1e-9), closeTo(0, 1e-9)]);
      expect(e.readout, startsWith('1.3'));
      expect(e.channel, TransformChannel.move);
      final snapped = ToolContext(frame: c.frame, vp: c.vp, snap: const SnapSettings(move: 0.5), snapping: true);
      expect(SceneTool.move.edit(x, v, snapped)!.delta[12], closeTo(1.5, 1e-9));
    });
  });

  group('the rotate tool', () {
    test('rings: the front half hit, the back half faint and never hit', () {
      final c = contextAt(EditorCamera.initial(), const [0, 0.5, 0]);
      final hs = SceneTool.rotate.handles(c);
      expect(hs.map((h) => h.id), ['rotate.x', 'rotate.y', 'rotate.z', 'rotate.view', 'rotate.free']);
      for (final h in hs.take(3)) {
        expect(h.shape.back, isNotEmpty, reason: '${h.id}: the initial camera sees every ring at an angle');
        final grab = px(c, h.grab);
        expect((grab - px(c, c.frame.pivot)).distance, greaterThan(20), reason: 'never grabbed on the pivot');
        expect(hit(c, [h], grab)?.id, h.id);
        for (final l in h.shape.lines) {
          for (var i = 0; i + 1 < l.length; i++) {
            final mid = vScale(vAdd(l[i], l[i + 1]), 0.5);
            expect(vDot(vSub(mid, c.frame.pivot), c.toCamera), greaterThanOrEqualTo(0), reason: '${h.id} front');
          }
        }
      }
      // A ring the camera looks straight down is all front.
      final top = EditorCamera.initial()..lookFrom(1, positive: true);
      final ct = contextAt(top, const [0, 0, 0]);
      final y = SceneTool.rotate.handles(ct).singleWhere((h) => h.id == 'rotate.y');
      expect(y.shape.back, isEmpty);
    });

    test('a quarter sweep on screen is a quarter turn about the axis', () {
      final c = contextAt(EditorCamera.initial(), const [0, 0, 0]);
      final y = SceneTool.rotate.handles(c).singleWhere((h) => h.id == 'rotate.y');
      final pivot = px(c, c.frame.pivot), grab = px(c, y.grab);
      final rel = grab - pivot;
      final v = y.constraint.solve(input(c, grab, pivot + Offset(-rel.dy, rel.dx)))!;
      expect(v.scalar.abs(), closeTo(math.pi / 2, 1e-6));
      final e = SceneTool.rotate.edit(y, v, c)!;
      expect(e.readout, endsWith('°'));
      expect(e.channel, TransformChannel.rotate);
    });
  });

  group('the scale tool', () {
    test('cube-tipped axes and a uniform cube; always local', () {
      final c = contextAt(EditorCamera.initial(), const [0, 0, 0]);
      final hs = SceneTool.scale.handles(c);
      expect(hs.map((h) => h.id), ['scale.uniform', 'scale.x', 'scale.y', 'scale.z']);
      expect(hs.first.shape.triangles.length, 12, reason: 'a cube');
      expect(SceneTool.scale.alwaysLocal, isTrue);
      final u = hs.first;
      final pivot = px(c, c.frame.pivot);
      // Unity's cube: 100 points right (or up) doubles it, wherever it was grabbed.
      final v = u.constraint.solve(input(c, pivot + const Offset(3, 0), pivot + const Offset(103, 0)))!;
      expect(v.scalar, closeTo(2, 1e-9));
      final up = u.constraint.solve(input(c, pivot, pivot + const Offset(10, -50)))!;
      expect(up.scalar, closeTo(1.5, 1e-9));
      final e = SceneTool.scale.edit(u, v, c)!;
      expect(e.delta[0], closeTo(2, 1e-9));
      expect(e.delta[5], closeTo(2, 1e-9));
      expect(e.readout, '×2');
    });
  });

  group('constraints', () {
    test('a plane move stays in the plane; a radius never goes below zero', () {
      final c = contextAt(EditorCamera.initial(), const [0, 0, 0]);
      final plane = const PlaneConstraint([0, 0, 0], [0, 1, 0]);
      final from = px(c, const [0, 0, 0]), to = px(c, const [1, 0, 2]);
      final v = plane.solve(input(c, from, to))!;
      expect(v.vector[1], closeTo(0, 1e-9));
      expect(v.vector[0], closeTo(1, 1e-6));
      expect(v.vector[2], closeTo(2, 1e-6));
      final radius = const RadiusConstraint([0, 0, 0], [1, 0, 0], 1);
      final shrink = radius.solve(input(c, px(c, const [1, 0, 0]), px(c, const [-3, 0, 0])))!;
      expect(shrink.scalar, 0);
      final grow = radius.solve(input(c, px(c, const [1, 0, 0]), px(c, const [2.5, 0, 0])))!;
      expect(grow.scalar, closeTo(2.5, 1e-6));
    });

    test('shapes: a cone, a cube, a quad', () {
      expect(cone(const [0, 0, 0], const [0, 1, 0], 0.1).length, 24);
      expect(cube(const [0, 0, 0], GizmoFrame.worldAxes, 1).length, 12);
      expect(quad(const [0, 0, 0], const [1, 0, 0], const [1, 1, 0], const [0, 1, 0]).length, 2);
      final pts = circle(const [0, 0, 0], const [0, 1, 0], 2);
      expect(pts.first, pts.last.map((v) => closeTo(v, 1e-9)).toList());
      for (final p in pts) {
        expect(vLength(p), closeTo(2, 1e-9));
      }
    });
  });

  group('the registry', () {
    test('the built-in tools, their keys and ids', () {
      expect(SceneTools.all.map((t) => t.id), ['select', 'move', 'rotate', 'scale', 'rect', 'transform']);
      expect(SceneTools.byKey(LogicalKeyboardKey.keyT), SceneTool.rect);
      expect(SceneTools.byKey(LogicalKeyboardKey.keyW), SceneTool.move);
      expect(SceneTools.byKey(LogicalKeyboardKey.keyY), SceneTool.transform);
      expect(SceneTools.byId('rotate'), SceneTool.rotate);
      expect(SceneTools.byId('nope'), isNull);
      expect(SceneTool.select.hasGizmo, isFalse);
      expect(() => SceneTools.register(const MoveTool()), throwsArgumentError);
      expect(() => SceneTools.unregister('move'), throwsArgumentError);
    });

    test('the transform tool is the three composed, with no drag code of its own', () {
      final c = contextAt(EditorCamera.initial(), const [0, 0.5, 0]);
      final hs = SceneTool.transform.handles(c);
      final ids = hs.map((h) => h.id).toSet();
      expect(ids, containsAll(['scale.uniform', 'move.x', 'move.xz', 'rotate.x', 'rotate.y', 'rotate.z']));
      expect(ids, isNot(contains('move.view')));
      expect(ids, isNot(contains('rotate.view')));
      expect(ids, isNot(contains('scale.x')));
      expect(hs.first.id, 'scale.uniform', reason: 'the centre cube is hit first');
      for (final id in ['move.x', 'rotate.y', 'scale.uniform']) {
        final h = hs.singleWhere((h) => h.id == id);
        expect(hit(c, hs, px(c, h.grab))?.id, id);
      }
      // Rings outside the arrows.
      final ring = hs.singleWhere((h) => h.id == 'rotate.x');
      expect(vDistance(ring.grab, c.frame.pivot), closeTo(c.frame.size * 1.25, 1e-9));
      final x = hs.singleWhere((h) => h.id == 'move.x');
      final e = SceneTool.transform.edit(x, const DragValue(2, [2, 0, 0]), c)!;
      expect(e.channel, TransformChannel.move);
    });

    test('a registered tool joins the toolbar order, the keys and editor.json', () {
      SceneTools.register(const NudgeTool());
      addTearDown(() => SceneTools.unregister('nudge'));
      expect(SceneTools.all.last.id, 'nudge');
      expect(() => SceneTools.register(const NudgeTool()), throwsArgumentError);
      final dir = Directory.systemTemp.createTempSync('nudge');
      addTearDown(() => dir.deleteSync(recursive: true));
      EditorState.load(dir.path)
        ..tool = SceneTools.byId('nudge')!
        ..changed();
      expect(EditorState.load(dir.path).tool.id, 'nudge');
    });
  });
}
