// Typed component access shares the authoritative gameplay journal.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

/// Hides its mesh on the first tick, brightens a light it adds on the second.
class Blinker extends Behaviour {
  final List<String> log = [];
  int ticks = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    final mesh = get<MeshRenderer>()!;
    log.add('tick $ticks visible=${mesh.visible} light=${has<PointLight>()}');
    if (ticks == 0) {
      set(mesh.copyWith(visible: false));
      expect(get<MeshRenderer>()!.visible, isFalse, reason: 'read-your-writes');
    } else if (ticks == 1) {
      set(const PointLight(color: Vec3(1, 0.5, 0.25), intensity: 800, range: 12, cullingMask: 0xffffffff));
    } else if (ticks == 2) {
      set(get<PointLight>()!.copyWith(intensity: 1600));
    }
    ticks++;
  }
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-components-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 4);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(fixture, cacheDir: cache);
    w.frame(1 / 60); // the loader's component commands apply at the first tick
    return w;
  }

  GameObject named(World w, String name) =>
      w.attach(w.allEntities().firstWhere((e) => w.nameOfEntity(e) == name));

  test('a loaded entity\'s components read back typed through native authority', () {
    final w = loaded();
    final robot = named(w, 'Robot');
    final mesh = robot.get<MeshRenderer>();
    expect(mesh, isNotNull);
    expect(mesh!.visible, isTrue);
    expect(mesh.mesh.sub, 'mesh/Cube');
    expect(mesh.material.asset, 'c0000000-0000-4000-8000-000000000020');
    expect(robot.has<MeshRenderer>(), isTrue);
    expect(robot.has<Camera>(), isFalse);
    expect(robot.get<PointLight>(), isNull);
    expect(robot.get<Name>()!.value, 'Robot');
    expect(robot.get<Pose>(), robot.localTransform);
    expect(robot.has<Pose>(), isTrue);
    expect(robot.components.map((c) => c.runtimeType), [Pose, Name, MeshRenderer, AudioSource]);
    w.frame(1 / 60);
    robot.get<MeshRenderer>();

    // The camera has its own set; the sun a directional light.
    expect(named(w, 'Camera').get<Camera>()!.projection, Projection.perspective);
    expect(named(w, 'Sun').get<DirectionalLight>()!.intensity, 10000);
    expect(w.componentsOf(named(w, 'Sun').entity).map((c) => c.runtimeType), [Pose, Name, DirectionalLight]);

    // Not a component; a component that cannot go.
    expect(() => robot.get<SnapshotTransformRecord>(), throwsArgumentError);
    expect(() => robot.remove<Pose>(), throwsStateError);
    expect(() => robot.remove<Name>(), throwsStateError);
  });

  test('a behaviour writes components through commands and reads them back across ticks', () {
    final w = loaded();
    final robot = named(w, 'Robot');
    final b = robot.addBehaviour(Blinker());
    w.frame(1 / 60); // tick 0: attach
    w.frame(1 / 60);
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(b.log, [
      'tick 0 visible=true light=false',
      'tick 1 visible=false light=false',
      'tick 2 visible=false light=true',
      'tick 3 visible=false light=true',
    ]);
    // What the world has, straight from a fresh query.
    final now = w.componentsOf(robot.entity);
    expect(now.whereType<MeshRenderer>().single.visible, isFalse);
    expect(now.whereType<PointLight>().single.intensity, 1600);
  });

  test('remove and rename publish immediately, while invalid writes reject synchronously', () {
    final w = loaded();
    final sun = named(w, 'Sun');
    final before = sun.get<DirectionalLight>()!;
    sun.remove<DirectionalLight>();
    expect(sun.has<DirectionalLight>(), isFalse);
    expect(w.componentsOf(sun.entity).map((c) => c.runtimeType), [Pose, Name]);
    sun.set(const Name(value: 'Star'));
    expect(sun.name, 'Star');
    expect(w.nameOf(sun), 'Star');
    expect(
      () =>
          sun.set(const PointLight(color: Vec3(1, 1, 1), intensity: 100, range: -1, cullingMask: 0xffffffff)),
      throwsStateError,
    );
    expect(sun.get<PointLight>(), isNull);
    expect(sun.components.map((c) => c.runtimeType), [Pose, Name]);
    expect(() => sun.remove<DirectionalLight>(), throwsStateError);
    sun.set(before);
    expect(w.componentsOf(sun.entity).whereType<DirectionalLight>().single, before);
  });

  test('spawned objects and component writes publish without a tick', () {
    final w = loaded();
    final lamp = w.spawn(name: 'Lamp')
      ..add(const PointLight(color: Vec3(0, 1, 0), intensity: 50, range: 4, cullingMask: 0xffffffff))
      ..set(const Name(value: 'Lamp 2'));
    expect(lamp.isPending, isFalse);
    expect(lamp.get<PointLight>()!.intensity, 50, reason: 'published immediately');
    expect(lamp.has<PointLight>(), isTrue);
    final shade = w.spawn(name: 'Shade', parent: lamp)
      ..add(
        const MeshRenderer(
          mesh: AssetRef.none,
          material: AssetRef.none,
          castShadows: true,
          receiveShadows: true,
          visible: true,
        ),
      );
    w.frame(1 / 60); // one tick: spawn, components, child — temp references (M3-08b)
    expect(lamp.isPending, isFalse);
    expect(shade.isPending, isFalse);
    expect(lamp.get<PointLight>()!.intensity, 50);
    expect(w.componentsOf(lamp.entity).whereType<PointLight>().single.range, 4);
    expect(w.nameOf(lamp), 'Lamp 2');
    expect(shade.parent, lamp);
    expect(w.componentsOf(shade.entity).whereType<MeshRenderer>(), hasLength(1));

    lamp.setPresentation(lamp.get<PointLight>()!.copyWith(intensity: 5));
    expect(lamp.get<PointLight>()!.intensity, 5);
    w.flushPresentation();
    expect(w.componentsOf(lamp.entity).whereType<PointLight>().single.intensity, 5, reason: 'no tick needed');

    final entity = lamp.entity;
    lamp.destroy();
    w.frame(1 / 60);
    expect(
      () =>
          lamp.set(const PointLight(color: Vec3(0, 1, 0), intensity: 50, range: 4, cullingMask: 0xffffffff)),
      throwsStateError,
    );
    expect(w.componentsOf(entity), isEmpty, reason: 'a destroyed entity has nothing');
  });

  test('component reads follow commits and aborts without advancing the clock', () {
    final w = loaded();
    final robot = named(w, 'Robot');
    final original = robot.get<MeshRenderer>()!;
    w.edit(() {
      robot.set(original.copyWith(visible: false));
      expect(robot.get<MeshRenderer>()!.visible, isFalse);
      expect(robot.components.whereType<MeshRenderer>().single.visible, isFalse);
    });
    expect(robot.get<MeshRenderer>()!.visible, isFalse);
    expect(w.componentsOf(robot.entity).whereType<MeshRenderer>().single.visible, isFalse);
    expect(
      () => w.edit(() {
        robot.remove<MeshRenderer>();
        expect(robot.has<MeshRenderer>(), isFalse);
        expect(robot.components.whereType<MeshRenderer>(), isEmpty);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(robot.has<MeshRenderer>(), isTrue);
    expect(robot.get<MeshRenderer>()!.visible, isFalse);
    robot.setPresentation(original);
    expect(w.componentsOf(robot.entity).whereType<MeshRenderer>().single.visible, isTrue);
    expect(robot.get<MeshRenderer>()!.visible, isTrue);
  });

  findTests(loaded);

  test('the typed table covers every native component the wire knows', () {
    expect(
      componentIds.keys,
      containsAll([Pose, Name, MeshRenderer, Camera, DirectionalLight, PointLight, SpotLight, Environment]),
    );
    for (final MapEntry(key: type, value: id) in componentIds.entries) {
      expect(componentCodecs[id]!.type, type);
    }
  });
}

void findTests(World Function() loaded) {
  test(
    'find, findAll, findChild and findDescendant read names and the hierarchy with one query per tick',
    () {
      final w = loaded();
      final robot = w.find('Robot');
      expect(robot, isNotNull);
      expect(robot!.name, 'Robot');
      expect(w.find('Robot'), same(robot), reason: 'the same handle for the same entity');
      expect(w.find('Nobody'), isNull);
      expect(w.findAll('Crate A').map((g) => g.entity), [w.find('Crate A')!.entity]);

      // Build a small tree in one tick, then walk it from the next.
      final arm = w.spawn(name: 'Arm', parent: robot);
      final hand = w.spawn(name: 'Hand', parent: arm);
      w.spawn(name: 'Finger', parent: hand);
      w.spawn(name: 'Hand', parent: robot); // a second Hand, a direct child
      expect(w.find('Arm'), same(arm), reason: 'published immediately');
      w.frame(1 / 60);
      expect(w.find('Arm')!.entity, arm.entity);
      expect(robot.findChild('Arm')!.entity, arm.entity);
      expect(robot.findChild('Finger'), isNull, reason: 'direct children only');
      expect(robot.findChild('Arm/Hand')!.entity, hand.entity);
      expect(robot.findChild('Arm/Hand/Finger')!.name, 'Finger');
      expect(robot.findChild('Arm/Nothing'), isNull);
      expect(robot.findDescendant('Finger')!.entity, robot.findChild('Arm/Hand/Finger')!.entity);
      expect(w.findAll('Hand'), hasLength(2));
      expect(
        robot.findChild('Hand')!.entity,
        isNot(hand.entity),
        reason: 'the direct child, not the one under Arm',
      );
      expect(hand.findChild(''), same(hand), reason: 'an empty path is the object itself');

      // A rename is immediately visible to queries.
      arm.set(const Name(value: 'Limb'));
      expect(w.find('Limb'), same(arm));
      w.frame(1 / 60);
      expect(w.find('Limb')!.entity, arm.entity);
      expect(w.find('Arm'), isNull);
    },
  );
}
