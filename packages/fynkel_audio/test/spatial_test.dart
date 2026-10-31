// 3D sound through the package (docs/stage-11.md phase 2): sources heard
// from where their objects are, by the camera or an AudioListener, moving
// and turning with them; playAt; panning; the debug spheres; the values
// refused. Each test listens to the mix with audio.render.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';
import 'package:test/test.dart';

/// A second of a steady level of 0.5, which loops.
AudioClip level() => AudioClip.fromSamples(Float32List(48000)..fillRange(0, 48000, 0.5), name: 'level');

World newWorld() {
  final world = World(source: (name) async => throw StateError('no file $name'));
  addTearDown(world.dispose);
  return world;
}

/// How loud each ear is: the level of the last of 2,048 frames rendered,
/// once the mixer has ramped to where the sound is.
(double, double) ears(World world) {
  final out = world.audio.render(2048);
  return (out[out.length - 2], out[out.length - 1]);
}

/// A 3D source of [level] on an object at [at], playing.
AudioSource sound(World world, Vec3 at, {double maxDistance = 50}) {
  final source = AudioSource(clip: level(), loop: true, spatialBlend: 1, maxDistance: maxDistance);
  world.add(
    GameObject(
      name: 'sound',
      transform: Transform(position: at),
      components: [source],
    ),
  );
  return source;
}

