// Code first, Tier 3 (docs/code-first.md, ADR-0026 §2): scenes and
// prefabs written from code by `package:fynkel/authoring.dart` — the same
// `ObjectSpec`s `world.build` spawns, as documents `fynkel validate`
// accepts, that load into the same world; written again, the same bytes;
// objects added in the editor kept. Over a copy of the fixture project,
// the `fynkel` CLI and the real native library.

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/authoring.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

/// The fixture's `game.Spinner` (schemas/behaviours/game.Spinner.schema.json).
class Spinner extends Behaviour {
  double speed = 1;
  Vec3 axis = Vec3.up;
  bool spinning = true;
}

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
    BehaviourField(
      name: 'axis',
      type: 'vec3',
      get: (b) => [(b as Spinner).axis.x, b.axis.y, b.axis.z],
      set: (b, v) {
        final l = (v as List).cast<num>();
        (b as Spinner).axis = Vec3(l[0].toDouble(), l[1].toDouble(), l[2].toDouble());
      },
    ),
    BehaviourField(
      name: 'spinning',
      type: 'boolean',
      get: (b) => (b as Spinner).spinning,
      set: (b, v) => (b as Spinner).spinning = v as bool,
    ),
  ],
);

void registerBehaviours(BehaviourRegistry r) => r.register(spinnerType);

/// A code prefab.
ObjectSpec lamp() => ObjectSpec(
      'Lamp',
      transform: const Transform(position: Vec3(0, 2, 0)),
      components: const [PointLight(range: 5)],
      children: [
        ObjectSpec('Bulb', components: const [PointLight(range: 1)], behaviours: [Spinner()..speed = 2]),
      ],
    );

