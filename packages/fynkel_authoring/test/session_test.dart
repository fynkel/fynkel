// A complete edit session over the authoring ABI on a private copy of the
// fixture project (blueprint §19.2e item 1 gate): open, every §11.1
// command, undo back to the original bytes, save, the stale-revision
// conflict, a material's base_color, duplicateObject of the rig — through
// the native runtime package:fynkel's hook built with `--features authoring`.

import 'dart:convert';
import 'dart:io';
import 'dart:math' show tan;

import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:test/test.dart';

const mainScene = 'assets/scenes/main.scene.json';
const mainSceneId = 'c0000000-0000-4000-8000-000000000030';
const rigScene = 'assets/scenes/rig.scene.json';
const rusted = 'assets/materials/rusted.material.json';
const rustedId = 'c0000000-0000-4000-8000-000000000020';
const coinPrefab = 'c0000000-0000-4000-8000-000000000042';
const cratePrefabEntity = 'c0000000-0000-4000-8000-0000000000b1';
const cratePrefabRenderer = 'c0000000-0000-4000-8000-0000000000c3';
const camera = 'c0000000-0000-4000-8000-0000000000e0';
const robot = 'c0000000-0000-4000-8000-0000000000e1';
const robotRenderer = 'c0000000-0000-4000-8000-0000000000c1';
const robotSpinner = 'c0000000-0000-4000-8000-0000000000c2';
const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const floor = 'c0000000-0000-4000-8000-0000000000e4';
const crateB = 'c0000000-0000-4000-8000-0000000000e5';
const sun = 'c0000000-0000-4000-8000-0000000000e3';
const floorTransform = 'c0000000-0000-4000-8000-0000000000d0';
const cratePrefab = 'c0000000-0000-4000-8000-000000000040';
const rig = 'c0000000-0000-4000-8000-0000000004e2';
// Fresh ids for what the session creates.
const lamp = '11111111-0000-4000-8000-000000000001';
const lampTransform = '11111111-0000-4000-8000-000000000002';
const lampLight = '11111111-0000-4000-8000-000000000003';
const lampRotator = '11111111-0000-4000-8000-000000000004';
const crateC = '11111111-0000-4000-8000-000000000005';
const coin = '11111111-0000-4000-8000-000000000006';
const coinTransform = '11111111-0000-4000-8000-000000000007';

final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute;

/// A private copy of the fixture project (the cooked cache left out).
Directory copyFixture(String tag) {
  final dir = Directory.systemTemp.createTempSync('fynkel_authoring_$tag');
  void copy(Directory from, Directory to) {
    to.createSync(recursive: true);
    for (final e in from.listSync()) {
      final name = e.uri.pathSegments.where((s) => s.isNotEmpty).last;
      if (name.startsWith('.')) continue;
      if (e is Directory) {
        copy(e, Directory('${to.path}/$name'));
      } else if (e is File) {
        e.copySync('${to.path}/$name');
      }
    }
  }

  copy(fixture, dir);
  // The session reports canonical paths (`/private/var/...` on macOS).
  return Directory(dir.resolveSymbolicLinksSync());
}

Map<String, Object?> transform(List<num> position) =>
    {'position': position, 'rotation': [0, 0, 0, 1], 'scale': [1, 1, 1]};

Map<String, Object?> entities(DocumentHandle doc) => doc.root()['entities'] as Map<String, Object?>;
Map<String, Object?> entity(DocumentHandle doc, String id) => entities(doc)[id] as Map<String, Object?>;
String nameOf(DocumentHandle doc, String id) => entity(doc, id)['name'] as String;