void main() {
  test('the camera hears a 3D source in the ear it is beside', () {
    final world = newWorld();
    world.add(GameObject(name: 'camera', components: [Camera()]));
    sound(world, Vec3(-3, 0, 0));
    world.update(1 / 60);
    var (l, r) = ears(world);
    expect(l, greaterThan(0.05));
    expect(r, closeTo(0, 1e-4));

    // Turned a quarter to the left, the camera faces it: the same in both.
    world.camera!.gameObject.transform = Transform(rotation: Quat.axisAngle(Vec3.up, math.pi / 2));
    world.update(1 / 60);
    (l, r) = ears(world);
    expect(l, closeTo(r, 1e-4));
    expect(l, greaterThan(0.05));
  });

  test('a source that moves is heard from where it moved to, after the frame', () {
    final world = newWorld();
    world.add(GameObject(name: 'camera', components: [Camera()]));
    final source = sound(world, Vec3(-3, 0, 0));
    world.update(1 / 60);
    source.gameObject.transform = Transform(position: Vec3(3, 0, 0));
    world.update(1 / 60);
    final (l, r) = ears(world);
    expect(l, closeTo(0, 1e-4));
    expect(r, greaterThan(0.05));
  });

  test('farther is quieter, and within the min distance it is full', () {
    final world = newWorld();
    world.add(GameObject(name: 'camera', components: [Camera()]));
    final source = sound(world, Vec3(0, 0, -0.5));
    world.update(1 / 60);
    expect(ears(world).$1, closeTo(0.5, 1e-3));
    source.gameObject.transform = Transform(position: Vec3(0, 0, -4));
    world.update(1 / 60);
    expect(ears(world).$1, closeTo(0.5 / 4, 1e-3));
    source.rolloffMode = AudioRolloffMode.linear;
    source.maxDistance = 7;
    // A straight line from 1 m (full) to 7 m (silent): half at 4 m.
    expect(ears(world).$1, closeTo(0.25, 1e-3));
  });

  test('an AudioListener hears instead of the camera, and the last enabled one hears', () {
    final world = newWorld();
    world.add(GameObject(name: 'camera', components: [Camera()]));
    sound(world, Vec3(-3, 0, 0));
    final player = world.add(
      GameObject(
        name: 'player',
        transform: Transform(position: Vec3(-6, 0, 0)),
        components: [AudioListener()],
      ),
    );
    world.update(1 / 60);
    var (l, r) = ears(world);
    expect(r, greaterThan(0.05), reason: 'the player is left of the sound, so hears it on the right');
    expect(l, closeTo(0, 1e-4));
    player.get<AudioListener>()!.enabled = false;
    world.update(1 / 60);
    (l, r) = ears(world);
    expect(l, greaterThan(0.05), reason: 'back to the camera');
  });

  test('a 2D source is the same wherever it is, and panStereo moves it', () {
    final world = newWorld();
    world.add(GameObject(name: 'camera', components: [Camera()]));
    final source = AudioSource(clip: level(), loop: true);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(-30, 0, 0)),
        components: [source],
      ),
    );
    world.update(1 / 60);
    var (l, r) = ears(world);
    expect(l, closeTo(0.5, 1e-3));
    expect(r, closeTo(0.5, 1e-3));
    source.panStereo = 1;
    (l, r) = ears(world);
    expect(l, closeTo(0, 1e-4));
    expect(r, closeTo(0.5, 1e-3));
  });

  test('playAt plays where it was played, heard from there as the camera moves', () {
    final world = newWorld();
    final camera = world.add(GameObject(name: 'camera', components: [Camera()]));
    world.update(1 / 60);
    world.audio.playAt(level(), Vec3(2, 0, 0), maxDistance: 20);
    var (l, r) = ears(world);
    expect(r, greaterThan(0.05));
    expect(l, closeTo(0, 1e-4));
    camera.transform = Transform(position: Vec3(4, 0, 0));
    world.update(1 / 60);
    (l, r) = ears(world);
    expect(l, greaterThan(0.05));
    expect(r, closeTo(0, 1e-4));
  });

  test('without a camera or listener, whoever hears is at the origin looking along -Z', () {
    final world = newWorld();
    sound(world, Vec3(3, 0, 0));
    world.update(1 / 60);
    final (l, r) = ears(world);
    expect(r, greaterThan(0.05));
    expect(l, closeTo(0, 1e-4));
  });

  test('32 3D sources, more than the first buffers hold, are each heard from where they are', () {
    final world = newWorld();
    world.add(GameObject(name: 'camera', components: [Camera()]));
    final sources = [for (var i = 0; i < Audio.maxVoices; i++) sound(world, Vec3(-3, 0, -i.toDouble()))];
    world.update(1 / 60);
    for (final source in sources) {
      source.gameObject.transform = Transform(position: Vec3(3, 0, 0));
    }
    world.update(1 / 60);
    final (l, r) = ears(world);
    expect(l, closeTo(0, 1e-4));
    expect(r, greaterThan(0.5));
    expect(world.audio.stats.frameMilliseconds, greaterThan(0));
  });

  test('drawSources draws each 3D source\'s min and max distances', () {
    final world = newWorld();
    final source = sound(world, Vec3.zero);
    source.stop();
    world.update(1 / 60);
    expect(world.debug.count, 0);
    world.audio.drawSources = true;
    world.update(1 / 60);
    expect(world.debug.count, greaterThan(0), reason: 'drawn while enabled, playing or not');
  });

  test('bad 3D values throw, and nothing changes', () {
    final world = newWorld();
    expect(() => AudioSource(spatialBlend: 1.5), throwsArgumentError);
    expect(() => AudioSource(panStereo: -2), throwsArgumentError);
    expect(() => AudioSource(minDistance: 0), throwsArgumentError);
    expect(() => AudioSource(minDistance: 5, maxDistance: 5), throwsArgumentError);
    final source = sound(world, Vec3.zero);
    expect(() => source.maxDistance = 0.5, throwsArgumentError);
    expect(source.maxDistance, 50);
    expect(() => world.audio.playAt(level(), Vec3.zero, minDistance: -1), throwsArgumentError);
    expect(() => world.audio.playAt(level(), Vec3.zero, maxDistance: 0.5), throwsArgumentError);
  });

  test("a source further away than a float holds doesn't turn the sound off (stage 14, AU5)", () {
    final world = newWorld();
    world.add(GameObject(name: 'camera', components: [Camera()]));
    final ended = <AudioClip>[];
    final short = AudioClip.fromSamples(Float32List(24000)..fillRange(0, 24000, 0.1), name: 'short');
    final source = AudioSource(clip: short, spatialBlend: 1)..onEnd = ended.add;
    // Each place a float, but the source's in the world 6e38 m away, past
    // what the engine's floats hold.
    final parent = world.add(GameObject(name: 'parent'));
    parent.add(
      GameObject(
        transform: Transform(position: Vec3(3e38, 0, 0)),
        components: [source],
      ),
    );
    world.update(1 / 60);
    parent.transform = Transform(position: Vec3(3e38, 0, 0));
    for (var i = 0; i < 6; i++) {
      world.update(1 / 60);
    }
    expect(world.audio.isTurnedOff, isFalse);
    // Brought back, it's heard where it is, and ends.
    parent.transform = Transform(position: Vec3(-3e38, 0, 0));
    world.update(1 / 60);
    expect(source.isPlaying, isTrue);
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60);
    }
    expect(world.audio.isTurnedOff, isFalse);
    expect(ended, [short]);
  });

  test('a min distance past the max names what was set, and says which to set first (stage 14, A4)', () {
    final world = newWorld();
    final source = sound(world, Vec3.zero);
    String error(void Function() f) {
      try {
        f();
      } on ArgumentError catch (e) {
        return e.toString();
      }
      return 'no error';
    }

    expect(
      error(() => source.minDistance = 600),
      allOf(
        contains('minDistance'),
        contains('600'),
        contains('maxDistance (50.0)'),
        contains('raise maxDistance first'),
      ),
    );
    expect(error(() => source.maxDistance = 0.5), allOf(contains('maxDistance'), contains('lower minDistance first')));
    expect(
      error(() => AudioSource(minDistance: 600)),
      allOf(contains('minDistance: 600.0'), contains('maxDistance: 500.0')),
    );
    source
      ..maxDistance = 1000
      ..minDistance = 600;
    expect((source.minDistance, source.maxDistance), (600, 1000));
  });
}
