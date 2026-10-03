// The project's optional modules from Dart (ADR-0029 Decision 3): a load
// reports them as `world.modules`, and the calls into a module the project
// turns off throw a StateError naming it before anything reaches the
// engine (which would reject the command with `module_off` anyway).

import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-modules-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    return w;
  }

  Matcher namesModule(String module) => throwsA(
    isA<StateError>().having(
      (e) => e.message,
      'message',
      allOf(contains('$module module'), contains('modules.$module')),
    ),
  );

  test('every module is on before a load and for a project that keeps them', () {
    final w = fresh();
    expect(w.modules, Modules.all);
    w.onLoadDiagnostics = (_) {};
    final report = w.loadProject(fixture, cacheDir: cache);
    expect(report.modules, Modules.all);
    for (final m in Module.values) {
      expect(w.modules.has(m), isTrue, reason: m.key);
    }
  });

  test('the load report reads the modules the project turned off', () {
    final r = LoadReport.fromJson({
      'ok': true,
      'modules': {'physics': true, 'audio': false, 'animation': false},
    });
    expect(r.modules, const Modules(audio: false, animation: false));
    expect(r.modules!.has(Module.physics), isTrue);
    expect(r.modules!.has(Module.audio), isFalse);
    expect(LoadReport.fromJson({'ok': true}).modules, isNull);
    expect(DiagnosticCode.fromValue(21), DiagnosticCode.moduleOff);
  });

  test('physics off: queries and body commands throw', () {
    final w = fresh()..modules = const Modules(physics: false);
    final o = w.spawn(name: 'o');
    expect(() => w.raycast(Vec3.zero, const Vec3(0, -1, 0)), namesModule('physics'));
    expect(() => w.overlap(ColliderShape.sphere, const Vec3(1, 1, 1), Vec3.zero), namesModule('physics'));
    expect(
      () => w.shapeCast(ColliderShape.sphere, const Vec3(1, 1, 1), Vec3.zero, Vec3.zero),
      namesModule('physics'),
    );
    expect(() => w.physicsQuery(const []), namesModule('physics'));
    expect(() => o.applyImpulse(const Vec3(0, 1, 0)), namesModule('physics'));
    expect(() => o.move(const Vec3(1, 0, 0)), namesModule('physics'));
    // The other modules still answer.
    o.set(const AudioSource());
    o.audio.stop();
  });

  test('audio off: sounds, buses and sources throw; the settings are kept quietly', () {
    final w = fresh()..modules = const Modules(audio: false);
    final o = w.spawn(name: 'o');
    expect(() => w.audio.play(const AssetRef('c0000000-0000-4000-8000-000000000006')), namesModule('audio'));
    expect(() => w.audio.setBus(AudioBus.music, 0.5), namesModule('audio'));
    expect(() => o.audio.play(), namesModule('audio'));
    w.audio.applySettings(const SettingsDocument());
  });

  test('animation off: animator calls throw', () {
    final w = fresh()..modules = const Modules(animation: false);
    final o = w.spawn(name: 'o');
    expect(() => o.animation.stop(), namesModule('animation'));
    expect(() => o.animation.crossfade(AssetRef.none, 0.2), namesModule('animation'));
    o.set(const RigidBody());
    o.applyImpulse(const Vec3(0, 1, 0));
  });
}