void main() {
  test('the loaded runtime speaks authoring ABI v3', () {
    expect(AuthoringSession.loadedAbiVersion, authoringAbiVersion);
    expect(authoringAbiVersion, 3);
  });

  test('a complete edit session on the main scene', () {
    final dir = copyFixture('session');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    final events = <AuthoringEvent>[];
    session.events.listen(events.add);

    // Open: the project, its asset database, the command list.
    final project = session.openProject(dir.path);
    expect(project.name, 'Fixture Game');
    expect(project.isReadOnly, isFalse);
    expect(project.assetCount, 17);
    expect(project.componentTypes, contains('fynkel.Transform'));
    expect(session.project, same(project));
    expect(session.commands(), Command.names);
    expect(events.single, isA<ProjectOpened>());

    final main = session.openDocument(mainScene);
    expect(main.state.kind, 'scene');
    expect(main.state.id, mainSceneId);
    expect(main.state.name, 'Main');
    expect(main.state.dirty, isFalse);
    expect(main.state.canUndo, isFalse);
    expect(main.state.relativePath, 'scenes/main.scene.json');
    expect(session.openDocument(mainSceneId), same(main), reason: 'idempotent by id');
    expect(session.documents.single, same(main));
    final original = File(main.state.path).readAsBytesSync();
    final rev0 = main.state.revision;
    expect(rev0, isNotEmpty);

    // One transaction per editor gesture, every §11.1 command once.
    Applied ok(List<Map<String, Object?>> commands) {
      final applied = main.apply(commands, baseRevision: main.state.revision);
      expect(applied.ok, isTrue, reason: applied.allDiagnostics.join('\n'));
      expect(applied.state, isNotNull);
      return applied;
    }

    var a = ok([
      Command.createObject(id: lamp, name: 'Lamp', components: [
        componentSpec('fynkel.Transform', id: lampTransform, data: transform([0, 3, 0])),
        componentSpec('fynkel.PointLight', id: lampLight, data: {'color': [1, 0.9, 0.8], 'intensity': 2, 'range': 5, 'culling_mask': 0xffffffff}),
      ]),
    ]);
    expect(a.baseRevision, rev0);
    expect(a.revision, isNot(rev0));
    expect(a.affectedIds, contains(lamp));
    expect(main.state.dirty, isTrue);
    expect(main.state.history, 1);

    ok([
      Command.addComponent(
          lamp, componentSpec('fynkel.Rotator', id: lampRotator, data: {'axis': [0, 1, 0], 'speed': 1, 'space': 'local'})),
    ]);
    ok([
      Command.setField(lamp, lampLight, 'intensity', 3),
      Command.assignAsset(robot, robotRenderer, 'material', assetRef(rustedId)),
    ]);
    // A behaviour's fields are optional: set one, unset another. A field a
    // component schema does not know is a validation error and rolls back.
    ok([Command.setField(robot, robotSpinner, 'spinning', false)]);
    ok([Command.unsetField(robot, robotSpinner, 'axis')]);
    final unknownField = main.apply([Command.setField(lamp, lampRotator, 'note', 1)]);
    expect(unknownField.ok, isFalse);
    expect(unknownField.diagnostics.first.code, 'FYN-E0005');
    expect(main.state.history, 5, reason: 'nothing was committed');
    a = ok([Command.renameObject(robot, 'Bot'), Command.reparentObject(lamp, parent: robot, index: 0)]);
    // The Hierarchy re-reads the affected entities only: the robot (renamed,
    // now the lamp's parent) and the lamp; the roots no longer list the lamp.
    final patch = main.entities(a.affectedIds);
    expect(patch.entities.keys, containsAll([robot, lamp]));
    expect(patch.entities[robot]!['name'], 'Bot');
    expect(patch.entities[robot]!['children'], [lamp]);
    expect(patch.entities[lamp]!['parent'], robot);
    expect(patch.state.revision, a.revision);
    expect(patch.roots, isNot(contains(lamp)));
    expect(patch.roots, contains(robot));
    expect(main.roots(), patch.roots);
    expect(main.entities(['nobody']).entities, {'nobody': null});
    a = ok([Command.duplicateObject(crateB, newId: crateC, name: 'Crate C')]);
    expect(a.affectedIds, contains(crateC));
    ok([
      Command.instantiatePrefab(
        id: coin,
        name: 'Coin',
        prefab: assetRef(coinPrefab),
        components: [componentSpec('fynkel.Transform', id: coinTransform, data: transform([0, 1, -2]))],
      ),
    ]);
    ok([
      Command.setPrefabOverride(crateA,
          prefabEntity: cratePrefabEntity, component: cratePrefabRenderer, field: 'visible', value: false),
    ]);
    var overrides = (entity(main, crateA)['prefab'] as Map)['overrides'] as List;
    expect(overrides, hasLength(2));
    ok([Command.revertPrefabOverride(crateA, index: 1)]);
    overrides = (entity(main, crateA)['prefab'] as Map)['overrides'] as List;
    expect(overrides, hasLength(1));
    ok([Command.removeComponent(robot, robotSpinner), Command.deleteObject(floor)]);
    ok([Command.setDocumentField('/name', 'Main (edited)')]);
    // createMaterial and saveDocument write files, after the other commands.
    ok([Command.createMaterial(name: 'Gold', path: 'assets/materials/gold.material.json'), Command.saveDocument()]);
    final gold = File('${dir.path}/assets/materials/gold.material.json');
    expect(gold.existsSync(), isTrue, reason: 'written into the project, not the cwd');
    expect(main.state.dirty, isFalse, reason: 'saveDocument saved');
    expect(session.refreshProject().assetCount, 18, reason: 'the new material is in the database');
    expect(session.assets(kind: 'document', document: 'material').map((a) => a['path']),
        containsAll(['materials/rusted.material.json', 'materials/gold.material.json']));
    const transactions = 13;
    expect(main.state.history, transactions);
    expect(main.state.canUndo, isTrue);
    expect(main.state.canRedo, isFalse);

    // What the document says now.
    final tree = entities(main);
    expect(nameOf(main, robot), 'Bot');
    expect(entity(main, lamp)['parent'], robot);
    expect((entity(main, robot)['children'] as List).first, lamp);
    expect((entity(main, robot)['components'] as Map).containsKey(robotSpinner), isFalse);
    expect(tree.containsKey(floor), isFalse);
    expect(nameOf(main, crateC), 'Crate C');
    expect((entity(main, coin)['prefab'] as Map)['asset'], {'asset': coinPrefab});
    final lampData = ((entity(main, lamp)['components'] as Map)[lampLight] as Map)['data'] as Map;
    expect(lampData['intensity'], 3);
    expect(main.state.name, 'Main (edited)');
    final inspect = main.inspect();
    expect(inspect['kind'], 'scene');
    expect(inspect['entityCount'], 8, reason: '6 - Floor + Lamp + Crate C + Coin');
    expect(inspect['diagnostics'], isEmpty);
    expect(main.validate(), isEmpty);
    expect(session.dependents(rustedId).map((d) => d['document']), contains(mainSceneId));

    // A stale base revision is refused and nothing is touched.
    final revNow = main.state.revision;
    final stale = main.apply([Command.renameObject(camera, 'Eye')], baseRevision: rev0);
    expect(stale.ok, isFalse);
    expect(stale.isConflict, isTrue);
    expect(stale.diagnostics.first.code, 'FYN-E0001');
    expect(main.state.revision, revNow);
    expect(nameOf(main, camera), 'Camera');
    // A malformed command is refused too, per command.
    final bad = main.applyRequest([
      {'command': 'warp'}
    ]);
    expect(bad.ok, isFalse);
    expect(bad.allDiagnostics.map((d) => d.code), contains('FYN-E0004'));
    // A request that is neither an array nor an object is malformed too.
    expect(main.applyRequest('{').allDiagnostics.map((d) => d.code), contains('FYN-E0004'));

    // Undo everything: the bytes are the original's again.
    for (var i = 0; i < transactions; i++) {
      final t = main.undo();
      expect(t.state.history, transactions - 1 - i, reason: "history is what is left to undo");
    }
    expect(main.state.revision, rev0);
    expect(main.state.canUndo, isFalse);
    expect(main.state.canRedo, isTrue);
    expect(() => main.undo(), throwsA(isA<AuthoringException>().having((e) => e.isNothingToReplay, 'E0009', isTrue)));
    expect(main.save(), main.state.path);
    expect(File(main.state.path).readAsBytesSync(), original, reason: 'undo is byte-exact');
    expect(main.state.dirty, isFalse);

    // Redo everything and save: the file carries the edits, the project validates.
    for (var i = 0; i < transactions; i++) {
      main.redo();
    }
    expect(main.state.revision, revNow);
    main.save();
    final saved = jsonDecode(File('${dir.path}/$mainScene').readAsStringSync()) as Map<String, Object?>;
    expect(saved['name'], 'Main (edited)');
    expect((saved['entities'] as Map).containsKey(floor), isFalse);
    expect((saved['entities'] as Map).containsKey(coin), isTrue);
    final report = session.validateProject();
    expect(report.ok, isTrue, reason: report.diagnostics.join('\n'));
    expect(report.documents, greaterThanOrEqualTo(11));

    // Save-as keeps the id, so the database reports the duplicate.
    final copyPath = '${dir.path}/assets/scenes/main_copy.scene.json';
    expect(main.save(path: copyPath), copyPath);
    expect(main.state.path, copyPath);
    expect(main.state.relativePath, 'scenes/main_copy.scene.json');
    expect(session.rescan().diagnostics.map((d) => d.code), contains('FYN-A0003'));

    // Reload drops the history; the document is the file's (the copy's now).
    main.reload();
    expect(main.state.history, 0);
    expect(main.state.canUndo, isFalse);
    expect(main.state.name, 'Main (edited)');
    File(copyPath).deleteSync();
    expect(session.rescan([copyPath]).assetCount, 18, reason: 'the deleted copy left the database');

    // Events: one per change, in order, synchronously.
    final changes = events.whereType<DocumentChanged>().toList();
    expect(changes.where((c) => c.kind == DocumentChangeKind.applied), hasLength(transactions));
    expect(changes.where((c) => c.kind == DocumentChangeKind.undone), hasLength(transactions));
    expect(changes.where((c) => c.kind == DocumentChangeKind.redone), hasLength(transactions));
    expect(changes.where((c) => c.kind == DocumentChangeKind.saved), hasLength(3));
    expect(changes.where((c) => c.kind == DocumentChangeKind.reloaded), hasLength(1));
    expect(changes.first.applied!.affectedIds, contains(lamp));
    expect(events.whereType<AssetsChanged>(), hasLength(2));

    // Close: the handle is dead, the session goes on.
    main.close();
    expect(main.isClosed, isTrue);
    expect(session.documents, isEmpty);
    expect(() => main.apply([]), throwsStateError);
    expect(events.whereType<DocumentClosed>().single.handle, main.handle);
    final again = session.openDocument(mainScene);
    expect(again.handle, isNot(main.handle), reason: 'handles are never reused');
    session.closeProject();
    expect(session.project, isNull);
    expect(again.isClosed, isTrue);
    expect(events.last, isA<ProjectClosed>());
    expect(
      () => session.query('project'),
      throwsA(isA<AuthoringException>()
          .having((e) => e.status, 'status', AuthoringStatus.invalidArgument)
          .having((e) => e.code, 'code', 'FYN-T0001')),
    );
    session.dispose();
    expect(session.isDisposed, isTrue);
    expect(() => session.openProject(dir.path), throwsStateError);
    session.dispose(); // idempotent
  });

  test('a material edits through document fields', () {
    final dir = copyFixture('material');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    session.openProject(dir.path);
    final material = session.openDocument(rustedId);
    expect(material.state.kind, 'material');
    expect(material.state.relativePath, 'materials/rusted.material.json');
    var inspect = material.inspect();
    expect(inspect['shader'], 'fynkel.pbr@1');
    expect((inspect['fields'] as Map)['base_color'], [1.0, 0.7, 0.5, 1.0]);

    final applied = material.apply([
      Command.setDocumentField('/base_color', [1, 0, 0, 1]),
      Command.unsetDocumentField('/metallic'),
    ], baseRevision: material.state.revision);
    expect(applied.ok, isTrue, reason: applied.allDiagnostics.join('\n'));
    inspect = material.inspect();
    expect((inspect['fields'] as Map)['base_color'], [1, 0, 0, 1]);
    expect((inspect['fields'] as Map).containsKey('metallic'), isFalse);
    expect(material.root()['base_color'], [1, 0, 0, 1]);

    // A value the schema rejects rolls back; an entity command is refused.
    final rejected = material.apply([Command.setDocumentField('/base_color', 'red')]);
    expect(rejected.ok, isFalse);
    expect(rejected.allDiagnostics, isNotEmpty);
    expect(material.root()['base_color'], [1, 0, 0, 1]);
    final refused = material.apply([Command.renameObject(rustedId, 'x')]);
    expect(refused.ok, isFalse);
    expect(refused.allDiagnostics.map((d) => d.code), contains('FYN-E0007'));

    material.undo();
    expect((material.inspect()['fields'] as Map)['metallic'], 0.25);
    expect(material.state.canUndo, isFalse);
    expect(material.state.dirty, isFalse);
  });

  test('duplicateObject copies the rig subtree with fresh ids in one step', () {
    final dir = copyFixture('rig');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    session.openProject(dir.path);
    final scene = session.openDocument(rigScene);
    final before = scene.inspect()['entityCount'] as int;
    final rigNames = _subtreeNames(scene, rig);
    expect(rigNames, ['Rig', 'Body', 'hips', 'spine', 'head', 'Prop']);

    final applied = scene.apply([Command.duplicateObject(rig, name: 'Rig copy')], baseRevision: scene.state.revision);
    expect(applied.ok, isTrue, reason: applied.allDiagnostics.join('\n'));
    expect(applied.mutations.single.command, 'duplicateObject');
    final fresh = applied.mutations.single.affectedIds.where((id) => !id.startsWith('c0000000-')).toList();
    expect(fresh.length, greaterThanOrEqualTo(rigNames.length), reason: 'every copied entity is a fresh id');
    final copyId = entities(scene).entries.firstWhere((e) => (e.value as Map)['name'] == 'Rig copy').key;
    expect(fresh, contains(copyId));
    expect(_subtreeNames(scene, copyId), ['Rig copy', 'Body', 'hips', 'spine', 'head', 'Prop']);
    expect(scene.inspect()['entityCount'], before + rigNames.length);
    expect(scene.validate(), isEmpty);

    scene.undo();
    expect(scene.inspect()['entityCount'], before);
    expect(entities(scene).containsKey(copyId), isFalse);
  });

  test('asset operations: new, rename with the sidecar, delete, settings, reimport, undo — item 5', () {
    final dir = copyFixture('ops');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    final events = <AuthoringEvent>[];
    session.events.listen(events.add);
    session.openProject(dir.path);
    final before = session.project!.assetCount;
    expect(session.operations(), containsAll(['newDocument', 'renameAsset', 'deleteAsset', 'setImportSettings', 'reimport']));
    expect(session.assetHistory().canUndo, isFalse);

    // A new material: written, indexed, on the history; the event carries it.
    final created = session.newDocument('material', 'materials/gold');
    expect(created.ok, isTrue);
    expect(created.label, 'New material gold.material.json');
    expect(created.entry!['name'], 'gold');
    expect(created.assetCount, before + 1);
    expect(created.history.canUndo, isTrue);
    expect(File('${dir.path}/assets/materials/gold.material.json').existsSync(), isTrue);
    final goldId = created.id!;
    final changed = events.whereType<AssetsChanged>().last;
    expect(changed.operation?.op, 'newDocument');
    expect(changed.report.assetCount, before + 1);
    final gold = session.assetDetail(goldId);
    expect(gold.document, 'material');
    expect(gold.material!['shader'], 'fynkel.pbr@1');
    expect(gold.size, greaterThan(0));
    expect(gold.dependents, isEmpty);

    // Rename a source: the sidecar follows and the id stays; the detail
    // shows the settings and sub-assets from the sidecar.
    const robotGlb = 'c0000000-0000-4000-8000-000000000001';
    final dependents = session.dependents(robotGlb).length;
    final renamed = session.renameAsset(robotGlb, 'models/bot.glb');
    expect(renamed.entry!['id'], robotGlb);
    expect(renamed.entry!['path'], 'models/bot.glb');
    expect(renamed.path, endsWith('/assets/models/bot.glb'));
    expect(File('${dir.path}/assets/models/bot.glb.meta.json').existsSync(), isTrue);
    expect(File('${dir.path}/assets/models/robot.glb.meta.json').existsSync(), isFalse);
    expect(session.dependents(robotGlb).length, dependents);
    final bot = session.assetDetail(robotGlb);
    expect(bot.isSource, isTrue);
    expect(bot.importer, 'fynkel.gltf');
    expect(bot.subAssets.keys, contains('prefab'));
    expect(bot.sidecarPath, endsWith('bot.glb.meta.json'));
    expect(session.validateProject().ok, isTrue, reason: 'every reference still resolves');

    // An open document moves with its file: a `moved` change, the state's
    // new path, and save writes there.
    final doc = session.openDocument(rusted);
    doc.apply([Command.setDocumentField('/double_sided', true)]);
    final moved = session.renameAsset(rustedId, 'materials/rust.material.json');
    expect(moved.label, 'Rename rusted.material.json → rust.material.json');
    expect(doc.state.path, endsWith('/assets/materials/rust.material.json'));
    expect(doc.state.dirty, isTrue);
    final movedEvent = events.whereType<DocumentChanged>().last;
    expect(movedEvent.kind, DocumentChangeKind.moved);
    expect(movedEvent.document, same(doc));
    doc.save();
    expect(File('${dir.path}/assets/materials/rust.material.json').readAsStringSync(), contains('"double_sided": true'));
    expect(File('${dir.path}/assets/materials/rusted.material.json').existsSync(), isFalse);

    // Refusals throw with the reason and touch nothing: a referenced
    // asset's delete lists every dependent in `related`.
    expect(
      () => session.deleteAsset(rustedId),
      throwsA(isA<AuthoringException>()
          .having((e) => e.status, 'status', AuthoringStatus.invalidArgument)
          .having((e) => e.code, 'code', 'FYN-T0003')
          .having((e) => e.diagnostics.single.related.length, 'related', greaterThanOrEqualTo(6))),
    );
    expect(() => session.renameAsset(robotGlb, 'models/bot.fbx'),
        throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0003')));
    expect(() => session.renameAsset('11111111-0000-4000-8000-000000000099', 'x.glb'),
        throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-A0004')));
    expect(() => session.operate('teleport'),
        throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0004')));
    expect(File('${dir.path}/assets/models/bot.glb').existsSync(), isTrue);

    // An unreferenced prefab goes to the trash and comes back on undo.
    final deleted = session.deleteAsset(coinPrefab);
    expect(deleted.entry, isNull);
    expect(deleted.json['trash'], endsWith('/.fynkel/trash/$coinPrefab/coin.prefab.json'));
    expect(File(deleted.json['trash'] as String).existsSync(), isTrue);
    expect(File('${dir.path}/assets/prefabs/coin.prefab.json').existsSync(), isFalse);
    expect(session.assets(document: 'prefab').map((e) => e['id']), isNot(contains(coinPrefab)));
    final undone = session.undoAsset();
    expect(undone.label, 'Undo Delete coin.prefab.json');
    expect(File('${dir.path}/assets/prefabs/coin.prefab.json').existsSync(), isTrue);
    expect(session.assets(document: 'prefab').map((e) => e['id']), contains(coinPrefab));
    // Forced: the dependents are warnings and validation catches the dangle.
    const swatch = 'c0000000-0000-4000-8000-000000000002';
    final forced = session.deleteAsset(swatch, force: true);
    expect(forced.diagnostics, isNotEmpty);
    expect(forced.diagnostics.every((d) => d.severity == Severity.warning), isTrue);
    expect(session.validateProject().ok, isFalse);
    session.undoAsset();
    expect(session.validateProject().ok, isTrue);

    // Import settings: written to the sidecar, read back, reimported with;
    // a bad value is the importer's refusal, not a throw.
    final settings = session.setImportSettings(swatch, {'colorSpace': 'linear'});
    expect(settings.label, 'Import settings of swatch.png');
    expect(session.assetDetail(swatch).settings, {'colorSpace': 'linear'});
    final reimported = session.reimport(swatch);
    expect(reimported.ok, isTrue);
    expect(reimported.json['sidecarWritten'], false);
    expect(((reimported.json['summary'] as Map)['textures'] as List).single['colorSpace'], 'linear');
    session.setImportSettings(swatch, {'colorSpace': 'purple'});
    final refused = session.reimport(swatch);
    expect(refused.ok, isFalse);
    expect(refused.diagnostics, isNotEmpty);
    session.undoAsset();
    expect(session.assetDetail(swatch).settings, {'colorSpace': 'linear'});
    expect(() => session.setImportSettings(rustedId, {}),
        throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0003')));

    // The history reads back and empties on undo; nothing to undo is E0009.
    var history = session.assetHistory();
    expect(history.undoLabel, 'Import settings of swatch.png');
    expect(history.undo.length, 4, reason: 'gold, bot, rust, settings');
    while (session.assetHistory().canUndo) {
      session.undoAsset();
    }
    expect(File('${dir.path}/assets/models/robot.glb.meta.json').existsSync(), isTrue);
    expect(File('${dir.path}/assets/materials/gold.material.json').existsSync(), isFalse);
    expect(doc.state.path, endsWith('/assets/materials/rusted.material.json'));
    expect(session.project!.assetCount, before, reason: 'the project description follows');
    expect(session.validateProject().ok, isTrue);
    expect(
      () => session.undoAsset(),
      throwsA(isA<AuthoringException>()
          .having((e) => e.status, 'status', AuthoringStatus.commandsRejected)
          .having((e) => e.code, 'code', 'FYN-E0009')),
    );
    history = session.assetHistory();
    expect(history.redo.length, 5, reason: 'the undone purple setting is still redoable');
    expect(history.redoLabel, 'New material gold.material.json');
    session.redoAsset();
    expect(File('${dir.path}/assets/materials/gold.material.json').existsSync(), isTrue);
    // Closing the project drops the history.
    session.closeProject();
    session.openProject(dir.path);
    expect(session.assetHistory().canUndo, isFalse);
    expect(session.assetHistory().canRedo, isFalse);
  });

  test('bad input is a diagnostic, never a crash', () {
    final dir = copyFixture('errors');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    // No project there.
    expect(
      () => session.openProject('${dir.path}/assets'),
      throwsA(isA<AuthoringException>().having((e) => e.status, 'status', AuthoringStatus.loadFailed)),
    );
    expect(session.project, isNull);
    session.openProject(dir.path);
    expect(
      () => session.openDocument('assets/scenes/nope.scene.json'),
      throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0006')),
    );
    expect(
      () => session.openDocument('assets/models/rig.glb'),
      throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-D0015')),
    );
    expect(
      () => session.queryValue('warp'),
      throwsA(isA<AuthoringException>()
          .having((e) => e.status, 'status', AuthoringStatus.invalidArgument)
          .having((e) => e.code, 'code', 'FYN-T0004')
          .having((e) => e.diagnostics.first.suggestion, 'suggestion', contains('commands'))),
    );
    expect(() => session.query('document', {'handle': 99}),
        throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0002')));
    expect(() => session.asset('11111111-0000-4000-8000-000000000099'),
        throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-A0004')));
    expect(session.asset(rustedId)['path'], 'materials/rusted.material.json');
    expect(session.schema('fynkel.PointLight')['title'], 'fynkel.PointLight');
    expect((session.schemas()['components'] as List), isNotEmpty);
    // Defaults of a native component; a behaviour has none here.
    final camera = session.componentDefaults('fynkel.Camera');
    expect(camera['projection'], 'perspective');
    expect(camera['near'], greaterThan(0));
    expect(session.componentDefaults('fynkel.Transform')['scale'], [1, 1, 1]);
    expect(() => session.componentDefaults('game.Spinner'),
        throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0003')));
    // The revision of a file on disk equals a freshly opened editor's.
    final onDisk = session.fileRevision(mainScene);
    final opened = session.openDocument(mainScene);
    expect(onDisk.revision, opened.state.revision);
    expect(onDisk.kind, 'scene');
    expect(onDisk.name, 'Main');
    expect(onDisk.path, opened.state.path);
    opened.apply([Command.renameObject(robot, 'Bot')]);
    expect(session.fileRevision(mainScene).revision, onDisk.revision, reason: 'unsaved edits do not move the file');
    expect(() => session.fileRevision('assets/scenes/nope.scene.json'),
        throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-D0900')));
    expect(session.metadata(), isNull, reason: 'the fixture has no behaviours.g.json');
    expect(session.migrationPreview().isEmpty, isTrue, reason: 'every fixture document is current');
    expect(session.migrationPreview().steps.map((s) => s.id), contains('scene/0-1/entities-keyed-by-id'));
    expect(session.isPoisoned, isFalse);
    final e = AuthoringException('x', AuthoringStatus.commandsRejected, [
      Diagnostic.fromJson({'code': 'FYN-E0001', 'severity': 'error', 'message': 'stale', 'document': 'a', 'line': 3}),
    ]);
    expect(e.isConflict, isTrue);
    expect(e.toString(), contains('FYN-E0001'));
    expect(e.diagnostics.single.toString(), 'error FYN-E0001: stale (a:3)');
  });

  test('the edit-preview world follows the document and never edits it — item 6', () {
    final dir = copyFixture('preview');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    final events = <AuthoringEvent>[];
    session.events.listen(events.add);
    session.openProject(dir.path);
    final scene = session.openDocument(mainScene);
    expect(scene.preview, isNull);

    // The first preview cooks the project into its cache and loads the world.
    final preview = scene.openPreview();
    expect(identical(scene.openPreview(), preview), isTrue, reason: 'idempotent');
    expect(scene.preview, same(preview));
    expect(preview.cook, isNotNull);
    expect(preview.cook!.ok, isTrue, reason: preview.cook!.diagnostics.join('; '));
    expect(File('${dir.path}/.fynkel/cache/index.json').existsSync(), isTrue);
    expect(preview.stats.entities, 7);
    expect(preview.stats.loads, 1);
    expect(preview.diagnostics, isEmpty);
    expect(preview.worldAddress, isNot(0));

    // The editor camera: 20 m up, 2 m aside, looking straight down (the
    // sun's icon sits right above the robot). Column-major: the rotation
    // −90° about X maps +Y to −Z and +Z to +Y.
    const editorCamera = PreviewCamera(
      world: [1, 0, 0, 0, 0, 0, -1, 0, 0, 1, 0, 0, 2, 20, 0, 1],
      near: 0.1,
      far: 100,
    );
    preview.setCamera(editorCamera);

    // Pick the robot through the pixel its centre projects to.
    final matrices = preview.worldMatrices([robot]);
    final m = matrices[robot]!;
    final centre = [m[12], m[13] + 0.5, m[14]];
    // View: camera at (2, 20, 0) looking down −Y; 60° vertical fov, 16:9.
    final vx = centre[0] - 2, vy = -centre[2], depth = 20 - centre[1];
    final f = 1 / tan(30 * 3.141592653589793 / 180);
    final ndc = [vx * f / (16 / 9) / depth, vy * f / depth];
    final pick = preview.pick(editorCamera, 16 / 9, ndc);
    expect(pick.hits, isNotEmpty);
    expect(pick.nearest!.id, robot, reason: pick.hits.map((h) => h.id).join(','));
    expect(pick.nearest!.mesh, isTrue);
    expect(preview.pickRay([centre[0], 5, centre[2]], [0, -1, 0]).nearest!.id, robot);
    // Hidden: gone from the picks and counted; shown again.
    expect(preview.setHidden([robot]), 1);
    expect(preview.pick(editorCamera, 16 / 9, ndc).hits.map((h) => h.id), isNot(contains(robot)));
    expect(preview.refreshStats().hidden, 1);
    expect(preview.setHidden([]), 0);
    // The whole scene under a marquee from above; outline and icon lines.
    final all = preview.pickRect(editorCamera, 16 / 9, [-1, -1], [1, 1]);
    expect(all, containsAll([robot, floor, crateA, crateB, camera, sun]));
    final outline = preview.lines(outline: [robot]);
    expect(outline.count, 12, reason: 'a box');
    expect(outline.data.length, 120);
    final icons = preview.lines(iconsAspect: 16 / 9);
    expect(icons.count, greaterThan(12), reason: 'the camera frustum and the sun arrows');

    // A transaction keeps the world in step, incrementally, and says so on
    // the record and as an event; the world dumps like a fresh load.
    events.clear();
    final before = preview.dump();
    final applied = scene.apply([Command.setField(floor, floorTransform, 'position', [0, -1.5, 0])]);
    expect(applied.ok, isTrue);
    expect(applied.previews.single.handle, scene.handle);
    expect(applied.previews.single.updated, 1);
    expect(applied.previews.single.reloaded, isFalse);
    expect(events.whereType<PreviewSynced>().single.document, same(scene));
    final after = preview.dump();
    expect(after, isNot(equals(before)));
    expect(preview.reload().entities, 7);
    expect(preview.dump(), after, reason: 'incremental == fresh');
    expect(preview.stats.loads, 2);
    // Undo brings it back.
    final undone = scene.undo();
    expect(undone.previews.single.updated, 1);
    expect(preview.dump(), before);

    // A prefab edited in another tab reaches the scene's preview.
    final prefab = session.openDocument(cratePrefab);
    events.clear();
    final edited = prefab.apply([Command.setField(cratePrefabEntity, cratePrefabRenderer, 'visible', false)]);
    expect(edited.ok, isTrue);
    expect(edited.previews.single.handle, scene.handle);
    expect(edited.previews.single.updated, greaterThan(0), reason: 'both crates');
    expect(preview.refreshStats().meshes, lessThan(preview.stats.entities));
    prefab.undo();

    // The gizmo's preview is not an edit.
    preview.setTransforms([
      const PreviewTransform(id: robot, position: [3, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1]),
    ]);
    expect(preview.worldMatrices([robot])[robot]![12], 3);
    expect(scene.refreshState().dirty, isFalse);
    preview.restoreTransforms([robot]);
    expect(preview.worldMatrices([robot])[robot]![12], m[12]);
    expect(preview.resync().isNoOp, isTrue);

    // Recook rebuilds the cache and reloads every preview over it; the
    // world pointer stays.
    final address = preview.worldAddress;
    final recook = preview.recook();
    expect(recook.ok, isTrue);
    expect(preview.cook, same(recook));
    expect(preview.stats.loads, 3);
    expect(preview.worldAddress, address);

    // Play's recook (ADR-0021 P5): the unsaved scene cooks as held, a
    // recovery copy is written, and the file is left alone.
    final file = File(scene.state.path);
    final onDiskText = file.readAsStringSync();
    expect(scene.apply([Command.setField(floor, floorTransform, 'position', [0, -2.5, 0])]).ok, isTrue);
    expect(preview.recook(live: true).ok, isTrue);
    expect(file.readAsStringSync(), onDiskText);
    final recovery = File('${dir.path}/.fynkel/recovery/$mainScene');
    expect(recovery.readAsStringSync(), contains('-2.5'));
    scene.undo();

    // Refusals: a material has no preview; a bad argument; an unknown id.
    final material = session.openDocument(rusted);
    expect(
      () => material.openPreview(),
      throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0003')),
    );
    expect(
      () => preview.pick(editorCamera, 16 / 9, [0]),
      throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0003')),
    );
    expect(
      () => preview.worldMatrices([lamp]),
      throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0003')),
    );

    // Closing the preview keeps the document; closing the document drops
    // its preview.
    preview.close();
    expect(preview.isClosed, isTrue);
    expect(preview.worldAddress, 0);
    expect(scene.preview, isNull);
    expect(() => preview.dump(), throwsStateError);
    final again = scene.openPreview();
    expect(again.cook, isNull, reason: 'the cache is open already');
    scene.close();
    expect(again.isClosed, isTrue);
    expect(again.worldAddress, 0);
  });


  test('prefab mode: the instance query, apply to prefab, unpack, imported hierarchies — item 7', () {
    final dir = copyFixture('prefab_mode');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    session.openProject(dir.path);
    expect(session.commands(), contains('unpackPrefab'));
    final main = session.openDocument(mainScene);
    final sceneBytes = File('${dir.path}/$mainScene').readAsStringSync();

    // Crate A's expansion: one derived entity with the override marked.
    var x = main.instance(crateA);
    expect(x.anchor, crateA);
    expect(x.expanded, isTrue);
    expect(x.isImported, isFalse);
    expect(x.asset, cratePrefab);
    expect(x.open, cratePrefab);
    expect(x.prefab!.name, 'Crate');
    expect(x.prefab!.kind, 'prefab');
    expect(x.prefab!.path, 'prefabs/crate.prefab.json');
    expect(x.prefab!.handle, isNull);
    expect(x.prefab!.readOnly, isNull);
    expect(x.diagnostics, isEmpty);
    expect(x.entities.map((e) => e.name), ['Crate']);
    final crateEntity = x.entities.single;
    expect(crateEntity.local, cratePrefabEntity);
    expect(crateEntity.parent, crateA);
    expect(crateEntity.added, isFalse);
    final renderer = crateEntity.components.values.singleWhere((c) => c.type == 'fynkel.MeshRenderer');
    expect(renderer.local, cratePrefabRenderer);
    expect(renderer.data['cast_shadows'], false);
    expect(renderer.overridden, ['cast_shadows']);
    expect(x.byLocal(cratePrefabEntity), same(crateEntity));
    expect(x.entity(crateEntity.id), same(crateEntity));
    final o = x.overrides.single;
    expect(o.index, 0);
    expect(o.op, 'setField');
    expect(o.entityId, crateEntity.id);
    expect(o.entityName, 'Crate');
    expect(o.componentId, renderer.id);
    expect(o.componentType, 'fynkel.MeshRenderer');
    expect(o.field, 'cast_shadows');
    expect(o.value, false);
    expect(o.rawOp, {
      'op': 'setField',
      'entity': cratePrefabEntity,
      'component': cratePrefabRenderer,
      'field': 'cast_shadows',
      'value': false,
    });
    expect(() => main.instance(camera), throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-E0008')));

    // Apply to prefab, the two-step way: the prefab document takes the
    // value, the instance drops the override; the prefab is dirty, not saved.
    final prefab = session.openDocument(cratePrefab);
    expect(main.instance(crateA).prefab!.handle, prefab.handle);
    final applied = prefab.apply([Command.setField(cratePrefabEntity, cratePrefabRenderer, 'cast_shadows', false)]);
    expect(applied.ok, isTrue, reason: '$applied');
    expect(main.apply([Command.revertPrefabOverride(crateA, index: 0)]).ok, isTrue);
    x = main.instance(crateA);
    expect(x.overrides, isEmpty);
    final inherited = x.entities.single.components.values.singleWhere((c) => c.type == 'fynkel.MeshRenderer');
    expect(inherited.data['cast_shadows'], false, reason: 'inherited now');
    expect(inherited.overridden, isEmpty);
    expect(prefab.state.dirty, isTrue);
    main.undo();
    prefab.undo();
    expect(main.instance(crateA).overrides, hasLength(1));
    expect(prefab.state.dirty, isFalse);

    // A broken override: shown with its diagnostic, the anchor kept.
    expect(
      main.apply([
        Command.setPrefabOverrideOp(crateA, {
          'op': 'setField',
          'entity': '11111111-0000-4000-8000-0000000000bb',
          'component': cratePrefabRenderer,
          'field': 'visible',
          'value': false,
        }),
      ]).ok,
      isTrue,
    );
    x = main.instance(crateA);
    expect(x.expanded, isFalse);
    expect(x.entities, isEmpty);
    expect(x.prefab!.name, 'Crate');
    expect(x.overrides, hasLength(2));
    expect(x.overrides[1].entityId, isNull);
    expect(x.diagnostics.single.code, 'FYN-D0403');
    final refused = main.apply([Command.unpackPrefab(crateA)]);
    expect(refused.ok, isFalse);
    expect(refused.mutations.single.diagnostics.single.code, 'FYN-E0004');
    main.undo();

    // Unpack Crate A: one transaction, the derived entity is a plain one.
    final unpacked = main.apply([Command.unpackPrefab(crateA)]);
    expect(unpacked.ok, isTrue, reason: '$unpacked');
    expect(unpacked.affectedIds, containsAll([crateA, crateEntity.id, renderer.id]));
    expect(entity(main, crateA).containsKey('prefab'), isFalse);
    expect((entity(main, crateA)['children'] as List), [crateEntity.id]);
    expect(entity(main, crateEntity.id)['parent'], crateA);
    expect(((entity(main, crateEntity.id)['components'] as Map)[renderer.id] as Map)['data'], renderer.data);
    expect(entities(main).keys.toList().indexOf(crateEntity.id), entities(main).keys.toList().indexOf(crateA) + 1);
    expect(() => main.instance(crateA), throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-E0008')));
    main.undo();
    expect(main.instance(crateA).expanded, isTrue);
    expect(File('${dir.path}/$mainScene').readAsStringSync(), sceneBytes, reason: 'nothing written');

    // The rig scene's Imported: a rig document's hierarchy, read-only.
    final rigDoc = session.openDocument(rigScene);
    const imported = 'c0000000-0000-4000-8000-0000000004e8';
    const columnRig = 'c0000000-0000-4000-8000-000000000050';
    x = rigDoc.instance(imported);
    expect(x.expanded, isTrue, reason: '${x.diagnostics}');
    expect(x.isImported, isTrue);
    expect(x.asset, columnRig);
    expect(x.sub, 'prefab');
    expect(x.open, '$columnRig#prefab');
    expect(x.prefab!.kind, 'rig');
    expect(x.prefab!.isImported, isTrue);
    expect(x.prefab!.readOnly, contains('column.rig.json#prefab'));
    expect(x.entities.map((e) => e.name), ['Rig', 'Body', 'hips', 'spine', 'head', 'Prop']);
    final prop = x.entities.last;
    expect(prop.components.values.singleWhere((c) => c.type == 'fynkel.MeshRenderer').overridden, ['material']);
    final hierarchy = session.openDocument(x.open);
    expect(session.openDocument('$columnRig#prefab'), same(hierarchy));
    expect(hierarchy.state.kind, 'prefab');
    expect(hierarchy.state.sub, 'prefab');
    expect(hierarchy.state.isImportedHierarchy, isTrue);
    expect(hierarchy.state.relativePath, 'rigs/column.rig.json#prefab');
    expect(hierarchy.state.readOnly, x.prefab!.readOnly);
    expect(rigDoc.instance(imported).prefab!.handle, hierarchy.handle);
    expect(entities(hierarchy).keys, contains(prop.local));
    expect(entities(hierarchy).length, 6);
    expect(hierarchy.roots(), hasLength(1));
    final readOnly = hierarchy.apply([Command.renameObject(prop.local, 'X')]);
    expect(readOnly.ok, isFalse);
    expect(readOnly.allDiagnostics.map((d) => d.code), contains('FYN-E0007'));
    expect(() => hierarchy.save(), throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-E0007')));
    hierarchy.reload();
    expect(hierarchy.state.readOnly, isNotNull);
    expect(
      () => session.openDocument('c0000000-0000-4000-8000-000000000003#prefab'),
      throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-D0401')),
      reason: 'a texture has no hierarchy',
    );
    // Unpack Imported: 15 entities, none an instance, one undo step.
    expect(rigDoc.apply([Command.unpackPrefab(imported)]).ok, isTrue);
    expect(rigDoc.inspect()['entityCount'], 15);
    expect(rigDoc.inspect()['instanceCount'], 0);
    expect(entities(rigDoc)[prop.id], isNotNull);
    rigDoc.undo();
    expect(rigDoc.inspect()['entityCount'], 9);
  });

  test('the migration preview names an outdated scene, its steps and the diff, and writes nothing', () {
    final dir = copyFixture('migrations');
    addTearDown(() => dir.deleteSync(recursive: true));
    final v0 = File('${fixture.parent.path}/migrate/v0_training.scene.json');
    final target = File('${dir.path}/assets/scenes/training.scene.json');
    v0.copySync(target.path);
    final before = target.readAsStringSync();
    final session = AuthoringSession();
    addTearDown(session.dispose);
    session.openProject(dir.path);
    final preview = session.migrationPreview();
    expect(preview.outdated, 1);
    final doc = preview.documents.single;
    expect(doc.kind, 'scene');
    expect(doc.from, 0);
    expect(doc.to, 4, reason: 'scenes are at formatVersion 4 (ADR-0020): four steps');
    expect(doc.ok, isTrue);
    expect(doc.path, target.path);
    expect(doc.applied.first.id, 'scene/0-1/entities-keyed-by-id');
    expect(doc.applied.map((a) => a.from), [0, 1, 2, 3]);
    expect(doc.diff, contains('formatVersion'));
    expect(target.readAsStringSync(), before, reason: 'a preview writes nothing');
    expect(session.validateProject().diagnostics.map((d) => d.code), contains('FYN-D0016'));
  });

  test('setLayersAndTags renames in the project file, reorders through the documents and re-reads the open ones', () {
    final dir = copyFixture('layers');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    final events = <AuthoringEvent>[];
    session.events.listen(events.add);
    session.openProject(dir.path);
    final main = session.openDocument(mainScene);
    int crateBLayer() {
      final components = (entities(main)[crateB] as Map)['components'] as Map;
      final layer = components.values.firstWhere((c) => (c as Map)['type'] == 'fynkel.Layer') as Map;
      return ((layer['data'] as Map)['layer'] as num).toInt();
    }

    expect(crateBLayer(), 1);
    final project = File('${dir.path}/fynkel.project.json');

    // A rename and a new tag: the project file only, nothing to re-read.
    final renamed = session.setLayersAndTags(layers: ['default', 'crates'], tags: ['pickup']);
    expect(renamed.label, 'Edit Tags and Layers');
    expect(renamed.reload, isEmpty);
    expect((jsonDecode(project.readAsStringSync()) as Map)['layers'], ['default', 'crates']);
    expect(events.whereType<DocumentChanged>().where((e) => e.kind == DocumentChangeKind.reloaded), isEmpty);

    // A layer inserted before `crates`: Crate B follows it to index 2 and
    // the open scene is re-read.
    final moved = session.setLayersAndTags(
      layers: ['default', 'ghosts', 'crates'],
      layersFrom: [0, null, 1],
      tags: ['pickup'],
    );
    expect(moved.reload, [main.handle]);
    expect(events.whereType<DocumentChanged>().last.kind, DocumentChangeKind.reloaded);
    expect(crateBLayer(), 2);
    expect(main.state.dirty, isFalse);

    session.undoAsset();
    expect(crateBLayer(), 1, reason: 'the undo rewrites the file back and re-reads it');

    // An unsaved edit protects the document: refused, nothing written.
    main.apply([Command.renameObject(floor, 'Ground')]);
    final text = project.readAsStringSync();
    expect(
      () => session.setLayersAndTags(layers: ['default', 'ghosts', 'crates'], layersFrom: [0, null, 1], tags: ['pickup']),
      throwsA(isA<AuthoringException>().having((e) => e.diagnostics.single.code, 'code', 'FYN-T0003')),
    );
    expect(project.readAsStringSync(), text);
  });
}

/// Names of [id] and its descendants, depth-first in children order.
List<String> _subtreeNames(DocumentHandle doc, String id) {
  final all = entities(doc);
  final out = <String>[];
  void walk(String id) {
    final e = all[id] as Map<String, Object?>;
    out.add(e['name'] as String);
    for (final c in e['children'] as List) {
      walk(c as String);
    }
  }

  walk(id);
  return out;
}
