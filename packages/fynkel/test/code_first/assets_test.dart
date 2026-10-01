// Code first, Tier 2 (docs/code-first.md, ADR-0026 §3): an asset named by
// its path while the game runs — `world.assets.find('prefabs/crate')`, as
// Unity's `Resources.Load` — from the cooked cache's index, over the real
// native library. (Names known while writing are the generated `Assets.…`
// constants; apps/cli/tests/create.rs covers those.)

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-code-first-assets-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.loadProject(fixture, cacheDir: cache);
    return w;
  }

  test('a prefab found by its path spawns; a scene found by its path loads', () async {
    final w = loaded();
    final crate = w.assets.find('prefabs/crate.prefab.json');
    expect(crate, const AssetRef('c0000000-0000-4000-8000-000000000040'));
    expect(w.assets.find('prefabs/crate'), crate, reason: 'the extension may be left out');
    expect(w.assets.find('assets/prefabs/crate.prefab.json'), crate, reason: 'from the project root too');
    expect(w.assets.pathOf(crate!), 'prefabs/crate.prefab.json');

    final spawned = w.instantiate(crate, name: 'From a path');
    w.frame(1 / 60);
    expect(w.find('From a path'), spawned);
    expect(spawned.get<MeshRenderer>()!.mesh.isNone, isFalse, reason: 'the look of the prefab came with it');

    final results = w.assets.find('scenes/results')!;
    final report = w.scenes.transition(results);
    w.frame(1 / 60);
    await report;
    expect(w.scenes.current, results.asset);
  });

  test('a sub-asset, a missing path, and a name two files share', () {
    final w = loaded();
    final robot = w.assets.find('models/robot.glb', sub: 'prefab')!;
    expect(robot.sub, 'prefab');
    expect(w.assets.pathOf(robot), 'models/robot.glb');
    expect(w.assets.all.map((a) => a.path), containsAll(['models/robot.glb', 'audio/tone.wav']));
    expect(w.assets.all.firstWhere((a) => a.path == 'audio/tone.wav').kind, 'fynkel.audio');

    expect(w.assets.find('prefabs/nothing'), isNull);
    expect(w.assets.pathOf(const AssetRef('00000000-0000-4000-8000-00000000abcd')), isNull);
    expect(
      () => w.assets.find('rigs/column'),
      throwsA(
        isA<StateError>().having(
          (e) => e.message,
          'message',
          allOf(contains('column.rig.json'), contains('column.blend.json')),
        ),
      ),
    );
  });

  test('before any load there is no cache to read', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    expect(() => w.assets.find('prefabs/crate'), throwsStateError);
  });
}
