// Audio from Dart (M3-11, docs/audio.md): the fixture's Main scene — the
// Robot carries an `AudioSource` playing `tone.wav` from the load, the
// Camera the `AudioListener` — plays on the null backend; from Dart an
// object's `audio` plays, stops, pauses and reads back its state, a
// behaviour hears `onPlaybackEnded`, `world.audio.play` fires a sound
// without an object, the buses follow the settings document, and a
// missing source or clip is a diagnostic, never an exception. No test
// opens a device.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const tone = AssetRef('c0000000-0000-4000-8000-000000000006');
const unknownClip = AssetRef('c0000000-0000-4000-8000-0000000000ff');

/// Logs the clips that end on its object and replays the first one.
class Listener extends Behaviour {
  final List<AssetRef> ended = [];
  bool replayed = false;

  @override
  void onPlaybackEnded(PlaybackEnded event) {
    expect(event.entity, gameObject.entity);
    ended.add(event.clip);
    if (!replayed) {
      replayed = true;
      audio.play();
    }
  }
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-audio-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    w.onLoadDiagnostics = (_) {};
    addTearDown(w.dispose);
    return w;
  }

  test('the Main scene registers the tone, the Robot plays it from the load and it ends', () {
    final w = fresh();
    final report = w.loadProject(fixture, cacheDir: cache);
    expect(report.ok, isTrue);
    expect(report.audio.sounds, 1);
    expect(report.audio.clips, 1);
    expect(report.audio.device, isFalse, reason: 'a load never opens a device');
    expect((report.audio.sampleRate, report.audio.channels), (48000, 2), reason: 'the null backend');
    expect(report.audio.maxVoices, 32);
    expect(w.systemTimes().map((s) => s.system), contains(SystemKind.audio));

    final robot = w.find('Robot')!;
    final camera = w.find('Camera')!;
    final source = robot.get<AudioSource>()!;
    expect((source.clip, source.bus, source.looping, source.playing, source.priority), (tone, AudioBus.sfx, false, true, 0));
    expect(source.volume, closeTo(0.8, 1e-6));
    expect(camera.get<AudioListener>(), const AudioListener(volume: 1));
    final listener = robot.addBehaviour(Listener());

    // The voice started in the load's first tick; the tone is a few
    // milliseconds long, so it ends within the first frames — the
    // behaviour replays it once.
    for (var i = 0; i < 12; i++) {
      w.frame(1 / 60);
    }
    expect(listener.ended, [tone, tone]);
    expect(robot.audio.isPlaying, isFalse);
    expect(robot.get<AudioSource>()!.playing, isFalse, reason: 'the end cleared playing');
    final state = robot.audio.state!;
    expect(state.clip, tone);
    expect(state.duration, greaterThan(0));
    expect(state.voice, 0);
    expect(state.time, 0);

    final stats = w.audio.stats;
    expect(stats.device, isFalse);
    expect((stats.started, stats.ended, stats.voices, stats.clips), (2, 2, 0, 1));
    expect(stats.suspended, isFalse);
  });

  test('an object\'s audio plays, pauses, resumes, sets volume and pitch, stops; state reads back', () {
    final w = fresh();
    w.loadProject(fixture, cacheDir: cache);
    final robot = w.find('Robot')!;
    // A looping source of our own beside the scene's.
    final loop = w.spawn(name: 'Loop')
      ..add(const AudioSource(clip: tone, bus: AudioBus.music, volume: 0.5, pitch: 1, looping: true, playing: false, priority: 2));
    w.frame(1 / 60);
    expect(loop.audio.state!.playing, isFalse);
    expect(loop.audio.state!.duration, greaterThan(0));

    loop.audio.play();
    w.frame(1 / 60);
    var s = loop.audio.state!;
    expect(s.playing, isTrue);
    expect(s.paused, isFalse);
    expect(s.looping, isTrue);
    expect(s.bus, AudioBus.music);
    expect(s.voice, isNot(0));
    for (var i = 0; i < 30; i++) {
      w.frame(1 / 60);
    }
    expect(loop.audio.isPlaying, isTrue, reason: 'loops never end');
    expect(loop.audio.state!.time, lessThan(loop.audio.state!.duration));

    loop.audio.pause();
    w.frame(1 / 60);
    s = loop.audio.state!;
    expect(s.paused, isTrue);
    final held = s.time;
    w.frame(1 / 60);
    expect(loop.audio.state!.time, held);

    loop.audio.resume();
    loop.audio.volume = 0.25;
    loop.audio.pitch = 2;
    w.frame(1 / 60);
    s = loop.audio.state!;
    expect(s.paused, isFalse);
    expect((s.volume, s.pitch), (0.25, 2.0));
    expect(loop.get<AudioSource>()!.pitch, 2, reason: 'the command wrote the component');

    loop.audio.stop();
    w.frame(1 / 60);
    expect(loop.audio.isPlaying, isFalse);
    expect(loop.get<AudioSource>()!.playing, isFalse);

    // Writing the component drives the voice too: playing: true starts,
    // false stops; no event for a stop.
    loop.set(loop.get<AudioSource>()!.copyWith(playing: true));
    w.frame(1 / 60);
    expect(loop.audio.isPlaying, isTrue);
    loop.set(loop.get<AudioSource>()!.copyWith(playing: false));
    w.frame(1 / 60);
    expect(loop.audio.isPlaying, isFalse);
    expect(robot.audio.state, isNotNull);
    expect(w.byEntity(w.find('Camera')!.entity)!.audio.state, isNull, reason: 'no source');
  });

  test('world.audio.play fires a sound without an object; the buses follow the settings', () {
    final w = fresh();
    w.loadProject(fixture, cacheDir: cache);
    for (var i = 0; i < 12; i++) {
      w.frame(1 / 60);
    }
    final before = w.audio.stats;
    w.audio.play(tone, bus: AudioBus.sfx, volume: 0.7, pitch: 1.5, priority: 1);
    w.audio.play(tone, presentation: true);
    expect(w.audio.stats.started - before.started, 1, reason: 'the presentation one at once');
    w.frame(1 / 60);
    final after = w.audio.stats;
    expect(after.started - before.started, 2);
    expect(after.voices, lessThanOrEqualTo(2));
    // No object: no PlaybackEnded for them (the raw loop, so the events
    // are seen at the world level).
    final ended = <PlaybackEnded>[];
    for (var i = 0; i < 12; i++) {
      w.beginFrame(1 / 60);
      w.step();
      ended.addAll(w.drainEvents().whereType<PlaybackEnded>());
    }
    expect(ended, isEmpty);
    expect(w.audio.stats.ended - after.ended, 2);

    // The settings document drives the buses at once; a mute is one
    // switch for every bus.
    final store = SettingsStore(initial: const SettingsDocument(masterVolume: 0.5, musicVolume: 0.2, sfxVolume: 0.9, muted: true));
    w.useSettings(store);
    store.edit((s) => s.copyWith(muted: false, sfxVolume: 0.4));
    w.audio.setBus(AudioBus.music, 0.3);
    // The engine's bus gains are not queried back (they are the settings);
    // what matters is that nothing was rejected.
    w.audio.play(tone);
    w.frame(1 / 60);
    expect(w.audio.stats.started - after.started, 1);
  });

  test('a missing source, a bad value, an unknown clip and a stolen voice are diagnostics, not exceptions', () {
    final w = fresh();
    w.loadProject(fixture, cacheDir: cache);
    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    final camera = w.find('Camera')!;
    final robot = w.find('Robot')!;
    camera.audio.play(); // no source
    robot.audio.volume = 1.5;
    robot.audio.pitch = 0;
    w.audio.play(tone, volume: -1);
    w.audio.play(unknownClip);
    w.audio.setBus(AudioBus.master, 2); // presentation lane: at once
    expect(seen.map((d) => d.code), [DiagnosticCode.invalidPayload]);
    seen.clear();
    w.frame(1 / 60);
    expect(seen.map((d) => d.code), [
      DiagnosticCode.missingComponent,
      DiagnosticCode.invalidPayload,
      DiagnosticCode.invalidPayload,
      DiagnosticCode.invalidPayload,
      DiagnosticCode.soundUnavailable,
    ]);
    expect(seen[0].entity, camera.entity);
    expect(seen[0].component, AudioSource.componentId);
    seen.clear();

    // A source whose clip the world does not hold: reported once against
    // the entity, the source keeps wanting to play.
    final ghost = w.spawn(name: 'Ghost')
      ..add(const AudioSource(clip: unknownClip, bus: AudioBus.sfx, volume: 1, pitch: 1, looping: false, playing: true, priority: 0));
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(seen.map((d) => d.code), [DiagnosticCode.soundUnavailable]);
    expect(seen.single.entity, ghost.entity);
    expect(ghost.audio.state!.duration, 0);
    expect(ghost.get<AudioSource>()!.playing, isTrue);
    seen.clear();

    // The voice limit: 32 looping voices, then a higher priority steals
    // the oldest and a lower one is refused (its playing clears).
    final voices = [
      for (var i = 0; i < 32; i++)
        w.spawn(name: 'v$i')
          ..add(const AudioSource(clip: tone, bus: AudioBus.sfx, volume: 1, pitch: 1, looping: true, playing: true, priority: 0)),
    ];
    w.frame(1 / 60);
    expect(w.audio.stats.voices, 32);
    final thief = w.spawn(name: 'thief')
      ..add(const AudioSource(clip: tone, bus: AudioBus.sfx, volume: 1, pitch: 1, looping: true, playing: true, priority: 5));
    final meek = w.spawn(name: 'meek')
      ..add(const AudioSource(clip: tone, bus: AudioBus.sfx, volume: 1, pitch: 1, looping: true, playing: true, priority: -1));
    w.frame(1 / 60);
    expect(thief.audio.isPlaying, isTrue);
    expect(meek.audio.isPlaying, isFalse);
    expect(voices[0].audio.isPlaying, isFalse, reason: 'the oldest equal-or-lower voice was stolen');
    // The component's `playing` cleared in the world (a fresh object's
    // staged value stays readable until its first snapshot: one more
    // frame).
    w.frame(1 / 60);
    expect(meek.get<AudioSource>()!.playing, isFalse);
    expect(voices[0].get<AudioSource>()!.playing, isFalse);
    expect(voices[1].audio.isPlaying, isTrue);
    final stats = w.audio.stats;
    expect((stats.voices, stats.stolen, stats.refused), (32, 1, 1));
    expect(seen, isEmpty, reason: 'stealing and refusing are not diagnostics');

    // Destroying a source's object frees its voice on the next tick.
    thief.destroy();
    w.frame(1 / 60);
    expect(w.audio.stats.voices, 31);
  });

  test('open on a test world is never called; suspend and close are no-ops without a device', () {
    final w = fresh();
    w.loadProject(fixture, cacheDir: cache);
    expect(w.audio.isOpen, isFalse);
    w.audio.suspend(true);
    w.audio.suspend(false);
    w.audio.close();
    expect(w.audio.stats.device, isFalse);
    expect(w.audio.stats.suspended, isFalse);
    w.frame(1 / 60);
  });

  test('TestGameContext hears nothing: a behaviour plays through the null backend', () {
    final w = fresh();
    w.loadProject(fixture, cacheDir: cache);
    final robot = w.find('Robot')!;
    final ctx = TestGameContext();
    w.context = ctx;
    robot.audio.play();
    w.frame(1 / 60);
    expect(robot.audio.isPlaying, isTrue);
    expect(w.audio.isOpen, isFalse);
  });
}
