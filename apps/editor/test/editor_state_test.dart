import 'dart:convert';
import 'dart:io';

import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/scene_view/editor_state.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';

void main() {
  late Directory dir;
  setUp(() => dir = Directory.systemTemp.createTempSync('fynkel_editor_state'));
  tearDown(() => dir.deleteSync(recursive: true));

  test('a fresh project has defaults, is written to .fynkel/editor.json and read back', () {
    final s = EditorState.load(dir.path);
    expect(s.tool, SceneTool.move);
    expect(s.space, GizmoSpace.world);
    expect(s.showGrid, isTrue);
    expect(s.showOverlays, isTrue);
    expect(s.showStats, isFalse);
    expect(s.snap.enabled, isFalse);
    expect(EditorState.fileOf(dir.path).existsSync(), isFalse, reason: 'nothing written until something changes');

    s.tool = SceneTool.rotate;
    s.space = GizmoSpace.local;
    s.snap = s.snap.copyWith(enabled: true, move: 0.25);
    s.showStats = true;
    final scene = s.scene('doc-1');
    scene.camera.orbit(50, 20);
    scene.camera.distance = 7;
    scene.hidden.add('e1');
    scene.locked.add('e2');
    s.changed();

    final f = EditorState.fileOf(dir.path);
    expect(f.existsSync(), isTrue);
    final json = jsonDecode(f.readAsStringSync()) as Map<String, Object?>;
    expect(json['formatVersion'], EditorState.formatVersion);
    expect(json['tool'], 'rotate');
    expect((json['scenes'] as Map).keys, ['doc-1']);

    final back = EditorState.load(dir.path);
    expect(back.tool, SceneTool.rotate);
    expect(back.space, GizmoSpace.local);
    expect(back.snap.enabled, isTrue);
    expect(back.snap.move, 0.25);
    expect(back.showStats, isTrue);
    expect(back.hasScene('doc-1'), isTrue);
    expect(back.hasScene('other'), isFalse);
    final bs = back.scene('doc-1');
    expect(bs.camera.distance, 7);
    expect(bs.camera.yaw, closeTo(scene.camera.yaw, 1e-12));
    expect(bs.hidden, {'e1'});
    expect(bs.locked, {'e2'});
    // The file is the editor's, never the scene: no entity data in it.
    expect(f.readAsStringSync(), isNot(contains('components')));
  });

  test('scheduleSave coalesces camera moves into one write; flush writes at once', () {
    fakeAsync((async) {
      final s = EditorState.load(dir.path);
      final f = EditorState.fileOf(dir.path);
      s.scene('d').camera.distance = 3;
      s.scheduleSave();
      s.scene('d').camera.distance = 4;
      s.scheduleSave();
      expect(s.savePending, isTrue);
      expect(f.existsSync(), isFalse);
      async.elapse(const Duration(milliseconds: 500));
      expect(s.savePending, isFalse);
      expect(EditorState.load(dir.path).scene('d').camera.distance, 4);

      s.scene('d').camera.distance = 5;
      s.scheduleSave();
      s.flush();
      expect(s.savePending, isFalse);
      expect(EditorState.load(dir.path).scene('d').camera.distance, 5);
      s.dispose();
    });
  });

  test('a file from a newer editor is read but never overwritten; garbage starts fresh', () {
    final f = EditorState.fileOf(dir.path)..createSync(recursive: true);
    f.writeAsStringSync(jsonEncode({'formatVersion': 99, 'tool': 'scale', 'future': true}));
    final newer = EditorState.load(dir.path);
    expect(newer.tool, SceneTool.scale);
    expect(newer.readOnlyReason, contains('newer'));
    expect(newer.save(), isNull);
    expect(jsonDecode(f.readAsStringSync())['formatVersion'], 99, reason: 'untouched');

    f.writeAsStringSync('{not json');
    final bad = EditorState.load(dir.path);
    expect(bad.loadProblem, isNotNull);
    expect(bad.tool, SceneTool.move);
    bad.changed();
    expect(jsonDecode(f.readAsStringSync())['formatVersion'], EditorState.formatVersion, reason: 'replaced on save');
  });

  test('inMemory never touches the disk', () {
    final s = EditorState.inMemory();
    s.scene('x').hidden.add('a');
    s.changed();
    expect(s.save(), isNull);
    expect(s.file, isNull);
  });

  test('SceneEditorState json survives an unknown camera', () {
    final st = SceneEditorState.fromJson({
      'camera': 'junk',
      'hidden': ['a', 1],
      'locked': null,
    });
    expect(st.camera.distance, EditorCamera.initial().distance);
    expect(st.hidden, {'a'});
    expect(st.locked, isEmpty);
  });
}
