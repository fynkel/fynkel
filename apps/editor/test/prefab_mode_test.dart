// Prefab mode over the fakes (M4-07, blueprint §19.2e item 7): a prefab
// instance opens to its derived entities in the Hierarchy (fetched once
// per anchor, refreshed when the prefab document changes), a derived
// entity's edits are override ops on the anchor (one transaction each),
// Revert override, Apply to prefab (a transaction on the prefab document,
// then the revert on the instance — two undo steps), Unpack (one
// transaction), an imported hierarchy opens read-only and takes overrides,
// a missing prefab is shown with its diagnostic and never rewritten, and
// the `prefab.*` commands, the context menu and the panels reach it all.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/documents/open_document.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_model.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_panel.dart';
import 'package:fynkel_editor/src/inspector/inspector_model.dart';
import 'package:fynkel_editor/src/inspector/prefab_actions.dart' as prefab;
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

/// The native schemas the Inspector draws fields from.
List<TypeSchema> nativeSchemas() {
  final out = <TypeSchema>[];
  for (final f in Directory('../../schemas/components').absolute.listSync().whereType<File>()) {
    if (!f.path.endsWith('.schema.json')) continue;
    final schema = jsonDecode(f.readAsStringSync()) as Map<String, Object?>;
    final x = schema['x-fynkel'] as Map? ?? const {};
    out.add(
      TypeSchema(
        type: x['type'] as String? ?? schema['title'] as String,
        isBehaviour: false,
        authored: x['authored'] != false,
        allowed: true,
        schema: schema,
      ),
    );
  }
  return out;
}

