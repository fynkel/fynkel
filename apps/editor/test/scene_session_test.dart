// The Scene view over the real `SessionProject` and the real edit-preview
// world (the M4-06 gate, headless, on a copy of the fixture project):
// every entity of every fixture scene is picked by a ray through its
// centre (prefab instances by their anchor); a gizmo drag is one undo step
// and leaves the world equal to a fresh load; the editor-only hidden set
// reaches the world and never the document; nothing ticks.
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_model.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/scene_view/editor_state.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_editor/src/scene_view/gizmo.dart';
import 'package:fynkel_editor/src/scene_view/png.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';
import 'package:fynkel_editor/src/scene_view/scene_view_model.dart';

import 'documents_session_test.dart' show mainScene, robot, crateA, crateB, floor;
import 'project_test.dart' show copyFixture;

final class Stack {
  Stack(this.dir, this.project) {
    tabs = DocumentTabs(project: project);
    hierarchy = HierarchyModel(tabs: tabs, project: project);
    editorState = EditorState.load(dir.path);
    model = SceneViewModel(
      tabs: tabs,
      project: project,
      hierarchy: hierarchy,
      editorState: editorState,
      chooseSavePath: (name) async => savePath == null ? null : '$savePath/$name',
    );
    model.setViewportSize(1600, 900);
  }

  /// Where Save Frame writes (null: the dialog was cancelled).
  String? savePath;

  static Future<Stack> open(String tag) async {
    final dir = copyFixture(tag);
    final project = await SessionProject.open(dir.path, watch: false);
    return Stack(dir, project);
  }

  final Directory dir;
  final SessionProject project;
  late final DocumentTabs tabs;
  late final HierarchyModel hierarchy;
  late final EditorState editorState;
  late final SceneViewModel model;

  void close() {
    model.dispose();
    hierarchy.dispose();
    tabs.dispose();
    editorState.dispose();
    project.close();
    dir.deleteSync(recursive: true);
  }

