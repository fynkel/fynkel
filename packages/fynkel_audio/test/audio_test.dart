// Sound through the package (docs/stage-11.md phase 1): clips loaded and
// made, sources that play, end, stop, pause and follow the game's pause,
// one-shots, and the values refused. No world here is on screen, so no
// device opens: the sound is made and timed without one.

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';
import 'package:test/test.dart';

/// A 16-bit WAV file of [samples] (mono).
Uint8List wav(List<double> samples, {int rate = 48000}) {
  final data = ByteData(44 + samples.length * 2);
  void text(int at, String s) {
    for (var i = 0; i < s.length; i++) {
      data.setUint8(at + i, s.codeUnitAt(i));
    }
  }

  text(0, 'RIFF');
  data.setUint32(4, 36 + samples.length * 2, Endian.little);
  text(8, 'WAVEfmt ');
  data
    ..setUint32(16, 16, Endian.little)
    ..setUint16(20, 1, Endian.little)
    ..setUint16(22, 1, Endian.little)
    ..setUint32(24, rate, Endian.little)
    ..setUint32(28, rate * 2, Endian.little)
    ..setUint16(32, 2, Endian.little)
    ..setUint16(34, 16, Endian.little);
  text(36, 'data');
  data.setUint32(40, samples.length * 2, Endian.little);
  for (var i = 0; i < samples.length; i++) {
    data.setInt16(44 + i * 2, (samples[i] * 32767).round(), Endian.little);
  }
  return data.buffer.asUint8List();
}

/// [seconds] of a quiet level, made in code.
AudioClip tone(double seconds, {String name = 'tone'}) => AudioClip.fromSamples(
  Float32List((48000 * seconds).round())..fillRange(0, (48000 * seconds).round(), 0.1),
  name: name,
);

World newWorld({Map<String, Uint8List> files = const {}}) {
  final world = World(source: (name) async => files[name] ?? (throw StateError('no file $name')));
  addTearDown(world.dispose);
  return world;
}

/// Runs [frames] frames of [dt] seconds.
void run(World world, {int frames = 1, double dt = 1 / 60}) {
  for (var i = 0; i < frames; i++) {
    world.update(dt);
  }
}

