// M2-16 gate: "a compiled Dart behaviour can be instantiated from authored
// data". The fixture project's scene has a `game.Spinner` component with
// `speed: 2.5` on the Robot; loading it with the generated registration
// attaches a real `Spinner` with that field set, and it spins.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:spinner_game/generated/behaviours.g.dart';
import 'package:spinner_game/spawner.dart';
import 'package:spinner_game/spinner.dart';
import 'package:spinner_game/weapon.dart';
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
    expect(behaviourTypes.map((t) => t.id),
        ['game.Blinker', 'game.Chime', 'game.Patrol', 'game.Spawner', 'game.Spinner', 'game.Wielder']);
    expect(dataTypes.map((t) => t.id), ['game.Weapon']);
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

  test('List<T> fields round-trip, each item converted, the JSON a copy', () {
    final registry = BehaviourRegistry()..registerAll(behaviourTypes);
    final delays = gameChimeType.field('delays')!;
    expect(delays.type, 'list');
    expect(delays.defaultValue, [0.5, 1.0]);
    expect(gameChimeType.field('moods')!.defaultValue, ['soft']);
    final c = registry.instantiate('game.Chime', fields: {
      'delays': [1, 2.5],
      'moods': ['bright', 'soft'],
      'sounds': [
        {'asset': 'c0000000-0000-4000-8000-000000000002'},
      ],
    }) as Chime;
    expect(c.delays, [1.0, 2.5]);
    expect(c.moods, [ChimeMood.bright, ChimeMood.soft]);
    expect(c.sounds, [const AssetRef('c0000000-0000-4000-8000-000000000002')]);
    final json = delays.get(c) as List;
    expect(json, [1.0, 2.5]);
    json.add(9);
    expect(c.delays, hasLength(2), reason: 'the JSON is a copy');
    expect(gameChimeType.field('moods')!.get(c), ['bright', 'soft']);
    expect(gameChimeType.field('sounds')!.get(c), [
      {'asset': 'c0000000-0000-4000-8000-000000000002'},
    ]);
    c.delays.add(3); // a set list is growable
    expect(() => registry.instantiate('game.Chime', fields: {'moods': ['loud']}), throwsA(isA<BehaviourDataError>()));
    expect(() => registry.instantiate('game.Chime', fields: {'delays': 1}), throwsA(isA<BehaviourDataError>()));
  });

  test('a data type (ADR-0027): registered with the behaviours, built from its data, shared by reference', () {
    final registry = BehaviourRegistry();
    registerBehaviours(registry);
    expect(registry.lookupData('game.Weapon'), same(registry.dataTypes.single));
    final damage = gameWeaponType.field('damage')!;
    expect((damage.defaultValue, damage.min), (1, 0), reason: 'the constructor default');
    expect(gameWeaponType.field('cooldown')!.defaultValue, 0.5);
    expect(gameWeaponType.field('upgrade')!.dataType, 'game.Weapon');
    final sword = registry.instantiateData('game.Weapon', fields: {
      'damage': 3,
      'sounds': [
        {'asset': 'c0000000-0000-4000-8000-000000000002'},
      ],
      // Resolved by the world, not here.
      'upgrade': {'asset': 'c0000000-0000-4000-8000-000000000a02'},
    }) as Weapon;
    expect((sword.damage, sword.cooldown, sword.upgrade), (3, 0.5, null));
    final weapon = registry.lookupData('game.Weapon');
    expect(sword.dataType, same(weapon));
    expect(sword.asset, isNull, reason: 'made from data, not loaded');
    expect(gameWeaponType.field('sounds')!.get(sword), [
      {'asset': 'c0000000-0000-4000-8000-000000000002'},
    ]);
    expect(registry.dataTypeOf(Weapon()), same(weapon));
    expect(() => registry.instantiateData('game.Weapon', fields: {'damage': 'lots'}), throwsA(isA<DataAssetError>()));
    expect(() => registry.instantiateData('game.Shield'), throwsA(isA<DataAssetError>()));
    expect(() => registerBehaviours(registry), throwsArgumentError, reason: 'data ids are registered once too');

    // A behaviour's data-typed fields hold the instance; its JSON is the
    // instance's asset (none for one made in code).
    final w = registry.instantiate('game.Wielder', fields: {
      'weapon': {'asset': 'c0000000-0000-4000-8000-000000000a01'},
    }) as Wielder;
    expect(w.weapon, isNull, reason: 'the world resolves it');
    final field = gameWielderType.field('weapon')!;
    expect((field.type, field.dataType, field.holdsData), ('data', 'game.Weapon', true));
    field.set(w, sword);
    gameWielderType.field('spares')!.set(w, [sword, Weapon(damage: 5)]);
    w.hit();
    expect(w.dealt, 3);
    expect(field.target!(w), same(sword));
    expect(field.get(w), isNull);
    expect(gameWielderType.field('spares')!.get(w), [null, null]);
    expect(dataJson(null), isNull);
  });

  test('@FynkelRecord (ADR-0027 §5): records alone and in lists, built from JSON objects, copied as values', () {
    final registry = BehaviourRegistry()..registerAll(behaviourTypes);
    const prefab = {'asset': 'c0000000-0000-4000-8000-000000000001'};
    final s = registry.instantiate('game.Spawner', fields: {
      'waves': [
        {'enemy': prefab, 'count': 5, 'spread': {'radius': 4, 'ring': true}},
        {'delay': 2.5},
      ],
      'boss': {'enemy': null},
    }) as Spawner;
    expect(s.waves, hasLength(2));
    final (a, b) = (s.waves[0], s.waves[1]);
    expect((a.enemy, a.count, a.delay, a.spread.radius, a.spread.ring),
        (const AssetRef('c0000000-0000-4000-8000-000000000001'), 5, 1.0, 4.0, true));
    expect((b.enemy, b.count, b.delay, b.spread.radius), (null, 3, 2.5, 2.0), reason: 'keys left out: the class defaults');
    expect(s.boss.count, 3, reason: "the record class's own default, not the field initializer's");
    expect(s.total, 11);

    final waves = gameSpawnerType.field('waves')!;
    expect(waves.type, 'list');
    expect(waves.defaultValue, const []);
    expect(gameSpawnerType.field('boss')!.type, 'record');
    expect(waves.get(s), [
      {'count': 5, 'delay': 1.0, 'enemy': prefab, 'spread': {'radius': 4.0, 'ring': true}},
      {'count': 3, 'delay': 2.5, 'enemy': null, 'spread': {'radius': 2.0, 'ring': false}},
    ]);
    // What a load or a clone does: the JSON back into new instances.
    final copy = Spawner();
    waves.set(copy, waves.get(s));
    expect(copy.waves[0], isNot(same(a)));
    expect(copy.waves[0].spread.radius, 4.0);
    copy.waves[0].spread.radius = 9;
    expect(a.spread.radius, 4.0, reason: 'no identity: a copy shares nothing');
    expect(() => registry.instantiate('game.Spawner', fields: {'boss': {'count': 'many'}}),
        throwsA(isA<BehaviourDataError>()));
  });

  test('a List<Vec3> field (a @Gizmo.path) round-trips', () {
    final registry = BehaviourRegistry()..registerAll(behaviourTypes);
    final waypoints = gamePatrolType.field('waypoints')!;
    expect(waypoints.type, 'vec3List');
    expect(waypoints.defaultValue, [
      [0.0, 0.0, 0.0],
      [4.0, 0.0, 0.0],
      [4.0, 0.0, -4.0],
    ]);
    final p = registry.instantiate('game.Patrol', fields: {
      'waypoints': [
        [1, 2, 3],
        [0, 0, 0],
      ],
    }) as Patrol;
    expect(p.waypoints, [const Vec3(1, 2, 3), Vec3.zero]);
    expect(waypoints.get(p), [
      [1.0, 2.0, 3.0],
      [0.0, 0.0, 0.0],
    ]);
    expect(
      () => registry.instantiate('game.Patrol', fields: {
        'waypoints': [
          [1, 2],
        ],
      }),
      throwsA(isA<BehaviourDataError>()),
    );
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
    expect(r.entityCount, 6, reason: 'a prefab instance is its root (ADR-0022)');
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