  /// The pixel the world point projects to in the focused tab's view.
  Offset pixelOf(Vec3 p) {
    final px = model.viewProjection!.project(p)!;
    return Offset(px[0], px[1]);
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('every entity of every fixture scene is picked by a ray through its centre', () async {
    final s = await Stack.open('pick');
    addTearDown(s.close);
    final scenes = s.project.documentAssets().where((d) => d.kind == 'scene').toList();
    expect(scenes.map((d) => d.name).toSet(), containsAll(['Main', 'Playground', 'Catalogue', 'Results', 'Rig']));
    var picked = 0;
    for (final doc in scenes) {
      final tab = s.tabs.openDocument(doc.id)!;
      final st = s.model.current!;
      expect(st.tab, tab);
      expect(st.hasPreview, isTrue, reason: '${doc.name}: ${st.openError}');
      final preview = st.preview!;
      expect(preview.diagnostics.where((d) => d.severity == Severity.error), isEmpty, reason: doc.name);
      for (final id in tab.tree.nodes.keys.toList()) {
        final node = tab.tree[id]!;
        // The centre of what the entity draws — its outline box (a mesh's
        // bounds, an icon's cross), from the world itself.
        final b = s.model.boundsOf([id]);
        expect(b, isNotNull, reason: '${doc.name}/${node.name} has no outline');
        // A prefab instance's outline is the union of its parts (the rig's
        // body and its prop, a gap between): its centre is its origin.
        final centre = node.isPrefabInstance
            ? matTranslation(preview.worldMatrices([id])[id]!)
            : vScale(vAdd(b!.$1, b.$2), 0.5);
        final box = b!;
        // Look at it from a diagonal above (30° down, 35° round — not so
        // steep that the ray slips through an open column like the rig's
        // body), through the pixel its centre takes.
        s.model.applyHidden();
        st.camera.frame(box.$1, box.$2);
        st.camera.yaw = 0.6;
        st.camera.pitch = -0.52;
        final px = s.pixelOf(centre);
        final hits = s.model.hitsAt(px);
        expect(hits, contains(id), reason: '${doc.name}/${node.name} ($id) at $px: hits $hits');
        picked++;
      }
      // The marquee over the whole picture takes every visible entity.
      s.model.frameAll();
      s.model.marqueeSelect(Offset.zero, const Offset(1600, 900));
      expect(tab.selection.toSet(), tab.tree.nodes.keys.toSet(), reason: doc.name);
      tab.clearSelection();
    }
    expect(picked, greaterThan(30));
  });

  test('a gizmo drag is one undo step and the world equals a fresh load; hidden never reaches the document', () async {
    final s = await Stack.open('drag');
    addTearDown(s.close);
    final tab = s.tabs.openDocument(mainScene)!;
    final st = s.model.current!;
    expect(st.hasPreview, isTrue, reason: st.openError);
    final preview = st.preview!;
    final syncs = <PreviewSyncReport>[];
    preview.syncs.listen(syncs.add);
    final history = tab.document.history;
    final revision = tab.document.revision;

    // The Robot has no Transform component (the fixture's choice): the
    // world places it at the origin and the gizmo has nothing to write.
    tab.select(robot);
    s.model.tool = SceneTool.move;
    expect(s.model.projectedGizmo(), isNull);
    expect(s.model.noGizmoReason, contains('no Transform'));
    expect(preview.worldMatrices([robot])[robot], isNotNull);

    // Move Crate A (a prefab instance, dragged by its anchor) along world X.
    tab.select(crateA);
    s.model.frameSelected();
    st.camera.distance = 8; // 120 px ≈ 1.2 m at this distance
    final startWorld = preview.worldMatrices([crateA])[crateA]!;
    final g = s.model.projectedGizmo()!;
    final tip = g.handles.singleWhere((h) => h.handle == GizmoHandle.x).tip!;
    expect(g.hitTest(tip), GizmoHandle.x);
    expect(s.model.beginDrag(GizmoHandle.x, tip), isTrue);
    s.model.updateDrag(tip + const Offset(120, 0));
    // Previewed in the world, not in the document.
    final previewed = preview.worldMatrices([crateA])[crateA]!;
    expect(previewed[12], greaterThan(startWorld[12] + 0.5));
    expect(tab.document.history, history);
    expect(tab.document.revision, revision);
    expect(tab.document.dirty, isFalse);

    final applied = s.model.endDrag();
    expect(applied, isNotNull, reason: s.model.notice);
    expect(applied!.ok, isTrue, reason: applied.diagnostics.join('; '));
    expect(tab.document.history, history + 1, reason: 'one transaction');
    expect(applied.mutations.length, 1, reason: 'the position only: ${applied.mutations}');
    final committed = preview.worldMatrices([crateA])[crateA]!;
    expect(committed[12], closeTo(previewed[12], 1e-4));
    expect(committed[13], closeTo(startWorld[13], 1e-6));
    expect(committed[14], closeTo(startWorld[14], 1e-6));
    expect(tab.tree[crateA]!.transform!.position[0], closeTo(previewed[12], 1e-4));
    await Future<void>.delayed(Duration.zero);
    expect(syncs.single.reloaded, isFalse, reason: 'the incremental sync');
    expect(syncs.single.updated, 1);

    // The world after the incremental sync is the world of a fresh load.
    final incremental = preview.dump();
    preview.reload();
    expect(preview.dump(), incremental);
    // One undo brings the robot back — in the document and the world.
    expect(tab.undo(), isTrue);
    expect(tab.document.history, history);
    expect(tab.document.revision, revision);
    final back = preview.worldMatrices([crateA])[crateA]!;
    for (var i = 0; i < 16; i++) {
      expect(back[i], closeTo(startWorld[i], 1e-6), reason: 'element $i');
    }

    // Rotate Crate B (a rigid body) about its local Y with snapping: one
    // transaction, rotation only.
    s.model.tool = SceneTool.rotate;
    s.model.snap = s.model.snap.copyWith(enabled: true, angle: 15);
    tab.select(crateB);
    s.model.frameSelected();
    final rg = s.model.projectedGizmo()!;
    final ring = rg.handles.singleWhere((h) => h.handle == GizmoHandle.y);
    final grab = ring.points[4];
    expect(s.model.beginDrag(GizmoHandle.y, grab), isTrue);
    final rel = grab - rg.pivot;
    s.model.updateDrag(rg.pivot + Offset(-rel.dy, rel.dx));
    expect(s.model.drag!.readout, endsWith('°'));
    final rotated = s.model.endDrag()!;
    expect(rotated.ok, isTrue);
    expect(tab.document.history, history + 1);
    expect(rotated.mutations.length, 1);
    final q = tab.tree[crateB]!.transform!.rotation;
    final angle = 2 * math.acos(q[3].clamp(-1.0, 1.0)) * 180 / math.pi;
    expect(angle % 15, anyOf(closeTo(0, 1e-3), closeTo(15, 1e-3)), reason: 'snapped: $angle°');
    expect(preview.dump(), (preview..reload()).dump());
    tab.undo();

    // Hidden: the world filters, the document never hears of it.
    tab.select(floor);
    s.model.toggleHiddenSelection();
    expect(preview.refreshStats().hidden, 1);
    expect(s.model.hitsAt(s.pixelOf(matTranslation(preview.worldMatrices([floor])[floor]!))), isNot(contains(floor)));
    expect(tab.document.dirty, isFalse);
    expect(tab.document.history, history);
    expect(File('${s.dir.path}/assets/scenes/main.scene.json').readAsStringSync(), isNot(contains('hidden')));
    s.editorState.flush();
    expect(EditorState.fileOf(s.dir.path).readAsStringSync(), contains(floor));
    // A reload re-applies it before the next frame.
    s.model.reload();
    expect(st.hiddenDirty, isTrue);
    s.model.applyHidden();
    expect(preview.refreshStats().hidden, 1);
    s.model.showAll();
    expect(preview.refreshStats().hidden, 0);

    // Nothing ticks: in edit mode a step only applies the sync's commands
    // (the counter moves with edits), never a system — the world stands
    // still with the document, Crate B's rigid body included.
    final still = preview.dump();
    final steps = preview.refreshStats().tick;
    await Future<void>.delayed(const Duration(milliseconds: 200));
    expect(preview.refreshStats().tick, steps);
    expect(preview.dump(), still);
    final crateBWorld = preview.worldMatrices([crateB])[crateB]!;
    expect(matTranslation(crateBWorld), tab.tree[crateB]!.transform!.position.map((v) => v.toDouble()).toList());
  });

  test('the camera of the scene can be looked through and written back; the state file keeps the view', () async {
    final s = await Stack.open('camera');
    addTearDown(s.close);
    final tab = s.tabs.openDocument(mainScene)!;
    final st = s.model.current!;
    final camId = tab.tree.nodes.entries.singleWhere((e) => e.value.hasComponent('fynkel.Camera')).key;
    tab.select(camId);
    expect(s.model.canAlignToSelectedCamera, isTrue);
    s.model.alignViewToSelectedCamera();
    final camWorld = st.preview!.worldMatrices([camId])[camId]!;
    for (var i = 0; i < 16; i++) {
      expect(st.camera.world[i], closeTo(camWorld[i], 1e-5), reason: 'element $i');
    }
    // Move the view, write it into the scene's camera: one transaction.
    st.camera.orbit(60, -20);
    final history = tab.document.history;
    expect(s.model.alignSelectedCameraToView(), isTrue);
    expect(tab.document.history, history + 1);
    final written = st.preview!.worldMatrices([camId])[camId]!;
    for (var i = 12; i < 15; i++) {
      expect(written[i], closeTo(st.camera.position[i - 12], 1e-4), reason: 'translation $i');
    }
    tab.undo();
    // The view survives a close and reopen through .fynkel/editor.json.
    st.camera.distance = 7.5;
    s.model.toggleProjection();
    s.editorState.flush();
    final again = EditorState.load(s.dir.path);
    expect(again.scene(mainScene).camera.distance, 7.5);
    expect(again.scene(mainScene).camera.orthographic, isTrue);
    again.dispose();
    // The state file is the editor's: the scene is untouched on disk.
    expect(tab.document.dirty, isFalse);
  });

  test('the viewport draws the preview world: a frame, its pixels, Save Frame as a PNG', () async {
    // The texture channel is the host's; the renderer itself (Metal,
    // offscreen) is real — as in fynkel_flutter's own controller test.
    const channel = MethodChannel('fynkel_flutter/viewport');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      switch (call.method) {
        case 'registerTexture':
          return 11;
        case 'rebindTexture' || 'unregisterTexture':
          return null;
        case 'stats':
          return <String, Object?>{};
      }
      throw MissingPluginException(call.method);
    });
    addTearDown(
      () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null),
    );
    final s = await Stack.open('render');
    addTearDown(s.close);
    final tab = s.tabs.openDocument(mainScene)!;
    final st = s.model.current!;
    expect(st.hasPreview, isTrue, reason: st.openError);
    expect(s.model.renderFrame(), isNull, reason: 'no ring yet');
    s.model.controller.requestSize(320, 200);
    for (var i = 0; i < 10; i++) {
      await Future<void>.delayed(Duration.zero);
    }
    expect(s.model.controller.isAttached, isTrue, reason: '${s.model.controller.error}');

