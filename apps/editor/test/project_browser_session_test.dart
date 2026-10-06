// The Project browser over the real `SessionProject` on a copy of the
// fixture project: every operation leaves the project validating clean,
// an id survives a rename, a referenced asset is refused and then forced,
// import settings reach the sidecar, and undo/redo run through the
// session's own history. New Behaviour registers its type with the session
// (what `fynkel codegen` writes is stood in for).
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/project_browser/project_browser_model.dart';

import 'project_test.dart' show copyFixture;

const swatchId = 'c0000000-0000-4000-8000-000000000002';
const rustedId = 'c0000000-0000-4000-8000-000000000020';
const sparkId = 'c0000000-0000-4000-8000-000000000041';

void main() {
  late Directory dir;
  late SessionProject project;
  late ProjectBrowserModel model;

  setUp(() async {
    dir = copyFixture('browser');
    project = await SessionProject.open(dir.path, watch: false);
    model = ProjectBrowserModel(project: project);
  });

  tearDown(() async {
    model.dispose();
    await project.close();
    dir.deleteSync(recursive: true);
  });

  Future<void> expectClean() async {
    await project.revalidate();
    expect(project.validationError, isNull);
    expect(project.errorCount, 0, reason: project.diagnostics.map((d) => d.message).join('\n'));
  }

  test('folders and previews come from the real asset database', () {
    expect(model.visibleFolders, ['audio', 'materials', 'models', 'prefabs', 'rigs', 'scenes', 'textures']);
    expect(model.visibleAssets, isEmpty);
    model.setView(BrowserView.list);
    expect(model.visibleAssets.length, project.assetCount);
    final swatch = model.entry(swatchId)!;
    expect(model.preview(swatch), isA<TexturePreview>());
    expect(model.detail(swatchId)!.importer, 'fynkel.image');
    expect(model.detail(swatchId)!.dependents.single['path'], 'materials/rusted.material.json');
    final rusted = model.entry(rustedId)!;
    expect(model.preview(rusted), isA<SwatchPreview>());
    expect(model.detail(rustedId)!.dependents.length, greaterThanOrEqualTo(3));
    expect(model.dragPayload(model.entry(sparkId)!)!.kind, 'prefab');
    final rig = model.entries.firstWhere((e) => e.path == 'models/rig.glb');
    expect(model.dragPayload(rig)!.sub, 'prefab');
  });

  test('rename and move keep the id, the sidecar travels, validation stays clean; undo brings it back', () async {
    final before = project.assetsRevision;
    expect(model.rename(swatchId, 'swatch2.png'), isTrue);
    expect(project.assetsRevision, greaterThan(before), reason: 'AssetsChanged reached the project synchronously');
    final moved = model.entry(swatchId)!;
    expect(moved.path, 'textures/swatch2.png');
    expect(File('${dir.path}/assets/textures/swatch2.png').existsSync(), isTrue);
    expect(File('${dir.path}/assets/textures/swatch2.png.meta.json').existsSync(), isTrue);
    expect(File('${dir.path}/assets/textures/swatch.png').existsSync(), isFalse);
    final meta = jsonDecode(File('${dir.path}/assets/textures/swatch2.png.meta.json').readAsStringSync()) as Map;
    expect(meta['id'], swatchId);
    // The material still names the id, so nothing dangles.
    await expectClean();
    expect(model.detail(swatchId)!.dependents.single['path'], 'materials/rusted.material.json');

    // Move it into a new folder through the browser (the folder is made on the way).
    expect(model.move([swatchId], 'textures/ui'), 1);
    expect(model.entry(swatchId)!.path, 'textures/ui/swatch2.png');
    expect(File('${dir.path}/assets/textures/ui/swatch2.png.meta.json').existsSync(), isTrue);
    await expectClean();

    // A document rename keeps the suffix and the id; scenes referencing it still validate.
    expect(model.rename(sparkId, 'sparkle'), isTrue);
    expect(model.entry(sparkId)!.path, 'prefabs/sparkle.prefab.json');
    await expectClean();
    expect(model.history.undo.last, 'Rename spark.prefab.json → sparkle.prefab.json');
    expect(model.history.undo.length, 3);

    model.undo();
    expect(model.entry(sparkId)!.path, 'prefabs/spark.prefab.json');
    model.undo();
    expect(model.entry(swatchId)!.path, 'textures/swatch2.png');
    model.undo();
    expect(model.entry(swatchId)!.path, 'textures/swatch.png');
    expect(model.history.canUndo, isFalse);
    expect(model.history.canRedo, isTrue);
    model.redo();
    expect(model.entry(swatchId)!.path, 'textures/swatch2.png');
    await expectClean();
  });

  test('a referenced asset is refused with its dependents; forced, it is trashed and the reference dangles; undo restores it', () async {
    expect(model.delete(ids: [swatchId]), isFalse);
    final refusal = model.pendingDelete!;
    expect(refusal.ids, [swatchId]);
    expect(refusal.dependents.map((d) => model.assetPath(d.document!)), ['materials/rusted.material.json']);
    expect(refusal.dependents.single.pointer, isNotNull);
    expect(File('${dir.path}/assets/textures/swatch.png').existsSync(), isTrue);

    expect(model.delete(ids: [swatchId], force: true), isTrue);
    expect(model.entry(swatchId), isNull);
    expect(File('${dir.path}/assets/textures/swatch.png').existsSync(), isFalse);
    final trashed = Directory('${dir.path}/.fynkel/trash')
        .listSync(recursive: true)
        .whereType<File>()
        .map((f) => f.uri.pathSegments.last);
    expect(trashed, containsAll(['swatch.png', 'swatch.png.meta.json']));
    await project.revalidate();
    expect(
      project.diagnostics.where((d) => d.severity == Severity.error),
      isNotEmpty,
      reason: 'the material now dangles',
    );

    model.undo();
    expect(model.entry(swatchId), isNotNull);
    expect(File('${dir.path}/assets/textures/swatch.png').existsSync(), isTrue);
    await expectClean();

    // An unreferenced asset goes straight away and comes back the same way.
    final coin = model.entries.firstWhere((e) => e.path == 'prefabs/coin.prefab.json');
    expect(model.delete(ids: [coin.id]), isTrue);
    expect(model.entry(coin.id), isNull);
    await expectClean();
    model.undo();
    expect(model.entry(coin.id)!.path, 'prefabs/coin.prefab.json');
    await expectClean();
  });

  test('new documents are validating documents with fresh ids; a taken name is refused', () async {
    model.setFolder('scenes');
    final r = model.newDocument('scene')!;
    expect(model.assetPath(r.path!), 'scenes/New Scene.scene.json');
    expect(r.id, isNotNull);
    expect(model.entry(r.id!)!.document, 'scene');
    final json = jsonDecode(File('${dir.path}/assets/scenes/New Scene.scene.json').readAsStringSync()) as Map;
    expect(json['id'], r.id);
    expect(json['name'], 'New Scene');
    final p = model.newDocument('prefab', inFolder: 'prefabs')!;
    expect(model.assetPath(p.path!), 'prefabs/New Prefab.prefab.json');
    final m = model.newDocument('material', inFolder: 'materials')!;
    expect(model.assetPath(m.path!), 'materials/New Material.material.json');
    expect(model.detail(m.id!)!.material!['shader'], 'fynkel.pbr@1');
    expect(model.preview(model.entry(m.id!)!), isA<SwatchPreview>());
    await expectClean();
    // Rename onto a taken name is refused and nothing changes.
    expect(model.rename(r.id!, 'main'), isFalse);
    expect(model.lastError, isNotNull);
    expect(model.entry(r.id!)!.path, 'scenes/New Scene.scene.json');
    expect(model.history.undo.length, 3);
  });

  test(
    'import settings reach the sidecar through the session, reimport runs the importer, undo takes them back',
    () async {
      final sidecar = File('${dir.path}/assets/textures/swatch.png.meta.json');
      expect(model.setImportSettings(swatchId, {'colorSpace': 'linear'}), isNotNull);
      var meta = jsonDecode(sidecar.readAsStringSync()) as Map;
      expect(meta['settings'], {'colorSpace': 'linear'});
      expect(meta['id'], swatchId);
      expect(model.detail(swatchId)!.settings, {'colorSpace': 'linear'});
      final r = model.reimport(swatchId)!;
      expect(r.ok, isTrue, reason: r.diagnostics.map((d) => d.message).join('\n'));
      expect(r.json['sidecarWritten'], isFalse, reason: 'an image has no sub-assets to record');
      expect(r.json['summary'], isA<Map>());
      expect(model.dragPayload(model.entry(swatchId)!)!.kind, 'texture');
      // A wrong setting is the importer's refusal, not a crash.
      expect(model.setImportSettings(swatchId, {'colorSpace': 'sepia'}), isNotNull);
      final bad = model.reimport(swatchId)!;
      expect(bad.ok, isFalse);
      expect(bad.diagnostics, isNotEmpty);
      model.undo();
      model.undo();
      meta = jsonDecode(sidecar.readAsStringSync()) as Map;
      expect(meta['settings'], {'compression': 'none'}, reason: 'the fixture swatch is kept uncompressed');
      await expectClean();
    },
  );

  test('folders: new, rename with assets, delete when empty; every step validates clean', () async {
    expect(model.newFolder(inFolder: 'textures'), isTrue);
    expect(Directory('${dir.path}/assets/textures/New Folder').existsSync(), isTrue);
    expect(model.folders, contains('textures/New Folder'));
    expect(model.renameFolder('textures/New Folder', 'ui'), isTrue);
    expect(Directory('${dir.path}/assets/textures/ui').existsSync(), isTrue);
    expect(model.renameFolder('audio', 'sounds'), isTrue);
    expect(model.entries.map((e) => e.path), contains('sounds/tone.wav'));
    expect(Directory('${dir.path}/assets/audio').existsSync(), isFalse);
    expect(File('${dir.path}/assets/sounds/tone.wav.meta.json').existsSync(), isTrue);
    await expectClean();
    expect(model.deleteFolder('sounds'), isFalse);
    expect(model.deleteFolder('textures/ui'), isTrue);
    expect(Directory('${dir.path}/assets/textures/ui').existsSync(), isFalse);
    model.undo();
    expect(model.entries.map((e) => e.path), contains('audio/tone.wav'));
    await expectClean();
  });

  test('New Behaviour: the session registers the generated type, Add Component sees its schema', () async {
    File('${dir.path}/pubspec.yaml').writeAsStringSync('name: fixture\n');
    Directory('${dir.path}/lib/game').createSync(recursive: true);
    expect(model.canNewBehaviour, isTrue);
    expect(project.typeSchemas().map((t) => t.type), isNot(contains('game.Hover')));

    final b = model.writeNewBehaviour('Hover')!;
    expect(b.rel, 'game/hover.dart');
    final problem = await model.generateBehaviour(b, () async {
      // What `fynkel codegen --project` leaves behind for it.
      final spinner = File('${dir.path}/schemas/behaviours/game.Spinner.schema.json').readAsStringSync();
      File('${dir.path}/schemas/behaviours/game.Hover.schema.json')
          .writeAsStringSync(spinner.replaceAll('game.Spinner', 'game.Hover'));
      File('${dir.path}/lib/generated/behaviours.g.json')
        ..createSync(recursive: true)
        ..writeAsStringSync(
          jsonEncode({
            'package': 'fixture',
            'behaviours': [
              {'id': 'game.Hover', 'version': 1, 'class': 'Hover', 'library': 'package:fixture/game/hover.dart'},
            ],
          }),
        );
      return null;
    });
    expect(problem, isNull, reason: model.lastError);
    expect(project.typeSchemas().map((t) => t.type), contains('game.Hover'));
    expect(model.scriptKind('game/hover.dart'), 'behaviour');
    final registry =
        (jsonDecode(File('${dir.path}/fynkel.project.json').readAsStringSync()) as Map)['components'] as Map;
    expect(registry['game.Hover'], {'kind': 'behaviour', 'version': 1, 'description': 'Hover (lib/game/hover.dart).'});
    expect(model.history.undoLabel, 'Register Behaviour game.Hover');
    await expectClean();
  });
}
