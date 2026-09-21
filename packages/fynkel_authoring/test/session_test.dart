// A complete edit session over the authoring ABI on a private copy of the
// fixture project (blueprint §19.2e item 1 gate): open, every §11.1
// command, undo back to the original bytes, save, the stale-revision
// conflict, a material's base_color, duplicateObject of the rig — through
// the native runtime package:fynkel's hook built with `--features authoring`.

import 'dart:convert';
import 'dart:io';

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
  test('the loaded runtime speaks authoring ABI v1', () {
    expect(AuthoringSession.loadedAbiVersion, authoringAbiVersion);
    expect(authoringAbiVersion, 1);
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
    expect(project.assetCount, 16);
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
        componentSpec('fynkel.PointLight', id: lampLight, data: {'color': [1, 0.9, 0.8], 'intensity': 2, 'range': 5}),
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
    ok([Command.renameObject(robot, 'Bot'), Command.reparentObject(lamp, parent: robot, index: 0)]);
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
    expect(session.refreshProject().assetCount, 17, reason: 'the new material is in the database');
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
    expect(session.rescan([copyPath]).assetCount, 17, reason: 'the deleted copy left the database');

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
    expect(doc.to, 3, reason: 'scenes are at formatVersion 3 (M3-19c): three steps');
    expect(doc.ok, isTrue);
    expect(doc.path, target.path);
    expect(doc.applied.first.id, 'scene/0-1/entities-keyed-by-id');
    expect(doc.applied.map((a) => a.from), [0, 1, 2]);
    expect(doc.diff, contains('formatVersion'));
    expect(target.readAsStringSync(), before, reason: 'a preview writes nothing');
    expect(session.validateProject().diagnostics.map((d) => d.code), contains('FYN-D0016'));
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
