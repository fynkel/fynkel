// The Inspector over the real `SessionProject` (the M4-04 gate, on copies):
// every component of the fixture's scenes and the reference game's
// playground parses into a spec with a real widget kind for every field;
// every field set to its own value through the Inspector's path saves
// byte-identically; a `minimum` violation and an unknown asset are refused
// (the editor's precheck and the crate's validation both) and leave the
// document untouched; a fresh component gets its defaults and round-trips.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/inspector/field_spec.dart';
import 'package:fynkel_editor/src/inspector/inspector_model.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';

import 'documents_session_test.dart' show mainScene, robot, crateA, floor;
import 'project_test.dart' show copyFixture, referenceGame;

Directory copyReferenceGame() {
  final dir = Directory.systemTemp.createTempSync('fynkel_editor_refgame');
  void copy(Directory from, Directory to) {
    to.createSync(recursive: true);
    for (final e in from.listSync()) {
      final name = e.uri.pathSegments.where((s) => s.isNotEmpty).last;
      if (name.startsWith('.') || name == 'build') continue;
      if (e is Directory) {
        copy(e, Directory('${to.path}/$name'));
      } else if (e is File) {
        e.copySync('${to.path}/$name');
      }
    }
  }

  copy(referenceGame, dir);
  return Directory(dir.resolveSymbolicLinksSync());
}

/// Sets every field of every component of every entity to the value it
/// already has, through the Inspector model (one selection per entity,
/// one transaction per component), and answers how many fields were set.
int rewriteEveryField(InspectorModel model, DocumentTabs tabs, String documentId) {
  final tab = tabs.openDocument(documentId)!;
  var fields = 0;
  for (final id in tab.tree.nodes.keys.toList()) {
    tab.select(id);
    for (final slot in List.of(model.slots)) {
      final data = slot.first.data;
      if (data.isEmpty) continue;
      final values = <String, Object?>{for (final e in data.entries) e.key: e.value};
      // Every field must have a spec kind the Inspector can draw — or the type has no schema at all.
      for (final key in data.keys) {
        final f = slot.spec.field(key);
        expect(f != null || slot.spec.missingSchema, isTrue, reason: '${slot.type}.$key has no spec');
        if (f != null) expect(f.kind, isNot(FieldKind.any), reason: '${slot.type}.$key falls through to JSON');
        // The precheck accepts the committed value.
        if (f != null) expect(model.precheck(f, data[key]), isNull, reason: '${slot.type}.$key: ${data[key]}');
      }
      expect(model.setFields(slot, values), isTrue, reason: '${slot.type} on $id: ${tab.lastError}');
      fields += values.length;
    }
  }
  return fields;
}

