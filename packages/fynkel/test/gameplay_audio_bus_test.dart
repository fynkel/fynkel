import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  late World world;
  setUp(() => world = World.create());
  tearDown(() => world.dispose());

  test('bus gains and global mute commit immediately without a simulation tick', () {
    expect(world.audio.busVolume(AudioBus.master), 1);
    expect(world.audio.muted, isFalse);
    world.audio.setBus(AudioBus.master, 0.5, muted: true);
    expect(world.audio.busVolume(AudioBus.master), 0.5);
    expect(world.audio.muted, isTrue);
    world.edit(() {
      expect(world.audio.busVolume(AudioBus.master), 0.5);
      world.audio.setBus(AudioBus.music, 0.25, muted: true);
      expect(world.audio.busVolume(AudioBus.music), 0.25);
      world.audio.setBus(AudioBus.sfx, 0.75);
      expect(world.audio.busVolume(AudioBus.master), 0.5);
      expect(world.audio.muted, isFalse, reason: 'mute is one global last-write-wins value');
    });
    expect(world.audio.busVolume(AudioBus.music), 0.25);
    expect(world.audio.busVolume(AudioBus.sfx), 0.75);
    expect(world.audio.muted, isFalse);
    expect(world.tick, 0);
    world.frame(1 / 60);
    expect(
      world.audio.busVolume(AudioBus.music),
      0.25,
      reason: 'a later tick cannot replay stale bus configuration',
    );
  });

  test('aborted explicit and automatic callbacks leave all buses and mute unchanged', () {
    world.audio.setBus(AudioBus.master, 0.5);
    void fail() {
      world.audio.setBus(AudioBus.master, 0.25, muted: true);
      world.audio.setBus(AudioBus.music, 0.25, muted: true);
      expect(world.audio.muted, isTrue);
      throw StateError('abort');
    }

    expect(() => world.edit(fail), throwsStateError);
    expect(world.audio.busVolume(AudioBus.master), 0.5);
    expect(world.audio.busVolume(AudioBus.music), 1);
    expect(world.audio.muted, isFalse);
    expect(() => world.frame(0, update: fail), throwsStateError);
    expect(world.audio.busVolume(AudioBus.master), 0.5);
    expect(world.audio.busVolume(AudioBus.music), 1);
    expect(world.audio.muted, isFalse);
  });

  test('a bad settings gain rejects all settings writes and prior gameplay changes', () {
    final object = world.edit(() => world.spawn(name: 'original'));
    world.audio.applySettings(const SettingsDocument(masterVolume: 0.5, musicVolume: 0.5, sfxVolume: 0.5));
    for (final invalid in [-1.0, 2.0, double.nan, double.infinity]) {
      final settings = SettingsDocument(
        masterVolume: 0.25,
        musicVolume: 0.75,
        sfxVolume: invalid,
        muted: true,
      );
      expect(
        () => world.edit(() {
          object.name = 'discarded';
          try {
            world.audio.applySettings(settings);
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(object.name, 'original');
      for (final bus in [AudioBus.master, AudioBus.music, AudioBus.sfx]) {
        expect(world.audio.busVolume(bus), 0.5);
      }
      expect(world.audio.muted, isFalse);
      expect(settings.masterVolume, 0.25);
    }
    expect(
      () => world.audio.applySettings(const SettingsDocument(masterVolume: 0.25, musicVolume: -1)),
      throwsStateError,
    );
    expect(world.audio.busVolume(AudioBus.master), 0.5);
  });

  test('module rejection is sticky while disabled audio settings remain a no-op', () {
    final object = world.edit(() => world.spawn(name: 'original'));
    world.modules = const Modules(audio: false);
    expect(
      () => world.edit(() {
        object.name = 'discarded';
        try {
          world.audio.setBus(AudioBus.master, 0.25);
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(object.name, 'original');
    world.edit(() {
      world.audio.applySettings(const SettingsDocument(masterVolume: -1));
      object.name = 'kept';
    });
    expect(object.name, 'kept');
    expect(world.audio.busVolume(AudioBus.master), 1);
    expect(world.audio.muted, isFalse);
    world.modules = Modules.all;
    expect(world.audio.busVolume(AudioBus.master), 1);
  });

  test('ordinary bus operations adopt older commands and keep operation order', () {
    final object = world.spawn(name: 'queued');
    expect(object.isPending, isFalse);
    world.audio.setBus(AudioBus.music, 0.25, muted: true);
    expect(object.isPending, isFalse);
    expect(world.find('queued'), same(object));
    world.edit(() => world.audio.setBus(AudioBus.music, 0.5, muted: true));
    world.audio.setBus(AudioBus.music, 0.75);
    expect(world.audio.busVolume(AudioBus.music), 0.75);
    expect(world.audio.muted, isFalse);
    expect(world.tick, 0);
  });

  test('failed adoption cannot apply bus writes or duplicate older invalid commands', () {
    final object = world.edit(() => world.spawn());
    world.native.submit(
      CommandLane.simulation,
      (CommandBatch()..setTransformOf(object.entity, const Pose(position: Vec3(double.nan, 0, 0)))).encoded(),
    );
    expect(() => world.audio.setBus(AudioBus.master, 0.25, muted: true), throwsStateError);
    final diagnostics = <FynkelDiagnostic>[];
    world.throwOnDiagnostics = false;
    world.onDiagnostics = diagnostics.addAll;
    world.step();
    world.drainEvents();
    expect(diagnostics.single.code, DiagnosticCode.invalidPayload);
    expect(world.audio.busVolume(AudioBus.master), 1);
    expect(world.audio.muted, isFalse);
    world.step();
    expect(diagnostics, hasLength(1));
  });
}
