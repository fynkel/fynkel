// The Inspector over the fakes (M4-04): the schemas parse into specs with
// the ADR-0018 hints (every native component schema in the repo, the
// editor's native-hint table, a behaviour with metadata), the model pairs
// a multi-selection into slots with mixed values and writes one
// transaction per edit, a rejected value keeps its message and leaves the
// document untouched, Add / Remove Component, prefab override revert,
// committed drift, the CLI check, the maths, and the widgets: a drag on a
// number is one transaction, an enum menu, a checkbox, the asset picker.
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Command;
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/documents/open_document.dart';
import 'package:fynkel_editor/src/inspector/codegen_check.dart';
import 'package:fynkel_editor/src/inspector/field_spec.dart';
import 'package:fynkel_editor/src/inspector/field_widgets.dart';
import 'package:fynkel_editor/src/inspector/inspector_math.dart';
import 'package:fynkel_editor/src/inspector/inspector_model.dart';
import 'package:fynkel_editor/src/inspector/inspector_panel.dart';
import 'package:fynkel_editor/src/inspector/native_hints.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';

import 'support/fakes.dart';

final componentSchemas = Directory('../../schemas/components').absolute;

/// Every `schemas/components/*.schema.json` as a `TypeSchema`.
List<TypeSchema> nativeSchemas() {
  final out = <TypeSchema>[];
  for (final f in componentSchemas.listSync().whereType<File>()) {
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
  out.sort((a, b) => a.type.compareTo(b.type));
  return out;
}

Map<String, Object?> spinnerSchema() => {
  'title': 'game.Spinner',
  'description': 'Rotates its object.',
  'type': 'object',
  'properties': {
    'axis': {
      'type': 'array',
      'items': {'type': 'number'},
      'minItems': 3,
      'maxItems': 3,
      'default': [0.0, 1.0, 0.0],
      'x-fynkel': {'type': 'vec3', 'group': 'Motion', 'order': 2},
    },
    'showcase': {
      'anyOf': [
        {r'$ref': 'https://fynkel.dev/schemas/envelope.schema.json#/\$defs/assetRef'},
        {'type': 'null'},
      ],
      'x-fynkel': {'type': 'assetRef', 'nullable': true, 'kind': 'asset(mesh)', 'header': 'Presentation'},
    },
    'speed': {
      'type': 'number',
      'minimum': 0,
      'default': 1.0,
      'x-fynkel': {
        'type': 'number',
        'tooltip': 'Angular speed',
        'min': 0,
        'step': 0.1,
        'group': 'Motion',
        'order': 1,
        'unit': 'rad/s',
        'showIf': 'spinning',
      },
    },
    'spinning': {
      'type': 'boolean',
      'default': true,
      'x-fynkel': {'type': 'boolean', 'label': 'Spinning', 'group': 'Motion', 'order': 0},
    },
    'mode': {
      'type': 'string',
      'enum': ['slow', 'fast'],
      'default': 'slow',
      'x-fynkel': {'type': 'string', 'enum': 'game.Mode'},
    },
  },
  'additionalProperties': false,
  'x-fynkel': {
    'kind': 'behaviour',
    'id': 'game.Spinner',
    'version': 1,
    'dart': 'package:spinner_game/spinner.dart#Spinner',
    'icon': 'rotate_right',
    'category': 'Motion',
  },
};

Map<String, Object?> spinnerMetadata() => {
  'package': 'spinner_game',
  'behaviours': [
    {
      'id': 'game.Spinner',
      'version': 1,
      'class': 'Spinner',
      'library': 'package:spinner_game/spinner.dart',
      'description': 'Rotates its object.',
      'icon': 'rotate_right',
      'category': 'Motion',
      'fields': [
        {
          'name': 'speed',
          'type': 'number',
          'nullable': false,
          'default': 1.0,
          'min': 0,
          'step': 0.1,
          'unit': 'rad/s',
          'showIf': 'spinning',
        },
      ],
      'schema': spinnerSchema(),
    },
  ],
};

const meshId = 'a0000000-0000-4000-8000-000000000001';
const materialId = 'a0000000-0000-4000-8000-000000000020';
const toneId = 'a0000000-0000-4000-8000-000000000006';

FakeProject inspectorProject(Directory dir) {
  final project = FakeProject(dir: dir.path);
  project.schemas = [
    ...nativeSchemas(),
    TypeSchema(
      type: 'game.Spinner',
      isBehaviour: true,
      authored: true,
      allowed: true,
      schema: spinnerSchema(),
      version: 1,
      dart: 'package:spinner_game/spinner.dart#Spinner',
    ),
  ];
  project.metadata = spinnerMetadata();
  project.assetEntries = [
    const AssetEntry(
      id: meshId,
      path: 'assets/models/robot.glb',
      name: 'robot',
      kind: 'source',
      importer: 'fynkel.gltf',
      subAssets: {'mesh/0': 'sub-mesh-0', 'anim/0': 'sub-anim-0'},
    ),
    const AssetEntry(
      id: materialId,
      path: 'assets/materials/steel.material.json',
      name: 'steel',
      kind: 'document',
      document: 'material',
    ),
    // An audio source is referenced whole (no sub-asset): the `sound` kind.
    const AssetEntry(id: toneId, path: 'assets/audio/tone.wav', name: 'tone', kind: 'source', importer: 'fynkel.audio'),
  ];
  project.defaults = {
    ...project.defaults,
    'fynkel.DirectionalLight': {
      'color': [1, 1, 1],
      'intensity': 100000.0,
      'cast_shadows': true,
    },
  };
  return project;
}

/// A scene with a camera, two crates (same components, different values) and a prefab instance.
FakeDocument scene() => FakeDocument(
  path: '/fake/main.scene.json',
  id: 'main',
  name: 'Main',
  roots: ['cam', 'a', 'b', 'inst'],
  entities: {
    'cam': FakeDocument.entity(
      'Camera',
      position: [0, 2, 6],
      components: {
        'c': {
          'type': 'fynkel.Camera',
          'version': 1,
          'data': {
            'projection': 'perspective',
            'fov_y': 1.0,
            'ortho_height': 10.0,
            'near': 0.1,
            'far': 100.0,
            'priority': 0,
            'exposure_mode': 'auto',
            'exposure': 12.0,
          },
        },
      },
    ),
    'a': FakeDocument.entity(
      'Crate A',
      position: [2, 0, 0],
      components: {
        'm': {
          'type': 'fynkel.MeshRenderer',
          'version': 1,
          'data': {
            'mesh': {'asset': meshId, 'sub': 'mesh/0'},
            'material': {'asset': materialId},
            'cast_shadows': true,
            'receive_shadows': true,
            'visible': true,
          },
        },
        's': {
          'type': 'game.Spinner',
          'version': 1,
          'data': {
            'speed': 2.5,
            'axis': [0, 1, 0],
            'spinning': true,
          },
        },
      },
    ),
    'b': FakeDocument.entity(
      'Crate B',
      position: [-2, 0, 0],
      components: {
        'm': {
          'type': 'fynkel.MeshRenderer',
          'version': 1,
          'data': {
            'mesh': {'asset': meshId, 'sub': 'mesh/0'},
            'material': {'asset': materialId},
            'cast_shadows': false,
            'receive_shadows': true,
            'visible': true,
          },
        },
        's': {
          'type': 'game.Spinner',
          'version': 1,
          'data': {
            'speed': 1.0,
            'axis': [0, 1, 0],
            'spinning': true,
          },
        },
      },
    ),
    'inst': FakeDocument.entity(
      'Instance',
      prefab: {
        'asset': {'asset': 'p0000000-0000-4000-8000-000000000040'},
        'instance': 'i0000000-0000-4000-8000-0000000000f1',
        'overrides': [
          {'op': 'setField', 'entity': 'b1', 'component': 'c3', 'field': 'cast_shadows', 'value': false},
          {'op': 'setField', 'entity': 'b1', 'component': 'c3', 'field': 'visible', 'value': false},
        ],
      },
    ),
  },
);

({FakeProject project, FakeDocument doc, DocumentTabs tabs, InspectorModel model}) rig({
  CodegenCheck? codegen,
  Future<String?> Function(String, {int? line, int? column})? openInIde,
}) {
  final dir = fakeProjectDir('inspector');
  addTearDown(() => dir.deleteSync(recursive: true));
  final project = inspectorProject(dir);
  final doc = scene();
  project.fakeDocuments.add(doc);
  final tabs = DocumentTabs(project: project);
  addTearDown(tabs.dispose);
  final model = InspectorModel(tabs: tabs, project: project, codegen: codegen, openInIde: openInIde);
  addTearDown(model.dispose);
  return (project: project, doc: doc, tabs: tabs, model: model);
}

void main() {
  group('specs', () {
    test('every native component schema parses with a real kind for every field, and the hint table names only real fields', () {
      final schemas = nativeSchemas();
      expect(schemas.length, greaterThanOrEqualTo(21));
      final specs = {for (final s in schemas) s.type: parseComponentSpec(s.type, s.schema, authored: s.authored)};
      for (final spec in specs.values) {
        expect(spec.missingSchema, isFalse);
        for (final f in spec.fields) {
          expect(f.kind, isNot(FieldKind.any), reason: '${spec.type}.${f.name} fell through to JSON');
        }
      }
      // The table only names types and fields the schemas have.
      for (final entry in nativeFieldHints.entries) {
        final spec = specs[entry.key];
        expect(spec, isNotNull, reason: 'native_hints names ${entry.key}, which has no schema');
        for (final field in entry.value.keys) {
          expect(
            spec!.field(field),
            isNotNull,
            reason: 'native_hints names ${entry.key}.$field, which the schema lacks',
          );
        }
      }
      // Kinds the structure and the table decide.
      final transform = specs['fynkel.Transform']!;
      expect(transform.field('position')!.kind, FieldKind.vec3);
      expect(transform.field('rotation')!.kind, FieldKind.quaternion);
      final camera = specs['fynkel.Camera']!;
      expect(camera.field('projection')!.kind, FieldKind.enumeration);
      expect(camera.field('projection')!.enumValues, containsAll(['perspective', 'orthographic']));
      expect(camera.field('fov_y')!.angle, isTrue);
      expect(camera.field('fov_y')!.label, 'Field of view');
      expect(camera.field('fov_y')!.group, 'Lens');
      expect(camera.field('priority')!.kind, FieldKind.integer);
      final light = specs['fynkel.DirectionalLight']!;
      expect(light.field('color')!.kind, FieldKind.color);
      expect(light.field('intensity')!.unit, 'lux');
      final mesh = specs['fynkel.MeshRenderer']!;
      expect(mesh.field('mesh')!.kind, FieldKind.assetRef);
      expect(mesh.field('mesh')!.assetKind, 'mesh');
      expect(mesh.field('material')!.assetKind, 'material');
      final collider = specs['fynkel.Collider']!;
      expect(collider.field('mask')!.kind, FieldKind.layerMask);
      expect(collider.field('layer'), isNull);
      expect(camera.field('culling_mask')!.kind, FieldKind.layerMask);
      expect(collider.field('mesh')!.nullable, isTrue);
      expect(collider.field('restitution')!.slider, isTrue);
      expect(specs['fynkel.Skin']!.field('skeleton')!.kind, FieldKind.entity);
      expect(specs['fynkel.Name']!.authored, isFalse);
      // Order: the hints put fields in their stated order.
      expect(camera.fields.map((f) => f.name).take(3), ['projection', 'fov_y', 'ortho_height']);
      expect(camera.groups, ['Lens', 'Clipping', 'Output', 'Exposure']);
    });

    test(
      'the audio components: the clip is a whole-source `sound` asset, volume a slider, the picker offers the source',
      () {
        final specs = {
          for (final s in nativeSchemas()) s.type: parseComponentSpec(s.type, s.schema, authored: s.authored),
        };
        final source = specs['fynkel.AudioSource']!;
        expect(source.field('clip')!.kind, FieldKind.assetRef);
        expect(source.field('clip')!.assetKind, 'sound');
        expect(source.field('volume')!.kind, FieldKind.number);
        expect(source.field('volume')!.slider, isTrue);
        expect(source.field('volume')!.max, 1);
        expect(source.field('pitch')!.min, 0.05);
        expect(source.field('bus')!.kind, FieldKind.enumeration);
        expect(source.field('bus')!.enumValues, ['master', 'music', 'sfx']);
        expect(source.field('priority')!.kind, FieldKind.integer);
        expect(source.field('looping')!.group, 'Playback');
        expect(specs['fynkel.AudioListener']!.field('volume')!.slider, isTrue);
        const tone = AssetEntry(
          id: toneId,
          path: 'assets/audio/tone.wav',
          name: 'tone',
          kind: 'source',
          importer: 'fynkel.audio',
        );
        expect(tone.wholeKind, 'sound');
        expect(tone.offeredKinds, {'sound'});
        const glb = AssetEntry(
          id: meshId,
          path: 'a.glb',
          name: 'a',
          kind: 'source',
          importer: 'fynkel.gltf',
          subAssets: {'mesh/0': 'x'},
        );
        expect(glb.wholeKind, isNull);
        expect(glb.offeredKinds, {'mesh'});
        const png = AssetEntry(id: 'i', path: 'a.png', name: 'a', kind: 'source', importer: 'fynkel.image');
        expect(png.wholeKind, 'texture', reason: 'a material names the image itself');
        expect(png.offeredKinds, {'texture'});
      },
    );

    test('a behaviour schema with metadata yields the hints, the library, defaults and show-if', () {
      final spec = parseComponentSpec(
        'game.Spinner',
        spinnerSchema(),
        metadata: (spinnerMetadata()['behaviours'] as List).first as Map<String, Object?>,
      );
      expect(spec.isBehaviour, isTrue);
      expect(spec.library, 'package:spinner_game/spinner.dart');
      expect(spec.className, 'Spinner');
      expect(spec.icon, 'rotate_right');
      expect(spec.category, 'Motion');
      expect(spec.version, 1);
      expect(spec.fields.map((f) => f.name), ['spinning', 'speed', 'axis', 'showcase', 'mode']);
      final speed = spec.field('speed')!;
      expect(speed.kind, FieldKind.number);
      expect(speed.min, 0);
      expect(speed.step, 0.1);
      expect(speed.unit, 'rad/s');
      expect(speed.tooltip, 'Angular speed');
      expect(speed.showIf, 'spinning');
      expect(speed.visibleWith({'spinning': false}), isFalse);
      expect(speed.visibleWith({'spinning': true}), isTrue);
      // The sibling missing from the data counts as its default (true).
      expect(speed.visibleWith({}, siblings: spec.fields), isTrue);
      expect(speed.visibleWith({}), isFalse);
      expect(spec.field('spinning')!.label, 'Spinning');
      expect(spec.field('axis')!.kind, FieldKind.vec3);
      final showcase = spec.field('showcase')!;
      expect(showcase.kind, FieldKind.assetRef);
      expect(showcase.nullable, isTrue);
      expect(showcase.assetKind, 'mesh');
      expect(showcase.header, 'Presentation');
      final mode = spec.field('mode')!;
      expect(mode.kind, FieldKind.enumeration);
      expect(mode.enumValues, ['slow', 'fast']);
      expect(spec.schemaDefaults(), {
        'axis': [0.0, 1.0, 0.0],
        'speed': 1.0,
        'spinning': true,
        'mode': 'slow',
      });
      expect(humanize('cast_shadows'), 'Cast Shadows');
      expect(humanize('fovY'), 'Fov Y');
    });

    test('a type nobody knows gets an empty spec that shows JSON', () {
      final r = rig();
      final spec = r.model.specFor('game.Unknown');
      expect(spec.missingSchema, isTrue);
      expect(spec.additionalProperties, isTrue);
      expect(spec.fields, isEmpty);
    });
  });

  group('model', () {
    test('a single selection becomes one slot per component, in component order', () {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      expect(r.model.hasDocument, isTrue);
      expect(r.model.slots, isEmpty);
      tab.select('cam');
      expect(r.model.single!.name, 'Camera');
      expect(r.model.slots.map((s) => s.key), ['fynkel.Transform#0', 'fynkel.Camera#0']);
      final cam = r.model.slots[1];
      expect(cam.value('fov_y'), 1.0);
      expect(cam.isMixed('fov_y'), isFalse);
      expect(cam.spec.field('fov_y')!.angle, isTrue);
    });

    test(
      'multi-selection pairs components by type and position, shows mixed values, and edits all in one transaction',
      () {
        final r = rig();
        final tab = r.tabs.openDocument('main')!;
        tab.setSelection(['a', 'b']);
        expect(r.model.entities.map((e) => e.name), ['Crate A', 'Crate B']);
        expect(r.model.slots.map((s) => s.key), ['fynkel.Transform#0', 'fynkel.MeshRenderer#0', 'game.Spinner#0']);
        final mesh = r.model.slots[1];
        expect(mesh.isMixed('cast_shadows'), isTrue);
        expect(mesh.isMixed('visible'), isFalse);
        final transform = r.model.slots[0];
        expect(transform.isMixed('position'), isTrue);
        expect(transform.isMixed('position/1'), isFalse);
        expect(transform.isMixed('position/0'), isTrue);
        final spinner = r.model.slots[2];
        expect(spinner.isMixed('speed'), isTrue);

        // One transaction sets both.
        final before = tab.document.history;
        expect(r.model.setField(spinner, 'speed', 3, spec: spinner.spec.field('speed')), isTrue);
        expect(tab.document.history, before + 1);
        expect(
          r.doc
              .entities(['a', 'b'])
              .entities
              .values
              .map((e) => ((e!['components'] as Map)['s'] as Map)['data']['speed']),
          [3, 3],
        );
        expect(r.model.slots[2].isMixed('speed'), isFalse);
        // One undo undoes both.
        tab.undo();
        expect(r.model.slots[2].isMixed('speed'), isTrue);
        expect(r.model.slots[2].value('speed'), 2.5);

        // A pointer write on one vector component.
        expect(r.model.setField(r.model.slots[0], '/position/1', 5), isTrue);
        expect(r.model.slots[0].value('position'), [2, 5, 0]);
        expect(r.model.slots[0].isMixed('position/1'), isFalse);
        // The camera has no MeshRenderer: with it in the selection, the slot disappears.
        tab.setSelection(['a', 'b', 'cam']);
        expect(r.model.slots.map((s) => s.key), ['fynkel.Transform#0']);
      },
    );

    test('a value the hints refuse never reaches the document; the message stays until the next accepted edit', () {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      final spinner = r.model.slots.singleWhere((s) => s.type == 'game.Spinner');
      final speed = spinner.spec.field('speed')!;
      final before = tab.document.history;
      var notified = 0;
      r.model.addListener(() => notified++);
      expect(r.model.setField(spinner, 'speed', -1, spec: speed), isFalse);
      expect(tab.document.history, before);
      expect(r.model.errorFor(spinner, 'speed'), contains('below the minimum 0'));
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('speed'), 2.5);
      expect(notified, 1);
      // Another field's edit does not clear it.
      expect(r.model.setField(spinner, 'spinning', false, spec: spinner.spec.field('spinning')), isTrue);
      expect(r.model.errorFor(spinner, 'speed'), isNotNull);
      // The next accepted edit of speed clears it.
      expect(r.model.setField(spinner, 'speed', 4, spec: speed), isTrue);
      expect(r.model.errorFor(spinner, 'speed'), isNull);
      expect(tab.document.history, before + 2);

      // An enum value outside the list, a non-integer for an integer, a missing asset.
      final cam = () {
        tab.select('cam');
        return r.model.slots.singleWhere((s) => s.type == 'fynkel.Camera');
      }();
      expect(r.model.setField(cam, 'projection', 'fisheye', spec: cam.spec.field('projection')), isFalse);
      expect(r.model.errorFor(cam, 'projection'), contains("'fisheye' is not one of"));
      expect(r.model.setField(cam, 'priority', 1.5, spec: cam.spec.field('priority')), isFalse);
      expect(r.model.errorFor(cam, 'priority'), 'not a whole number');
      expect(r.model.setField(cam, 'priority', 2, spec: cam.spec.field('priority')), isTrue);
      tab.select('a');
      final mesh = r.model.slots.singleWhere((s) => s.type == 'fynkel.MeshRenderer');
      expect(r.model.setField(mesh, 'mesh', {'asset': 'nope'}, spec: mesh.spec.field('mesh')), isFalse);
      expect(r.model.errorFor(mesh, 'mesh'), contains('FYN-A0004'));
      expect(r.model.setField(mesh, 'mesh', {'asset': meshId, 'sub': 'anim/0'}, spec: mesh.spec.field('mesh')), isTrue);
      expect(
        r.model.setField(mesh, 'mesh', {'asset': meshId, 'sub': 'mesh/9'}, spec: mesh.spec.field('mesh')),
        isFalse,
      );
      expect(r.model.setField(mesh, 'material', null, spec: mesh.spec.field('material')), isFalse);
      expect(r.model.errorFor(mesh, 'material'), contains('cannot be empty'));
    });

    test("the document's own refusal (the crate's validation) shows its diagnostic and reverts the value", () {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      r.doc.reject = (c) => c['field'] == 'fov_y' && (c['value'] as num) > 3 ? 'FYN-D0301: fov_y above 3' : null;
      tab.select('cam');
      final cam = r.model.slots.singleWhere((s) => s.type == 'fynkel.Camera');
      final before = tab.document.history;
      // No spec: the precheck is skipped and the document decides.
      expect(r.model.setField(cam, 'fov_y', 3.5), isFalse);
      expect(tab.document.history, before);
      expect(r.model.errorFor(cam, 'fov_y'), contains('FYN-D0301'));
      expect(r.model.slots.singleWhere((s) => s.type == 'fynkel.Camera').value('fov_y'), 1.0);
      expect(r.model.setField(cam, 'fov_y', 2.0), isTrue);
      expect(r.model.errorFor(cam, 'fov_y'), isNull);
    });

    test('Add Component uses native defaults or the behaviour schema defaults; Remove drops it; both on every selected entity', () {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.setSelection(['a', 'b']);
      final addable = r.model.addableTypes();
      expect(addable.first.spec.isBehaviour, isTrue, reason: 'behaviours first');
      expect(addable.map((t) => t.type), contains('fynkel.Camera'));
      expect(addable.map((t) => t.type), isNot(contains('fynkel.Name')), reason: 'not authored');
      expect(r.model.addableTypes('spin').map((t) => t.type), ['game.Spinner']);
      expect(
        r.model.addableTypes('light').map((t) => t.type),
        containsAll(['fynkel.DirectionalLight', 'fynkel.PointLight', 'fynkel.SpotLight']),
      );

      final before = tab.document.history;
      expect(r.model.addComponent('fynkel.DirectionalLight'), isTrue);
      expect(tab.document.history, before + 1);
      expect(r.model.slots.map((s) => s.key), contains('fynkel.DirectionalLight#0'));
      final light = r.model.slots.singleWhere((s) => s.type == 'fynkel.DirectionalLight');
      expect(light.instances.length, 2);
      expect(light.value('intensity'), 100000.0);
      // A second Spinner is refused before any command is sent: one component per type (FYN-D0204).
      final applies = r.doc.calls.where((c) => c == 'apply').length;
      expect(r.model.entitiesWith('game.Spinner').map((e) => e.id), ['a', 'b']);
      expect(r.model.addComponent('game.Spinner'), isFalse);
      expect(r.model.lastError, contains('already on every selected entity'));
      expect(r.model.lastError, contains('FYN-D0204'));
      expect(r.doc.calls.where((c) => c == 'apply').length, applies);
      expect(tab.document.history, before + 1);
      // A document that already holds two (the crate keeps and reports them) still pairs them as slot #1.
      tab.transaction([
        for (final id in ['a', 'b'])
          Command.addComponent(id, {
            'type': 'game.Spinner',
            'version': 1,
            'data': {'speed': 1.0, 'spinning': true, 'mode': 'slow'},
          }),
      ]);
      final second = r.model.slots.singleWhere((s) => s.key == 'game.Spinner#1');
      expect(second.value('speed'), 1.0);
      expect(second.value('spinning'), true);
      expect(second.value('mode'), 'slow');
      expect(r.model.lastError, isNull, reason: 'an accepted transaction clears the message');
      // A native type without defaults is refused with a message.
      expect(r.model.addComponent('fynkel.Skin'), isFalse);
      expect(r.model.lastError, contains('No defaults'));
      // Remove from both.
      expect(r.model.removeComponent(light), isTrue);
      expect(r.model.slots.map((s) => s.key), isNot(contains('fynkel.DirectionalLight#0')));
      expect(tab.document.history, before + 3);
      tab.undo();
      expect(r.model.slots.map((s) => s.key), contains('fynkel.DirectionalLight#0'));
    });

    test(
      'a rejection shows the reasons behind the summary; the message goes with the selection, an accepted edit or ×',
      () {
        final r = rig();
        final tab = r.tabs.openDocument('main')!;
        tab.select('a');
        // The crate's shape: FYN-E0005 "would leave the document with 1 error(s)" with the real reason related.
        r.doc.validateAfter = (commands) => [
          for (final c in commands)
            if (c['command'] == 'addComponent')
              {
                'code': 'FYN-D0204',
                'severity': 'error',
                'message': 'entity already has a fynkel.DirectionalLight component (k9)',
                'suggestion': 'an entity holds one component per type; remove one',
              },
        ];
        final history = tab.document.history;
        expect(r.model.addComponent('fynkel.DirectionalLight'), isFalse);
        expect(tab.document.history, history);
        expect(r.model.lastError, contains('FYN-D0204: entity already has a fynkel.DirectionalLight component'));
        expect(r.model.lastError, contains('— an entity holds one component per type; remove one'));
        expect(r.model.lastError, isNot(contains('would leave the document')));
        expect(r.model.slots.map((s) => s.type), isNot(contains('fynkel.DirectionalLight')), reason: 'rolled back');
        // 1. Selecting something else clears it.
        tab.select('b');
        expect(r.model.lastError, isNull);
        // 2. An accepted transaction from anywhere (the Hierarchy's rename here) clears it.
        expect(r.model.addComponent('fynkel.DirectionalLight'), isFalse);
        expect(r.model.lastError, isNotNull);
        expect(tab.transaction([Command.renameObject('b', 'Crate B2')])!.ok, isTrue);
        expect(r.model.lastError, isNull);
        // 3. The × clears it.
        expect(r.model.addComponent('fynkel.DirectionalLight'), isFalse);
        expect(r.model.lastError, isNotNull);
        r.model.dismissError();
        expect(r.model.lastError, isNull);
        // The same description serves a field's refusal (mutation diagnostics without related).
        r.doc.validateAfter = null;
        r.doc.reject = (c) => c['field'] == 'fov_y' ? 'FYN-D0301: fov_y above 3' : null;
        final a = tab.transaction([Command.setField('cam', 'c', 'fov_y', 9)])!;
        expect(a.ok, isFalse);
        expect(describeRejection(a), 'FYN-E0002: FYN-D0301: fov_y above 3');
      },
    );

    test('reset to defaults, rename, prefab override revert', () {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      final spinner = r.model.slots.singleWhere((s) => s.type == 'game.Spinner');
      expect(r.model.resetToDefaults(spinner), isTrue);
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('speed'), 1.0);
      expect(r.model.rename('Box A'), isTrue);
      expect(r.model.single!.name, 'Box A');
      expect(r.model.rename('  '), isFalse);

      tab.select('inst');
      final info = r.model.single!.prefab!;
      expect(info.assetId, 'p0000000-0000-4000-8000-000000000040');
      expect(info.overrides.length, 2);
      expect(describeOverride(info.overrides.first), 'b1 › c3 › cast_shadows = false');
      expect(r.model.revertOverride('inst', index: 0), isTrue);
      expect(r.model.single!.prefab!.overrides.map((o) => o['field']), ['visible']);
      expect(r.model.revertOverride('inst'), isTrue);
      expect(r.model.single!.prefab!.overrides, isEmpty);
    });

    test('Open script resolves the metadata library inside the project and hands it to the IDE', () async {
      final opened = <String>[];
      final r = rig(
        openInIde: (file, {line, column}) async {
          opened.add(file);
          return null;
        },
      );
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      final spinner = r.model.slots.singleWhere((s) => s.type == 'game.Spinner');
      expect(r.model.scriptPath(spinner), '${r.project.dir}/lib/spinner.dart');
      expect(await r.model.openScript(spinner), isNull);
      expect(opened, ['${r.project.dir}/lib/spinner.dart']);
      final mesh = r.model.slots.singleWhere((s) => s.type == 'fynkel.MeshRenderer');
      expect(await r.model.openScript(mesh), contains('no source file'));
      // A library from another package is not this project's file.
      r.project.metadata = {...spinnerMetadata(), 'package': 'other_game'};
      r.project.notifyListeners();
      expect(r.model.scriptPath(r.model.slots.singleWhere((s) => s.type == 'game.Spinner')), isNull);
    });

    test('entity names, assets by kind, describing a reference', () {
      final r = rig();
      r.tabs.openDocument('main');
      expect(r.model.entityNames(), ['Camera', 'Crate A', 'Crate B', 'Instance']);
      expect(r.model.assetsFor('mesh').map((a) => a.name), ['robot']);
      expect(r.model.assetsFor('clip').map((a) => a.name), ['robot']);
      expect(r.model.assetsFor('material').map((a) => a.name), ['steel']);
      expect(r.model.assetsFor('sound').map((a) => a.name), ['tone'], reason: 'an audio source, whole');
      expect(r.model.assetsFor('texture'), isEmpty);
      expect(r.model.describeAsset({'asset': toneId}), 'tone');
      expect(r.model.describeAsset({'asset': meshId, 'sub': 'mesh/0'}), 'robot › mesh/0');
      expect(r.model.describeAsset({'asset': materialId}), 'steel');
      expect(r.model.describeAsset({'asset': 'sub-anim-0'}), 'robot › anim/0');
      expect(r.model.describeAsset(null), 'None');
      expect(r.model.describeAsset({'asset': 'zzz'}), 'zzz');
    });

    test('too many entities shows a count only; a read-only document cannot be edited', () {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.setSelection(['cam', 'a', 'b', 'inst']);
      expect(r.model.slots.map((s) => s.key), ['fynkel.Transform#0']);
      final small = InspectorModel(tabs: r.tabs, project: r.project, maxEntities: 2);
      addTearDown(small.dispose);
      expect(small.tooMany, isTrue);
      expect(small.slots, isEmpty);
    });
  });

  group('codegen', () {
    test('committed drift compares the metadata with the schema files', () {
      final meta = spinnerMetadata();
      expect(committedDrift(null, {}), isEmpty);
      expect(committedDrift(meta, {'game.Spinner': spinnerSchema()}), isEmpty);
      // Key order does not matter, 1 and 1.0 do not either.
      final reordered = Map<String, Object?>.fromEntries(spinnerSchema().entries.toList().reversed);
      (reordered['x-fynkel'] as Map)['version'] = 1.0;
      expect(committedDrift(meta, {'game.Spinner': reordered}), isEmpty);
      final changed = spinnerSchema();
      ((changed['properties'] as Map)['speed'] as Map)['minimum'] = 1;
      expect(committedDrift(meta, {'game.Spinner': changed}).map((d) => '$d'), ['game.Spinner: schema differs']);
      expect(committedDrift(meta, {}).map((d) => '$d'), ['game.Spinner: no schema file']);
      expect(committedDrift(meta, {'game.Spinner': null}).map((d) => '$d'), [
        'game.Spinner: declared without a schema file',
      ]);
      expect(committedDrift(meta, {'game.Spinner': spinnerSchema(), 'game.Other': {}}).map((d) => '$d'), [
        'game.Other: not in behaviours.g.json',
      ]);
    });

    test(
      'the CLI check runs `fynkel codegen --check --project <dir> --root <root> --json` and reads the report',
      () async {
        final dir = fakeProjectDir('codegen');
        addTearDown(() => dir.deleteSync(recursive: true));
        final runner = RecordingRunner();
        final check = CodegenCheck(
          projectDir: dir.path,
          run: runner.call,
          cli: () => '/fake/fynkel',
          repoRoot: () => '/fake/root',
        );
        addTearDown(check.dispose);
        // Not a Dart package yet.
        await check.check();
        expect(check.status, CodegenStatus.notADartPackage);
        expect(runner.calls, isEmpty);
        File('${dir.path}/pubspec.yaml').writeAsStringSync('name: game\n');

        runner.stdout = jsonEncode({
          'ok': true,
          'check': true,
          'native': {'status': 'upToDate'},
          'project': {'dir': dir.path, 'status': 'upToDate', 'files': 5},
        });
        await check.check();
        expect(check.status, CodegenStatus.upToDate);
        expect(runner.calls.single.$1, '/fake/fynkel');
        expect(runner.calls.single.$2, ['codegen', '--check', '--project', dir.path, '--root', '/fake/root', '--json']);
        expect(runner.calls.single.$3, dir.path);
        expect(check.command, 'fynkel codegen --project ${dir.path}');

        runner.exitCode = 2;
        runner.stdout = jsonEncode({
          'ok': false,
          'check': true,
          'native': {'status': 'upToDate'},
          'project': {
            'dir': dir.path,
            'status': 'stale',
            'stale': ['lib/generated/behaviours.g.json'],
          },
        });
        await check.check();
        expect(check.status, CodegenStatus.stale);
        expect(check.stalePaths, ['lib/generated/behaviours.g.json']);

        runner.exitCode = 1;
        runner.stdout = jsonEncode({
          'ok': false,
          'check': true,
          'native': {'status': 'upToDate'},
          'project': {'dir': dir.path, 'status': 'error', 'error': 'dart run build_runner failed: boom'},
        });
        await check.check();
        expect(check.status, CodegenStatus.failed);
        expect(check.error, contains('boom'));

        runner.stdout = '';
        runner.stderr = 'error: no such file\nmore';
        await check.check();
        expect(check.status, CodegenStatus.failed);
        expect(check.error, 'error: no such file');

        final noCli = CodegenCheck(projectDir: dir.path, run: runner.call, cli: () => null, repoRoot: () => null);
        addTearDown(noCli.dispose);
        await noCli.check();
        expect(noCli.status, CodegenStatus.failed);
        expect(noCli.error, contains('no `fynkel` CLI'));
        expect(noCli.arguments(), ['codegen', '--check', '--project', dir.path, '--json']);
        expect(check.runs, 4);
      },
    );

    test('a check still running when the project closes drops its answer', () async {
      final dir = fakeProjectDir('codegen-closed');
      addTearDown(() => dir.deleteSync(recursive: true));
      File('${dir.path}/pubspec.yaml').writeAsStringSync('name: game\n');
      final answer = Completer<ProcessResult>();
      final check = CodegenCheck(
        projectDir: dir.path,
        run: (exe, args, {workingDirectory}) => answer.future,
        cli: () => '/fake/fynkel',
        repoRoot: () => null,
      );
      final running = check.check();
      expect(check.status, CodegenStatus.running);
      check.dispose();
      answer.complete(ProcessResult(1, 0, '{"ok": true}', ''));
      await running;
    });
  });

  group('maths', () {
    test('Euler degrees round-trip through the quaternion, poles included', () {
      for (final e in const [
        Euler(0, 0, 0),
        Euler(30, 0, 0),
        Euler(0, 45, 0),
        Euler(0, 0, -60),
        Euler(10, 20, 30),
        Euler(-170, 80, 95),
        Euler(0, 90, 0),
        Euler(45, -90, 0),
      ]) {
        final q = e.toQuaternion();
        expect(q.map((c) => c * c).reduce((a, b) => a + b), closeTo(1, 1e-12));
        final back = Euler.fromQuaternion(q);
        // The same rotation, even when the triple differs at a pole.
        expect(sameRotation(back.toQuaternion(), q, eps: 1e-9), isTrue, reason: '$e → $q → $back');
        if (e.y.abs() < 89) {
          expect(back.x, closeTo(e.x, 1e-9));
          expect(back.y, closeTo(e.y, 1e-9));
          expect(back.z, closeTo(e.z, 1e-9));
        }
      }
      // The fixture's Sun: -50° about X.
      final sun = Euler.fromQuaternion([-0.422618, 0, 0, 0.906308]).clean;
      expect(sun.x, closeTo(-50, 1e-3));
      expect(sun.y, 0);
      expect(sun.z, 0);
      expect(Euler.fromQuaternion([0, 0, 0, 1]).clean, Euler.zero);
      expect(Euler(90, 0, 0).toQuaternion()[0], closeTo(0.7071067811865476, 1e-12));
    });

    test('colours: linear ↔ sRGB ↔ hex ↔ HSV', () {
      expect(linearRgbToHex([1, 1, 1]), '#FFFFFF');
      expect(linearRgbToHex([0, 0, 0]), '#000000');
      expect(linearRgbToHex([1, 0.97, 0.9]), '#FFFCF3');
      expect(linearRgbToHex([0.5, 0.5, 0.5, 0.5]), '#BCBCBC80');
      expect(hexToLinearRgb('#FFFFFF'), [1, 1, 1]);
      expect(hexToLinearRgb('fff'), [1, 1, 1]);
      expect(hexToLinearRgb('#zz0000'), isNull);
      expect(hexToLinearRgb('#12345'), isNull);
      final back = hexToLinearRgb('#BCBCBC80')!;
      expect(back[0], closeTo(0.5, 0.005));
      expect(back[3], closeTo(0.5, 0.005));
      expect(srgbToLinear(linearToSrgb(0.3)), closeTo(0.3, 1e-12));
      final (h, s, v) = rgbToHsv(1, 0, 0);
      expect((h, s, v), (0, 1, 1));
      expect(rgbToHsv(0, 1, 0).$1, 120);
      expect(rgbToHsv(0, 0, 1).$1, 240);
      final (r, g, b) = hsvToRgb(240, 1, 1);
      expect((r, g, b), (0, 0, 1));
      final (r2, g2, b2) = hsvToRgb(60, 0.5, 0.8);
      final (h2, s2, v2) = rgbToHsv(r2, g2, b2);
      expect(h2, closeTo(60, 1e-9));
      expect(s2, closeTo(0.5, 1e-9));
      expect(v2, closeTo(0.8, 1e-9));
    });

    test('numbers keep their JSON form; expressions parse', () {
      expect(jsonNumber(1, previous: 1), 1);
      expect(jsonNumber(1, previous: 1), isA<int>());
      expect(jsonNumber(1, previous: 1.0), isA<double>());
      expect(jsonNumber(1, previous: null), isA<int>());
      expect(jsonNumber(2.5, previous: 2), 2.5);
      expect(jsonNumber(0.1 + 0.2, previous: 0.5), 0.3);
      expect(jsonNumber(-0.0, previous: 1.0), 0.0);
      expect(jsonNumber(2.7, integer: true), 3);
      expect(formatNumber(1.5), '1.5');
      expect(formatNumber(2), '2');
      expect(formatNumber(0.1 + 0.2), '0.3');
      expect(formatNumber(-0.0000001), '0');
      expect(formatNumber(45.000002), '45.000002');
      expect(formatNumber(45.000002, decimals: 3), '45', reason: 'degrees from a float32 radian');
      expect(formatNumber(0.7853982 * 180 / math.pi, decimals: 3), '45');
      expect(formatNumber(34.9999981, decimals: 3), '35');
      expect(formatNumber(12.3456789, decimals: 3), '12.346');
      expect(parseNumber('1.5'), 1.5);
      expect(parseNumber('1,5'), 1.5);
      expect(parseNumber('1 + 2 * 3'), 7);
      expect(parseNumber('(1 + 2) * 3'), 9);
      expect(parseNumber('-4 / 2'), -2);
      expect(parseNumber('2e3'), 2000);
      expect(parseNumber('1 +'), isNull);
      expect(parseNumber('abc'), isNull);
      expect(parseNumber(''), isNull);
      expect(parseNumber('1/0'), isNull);
    });
  });

  group('widgets', () {
    Future<void> pumpPanel(WidgetTester tester, InspectorModel model) async {
      tester.view.physicalSize = const Size(1000, 1800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Align(
              alignment: Alignment.topLeft,
              child: SizedBox(width: 420, height: 1800, child: InspectorPanel(model: model)),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
    }

    Finder inField(String slotKey, String field, Finder inner) =>
        find.descendant(of: find.byKey(ValueKey('field.$slotKey.$field')), matching: inner);

    testWidgets('a drag on a number label is one transaction; typing commits on ↩ and an expression works', (
      tester,
    ) async {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      await pumpPanel(tester, r.model);
      expect(find.text('Crate A'), findsOneWidget);
      expect(find.byKey(const ValueKey('card.fynkel.Transform#0')), findsOneWidget);
      expect(find.byKey(const ValueKey('card.game.Spinner#0')), findsOneWidget);
      // Groups from the hints.
      expect(find.text('MOTION'), findsOneWidget);
      expect(find.text('SHADOWS'), findsOneWidget);

      // Drag X of position by 100 px: 0.01 per pixel → 3.0.
      final before = tab.document.history;
      final x = inField('fynkel.Transform#0', 'position', find.text('X'));
      expect(x, findsOneWidget);
      await tester.drag(x, const Offset(100, 0));
      await tester.pumpAndSettle();
      expect(tab.document.history, before + 1, reason: 'a drag is one transaction');
      List pos() => r.model.slots.singleWhere((s) => s.type == 'fynkel.Transform').value('position') as List;
      expect(pos()[0], closeTo(3.0, 1e-9));
      expect(pos()[1], 0);
      // The Spinner's speed by the label drag with its `step` 0.1: 20 px → +2.0.
      await tester.drag(inField('game.Spinner#0', 'speed', find.text('Speed')), const Offset(20, 0));
      await tester.pumpAndSettle();
      expect(tab.document.history, before + 2);
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('speed'), closeTo(4.5, 1e-9));
      // A drag below the minimum clamps at it.
      await tester.drag(inField('game.Spinner#0', 'speed', find.text('Speed')), const Offset(-500, 0));
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('speed'), 0);

      // Typing: an expression, then ↩.
      final speedText = inField('game.Spinner#0', 'speed', find.byType(TextField));
      await tester.enterText(speedText, '1 + 0.5');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('speed'), 1.5);
      expect(tab.document.history, before + 4);
      // A typed value below the minimum: refused, message shown, value back.
      await tester.enterText(speedText, '-2');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(tab.document.history, before + 4);
      expect(find.text('below the minimum 0'), findsOneWidget);
      expect(tester.widget<TextField>(speedText).controller!.text, '1.5');
      // Fixed → the message goes.
      await tester.enterText(speedText, '2');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.text('below the minimum 0'), findsNothing);
      // The unit shows.
      expect(find.text('rad/s'), findsOneWidget);
    });

    testWidgets('checkbox, enum menu, show-if, mixed values and the multi header', (tester) async {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      await pumpPanel(tester, r.model);
      // speed is shown while spinning is true; unticking hides it (showIf).
      expect(find.byKey(const ValueKey('field.game.Spinner#0.speed')), findsOneWidget);
      await tester.tap(inField('game.Spinner#0', 'spinning', find.byType(Checkbox)));
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('spinning'), false);
      expect(find.byKey(const ValueKey('field.game.Spinner#0.speed')), findsNothing);

      // Enum: the Camera's projection.
      tab.select('cam');
      await tester.pumpAndSettle();
      expect(find.text('Perspective'), findsOneWidget);
      await tester.tap(find.text('Perspective'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Orthographic').last);
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'fynkel.Camera').value('projection'), 'orthographic');
      // The angle field shows degrees to three decimals: fov_y 1.0 rad ≈ 57.296°.
      expect(find.text('57.296'), findsOneWidget);
      // Rotation as Euler: the Transform's identity → 0, 0, 0 under X Y Z.
      expect(inField('fynkel.Transform#0', 'rotation', find.text('0')), findsNWidgets(3));

      // Mixed: two crates; cast_shadows disagree → tristate null; speed shows —.
      tab.setSelection(['a', 'b']);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inspector.multi')), findsOneWidget);
      expect(find.textContaining('2 entities'), findsOneWidget);
      final cast = tester.widget<Checkbox>(inField('fynkel.MeshRenderer#0', 'cast_shadows', find.byType(Checkbox)));
      expect(cast.tristate, isTrue);
      expect(cast.value, isNull);
      // X differs, Y and Z agree (the '—' hint of an empty field is not a value).
      expect(
        inField(
          'fynkel.Transform#0',
          'position',
          find.byWidgetPredicate((w) => w is EditableText && w.controller.text == '—'),
        ),
        findsOneWidget,
      );
      // Ticking the mixed box sets both true in one transaction.
      final before = tab.document.history;
      await tester.tap(inField('fynkel.MeshRenderer#0', 'cast_shadows', find.byType(Checkbox)));
      await tester.pumpAndSettle();
      expect(tab.document.history, before + 1);
      expect(r.model.slots.singleWhere((s) => s.type == 'fynkel.MeshRenderer').isMixed('cast_shadows'), isFalse);
      expect(r.model.slots.singleWhere((s) => s.type == 'fynkel.MeshRenderer').value('cast_shadows'), true);
    });

    testWidgets('the asset picker lists the kind only, an unknown drop is refused, Clear on a nullable field', (
      tester,
    ) async {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      await pumpPanel(tester, r.model);
      expect(inField('fynkel.MeshRenderer#0', 'material', find.text('steel')), findsOneWidget);
      expect(inField('fynkel.MeshRenderer#0', 'mesh', find.text('robot › mesh/0')), findsOneWidget);
      // Open the mesh picker: only mesh sub-assets are rows, not the material or the clip.
      await tester.tap(inField('fynkel.MeshRenderer#0', 'mesh', find.byKey(const ValueKey('asset.pick'))));
      await tester.pumpAndSettle();
      expect(find.textContaining('Choose a mesh'), findsOneWidget);
      expect(find.byKey(const ValueKey('assetPicker.row.0')), findsOneWidget);
      expect(find.byKey(const ValueKey('assetPicker.row.1')), findsNothing);
      await tester.enterText(find.byKey(const ValueKey('assetPicker.search')), 'zzz');
      await tester.pumpAndSettle();
      expect(find.text('No mesh matches.'), findsOneWidget);
      await tester.enterText(find.byKey(const ValueKey('assetPicker.search')), 'rob');
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('assetPicker.row.0')));
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'fynkel.MeshRenderer').value('mesh'), {
        'asset': meshId,
        'sub': 'mesh/0',
      });

      // A sound picker lists the whole audio source as one row, and nothing else.
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: AssetField(
              value: null,
              kind: 'sound',
              describe: r.model.describeAsset,
              candidates: () => r.model.assetsFor('sound'),
              onCommit: (_) {},
            ),
          ),
        ),
      );
      await tester.tap(find.byKey(const ValueKey('asset.pick')));
      await tester.pumpAndSettle();
      expect(find.textContaining('Choose a sound'), findsOneWidget);
      expect(find.byKey(const ValueKey('assetPicker.none')), findsNothing, reason: 'the field is not nullable');
      expect(find.byKey(const ValueKey('assetPicker.row.0')), findsOneWidget);
      expect(find.byKey(const ValueKey('assetPicker.row.1')), findsNothing);
      expect(
        find.descendant(of: find.byKey(const ValueKey('assetPicker.row.0')), matching: find.text('tone')),
        findsOneWidget,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      await pumpPanel(tester, r.model);

      // The Spinner's nullable showcase: None → set through the picker → Clear.
      expect(inField('game.Spinner#0', 'showcase', find.text('None')), findsOneWidget);
      await tester.tap(inField('game.Spinner#0', 'showcase', find.byKey(const ValueKey('asset.pick'))));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('assetPicker.row.0')));
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('showcase'), {
        'asset': meshId,
        'sub': 'mesh/0',
      });
      await tester.tap(inField('game.Spinner#0', 'showcase', find.byTooltip('Clear')));
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('showcase'), isNull);
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').isSet('showcase'), isTrue);
      // The picker's None row (a nullable field only) empties it too.
      await tester.tap(inField('game.Spinner#0', 'showcase', find.byKey(const ValueKey('asset.pick'))));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('assetPicker.row.0')));
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('showcase'), isNotNull);
      await tester.tap(inField('game.Spinner#0', 'showcase', find.byKey(const ValueKey('asset.pick'))));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('assetPicker.none')));
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'game.Spinner').value('showcase'), isNull);

      // A programmatic unknown reference is refused with FYN-A0004 (what a bad drop would produce).
      final mesh = r.model.slots.singleWhere((s) => s.type == 'fynkel.MeshRenderer');
      final before = tab.document.history;
      expect(
        r.model.setField(
          mesh,
          'mesh',
          const AssetDragPayload(id: 'ghost', kind: 'mesh', name: 'ghost').toRef(),
          spec: mesh.spec.field('mesh'),
        ),
        isFalse,
      );
      await tester.pumpAndSettle();
      expect(tab.document.history, before);
      expect(find.textContaining('FYN-A0004'), findsOneWidget);
    });

    testWidgets('Add Component popup, the card menu (Remove), the prefab section, the codegen banner', (tester) async {
      final runner = RecordingRunner(
        stdout: jsonEncode({
          'ok': false,
          'project': {
            'status': 'stale',
            'stale': ['lib/generated/behaviours.g.json'],
          },
        }),
        exitCode: 2,
      );
      final dir = fakeProjectDir('inspector_banner');
      addTearDown(() => dir.deleteSync(recursive: true));
      File('${dir.path}/pubspec.yaml').writeAsStringSync('name: game\n');
      final project = inspectorProject(dir);
      final doc = scene();
      project.fakeDocuments.add(doc);
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final codegen = CodegenCheck(
        projectDir: dir.path,
        run: runner.call,
        cli: () => '/fake/fynkel',
        repoRoot: () => null,
      );
      addTearDown(codegen.dispose);
      final model = InspectorModel(tabs: tabs, project: project, codegen: codegen);
      addTearDown(model.dispose);
      final tab = tabs.openDocument('main')!;
      tab.select('cam');
      await pumpPanel(tester, model);
      // No drift, no check yet: no banner.
      expect(find.byKey(const ValueKey('inspector.codegenBanner')), findsNothing);
      await codegen.check();
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inspector.codegenBanner')), findsOneWidget);
      expect(find.textContaining('stale'), findsWidgets);
      expect(find.textContaining('fynkel codegen --project'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('inspector.checkAgain')));
      await tester.pumpAndSettle();
      expect(runner.calls.length, 2);
      // Committed drift shows even without the CLI: the schema file changed under the metadata.
      final drifted = spinnerSchema();
      ((drifted['properties'] as Map)['speed'] as Map)['minimum'] = 5;
      project.schemas = [
        ...nativeSchemas(),
        TypeSchema(type: 'game.Spinner', isBehaviour: true, authored: true, allowed: true, schema: drifted),
      ];
      project.notifyListeners();
      await tester.pumpAndSettle();
      expect(find.textContaining('metadata is out of date'), findsOneWidget);
      expect(find.textContaining('game.Spinner: schema differs'), findsOneWidget);
      // The Inspector still shows the last committed fields (the metadata's schema and hints).
      tab.select('a');
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('field.game.Spinner#0.speed')), findsOneWidget);

      // Add Component: search, pick.
      final before = tab.document.history;
      await tester.tap(find.byKey(const ValueKey('inspector.addComponent')));
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const ValueKey('addComponent.search')), 'directional');
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('addComponent.fynkel.DirectionalLight')));
      await tester.pumpAndSettle();
      expect(tab.document.history, before + 1);
      expect(find.byKey(const ValueKey('card.fynkel.DirectionalLight#0')), findsOneWidget);
      // The colour swatch and hex of the new light.
      expect(inField('fynkel.DirectionalLight#0', 'color', find.text('#FFFFFF')), findsOneWidget);

      // Remove it through the card menu.
      await tester.tap(find.byKey(const ValueKey('card.menu.fynkel.DirectionalLight#0')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Remove Component'));
      await tester.pumpAndSettle();
      expect(tab.document.history, before + 2);
      expect(find.byKey(const ValueKey('card.fynkel.DirectionalLight#0')), findsNothing);

      // Prefab section with revert.
      tab.select('inst');
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('prefab.title')), findsOneWidget);
      expect(find.byKey(const ValueKey('prefab.override.1')), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('prefab.revert.0')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('prefab.override.1')), findsNothing);
      expect(find.byKey(const ValueKey('prefab.override.0')), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('prefab.revertAll')));
      await tester.pumpAndSettle();
      expect(find.textContaining('No overrides'), findsOneWidget);

      // Empty states.
      tab.clearSelection();
      await tester.pumpAndSettle();
      expect(find.textContaining('Nothing selected'), findsOneWidget);
    });

    testWidgets('Add Component greys out a type the entity already holds; a refusal shows under the cards until ×', (
      tester,
    ) async {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      await pumpPanel(tester, r.model);
      final before = tab.document.history;
      await tester.tap(find.byKey(const ValueKey('inspector.addComponent')));
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const ValueKey('addComponent.search')), 'transform');
      await tester.pumpAndSettle();
      final held = find.byKey(const ValueKey('addComponent.note.fynkel.Transform'));
      expect(held, findsOneWidget);
      expect(tester.widget<Text>(held).data, 'already on Crate A');
      // ↩ with one visible match adds nothing when that match is held; a tap on the row does nothing.
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('addComponent.search')), findsOneWidget, reason: 'the dialog stays open');
      await tester.tap(find.byKey(const ValueKey('addComponent.fynkel.Transform')), warnIfMissed: false);
      await tester.pumpAndSettle();
      expect(tab.document.history, before);
      expect(find.byKey(const ValueKey('addComponent.search')), findsOneWidget);
      // A type the entity lacks is offered plainly.
      await tester.enterText(find.byKey(const ValueKey('addComponent.search')), 'point');
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('addComponent.note.fynkel.PointLight')), findsNothing);
      expect(find.byKey(const ValueKey('addComponent.fynkel.PointLight')), findsOneWidget);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('addComponent.search')), findsNothing);

      // A refusal that is not one field's shows under the cards, with a ×.
      expect(r.model.addComponent('fynkel.Transform'), isFalse);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inspector.lastError')), findsOneWidget);
      expect(find.textContaining('Transform is already on Crate A'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('inspector.dismissError')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inspector.lastError')), findsNothing);
    });

    testWidgets('an angle shows rounded degrees; compact axis fields drop the unit, the row label carries it', (
      tester,
    ) async {
      final commits = <Object>[];
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SizedBox(
              width: 300,
              child: Column(
                children: [
                  NumberField(key: const ValueKey('angle'), value: 0.7853982, angle: true, onCommit: commits.add),
                  FieldRow(
                    label: 'Position',
                    unit: 'm',
                    child: VecField(
                      value: const [20, 0, 1.5],
                      components: 3,
                      unit: 'm',
                      onCommitComponent: (i, v) => commits.add([i, v]),
                    ),
                  ),
                  FieldRow(
                    label: 'Fov',
                    child: NumberField(key: const ValueKey('plain'), value: 1.5, unit: 'm', onCommit: commits.add),
                  ),
                ],
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      // 0.7853982 rad (float32 π/4) is 45.0000023°: shown as 45, stored untouched until an edit.
      expect(
        tester
            .widget<TextField>(
              find.descendant(of: find.byKey(const ValueKey('angle')), matching: find.byType(TextField)),
            )
            .controller!
            .text,
        '45',
      );
      expect(commits, isEmpty);
      // The axis fields: no suffix inside; the unit once after the row label.
      final axisFields = tester.widgetList<TextField>(
        find.descendant(of: find.byType(VecField), matching: find.byType(TextField)),
      );
      expect(axisFields.map((f) => f.controller!.text), ['20', '0', '1.5']);
      expect(axisFields.map((f) => f.decoration!.suffixText), everyElement(isNull));
      expect(find.byKey(const ValueKey('field.unit')), findsOneWidget);
      expect(tester.widget<Text>(find.byKey(const ValueKey('field.unit'))).data, 'm');
      // A full-width number draws its unit right after the value; a compact degree field keeps °.
      String unitOf(String key) => tester
          .widget<Text>(
            find.descendant(of: find.byKey(ValueKey(key)), matching: find.byKey(const ValueKey('number.unit'))),
          )
          .data!;
      expect(unitOf('plain'), 'm');
      expect(unitOf('angle'), '°');
    });

    testWidgets('the layer mask names the project layers; the colour picker commits linear values', (tester) async {
      final r = rig();
      File('${r.project.dir}/fynkel.project.json').writeAsStringSync(
        jsonEncode({
          'name': 'x',
          'layers': ['default', 'player', 'props'],
        }),
      );
      r.doc.reject = null;
      // Give the camera entity a Collider and a light to exercise both widgets.
      final tab = r.tabs.openDocument('main')!;
      tab.transaction([
        {
          'command': 'addComponent',
          'entity': 'cam',
          'component': {
            'id': 'col',
            'type': 'fynkel.Collider',
            'data': {
              'shape': 'box',
              'size': [1, 1, 1],
              'mesh': null,
              'offset': [0, 0, 0],
              'friction': 0.5,
              'restitution': 0.0,
              'is_sensor': false,
              'mask': 2,
            },
          },
        },
        {
          'command': 'addComponent',
          'entity': 'cam',
          'component': {
            'id': 'sun',
            'type': 'fynkel.DirectionalLight',
            'data': {
              'color': [1, 0.5, 0],
              'intensity': 1.0,
              'cast_shadows': true,
            },
          },
        },
      ]);
      tab.select('cam');
      await pumpPanel(tester, r.model);
      expect(inField('fynkel.Collider#0', 'mask', find.text('player')), findsOneWidget);
      await tester.tap(inField('fynkel.Collider#0', 'mask', find.byKey(const ValueKey('layerMask.summary'))));
      await tester.pumpAndSettle();
      await tester.tap(find.ancestor(of: find.text('props'), matching: find.byType(CheckedPopupMenuItem<int>)));
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'fynkel.Collider').value('mask'), 6);
      expect(inField('fynkel.Collider#0', 'mask', find.text('player, props')), findsOneWidget);

      // Colour: the hex field commits linear values, ints kept where the document had them.
      expect(inField('fynkel.DirectionalLight#0', 'color', find.text('#FFBC00')), findsOneWidget);
      await tester.enterText(
        inField('fynkel.DirectionalLight#0', 'color', find.byKey(const ValueKey('color.hex'))),
        '#000000',
      );
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(r.model.slots.singleWhere((s) => s.type == 'fynkel.DirectionalLight').value('color'), [0, 0, 0]);
      // The picker dialog: OK commits.
      await tester.tap(inField('fynkel.DirectionalLight#0', 'color', find.byKey(const ValueKey('color.swatch'))));
      await tester.pumpAndSettle();
      expect(find.text('Colour'), findsOneWidget);
      await tester.enterText(find.byKey(const ValueKey('picker.hex')), '#FFFFFF');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      await tester.tap(find.text('OK'));
      await tester.pumpAndSettle();
      final c = r.model.slots.singleWhere((s) => s.type == 'fynkel.DirectionalLight').value('color') as List;
      expect(c.map((v) => (v as num).toDouble()), everyElement(closeTo(1, 1e-9)));

      // Euler: type 90 into X → the quaternion [0.7071, 0, 0, 0.7071]; the row keeps showing 90.
      final rotX = find
          .descendant(
            of: find.byKey(const ValueKey('field.fynkel.Transform#0.rotation')),
            matching: find.byType(TextField),
          )
          .first;
      await tester.enterText(rotX, '90');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      final q = r.model.slots.singleWhere((s) => s.type == 'fynkel.Transform').value('rotation') as List;
      expect((q[0] as num).toDouble(), closeTo(0.7071067811865476, 1e-9));
      expect(q[1], 0);
      expect((q[3] as num).toDouble(), closeTo(0.7071067811865476, 1e-9));
      expect(inField('fynkel.Transform#0', 'rotation', find.text('90')), findsOneWidget);
    });
  });

  group('object header (ADR-0020)', () {
    void declare(FakeProject project) => File('${project.dir}/fynkel.project.json').writeAsStringSync(
      jsonEncode({
        'name': 'x',
        'layers': ['default', 'player', 'props'],
        'tags': ['pickup', 'enemy', 'boss'],
      }),
    );

    /// The component of [type] on [entity] in the document, or null.
    Map<String, Object?>? componentOf(FakeDocument doc, String entity, String type) {
      final components = doc.entities([entity]).entities[entity]!['components'] as Map;
      for (final c in components.values) {
        if ((c as Map)['type'] == type) return Map<String, Object?>.from(c['data'] as Map);
      }
      return null;
    }

    test('layer and tags are header values, not cards; edits add the component once, then set it', () {
      final r = rig();
      declare(r.project);
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      expect(r.model.layer, 0);
      expect(r.model.tagsInUse, isEmpty);
      expect(
        r.model.addableTypes().map((t) => t.type),
        isNot(anyOf(contains('fynkel.Layer'), contains('fynkel.Tags'))),
      );

      final before = r.doc.history;
      expect(r.model.setLayer(2), isTrue);
      expect(componentOf(r.doc, 'a', 'fynkel.Layer'), {'layer': 2});
      expect(r.model.layer, 2);
      expect(r.model.slots.map((s) => s.type), isNot(contains('fynkel.Layer')));
      expect(r.model.setLayer(1), isTrue);
      expect(componentOf(r.doc, 'a', 'fynkel.Layer'), {'layer': 1});

      expect(r.model.setTag(1, true), isTrue);
      expect(r.model.setTag(0, true), isTrue);
      expect(componentOf(r.doc, 'a', 'fynkel.Tags'), {'bits': 3});
      expect(r.model.tagsInUse, [0, 1]);
      expect(r.model.setTag(1, false), isTrue);
      expect(componentOf(r.doc, 'a', 'fynkel.Tags'), {'bits': 1});
      expect(r.model.slots.map((s) => s.type), isNot(contains('fynkel.Tags')));
      expect(r.doc.history - before, 5, reason: 'one transaction per edit');
      // Nothing to change is no transaction.
      expect(r.model.setLayer(1), isTrue);
      expect(r.model.setTag(0, true), isTrue);
      expect(r.doc.history - before, 5);
    });

    test('several entities: a mixed layer and a mixed tag, set together in one transaction', () {
      final r = rig();
      declare(r.project);
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      r.model.setLayer(2);
      r.model.setTag(0, true);
      tab.select('b', toggle: true);
      expect(r.model.entities.length, 2);
      expect(r.model.layer, isNull);
      expect(r.model.tagState(0), TagState.mixed);
      expect(r.model.tagState(1), TagState.none);
      final before = r.doc.history;
      expect(r.model.setLayer(1), isTrue);
      expect(r.doc.history - before, 1);
      expect(
        [componentOf(r.doc, 'a', 'fynkel.Layer'), componentOf(r.doc, 'b', 'fynkel.Layer')],
        [
          {'layer': 1},
          {'layer': 1},
        ],
      );
      expect(r.model.setTag(0, true), isTrue);
      expect(r.model.tagState(0), TagState.all);
      expect(componentOf(r.doc, 'b', 'fynkel.Tags'), {'bits': 1});
    });

    testWidgets('the header shows the layer by name and the tags as chips', (tester) async {
      final r = rig();
      declare(r.project);
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      tester.view.physicalSize = const Size(1000, 1200);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Align(
              alignment: Alignment.topLeft,
              child: SizedBox(width: 420, height: 1200, child: InspectorPanel(model: r.model)),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Add tag'), findsOneWidget);
      expect(tester.widget<Text>(find.byKey(const ValueKey('layer.summary'))).data, 'default');

      await tester.tap(find.byKey(const ValueKey('layer.summary')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('layer.2')));
      await tester.pumpAndSettle();
      expect(tester.widget<Text>(find.byKey(const ValueKey('layer.summary'))).data, 'props');
      expect(componentOf(r.doc, 'a', 'fynkel.Layer'), {'layer': 2});

      await tester.tap(find.byKey(const ValueKey('tag.add')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('tag.2')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('tag.chip.2')), findsOneWidget);
      expect(find.text('boss'), findsOneWidget);
      expect(find.text('Add tag'), findsNothing);

      await tester.tap(
        find.descendant(
          of: find.byKey(const ValueKey('tag.chip.2')),
          matching: find.byKey(const ValueKey('tag.remove')),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('tag.chip.2')), findsNothing);
      expect(componentOf(r.doc, 'a', 'fynkel.Tags'), {'bits': 0});
      // No Layer or Tags card.
      expect(find.byKey(const ValueKey('card.fynkel.Layer#0')), findsNothing);
      expect(find.byKey(const ValueKey('card.fynkel.Tags#0')), findsNothing);
    });

    testWidgets('the header checkbox switches the object off and on, one undo step each', (tester) async {
      final r = rig();
      final tab = r.tabs.openDocument('main')!;
      tab.select('a');
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Align(
              alignment: Alignment.topLeft,
              child: SizedBox(width: 420, height: 900, child: InspectorPanel(model: r.model)),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      Checkbox box() => tester.widget<Checkbox>(find.byKey(const ValueKey('inspector.active')));
      expect(box().value, isTrue);
      final before = r.doc.history;
      await tester.tap(find.byKey(const ValueKey('inspector.active')));
      await tester.pumpAndSettle();
      expect(box().value, isFalse);
      expect(r.doc.entities(['a']).entities['a']!['active'], false);
      expect(r.doc.history - before, 1);
      await tester.tap(find.byKey(const ValueKey('inspector.active')));
      await tester.pumpAndSettle();
      expect(box().value, isTrue);
      expect(r.doc.entities(['a']).entities['a']!.containsKey('active'), isFalse);
    });
  });
}
