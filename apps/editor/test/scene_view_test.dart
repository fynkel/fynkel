// The Scene view over the fake preview backend (blueprint item 6): the
// preview per tab, picking (click, cycling, marquee), the gizmo drag as
// exactly one transaction with the world back to the document, the
// editor-only hidden/locked flags, the camera commands, the hotkeys and
// the prefab drop. The real world is covered by scene_session_test.dart.

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_model.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart';
import 'package:fynkel_editor/src/inspector/field_widgets.dart' show AssetDragPayload;
import 'package:fynkel_editor/src/scene_view/editor_state.dart';
import 'package:fynkel_editor/src/scene_view/gizmo.dart';
import 'package:fynkel_editor/src/scene_view/orientation_gizmo.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';
import 'package:fynkel_editor/src/scene_view/scene_panel.dart';
import 'package:fynkel_editor/src/scene_view/scene_view_model.dart';
import 'package:fynkel_editor/src/theme/tokens.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';

import 'support/fakes.dart';

/// A scene laid out for picking: unit cubes (the fake's meshes) at known
/// places, a camera entity, a stack of two down the view axis (Floor at
/// the origin, Behind 6 m straight behind it; the rig looks down −Z).
FakeDocument scene({String kind = 'scene', String id = 's', String? readOnly}) => FakeDocument(
  readOnly: readOnly,
  path: '/fake/game/assets/${kind}s/$id.$kind.json',
  id: id,
  kind: kind,
  name: 'S',
  relativePath: '${kind}s/$id.$kind.json',
  entities: {
    'cam': FakeDocument.entity(
      'Camera',
      position: [0, 3, 12],
      components: {
        'c': {
          'type': 'fynkel.Camera',
          'version': 1,
          'data': {
            'projection': 'perspective',
            'fov_y': 0.9,
            'near': 0.2,
            'far': 80,
            'exposure_mode': 'manual',
            'exposure': 11,
          },
        },
      },
    ),
    'floor': FakeDocument.entity('Floor', position: [0, 0, 0]),
    'crate': FakeDocument.entity(
      'Crate',
      position: [4, 0, 0],
      components: {
        'm': {'type': 'fynkel.MeshRenderer', 'version': 1, 'data': {}},
      },
    ),
    'behind': FakeDocument.entity('Behind', position: [0, 0, -6]),
    'robot': FakeDocument.entity(
      'Robot',
      children: ['arm'],
      position: [-4, 0, 0],
      rotation: [0, 0.7071067811865476, 0, 0.7071067811865476],
    ),
    'arm': FakeDocument.entity('Arm', parent: 'robot', position: [0, 1.5, 0]),
    'far': FakeDocument.entity('Far', position: [20, 0, -40]),
  },
);

final class Rig {
  Rig({FakeDocument? doc, EditorState? state}) {
    project = FakeProject(dir: '/fake/game');
    document = project.addDocument(doc ?? scene());
    tabs = DocumentTabs(project: project);
    hierarchy = HierarchyModel(tabs: tabs, project: project);
    editorState = state ?? EditorState.inMemory();
    model = SceneViewModel(
      tabs: tabs,
      project: project,
      hierarchy: hierarchy,
      editorState: editorState,
      chooseSavePath: (name) async => savePath,
    );
  }

  late final FakeProject project;
  late final FakeDocument document;
  late final DocumentTabs tabs;
  late final HierarchyModel hierarchy;
  late final EditorState editorState;
  late final SceneViewModel model;
  String? savePath;

  /// Opens the document, looks straight down −Z from z = 20 at the origin
  /// (an orthographic-friendly, predictable view) and sizes the picture.
  SceneTab open({double width = 1000, double height = 800}) {
    final tab = tabs.openDocument(document.id)!;
    model.setViewportSize(width, height);
    final st = model.current!;
    st.camera.alignTo(Trs(position: const [0, 0, 20], rotation: const [0, 0, 0, 1], scale: const [1, 1, 1]).toMatrix());
    expect(tab, st.tab);
    return st;
  }

  FakePreviewBackend get preview => document.fakePreview!;
  ViewProjection get vp => model.viewProjection!;

  /// The pixel of an entity's world origin.
  Offset pixelOf(String id) {
    final w = preview.worldOf(id)!;
    final px = vp.project(matTranslation(w))!;
    return Offset(px[0], px[1]);
  }

  int applies() => document.calls.where((c) => c == 'apply').length;

  void dispose() {
    model.dispose();
    hierarchy.dispose();
    tabs.dispose();
    editorState.dispose();
  }
}

