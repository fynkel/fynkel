// M2-16 gate: "a compiled Dart behaviour can be instantiated from authored
// data". The fixture project's scene has a `game.Spinner` component with
// `speed: 2.5` on the Robot; loading it with the generated registration
// attaches a real `Spinner` with that field set, and it spins.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:spinner_game/generated/behaviours.g.dart';
import 'package:spinner_game/spinner.dart';
import 'package:test/test.dart';

void main() {
  late NativeLibrary lib;
  late String cache;
  final fixture = Directory('../../../../crates/fynkel_document/fixtures/project').absolute.path;
  setUpAll(() {
    lib = NativeLibrary.open();
    // The world loads from the cooked cache only: cook the fixture first.
    cache = cookProject(fixture, cacheDir: Directory.systemTemp.createTempSync('spinner-cache-').path);
  });
  tearDownAll(() {
    lib.dispose();
    Directory(cache).deleteSync(recursive: true);
  });

  test('generated registration covers every @FynkelBehaviour class', () {
    expect(behaviourTypes.map((t) => t.id), ['game.Blinker', 'game.Spinner']);
    expect(gameSpinnerType.version, 1);
    expect(gameBlinkerType.version, 2);
    expect(gameSpinnerType.fields.map((f) => f.name), ['axis', 'showcase', 'speed', 'spinning']);
    final speed = gameSpinnerType.field('speed')!;
    expect(speed.type, 'number');
    expect(speed.defaultValue, 1.0);
    expect(speed.min, 0);
    expect(speed.step, 0.1);
    expect(speed.tooltip, 'Angular speed in radians per second');
    expect(gameSpinnerType.field('spinning')!.label, 'Spinning');
    expect(gameSpinnerType.field('axis')!.defaultValue, [0.0, 1.0, 0.0]);

    final registry = BehaviourRegistry();
    registerBehaviours(registry);
    expect(registry.contains('game.Spinner'), isTrue);
    expect(() => registerBehaviours(registry), throwsArgumentError, reason: 'ids are registered once');
    registerBehaviours(registry, replace: true);
  });

  test('field serializers round-trip JSON values', () {
    final registry = BehaviourRegistry()..registerAll(behaviourTypes);
    final s = registry.instantiate('game.Spinner', fields: {
      'speed': 3,
      'axis': [1, 0, 0],
      'spinning': false,
      'showcase': {'asset': 'c0000000-0000-4000-8000-000000000001', 'sub': 'mesh/0'},
    }) as Spinner;
    expect(s.speed, 3.0);
    expect(s.axis, const Vec3(1, 0, 0));
    expect(s.spinning, isFalse);
    expect(s.showcase, const AssetRef('c0000000-0000-4000-8000-000000000001', sub: 'mesh/0'));
    final t = gameSpinnerType;
    expect(t.field('speed')!.get(s), 3.0);
    expect(t.field('axis')!.get(s), [1.0, 0.0, 0.0]);
    expect(t.field('showcase')!.get(s), {'asset': 'c0000000-0000-4000-8000-000000000001', 'sub': 'mesh/0'});
    final none = registry.instantiate('game.Spinner', fields: {'showcase': null}) as Spinner;
    expect(none.showcase, isNull);
    expect(t.field('showcase')!.get(none), isNull);

    expect(() => registry.instantiate('game.Spinner', fields: {'speed': 'fast'}), throwsA(isA<BehaviourDataError>()));
    expect(() => registry.instantiate('game.Spinner', fields: {'axis': [1, 2]}), throwsA(isA<BehaviourDataError>()));
    expect(() => registry.instantiate('game.Spinner', fields: {'nope': 1}), throwsA(isA<BehaviourDataError>()));
    expect(() => registry.instantiate('game.Spinner', version: 2), throwsA(isA<BehaviourDataError>()));
  });

  test('a scene component instantiates the compiled class with its field set', () {
    final registry = BehaviourRegistry();
    registerBehaviours(registry);
    final w = World.create(library: lib, registry: registry)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final warnings = <LoadDiagnostic>[];
    w.onLoadDiagnostics = warnings.addAll;

    final r = w.loadProject(fixture, cacheDir: cache);
    expect(r.ok, isTrue);
    expect(r.behaviours, hasLength(1));
    expect(r.behaviours.single.type, 'game.Spinner');
    expect(r.behaviours.single.known, isTrue);
    expect(r.behavioursAttached, 1);
    expect(warnings, isEmpty, reason: '$warnings');

    final robot = w.attach(r.behaviours.single.entity);
    expect(robot.name, 'Robot');
    final spinner = robot.behaviour<Spinner>();
    expect(spinner, isNotNull);
    expect(spinner!.speed, 2.5, reason: 'authored value, not the class default');
    expect(spinner.axis, Vec3.up);
    expect(spinner.spinning, isTrue, reason: 'unset fields keep the class default');
    expect(w.behaviours, [spinner]);

    // It runs: one frame of two ticks at 1/30 s spins it by 2.5 * 2/60 rad.
    w.frame(1 / 30);
    expect(spinner.hasStarted, isTrue);
    expect(spinner.angle, closeTo(2.5 * 2 / 60, 1e-9));
    w.frame(1 / 60);
    w.frame(1 / 60);
    final q = robot.worldTransform!.rotation;
    final half = spinner.angle / 2;
    expect(q.y, closeTo(math.sin(half), 1e-4));
    expect(q.w, closeTo(math.cos(half), 1e-4));
  });

  test('an unregistered type or bad data is a warning, not a failed load', () {
    final w = World.create(library: lib)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final warnings = <LoadDiagnostic>[];
    w.onLoadDiagnostics = warnings.addAll;
    final r = w.loadProject(fixture, cacheDir: cache);
    expect(r.ok, isTrue);
    expect(r.behavioursAttached, 0);
    expect(r.entityCount, 5);
    expect(warnings.map((d) => d.code), ['FYN-B0001']);
    expect(warnings.single.pointer, endsWith('/components/c0000000-0000-4000-8000-0000000000c2'));
    expect(w.behaviours, isEmpty);

    // A registered type whose field rejects the authored value.
    final strict = World.create(
      library: lib,
      registry: BehaviourRegistry()
        ..register(BehaviourType(
          id: 'game.Spinner',
          create: Spinner.new,
          fields: [
            BehaviourField(
              name: 'speed',
              type: 'number',
              get: (b) => (b as Spinner).speed,
              set: (b, v) => throw BehaviourDataError('game.Spinner', 'too fast', field: 'speed'),
            ),
          ],
        )),
    )..throwOnDiagnostics = true;
    addTearDown(strict.dispose);
    final strictWarnings = <LoadDiagnostic>[];
    strict.onLoadDiagnostics = strictWarnings.addAll;
    final sr = strict.loadProject(fixture, cacheDir: cache);
    expect(sr.ok, isTrue);
    expect(sr.behavioursAttached, 0);
    expect(strictWarnings.map((d) => d.code), ['FYN-B0002']);
    expect(strictWarnings.single.message, contains('too fast'));
    expect(strictWarnings.single.pointer, endsWith('/data/speed'));
    expect(strict.behaviours, isEmpty);
  });
}