    // Through the scene's camera, nothing of the editor's in the picture.
    final camId = tab.tree.nodes.entries.singleWhere((e) => e.value.hasComponent('fynkel.Camera')).key;
    tab.select(camId);
    s.model.alignViewToSelectedCamera();
    tab.clearSelection();
    if (s.model.showGrid) s.model.toggleGrid();
    if (s.model.showOverlays) s.model.toggleOverlays();
    expect(s.model.debugLines(st), isEmpty);
    // The mocked host never acquires frames, so the ring (3 slots) allows
    // three renders in this test: the capture and two saves.
    final pixels = s.model.captureFrame()!;
    expect(st.frames, 1);
    expect(st.lastSlot, greaterThanOrEqualTo(0));
    expect(st.appliedCamera, st.cameraVersion, reason: 'the camera reached the world');
    expect(s.model.viewportWidth, 320);
    expect(pixels.length, 320 * 200 * 4);
    expect(pixels.any((b) => b != 0), isTrue, reason: 'not black');
    // Distinct colours: the floor, the crates, the sky — not one clear colour.
    final colours = <int>{};
    for (var i = 0; i < pixels.length; i += 4) {
      colours.add((pixels[i] << 16) | (pixels[i + 1] << 8) | pixels[i + 2]);
    }
    expect(colours.length, greaterThan(20));
    final stats = s.model.viewportStats();
    expect(stats, isNotNull);
    expect(stats!.instances, greaterThan(0), reason: 'the meshes drew');
    expect(stats.assetErrors, 0, reason: 'the preview world brings its own asset store');

