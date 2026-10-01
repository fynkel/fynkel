import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

import 'scenes_test.dart' as fixture;

void main() {
  late NativeLibrary lib;
  late Directory cache;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-gameplay-scenes-');
    cookProject(
      Directory('../../crates/fynkel_document/fixtures/project').absolute.path,
      cacheDir: cache.path,
    );
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    lib.dispose();
    cache.deleteSync(recursive: true);
  });

  World fresh() {
    final world = World.create(library: lib);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    world.loadScene(fixture.playground, cacheDir: cache.path);
    return world;
  }

  test('callback scene requests publish after commit and load at the next frame', () async {
    final world = fresh();
    late Future<LoadReport> request;
    world.frame(
      0,
      update: () {
        request = world.scenes.transition(fixture.results);
        expect(world.scenes.state, SceneState.ready);
        expect(world.scenes.pending, isNull);
      },
    );
    expect(world.scenes.state, SceneState.pending);
    expect(world.scenes.current, fixture.playground);
    world.frame(0);
    expect((await request).sceneId, fixture.results);
    expect(world.scenes.current, fixture.results);
  });

  test('aborted callback scene requests fail their futures without changing scene state', () async {
    final world = fresh();
    late Future<LoadReport> aborted;
    expect(
      () => world.frame(
        0,
        update: () {
          aborted = world.scenes.transition(fixture.playground);
          throw StateError('callback failed');
        },
      ),
      throwsStateError,
    );
    await expectLater(aborted, throwsStateError);
    expect(world.scenes.pending, isNull);
    expect(world.scenes.state, SceneState.ready);
    world.frame(0);
    expect(world.scenes.current, fixture.playground);
  });

  test('explicit edit abort leaves scene state unchanged and resolves rejected futures', () async {
    final world = fresh();
    final original = world.scenes.transition(fixture.results);
    late Future<LoadReport> aborted;
    expect(
      () => world.edit(() {
        aborted = world.scenes.transition(fixture.playground);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    await expectLater(aborted, throwsStateError);
    expect(world.scenes.pending, fixture.results);
    world.frame(0);
    expect((await original).sceneId, fixture.results);
  });

  test('scene cancellation follows commit order and aborted cancellation changes nothing', () async {
    final world = fresh();
    late Future<LoadReport> request;
    world.edit(() {
      request = world.scenes.transition(fixture.results);
      world.scenes.cancel();
      expect(world.scenes.pending, isNull);
    });
    await expectLater(request, throwsStateError);
    expect(world.scenes.state, SceneState.ready);
    final original = world.scenes.transition(fixture.results);
    expect(
      () => world.edit(() {
        world.scenes.cancel();
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(world.scenes.pending, fixture.results);
    world.frame(0);
    expect((await original).sceneId, fixture.results);
  });

  test('zone subscribers commit independently and a failed subscriber does not stop the next', () {
    final world = fresh();
    world.instantiate(
      fixture.coinPrefab,
      transform: identityTransform.copyWith(position: const Vec3(0, 0, -2)),
    );
    final player = world.find('Player')!;
    final before = player.name;
    var secondRan = false;
    world.zones.on('collect', (_) {
      player.name = 'aborted';
      throw StateError('zone subscriber failed');
    });
    world.zones.on('collect', (_) {
      expect(player.name, before);
      player.name = 'committed';
      secondRan = true;
    });
    final recording = loadInputRecording(
      '${Directory('../../crates/fynkel_document/fixtures/project').absolute.path}/test/platform.input.json',
    );
    expect(() => recording.replay(world), throwsStateError);
    expect(secondRan, isTrue);
    expect(player.name, 'committed');
  });

  test('background scene cancellation is discarded on abort and executed on commit', () async {
    final world = fresh();
    final loading = world.scenes.loadAsync(fixture.results);
    expect(
      () => world.edit(() {
        loading.cancel();
        expect(loading.isDone, isFalse);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(loading.isDone, isFalse);
    expect(world.scenes.loading, contains(loading));
    world.edit(() {
      loading.cancel();
      expect(loading.isDone, isFalse);
    });
    await expectLater(loading.done, throwsStateError);
    expect(loading.isDone, isTrue);
    expect(world.scenes.loading, isEmpty);
    expect(world.scenes.current, fixture.playground);
  });

  test('a failed boundary listener does not strand cancelled or later staged requests', () async {
    final world = fresh();
    final original = world.scenes.transition(fixture.results);
    world.scenes.addListener((state) {
      if (state == SceneState.ready) throw StateError('listener failed');
    });
    late Future<LoadReport> later;
    expect(
      () => world.edit(() {
        world.scenes.cancel();
        later = world.scenes.transition(fixture.results);
      }),
      throwsStateError,
    );
    await expectLater(original, throwsStateError);
    expect(world.scenes.pending, fixture.results);
    // The load still occurs, and its listener failure settles its future too.
    expect(() => world.frame(0), throwsStateError);
    await expectLater(later, throwsStateError);
    expect(world.scenes.current, fixture.results);
  });

  test('dispose settles transitions staged both before and after it', () async {
    final world = fresh();
    late Future<LoadReport> before;
    late Future<LoadReport> after;
    world.frame(
      0,
      update: () {
        before = world.scenes.transition(fixture.results);
        world.dispose();
        after = world.scenes.transition(fixture.playground);
        expect(world.isDisposed, isFalse);
      },
    );
    expect(world.isDisposed, isTrue);
    await expectLater(before, throwsStateError);
    await expectLater(after, throwsStateError);
    expect(world.scenes.pending, isNull);
  });

  test('callback time scale persistence and disposal are discarded together on abort', () {
    final world = fresh();
    final root = world.spawn(name: 'root');
    expect(
      () => world.frame(
        0,
        update: () {
          world.timeScale = 0.5;
          world.dontDestroyOnLoad(root);
          world.dispose();
          expect(world.timeScale, 0.5);
          throw StateError('abort');
        },
      ),
      throwsStateError,
    );
    expect(world.isDisposed, isFalse);
    expect(world.timeScale, 1);
    expect(world.persistentObjects, isNot(contains(root)));
    world.frame(
      0,
      update: () {
        world.timeScale = 0.5;
        world.dontDestroyOnLoad(root);
      },
    );
    expect(world.timeScale, 0.5);
    expect(world.persistentObjects, contains(root));
  });
}