void main() {
  test('fixture project: every field of every scene component is editable and round-trips byte-exactly', () async {
    final dir = copyFixture('inspector');
    addTearDown(() => dir.deleteSync(recursive: true));
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final model = InspectorModel(tabs: tabs, project: project);
    addTearDown(model.dispose);

    // Every schema the project offers parses; native kinds are all real.
    final schemas = project.typeSchemas();
    expect(schemas.where((s) => !s.isBehaviour).length, greaterThanOrEqualTo(21));
    for (final s in schemas) {
      final spec = model.specFor(s.type);
      expect(spec.missingSchema, isFalse, reason: s.type);
      for (final f in spec.fields) {
        expect(f.kind, isNot(FieldKind.any), reason: '${s.type}.${f.name}');
      }
    }
    final spinner = model.specFor('game.Spinner');
    expect(spinner.isBehaviour, isTrue);
    expect(spinner.field('speed')!.min, 0);
    expect(spinner.field('speed')!.step, 0.1);
    expect(spinner.field('showcase')!.nullable, isTrue);
    expect(spinner.library, 'package:spinner_game/spinner.dart');
    // No behaviours.g.json in the fixture: no drift to report, no banner source.
    expect(model.hasMetadata, isFalse);

    final scenes = {
      mainScene: File('${dir.path}/assets/scenes/main.scene.json'),
      for (final f in Directory('${dir.path}/assets/scenes').listSync().whereType<File>())
        if (f.path.endsWith('playground.scene.json')) (jsonDecode(f.readAsStringSync()) as Map)['id'] as String: f,
    };
    expect(scenes.length, 2);
    for (final entry in scenes.entries) {
      final original = entry.value.readAsBytesSync();
      final fields = rewriteEveryField(model, tabs, entry.key);
      expect(fields, greaterThan(20), reason: entry.value.path);
      final tab = tabs.focused!;
      // Setting a field to its own value is a no-op for the content hash: the tab may stay clean. Save anyway and compare.
      expect(tab.save(), isTrue, reason: tab.lastError);
      expect(
        entry.value.readAsBytesSync(),
        original,
        reason: '${entry.value.path} changed bytes after setting every field to itself',
      );
    }

    // A real edit round-trips through save too: the Sun's colour, Crate B's mass.
    final tab = tabs.openDocument(mainScene)!;
    tab.select(floor);
    final collider = model.slots.singleWhere((s) => s.type == 'fynkel.Collider');
    expect(model.setField(collider, 'friction', 0.75, spec: collider.spec.field('friction')), isTrue);
    expect(model.setField(collider, 'mask', 3, spec: collider.spec.field('mask')), isTrue);
    expect(tab.save(), isTrue);
    final saved = jsonDecode(File('${dir.path}/assets/scenes/main.scene.json').readAsStringSync()) as Map;
    final floorComponents = ((saved['entities'] as Map)[floor] as Map)['components'] as Map;
    final savedCollider = floorComponents.values.singleWhere((c) => c['type'] == 'fynkel.Collider') as Map;
    expect(savedCollider['data']['friction'], 0.75);
    expect(savedCollider['data']['mask'], 3);
    expect(tab.document.validate().where((d) => d.severity == Severity.error), isEmpty);
  });

  test('fixture project: the header writes fynkel.Layer and fynkel.Tags; an undeclared one is refused', () async {
    final dir = copyFixture('inspector_layers');
    addTearDown(() => dir.deleteSync(recursive: true));
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final model = InspectorModel(tabs: tabs, project: project);
    addTearDown(model.dispose);
    expect(project.layerNames(), ['default', 'props']);
    expect(project.tagNames(), isEmpty);

    final tab = tabs.openDocument(mainScene)!;
    tab.select(robot);
    expect(model.layer, 0);
    expect(model.setLayer(1), isTrue, reason: '${model.lastError}');
    expect(model.layer, 1);
    final components = tab.document.entities([robot]).entities[robot]!['components'] as Map;
    expect(components.values.where((c) => (c as Map)['type'] == 'fynkel.Layer').single['data'], {'layer': 1});
    expect(model.slots.map((s) => s.type), isNot(contains('fynkel.Layer')));

    // The project declares no tags and no layer 3: the crate's validation refuses both.
    final revision = tab.document.revision;
    expect(model.setTag(0, true), isFalse);
    expect(model.lastError, contains('FYN-H0012'));
    expect(model.setLayer(3), isFalse);
    expect(model.lastError, contains('FYN-H0012'));
    expect(tab.document.revision, revision);
    expect(model.layer, 1);
  });

  test(
    'fixture project: a minimum violation and an unknown asset are refused and leave the document untouched',
    () async {
      final dir = copyFixture('inspector_reject');
      addTearDown(() => dir.deleteSync(recursive: true));
      final project = await SessionProject.open(dir.path, watch: false);
      addTearDown(project.close);
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final model = InspectorModel(tabs: tabs, project: project);
      addTearDown(model.dispose);
      final file = File('${dir.path}/assets/scenes/main.scene.json');
      final original = file.readAsBytesSync();

      final tab = tabs.openDocument(mainScene)!;
      tab.select(robot);
      final spinner = model.slots.singleWhere((s) => s.type == 'game.Spinner');
      final revision = tab.document.revision;
      final history = tab.document.history;

      // 1. The editor's precheck: the hint's `min` refuses before any command is sent.
      expect(model.setField(spinner, 'speed', -1, spec: spinner.spec.field('speed')), isFalse);
      expect(model.errorFor(spinner, 'speed'), contains('below the minimum 0'));
      expect(tab.document.revision, revision);
      expect(tab.document.history, history);
      expect(model.slots.singleWhere((s) => s.type == 'game.Spinner').value('speed'), 2.5);

      // 2. The crate's validation (no spec → no precheck): the schema `minimum` rejects the transaction.
      expect(model.setField(spinner, 'speed', -1), isFalse);
      final message = model.errorFor(spinner, 'speed')!;
      expect(message, contains('FYN-'));
      expect(message.toLowerCase(), anyOf(contains('minimum'), contains('rejected'), contains('nothing was applied')));
      expect(tab.document.revision, revision);
      expect(tab.document.history, history);
      expect(tab.document.dirty, isFalse);
      expect(model.slots.singleWhere((s) => s.type == 'game.Spinner').value('speed'), 2.5);
      // Also directly: the transaction is not ok and names an error.
      final a = tab.transaction([Command.setField(robot, spinner.first.componentId, 'speed', -1)])!;
      expect(a.ok, isFalse);
      expect(a.allDiagnostics.where((d) => d.severity == Severity.error), isNotEmpty);
      expect(tab.document.revision, revision);

      // 3. An unknown asset: refused editor-side with FYN-A0004 (the crate does not resolve assets at apply).
      final mesh = model.slots.singleWhere((s) => s.type == 'fynkel.MeshRenderer');
      expect(project.assetExists('c0000000-0000-4000-8000-000000000001', sub: 'mesh/Cube'), isTrue);
      expect(project.assetExists('c0000000-0000-4000-8000-0000000000ff'), isFalse);
      expect(
        model.setField(mesh, 'mesh', {'asset': 'c0000000-0000-4000-8000-0000000000ff'}, spec: mesh.spec.field('mesh')),
        isFalse,
      );
      expect(model.errorFor(mesh, 'mesh'), contains('FYN-A0004'));
      expect(
        model.setField(mesh, 'mesh', {
          'asset': 'c0000000-0000-4000-8000-000000000001',
          'sub': 'mesh/7',
        }, spec: mesh.spec.field('mesh')),
        isFalse,
      );
      expect(tab.document.revision, revision);
      expect(tab.document.dirty, isFalse);
      expect(file.readAsBytesSync(), original);

      // 4. A second MeshRenderer (the gate's finding): refused editor-side before any command, naming the entity —
      expect(model.entitiesWith('fynkel.MeshRenderer').map((e) => e.id), [robot]);
      expect(model.addComponent('fynkel.MeshRenderer'), isFalse);
      expect(model.lastError, contains('MeshRenderer is already on Robot'));
      expect(model.lastError, contains('FYN-D0204'));
      expect(tab.document.revision, revision);
      // — and, asked straight, the crate answers FYN-E0005 with the reason related; the editor shows the reason.
      final dup = tab.transaction([
        Command.addComponent(robot, {
          'type': 'fynkel.MeshRenderer',
          'version': 1,
          'data': project.componentDefaults('fynkel.MeshRenderer')!,
        }),
      ])!;
      expect(dup.ok, isFalse);
      expect(dup.diagnostics.map((d) => d.code), contains('FYN-E0005'));
      expect(dup.diagnostics.expand((d) => d.related).map((d) => d.code), contains('FYN-D0204'));
      expect(tab.lastError, contains('FYN-D0204: entity already has a fynkel.MeshRenderer component'));
      expect(tab.lastError, contains('an entity holds one component per type'));
      expect(tab.lastError, isNot(contains('would leave the document')));
      expect(tab.document.revision, revision);
      expect(file.readAsBytesSync(), original);

      // The message stays until the field's next accepted edit.
      expect(model.errorFor(spinner, 'speed'), isNotNull);
      expect(model.setField(spinner, 'speed', 3.5, spec: spinner.spec.field('speed')), isTrue);
      expect(model.errorFor(spinner, 'speed'), isNull);
      expect(model.errorFor(mesh, 'mesh'), isNotNull);
      expect(tab.document.history, history + 1);
      // The general message: gone after that accepted edit; back on a refusal; gone with the selection.
      expect(model.lastError, isNull);
      expect(model.addComponent('fynkel.MeshRenderer'), isFalse);
      expect(model.lastError, isNotNull);
      tab.select(crateA);
      expect(model.lastError, isNull);
    },
  );

  test(
    'fixture project: Add Component with defaults, Remove, a prefab override revert, all one transaction each',
    () async {
      final dir = copyFixture('inspector_add');
      addTearDown(() => dir.deleteSync(recursive: true));
      final project = await SessionProject.open(dir.path, watch: false);
      addTearDown(project.close);
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final model = InspectorModel(tabs: tabs, project: project);
      addTearDown(model.dispose);
      final tab = tabs.openDocument(mainScene)!;

      // Native defaults from the crate; a behaviour's from its schema.
      tab.setSelection([floor, crateA]);
      expect(model.slots.map((s) => s.type), ['fynkel.Transform']);
      var history = tab.document.history;
      expect(model.addComponent('fynkel.PointLight'), isTrue, reason: model.lastError);
      expect(tab.document.history, ++history);
      final light = model.slots.singleWhere((s) => s.type == 'fynkel.PointLight');
      expect(light.instances.length, 2);
      expect(light.value('color'), isA<List>());
      expect(light.isMixed('intensity'), isFalse);
      expect(model.addComponent('game.Spinner'), isTrue, reason: model.lastError);
      expect(tab.document.history, ++history);
      final spinner = model.slots.singleWhere((s) => s.type == 'game.Spinner');
      expect(spinner.value('speed'), 1.0);
      expect(spinner.value('axis'), [0.0, 1.0, 0.0]);
      expect(spinner.value('spinning'), true);
      // The types Add Component offers: authored natives plus the behaviour, searchable.
      expect(model.addableTypes('spin').map((t) => t.type), ['game.Spinner']);
      expect(model.addableTypes().map((t) => t.type), isNot(contains('fynkel.Name')));
      // Remove from both, undo brings both back.
      expect(model.removeComponent(light), isTrue);
      expect(tab.document.history, ++history);
      expect(model.slots.map((s) => s.type), isNot(contains('fynkel.PointLight')));
      expect(tab.undo(), isTrue);
      expect(model.slots.map((s) => s.type), contains('fynkel.PointLight'));
      expect(tab.save(), isTrue, reason: tab.lastError);
      expect(tab.document.validate().where((d) => d.severity == Severity.error), isEmpty);

      // The prefab instance's override list with revert: its shadows and
      // place, and the two components added to it — overrides on the
      // prefab's root, which it is (ADR-0022).
      tab.select(crateA);
      final info = model.single!.prefab!;
      expect(info.overrides.map((o) => o['op']), ['setField', 'setField', 'addComponent', 'addComponent']);
      expect(info.overrides.first['field'], 'cast_shadows');
      expect(model.revertOverride(crateA, index: 0), isTrue, reason: model.lastError);
      expect(model.single!.prefab!.overrides.length, 3);
      expect(tab.undo(), isTrue);
      expect(model.single!.prefab!.overrides.length, 4);
      expect(model.revertOverride(crateA), isTrue);
      expect(model.single!.prefab!.overrides, isEmpty);
    },
  );

  test('reference game (a copy): every behaviour has metadata, every field parses, the playground round-trips byte-exactly', () async {
    final dir = copyReferenceGame();
    addTearDown(() => dir.deleteSync(recursive: true));
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final model = InspectorModel(tabs: tabs, project: project);
    addTearDown(model.dispose);

    expect(model.hasMetadata, isTrue);
    expect(model.committedDrift, isEmpty, reason: 'the committed reference game is up to date');
    final behaviours = project.typeSchemas().where((s) => s.isBehaviour).toList();
    expect(behaviours.map((s) => s.type), containsAll(['game.Player', 'game.Gate', 'game.Switch', 'game.Collectible']));
    for (final b in behaviours) {
      final spec = model.specFor(b.type);
      expect(spec.library, startsWith('package:reference_game/'), reason: b.type);
      expect(spec.className, isNotNull);
      expect(spec.fields, isNotEmpty, reason: b.type);
      for (final f in spec.fields) {
        expect(f.kind, isNot(FieldKind.any), reason: '${b.type}.${f.name}');
      }
    }
    // A metadata `min` reaches the spec.
    expect(model.specFor('game.Collectible').field('value')!.min, 1);
    expect(model.specFor('game.Collectible').field('value')!.kind, FieldKind.integer);
    // The script path resolves into the copy (the Gate: the Player lives
    // in its prefab since c7e23f9).
    tabs.openDocument(
      (jsonDecode(File('${dir.path}/assets/scenes/playground.scene.json').readAsStringSync()) as Map)['id'] as String,
    );
    final tab = tabs.focused!;
    final gateId = tab.tree.nodes.entries.firstWhere((e) => e.value.componentTypes.contains('game.Gate')).key;
    tab.select(gateId);
    final gate = model.slots.singleWhere((s) => s.type == 'game.Gate');
    final script = model.scriptPath(gate)!;
    expect(script, startsWith(dir.path));
    expect(File(script).existsSync(), isTrue, reason: script);

    final file = File('${dir.path}/assets/scenes/playground.scene.json');
    final original = file.readAsBytesSync();
    final fields = rewriteEveryField(model, tabs, tab.document.id);
    expect(fields, greaterThan(500));
    expect(tab.save(), isTrue, reason: tab.lastError);
    expect(
      file.readAsBytesSync(),
      original,
      reason: 'playground.scene.json changed bytes after setting every field to itself',
    );
  }, timeout: const Timeout(Duration(minutes: 3)));
}
