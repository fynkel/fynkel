import 'dart:convert';
import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

class ApplyAudioSettingsOnCreate extends Behaviour {
  ApplyAudioSettingsOnCreate(this.settings);
  final SettingsDocument settings;
  int calls = 0;

  @override
  void onCreate() {
    calls++;
    world.audio.applySettings(settings);
  }
}

void main() {
  const initial = SettingsDocument(masterVolume: 0.8, musicVolume: 0.7, sfxVolume: 0.6);
  const changed = SettingsDocument(masterVolume: 0.2, musicVolume: 0.3, sfxVolume: 0.4, muted: true);
  late NativeLibrary lib;
  late Directory temp;
  late String cache;
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  setUpAll(() {
    temp = Directory.systemTemp.createTempSync('fynkel-audio-settings-');
    cache = '${temp.path}/cache';
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    lib.dispose();
    temp.deleteSync(recursive: true);
  });

  World fresh() {
    final world = World.create(library: lib);
    world.onLoadDiagnostics = (_) {};
    addTearDown(world.dispose);
    return world;
  }

  void expectAudio(World world, SettingsDocument expected) {
    expect(world.audio.busVolume(AudioBus.master), closeTo(expected.masterVolume, 1e-6));
    expect(world.audio.busVolume(AudioBus.music), closeTo(expected.musicVolume, 1e-6));
    expect(world.audio.busVolume(AudioBus.sfx), closeTo(expected.sfxVolume, 1e-6));
    expect(world.audio.muted, expected.muted);
  }

  test('all settings audio gains publish together from callbacks and explicit abort preserves them', () {
    final world = fresh();
    world.audio.applySettings(initial);
    world.frame(
      0,
      update: () {
        world.audio.applySettings(changed);
        expectAudio(world, changed);
      },
    );
    expectAudio(world, changed);
    expect(
      () => world.edit(() {
        world.audio.applySettings(initial);
        expectAudio(world, initial);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expectAudio(world, changed);
    expect(world.audio.stats.started, 0);
  });

  test('a SettingsStore listener failure rolls back engine gains but not its external document or file', () {
    final world = fresh();
    final path = '${temp.path}/settings.json';
    final store = SettingsStore(path: path, initial: initial);
    world.useSettings(store);
    void fail(SettingsDocument settings) => throw StateError('listener failed');
    store.addListener(fail);
    expect(() => world.frame(0, update: () => store.update(changed)), throwsStateError);
    expect(store.current, changed, reason: 'SettingsStore is outside the native transaction');
    expect(
      SettingsDocument.fromJson(jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>),
      changed,
    );
    expectAudio(world, initial);
    store.removeListener(fail);
    world.frame(0, update: store.apply);
    // Explicit replay reconciles the external settings document.
    expectAudio(world, changed);
  });

  test('settings audio is a module-off no-op even inside a successful callback', () {
    final world = fresh();
    world.audio.applySettings(initial);
    world.modules = const Modules(audio: false);
    final store = SettingsStore(initial: changed);
    world.useSettings(store);
    world.frame(
      0,
      update: () {
        world.audio.applySettings(changed);
        store.update(changed.copyWith(masterVolume: 0.1));
      },
    );
    expect(store.current.masterVolume, 0.1);
    world.modules = Modules.all;
    expectAudio(world, initial);
  });

  test('lifecycle settings replay commits at the callback boundary without an audio tick', () {
    final world = fresh();
    final behaviour = ApplyAudioSettingsOnCreate(changed);
    world.edit(() => world.spawn(name: 'settings').addBehaviour(behaviour));
    world.frame(0);
    expect(behaviour.calls, 1);
    expectAudio(world, changed);
    world.frame(0);
    expect(behaviour.calls, 1);
    expectAudio(world, changed);
  });

  test('project settings commit mixer gains and input bindings together from a callback', () {
    final world = fresh()..loadProject(fixture, cacheDir: cache);
    final store = SettingsStore(initial: initial);
    world.useSettings(store);
    final rebound = changed.rebind('jump', const ['keyboard:J']);
    world.frame(0, update: () => store.update(rebound));
    expect(store.current, rebound);
    world.frame(0, input: const [InputEvent.reset(), InputEvent.down(InputDevice.keyboard, 'J')]);
    expect(world.input.state('jump')!.held, isTrue);
    expectAudio(world, changed);
    expect(
      () => world.frame(
        0,
        update: () {
          store.update(initial);
          throw StateError('abort settings');
        },
      ),
      throwsStateError,
    );
    expect(store.current, initial, reason: 'the external settings document is outside transaction rollback');
    expectAudio(world, changed);
    world.frame(0, input: const [InputEvent.reset(), InputEvent.down(InputDevice.keyboard, 'J')]);
    expect(world.input.state('jump')!.held, isTrue, reason: 'aborted input changes roll back with audio');
    world.frame(0, update: store.apply);
    expectAudio(world, initial);
    world.frame(0, input: const [InputEvent.reset(), InputEvent.down(InputDevice.keyboard, 'J')]);
    expect(world.input.state('jump')!.held, isFalse);
  });
}
