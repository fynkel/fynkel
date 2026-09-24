// Prefab mode over the real `SessionProject` (the M4-07 gate, on a copy of
// the fixture project): Crate A's override shows on its derived entity and
// reverts; a coin instance's override applies to coin.prefab.json (two undo
// steps, one per document) and the instance unpacks into plain entities —
// each saved file reloads clean and `inspect` (what `fynkel scene inspect
// --json` prints) shows it; the Rig scene's Imported instance expands from
// `column.rig.json#prefab` read-only, takes an override and unpacks; a
// scene naming a prefab nobody has opens, shows FYN-D0401 and is saved
// byte-identically; `rig.glb#prefab` opens read-only, a texture's does not.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_model.dart';
import 'package:fynkel_editor/src/inspector/inspector_model.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';

import 'documents_session_test.dart' show crateA, mainScene, readJson;
import 'project_test.dart' show copyFixture;

const cratePrefab = 'c0000000-0000-4000-8000-000000000040';
const coinPrefab = 'c0000000-0000-4000-8000-000000000042';
const coinRoot = 'c0000000-0000-4000-8000-0000000000b3';
const coinTrigger = 'c0000000-0000-4000-8000-0000000000cf';
const rigScene = 'c0000000-0000-4000-8000-000000000034';
const importedAnchor = 'c0000000-0000-4000-8000-0000000004e8';
const columnRig = 'c0000000-0000-4000-8000-000000000050';
const rigGlb = 'c0000000-0000-4000-8000-000000000005';
const texture = 'c0000000-0000-4000-8000-000000000003';
const rusted = 'c0000000-0000-4000-8000-000000000020';

final class Session {
  Session._(this.dir, this.project, this.tabs, this.hierarchy, this.inspector);

  static Future<Session> open(String tag) async {
    final dir = copyFixture(tag);
    final project = await SessionProject.open(dir.path, watch: false);
    final tabs = DocumentTabs(project: project);
    final hierarchy = HierarchyModel(tabs: tabs, project: project);
    final inspector = InspectorModel(tabs: tabs, project: project);
    final s = Session._(dir, project, tabs, hierarchy, inspector);
    addTearDown(s.close);
    return s;
  }

  final Directory dir;
  final SessionProject project;
  final DocumentTabs tabs;
  final HierarchyModel hierarchy;
  final InspectorModel inspector;

  void close() {
    inspector.dispose();
    hierarchy.dispose();
    tabs.dispose();
    project.close();
    dir.deleteSync(recursive: true);
  }

  String file(String rel) => '${dir.path}/$rel';

  /// The raw override list of [anchor] as the document holds it — the same
  /// bytes `fynkel scene inspect` counts. Also checks that count.
  List<Map<String, Object?>> inspectedOverrides(String documentId, String anchor) {
    final doc = tabs.byId(documentId)!.document;
    final e = doc.entities([anchor]).entities[anchor];
    if (e == null) throw StateError('no entity $anchor in $documentId');
    final p = e['prefab'];
    final list = [
      for (final o in (p is Map ? p['overrides'] as List? : null) ?? const [])
        if (o is Map) Map<String, Object?>.from(o),
    ];
    expect(_inspectedCount(doc.inspect()['roots'], anchor), list.length, reason: 'inspect counts the overrides');
    return list;
  }

  static int? _inspectedCount(Object? entities, String anchor) {
    if (entities is! List) return null;
    for (final e in entities) {
      if (e is! Map) continue;
      if (e['id'] == anchor) return (e['prefab'] as Map?)?['overrides'] as int?;
      final inner = _inspectedCount(e['children'], anchor);
      if (inner != null) return inner;
    }
    return null;
  }
}

ComponentSlot slotOf(InspectorModel m, String type) => m.slots.singleWhere((s) => s.type == type);

