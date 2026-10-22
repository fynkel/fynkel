// Music and the mix (docs/stage-11.md phase 3): groups under the master
// volume, the music player and its crossfades, streamed clips, and a
// clip's file read again on a hot reload. Each listens to the mix with
// audio.render.

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';
import 'package:test/test.dart';

import 'audio_test.dart' show wav;

/// [seconds] of a constant [value].
AudioClip level(double value, {double seconds = 1, String name = 'level'}) {
  final frames = (48000 * seconds).round();
  return AudioClip.fromSamples(Float32List(frames)..fillRange(0, frames, value), name: name);
}

World newWorld({Map<String, Uint8List>? files}) {
  final world = World(source: (name) async => files?[name] ?? (throw StateError('no file $name')));
  addTearDown(world.dispose);
  return world;
}

/// The left of the last frame of [frames] rendered.
double heard(World world, {int frames = 2048}) {
  final out = world.audio.render(frames);
  return out[out.length - 2];
}

AudioSource looping(World world, AudioClip clip, {AudioGroup? group}) {
  final source = AudioSource(clip: clip, loop: true)..group = group;
  world.add(GameObject()).addComponent(source);
  world.update(0);
  return source;
}

void main() {
  test("a group's volume reaches its sounds at once, under the master volume", () {
    final world = newWorld();
    final effects = world.audio.group('effects');
    expect(world.audio.group('effects'), same(effects));
    looping(world, level(0.5), group: effects);
    expect(heard(world), closeTo(0.5, 0.005));
    effects.volume = 0.5;
    expect(heard(world, frames: 512), closeTo(0.25, 0.005));
    world.audio.volume = 0.5;
    expect(heard(world), closeTo(0.125, 0.005));
    effects.mute = true;
    expect(heard(world), closeTo(0, 1e-4));
    expect(effects.volume, 0.5, reason: 'muting keeps the volume');
    effects.mute = false;
    expect(heard(world), closeTo(0.125, 0.005));
    // A sound in no group, or another, doesn't follow it.
    final music = world.audio.group('music');
    music.volume = 0.2;
    expect(heard(world), closeTo(0.125, 0.005));
    expect(effects.toString(), "AudioGroup('effects', 0.50)");
  });

  test('a source moved to a group follows it from then, one-shots too', () {
    final world = newWorld();
    final quiet = world.audio.group('quiet')..volume = 0.25;
    final source = looping(world, level(0.4));
    expect(heard(world), closeTo(0.4, 0.005));
    source.group = quiet;
    expect(heard(world), closeTo(0.1, 0.005));
    source.stop();
    world.audio.playOneShot(level(0.4), group: quiet);
    expect(heard(world), closeTo(0.1, 0.005));
  });

  test('a group fades over its seconds', () {
    final world = newWorld();
    final music = world.audio.group('music');
    looping(world, level(0.8, seconds: 3), group: music);
    heard(world);
    music.fade(0, 1);
    expect(music.volume, 0);
    expect(heard(world, frames: 24000), closeTo(0.4, 0.01));
    expect(heard(world, frames: 24000), closeTo(0, 0.01));
    expect(() => music.fade(2, 1), throwsArgumentError);
    expect(() => music.fade(0.5, -1), throwsArgumentError);
  });

  test('there is room for 15 groups', () {
    final world = newWorld();
    for (var i = 0; i < Audio.maxGroups; i++) {
      world.audio.group('g$i');
    }
    expect(() => world.audio.group('one more'), throwsStateError);
    expect(world.audio.group('g3').name, 'g3');
  });

  test('music loops in its group, plays on through a pause, and playing it again goes on', () {
    final world = newWorld();
    final music = world.audio.music;
    final song = level(0.5, seconds: 0.5);
    music.play(song);
    for (var i = 0; i < 7; i++) {
      world.update(0.1);
    }
    expect(music.isPlaying, isTrue, reason: 'it loops');
    expect(music.clip, same(song));
    expect(music.time, closeTo(0.2, 0.02));
    world.audio.group('music').volume = 0.5;
    expect(heard(world), closeTo(0.25, 0.005));
    world.paused = true;
    world.update(0.1);
    expect(heard(world), closeTo(0.25, 0.005), reason: 'it ignores the pause');
    world.paused = false;
    final time = music.time;
    music.play(song);
    expect(music.time, time, reason: 'not started again');
  });

  test('music crossfades from one track to the next', () {
    final world = newWorld();
    final music = world.audio.music;
    final (calm, chase) = (level(0.4, seconds: 4, name: 'calm'), level(0.2, seconds: 4, name: 'chase'));
    music.play(calm);
    expect(heard(world), closeTo(0.4, 0.005));
    music.crossFade(chase, 1);
    expect(music.clip, same(chase));
    // Half way, half of each.
    expect(heard(world, frames: 24000), closeTo(0.3, 0.005));
    expect(heard(world, frames: 24000), closeTo(0.2, 0.005));
    world.update(0);
    expect(world.audio.stats.voices, 1, reason: 'the first stopped once faded out');
  });

  test('music fades in, and stops with or without fading out', () {
    final world = newWorld();
    final music = world.audio.music;
    music.play(level(0.4, seconds: 4), fadeIn: 0.5);
    expect(heard(world, frames: 12000), closeTo(0.2, 0.005));
    expect(heard(world, frames: 12000), closeTo(0.4, 0.005));
    music.stop(fadeOut: 0.5);
    expect((music.isPlaying, music.clip), (false, null));
    expect(heard(world, frames: 12000), closeTo(0.2, 0.005));
    expect(heard(world, frames: 13000), closeTo(0, 1e-4));
    music.play(level(0.4));
    music.stop();
    expect(heard(world), closeTo(0, 1e-4));
    expect(() => music.stop(fadeOut: -1), throwsArgumentError);
  });

  test("music that doesn't loop ends and says so", () {
    final world = newWorld();
    final music = world.audio.music..loop = false;
    final ended = <AudioClip>[];
    music.onEnd = ended.add;
    final jingle = level(0.3, seconds: 0.2);
    music.play(jingle);
    world.update(0.3);
    expect(ended, [jingle]);
    expect((music.isPlaying, music.clip), (false, null));
  });

  test('a long clip streams, and plays as a decoded one would', () async {
    final samples = [for (var i = 0; i < 48000 * 11; i++) (i % 480) / 960];
    final long = await AudioClip.decode(wav(samples), name: 'long.wav');
    expect(long.streamed, isTrue);
    expect(long.bytes, 44 + samples.length * 2, reason: 'its file, not its samples');
    expect(long.toString(), contains('streamed'));
    final whole = await AudioClip.decode(wav(samples), name: 'long.wav', stream: false);
    expect(whole.streamed, isFalse);
    expect((whole.frames, whole.length), (long.frames, long.length));
    final short = await AudioClip.decode(wav(samples.sublist(0, 4800)), stream: true);
    expect(short.streamed, isTrue);

    Float32List played(AudioClip clip) {
      final world = newWorld();
      world.audio.music.play(clip);
      world.update(0);
      final out = world.audio.render(48000 * 2);
      expect(world.audio.stats.starved, 0);
      return out;
    }

    expect(played(long), played(whole));
  });

  test("a hot reload gives a clip its file's new sound, from the next time it's played", () async {
    final files = {'beep.wav': wav(List.filled(48000, 0.5)), 'same.wav': wav(List.filled(4800, 0.25))};
    final world = newWorld(files: files);
    final beep = await world.assets.audio('beep.wav');
    final unchanged = await world.assets.audio('same.wav');
    final source = looping(world, beep);
    expect(heard(world), closeTo(0.5, 0.001));
    files['beep.wav'] = wav(List.filled(24000, 0.125));
    await world.reloadAssets();
    expect(beep.length, 0.5);
    expect(heard(world), closeTo(0.5, 0.001), reason: 'what plays plays on');
    source.play();
    expect(heard(world), closeTo(0.125, 0.001));
    expect(unchanged.length, 0.1);

    // A file that isn't sound now keeps the old, and says why.
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    files['beep.wav'] = Uint8List.fromList([1, 2, 3]);
    await world.reloadAssets();
    await pumpEventQueue();
    expect(errors.single.error, isA<FormatException>());
    expect(beep.length, 0.5);
  });
}
