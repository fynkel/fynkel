// The native component catalogue from Dart (M3-10, blueprint §19.2d item
// 10): the six components are ordinary typed components (`add`/`get`/
// `set`/`remove` on the M3-07a path), the fixture's Catalogue scene moves
// with no behaviour attached, and `systemTimes()` reports every phase.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const catalogue = 'c0000000-0000-4000-8000-000000000032';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-catalogue-cache-').path;
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

  /// One fixed tick the way `frame` runs it: step, then the events (which
  /// resolves spawns and sends the transform subscriptions).
  void tick(World w, [int n = 1]) {
    for (var i = 0; i < n; i++) {
      w.step();
      w.drainEvents();
    }
  }

  test('the catalogue components are typed native components', () {
    final w = fresh();
    final hero = w.spawn(name: 'Hero');
    final lamp = w.spawn(name: 'Lamp')
      ..add(Follow(target: hero, offset: const Vec3(0, 2, 0), smoothing: 0, lookAt: false));
    final spinner =
        w.spawn(
            name: 'Spinner',
            transform: const Pose(position: Vec3(3, 0, 0), rotation: Quat.identity, scale: Vec3.one),
          )
          ..add(const Rotator(axis: Vec3(0, 1, 0), speed: 2, space: MotionSpace.local))
          ..add(
            const Oscillate(
              axis: Vec3(0, 1, 0),
              amplitude: 0.5,
              frequency: 1,
              phase: 0,
              space: MotionSpace.world,
            ),
          )
          ..add(const LookAtCamera(lockY: true))
          ..add(const LifetimeDespawn(seconds: 0.5));
    tick(w, 2);
    expect(lamp.get<Follow>()?.target, same(hero));
    expect(spinner.get<Rotator>()?.speed, 2);
    expect(spinner.has<Oscillate>(), isTrue);
    expect(spinner.get<LookAtCamera>()?.lockY, isTrue);
    expect(spinner.get<LifetimeDespawn>()!.seconds, closeTo(0.5 - 2 / 60, 1e-5), reason: 'counts down');
    expect(hero.has<Emitter>(), isFalse);
    // They moved without a command: the lamp is 2 m over the hero, the
    // spinner turned 2 rad/s × 2 ticks about Y and bobbed.
    expect(lamp.worldTransform!.position.y, closeTo(2, 1e-4));
    expect(spinner.localTransform.rotation.y, closeTo(math.sin(2 * 2 / 60 / 2), 1e-4));
    expect(spinner.localTransform.position.y, closeTo(0.5 * math.sin(2 * math.pi * 2 / 60), 1e-4));
    expect(w.systemTimes().firstWhere((t) => t.system == SystemKind.rotator).entities, 1);

    // Replacing and removing are ordinary writes.
    spinner.set(const Rotator(axis: Vec3(0, 1, 0), speed: 0, space: MotionSpace.local));
    spinner.remove<Oscillate>();
    tick(w);
    final before = spinner.localTransform;
    tick(w);
    expect(spinner.localTransform.rotation.y, closeTo(before.rotation.y, 1e-6), reason: 'speed 0');
    expect(spinner.localTransform.position.y, before.position.y, reason: 'no oscillator');
    // The lifetime runs out at 0.5 s: the spinner is gone, hero and lamp stay.
    tick(w, 30);
    expect(spinner.isAlive, isFalse);
    expect(hero.isAlive && lamp.isAlive, isTrue);
    // A destroyed target is missing, never replaced (ADR-0035 §5): the lamp
    // stays where it is, even with another "Hero" in the world.
    final at = lamp.worldTransform!.position;
    hero.destroy();
    w.spawn(
      name: 'Hero',
      transform: const Pose(position: Vec3(5, 0, 0), rotation: Quat.identity, scale: Vec3.one),
    );
    tick(w, 2);
    expect(lamp.get<Follow>()!.target, isNull);
    expect(lamp.worldTransform!.position.x, closeTo(at.x, 1e-6));
  });

  test('an emitter without its prefab is reported once', () {
    final w = fresh();
    final seen = <FynkelDiagnostic>[];
    w
      ..throwOnDiagnostics = false
      ..onDiagnostics = seen.addAll;
    w
        .spawn(name: 'Fountain')
        .add(
          const Emitter(
            prefab: AssetRef('c0000000-0000-4000-8000-000000000041'),
            rate: 60,
            lifetime: 1,
            maxAlive: 0,
            velocity: Vec3(0, 0, 0),
            enabled: true,
          ),
        );
    tick(w, 4);
    final reports = seen.where((d) => d.code == DiagnosticCode.prefabUnavailable).toList();
    expect(reports, hasLength(1), reason: '$seen');
    expect(reports.single.component, Emitter.componentId);
    expect(w.allEntities(), hasLength(1));
  });

  test('the Catalogue scene runs with no behaviour and reports per-system time', () {
    final w = fresh();
    w.onLoadDiagnostics = (_) {};
    final report = w.loadScene(catalogue, cacheDir: cache);
    expect(report.ok, isTrue, reason: '$report');
    expect(report.entityCount, 10);
    expect(report.physics.prefabs, 1, reason: 'the Spark prefab the Fountain names');
    final beacon = w.find('Beacon')!;
    final buoy = w.find('Buoy')!;
    final lamp = w.find('Lamp')!;
    final sign = w.find('Sign')!;
    final puff = w.find('Puff')!;
    final core = w.find('Puff core')!;
    tick(w, 300);
    // 5 s at 1.5 rad/s about Y.
    expect(beacon.localTransform.rotation.y.abs(), closeTo(math.sin(7.5 / 2).abs(), 1e-3));
    // 2.5 periods at 0.5 Hz: the buoy is back at its rest height.
    expect(buoy.localTransform.position.y, closeTo(1, 1e-2));
    // The lamp trails it, 2 m up, on its axis.
    expect(lamp.worldTransform!.position.x, closeTo(0, 1e-4));
    expect(lamp.worldTransform!.position.y, closeTo(3, 0.4));
    // The sign's +Z points at the camera (0, 4, 10) with the tilt locked.
    final facing = sign.localTransform.rotation.rotate(const Vec3(0, 0, 1));
    final toCamera = const Vec3(0, 4, 10) - sign.worldTransform!.position;
    final flat = Vec3(toCamera.x, 0, toCamera.z);
    expect(facing.dot(flat) / flat.length, closeTo(1, 1e-3));
    expect(facing.y.abs(), lessThan(1e-4));
    expect(puff.isAlive, isFalse, reason: 'despawned at 1 s');
    expect(core.isAlive, isFalse, reason: 'with its subtree');
    // 12 Sparks alive (one root each), every one a dynamic body.
    final sparks = w.findAll('Spark');
    expect(sparks, hasLength(12));
    expect(w.allEntities(), hasLength(10 - 2 + 12));
    final bodies = sparks.where((s) => s.has<RigidBody>()).toList();
    expect(bodies, hasLength(12));
    expect(bodies.first.bodyState(), isNotNull);

    final times = w.systemTimes();
    expect(times.map((t) => t.system), SystemKind.values);
    final by = {for (final t in times) t.system: t};
    expect(by[SystemKind.step]!.entities, 20);
    expect(by[SystemKind.rotator]!.entities, 1);
    expect(by[SystemKind.oscillate]!.entities, 1);
    expect(
      by[SystemKind.lifetimeDespawn]!.entities,
      inInclusiveRange(11, 12),
      reason: 'counted before this tick\'s emission',
    );
    expect(by[SystemKind.emitter]!.entities, 1);
    expect(by[SystemKind.physics]!.entities, 12 + 13);
    expect(by[SystemKind.follow]!.entities, 1);
    expect(by[SystemKind.lookAtCamera]!.entities, 1);
    expect(by[SystemKind.step]!.micros, greaterThanOrEqualTo(by[SystemKind.physics]!.micros));
    expect(by[SystemKind.physics]!.milliseconds, by[SystemKind.physics]!.micros / 1000);
  });
}
