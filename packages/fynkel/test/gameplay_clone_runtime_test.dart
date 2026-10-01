import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late Directory cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-clone-runtime-');
    cookProject(fixture, cacheDir: cache.path);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    lib.dispose();
    cache.deleteSync(recursive: true);
  });

  World fresh() {
    final world = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    world.onLoadDiagnostics = (_) {};
    addTearDown(world.dispose);
    return world;
  }

  test('clones copy body configuration but start with independent zero velocity', () {
    final world = fresh();
    final source = world.edit(
      () => world.spawn(name: 'moving')
        ..set(const RigidBody(gravityScale: 0, linearDamping: 0, angularDamping: 0))
        ..set(const Collider()),
    );
    world.frame(1 / 60);
    world.edit(() {
      source.setLinearVelocity(const Vec3(3, 0, 0));
      source.setAngularVelocity(const Vec3(0, 2, 0));
    });
    world.frame(1 / 60);
    final copy = world.edit(() => source.clone(transform: const Pose(position: Vec3(10, 0, 0))));
    expect(copy.get<RigidBody>(), source.get<RigidBody>());
    expect(copy.get<Collider>(), source.get<Collider>());
    world.frame(1 / 60);
    expect(source.bodyState()!.linearVelocity.x, closeTo(3, 1e-5));
    expect(source.bodyState()!.angularVelocity.y, closeTo(2, 1e-5));
    expect(copy.bodyState()!.linearVelocity.length, closeTo(0, 1e-5));
    expect(copy.bodyState()!.angularVelocity.length, closeTo(0, 1e-5));
  });

  test('cloned audio shares the clip but starts a fresh voice rather than copying pause or cursor', () {
    final world = fresh()..loadProject(fixture, cacheDir: cache.path);
    const clip = AssetRef('c0000000-0000-4000-8000-000000000006');
    final source = world.edit(
      () => world.spawn(name: 'loop')..set(const AudioSource(clip: clip, playing: true, looping: true)),
    );
    for (var i = 0; i < 5; i++) {
      world.frame(1 / 60);
    }
    world.edit(source.audio.pause);
    world.frame(1 / 60);
    final previous = source.audio.state!;
    expect(previous.paused, isTrue);
    expect(previous.time, greaterThan(0.02));
    final started = world.audio.stats.started;
    final copy = world.edit(() => source.clone());
    expect(copy.get<AudioSource>(), source.get<AudioSource>());
    expect(copy.get<AudioSource>()!.clip, clip);
    expect(world.audio.stats.started, started, reason: 'cloning configuration starts no voice before a tick');
    world.frame(1 / 60);
    final copied = copy.audio.state!;
    expect(copied.playing, isTrue);
    expect(copied.paused, isFalse);
    expect(copied.voice, isNot(previous.voice));
    expect(copied.time, lessThan(previous.time));
    expect(source.audio.state!.time, previous.time);
    expect(world.audio.stats.started, started + 1);
  });

  test('cloned animator shares registered clip resources with a fresh playback cursor', () {
    final world = fresh();
    world.loadScene('c0000000-0000-4000-8000-000000000034', cacheDir: cache.path);
    final source = world.find('Rig')!;
    for (var i = 0; i < 12; i++) {
      world.frame(1 / 60);
    }
    final previous = source.animation.state!.time;
    expect(previous, greaterThan(0.1));
    final copy = world.edit(() => source.clone(name: 'Copied rig'));
    expect(copy.get<Animator>(), source.get<Animator>());
    expect(
      copy.animation.state!.time,
      0,
      reason: 'the synthesized state starts at zero, not the source cursor',
    );
    world.frame(1 / 60);
    expect(copy.animation.state!.time, 0, reason: 'first tick initializes fresh playback');
    expect(source.animation.state!.time, closeTo(previous + 1 / 60, 1e-5));
    world.edit(source.destroy);
    world.frame(1 / 60);
    expect(copy.animation.state!.time, closeTo(1 / 60, 1e-5), reason: 'shared clip outlives the source');
  });

  test('cloning cooked prefab configuration is atomic and keeps shared mesh references', () {
    final world = fresh()..loadProject(fixture, cacheDir: cache.path);
    final source = world.instantiate(world.assets.find('prefabs/coin')!, name: 'Coin');
    final count = world.allEntities().length;
    late GameObject aborted;
    expect(
      () => world.edit(() {
        aborted = source.clone(name: 'discard');
        expect(aborted.get<MeshRenderer>(), source.get<MeshRenderer>());
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(aborted.isAlive, isFalse);
    expect(world.allEntities(), hasLength(count));
    final copy = world.edit(() => source.clone(name: 'copy'));
    expect(copy.get<MeshRenderer>(), source.get<MeshRenderer>());
    expect(copy.get<TriggerZone>(), source.get<TriggerZone>());
    world.edit(source.destroy);
    world.frame(1 / 60);
    expect(copy.isAlive, isTrue);
    expect(copy.get<MeshRenderer>(), isNotNull);
  });

  test('mutated lifetime remains component data and is copied at its current value', () {
    final world = fresh();
    final source = world.edit(() => world.spawn(name: 'temporary')..set(const LifetimeDespawn(seconds: 1)));
    for (var i = 0; i < 12; i++) {
      world.frame(1 / 60);
    }
    final remaining = source.get<LifetimeDespawn>()!.seconds;
    expect(remaining, lessThan(1));
    final copy = world.edit(() => source.clone());
    expect(copy.get<LifetimeDespawn>()!.seconds, remaining);
  });

  test('detached clones have runtime ownership rather than source scene or persistence membership', () {
    final world = fresh()..loadProject(fixture, cacheDir: cache.path);
    final source = world.find('Robot')!;
    final sceneId = world.loadedScenes.first.id;
    final copy = world.edit(() => source.clone(name: 'runtime robot'));
    world.unloadScene(sceneId);
    world.drainEvents();
    expect(source.isAlive, isFalse);
    expect(copy.isAlive, isTrue);
    final persistent = world.edit(() => world.spawn(name: 'persistent'));
    world.dontDestroyOnLoad(persistent);
    expect(world.persistentObjects, contains(persistent));
    final ordinary = world.edit(() => persistent.clone(name: 'ordinary'));
    expect(world.persistentObjects, isNot(contains(ordinary)));
  });
}