void main() {
  test('Crate A: the override shows bold on the derived entity, reverts, and the saved scene reloads clean', () async {
    final s = await Session.open('prefab_crate');
    final tab = s.tabs.openDocument(mainScene)!;
    final x = tab.expansionOf(crateA)!;
    expect(x.expanded, isTrue);
    expect(x.prefab!.name, 'Crate');
    expect(x.prefab!.kind, 'prefab');
    expect(x.prefab!.readOnly, isNull);
    expect(x.entities.length, 1);
    final derived = x.entities.single;
    expect(derived.parent, crateA);
    expect(derived.local, 'c0000000-0000-4000-8000-0000000000b1');
    expect(x.overrides.single.entityId, derived.id);
    expect(x.overrides.single.componentType, 'fynkel.MeshRenderer');
    // The Hierarchy shows it under the anchor; the Inspector marks the field.
    s.hierarchy.expand(crateA);
    expect(tab.tree.rows.map((r) => r.id), contains(derived.id));
    expect(tab.tree[derived.id]!.overriddenFields, 1);
    tab.select(derived.id);
    final mesh = slotOf(s.inspector, 'fynkel.MeshRenderer');
    expect(mesh.isDerived, isTrue);
    expect(mesh.value('cast_shadows'), false);
    expect(mesh.isOverridden('cast_shadows'), isTrue);
    expect(mesh.isOverridden('mesh'), isFalse);
    // Revert override: one transaction; the derived entity shows the prefab's value.
    expect(s.inspector.revertField(mesh, 'cast_shadows'), isTrue);
    expect(tab.document.history, 1);
    expect(s.inspectedOverrides(mainScene, crateA), isEmpty);
    expect(slotOf(s.inspector, 'fynkel.MeshRenderer').value('cast_shadows'), true);
    expect(slotOf(s.inspector, 'fynkel.MeshRenderer').isOverridden('cast_shadows'), isFalse);
    // A new override on a derived field: whole key, and it round-trips through save.
    final t = slotOf(s.inspector, 'fynkel.Transform');
    expect(s.inspector.setField(t, '/position/1', 2.5), isTrue);
    final o = s.inspectedOverrides(mainScene, crateA).single;
    expect(o['field'], 'position');
    expect(o['value'], [0, 2.5, 0]);
    expect(tab.save(), isTrue);
    final saved = readJson(s.file('assets/scenes/main.scene.json'));
    final anchor = (saved['entities'] as Map)[crateA] as Map;
    expect((anchor['prefab'] as Map)['overrides'], [
      {
        'op': 'setField',
        'entity': 'c0000000-0000-4000-8000-0000000000b1',
        'component': 'c0000000-0000-4000-8000-0000000000c2',
        'field': 'position',
        'value': [0, 2.5, 0],
      },
    ]);
    final again = await SessionProject.open(s.dir.path, watch: false);
    addTearDown(again.close);
    expect(again.diagnostics.where((d) => d.severity == Severity.error), isEmpty);
  });

  test('coin: Apply to prefab writes coin.prefab.json (two undo steps); Unpack makes plain entities', () async {
    final s = await Session.open('prefab_coin');
    final tab = s.tabs.openDocument(mainScene)!;
    expect(s.hierarchy.instantiatePrefab({'asset': coinPrefab}, name: 'Coin'), isTrue);
    final anchor = tab.selection.single;
    expect(tab.document.history, 1);
    // Every object has a Transform, the instance dropped on the Hierarchy too
    // (the user's gate finding, 2026-09-24) — and the Inspector keeps it.
    final anchorComponents =
        (tab.document.entities([anchor]).entities[anchor]!['components'] as Map).values.cast<Map>();
    expect(anchorComponents.map((c) => c['type']), ['fynkel.Transform']);
    expect(s.inspector.removeComponent(slotOf(s.inspector, 'fynkel.Transform')), isFalse);
    expect(s.inspector.lastError, contains('cannot be removed'));
    expect(tab.document.history, 1);
    final x = tab.expansionOf(anchor)!;
    expect(x.prefab!.name, 'Coin');
    final coin = x.byLocal(coinRoot)!;
    expect(coin.components.values.map((c) => c.type), contains('fynkel.TriggerZone'));
    // An override on the derived TriggerZone's event.
    tab.select(coin.id);
    final trigger = slotOf(s.inspector, 'fynkel.TriggerZone');
    expect(trigger.value('event'), 'collect');
    expect(s.inspector.setField(trigger, 'event', 'pickup'), isTrue);
    expect(tab.document.history, 2);
    expect(s.inspectedOverrides(mainScene, anchor).single['value'], 'pickup');
    expect(slotOf(s.inspector, 'fynkel.TriggerZone').isOverridden('event'), isTrue);
    // Apply to prefab: coin.prefab.json's tab opens behind, takes the value, the instance loses the override.
    expect(s.inspector.cannotApply(anchor), isNull);
    expect(s.inspector.applyOverrides(anchor), isTrue);
    final prefabTab = s.tabs.byId(coinPrefab)!;
    expect(s.tabs.focused, same(tab));
    expect(prefabTab.document.history, 1);
    expect(prefabTab.dirty, isTrue);
    expect(tab.document.history, 3);
    expect(s.inspectedOverrides(mainScene, anchor), isEmpty);
    Object? prefabEvent() =>
        (prefabTab.document.entities([coinRoot]).entities[coinRoot]!['components']
            as Map)[coinTrigger]['data']['event'];
    expect(prefabEvent(), 'pickup');
    // The instance shows the prefab's live (unsaved) value, unmarked.
    tab.select(coin.id);
    expect(slotOf(s.inspector, 'fynkel.TriggerZone').value('event'), 'pickup');
    expect(slotOf(s.inspector, 'fynkel.TriggerZone').isOverridden('event'), isFalse);
    // Undo, one document at a time, then redo both and save both.
    expect(tab.undo(), isTrue);
    expect(s.inspectedOverrides(mainScene, anchor).single['value'], 'pickup');
    expect(prefabTab.undo(), isTrue);
    expect(prefabEvent(), 'collect');
    expect(prefabTab.redo(), isTrue);
    expect(tab.redo(), isTrue);
    expect(prefabTab.save(), isTrue);
    expect(tab.save(), isTrue);
    final savedPrefab = readJson(s.file('assets/prefabs/coin.prefab.json'));
    expect(
      (((savedPrefab['entities'] as Map)[coinRoot] as Map)['components'] as Map)[coinTrigger]['data']['event'],
      'pickup',
    );
    // Unpack: the instance becomes plain entities under the anchor, one transaction.
    tab.select(anchor);
    expect(s.inspector.cannotUnpack(anchor), isNull);
    final instancesBefore = tab.document.inspect()['instanceCount'] as int;
    expect(s.inspector.unpack(anchor), isTrue);
    expect(tab.document.history, 4);
    expect(tab.tree[coin.id]!.isDerived, isFalse);
    expect(tab.tree[coin.id]!.parent, anchor);
    expect(tab.tree.entityCount, greaterThan(6));
    expect(tab.document.inspect()['instanceCount'], instancesBefore - 1, reason: 'inspect sees one instance fewer');
    expect(tab.document.entities([anchor]).entities[anchor]!.containsKey('prefab'), isFalse);
    expect(tab.save(), isTrue);
    final savedScene = readJson(s.file('assets/scenes/main.scene.json'));
    final e = (savedScene['entities'] as Map)[coin.id] as Map;
    expect(e['parent'], anchor);
    expect((e['components'] as Map).values.map((c) => (c as Map)['type']), contains('fynkel.TriggerZone'));
    expect(((savedScene['entities'] as Map)[anchor] as Map).containsKey('prefab'), isFalse);
    final again = await SessionProject.open(s.dir.path, watch: false);
    addTearDown(again.close);
    expect(again.diagnostics.where((d) => d.severity == Severity.error), isEmpty);
    expect(tab.undo(), isTrue);
    expect(tab.tree[coin.id]!.isDerived, isTrue);
  });

  test(
    'Imported: the rig document\'s hierarchy expands read-only, takes overrides, unpacks; Apply is refused',
    () async {
      final s = await Session.open('prefab_rig');
      final tab = s.tabs.openDocument(rigScene)!;
      final x = tab.expansionOf(importedAnchor)!;
      expect(x.isImported, isTrue);
      expect(x.sub, 'prefab');
      expect(x.prefab!.kind, 'rig');
      expect(x.prefab!.isImported, isTrue);
      expect(x.prefab!.readOnly, isNotNull);
      expect(x.open, '$columnRig#prefab');
      expect(x.entities.length, greaterThan(1));
      final overridden = x.overrides.single;
      expect(overridden.componentType, 'fynkel.MeshRenderer');
      expect(overridden.entityId, isNotNull);
      expect(s.inspector.cannotApply(importedAnchor), contains('cannot be edited'));
      // The derived entity's material shows the override; the rest is inherited.
      s.hierarchy.expand(importedAnchor);
      tab.select(overridden.entityId!);
      final mesh = slotOf(s.inspector, 'fynkel.MeshRenderer');
      expect(mesh.isOverridden('material'), isTrue);
      expect((mesh.value('material') as Map)['asset'], rusted);
      expect(s.inspector.cannotApplyField(mesh, 'material'), contains('cannot be edited'));
      // Another override, saved, reloads clean.
      final t = slotOf(s.inspector, 'fynkel.Transform');
      expect(s.inspector.setField(t, '/scale/0', 2), isTrue);
      expect(s.inspectedOverrides(rigScene, importedAnchor).length, 2);
      expect(tab.save(), isTrue);
      final again = await SessionProject.open(s.dir.path, watch: false);
      addTearDown(again.close);
      expect(again.diagnostics.where((d) => d.severity == Severity.error), isEmpty);
      // Open prefab: the hierarchy in a read-only tab named after the rig file.
      expect(s.inspector.openPrefab(importedAnchor), isTrue);
      final rigTab = s.tabs.focused!;
      expect(rigTab.isImportedHierarchy, isTrue);
      expect(rigTab.document.readOnly, isNotNull);
      expect(rigTab.tabLabel, 'column.rig.json › prefab');
      expect(rigTab.tree.entityCount, x.entities.length);
      expect(rigTab.transaction([Command.renameObject(rigTab.tree.roots.first, 'X')])?.ok, isNot(true));
      // Unpack the imported instance: plain entities under the derived ids.
      s.tabs.focus(tab);
      expect(s.inspector.unpack(importedAnchor), isTrue);
      expect(tab.tree[overridden.entityId!]!.isDerived, isFalse);
      expect(tab.save(), isTrue);
      final saved = readJson(s.file('assets/scenes/rig.scene.json'));
      expect(((saved['entities'] as Map)[importedAnchor] as Map).containsKey('prefab'), isFalse);
      expect((saved['entities'] as Map).containsKey(overridden.entityId), isTrue);
    },
  );

  test('a missing prefab opens, is shown with FYN-D0401, and the file is never rewritten', () async {
    final s = await Session.open('prefab_broken');
    final path = s.file('assets/scenes/main.scene.json');
    final text = File(path).readAsStringSync();
    final json = jsonDecode(text) as Map<String, Object?>;
    final anchor = ((json['entities'] as Map)[crateA] as Map);
    ((anchor['prefab'] as Map)['asset'] as Map)['asset'] = 'c0000000-0000-4000-8000-0000000000ff';
    File(path).writeAsStringSync('${const JsonEncoder.withIndent('  ').convert(json)}\n');
    final before = File(path).readAsStringSync();
    final project = await SessionProject.open(s.dir.path, watch: false);
    addTearDown(project.close);
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final inspector = InspectorModel(tabs: tabs, project: project);
    addTearDown(inspector.dispose);
    final tab = tabs.openDocument(mainScene)!;
    final x = tab.expansionOf(crateA)!;
    expect(x.expanded, isFalse);
    expect(x.prefab, isNull);
    expect(x.diagnostics.map((d) => d.code), contains('FYN-D0401'));
    expect(x.overrides.single.field, 'cast_shadows', reason: 'the instance keeps its data');
    expect(tab.tree.childrenOf(crateA), isEmpty);
    tab.select(crateA);
    expect(inspector.single!.expansion!.diagnostics, isNotEmpty);
    expect(inspector.cannotApply(crateA), contains('missing'));
    expect(inspector.cannotUnpack(crateA), isNotNull);
    expect(tab.dirty, isFalse);
    expect(File(path).readAsStringSync(), before);
    // Revert override still works on it (the override list is the instance's own).
    expect(inspector.revertOverride(crateA), isTrue);
    expect(tab.expansionOf(crateA)!.overrides, isEmpty);
    expect(tab.undo(), isTrue);
    expect(File(path).readAsStringSync(), before);
  });

  test('a model source\'s #prefab opens read-only; a texture has none', () async {
    final s = await Session.open('prefab_glb');
    final tab = s.tabs.openDocument('$rigGlb#prefab')!;
    expect(tab.isImportedHierarchy, isTrue);
    expect(tab.kind, 'prefab');
    expect(tab.document.readOnly, isNotNull);
    expect(tab.tabLabel, 'rig.glb › prefab');
    expect(tab.tree.entityCount, greaterThan(0));
    expect(tab.tree.roots, isNotEmpty);
    expect(s.tabs.openDocument('$rigGlb#prefab'), same(tab), reason: 'opened once');
    expect(s.tabs.openDocument('$texture#prefab'), isNull);
    expect(s.tabs.openError, contains('FYN-D0401'));
  });

  test(
    'the active checkbox: a scene object writes `active`, a derived one a setActive override that reverts',
    () async {
      final s = await Session.open('active_checkbox');
      final tab = s.tabs.openDocument(mainScene)!;
      const robot = 'c0000000-0000-4000-8000-0000000000e1';

      // A scene object: one undo step, `"active": false` in the document, greyed.
      tab.select(robot);
      expect(s.inspector.active, isTrue);
      expect(s.inspector.setActive(false), isTrue, reason: '${s.inspector.lastError}');
      expect(tab.document.entities([robot]).entities[robot]!['active'], false);
      expect(s.inspector.active, isFalse);
      expect(tab.tree[robot]!.active, isFalse);
      expect(tab.tree.activeInHierarchy(robot), isFalse);
      expect(tab.undo(), isTrue);
      expect(tab.document.entities([robot]).entities[robot]!.containsKey('active'), isFalse);
      expect(s.inspector.active, isTrue);

      // Crate A off: its derived child is greyed with it but keeps its own flag.
      s.hierarchy.expand(crateA);
      final derived = tab.expansionOf(crateA)!.entities.single;
      tab.select(crateA);
      expect(s.inspector.setActive(false), isTrue);
      expect(tab.tree[derived.id]!.active, isTrue);
      expect(tab.tree.activeInHierarchy(derived.id), isFalse);
      expect(tab.undo(), isTrue);

      // The derived child: an override on the anchor, marked, and Revert drops it.
      final before = s.inspectedOverrides(mainScene, crateA).length;
      tab.select(derived.id);
      expect(s.inspector.active, isTrue);
      expect(s.inspector.activeOverridden, isFalse);
      expect(s.inspector.setActive(false), isTrue, reason: '${s.inspector.lastError}');
      final overrides = s.inspectedOverrides(mainScene, crateA);
      expect(overrides, hasLength(before + 1));
      expect(overrides.last, {'op': 'setActive', 'entity': derived.local, 'value': false});
      expect(s.inspector.active, isFalse);
      expect(s.inspector.activeOverridden, isTrue);
      expect(tab.tree[derived.id]!.active, isFalse);
      expect(s.inspector.revertActive(), isTrue, reason: '${s.inspector.lastError}');
      expect(s.inspectedOverrides(mainScene, crateA), hasLength(before));
      expect(s.inspector.active, isTrue);
      expect(s.inspector.activeOverridden, isFalse);
    },
  );

  test('Hierarchy → Project: an object and its child become a prefab, the scene holds an instance in its place', () async {
    final s = await Session.open('prefab_create');
    final tab = s.tabs.openDocument(mainScene)!;
    // A lamp with a bulb, moved off the origin.
    expect(s.hierarchy.createEmpty(underSelection: false), isTrue);
    final lamp = tab.selection.single;
    expect(s.hierarchy.rename(lamp, 'Lamp'), isTrue);
    expect(s.hierarchy.createWith('fynkel.PointLight', parent: lamp), isTrue);
    final bulb = tab.selection.single;
    tab.select(lamp);
    expect(s.inspector.setField(slotOf(s.inspector, 'fynkel.Transform'), 'position', [3.0, 1.0, -2.0]), isTrue);
    final history = tab.document.history;

    // Refused: a prefab instance, a child of one.
    expect(s.hierarchy.cannotCreatePrefab([crateA]), contains('prefab instance'));
    expect(s.hierarchy.cannotCreatePrefab([lamp, bulb]), contains('one object'));
    expect(s.hierarchy.cannotCreatePrefab([lamp]), isNull);

    final id = s.hierarchy.createPrefab(lamp, 'prefabs')!;
    final file = File(s.file('assets/prefabs/Lamp.prefab.json'));
    expect(file.existsSync(), isTrue);
    final prefab = jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
    expect(prefab['id'], id);
    expect(prefab['name'], 'Lamp');
    expect(prefab['roots'], [lamp]);
    final entities = prefab['entities'] as Map<String, Object?>;
    expect(entities.keys, unorderedEquals([lamp, bulb]));
    Map<String, Object?> componentOf(Map<String, Object?> e, String type) =>
        ((e['components'] as Map).values.cast<Map<String, Object?>>()).firstWhere((c) => c['type'] == type);
    // The root starts at the origin; the child keeps its components.
    expect(componentOf(entities[lamp] as Map<String, Object?>, 'fynkel.Transform')['data'], containsPair('position', [0.0, 0.0, 0.0]));
    expect(componentOf(entities[bulb] as Map<String, Object?>, 'fynkel.PointLight'), isNotNull);

    // The scene: one transaction, the object gone, an instance at its place with its Transform.
    expect(tab.document.history, history + 1);
    final anchor = tab.selection.single;
    final scene = tab.document.entities([lamp, anchor]).entities;
    expect(scene[lamp], isNull);
    expect((scene[anchor]!['prefab'] as Map)['asset'], {'asset': id});
    expect(scene[anchor]!['name'], 'Lamp');
    expect(componentOf(scene[anchor]!, 'fynkel.Transform')['data'], containsPair('position', [3.0, 1.0, -2.0]));
    expect(tab.expansionOf(anchor)!.prefab!.name, 'Lamp');
    expect(tab.document.validate().where((d) => d.severity == Severity.error), isEmpty);

    // ⌘Z brings the object back; the prefab file stays, as in Unity.
    expect(tab.undo(), isTrue);
    expect(tab.document.entities([lamp]).entities[lamp], isNotNull);
    expect(file.existsSync(), isTrue);

    // A second drag of the same kind of object is numbered.
    expect(s.hierarchy.createEmpty(underSelection: false), isTrue);
    final other = tab.selection.single;
    expect(s.hierarchy.rename(other, 'Lamp'), isTrue);
    s.hierarchy.createPrefab(other, 'prefabs');
    expect(File(s.file('assets/prefabs/Lamp 1.prefab.json')).existsSync(), isTrue);

  });
}
