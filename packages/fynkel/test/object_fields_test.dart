// Object fields (docs/unity-parity.md § Tier 2, Inspector fields): a
// script's GameObject and script fields are set from the scene — to the
// object, to that script on it — whatever order the scripts attach in; a
// prefab instance's references reach its own copies; a reference to
// something gone is None with FYN-B0003. Over the real native library and
// a copy of the fixture project.

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

class Spinner extends Behaviour {
  double speed = 1;
}

enum Mode { idle, aim, fire }

class Turret extends Behaviour {
  GameObject? target;
  Spinner? spinner;
  GameObject? gone;
  Mode mode = Mode.idle;
  NativeComponent<PointLight>? lamp;
  NativeComponent<PointLight>? dark;
}

/// What the generator emits for the two classes.
final spinnerType = BehaviourType(
  id: 'game.Spinner',
  create: Spinner.new,
  fields: [
    BehaviourField(
      name: 'speed',
      type: 'number',
      get: (b) => (b as Spinner).speed,
      set: (b, v) => (b as Spinner).speed = (v as num).toDouble(),
    ),
  ],
);
final turretType = BehaviourType(
  id: 'game.Turret',
  create: Turret.new,
  fields: [
    BehaviourField(
      name: 'target',
      type: 'gameObject',
      get: (b) => referenceJson((b as Turret).target),
      target: (b) => (b as Turret).target,
      set: (b, v) => (b as Turret).target = v as GameObject?,
    ),
    BehaviourField(
      name: 'spinner',
      type: 'behaviour',
      get: (b) => referenceJson((b as Turret).spinner),
      target: (b) => (b as Turret).spinner,
      set: (b, v) => (b as Turret).spinner = v as Spinner?,
    ),
    BehaviourField(
      name: 'gone',
      type: 'gameObject',
      get: (b) => referenceJson((b as Turret).gone),
      target: (b) => (b as Turret).gone,
      set: (b, v) => (b as Turret).gone = v as GameObject?,
    ),
    BehaviourField(
      name: 'lamp',
      type: 'component',
      component: PointLight.schemaName,
      get: (b) => referenceJson((b as Turret).lamp),
      set: (b, v) => (b as Turret).lamp = nativeComponentOf<PointLight>(v),
      target: (b) => (b as Turret).lamp,
    ),
    BehaviourField(
      name: 'dark',
      type: 'component',
      component: PointLight.schemaName,
      get: (b) => referenceJson((b as Turret).dark),
      set: (b, v) => (b as Turret).dark = nativeComponentOf<PointLight>(v),
      target: (b) => (b as Turret).dark,
    ),
    BehaviourField(
      name: 'mode',
      type: 'enum',
      get: (b) => (b as Turret).mode.name,
      set: (b, v) => (b as Turret).mode = Mode.values.byName(v as String),
    ),
  ],
);

const sceneId = 'e0000000-0000-4000-8000-000000000001';
const prefabId = 'e0000000-0000-4000-8000-000000000002';
const turretId = 'e0000000-0000-4000-8000-0000000000a1';
const spinnerObjectId = 'e0000000-0000-4000-8000-0000000000a2';
const plainId = 'e0000000-0000-4000-8000-0000000000a3';
const spinnerComponentId = 'e0000000-0000-4000-8000-0000000000c2';

Map<String, Object?> transform() => {
  'type': 'fynkel.Transform',
  'version': 1,
  'data': {
    'position': [0, 0, 0],
    'rotation': [0, 0, 0, 1],
    'scale': [1, 1, 1],
  },
};

Map<String, Object?> light() => {
  'type': 'fynkel.PointLight',
  'version': 1,
  'data': {
    'color': [1, 1, 1],
    'intensity': 40.0,
    'range': 10.0,
    'culling_mask': 4294967295,
  },
};

Map<String, Object?> entity(
  String name,
  Map<String, Object?> components, {
  String? parent,
  List<String> children = const [],
}) => {'name': name, 'parent': parent, 'children': children, 'components': components};

