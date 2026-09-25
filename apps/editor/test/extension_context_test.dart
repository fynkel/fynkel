// Editor extensions (ADR-0028 step 2): what an `EditorContext` reads — the
// scene in front, its selection, the assets — and how it changes them: one
// undo step through the tab, prefab overrides where the object comes from
// a prefab, nothing written when one change is refused.
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart'
    show AssetRef, BehaviourRegistry, DataAsset, DataField, DataType, ObjectSpec, Transform, Vec3;
import 'package:fynkel_authoring/fynkel_authoring.dart' show Severity;
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/documents/open_document.dart';
import 'package:fynkel_editor/src/extensions/editor_access.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import 'support/fakes.dart';

class Coin extends DataAsset {
  int value = 1;
}

final coinType = DataType(
  id: 'game.Coin',
  create: Coin.new,
  fields: [
    DataField(
      name: 'value',
      type: 'integer',
      defaultValue: 1,
      get: (d) => (d as Coin).value,
      set: (d, v) => (d as Coin).value = v as int,
    ),
  ],
);

class _Unregistered extends DataAsset {}

void registerGame(BehaviourRegistry r) => r.registerData(coinType);

/// The prefab the scene's crate is an instance of: a root with a light.
FakeDocument cratePrefab() => FakeDocument(
  path: '/fake/game/assets/prefabs/crate.prefab.json',
  relativePath: 'prefabs/crate.prefab.json',
  id: 'crate-prefab',
  kind: 'prefab',
  name: 'Crate',
  roots: ['root'],
  entities: {
    'root': FakeDocument.entity('Crate', children: ['lamp']),
    'lamp': FakeDocument.entity(
      'Lamp',
      parent: 'root',
      components: {
        'l': {
          'type': 'fynkel.Light',
          'version': 1,
          'data': {
            'kind': 'point',
            'color': [1, 1, 1],
            'intensity': 1,
          },
        },
      },
    ),
  },
);

/// A camera, a level with two coins, and a crate instance.
FakeDocument scene() => FakeDocument(
  path: '/fake/game/assets/scenes/main.scene.json',
  relativePath: 'scenes/main.scene.json',
  id: 'main',
  name: 'Main',
  roots: ['cam', 'level', 'crate'],
  entities: {
    'cam': FakeDocument.entity('Camera', position: [0, 2, 6]),
    'level': FakeDocument.entity('Level', children: ['c1', 'c2']),
    'c1': FakeDocument.entity('Coin', parent: 'level', position: [1, 0, 0]),
    'c2': FakeDocument.entity('Coin', parent: 'level', position: [2, 0, 0]),
    'crate': FakeDocument.entity(
      'Crate A',
      position: [3, 0, 0],
      prefab: {'asset': 'crate-prefab', 'instance': 'i1', 'overrides': <Object?>[]},
    ),
  },
);