void main() {
  group('previews per tab', () {
    test('a scene tab opens its preview once; a material tab has none; a closed tab closes it', () {
      final r = Rig();
      addTearDown(r.dispose);
      final material = r.project.addDocument(scene(kind: 'material', id: 'mat'));
      final st = r.open();
      expect(st.hasPreview, isTrue);
      expect(r.document.previewsOpened, 1);
      expect(r.model.hasView, isTrue);
      expect(r.model.current, same(st), reason: 'the same SceneTab while the tab is open');
      expect(r.document.previewsOpened, 1);

      final mt = r.tabs.openDocument(material.id)!;
      final ms = r.model.current!;
      expect(ms.tab, mt);
      expect(ms.hasPreview, isFalse);
      expect(ms.openError, contains('FYN-T0003'));
      expect(r.model.hasView, isFalse);

      r.tabs.focus(st.tab);
      expect(r.model.current, same(st));
      r.tabs.closeDiscarding(st.tab);
      expect(r.preview.isClosed, isTrue);
      expect(r.document.isClosed, isTrue);
    });

    test('a sync that fell back to a reload is announced; hidden flags are re-applied after it', () async {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      st.tab.select('crate');
      r.model.toggleHiddenSelection();
      expect(st.hiddenDirty, isFalse, reason: 'a toggle reaches the world at once');
      expect(r.model.renderFrame(), isNull, reason: 'no viewport attached in a test');
      // A whole-document change (the fake reports `reloaded`).
      r.document.reload();
      await Future<void>.delayed(Duration.zero);
      expect(r.model.notice, contains('reloaded'));
      expect(st.fallbackReloads, 1);
      expect(st.hiddenDirty, isTrue);
    });
  });

  group('picking', () {
    test('a click selects the nearest hit, repeats cycle the stack, ⌘ toggles, empty clears', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      final px = r.pixelOf('floor');
      final hits = r.model.hitsAt(px);
      expect(hits, ['floor', 'behind'], reason: 'nearest first; Behind stands 6 m behind the floor');

      r.model.click(px);
      expect(st.tab.selection.toList(), ['floor']);
      r.model.click(px);
      expect(st.tab.selection.toList(), ['behind'], reason: 'a repeated click on the same pixel cycles');
      r.model.click(px);
      expect(st.tab.selection.toList(), ['floor']);

      r.model.click(r.pixelOf('crate'), toggle: true);
      expect(st.tab.selection.toSet(), {'floor', 'crate'});
      r.model.click(r.pixelOf('crate'), toggle: true);
      expect(st.tab.selection.toList(), ['floor']);

      r.model.click(const Offset(5, 5));
      expect(st.tab.selection, isEmpty);
      r.model.click(r.pixelOf('crate'), add: true);
      r.model.click(const Offset(5, 5), add: true);
      expect(st.tab.selection.toList(), ['crate'], reason: 'a modifier click on nothing keeps the selection');
    });

    test('a click inside a rotated child picks the child (hierarchy world matrices are honoured)', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      // Arm sits 1.5 m above the robot's origin whatever the robot's yaw.
      final armWorld = r.preview.worldOf('arm')!;
      expect(matTranslation(armWorld)[0], closeTo(-4, 1e-9));
      expect(matTranslation(armWorld)[1], closeTo(1.5, 1e-9));
      r.model.click(r.pixelOf('arm'));
      expect(st.tab.selection.toList(), ['arm']);
      expect(st.tab.tree.expanded, contains('robot'), reason: 'the Hierarchy reveals the pick');
    });

    test('a marquee selects what its rectangle overlaps; ⇧ adds, ⌘ toggles', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      final a = r.pixelOf('floor') - const Offset(30, 30);
      final b = r.pixelOf('crate') + const Offset(30, 30);
      r.model.marqueeSelect(b, a);
      expect(st.tab.selection.toSet(), {
        'crate',
        'behind',
        'floor',
      }, reason: 'Behind projects onto the floor; any corner order');
      r.model.marqueeSelect(
        r.pixelOf('arm') - const Offset(10, 10),
        r.pixelOf('arm') + const Offset(10, 10),
        add: true,
      );
      expect(st.tab.selection.toSet(), {'crate', 'behind', 'floor', 'arm'});
      r.model.marqueeSelect(
        r.pixelOf('arm') - const Offset(10, 10),
        r.pixelOf('arm') + const Offset(10, 10),
        toggle: true,
      );
      expect(st.tab.selection.toSet(), {'crate', 'behind', 'floor'});
    });

    test('pointer events drive click, marquee and orbit', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      final px = r.pixelOf('crate');
      r.model.pointerDown(_down(px, kPrimaryMouseButton), px);
      r.model.pointerUp(_up(px), px);
      expect(st.tab.selection.toList(), ['crate']);

      // A drag of more than 4 px is a marquee.
      final a = r.pixelOf('floor') - const Offset(40, 40);
      final b = r.pixelOf('crate') + const Offset(40, 40);
      r.model.pointerDown(_down(a, kPrimaryMouseButton), a);
      r.model.pointerMove(_move(a + const Offset(2, 2)), a + const Offset(2, 2));
      expect(r.model.marquee, isNull);
      r.model.pointerMove(_move(b), b);
      expect(r.model.marquee, isNotNull);
      r.model.pointerUp(_up(b), b);
      expect(r.model.marquee, isNull);
      expect(st.tab.selection.toSet(), {'floor', 'crate', 'behind'});

      // The middle button pans: the pivot moves, the selection does not.
      final pivot = List.of(st.camera.pivot);
      r.model.pointerDown(_down(px, kMiddleMouseButton), px);
      r.model.pointerMove(_move(px + const Offset(50, 0)), px + const Offset(50, 0));
      r.model.pointerUp(_up(px), px);
      expect(st.camera.pivot, isNot(pivot));
      expect(st.tab.selection.toSet(), {'floor', 'crate', 'behind'});
      expect(st.cameraVersion, greaterThan(1));
      // Scroll dollies.
      final d = st.camera.distance;
      r.model.pointerSignal(PointerScrollEvent(position: px, scrollDelta: const Offset(0, -40)), px);
      expect(st.camera.distance, lessThan(d));
    });

    test('a trackpad scrolls and pinches the view like the wheel (macOS sends them as pan-zoom)', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      const px = Offset(800, 450);
      final d0 = st.camera.distance;
      // Fingers that move the page down, as a wheel rolled away does: the view comes closer.
      r.model.panZoomStart(const PointerPanZoomStartEvent(position: px), px);
      r.model.panZoomUpdate(const PointerPanZoomUpdateEvent(position: px, pan: Offset(0, 20)), px);
      r.model.panZoomUpdate(const PointerPanZoomUpdateEvent(position: px, pan: Offset(0, 40)), px);
      r.model.panZoomEnd(const PointerPanZoomEndEvent(position: px), px);
      final d1 = st.camera.distance;
      expect(d1, lessThan(d0));
      // The same as one wheel scroll of 40 (the gesture's totals are applied as deltas).
      final wheel = Rig();
      addTearDown(wheel.dispose);
      final ws = wheel.open();
      wheel.model.pointerSignal(PointerScrollEvent(position: px, scrollDelta: const Offset(0, -40)), px);
      expect(d1, closeTo(ws.camera.distance, 1e-9));

      // A pinch out halves the distance; a pinch in doubles it.
      r.model.panZoomStart(const PointerPanZoomStartEvent(position: px), px);
      r.model.panZoomUpdate(const PointerPanZoomUpdateEvent(position: px, scale: 1.5), px);
      r.model.panZoomUpdate(const PointerPanZoomUpdateEvent(position: px, scale: 2), px);
      r.model.panZoomEnd(const PointerPanZoomEndEvent(position: px), px);
      expect(st.camera.distance, closeTo(d1 / 2, 1e-9));
      r.model.panZoomStart(const PointerPanZoomStartEvent(position: px), px);
      r.model.panZoomUpdate(const PointerPanZoomUpdateEvent(position: px, scale: 0.5), px);
      r.model.panZoomEnd(const PointerPanZoomEndEvent(position: px), px);
      expect(st.camera.distance, closeTo(d1, 1e-9));
      expect(st.cameraVersion, greaterThan(1));
    });
  });

  group('gizmo drags', () {
    test('a move along X is one transaction of one setField; the preview is restored; undo is one step', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      r.model.tool = SceneTool.move;
      st.tab.select('floor');
      final g = r.model.projectedGizmo()!;
      final tip = g.handles.singleWhere((h) => h.handle == GizmoHandle.x).tip!;
      expect(g.hitTest(tip), GizmoHandle.x);

      final startPosition = st.tab.tree['floor']!.transform!.position;
      expect(r.model.beginDrag(GizmoHandle.x, tip), isTrue);
      expect(r.model.drag, isNotNull);
      r.model.updateDrag(tip + const Offset(80, 0));
      // Previewed, not written.
      expect(r.preview.overrides.keys, ['floor']);
      expect(r.applies(), 0);
      final previewed = r.preview.overrides['floor']!;
      expect(previewed.position[0], greaterThan(startPosition[0] + 0.5));
      expect(previewed.position[1], closeTo(0, 1e-9), reason: 'X only');
      expect(previewed.position[2], closeTo(0, 1e-9));
      expect(r.model.drag!.readout, endsWith(' m'));

      final applied = r.model.endDrag();
      expect(applied?.ok, isTrue);
      expect(r.applies(), 1, reason: 'exactly one transaction');
      expect(r.document.history, 1);
      expect(applied!.mutations, isNotEmpty);
      expect(r.preview.overrides, isEmpty, reason: 'the world is the document again');
      final committed = st.tab.tree['floor']!.transform!;
      expect(committed.position[0], closeTo(previewed.position[0], 1e-9));
      expect(committed.rotation, [0, 0, 0, 1]);
      // The dump equals a fresh world: the fake's world is its document.
      final dump = r.preview.dump();
      final entities = dump['entities'] as Map;
      expect((entities['floor'] as Map)['world'], committed.toMatrix());

      expect(st.tab.undo(), isTrue);
      expect(st.tab.tree['floor']!.transform!.position, startPosition);
      expect(r.document.history, 0);
    });

    test('snapping rounds the move; ⌘ inverts it; a drag that does not move writes nothing', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      r.model.tool = SceneTool.move;
      r.model.snap = r.model.snap.copyWith(enabled: true, move: 1);
      st.tab.select('floor');
      var tip = r.model.projectedGizmo()!.handles.singleWhere((h) => h.handle == GizmoHandle.x).tip!;
      r.model.beginDrag(GizmoHandle.x, tip);
      r.model.updateDrag(tip + const Offset(70, 0));
      final x = r.preview.overrides['floor']!.position[0];
      expect(x, closeTo(x.roundToDouble(), 1e-9));
      expect(x, greaterThan(0.5));
      r.model.endDrag();
      expect(r.applies(), 1);

      tip = r.model.projectedGizmo()!.handles.singleWhere((h) => h.handle == GizmoHandle.x).tip!;
      r.model.beginDrag(GizmoHandle.x, tip);
      r.model.updateDrag(tip + const Offset(70, 0), invertSnap: true);
      final x2 = r.preview.overrides['floor']!.position[0];
      expect(x2, isNot(closeTo(x2.roundToDouble(), 1e-6)), reason: '⌘ turns snapping off for this drag');
      r.model.cancelDrag();
      expect(r.preview.overrides, isEmpty);
      expect(r.applies(), 1, reason: 'Escape writes nothing');

      tip = r.model.projectedGizmo()!.handles.singleWhere((h) => h.handle == GizmoHandle.x).tip!;
      r.model.beginDrag(GizmoHandle.x, tip);
      r.model.updateDrag(tip + const Offset(1, 0));
      expect(r.model.endDrag(), isNull);
      expect(r.applies(), 1, reason: 'a click on a handle writes nothing');
    });

    test('rotate about Y writes rotation only; scale from the centre writes scale only', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      st.tab.select('crate');
      r.model.tool = SceneTool.rotate;
      var g = r.model.projectedGizmo()!;
      final yRing = g.handles.singleWhere((h) => h.handle == GizmoHandle.y);
      final grab = yRing.points[3];
      expect(g.hitTest(grab), GizmoHandle.y);
      r.model.beginDrag(GizmoHandle.y, grab);
      // Sweep a quarter turn around the projected pivot.
      final rel = grab - g.pivot;
      final rotated = g.pivot + Offset(-rel.dy, rel.dx);
      r.model.updateDrag(rotated);
      expect(r.model.drag!.readout, endsWith('°'));
      final applied = r.model.endDrag()!;
      expect(applied.ok, isTrue);
      final t = st.tab.tree['crate']!.transform!;
      expect(t.position, [4, 0, 0]);
      expect(t.scale, [1, 1, 1]);
      expect(t.rotation[1].abs(), greaterThan(0.5), reason: 'turned about Y');
      expect(t.rotation[0].abs() + t.rotation[2].abs(), lessThan(1e-6));
      expect(r.applies(), 1);

      r.model.tool = SceneTool.scale;
      g = r.model.projectedGizmo()!;
      r.model.beginDrag(GizmoHandle.center, g.pivot + const Offset(40, 0));
      r.model.updateDrag(g.pivot + const Offset(80, 0));
      expect(r.model.drag!.readout, startsWith('×'));
      expect(r.model.endDrag()!.ok, isTrue);
      final s = st.tab.tree['crate']!.transform!;
      expect(s.scale[0], closeTo(2, 1e-6));
      expect(s.scale[1], closeTo(2, 1e-6));
      expect(s.scale[2], closeTo(2, 1e-6));
      expect(s.position, [4, 0, 0]);
      expect(r.applies(), 2);
    });

    test('a multi-selection moves around the lead in one transaction; locked and children are skipped', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      r.model.tool = SceneTool.move;
      st.tab.select('crate');
      st.tab.select('robot', toggle: true);
      st.tab.select('arm', toggle: true); // below robot: the parent moves it
      r.editorState.scene(r.document.id).locked.add('floor');
      st.tab.select('floor', toggle: true);
      final g = r.model.projectedGizmo()!;
      final tip = g.handles.singleWhere((h) => h.handle == GizmoHandle.z).tip!;
      r.model.beginDrag(GizmoHandle.z, tip);
      expect(r.model.drag!.ids.toSet(), {'crate', 'robot'});
      r.model.updateDrag(tip + const Offset(0, -60));
      final a = r.model.endDrag()!;
      expect(a.ok, isTrue);
      expect(r.applies(), 1);
      expect(r.document.history, 1);
      final crate = st.tab.tree['crate']!.transform!, robot = st.tab.tree['robot']!.transform!;
      expect(crate.position[2], isNot(closeTo(0, 1e-6)));
      expect(robot.position[2], closeTo(crate.position[2], 1e-6), reason: 'the same delta');
      expect(st.tab.tree['arm']!.transform!.position, [0, 1.5, 0], reason: 'the child\'s local is untouched');
      expect(st.tab.tree['floor']!.transform!.position, [0, 0, 0], reason: 'locked');
      expect(st.tab.undo(), isTrue);
      expect(st.tab.tree['crate']!.transform!.position, [4, 0, 0]);
      expect(st.tab.tree['robot']!.transform!.position, [-4, 0, 0]);
    });

    test('a move of a derived entity is one setPrefabOverride on its anchor; the prefab is untouched', () {
      final r = Rig();
      addTearDown(r.dispose);
      final prefabDoc = r.project.addDocument(
        FakeDocument(
          path: '/fake/game/assets/prefabs/crate.prefab.json',
          relativePath: 'prefabs/crate.prefab.json',
          id: 'crate-prefab',
          kind: 'prefab',
          name: 'Crate',
          roots: ['root'],
          entities: {
            'root': FakeDocument.entity('Crate', children: ['mesh']),
            'mesh': FakeDocument.entity('Mesh', parent: 'root', position: [0, 1, 0]),
          },
        ),
      );
      final prefabText = prefabDoc.revision;
      expect(
        r.document.apply([
          Command.instantiatePrefab(
            prefab: {'asset': 'crate-prefab'},
            id: 'inst',
            name: 'Crate A',
            components: [
              {
                'id': 'inst:t',
                'type': 'fynkel.Transform',
                'version': 1,
                'data': {
                  'position': [2, 0, 0],
                  'rotation': [0, 0, 0, 1],
                  'scale': [1, 1, 1],
                },
              },
            ],
          ),
        ]).ok,
        isTrue,
      );
      final st = r.open();
      r.model.tool = SceneTool.move;
      r.hierarchy.expand('inst');
      final mesh = st.tab.tree['inst:mesh']!;
      expect(mesh.isDerived, isTrue);
      expect(mesh.transformId, 'inst:t');
      st.tab.select('inst:mesh');
      expect(r.model.noGizmoReason, isNull, reason: 'a derived entity with a Transform has the gizmo');
      expect(r.model.gizmoGeometry()!.pivot, [2, 1, 0], reason: 'the derived entity\'s world origin');
      final g = r.model.projectedGizmo()!;
      final tip = g.handles.singleWhere((h) => h.handle == GizmoHandle.x).tip!;
      expect(r.model.beginDrag(GizmoHandle.x, tip), isTrue);
      r.model.updateDrag(tip + const Offset(80, 0));
      expect(r.preview.overrides.keys, ['inst:mesh'], reason: 'previewed on the derived entity in the world');
      final previewed = r.preview.overrides['inst:mesh']!;
      expect(previewed.position[0], greaterThan(0.5));
      final applied = r.model.endDrag();
      expect(applied?.ok, isTrue);
      expect(r.applies(), 2, reason: 'the instantiate, then exactly one transaction for the drag');
      expect(r.document.history, 2);
      // The override, on the anchor, addresses the prefab-local ids with the whole position.
      final overrides = (r.document.entitiesView['inst']!['prefab'] as Map)['overrides'] as List;
      expect(overrides, hasLength(1));
      final o = overrides.single as Map;
      expect(o['op'], 'setField');
      expect(o['entity'], 'mesh');
      expect(o['component'], 't');
      expect(o['field'], 'position');
      expect((o['value'] as List)[0], closeTo(previewed.position[0], 1e-9));
      expect((o['value'] as List)[1], closeTo(1, 1e-9));
      expect(prefabDoc.revision, prefabText, reason: 'the prefab document is never written by a drag');
      // The world and the Hierarchy follow the committed override; undo is one step.
      expect(r.preview.overrides, isEmpty);
      final committed = st.tab.tree['inst:mesh']!.transform!;
      expect(committed.position[0], closeTo(previewed.position[0], 1e-9));
      expect(st.tab.tree['inst:mesh']!.overriddenFields, 1);
      expect(r.preview.worldOf('inst:mesh')![12], closeTo(2 + previewed.position[0], 1e-9));
      expect(st.tab.undo(), isTrue);
      expect(st.tab.tree['inst:mesh']!.transform!.position, [0, 1, 0]);
      expect(((r.document.entitiesView['inst']!['prefab'] as Map)['overrides'] as List), isEmpty);
    });

    test('a read-only document has no gizmo drag', () {
      final r = Rig(doc: scene(readOnly: 'generated'));
      addTearDown(r.dispose);
      final st = r.open();
      r.model.tool = SceneTool.move;
      st.tab.select('floor');
      expect(r.model.canEdit, isFalse);
      expect(r.model.noGizmoReason, contains('read-only'));
      expect(r.model.projectedGizmo(), isNull);
      expect(r.model.beginDrag(GizmoHandle.x, const Offset(500, 400)), isFalse);
    });
  });

  group('hidden and locked (editor-only, in editor.json)', () {
    test('hidden subtrees leave the picture and picking; Show All brings them back', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      st.tab.select('robot');
      r.model.toggleHiddenSelection();
      expect(r.editorState.scene(r.document.id).hidden, {'robot'});
      expect(r.model.isHidden('robot'), isTrue);
      expect(r.preview.hiddenIds, {'robot', 'arm'}, reason: 'the subtree');
      expect(r.model.hitsAt(r.pixelOf('arm')), isEmpty);
      expect(r.model.hasHidden, isTrue);
      expect(r.preview.calls.where((c) => c == 'hidden').length, 1);
      // Nothing in the document changed.
      expect(r.applies(), 0);
      expect(r.document.dirty, isFalse);

      r.model.toggleHiddenSelection();
      expect(r.editorState.scene(r.document.id).hidden, isEmpty);
      st.tab.select('crate');
      r.model.toggleHiddenSelection();
      r.model.showAll();
      expect(r.model.hasHidden, isFalse);
    });

    test('locked entities are never picked or dragged; the Hierarchy still selects them', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      st.tab.select('floor');
      r.model.toggleLockedSelection();
      expect(r.editorState.scene(r.document.id).locked, {'floor'});
      expect(r.model.hitsAt(r.pixelOf('floor')), ['behind'], reason: 'the lock lets the click through to Behind');
      r.model.tool = SceneTool.move;
      expect(r.model.gizmoGeometry(), isNull, reason: 'nothing draggable is selected');
      expect(r.model.noGizmoReason, contains('locked'));
      st.tab.select('crate', toggle: true);
      expect(r.model.gizmoGeometry(), isNotNull);
      expect(r.model.noGizmoReason, isNull);
      expect(r.model.gizmoGeometry()!.pivot, [4, 0, 0], reason: 'the crate, not the locked floor');
      r.model.toggleLockedSelection();
      expect(r.editorState.scene(r.document.id).locked, {'floor', 'crate'}, reason: 'not all were locked → lock all');
      r.model.toggleLockedSelection();
      expect(r.editorState.scene(r.document.id).locked, isEmpty, reason: 'all locked → unlock all');
    });
  });

  group('camera commands', () {
    test('frame selected / frame all move the pivot onto the bounds; the tool, space and toggles persist', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      st.tab.select('far');
      r.model.frameSelected();
      expect(st.camera.pivot[2], closeTo(-40, 1e-9));
      expect(st.camera.distance, lessThan(10));
      r.model.frameAll();
      expect(
        st.camera.pivot[2],
        closeTo((-40.5 + 12.5) / 2, 1e-9),
        reason: 'the centre of every entity, Far to the camera entity',
      );
      expect(r.editorState.savePending, isTrue);

      expect(r.model.tool, SceneTool.move);
      r.model.tool = SceneTool.rotate;
      r.model.toggleSpace();
      r.model.toggleGrid();
      r.model.toggleOverlays();
      r.model.toggleStats();
      expect(r.editorState.tool, SceneTool.rotate);
      expect(r.editorState.space, GizmoSpace.local);
      expect(r.editorState.showGrid, isFalse);
      expect(r.editorState.showOverlays, isFalse);
      expect(r.editorState.showStats, isTrue);
      r.model.toggleProjection();
      expect(st.camera.orthographic, isTrue);
      r.model.resetView();
      expect(st.camera.orthographic, isFalse);
      expect(st.camera.pivot, EditorCamera.initial().pivot);
    });

    test('look through the selected camera takes its matrix and lens; align camera to view is one transaction', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      expect(r.model.canAlignToSelectedCamera, isFalse);
      st.tab.select('cam');
      expect(r.model.canAlignToSelectedCamera, isTrue);
      r.model.alignViewToSelectedCamera();
      expect(matTranslation(st.camera.world), [0, 3, 12]);
      expect(st.camera.fovY, 0.9);
      expect(st.camera.near, 0.2);
      expect(st.camera.far, 80);
      expect(st.camera.manualExposure, 11);
      expect(st.camera.toPreview().toJson()['exposureMode'], 'manual');

      st.camera.orbit(120, 30);
      st.camera.dolly(3);
      st.camera.toggleProjection();
      expect(r.model.alignSelectedCameraToView(), isTrue);
      expect(r.applies(), 1);
      expect(r.document.history, 1);
      final cam = r.document.entitiesView['cam']!;
      final t = (cam['components'] as Map)['t'] as Map;
      final data = t['data'] as Map;
      final pos = (data['position'] as List).cast<num>();
      for (var i = 0; i < 3; i++) {
        expect(pos[i].toDouble(), closeTo(st.camera.position[i], 1e-5));
      }
      final c = ((cam['components'] as Map)['c'] as Map)['data'] as Map;
      expect(c['projection'], 'orthographic');
      expect(c['fov_y'], closeTo(0.9, 1e-9));
      expect(c['near'], closeTo(0.2, 1e-9));
      expect(c['far'], closeTo(80, 1e-9));
      expect(c['exposure_mode'], 'manual', reason: 'exposure is the scene camera\'s own; the view only reads it');
      // The view is now that camera exactly.
      expect(st.camera.alignedWorld, isNotNull);
    });

    test('reload and rebuild go to the preview and say so; a changed asset set rebuilds on its own', () {
      final r = Rig();
      addTearDown(r.dispose);
      r.open();
      r.model.reload();
      expect(r.preview.calls, contains('reload'));
      expect(r.model.notice, contains('reloaded'));
      r.model.recook();
      expect(r.preview.calls, contains('recook'));
      expect(r.model.notice, contains('Cache rebuilt'));
      final before = r.preview.calls.where((c) => c == 'recook').length;
      r.project.createFolder('x');
      expect(r.preview.calls.where((c) => c == 'recook').length, before + 1);
    });

    test('Save Frame without an attached viewport says so and writes nothing', () async {
      final r = Rig();
      addTearDown(r.dispose);
      r.open();
      r.savePath = '/nowhere/frame.png';
      expect(await r.model.saveFrame(), isNull);
      expect(r.model.notice, contains('not attached'));
    });

    test('the grid is the renderer\'s, and overlay widths are logical pixels times the ratio', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      if (!r.model.showGrid) r.model.toggleGrid();
      if (r.model.showOverlays) r.model.toggleOverlays();
      r.model.pixelRatio = 2;
      final o = r.model.overlay(st);
      expect(o.grid, isNotNull, reason: 'a shader grid, not line segments');
      expect(o.grid!.xAxis, GizmoTokens.xAxis);
      expect(o.lineCount, 0, reason: 'no Y axis: the grid carries X and Z');
      // The icons' lines: 1 logical px on a 2× display.
      r.model.toggleOverlays();
      final icons = r.model.overlay(st);
      expect(icons.lineCount, greaterThan(0));
      expect(icons.lines[10], 2);
      r.model.toggleOverlays();
      r.model.toggleGrid();
      final off = r.model.overlay(st);
      expect(off.grid, isNull);
      expect(off.lineCount, 0);
    });
  });

  group('prefab drop', () {
    test('a prefab dropped on a mesh lands at the hit, on nothing at the ground plane, in one transaction', () {
      final r = Rig();
      addTearDown(r.dispose);
      final st = r.open();
      const payload = AssetDragPayload(id: 'prefab-crate', kind: 'prefab', name: 'crate.prefab.json');
      expect(r.model.canDropAsset(payload), isTrue);
      expect(r.model.canDropAsset(const AssetDragPayload(id: 'm', kind: 'material', name: 'm.material.json')), isFalse);

      final onCrate = r.model.dropPoint(r.pixelOf('crate'))!;
      expect(onCrate[2], closeTo(0.5, 1e-6), reason: 'the near face of the unit cube');
      expect(onCrate[0], closeTo(4 * 19.5 / 20, 1e-6), reason: 'where the slanted ray meets that face');

      // The top-left pixel's ray climbs away from the ground (the camera
      // sits at y = 0, on the plane — that is not a hit), so 5 m along it.
      final nothing = r.model.dropPoint(const Offset(10, 10))!;
      expect(nothing[1], greaterThan(1));
      expect(vDistance(nothing, const [0, 0, 20]), closeTo(5, 1e-6));

      expect(r.model.dropPrefab(payload, r.pixelOf('crate')), isTrue);
      expect(r.applies(), 1);
      final created = st.tab.selection.single;
      final n = st.tab.tree[created]!;
      expect(n.isPrefabInstance, isTrue);
      expect(n.name, 'crate');
      expect(n.transform!.position[0], closeTo(4 * 19.5 / 20, 1e-6));
      expect(n.transform!.position[2], closeTo(0.5, 1e-6));
    });
  });

  group('panel', () {
    Future<Rig> pump(WidgetTester tester) async {
      tester.view.physicalSize = const Size(1000, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final r = Rig();
      addTearDown(r.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Column(
              children: [
                Draggable<AssetDragPayload>(
                  key: const Key('drag.source'),
                  data: const AssetDragPayload(id: 'prefab-crate', kind: 'prefab', name: 'crate.prefab.json'),
                  dragAnchorStrategy: pointerDragAnchorStrategy,
                  feedback: const SizedBox(width: 10, height: 10),
                  child: const SizedBox(width: 40, height: 40, child: Text('P')),
                ),
                Expanded(child: ScenePanel(model: r.model, showViewport: false)),
              ],
            ),
          ),
        ),
      );
      expect(find.text('No document open'), findsOneWidget);
      r.tabs.openDocument(r.document.id);
      await tester.pump();
      final st = r.model.current!;
      st.camera.alignTo(
        Trs(position: const [0, 0, 20], rotation: const [0, 0, 0, 1], scale: const [1, 1, 1]).toMatrix(),
      );
      await tester.pump();
      expect(r.model.viewportWidth, greaterThan(0));
      return r;
    }

    /// The panel's picture origin in the window (physical = logical here).
    Offset origin(WidgetTester tester) => tester.getTopLeft(find.byKey(const Key('scene.overlay')));

    testWidgets('item 9e: the orientation gizmo turns with the camera; its ends, centre, caption and drag', (
      tester,
    ) async {
      final r = await pump(tester);
      final st = r.model.current!;
      final gizmo = find.byKey(const Key('scene.orient'));
      expect(gizmo, findsOneWidget);
      final centre = tester.getCenter(gizmo);
      expect(find.text('Front · Persp'), findsOneWidget, reason: 'the rig looks down −Z from +Z');

      // The ends as drawn now: +X to the right, +Y up, Z towards the viewer.
      Offset endOf(int axis, bool positive) =>
          centre +
          OrientationGizmo.endsFor(st.camera).singleWhere((e) => e.axis == axis && e.positive == positive).offset;
      expect(endOf(0, true).dx, greaterThan(centre.dx + 20));
      expect(endOf(1, true).dy, lessThan(centre.dy - 20));

      await tester.tapAt(endOf(1, true));
      await tester.pump();
      expect(st.camera.viewName, 'Top');
      expect(find.text('Top · Persp'), findsOneWidget);
      expect(st.cameraVersion, greaterThan(1), reason: 'the world is told');

      expect(tester.getCenter(gizmo), centre, reason: 'the circle stays put while the caption changes');
      await tester.tapAt(endOf(0, false));
      await tester.pump();
      expect(st.camera.viewName, 'Left');
      // Again on the same end: from the other side.
      await tester.tapAt(endOf(0, false));
      await tester.pump();
      expect(st.camera.viewName, 'Right');

      // Looking along X, the X end covers the centre: a click there is a
      // click on that end (from the other side again), what the eye sees.
      expect(OrientationGizmo.endAt(st.camera, Offset.zero)?.axis, 0);
      await tester.tapAt(centre);
      await tester.pump();
      expect(st.camera.viewName, 'Left');
      expect(st.camera.orthographic, isFalse);

      // The caption always switches the projection.
      await tester.tap(find.byKey(const Key('scene.orient.projection')));
      await tester.pump();
      expect(st.camera.orthographic, isTrue);
      expect(find.text('Left · Ortho'), findsOneWidget);

      // A drag orbits (and is not a click).
      final yaw = st.camera.yaw;
      await tester.dragFrom(centre + const Offset(-20, 20), const Offset(100, 0));
      await tester.pump();
      expect(st.camera.yaw, isNot(closeTo(yaw, 1e-6)));
      expect(st.camera.viewName, isNull);
      expect(st.tab.selection, isEmpty, reason: 'nothing reached the picture under it');

      // Between views no end covers the centre, and a click there switches
      // the projection too.
      expect(OrientationGizmo.endAt(st.camera, Offset.zero), isNull);
      final ortho = st.camera.orthographic;
      await tester.tapAt(centre);
      await tester.pump();
      expect(st.camera.orthographic, !ortho);
      await tester.pump(const Duration(seconds: 1)); // the camera's debounced save
    });

    testWidgets('a tap selects, hotkeys switch tools and frame, Escape clears, ⌫ deletes', (tester) async {
      final r = await pump(tester);
      final st = r.model.current!;
      final o = origin(tester);
      await tester.tapAt(o + r.pixelOf('crate'));
      await tester.pump();
      expect(st.tab.selection.toList(), ['crate']);
      expect(r.model.hasFocus, isTrue);
      expect(find.byKey(const Key('scene.status')), findsOneWidget);
      expect(find.textContaining('1 selected'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.keyE);
      expect(r.model.tool, SceneTool.rotate);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyR);
      expect(r.model.tool, SceneTool.scale);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyQ);
      expect(r.model.tool, SceneTool.select);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyW);
      expect(r.model.tool, SceneTool.move);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyX);
      expect(r.model.space, GizmoSpace.local);
      await tester.sendKeyEvent(LogicalKeyboardKey.digit5);
      expect(st.camera.orthographic, isTrue);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyF);
      expect(st.camera.pivot[0], closeTo(4, 1e-9), reason: 'framed the crate');
      await tester.sendKeyEvent(LogicalKeyboardKey.keyH);
      expect(r.model.isHidden('crate'), isTrue);
      await tester.pump();
      expect(find.byKey(const Key('scene.hiddenChip')), findsOneWidget);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyH);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      expect(r.model.hasHidden, isFalse);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyL);
      expect(r.model.isLocked('crate'), isTrue);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyL);
      expect(r.model.isLocked('crate'), isFalse);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      expect(st.tab.selection, isEmpty);

      await tester.tapAt(o + r.pixelOf('crate'));
      await tester.pump();
      expect(st.tab.selection.toList(), ['crate']);
      await tester.sendKeyEvent(LogicalKeyboardKey.backspace);
      await tester.pump();
      expect(st.tab.tree['crate'], isNull);
      expect(r.applies(), 1);
      await tester.pump(const Duration(seconds: 1)); // the editor-state save timer
    });

    testWidgets('the toolbar buttons switch tools and toggles; the overlay paints the gizmo; stats show', (
      tester,
    ) async {
      final r = await pump(tester);
      final st = r.model.current!;
      await tester.tap(find.byKey(const Key('scene.tool.rotate')));
      expect(r.model.tool, SceneTool.rotate);
      await tester.tap(find.byKey(const Key('scene.space')));
      expect(r.model.space, GizmoSpace.local);
      await tester.tap(find.byKey(const Key('scene.snap')));
      expect(r.model.snap.enabled, isTrue);
      await tester.tap(find.byKey(const Key('scene.grid')));
      expect(r.model.showGrid, isFalse);
      await tester.tap(find.byKey(const Key('scene.stats')));
      await tester.pump();
      expect(find.byKey(const Key('scene.statsBox')), findsOneWidget);
      expect(find.textContaining('entities 7'), findsOneWidget);
      await tester.tap(find.byKey(const Key('scene.projection')));
      expect(st.camera.orthographic, isTrue);
      st.tab.select('crate');
      await tester.pump();
      expect(r.model.projectedGizmo(), isNotNull);
      await tester.tap(find.byKey(const Key('scene.hide')));
      expect(r.model.isHidden('crate'), isTrue);
      await tester.pump(); // Show All is enabled only while something is hidden
      await tester.tap(find.byKey(const Key('scene.showAll')));
      expect(r.model.hasHidden, isFalse);
      await tester.tap(find.byKey(const Key('scene.reload')));
      await tester.pump();
      expect(find.byKey(const Key('scene.notice')), findsOneWidget);
      await tester.tap(find.byKey(const Key('scene.dismissNotice')));
      await tester.pump();
      expect(find.byKey(const Key('scene.notice')), findsNothing);
      await tester.pump(const Duration(seconds: 1)); // the editor-state save timer
    });

    testWidgets('a gizmo drag through the pointer is one transaction', (tester) async {
      final r = await pump(tester);
      final st = r.model.current!;
      final o = origin(tester);
      st.tab.select('floor');
      r.model.tool = SceneTool.move;
      await tester.pump();
      final tip = r.model.projectedGizmo()!.handles.singleWhere((h) => h.handle == GizmoHandle.x).tip!;
      final gesture = await tester.startGesture(o + tip, kind: PointerDeviceKind.mouse);
      await tester.pump();
      expect(r.model.drag, isNotNull);
      await gesture.moveBy(const Offset(60, 0));
      await tester.pump();
      expect(find.byKey(const Key('scene.readout')), findsOneWidget);
      expect(r.preview.overrides.keys, ['floor']);
      await gesture.up();
      await tester.pump();
      expect(r.model.drag, isNull);
      expect(r.applies(), 1);
      expect(st.tab.tree['floor']!.transform!.position[0], greaterThan(0.5));
      expect(r.preview.overrides, isEmpty);
    });

    testWidgets('a prefab dragged from the browser drops where it hits', (tester) async {
      final r = await pump(tester);
      final st = r.model.current!;
      final o = origin(tester);
      final from = tester.getCenter(find.byKey(const Key('drag.source')));
      final gesture = await tester.startGesture(from);
      await tester.pump(kLongPressTimeout + const Duration(milliseconds: 50));
      await gesture.moveTo(o + r.pixelOf('crate'));
      await tester.pump();
      await gesture.up();
      await tester.pumpAndSettle();
      expect(r.applies(), 1);
      final created = st.tab.selection.single;
      expect(st.tab.tree[created]!.isPrefabInstance, isTrue);
      expect(st.tab.tree[created]!.transform!.position[0], closeTo(4 * 19.5 / 20, 1e-6));
      expect(st.tab.tree[created]!.transform!.position[2], closeTo(0.5, 1e-6));
    });

    testWidgets('a material tab shows why there is no picture; a scene without a preview offers Try again', (
      tester,
    ) async {
      final r = await pump(tester);
      final m = r.project.addDocument(scene(kind: 'material', id: 'mat'));
      r.tabs.openDocument(m.id);
      await tester.pump();
      expect(find.text('A material has no world'), findsOneWidget);
      expect(find.byKey(const Key('scene.retry')), findsNothing);
    });
  });
  group('shell', () {
    testWidgets('every scene.* / view.* command runs through the registry against the focused scene', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      final doc = project.addDocument(scene());
      final prefs = Preferences.inMemory();
      await pumpLarge(tester, shellApp(prefs: prefs, projects: {'/fake/game': project}, initialProject: '/fake/game'));
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      final registry = shell.registry;
      // Nothing open: the view commands refuse politely.
      for (final id in ['view.frameAll', 'view.reload', 'scene.tool.rotate', 'view.saveFrame']) {
        expect(registry.byId(id), isNotNull, reason: id);
        expect(registry.execute(id), isFalse, reason: '$id with no scene');
      }
      shell.tabs!.openDocument(doc.id);
      await tester.pump();
      final sv = shell.scene!;
      final st = sv.current!;
      expect(st.hasPreview, isTrue);
      sv.setViewportSize(1000, 800);

      expect(registry.execute('scene.tool.rotate'), isTrue);
      expect(sv.tool, SceneTool.rotate);
      expect(registry.execute('scene.tool.select'), isTrue);
      expect(registry.execute('scene.tool.scale'), isTrue);
      expect(registry.execute('scene.tool.move'), isTrue);
      expect(sv.tool, SceneTool.move);
      expect(registry.execute('scene.toggleSpace'), isTrue);
      expect(sv.space, GizmoSpace.local);
      expect(registry.execute('scene.toggleSnap'), isTrue);
      expect(sv.snap.enabled, isTrue);
      expect(registry.execute('view.toggleGrid'), isTrue);
      expect(sv.showGrid, isFalse);
      expect(registry.execute('view.toggleOverlays'), isTrue);
      expect(sv.showOverlays, isFalse);
      expect(registry.execute('view.toggleStats'), isTrue);
      expect(sv.showStats, isTrue);
      expect(registry.execute('view.toggleProjection'), isTrue);
      expect(st.camera.orthographic, isTrue);

      st.tab.select('far');
      registry.refresh();
      expect(registry.execute('view.frameSelected'), isTrue);
      expect(st.camera.pivot[0], closeTo(20, 1e-9));
      expect(registry.execute('view.frameAll'), isTrue);
      expect(st.camera.pivot[0], isNot(closeTo(20, 1e-9)));
      expect(registry.execute('view.reset'), isTrue);
      expect(st.camera.orthographic, isFalse);

      expect(registry.byId('view.alignToCamera')!.isEnabled, isFalse, reason: 'Far is no camera');
      st.tab.select('cam');
      registry.refresh();
      expect(registry.execute('view.alignToCamera'), isTrue);
      expect(matTranslation(st.camera.world), [0, 3, 12]);
      st.camera.orbit(40, 10);
      expect(registry.execute('scene.alignCameraToView'), isTrue);
      expect(doc.history, 1);

      st.tab.select('robot');
      registry.refresh();
      expect(registry.execute('scene.toggleHidden'), isTrue);
      expect(sv.isHidden('robot'), isTrue);
      expect(doc.fakePreview!.hiddenIds, {'robot', 'arm'});
      expect(registry.execute('scene.showAll'), isTrue);
      expect(sv.hasHidden, isFalse);
      expect(registry.execute('scene.toggleLocked'), isTrue);
      expect(sv.isLocked('robot'), isTrue);
      expect(registry.execute('scene.toggleLocked'), isTrue);
      expect(sv.isLocked('robot'), isFalse);

      final calls = doc.fakePreview!.calls;
      expect(registry.execute('view.reload'), isTrue);
      expect(calls.last, 'reload');
      expect(registry.execute('view.recook'), isTrue);
      expect(calls.last, 'recook');
      expect(registry.execute('view.saveFrame'), isTrue);
      await tester.pump();
      expect(sv.notice, contains('not attached'));
      // Nothing above touched the document but the camera alignment.
      expect(doc.history, 1);
      // The flags live in the editor's state file, never in the scene.
      expect(shell.editorState!.scene(doc.id).hidden, isEmpty);
      await tester.pump(const Duration(seconds: 1));
    });
  });
}

PointerDownEvent _down(Offset p, int buttons) =>
    PointerDownEvent(position: p, buttons: buttons, kind: PointerDeviceKind.mouse);
PointerMoveEvent _move(Offset p) => PointerMoveEvent(position: p, kind: PointerDeviceKind.mouse);
PointerUpEvent _up(Offset p) => PointerUpEvent(position: p, kind: PointerDeviceKind.mouse);