void main() {
  test('a clip made in code knows its length, rate and memory', () {
    final clip = AudioClip.fromSamples(Float32List(24000), rate: 24000, name: 'silence');
    expect((clip.rate, clip.channels, clip.frames, clip.length, clip.bytes), (24000, 1, 24000, 1.0, 96000));
    expect(clip.toString(), 'AudioClip(silence, 1.00 s, 24000 Hz × 1)');
    clip.dispose();
    expect(clip.isDisposed, isTrue);
    expect(() => AudioClip.fromSamples(Float32List(3), channels: 2), throwsArgumentError);
    expect(() => AudioClip.fromSamples(Float32List(4), channels: 3), throwsArgumentError);
  });

  test('assets load a sound by name, share it, and refuse a file that isn\'t one', () async {
    final world = newWorld(
      files: {
        'beep.wav': wav(List.filled(4800, 0.5)),
        'notes.ogg': Uint8List.fromList([1, 2, 3]),
      },
    );
    final holder = world.add(GameObject());
    final beep = await holder.assets.audio('beep.wav');
    expect((beep.name, beep.length), ('beep.wav', 0.1));
    expect(await world.assets.audio('beep.wav'), same(beep));
    await expectLater(
      world.assets.audio('notes.ogg'),
      throwsA(isA<FormatException>().having((e) => e.message, 'message', startsWith('notes.ogg: '))),
    );
    // Preloading knows the sound files once the world has sound.
    expect(() => world.assets.preload(['quiet.wav']), throwsArgumentError);
    world.audio;
    await world.assets.preload(['beep.wav']);
  });

  test('a source plays its clip when it comes into the world, then ends and says so once', () {
    final world = newWorld();
    final clip = tone(0.1);
    final ended = <AudioClip>[];
    final source = AudioSource(clip: clip)..onEnd = ended.add;
    world.add(GameObject(components: [source]));
    expect(source.isPlaying, isTrue);
    run(world, frames: 3);
    expect(source.isPlaying, isTrue);
    expect(source.playTime, closeTo(3 / 60, 0.01));
    run(world, frames: 4);
    expect(source.isPlaying, isFalse);
    expect(ended, [clip]);
    run(world, frames: 10);
    expect(ended, [clip]);
    expect(world.audio.stats.voices, 0);
    expect(world.audio.stats.device, isFalse);
  });

  test('a source that doesn\'t play on awake waits for play, and play starts again', () {
    final world = newWorld();
    final source = AudioSource(clip: tone(1), playOnAwake: false);
    world.add(GameObject(components: [source]));
    run(world);
    expect(source.isPlaying, isFalse);
    source.play();
    run(world, frames: 30);
    expect(source.playTime, closeTo(0.5, 0.02));
    source.play();
    run(world);
    expect(source.playTime, closeTo(1 / 60, 0.01));
    expect(() => AudioSource().play(), throwsStateError);
  });

  test('stop ends the sound at once, without onEnd', () {
    final world = newWorld();
    var ends = 0;
    final source = AudioSource(clip: tone(0.2))..onEnd = (_) => ends++;
    world.add(GameObject(components: [source]));
    run(world);
    source.stop();
    expect(source.isPlaying, isFalse);
    run(world, frames: 30);
    expect(ends, 0);
    expect(world.audio.stats.voices, 0);
  });

  test('a paused clip holds its place, isn\'t playing, and plays on from it', () {
    final world = newWorld();
    final source = AudioSource(clip: tone(1));
    world.add(GameObject(components: [source]));
    run(world, frames: 15);
    source.pause();
    run(world);
    final held = source.playTime;
    expect(source.isPlaying, isFalse);
    run(world, frames: 60);
    expect(source.playTime, held);
    source.unPause();
    run(world, frames: 15);
    expect(source.isPlaying, isTrue);
    expect(source.playTime, closeTo(held + 0.25, 0.02));
  });

  test('playTime set while stopped starts the next play there, and while playing moves it', () {
    final world = newWorld();
    final source = AudioSource(clip: tone(1), playOnAwake: false);
    world.add(GameObject(components: [source]));
    source.playTime = 0.5;
    expect(source.playTime, 0.5);
    source.play();
    run(world);
    expect(source.playTime, closeTo(0.5 + 1 / 60, 0.01));
    source.playTime = 0.1;
    run(world);
    expect(source.playTime, closeTo(0.1 + 1 / 60, 0.01));
  });

  test('the game paused holds its sounds, but not those that ignore the pause', () {
    final world = newWorld();
    final game = AudioSource(clip: tone(1));
    final menu = AudioSource(clip: tone(1), ignorePause: true);
    world.add(GameObject(components: [game, menu]));
    run(world);
    world.paused = true;
    run(world, frames: 30);
    expect(game.playTime, lessThan(0.05));
    expect(menu.playTime, greaterThan(0.45));
    world.paused = false;
    run(world, frames: 30);
    expect(game.playTime, closeTo(0.5, 0.05));
  });

  test('a source that follows the time scale goes slower with it, and holds at 0', () {
    final world = newWorld();
    final slow = AudioSource(clip: tone(2), followTimeScale: true);
    final normal = AudioSource(clip: tone(2));
    world.add(GameObject(components: [slow, normal]));
    run(world);
    world.time.scale = 0.5;
    run(world, frames: 60);
    expect(slow.playTime, closeTo(0.5, 0.05));
    expect(normal.playTime, closeTo(1, 0.05));
    world.time.scale = 0;
    final held = slow.playTime;
    run(world, frames: 30);
    expect(slow.playTime, closeTo(held, 0.01));
    world.time.scale = 1;
    run(world, frames: 30);
    expect(slow.playTime, closeTo(held + 0.5, 0.05));
  });

  test('disabling or destroying a source stops its sounds; enabling plays on awake again', () {
    final world = newWorld();
    final source = AudioSource(clip: tone(1));
    final object = world.add(GameObject(components: [source]));
    source.playOneShot(tone(1));
    run(world);
    expect(world.audio.stats.voices, 2);
    source.enabled = false;
    run(world, frames: 2);
    expect(source.isPlaying, isFalse);
    expect(world.audio.stats.voices, 0);
    source.enabled = true;
    run(world);
    expect(source.isPlaying, isTrue);
    object.destroy();
    run(world, frames: 2);
    expect(world.audio.stats.voices, 0);
  });

  test('one-shots play over the clip at their own volume and pitch, and stop with it', () {
    final world = newWorld();
    final source = AudioSource(clip: tone(1), volume: 0.5);
    world.add(GameObject(components: [source]));
    source
      ..playOneShot(tone(0.1), volume: 0.5)
      ..playOneShot(tone(0.1), pitch: 2);
    run(world);
    expect(world.audio.stats.voices, 3);
    run(world, frames: 4);
    // The one at pitch 2 lasted half as long.
    expect(world.audio.stats.voices, 2);
    run(world, frames: 3);
    expect(world.audio.stats.voices, 1);
    expect(source.isPlaying, isTrue);
    world.audio.playOneShot(tone(0.5), ignorePause: true);
    run(world);
    expect(world.audio.stats.voices, 2);
    expect(world.audio.stats.started, 4);
  });

  test('values out of range throw, and nothing changes', () {
    final world = newWorld();
    final source = AudioSource(clip: tone(1));
    world.add(GameObject(components: [source]));
    expect(() => source.volume = 1.5, throwsArgumentError);
    expect(() => source.pitch = 0, throwsArgumentError);
    expect(() => source.priority = 300, throwsArgumentError);
    expect(() => source.playTime = -1, throwsArgumentError);
    expect(() => AudioSource(volume: -1), throwsArgumentError);
    expect(() => world.audio.volume = 2, throwsArgumentError);
    expect(() => world.audio.playOneShot(tone(1), pitch: 10), throwsArgumentError);
    expect((source.volume, source.pitch, source.priority, world.audio.volume), (1.0, 1.0, 128, 1.0));
  });

  test('every session can be set, voice chat too, while sound plays', () {
    final world = newWorld();
    world.add(GameObject(components: [AudioSource(clip: tone(1))]));
    run(world);
    for (final session in AudioSession.values) {
      world.audio.session = session;
      run(world);
      expect(world.audio.session, session);
    }
  });

  test('a world disposed while sounds play lets them go, and its audio then refuses', () {
    final world = World();
    final source = AudioSource(clip: tone(1));
    world.add(GameObject(components: [source]));
    run(world);
    final audio = world.audio;
    world.dispose();
    expect(() => audio.volume = 0.5, throwsStateError);
  });
}
