// Typed native component access from Dart (M3-07a, docs/design/dart-sdk.md
// §21.2): a GameObject reads its native components through one query per
// tick and writes them as `SetComponent`/`RemoveComponent` commands — the
// same path the loader uses — with read-your-writes until the tick applies
// them. No mutable Dart mirror of native state (blueprint §3).

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

  GameObject named(World w, String name) => w.attach(w.allEntities().firstWhere((e) => w.nameOfEntity(e) == name));

  test('a loaded entity\'s components read back typed, one query per object per tick', () {
    final w = loaded();
    final robot = named(w, 'Robot');
    final q0 = w.stats().queryCalls;
    final mesh = robot.get<MeshRenderer>();
    expect(mesh, isNotNull);
    expect(mesh!.visible, isTrue);
    expect(mesh.mesh.sub, 'mesh/Cube');
    expect(mesh.material.asset, 'c0000000-0000-4000-8000-000000000020');
    expect(robot.has<MeshRenderer>(), isTrue);
    expect(robot.has<Camera>(), isFalse);
    expect(robot.get<PointLight>(), isNull);
    expect(robot.get<Name>()!.value, 'Robot');
    expect(robot.get<Transform>(), robot.localTransform);
    expect(robot.has<Transform>(), isTrue);
    expect(robot.components.map((c) => c.runtimeType), [Transform, Name, MeshRenderer, AudioSource]);
    expect(w.stats().queryCalls - q0, 1, reason: 'every read this tick came from one query');
    w.frame(1 / 60);
    robot.get<MeshRenderer>();
    expect(w.stats().queryCalls - q0, 2, reason: 'a new tick, a new query');

    // The camera has its own set; the sun a directional light.
    expect(named(w, 'Camera').get<Camera>()!.projection, Projection.perspective);
    expect(named(w, 'Sun').get<DirectionalLight>()!.intensity, 10000);
    expect(w.componentsOf(named(w, 'Sun').entity).map((c) => c.runtimeType), [Transform, Name, DirectionalLight]);

    // Not a component; a component that cannot go.
    expect(() => robot.get<SnapshotTransformRecord>(), throwsArgumentError);
    expect(() => robot.remove<Transform>(), throwsArgumentError);
    expect(() => robot.remove<Name>(), throwsArgumentError);
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

  test('remove, rename, and a rejected value falls back to what the world has', () {
    final w = loaded();
    w.throwOnDiagnostics = false;
    final diagnostics = <FynkelDiagnostic>[];
    w.onDiagnostics = diagnostics.addAll;
    final sun = named(w, 'Sun');
    final before = sun.get<DirectionalLight>()!;

    sun.remove<DirectionalLight>();
    expect(sun.has<DirectionalLight>(), isFalse);
    expect(sun.components.map((c) => c.runtimeType), [Transform, Name]);
    sun.set(const Name(value: 'Star'));
    expect(sun.get<Name>()!.value, 'Star');
    expect(sun.name, 'Star', reason: 'the handle follows its own rename');
    sun.set(const PointLight(color: Vec3(1, 1, 1), intensity: 100, range: -1, cullingMask: 0xffffffff));
    expect(sun.get<PointLight>()!.range, -1, reason: 'staged, not validated yet');
    w.frame(1 / 60);
    expect(w.componentsOf(sun.entity).map((c) => c.runtimeType), [Transform, Name]);
    expect(w.nameOf(sun), 'Star');
    sun.name = 'Sun again';
    expect(sun.name, 'Sun again');
    w.frame(1 / 60);
    expect(w.nameOf(sun), 'Sun again', reason: 'setting the name renames the entity');
    sun.name = 'Star';
    w.frame(1 / 60);
    expect(sun.get<PointLight>(), isNull, reason: 'rejected: the read is the world\'s');
    expect(diagnostics.single.code, DiagnosticCode.invalidPayload);
    expect(diagnostics.single.message, contains('range'));

    // Removing what is not there is a diagnostic too, not a crash.
    sun.remove<DirectionalLight>();
    w.frame(1 / 60);
    expect(diagnostics, hasLength(2));
    expect(diagnostics.last.message, contains('DirectionalLight'));

    // Back as it was.
    sun.set(before);
    w.frame(1 / 60);
    expect(w.componentsOf(sun.entity).whereType<DirectionalLight>().single, before);
  });

  test('a spawned object takes components in the tick of its spawn; the presentation lane applies now', () {
    final w = loaded();
    final lamp = w.spawn(name: 'Lamp')
      ..add(const PointLight(color: Vec3(0, 1, 0), intensity: 50, range: 4, cullingMask: 0xffffffff))
      ..set(const Name(value: 'Lamp 2'));
    expect(lamp.isPending, isTrue);
    expect(lamp.get<PointLight>()!.intensity, 50, reason: 'read-your-writes before the spawn');
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
    expect(shade.parent, lamp.entity);
    expect(w.componentsOf(shade.entity).whereType<MeshRenderer>(), hasLength(1));

    lamp.setPresentation(lamp.get<PointLight>()!.copyWith(intensity: 5));
    expect(lamp.get<PointLight>()!.intensity, 5);
    w.flushPresentation();
    expect(w.componentsOf(lamp.entity).whereType<PointLight>().single.intensity, 5, reason: 'no tick needed');

    final entity = lamp.entity;
    lamp.destroy();
    w.frame(1 / 60);
    expect(() => lamp.set(const PointLight(color: Vec3(0, 1, 0), intensity: 50, range: 4, cullingMask: 0xffffffff)), throwsStateError);
    expect(w.componentsOf(entity), isEmpty, reason: 'a destroyed entity has nothing');
  });

  findTests(loaded);

  test('the typed table covers every native component the wire knows', () {
    expect(
      componentIds.keys,
      containsAll([Transform, Name, MeshRenderer, Camera, DirectionalLight, PointLight, SpotLight, Environment]),
    );
    for (final MapEntry(key: type, value: id) in componentIds.entries) {
      expect(componentCodecs[id]!.type, type);
    }
  });
}

void findTests(World Function() loaded) {
  test('find, findAll, findChild and findDescendant read names and the hierarchy with one query per tick', () {
    final w = loaded();
    final q0 = w.stats().queryCalls;
    final robot = w.find('Robot');
    expect(robot, isNotNull);
    expect(robot!.name, 'Robot');
    expect(w.find('Robot'), same(robot), reason: 'the same handle for the same entity');
    expect(w.find('Nobody'), isNull);
    expect(w.findAll('Crate A').map((g) => g.entity), [w.find('Crate A')!.entity]);
    expect(w.stats().queryCalls - q0, 1, reason: 'one FYNKEL_QUERY_ENTITIES for the tick');

    // Build a small tree in one tick, then walk it from the next.
    final arm = w.spawn(name: 'Arm', parent: robot);
    final hand = w.spawn(name: 'Hand', parent: arm);
    w.spawn(name: 'Finger', parent: hand);
    w.spawn(name: 'Hand', parent: robot); // a second Hand, a direct child
    expect(w.find('Arm'), isNull, reason: 'not spawned until the tick');
    w.frame(1 / 60);
    expect(w.find('Arm')!.entity, arm.entity);
    expect(robot.findChild('Arm')!.entity, arm.entity);
    expect(robot.findChild('Finger'), isNull, reason: 'direct children only');
    expect(robot.findChild('Arm/Hand')!.entity, hand.entity);
    expect(robot.findChild('Arm/Hand/Finger')!.name, 'Finger');
    expect(robot.findChild('Arm/Nothing'), isNull);
    expect(robot.findDescendant('Finger')!.entity, robot.findChild('Arm/Hand/Finger')!.entity);
    expect(w.findAll('Hand'), hasLength(2));
    expect(robot.findChild('Hand')!.entity, isNot(hand.entity), reason: 'the direct child, not the one under Arm');
    expect(hand.findChild(''), same(hand), reason: 'an empty path is the object itself');

    // A rename lands at the tick, and the table follows.
    arm.set(const Name(value: 'Limb'));
    expect(w.find('Limb'), isNull);
    w.frame(1 / 60);
    expect(w.find('Limb')!.entity, arm.entity);
    expect(w.find('Arm'), isNull);
  });
}