void main() {
  late FakeProject project;
  late DocumentTabs tabs;
  late OpenDocument tab;
  late EditorExtensionHost host;
  late EditorContext context;
  late List<String> pinged;
  late List<String> opened;

  setUp(() {
    final dir = fakeProjectDir('extension_context');
    addTearDown(() => dir.deleteSync(recursive: true));
    project = FakeProject(dir: dir.path);
    project.addDocument(cratePrefab());
    project.addDocument(scene());
    project.assetEntries = [
      const AssetEntry(id: 'main', path: 'scenes/main.scene.json', name: 'main', kind: 'document', document: 'scene'),
      const AssetEntry(
        id: 'gold',
        path: 'data/gold.data.json',
        name: 'gold',
        kind: 'document',
        document: 'data',
        dataType: 'game.Coin',
      ),
      const AssetEntry(id: 'hero', path: 'models/hero.glb', name: 'hero', kind: 'source', importer: 'fynkel.gltf'),
    ];
    tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    tab = tabs.openDocument('main')!;
    pinged = [];
    opened = [];
    host = EditorExtensionHost(
      const [],
      access: EditorAccess(
        tabs: () => tabs,
        project: () => project,
        browser: () => null,
        ping: pinged.add,
        open: (a) => opened.add(a.id),
        registerBehaviours: registerGame,
      ),
    );
    context = host.contextFor('game');
  });

  List<String> errors() => [
    for (final d in host.log)
      if (d.severity == Severity.error) d.message,
  ];

  test('the scene: objects in the Hierarchy order, found by name or path, the instance with its prefab', () {
    final s = context.scene!;
    expect(s.id, 'main');
    expect(s.path, 'scenes/main.scene.json');
    expect(s.isPrefab, isFalse);
    expect(s.isPlaying, isFalse);
    expect([for (final o in s.roots) o.name], ['Camera', 'Level', 'Crate A']);
    expect(s.objects.map((o) => o.name).take(4), ['Camera', 'Level', 'Coin', 'Coin']);
    final level = s.find('Level')!;
    expect(level.childIds, ['c1', 'c2']);
    expect(s.byId('c2')!.parentId, 'level');
    expect(s.find('Level/Coin')!.id, 'c1');
    expect(s.find('Level/Nope'), isNull);
    expect(s.find('Camera')!.component('fynkel.Transform')!['position'], [0, 2, 6]);

    final crate = s.find('Crate A')!;
    expect(crate.prefab, AssetRef('crate-prefab'));
    expect(crate.fromPrefab, isFalse);
    // The prefab's lamp, under the instance, from the prefab.
    final lamp = s.find('Lamp')!;
    expect(lamp.fromPrefab, isTrue);
    // Under the instance, through what the Hierarchy shows above it.
    var up = s.byId(lamp.parentId!);
    while (up != null && up.id != 'crate') {
      up = up.parentId == null ? null : s.byId(up.parentId!);
    }
    expect(up?.id, 'crate');
    expect(lamp.component('fynkel.Light')!['intensity'], 1);
  });

  test('selection and select follow the tab', () {
    expect(context.selection, isEmpty);
    tab.setSelection(['c2', 'cam']);
    expect([for (final o in context.selection) o.id], ['c2', 'cam']);
    final s = context.scene!;
    context.select([s.byId('c1')!, s.byId('level')!]);
    expect(tab.selection.toList(), ['c1', 'level']);
    context.select(const []);
    expect(tab.selection, isEmpty);
  });

  test('an edit is one undo step: fields, names, removals and objects added from specs', () {
    final s = context.scene!;
    final r = context.edit('Tidy', (edit) {
      edit.setField(s.find('Camera')!, 'fynkel.Transform', 'position', Vec3(0, 5, 10));
      edit.rename(s.byId('c1')!, 'Gold Coin');
      edit.remove(s.byId('c2')!);
      edit.add(
        ObjectSpec(
          'Ring',
          transform: Transform(position: Vec3(0, 1, 0)),
          children: [ObjectSpec('Spark', transform: Transform(position: Vec3(1, 0, 0)))],
        ),
        parent: s.find('Level'),
      );
    });
    expect(r.ok, isTrue, reason: r.error);
    expect(errors(), isEmpty);
    expect(r.added.single.name, 'Ring');
    expect(r.added.single.parentId, 'level');

    final after = context.scene!;
    expect(after.find('Camera')!.component('fynkel.Transform')!['position'], [0.0, 5.0, 10.0]);
    expect(after.byId('c1')!.name, 'Gold Coin');
    expect(after.byId('c2'), isNull);
    final ring = after.find('Level/Ring')!;
    expect(ring.component('fynkel.Transform')!['position'], [0.0, 1.0, 0.0]);
    expect([for (final c in ring.childIds) after.byId(c)!.name], ['Spark']);

    // One step back undoes all of it.
    expect(tab.undo(), isTrue);
    final undone = context.scene!;
    expect(undone.find('Camera')!.component('fynkel.Transform')!['position'], [0, 2, 6]);
    expect(undone.byId('c1')!.name, 'Coin');
    expect(undone.byId('c2'), isNotNull);
    expect(undone.find('Ring'), isNull);
  });

  test('on a prefab instance a change is an override, and a structural one is refused', () {
    final s = context.scene!;
    final r = context.edit('Brighter', (edit) {
      edit.setField(s.find('Lamp')!, 'fynkel.Light', 'intensity', 3);
      edit.setField(s.find('Lamp')!, 'fynkel.Light', 'color', [1, 0, 0]);
    });
    expect(r.ok, isTrue, reason: r.error);
    final overrides = (project.fakeDocuments.last.entitiesView['crate']!['prefab'] as Map)['overrides'] as List;
    expect([for (final o in overrides) '${(o as Map)['entity']}.${o['field']}'], ['lamp.intensity', 'lamp.color']);
    final lamp = context.scene!.find('Lamp')!;
    expect(lamp.component('fynkel.Light')!['intensity'], 3);

    final refused = context.edit('Tear down', (edit) => edit.remove(lamp));
    expect(refused.ok, isFalse);
    expect(refused.error, contains('comes from its prefab'));
    expect(errors().last, contains('"Tear down" changed nothing'));
  });

  test('one refused change writes nothing, and a throw in the changes is a refusal too', () {
    final s = context.scene!;
    final r = context.edit('Half', (edit) {
      edit.rename(s.find('Camera')!, 'Eye');
      edit.setField(s.find('Camera')!, 'game.Missing', 'speed', 1);
    });
    expect(r.ok, isFalse);
    expect(r.error, contains('has no game.Missing'));
    expect(context.scene!.find('Camera'), isNotNull);
    expect(tab.document.canUndo, isFalse);

    final thrown = context.edit('Boom', (edit) => throw StateError('no level'));
    expect(thrown.ok, isFalse);
    expect(thrown.error, contains('no level'));
    expect(errors(), hasLength(2));
  });

  test('assets: found by path, stem, name, kind and data type; opened, pinged and created', () {
    final assets = context.assets;
    expect(assets.all, hasLength(3));
    expect(assets.find('data/gold.data.json')!.id, 'gold');
    expect(assets.find('data/gold')!.id, 'gold');
    expect(assets.find('hero')!.kind, 'model');
    expect([for (final a in assets.ofKind('scene')) a.id], ['main']);
    expect([for (final a in assets.ofDataType('game.Coin')) a.id], ['gold']);

    assets.ping(assets.find('hero')!);
    assets.open(assets.find('main')!);
    expect(pinged, ['hero']);
    expect(opened, ['main']);

    final silver = Coin()..value = 5;
    final made = assets.createData('data/silver', silver)!;
    expect(made.path, 'data/silver.data.json');
    expect(made.dataType, 'game.Coin');
    expect(silver.asset, AssetRef(made.id));
    expect(project.newData[made.id], {'value': 5});
    expect(context.assets.ofDataType('game.Coin'), hasLength(2));

    // A type the game did not register: nothing written, the Console says why.
    expect(assets.createData('data/other', _Unregistered()), isNull);
    expect(errors().single, contains('not a registered data type'));
  });
}
