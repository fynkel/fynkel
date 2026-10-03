// Code first (ADR-0026): a project that names no startup scene — a game
// that builds its world from code. `loadProject` opens it with nothing
// loaded: its layers, tags, input and assets ready (a prefab
// instantiated, a sound played by id, a primitive built), the scene
// manager empty until a scene is loaded. Over a copy of the fixture
// project and the real native library.

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String project;
  late NativeLibrary lib;

  setUpAll(() {
    lib = NativeLibrary.open();
    project = Directory.systemTemp.createTempSync('fynkel-code-first-no-scene-').path;
    Process.runSync('cp', ['-R', '$fixture/.', project]);
    final stale = Directory('$project/.fynkel');
    if (stale.existsSync()) stale.deleteSync(recursive: true);
    final file = File('$project/fynkel.project.json');
    final json = jsonDecode(file.readAsStringSync()) as Map<String, Object?>..remove('startupScene');
    file.writeAsStringSync(const JsonEncoder.withIndent('  ').convert(json));
    cookProject(project);
  });
  tearDownAll(() {
    Directory(project).deleteSync(recursive: true);
    lib.dispose();
  });

  test('the project opens empty; everything is built from code', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    final report = w.loadProject(project);
    expect(report.ok, isTrue);
    expect(report.sceneId, isNull);
    expect(w.scenes.state, SceneState.empty);
    expect(w.scenes.current, isNull);
    expect(w.layersAndTags.layers, ['default', 'props']);
    expect(report.inputActions.map((a) => a.name), contains('move'));

    final floor = w.createPrimitive(Primitive.plane);
    final coin = w.instantiate(w.assets.find('prefabs/coin')!, name: 'Coin');
    final crate = w.build(
      const ObjectSpec(
        'Crate',
        layer: 'props',
        transform: Pose(position: Vec3(3, 2, 0)),
        components: [RigidBody(), Collider()],
      ),
    );
    final diagnostics = <FynkelDiagnostic>[];
    w.onDiagnostics = diagnostics.addAll;
    w.audio.play(w.assets.find('audio/tone.wav')!);
    for (var i = 0; i < 120; i++) {
      w.frame(1 / 60);
    }
    expect(diagnostics, isEmpty, reason: 'the sound is registered from the cache on demand');
    expect(w.audio.stats.started, 1);
    expect(coin.get<TriggerZone>(), isNotNull);
    expect(floor.get<Collider>()!.shape, ColliderShape.mesh);
    expect(crate.localTransform.position.y, closeTo(0.5, 0.05), reason: 'gravity from the project');
    expect(crate.layer, 'props');
  });

  test('a scene still loads by id afterwards', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.loadProject(project);
    w.loadScene('c0000000-0000-4000-8000-000000000030');
    expect(w.scenes.state, SceneState.ready);
    expect(w.find('Camera'), isNotNull);
  });
}