List<ObjectSpec> level(AssetRef lampRef, AssetRef robotRef) => [
      const ObjectSpec('Floor', components: [Collider(size: Vec3(20, 0.5, 20), offset: Vec3(0, -0.5, 0))]),
      for (var i = 0; i < 3; i++)
        ObjectSpec(
          'Crate',
          transform: Transform(position: Vec3(i * 2.0, 1, 0)),
          layer: 'props',
          tags: const ['pickup'],
          components: const [RigidBody(mass: 2), Collider()],
        ),
      ObjectSpec.primitive(Primitive.sphere, transform: const Transform(position: Vec3(0, 4, 0))),
      ObjectSpec.prefab(
        lampRef,
        name: 'Red lamp',
        transform: const Transform(position: Vec3(5, 3, 0)),
        tags: const ['pickup'],
        components: const [PointLight(range: 9), Rotator()],
        behaviours: [Spinner()..speed = 4],
        children: const [ObjectSpec('Shade')],
      ),
      ObjectSpec.prefab(robotRef, transform: const Transform(position: Vec3(-3, 0, 0))),
      const ObjectSpec('Hidden', active: false, children: [ObjectSpec('Inside')]),
    ];

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String project;
  late NativeLibrary lib;
  late AssetRef robotRef;

  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  setUp(() {
    project = Directory.systemTemp.createTempSync('fynkel-code-first-authoring-').path;
    Process.runSync('cp', ['-R', '$fixture/.', project]);
    final stale = Directory('$project/.fynkel');
    if (stale.existsSync()) stale.deleteSync(recursive: true);
    final file = File('$project/fynkel.project.json');
    final json = jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
    json['tags'] = ['pickup'];
    file.writeAsStringSync(const JsonEncoder.withIndent('  ').convert(json));
    cookProject(project);
    final meta = jsonDecode(File('$project/assets/models/robot.glb.meta.json').readAsStringSync()) as Map<String, Object?>;
    robotRef = AssetRef(meta['id'] as String, sub: 'prefab');
    addTearDown(() => Directory(project).deleteSync(recursive: true));
  });

  ProjectWriter writer() => ProjectWriter(project, registerBehaviours: registerBehaviours);

  (WrittenDocument, WrittenDocument) writeAll(ProjectWriter w) {
    final prefab = w.writePrefab('prefabs/lamp', lamp());
    final scene = w.writeScene('scenes/code', SceneSpec('Code', level(prefab.ref, robotRef)));
    return (prefab, scene);
  }

  void validate() {
    final r = runFynkel(['validate', '--project', project, '--json']);
    expect(r.exitCode, 0, reason: '${r.stdout}${r.stderr}');
  }

  World world() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..registry.register(spinnerType);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (d) => fail('load diagnostics: $d');
    w.loadProject(project);
    w.layersAndTags = const LayersAndTags(layers: ['default', 'props'], tags: ['pickup']);
    return w;
  }

  /// Every object under [roots], depth first: path, active, components, scripts, pose, layer, tags.
  List<String> shape(World w, Iterable<GameObject> roots) {
    final out = <String>[];
    void walk(GameObject o, String at) {
      final path = '$at/${o.name}';
      final types = [for (final c in o.components) c.runtimeType.toString()]..sort();
      final scripts = [for (final b in o.behaviours) '${b.runtimeType}(${(b as Spinner).speed})'];
      out.add('$path ${o.activeSelf ? '' : '(off) '}$types $scripts ${o.localTransform.position} '
          '${o.layer} ${o.tags}');
      for (final c in o.children) {
        walk(c, path);
      }
    }

    for (final r in roots) {
      walk(r, '');
    }
    return out;
  }

  // Every root name the fixture scene and the level use.
  const names = ['Floor', 'Crate', 'Sphere', 'Red lamp', 'Robot', 'Hidden', 'Camera', 'Sun', 'Crate A', 'Crate B'];

  test('a written scene validates, and loads into the world build makes', () {
    final (prefab, scene) = writeAll(writer());
    expect(prefab.changed && scene.changed, isTrue);
    validate();
    cookProject(project);

    final written = world()..loadScene(scene.ref.asset);
    written.frame(1 / 60);
    final built = world();
    for (final o in [for (final name in names) ...built.findAll(name)]) {
      o.destroy(); // the startup scene's
    }
    level(prefab.ref, robotRef).forEach(built.build);
    built.frame(1 / 60);

    List<String> roots(World w) =>
        shape(w, [for (final name in names) ...w.findAll(name)].where((o) => o.parent == null))..sort();
    expect(roots(written), roots(built));
    expect(roots(written).where((l) => l.startsWith('/Crate ')), hasLength(3));
  });

  test('written again: the same bytes, the file untouched', () {
    final (prefab, scene) = writeAll(writer());
    final bytes = File('$project/assets/${scene.path}').readAsStringSync();
    final (prefab2, scene2) = writeAll(writer());
    expect(prefab2.changed, isFalse);
    expect(scene2.changed, isFalse);
    expect(scene2.ref, scene.ref, reason: 'the id is kept');
    expect(prefab2.ref, prefab.ref);
    expect(File('$project/assets/${scene.path}').readAsStringSync(), bytes);
  });

  test('objects added in the editor are kept; the code\'s are replaced', () {
    final (_, scene) = writeAll(writer());
    final file = File('$project/assets/${scene.path}');
    final doc = jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
    final entities = doc['entities'] as Map<String, Object?>;
    final floor = entities.entries.firstWhere((e) => (e.value as Map)['name'] == 'Floor').key;
    // The editor adds a Crate of its own under the Floor, and a root.
    const mine = 'e4000000-0000-4000-8000-000000000001', root = 'e4000000-0000-4000-8000-000000000002';
    entities[mine] = {'name': 'Crate', 'parent': floor, 'children': [], 'components': {}};
    ((entities[floor] as Map)['children'] as List).add(mine);
    entities[root] = {'name': 'Notes', 'parent': null, 'children': [], 'components': {}};
    file.writeAsStringSync(const JsonEncoder.withIndent('  ').convert(doc));

    final again = writer().writeScene('scenes/code', SceneSpec('Code', level(writer().writePrefab('prefabs/lamp', lamp()).ref, robotRef)));
    expect(again.kept, unorderedEquals(['Crate', 'Notes']));
    final after = (jsonDecode(file.readAsStringSync()) as Map<String, Object?>)['entities'] as Map<String, Object?>;
    expect(((after[floor] as Map)['children'] as List), [mine]);
    expect(after[root], isNotNull);
    expect(after.values.where((e) => (e as Map)['name'] == 'Crate'), hasLength(4), reason: 'three of code, one of the editor');
    validate();

    // Code that no longer makes an object drops it; the editor's stay.
    writer().writeScene('scenes/code', const SceneSpec('Code', [ObjectSpec('Floor')]));
    final fewer = (jsonDecode(file.readAsStringSync()) as Map<String, Object?>)['entities'] as Map<String, Object?>;
    expect(fewer.values.map((e) => (e as Map)['name']), unorderedEquals(['Floor', 'Crate', 'Notes']));
  });

  test('mistakes throw before anything is written', () {
    final w = writer();
    expect(() => ProjectWriter(project).writePrefab('prefabs/x', lamp()), throwsArgumentError, reason: 'unregistered script');
    expect(() => w.writeScene('scenes/x', const SceneSpec('X', [ObjectSpec('A', tags: ['nope'])])), throwsArgumentError);
    expect(() => w.writeScene('scenes/x', const SceneSpec('X', [ObjectSpec('A', layer: 'nope')])), throwsArgumentError);
    expect(
      () => w.writeScene('scenes/x', const SceneSpec('X', [ObjectSpec.prefab(AssetRef('00000000-0000-4000-8000-00000000abcd'))])),
      throwsArgumentError,
    );
    expect(
      () => w.writeScene('scenes/x', SceneSpec('X', [ObjectSpec('A', behaviours: [Spinner(), Spinner()])])),
      throwsArgumentError,
      reason: 'one script of a type per object in a document',
    );
    expect(File('$project/assets/scenes/x.scene.json').existsSync(), isFalse);
    Directory('$project/.fynkel').deleteSync(recursive: true);
    expect(() => writer().writeScene('scenes/x', SceneSpec('X', [ObjectSpec.prefab(robotRef)])), throwsStateError,
        reason: 'a model\'s prefab needs the cook');
  });
}
