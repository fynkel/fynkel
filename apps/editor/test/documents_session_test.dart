// The tabs and the Hierarchy over the real session (the M4-03 gate): every
// Hierarchy operation is one transaction that the saved file and the
// `inspect` view (what `fynkel scene inspect --json` prints) reflect; the
// conflict path with the file edited under a dirty tab; a 10 000-entity
// scene opens, selects and edits without rebuilding the tree.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_model.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';

import 'project_test.dart' show copyFixture;

const mainScene = 'c0000000-0000-4000-8000-000000000030';
const camera = 'c0000000-0000-4000-8000-0000000000e0';
const robot = 'c0000000-0000-4000-8000-0000000000e1';
const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const floor = 'c0000000-0000-4000-8000-0000000000e4';
const crateB = 'c0000000-0000-4000-8000-0000000000e5';

Map<String, Object?> readJson(String path) => jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>;

void main() {
  test('every Hierarchy operation is one transaction; save and inspect reflect it', () async {
    final dir = copyFixture('hier');
    addTearDown(() => dir.deleteSync(recursive: true));
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final model = HierarchyModel(tabs: tabs, project: project);
    addTearDown(model.dispose);

    final tab = tabs.openDocument(mainScene)!;
    expect(tab.kind, 'scene');
    expect(tab.title, 'Main');
    expect(tab.tabLabel, 'Main'); // the file is main.scene.json
    final tree = tab.tree;
    expect(tree.builds, 1);
    expect(tree.roots.first, camera);
    expect(tree[robot]!.name, 'Robot');
    expect(tree[robot]!.hasComponent('game.Spinner'), isTrue);
    var history = 0;

    // Rename.
    expect(model.rename(robot, 'Bot'), isTrue);
    expect(tab.document.history, ++history);
    expect(tree[robot]!.name, 'Bot');
    expect(tree.builds, 1);

    // Create empty under the camera, create with a Light at the root.
    model.select(camera);
    expect(model.createEmpty(), isTrue);
    expect(tab.document.history, ++history);
    final empty = model.selection.single;
    expect(tree[empty]!.parent, camera);
    expect(tree[camera]!.children, [empty]);
    expect(tree[empty]!.transform, isNotNull);
    model.clearSelection();
    expect(model.createWith('fynkel.PointLight'), isTrue);
    expect(tab.document.history, ++history);
    final light = model.selection.single;
    expect(tree[light]!.componentTypes, containsAll(['fynkel.Transform', 'fynkel.PointLight']));
    expect(tree.roots.last, light);

    // Reparent crateA under floor, keeping the world transform, in one transaction.
    final crateWorldBefore = Trs.fromMatrix(tree.worldMatrix(crateA));
    expect(model.reparent([crateA], DropTarget(parent: floor, index: 0)), isTrue);
    expect(tab.document.history, ++history);
    expect(tree[crateA]!.parent, floor);
    expect(tree[floor]!.children.first, crateA);
    final crateWorldAfter = Trs.fromMatrix(tree.worldMatrix(crateA));
    expect(crateWorldAfter.sameAs(crateWorldBefore), isTrue);
    expect(tree[crateA]!.transform!.sameAs(crateWorldBefore), isFalse); // the local changed (floor is at y −1)

    // Duplicate the floor (with the crate under it) → one transaction, the copy selected.
    model.select(floor);
    expect(model.duplicate(), isTrue);
    expect(tab.document.history, ++history);
    final floorCopy = model.selection.single;
    expect(tree[floorCopy]!.name, 'Floor');
    expect(tree[floorCopy]!.children.length, 1);
    expect(tree.roots.last, floorCopy); // a scene's roots follow the entity key order: the copy is newest
    // A child's copy lands right after its original.
    model.select(crateA);
    expect(model.duplicate(), isTrue);
    expect(tab.document.history, ++history);
    final crateCopy = model.selection.single;
    expect(tree[floor]!.children, [crateA, crateCopy]);

    // Delete crateB.
    model.select(crateB);
    expect(model.delete(), isTrue);
    expect(tab.document.history, ++history);
    expect(tree[crateB], isNull);
    expect(tree.builds, 1); // nothing above rebuilt the tree

    // Undo the delete, redo it.
    expect(tab.undo(), isTrue);
    expect(tree[crateB], isNotNull);
    expect(tab.redo(), isTrue);
    expect(tree[crateB], isNull);
    expect(tree.builds, 1);

    // Save: the file and the inspect view carry every operation.
    expect(tab.dirty, isTrue);
    expect(tab.save(), isTrue);
    expect(tab.dirty, isFalse);
    final onDisk = readJson(tab.path);
    final entities = onDisk['entities'] as Map;
    expect(entities[robot]['name'], 'Bot');
    expect(entities[empty]['parent'], camera);
    expect(entities[light], isNotNull);
    expect(entities[crateA]['parent'], floor);
    expect(entities[floorCopy], isNotNull);
    expect(entities[crateB], isNull);
    final inspected = tab.document.inspect();
    expect(jsonEncode(inspected), contains('"Bot"'));
    expect(jsonEncode(inspected), isNot(contains(crateB)));
    // The saved document validates clean.
    expect(tab.document.validate().where((d) => d.severity == Severity.error), isEmpty);
    // Reopening the file from a fresh session sees the same.
    final again = await SessionProject.open(dir.path, watch: false);
    addTearDown(again.close);
    final doc = again.openDocument(tab.path);
    expect(doc.root()['entities'], hasLength(entities.length));
    expect(doc.roots().last, floorCopy);
    expect((doc.root()['entities'] as Map)[floor]['children'], [crateA, crateCopy]);
  });

  test('the conflict path: the file edited from outside while the tab is dirty is never overwritten', () async {
    final dir = copyFixture('conflict');
    addTearDown(() => dir.deleteSync(recursive: true));
    final project = await SessionProject.open(dir.path);
    addTearDown(project.close);
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final tab = tabs.openDocument(mainScene)!;
    final baseRevision = tab.document.revision;
    await Future<void>.delayed(const Duration(milliseconds: 300)); // the watcher's ready

    // Local edit, then the "CLI" rewrites the file (a different rename).
    tab.transaction([Command.renameObject(camera, 'Mine')]);
    expect(tab.dirty, isTrue);
    final j = readJson(tab.path);
    ((j['entities'] as Map)[camera] as Map)['name'] = 'Theirs';
    final theirs = jsonEncode(j);
    final flagged = Completer<void>();
    tab.addListener(() {
      if (tab.hasConflict && !flagged.isCompleted) flagged.complete();
    });
    File(tab.path).writeAsStringSync(theirs);
    await flagged.future.timeout(const Duration(seconds: 15));

    final c = tab.conflict!;
    expect(c.baseRevision, baseRevision);
    expect(c.ourRevision, tab.document.revision);
    expect(c.diskRevision, project.fileRevision(tab.path));
    expect(c.diskRevision, isNot(baseRevision));
    expect(c.history, 1);
    expect(c.diskText, theirs);
    expect(tab.tree[camera]!.name, 'Mine');

    // Save is refused; the file still holds the other writer's text.
    final original = tab.path;
    expect(tab.save(), isFalse);
    expect(tab.lastError, contains('changed on disk'));
    expect(File(original).readAsStringSync(), theirs);
    // Save As writes a copy beside it; the original is untouched.
    final copy = '${dir.path}/assets/scenes/main copy.scene.json';
    expect(tab.saveAs(copy), isTrue);
    expect(tab.hasConflict, isFalse);
    expect(File(original).readAsStringSync(), theirs);
    expect(tab.path, copy);
    expect(tab.title, 'Main'); // the name inside the file is unchanged …
    expect(tab.tabLabel, 'Main · main copy'); // … so the tab shows the file too
    expect((readJson(copy)['entities'] as Map)[camera]['name'], 'Mine');
    expect(readJson(copy)['id'], mainScene); // same document id: the CLI will flag the duplicate; the user renames

    // A tab with no local edits reloads silently on an external change.
    final rig = tabs.openDocument('${dir.path}/assets/scenes/rig.scene.json')!;
    await Future<void>.delayed(const Duration(milliseconds: 300));
    final rigJson = readJson(rig.path);
    ((rigJson['entities'] as Map)['c0000000-0000-4000-8000-0000000004e0'] as Map)['name'] = 'Reloaded Eye';
    final reloaded = Completer<void>();
    rig.addListener(() {
      if (rig.tree['c0000000-0000-4000-8000-0000000004e0']?.name == 'Reloaded Eye' && !reloaded.isCompleted) {
        reloaded.complete();
      }
    });
    File(rig.path).writeAsStringSync(jsonEncode(rigJson));
    await reloaded.future.timeout(const Duration(seconds: 15));
    expect(rig.hasConflict, isFalse);
    expect(rig.dirty, isFalse);
    expect(rig.tree.builds, 2); // a reload rebuilds; edits patch
  }, timeout: const Timeout(Duration(seconds: 60)));

  test('a 10 000-entity scene opens, selects and edits without rebuilding the tree', () async {
    final dir = copyFixture('big');
    addTearDown(() => dir.deleteSync(recursive: true));
    // A generated scene: 2 500 chains of depth 4, Transforms everywhere.
    final entities = <String, Object?>{};
    String id(int n) => 'b0000000-0000-4000-8000-${n.toString().padLeft(12, '0')}';
    Map<String, Object?> transform(int n) => {
      'type': 'fynkel.Transform',
      'version': 1,
      'data': {
        'position': [n % 7, 0, n % 5],
        'rotation': [0, 0, 0, 1],
        'scale': [1, 1, 1],
      },
    };
    var n = 0;
    while (n < 10000) {
      final root = n;
      entities[id(n)] = {
        'name': 'Root $n',
        'parent': null,
        'children': <String>[],
        'components': {id(100000 + n): transform(n)},
      };
      n++;
      var parent = root;
      for (var d = 1; d < 4 && n < 10000; d++) {
        entities[id(n)] = {
          'name': 'Node $n',
          'parent': id(parent),
          'children': <String>[],
          'components': {id(100000 + n): transform(n)},
        };
        ((entities[id(parent)] as Map)['children'] as List).add(id(n));
        parent = n;
        n++;
      }
    }
    final path = '${dir.path}/assets/scenes/big.scene.json';
    File(path).writeAsStringSync(
      jsonEncode({
        r'$schema': 'https://fynkel.dev/schemas/scene.schema.json',
        'formatVersion': 5,
        'id': 'b0000000-0000-4000-8000-ffffffffffff',
        'name': 'Big',
        'entities': entities,
      }),
    );
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final model = HierarchyModel(tabs: tabs, project: project);
    addTearDown(model.dispose);

    final sw = Stopwatch()..start();
    final tab = tabs.openDocument(path)!;
    final tree = tab.tree;
    final openMs = sw.elapsedMilliseconds;
    expect(tree.entityCount, 10000);
    expect(tree.roots.length, 2500);
    expect(tree.builds, 1);
    model.expandAll();
    expect(tree.rows.length, 10000);

    sw.reset();
    model.select(id(5001));
    model.select(id(5020), range: true);
    expect(model.selection.length, 20);
    final node = tree[id(5001)]!;
    expect(model.rename(id(5001), 'Picked'), isTrue);
    expect(model.reparent([id(5002)], DropTarget(parent: id(0), index: 0)), isTrue);
    final editMs = sw.elapsedMilliseconds;
    expect(tree.builds, 1);
    expect(identical(tree[id(5001)], node), isTrue);
    expect(tree[id(5002)]!.parent, id(0));
    expect(tree[id(5000)]!.version, 1);
    expect(tab.document.history, 2);
    expect(tab.save(), isTrue);
    // Bounds are loose on purpose (CI machines vary); the point is "no rebuild" above.
    expect(openMs, lessThan(20000));
    expect(editMs, lessThan(5000));
    // ignore: avoid_print
    print('10k scene: open+build ${openMs}ms, select+rename+reparent ${editMs}ms');
  }, timeout: const Timeout(Duration(seconds: 120)));
}