    // Save Frame: cancelled = nothing rendered or written.
    expect(await s.model.saveFrame(), isNull);
    expect(st.frames, 1);
    // With the grid and the outline on, the debug lines go in and the
    // notice says the frame is not a clean one.
    s.model.toggleGrid();
    tab.select(floor);
    expect(s.model.debugLines(st).length, greaterThan(120));
    s.savePath = s.dir.path;
    final overlaid = await s.model.saveFrame();
    expect(overlaid, '${s.dir.path}/main-320x200.png', reason: '${s.model.notice}');
    expect(s.model.notice, contains('clean frame'));
    final (w, h, withGrid) = decodePng(File(overlaid!).readAsBytesSync());
    expect((w, h), (320, 200));
    expect(withGrid, isNot(pixels), reason: 'the grid is in it');
    // Clean again: the PNG holds the very pixels of the first capture
    // (the renderer is deterministic) and the notice points at `fynkel render`.
    s.model.toggleGrid();
    tab.clearSelection();
    final clean = await s.model.saveFrame();
    expect(clean, isNotNull, reason: '${s.model.notice}');
    expect(s.model.notice, contains('fynkel render --size 320x200'));
    expect(s.model.notice, isNot(contains('clean frame')));
    final (_, _, back) = decodePng(File(clean!).readAsBytesSync());
    expect(back, pixels);
    // FYNKEL_SCENE_FRAME_OUT=<file.png> keeps the clean frame for a pixel
    // comparison with `fynkel render --scene main --size 320x200`.
    final keep = Platform.environment['FYNKEL_SCENE_FRAME_OUT'];
    if (keep != null && keep.isNotEmpty) File(clean).copySync(keep);
  });
}