/// The crate prefab: a root with a lit mesh child.
FakeDocument cratePrefab() => FakeDocument(
  path: '/fake/game/assets/prefabs/crate.prefab.json',
  relativePath: 'prefabs/crate.prefab.json',
  id: 'crate-prefab',
  kind: 'prefab',
  name: 'Crate',
  roots: ['root'],
  entities: {
    'root': FakeDocument.entity('Crate', children: ['mesh']),
    'mesh': FakeDocument.entity(
      'Mesh',
      parent: 'root',
      position: [0, 1, 0],
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

/// An imported hierarchy (`rig.glb#prefab`): read-only, derived from the source.
FakeDocument importedRig() => FakeDocument(
  path: '/fake/game/assets/models/rig.glb',
  relativePath: 'models/rig.glb',
  id: 'rig-src',
  sourceId: 'rig-src',
  sub: 'prefab',
  kind: 'prefab',
  name: 'rig',
  readOnly: 'derived from models/rig.glb — edit the source',
  roots: ['n0'],
  entities: {
    'n0': FakeDocument.entity('Armature', children: ['n1']),
    'n1': FakeDocument.entity('Bone', parent: 'n0', position: [0, 2, 0]),
  },
);

/// The scene: a camera, a crate instance with one override, an instance of
/// a prefab nobody has, and an instance of the imported rig.
FakeDocument scene() => FakeDocument(
  path: '/fake/game/assets/scenes/main.scene.json',
  relativePath: 'scenes/main.scene.json',
  id: 'main',
  name: 'Main',
  roots: ['cam', 'inst', 'broken', 'imported'],
  entities: {
    'cam': FakeDocument.entity('Camera', position: [0, 2, 6]),
    'inst': FakeDocument.entity(
      'Crate A',
      position: [3, 0, 0],
      prefab: {
        'asset': 'crate-prefab',
        'instance': 'i1',
        'overrides': [
          {'op': 'setField', 'entity': 'mesh', 'component': 'l', 'field': 'intensity', 'value': 5},
        ],
      },
    ),
    'broken': FakeDocument.entity(
      'Broken',
      prefab: {
        'asset': 'nope',
        'instance': 'i9',
        'overrides': [
          {'op': 'setField', 'entity': 'x', 'component': 'y', 'field': 'z', 'value': 1},
        ],
      },
    ),
    'imported': FakeDocument.entity(
      'Imported',
      prefab: {
        'asset': {'asset': 'rig-src', 'sub': 'prefab'},
        'instance': 'i2',
        'overrides': [],
      },
    ),
  },
);

final class Fixture {
  Fixture() {
    project = FakeProject(dir: '/fake/game');
    project.schemas = nativeSchemas();
    prefabDoc = project.addDocument(cratePrefab());
    rig = project.addDocument(importedRig());
    sceneDoc = project.addDocument(scene());
    tabs = DocumentTabs(project: project);
    hierarchy = HierarchyModel(tabs: tabs, project: project);
    inspector = InspectorModel(tabs: tabs, project: project);
    tab = tabs.openDocument('main')!;
  }

  late final FakeProject project;
  late final FakeDocument prefabDoc;
  late final FakeDocument rig;
  late final FakeDocument sceneDoc;
  late final DocumentTabs tabs;
  late final HierarchyModel hierarchy;
  late final InspectorModel inspector;
  late final OpenDocument tab;

  List<Map<String, Object?>> overridesOf(String anchor) => [
    for (final o in (sceneDoc.entitiesView[anchor]!['prefab'] as Map)['overrides'] as List)
      Map<String, Object?>.from(o as Map),
  ];

  void dispose() {
    hierarchy.dispose();
    inspector.dispose();
    tabs.dispose();
  }
}

void main() {
  group('Hierarchy', () {
    test('an instance opens to its derived entities, fetched once and cached', () {
      final f = Fixture();
      addTearDown(f.dispose);
      final tree = f.hierarchy.tree!;
      expect(tree.rows.map((r) => r.id), ['cam', 'inst', 'broken', 'imported']);
      expect(tree.rows[1].hasChildren, isTrue, reason: 'an instance opens even before its expansion is fetched');
      expect(tree.expansionFetches, 0);
      f.hierarchy.expand('inst');
      expect(tree.rows.map((r) => r.id), ['cam', 'inst', 'i1:root', 'i1:mesh', 'broken', 'imported']);
      expect(tree.expansionFetches, 1);
      final mesh = tree['i1:mesh']!;
      expect(mesh.isDerived, isTrue);
      expect(mesh.anchor, 'inst');
      expect(mesh.local, 'mesh');
      expect(mesh.overriddenFields, 1);
      expect(mesh.transformId, 'i1:t', reason: 'the derived component id — the world knows it');
      expect(mesh.transformLocal, 't', reason: 'the prefab-local id an override addresses');
      expect(mesh.transform!.position, [0, 1, 0]);
      expect(tree['i1:root']!.parent, 'inst');
      expect(tree.entityCount, 4, reason: 'derived entities are not document entities');
      expect(tree.anchorOf('i1:mesh'), 'inst');
      expect(tree.anchorOf('cam'), isNull);
      f.hierarchy.collapse('inst');
      f.hierarchy.expand('inst');
      expect(tree.expansionFetches, 1, reason: 'cached');
      // The world matrix goes through the anchor.
      expect(tree.worldMatrix('i1:mesh')[12], 3);
      expect(tree.worldMatrix('i1:mesh')[13], 1);
    });

    test('revealAndSelect finds a derived entity of an instance not opened yet (a Console diagnostic)', () {
      final f = Fixture();
      addTearDown(f.dispose);
      final tree = f.hierarchy.tree!;
      expect(tree.expansionFetches, 0);
      expect(f.hierarchy.revealAndSelect('i1:mesh'), isTrue);
      expect(tree.rows.map((r) => r.id), contains('i1:mesh'));
      expect(f.tab.selection, {'i1:mesh'});
      expect(tree.expansionFetches, greaterThanOrEqualTo(1));
      expect(f.hierarchy.revealAndSelect('nobody'), isFalse);
    });

    test('a missing prefab shows as such: no rows, the diagnostic, nothing rewritten', () {
      final f = Fixture();
      addTearDown(f.dispose);
      final tree = f.hierarchy.tree!;
      final revision = f.sceneDoc.revision;
      f.hierarchy.expand('broken');
      expect(tree.rows.map((r) => r.id), ['cam', 'inst', 'broken', 'imported']);
      final x = f.tab.expansionOf('broken')!;
      expect(x.expanded, isFalse);
      expect(x.prefab, isNull);
      expect(x.diagnostics.single.code, 'FYN-D0401');
      expect(x.overrides.single.field, 'z', reason: 'the instance keeps its overrides');
      expect(f.sceneDoc.revision, revision);
      expect(f.sceneDoc.dirty, isFalse);
      expect(f.inspector.cannotApply('broken'), contains('missing'));
      expect(f.inspector.cannotUnpack('broken'), contains('does not expand'));
    });

    test('derived entities are not duplicated, dragged, dropped on or instantiated under', () {
      final f = Fixture();
      addTearDown(f.dispose);
      f.hierarchy.expand('inst');
      f.hierarchy.select('i1:mesh');
      expect(f.hierarchy.duplicate(), isFalse);
      expect(f.tab.lastError, contains('cannot be duplicated'));
      expect(f.hierarchy.canDrop(['i1:mesh'], const DropTarget(parent: null, index: 0)), isFalse);
      expect(f.hierarchy.canDrop(['cam'], const DropTarget(parent: 'i1:root', index: 0)), isFalse);
      expect(f.hierarchy.canDrop(['cam'], const DropTarget(parent: 'inst', index: 0)), isTrue);
      expect(
        f.hierarchy.canDropAsset({'asset': 'crate-prefab'}, 'prefab', const DropTarget(parent: 'i1:root', index: 0)),
        isFalse,
      );
      expect(f.hierarchy.moveToRoot(), isFalse);
      expect(f.hierarchy.canRename('i1:mesh'), isFalse);
      expect(f.hierarchy.rename('i1:mesh', 'Other'), isFalse);
      expect(f.tab.lastError, contains('named by the prefab'));
      expect(f.sceneDoc.history, 0, reason: 'nothing was a transaction');
    });

    test('Delete on a derived entity is one removeObject override; undo brings it back', () {
      final f = Fixture();
      addTearDown(f.dispose);
      final tree = f.hierarchy.tree!;
      f.hierarchy.expand('inst');
      f.hierarchy.select('i1:mesh');
      expect(f.hierarchy.delete(), isTrue);
      expect(f.sceneDoc.history, 1);
      expect(f.overridesOf('inst').last, {'op': 'removeObject', 'entity': 'mesh'});
      expect(tree.rows.map((r) => r.id), ['cam', 'inst', 'i1:root', 'broken', 'imported']);
      expect(f.tab.selection, ['broken'], reason: 'the row that took its place');
      f.sceneDoc.undo();
      expect(tree.rows.map((r) => r.id), ['cam', 'inst', 'i1:root', 'i1:mesh', 'broken', 'imported']);
    });

    test('Create Empty Child under a derived entity is one addObject override; the added row renames and deletes', () {
      final f = Fixture();
      addTearDown(f.dispose);
      final tree = f.hierarchy.tree!;
      f.hierarchy.expand('inst');
      expect(f.hierarchy.createEmpty(parent: 'i1:root'), isTrue);
      expect(f.sceneDoc.history, 1);
      final op = f.overridesOf('inst').last;
      expect(op['op'], 'addObject');
      expect(op['parent'], 'root');
      expect(op['name'], 'Empty');
      final local = op['entity'] as String;
      expect(local, matches(RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$')));
      expect((op['components'] as Map).values.single['type'], 'fynkel.Transform');
      final added = tree['i1:$local']!;
      expect(added.added, isTrue);
      expect(added.parent, 'i1:root');
      expect(f.tab.selection, ['i1:$local']);
      expect(f.hierarchy.renaming, 'i1:$local');
      // Rename re-issues the addObject (same target → replaced, not appended).
      expect(f.hierarchy.canRename('i1:$local'), isTrue);
      expect(f.hierarchy.rename('i1:$local', 'Lamp'), isTrue);
      expect(f.sceneDoc.history, 2);
      expect(f.overridesOf('inst').length, 2);
      expect(f.overridesOf('inst').last['name'], 'Lamp');
      expect(tree['i1:$local']!.name, 'Lamp');
      // Delete reverts the addObject rather than adding a removeObject.
      f.hierarchy.select('i1:$local');
      expect(f.hierarchy.delete(), isTrue);
      expect(f.sceneDoc.history, 3);
      expect(f.overridesOf('inst').length, 1);
      expect(tree['i1:$local'], isNull);
    });

    test('→ and ← walk into and out of an instance', () {
      final f = Fixture();
      addTearDown(f.dispose);
      f.hierarchy.select('inst');
      f.hierarchy.arrow(true);
      expect(f.hierarchy.tree!.expanded, contains('inst'));
      f.hierarchy.arrow(true);
      expect(f.tab.selection, ['i1:root']);
      expect(f.hierarchy.tree!.expanded, contains('i1:root'), reason: 'an instance opens whole');
      f.hierarchy.arrow(false);
      expect(f.hierarchy.tree!.expanded, isNot(contains('i1:root')));
      f.hierarchy.arrow(false);
      expect(f.tab.selection, ['inst']);
      f.hierarchy.arrow(false);
      expect(f.hierarchy.tree!.expanded, isNot(contains('inst')));
      f.hierarchy.select('broken');
      f.hierarchy.arrow(true);
      expect(f.hierarchy.tree!.expanded, isNot(contains('broken')), reason: 'nothing to open to');
    });
  });

  group('Inspector', () {
    test('a derived entity reads from the expansion; overridden fields are marked', () {
      final f = Fixture();
      addTearDown(f.dispose);
      f.hierarchy.expand('inst');
      f.tab.select('i1:mesh');
      final e = f.inspector.single!;
      expect(e.isDerived, isTrue);
      expect(e.anchor, 'inst');
      expect(e.name, 'Mesh');
      expect(f.inspector.canRename, isFalse);
      final light = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light');
      expect(light.isDerived, isTrue);
      expect(light.value('intensity'), 5, reason: 'the applied value');
      expect(light.isOverridden('intensity'), isTrue);
      expect(light.isOverridden('color'), isFalse);
      expect(light.first.local, 'l');
      expect(light.first.entityLocal, 'mesh');
      final t = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      expect(t.isOverridden('/position/1'), isFalse);
      // The anchor's entity carries its expansion.
      f.tab.select('inst');
      expect(f.inspector.single!.expansion!.prefab!.name, 'Crate');
      expect(f.inspector.single!.expansion!.overrides.single.entityName, 'Mesh');
    });

    test('edits on a derived component are override ops: whole keys, one transaction each', () {
      final f = Fixture();
      addTearDown(f.dispose);
      f.hierarchy.expand('inst');
      f.tab.select('i1:mesh');
      final t = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      // A nested pointer folds into its top-level key over the applied value.
      expect(f.inspector.setField(t, '/position/1', 7), isTrue);
      expect(f.sceneDoc.history, 1);
      expect(f.overridesOf('inst').last, {
        'op': 'setField',
        'entity': 'mesh',
        'component': 't',
        'field': 'position',
        'value': [0, 7, 0],
      });
      expect(f.hierarchy.tree!['i1:mesh']!.transform!.position, [0, 7, 0]);
      final t2 = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      expect(t2.isOverridden('/position/0'), isTrue);
      // A second lane replaces the same override (same target) rather than appending.
      expect(f.inspector.setField(t2, '/position/0', 2), isTrue);
      expect(f.overridesOf('inst').length, 2);
      expect(f.overridesOf('inst').last['value'], [2, 7, 0]);
      // setFields: the whole vector in one transaction.
      final t3 = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      expect(
        f.inspector.setFields(t3, {
          'scale': [2, 2, 2],
        }),
        isTrue,
      );
      expect(f.sceneDoc.history, 3);
      expect(f.overridesOf('inst').length, 3);
      // unsetField shares the target with setField: the override becomes an unset.
      final light = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light');
      expect(f.inspector.unsetField(light, 'intensity'), isTrue);
      expect(f.overridesOf('inst').first, {
        'op': 'unsetField',
        'entity': 'mesh',
        'component': 'l',
        'field': 'intensity',
      });
      expect(f.overridesOf('inst').length, 3);
      // Revert on the field drops that override.
      final light2 = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light');
      expect(light2.isOverridden('intensity'), isTrue);
      expect(f.inspector.canRevertField(light2, 'intensity'), isTrue);
      expect(f.inspector.revertField(light2, 'intensity'), isTrue);
      expect(f.overridesOf('inst').length, 2);
      expect(f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light').value('intensity'), 1);
      // Reset to defaults writes setField overrides for what differs.
      final t4 = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      expect(f.inspector.resetToDefaults(t4), isTrue);
      expect(f.hierarchy.tree!['i1:mesh']!.transform!.position, [0, 0, 0]);
      expect(f.sceneDoc.history, 6);
    });

    test('Add / Remove Component on a derived entity: add override, its revert, a removeComponent override', () {
      final f = Fixture();
      addTearDown(f.dispose);
      f.hierarchy.expand('inst');
      f.tab.select('i1:root');
      expect(f.inspector.addComponent('fynkel.Camera'), isTrue);
      var op = f.overridesOf('inst').last;
      expect(op['op'], 'addComponent');
      expect(op['entity'], 'root');
      expect(op['type'], 'fynkel.Camera');
      expect(op['data'], {'projection': 'perspective', 'fov': 60, 'near': 0.1, 'far': 1000});
      final cam = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Camera');
      expect(cam.first.added, isTrue);
      expect(cam.first.local, op['component']);
      // Removing the added component reverts the override that added it.
      expect(f.inspector.removeComponent(cam), isTrue);
      expect(f.overridesOf('inst').length, 1);
      expect(f.inspector.slots.any((s) => s.type == 'fynkel.Camera'), isFalse);
      // Removing an inherited component is a removeComponent override.
      f.tab.select('i1:mesh');
      final light = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light');
      expect(f.inspector.removeComponent(light), isTrue);
      op = f.overridesOf('inst').last;
      expect(op, {'op': 'removeComponent', 'entity': 'mesh', 'component': 'l'});
      expect(f.inspector.slots.any((s) => s.type == 'fynkel.Light'), isFalse);
      expect(f.sceneDoc.history, 3);
    });

    test('Apply to prefab: a transaction on the prefab document, then the revert — two undo steps', () {
      final f = Fixture();
      addTearDown(f.dispose);
      expect(f.inspector.cannotApply('inst'), isNull);
      f.tab.select('inst');
      expect(f.inspector.applyOverrides('inst'), isTrue);
      // The prefab took the value, in its own tab, behind the scene's.
      final prefabTab = f.tabs.byId('crate-prefab')!;
      expect(f.tabs.focused, same(f.tab));
      expect(prefabTab.dirty, isTrue);
      expect(f.prefabDoc.history, 1);
      expect((f.prefabDoc.entitiesView['mesh']!['components'] as Map)['l']['data']['intensity'], 5);
      // The instance lost its override; its derived entity still shows 5 — from the prefab now.
      expect(f.overridesOf('inst'), isEmpty);
      expect(f.sceneDoc.history, 1);
      f.hierarchy.expand('inst');
      f.tab.select('i1:mesh');
      final light = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light');
      expect(light.value('intensity'), 5);
      expect(light.isOverridden('intensity'), isFalse);
      // Undo, one document at a time.
      f.sceneDoc.undo();
      expect(f.overridesOf('inst').length, 1);
      f.prefabDoc.undo();
      expect((f.prefabDoc.entitiesView['mesh']!['components'] as Map)['l']['data']['intensity'], 1);
      // The scene's expansions followed the prefab's change.
      expect(f.tabs.prefabChangeBroadcasts, greaterThanOrEqualTo(2));
      f.tab.select('i1:mesh');
      expect(f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light').value('intensity'), 5);
      expect(f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light').isOverridden('intensity'), isTrue);
    });

    test('Apply on one field applies just that override; a refusing prefab leaves the scene untouched', () {
      final f = Fixture();
      addTearDown(f.dispose);
      f.hierarchy.expand('inst');
      f.tab.select('i1:mesh');
      final t = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      expect(f.inspector.setField(t, '/position/1', 7), isTrue);
      expect(f.overridesOf('inst').length, 2);
      final t2 = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      expect(f.inspector.cannotApplyField(t2, 'position'), isNull);
      expect(f.inspector.applyField(t2, 'position'), isTrue);
      expect(f.overridesOf('inst').length, 1, reason: 'the light override stays');
      expect(f.overridesOf('inst').single['field'], 'intensity');
      expect((f.prefabDoc.entitiesView['mesh']!['components'] as Map)['t']['data']['position'], [0, 7, 0]);
      // The prefab refuses: nothing changes on the scene.
      f.prefabDoc.reject = (_) => 'FYN-D0404: no';
      final light = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Light');
      expect(f.inspector.applyField(light, 'intensity'), isFalse);
      expect(f.inspector.errorFor(light, 'intensity'), contains('refused'));
      expect(f.overridesOf('inst').length, 1);
      expect(f.sceneDoc.history, 2);
    });

    test('Unpack is one transaction: the instance becomes plain entities; undo restores it', () {
      final f = Fixture();
      addTearDown(f.dispose);
      final tree = f.hierarchy.tree!;
      f.hierarchy.expand('inst');
      f.tab.select('i1:mesh');
      expect(f.inspector.cannotUnpack('inst'), isNull);
      expect(f.inspector.unpack('inst'), isTrue);
      expect(f.sceneDoc.history, 1);
      expect(f.sceneDoc.entitiesView['inst']!['prefab'], isNull);
      expect(tree['i1:mesh']!.isDerived, isFalse, reason: 'a document entity now');
      expect(tree['i1:mesh']!.transformId, 'i1:t', reason: 'component ids are derived too');
      expect(tree.entityCount, 6);
      expect(f.tab.selection, ['inst']);
      expect((f.sceneDoc.entitiesView['i1:mesh']!['components'] as Map)['i1:l']['data']['intensity'], 5);
      f.sceneDoc.undo();
      expect(f.sceneDoc.entitiesView['inst']!['prefab'], isNotNull);
      expect(tree.entityCount, 4);
      expect(tree['i1:mesh']?.isDerived ?? true, isTrue);
    });

    test('an imported hierarchy opens read-only and its instance takes overrides but no Apply', () {
      final f = Fixture();
      addTearDown(f.dispose);
      final x = f.tab.expansionOf('imported')!;
      expect(x.isImported, isTrue);
      expect(x.prefab!.isImported, isTrue);
      expect(x.prefab!.readOnly, isNotNull);
      expect(x.open, 'rig-src#prefab');
      f.hierarchy.expand('imported');
      expect(f.hierarchy.tree!.rows.map((r) => r.id), containsAllInOrder(['imported', 'i2:n0', 'i2:n1']));
      f.tab.select('i2:n1');
      final t = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      expect(f.inspector.setField(t, '/position/1', 3), isTrue);
      expect(f.overridesOf('imported').single['value'], [0, 3, 0]);
      final t2 = f.inspector.slots.singleWhere((s) => s.type == 'fynkel.Transform');
      expect(t2.isOverridden('position'), isTrue);
      expect(f.inspector.cannotApplyField(t2, 'position'), contains('cannot be edited'));
      expect(f.inspector.cannotApply('imported'), contains('cannot be edited'));
      expect(f.inspector.cannotUnpack('imported'), isNull);
      // Open prefab: the hierarchy in its own read-only tab, named after the source.
      expect(f.inspector.openPrefab('imported'), isTrue);
      final rigTab = f.tabs.focused!;
      expect(rigTab.isImportedHierarchy, isTrue);
      expect(rigTab.tabLabel, 'rig.glb › prefab');
      expect(rigTab.fileStem, 'rig');
      expect(rigTab.document.readOnly, contains('derived'));
      expect(rigTab.tree.roots, ['n0']);
      expect(rigTab.transaction([Command.renameObject('n0', 'X')])?.ok, isNot(true));
      expect(f.rig.history, 0);
      expect(f.tabs.openDocument('nothing#prefab'), isNull);
      expect(f.tabs.openError, contains('FYN-D0401'));
    });

    test('a prefab tab\'s edits reach the scene\'s derived entities live; closing it reverts them', () {
      final f = Fixture();
      addTearDown(f.dispose);
      f.hierarchy.expand('inst');
      expect(f.hierarchy.tree!['i1:mesh']!.name, 'Mesh');
      final prefabTab = f.tabs.openDocument('crate-prefab', focus: false)!;
      expect(f.tabs.focused, same(f.tab));
      expect(prefabTab.transaction([Command.renameObject('mesh', 'Box')])?.ok, isTrue);
      expect(f.tabs.prefabChangeBroadcasts, 1);
      expect(f.hierarchy.tree!['i1:mesh']!.name, 'Box');
      f.tabs.closeDiscarding(prefabTab);
      expect(f.hierarchy.tree!['i1:mesh']!.name, 'Mesh');
      // A foreign file change (the prefab edited outside) refreshes too.
      f.prefabDoc.apply([Command.renameObject('mesh', 'Disk')]);
      f.project.fileChangesController.add(['/fake/game/assets/prefabs/crate.prefab.json']);
    });

    test('selectionAnchor: the anchor or its derived entities, one instance only', () {
      final f = Fixture();
      addTearDown(f.dispose);
      f.hierarchy.expand('inst');
      f.tab.select('i1:mesh');
      expect(prefab.selectionAnchor(f.tab), 'inst');
      f.tab.select('inst');
      expect(prefab.selectionAnchor(f.tab), 'inst');
      f.tab.select('i1:root', toggle: true);
      expect(prefab.selectionAnchor(f.tab), 'inst');
      f.tab.select('cam', toggle: true);
      expect(prefab.selectionAnchor(f.tab), isNull);
      f.tab.select('cam');
      expect(prefab.selectionAnchor(f.tab), isNull);
    });
  });

  group('shell', () {
    late FakeProject project;
    late Preferences prefs;

    setUp(() {
      project = FakeProject(dir: '/fake/game');
      project.schemas = nativeSchemas();
      project.addDocument(cratePrefab());
      project.addDocument(importedRig());
      project.addDocument(scene());
      project.assetEntries = [
        const AssetEntry(
          id: 'crate-prefab',
          path: 'assets/prefabs/crate.prefab.json',
          name: 'crate',
          kind: 'document',
          document: 'prefab',
        ),
      ];
      prefs = Preferences.inMemory();
    });

    Future<EditorShellState> openShell(WidgetTester tester) async {
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          store: MemoryLayoutStore(),
          projects: {'/fake/game': project},
          initialProject: '/fake/game',
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      shell.openDocument('main');
      await tester.pumpAndSettle();
      return shell;
    }

    testWidgets('the prefab.* commands are enabled by the selection and run through the registry', (tester) async {
      final shell = await openShell(tester);
      final r = shell.registry;
      for (final id in [
        'prefab.applyOverrides',
        'prefab.revertOverrides',
        'prefab.unpack',
        'prefab.open',
        'prefab.selectAsset',
      ]) {
        expect(r.byId(id), isNotNull, reason: id);
        expect(r.byId(id)!.isEnabled, isFalse, reason: '$id with nothing selected');
      }
      final tab = shell.tabs!.focused!;
      tab.select('inst');
      await tester.pumpAndSettle();
      expect(r.byId('prefab.applyOverrides')!.isEnabled, isTrue);
      expect(r.byId('prefab.revertOverrides')!.isEnabled, isTrue);
      expect(r.byId('prefab.unpack')!.isEnabled, isTrue);
      expect(r.byId('prefab.open')!.isEnabled, isTrue);
      expect(r.byId('prefab.selectAsset')!.isEnabled, isTrue);
      tab.select('broken');
      await tester.pumpAndSettle();
      expect(r.byId('prefab.applyOverrides')!.isEnabled, isFalse);
      expect(r.byId('prefab.unpack')!.isEnabled, isFalse);
      expect(r.byId('prefab.open')!.isEnabled, isFalse);
      expect(r.byId('prefab.selectAsset')!.isEnabled, isTrue);
      tab.select('inst');
      await tester.pumpAndSettle();
      r.execute('prefab.selectAsset');
      await tester.pumpAndSettle();
      expect(shell.browser!.selection, contains('crate-prefab'));
      r.execute('prefab.open');
      await tester.pumpAndSettle();
      expect(shell.tabs!.focused!.document.id, 'crate-prefab');
      shell.tabs!.focus(tab);
      await tester.pumpAndSettle();
      r.execute('prefab.revertOverrides');
      await tester.pumpAndSettle();
      expect((project.fakeDocuments.last.entitiesView['inst']!['prefab'] as Map)['overrides'], isEmpty);
      r.execute('prefab.unpack');
      await tester.pumpAndSettle();
      expect(project.fakeDocuments.last.entitiesView['inst']!['prefab'], isNull);
      expect(project.fakeDocuments.last.history, 2);
    });

    testWidgets('derived rows, the context menu\'s prefab items, the read-only banner', (tester) async {
      final shell = await openShell(tester);
      final tab = shell.tabs!.focused!;
      shell.hierarchy!.expand('inst');
      tab.select('i1:mesh');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('hierarchy.name.i1:mesh')), findsOneWidget);
      expect(find.byKey(const Key('hierarchy.derived.i1:mesh')), findsOneWidget, reason: 'one overridden field');
      expect(find.byKey(const Key('hierarchy.derived.i1:root')), findsNothing);
      final name = tester.widget<Text>(find.byKey(const Key('hierarchy.name.i1:mesh')));
      expect(name.style?.fontStyle, FontStyle.italic);
      expect(name.style?.fontWeight, FontWeight.bold);
      // The context menu on a derived row offers the prefab commands.
      final panel = tester.widget<HierarchyPanel>(find.byType(HierarchyPanel));
      expect(panel.commands, isNotNull);
      await tester.tap(find.byKey(const Key('hierarchy.name.i1:mesh')), buttons: kSecondaryButton);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('hierarchy.menu.prefab.open')), findsOneWidget);
      expect(find.byKey(const Key('hierarchy.menu.prefab.unpack')), findsOneWidget);
      await tester.tap(find.byKey(const Key('hierarchy.menu.prefab.open')));
      await tester.pumpAndSettle();
      expect(shell.tabs!.focused!.document.id, 'crate-prefab');
      expect(find.byKey(const Key('hierarchy.readOnly')), findsNothing);
      // The imported hierarchy's tab shows why it is read-only.
      shell.openDocument('rig-src#prefab');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('hierarchy.readOnly')), findsOneWidget);
      expect(find.text('rig.glb › prefab'), findsOneWidget);
    });

    testWidgets('the Inspector shows bold overridden fields with Revert/Apply, and the anchor section', (tester) async {
      final shell = await openShell(tester);
      final tab = shell.tabs!.focused!;
      shell.hierarchy!.expand('inst');
      tab.select('i1:mesh');
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('prefab.derived')), findsOneWidget);
      final row = find.byKey(const ValueKey('field.fynkel.Light#0.intensity'));
      expect(row, findsOneWidget);
      expect(find.descendant(of: row, matching: find.byKey(const ValueKey('field.revert'))), findsOneWidget);
      expect(find.descendant(of: row, matching: find.byKey(const ValueKey('field.apply'))), findsOneWidget);
      final colorRow = find.byKey(const ValueKey('field.fynkel.Light#0.color'));
      expect(find.descendant(of: colorRow, matching: find.byKey(const ValueKey('field.revert'))), findsNothing);
      final nameField = tester.widget<TextField>(
        find.descendant(of: find.byKey(const ValueKey('inspector.name')), matching: find.byType(TextField)),
      );
      expect(nameField.enabled, isFalse, reason: 'named by the prefab');
      await tester.tap(find.descendant(of: row, matching: find.byKey(const ValueKey('field.revert'))));
      await tester.pumpAndSettle();
      expect((project.fakeDocuments.last.entitiesView['inst']!['prefab'] as Map)['overrides'], isEmpty);
      expect(find.descendant(of: row, matching: find.byKey(const ValueKey('field.revert'))), findsNothing);
      // The anchor.
      await tester.tap(find.byKey(const ValueKey('prefab.selectInstance')));
      await tester.pumpAndSettle();
      expect(tab.selection, ['inst']);
      expect(find.byKey(const ValueKey('prefab.title')), findsOneWidget);
      expect(find.byKey(const ValueKey('prefab.unpack')), findsOneWidget);
      expect(find.byKey(const ValueKey('prefab.open')), findsOneWidget);
      expect(find.byKey(const ValueKey('prefab.selectAsset')), findsOneWidget);
      expect(find.byKey(const ValueKey('prefab.applyAll')), findsNothing, reason: 'no overrides left');
      // A broken one.
      tab.select('broken');
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('prefab.broken')), findsOneWidget);
      expect(find.byKey(const ValueKey('prefab.diagnostic.0')), findsOneWidget);
      expect(find.byKey(const ValueKey('prefab.override.0')), findsOneWidget, reason: 'its data is kept');
      expect(tester.widget<TextButton>(find.byKey(const ValueKey('prefab.applyAll'))).enabled, isFalse);
      expect(tester.widget<TextButton>(find.byKey(const ValueKey('prefab.unpack'))).enabled, isFalse);
    });
  });
}