void copyDir(Directory from, Directory to) {
  to.createSync(recursive: true);
  for (final e in from.listSync()) {
    final name = e.uri.pathSegments.lastWhere((s) => s.isNotEmpty);
    if (e is Directory) {
      if (name == '.fynkel') continue;
      copyDir(e, Directory('${to.path}/$name'));
    } else if (e is File) {
      e.copySync('${to.path}/$name');
    }
  }
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute;
  late Directory project;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    project = Directory.systemTemp.createTempSync('fynkel-object-fields-');
    copyDir(fixture, project);
    // Turret comes first, so its Spinner is attached after it.
    File('${project.path}/assets/scenes/refs.scene.json').writeAsStringSync(
      jsonEncode({
        r'$schema': 'https://fynkel.dev/schemas/scene.schema.json',
        'formatVersion': 7,
        'id': sceneId,
        'name': 'Refs',
        'entities': {
          turretId: entity('Turret', {
            'e0000000-0000-4000-8000-0000000000c0': transform(),
            'e0000000-0000-4000-8000-0000000000c1': {
              'type': 'game.Turret',
              'version': 1,
              'data': {
                'target': {'entity': plainId},
                'spinner': {'entity': spinnerObjectId, 'component': spinnerComponentId},
                'gone': {'entity': 'e0000000-0000-4000-8000-0000000000ff'},
                'mode': 'fire',
                'lamp': {'entity': plainId},
                'dark': {'entity': spinnerObjectId},
              },
            },
          }),
          spinnerObjectId: entity('Spinning', {
            'e0000000-0000-4000-8000-0000000000c3': transform(),
            spinnerComponentId: {
              'type': 'game.Spinner',
              'version': 1,
              'data': {'speed': 3.0},
            },
          }),
          plainId: entity('Plain', {
            'e0000000-0000-4000-8000-0000000000c4': transform(),
            'e0000000-0000-4000-8000-0000000000c5': light(),
          }),
        },
      }),
    );
    // A prefab whose root's script references its own child.
    File('${project.path}/assets/prefabs/turret.prefab.json').writeAsStringSync(
      jsonEncode({
        r'$schema': 'https://fynkel.dev/schemas/prefab.schema.json',
        'formatVersion': 7,
        'id': prefabId,
        'name': 'TurretPrefab',
        'entities': {
          'e0000000-0000-4000-8000-0000000000b1': entity(
            'TurretPrefab',
            {
              'e0000000-0000-4000-8000-0000000000d0': transform(),
              'e0000000-0000-4000-8000-0000000000d1': {
                'type': 'game.Turret',
                'version': 1,
                'data': {
                  'target': {'entity': 'e0000000-0000-4000-8000-0000000000b2'},
                  'spinner': {
                    'entity': 'e0000000-0000-4000-8000-0000000000b2',
                    'component': 'e0000000-0000-4000-8000-0000000000d3',
                  },
                  'lamp': {'entity': 'e0000000-0000-4000-8000-0000000000b2'},
                },
              },
            },
            children: ['e0000000-0000-4000-8000-0000000000b2'],
          ),
          'e0000000-0000-4000-8000-0000000000b2': entity('Barrel', {
            'e0000000-0000-4000-8000-0000000000d2': transform(),
            'e0000000-0000-4000-8000-0000000000d4': light(),
            'e0000000-0000-4000-8000-0000000000d3': {
              'type': 'game.Spinner',
              'version': 1,
              'data': {'speed': 2.0},
            },
          }, parent: 'e0000000-0000-4000-8000-0000000000b1'),
        },
        'root': 'e0000000-0000-4000-8000-0000000000b1',
      }),
    );
    cache = '${project.path}/.fynkel/cache';
    cookProject(project.path, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    project.deleteSync(recursive: true);
    lib.dispose();
  });

  World world() {
    final w = World.create(library: lib);
    addTearDown(w.dispose);
    w.registry
      ..register(spinnerType)
      ..register(turretType);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(project.path, cacheDir: cache);
    return w;
  }

  test(
    'a scene: the object, the script on it (attached later), the enum; a gone target is None with FYN-B0003',
    () {
      final w = world();
      final report = w.loadScene(sceneId, cacheDir: cache);
      final turret = w.behaviours.whereType<Turret>().single;
      final spinner = w.behaviours.whereType<Spinner>().single;
      expect(turret.target?.name, 'Plain');
      expect(turret.target?.authoringId, plainId);
      expect(identical(turret.spinner, spinner), isTrue);
      expect(turret.spinner!.speed, 3.0);
      expect(turret.mode, Mode.fire);
      expect(turret.gone, isNull);
      final warnings = report.warnings.where((d) => d.code == 'FYN-B0003').toList();
      expect(warnings.map((d) => d.pointer?.split('/').last), unorderedEquals(['gone', 'dark']));
      expect(
        warnings.firstWhere((d) => d.pointer!.endsWith('/data/gone')).message,
        contains('game.Turret.gone'),
      );
      // A built-in component field (ADR-0036 §2): the light on that object;
      // an object without one is None.
      expect(turret.lamp, same(turret.target!.component<PointLight>()));
      expect(turret.lamp!.value.intensity, 40);
      expect(turret.dark, isNull);
      expect(
        warnings.firstWhere((d) => d.pointer!.endsWith('/data/dark')).message,
        contains('no fynkel.PointLight'),
      );

      // What the Inspector reads back in Play: the same JSON as the scene.
      expect(turretType.field('target')!.get(turret), {'entity': plainId});
      expect(turretType.field('spinner')!.get(turret), {
        'entity': spinnerObjectId,
        'component': spinnerComponentId,
      });
      expect(turretType.field('gone')!.get(turret), isNull);
      expect(turretType.field('lamp')!.get(turret), {'entity': plainId});

      // Removing the light clears the field at the next settle (ADR-0036 §5).
      turret.lamp!.destroy();
      w.frame(1 / 60);
      expect(turret.lamp, isNull);
    },
  );

  test('a clone\'s component field points at the copy\'s own component', () {
    final w = world();
    final a = w.instantiate(prefabId);
    w.frame(1 / 60);
    final copy = a.clone();
    w.frame(1 / 60);
    final t = copy.behaviour<Turret>()!;
    expect(t.lamp!.gameObject.parent, copy);
    expect(t.lamp, isNot(same(a.behaviour<Turret>()!.lamp)));
  });

  test('a prefab instance references its own copy, not another instance\'s', () {
    final w = world();
    final a = w.instantiate(prefabId);
    final b = w.instantiate(prefabId);
    w.frame(1 / 60);
    final turrets = [a.behaviour<Turret>()!, b.behaviour<Turret>()!];
    for (final (i, root) in [a, b].indexed) {
      final t = turrets[i];
      expect(t.target, isNotNull);
      expect(t.target!.parent, root, reason: 'its own Barrel');
      expect(identical(t.spinner, t.target!.behaviour<Spinner>()), isTrue);
      expect(t.lamp, same(t.target!.component<PointLight>()), reason: 'its own Barrel\'s light');
    }
    expect(identical(turrets[0].target, turrets[1].target), isFalse);
  });

  test('the registry leaves reference fields to the world', () {
    final w = World.create(library: lib);
    addTearDown(w.dispose);
    w.registry.register(turretType);
    final t = w.registry.instantiate(
      'game.Turret',
      fields: {
        'target': {'entity': plainId},
        'mode': 'aim',
      },
    ) as Turret;
    expect((t.target, t.mode), (null, Mode.aim));
  });
}
