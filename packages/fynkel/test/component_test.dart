// Components and behaviours (docs/stage-4.md §3.1, step 1).

import 'dart:async';
import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'tapping.dart';

import 'mesh_test.dart' show throwsRefusal;

/// Every call any object or behaviour gets, in order.
final log = <String>[];

class Logged extends GameObject {
  Logged(String name, {super.components, super.children}) : super(name: name);

  @override
  void onStart() {
    super.onStart();
    log.add('start $name');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update $name');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    log.add('destroy $name');
  }
}

/// Logs every call it gets, with its [label].
class Logs extends Behaviour {
  Logs(this.label, {this.load});

  final String label;

  /// What its load waits for, if anything.
  final Future<void>? load;

  @override
  FutureOr<void> onLoad() {
    super.onLoad();
    log.add('load $label');
    return load;
  }

  @override
  void onStart() {
    super.onStart();
    log.add('start $label');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update $label');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    log.add('destroy $label');
  }
}

abstract interface class Damageable {
  void hurt(int damage);
}

class Health extends Behaviour implements Damageable {
  Health(this.hp);
  int hp;

  @override
  void hurt(int damage) => hp -= damage;
}

class Armour extends Health {
  Armour(super.hp);
}

/// Takes taps when [takes], and logs them.
class Clicks extends Behaviour {
  Clicks(this.label, {this.takes = false});
  final String label;
  final bool takes;

  @override
  bool onClick(Finger finger) {
    log.add('tap $label');
    return takes;
  }
}

/// Removes [target] in its first update.
class Remover extends Behaviour {
  Remover(this.target);
  final Behaviour target;

  @override
  void onUpdate() {
    super.onUpdate();
    if (!target.isRemoved) target.gameObject.removeComponent(target);
  }
}

MeshRenderer cube() => MeshRenderer(Mesh.cube(), Material());
const centre = ViewPoint(50, 50, width: 100, height: 100);

void main() {
  late World world;
  setUp(() {
    world = World();
    log.clear();
  });
  tearDown(() => world.dispose());

  test('behaviours get the lifecycle after their object, in the order they were added', () {
    final root = world.add(
      Logged(
        'root',
        components: [Logs('a'), Logs('b')],
        children: [
          Logged('child', components: [Logs('c')]),
        ],
      ),
    );
    expect(log, ['load a', 'load b', 'load c']);
    log.clear();

    world.update(1 / 60);
    expect(log, [
      'start root', 'start a', 'start b', 'start child', 'start c', //
      'update root', 'update a', 'update b', 'update child', 'update c',
    ]);
    log.clear();

    root.destroy();
    expect(log, ['destroy child', 'destroy c', 'destroy root', 'destroy a', 'destroy b']);
  });

  test('more suns or lamps than a frame takes are noted, once each', () {
    final said = <String>[];
    runZoned(() {
      final world = World();
      addTearDown(world.dispose);
      for (var i = 0; i < 4; i++) {
        world.add(GameObject(components: [Light.sun()]));
      }
      world.add(GameObject(components: [Light.ambient()]));
      for (var i = 0; i < 64; i++) {
        world.add(GameObject(components: [Light.point()]));
      }
      expect(said, isEmpty, reason: 'four suns, 64 lamps and an ambient light all light a frame');
      final fifth = world.add(GameObject(name: 'fifth', components: [Light.sun()]));
      world.add(GameObject(components: [Light.sun()]));
      fifth.get<Light>()!.intensity = 2;
      expect(said, hasLength(1));
      world.add(GameObject(name: 'spare', components: [Light.spot()]));
      world.add(GameObject(components: [Light.point()]));
    }, zoneSpecification: ZoneSpecification(print: (_, _, _, line) => said.add(line)));
    expect(said, hasLength(2));
    expect(said[0], allOf(contains('fifth'), contains('5 suns'), contains('the first four made light the scene')));
    expect(said[1], allOf(contains('spare'), contains('65 point and spot lights'), contains('nearest the camera')));
  });

  test('the suns and lamps are counted as they come, go and change kind', () {
    final said = <String>[];
    runZoned(() {
      final world = World();
      addTearDown(world.dispose);
      final suns = [
        for (var i = 0; i < 4; i++) world.add(GameObject(components: [Light.sun()])),
      ];
      suns[0].destroy();
      suns[1].removeComponent(suns[1].get<Light>()!);
      world.add(GameObject(components: [Light.sun()]));
      world.add(GameObject(components: [Light.sun()]));
      expect(said, isEmpty, reason: 'two went, so four light the scene');
      final lamp = world.add(GameObject(name: 'turned', components: [Light.point()]));
      lamp.get<Light>()!.kind = LightKind.sun;
    }, zoneSpecification: ZoneSpecification(print: (_, _, _, line) => said.add(line)));
    expect(said, hasLength(1));
    expect(said[0], allOf(contains('turned'), contains('5 suns')));
  });

  test('a spot light wider than 150° is refused shadows, saying why', () {
    // Hardening R7: its map can't be wider than 160°, and outside it a
    // surface would be lit as if nothing stood in the way.
    final world = World();
    addTearDown(world.dispose);
    final says = throwsA(
      isA<ArgumentError>().having((e) => '${e.message}', 'message', allOf(contains('150°'), contains('160°'))),
    );
    expect(() => world.add(GameObject(components: [Light.spot(angle: 160.deg, shadows: true)])), says);
    final wide = world.add(GameObject(components: [Light.spot(angle: 160.deg)])).get<Light>()!;
    expect(() => wide.shadows = true, says);
    expect(wide.shadows, isFalse);
    final narrow = world.add(GameObject(components: [Light.spot(angle: 150.deg, shadows: true)])).get<Light>()!;
    expect(() => narrow.angle = 160.deg, says);
    expect(narrow.angle.inDegrees, closeTo(150, 1e-9));
    // Without shadows it may be as wide as it likes.
    wide.angle = 179.deg;
  });

  test('changing a light costs the same however many objects there are', () {
    Duration time(int others) {
      final world = World();
      addTearDown(world.dispose);
      for (var i = 0; i < others; i++) {
        world.add(GameObject());
      }
      final light = world.add(GameObject(components: [Light.point()])).get<Light>()!;
      final watch = Stopwatch()..start();
      for (var i = 0; i < 2000; i++) {
        light.intensity = 1 + i % 2;
      }
      return watch.elapsed;
    }

    time(0);
    final alone = time(0);
    final crowded = time(5000);
    expect(crowded.inMicroseconds, lessThan(alone.inMicroseconds * 3 + 2000), reason: 'alone $alone, crowded $crowded');
  });

  test('get finds a component by its type, a subtype or an interface, before and after adding', () {
    final armour = Armour(5);
    final renderer = cube();
    final knight = GameObject(components: [renderer, armour, Health(10)]);

    expect(knight.get<Armour>(), same(armour));
    expect(knight.get<Health>(), same(armour), reason: 'the first one that is a Health');
    expect(knight.get<Damageable>(), same(armour));
    expect(knight.getAll<Health>(), hasLength(2));
    expect(knight.get<MeshRenderer>(), same(renderer));
    expect(knight.get<Camera>(), isNull);
    expect(knight.has<Light>(), isFalse);
    expect(armour.gameObject, same(knight));
    expect(armour.get<MeshRenderer>(), same(renderer), reason: "a component reaches its object's others");

    world.add(knight);
    expect(knight.get<Damageable>(), same(armour));
    expect(knight.getAll<Component>(), [renderer, armour, isA<Health>()]);
  });

  test('a component belongs to one object, and an object has at most one of each engine kind', () {
    final health = Health(1);
    GameObject(components: [health]);
    expect(() => GameObject(components: [health]), throwsStateError);
    expect(() => GameObject(components: [Light.sun(), Light.ambient()]), throwsArgumentError);
    final lit = world.add(GameObject(components: [Light.sun()]));
    expect(() => lit.addComponent(Light.ambient()), throwsArgumentError);
    expect(() => lit.addComponent(health), throwsStateError);
    lit.addComponent(Health(2));
    lit.addComponent(Health(3));
    expect(lit.getAll<Health>().map((h) => h.hp), [2, 3], reason: 'any number of behaviours');
  });

  test('the world finds behaviours and components by type, and forgets those removed or destroyed', () {
    final a = world.add(GameObject(components: [Health(1)]));
    final b = world.add(GameObject(components: [Armour(2), Light.sun()]));
    expect(world.all<Health>().map((h) => h.hp), unorderedEquals([1, 2]));
    expect(world.all<Damageable>(), hasLength(2));
    expect(world.first<Armour>()!.hp, 2);
    expect(world.all<Light>(), [b.get<Light>()]);

    a.removeComponent(a.get<Health>()!);
    expect(world.all<Health>().map((h) => h.hp), [2]);
    b.destroy();
    expect(world.all<Health>(), isEmpty);
    expect(world.first<Light>(), isNull);
  });

  test('the world leaves out what is turned off, with its components, unless asked; a disabled behaviour is found', () {
    final pool = world.add(GameObject()..active = false);
    final waiting = pool.add(GameObject(components: [Health(1)]));
    final out = world.add(GameObject(components: [Health(2)]));
    expect(world.all<Health>().map((h) => h.hp), [2], reason: 'its parent is off');
    expect(world.only<Health>().hp, 2);
    expect(world.all<Health>(includeInactive: true).map((h) => h.hp), unorderedEquals([1, 2]));
    expect(() => world.only<Health>(includeInactive: true), throwsStateError);
    out.active = false;
    expect(world.first<Health>(), isNull);
    expect(world.first<Health>(includeInactive: true), isNotNull);
    pool.active = true;
    expect(world.only<Health>(), waiting.get<Health>());
    waiting.get<Health>()!.enabled = false;
    expect(world.all<Health>(), hasLength(1), reason: 'disabled, on an active object');
  });

  test("the engine's components are changed in place, and the engine checks each change", () {
    final eye = world.add(GameObject(components: [Camera(fovY: 50.deg)]));
    final camera = eye.get<Camera>()!;
    camera.fovY = 70.deg;
    expect(camera.fovY, 70.deg);
    expect(() => camera.fovY = 500.deg, throwsRefusal(FynkelStatus.invalidValue));
    expect(camera.fovY, 70.deg, reason: 'a refused change is undone');

    final box = world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, -5)),
        components: [cube()],
      ),
    );
    final ray = Ray(Vec3.zero, Vec3(0, 0, -1));
    expect(world.raycast(ray)?.gameObject, same(box));
    box.get<MeshRenderer>()!.mesh = null;
    expect(world.raycast(ray), isNull, reason: 'the engine has the change at once');
    box.get<MeshRenderer>()!.mesh = Mesh.cube();
    expect(world.raycast(ray)?.gameObject, same(box));
    box.removeComponent(box.get<MeshRenderer>()!);
    expect(world.raycast(ray), isNull);
    expect(box.has<MeshRenderer>(), isFalse);
    box.addComponent(cube());
    expect(world.raycast(ray)?.gameObject, same(box));
  });

  test('a behaviour added to a live object loads, starts the next frame and updates, without hiding it', () async {
    final load = Completer<void>();
    final object = world.add(Logged('o'));
    world.update(1 / 60);
    log.clear();

    object.addComponent(Logs('late', load: load.future));
    expect(log, ['load late']);
    expect(object.activeInHierarchy, isTrue);
    world.update(1 / 60);
    expect(log, ['load late', 'update o'], reason: 'not started while it loads');

    load.complete();
    await pumpEventQueue();
    log.clear();
    world.update(1 / 60);
    expect(log, ['start late', 'update o', 'update late']);
  });

  test("an object appears only when its behaviours' loads are done too", () async {
    final load = Completer<void>();
    final object = world.add(
      GameObject(
        components: [
          Logs('slow', load: load.future),
          cube(),
        ],
      ),
    );
    expect(object.activeInHierarchy, isFalse);
    load.complete();
    await object.loaded;
    expect(object.activeInHierarchy, isTrue);
  });

  test("a behaviour's failed load fails its object's load, or after that, removes only the behaviour", () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    final early = world.add(GameObject(components: [Logs('bad', load: Future.error('no'))]));
    await expectLater(early.loaded, throwsA('no'));
    expect(early.isDestroyed, isTrue);

    final live = world.add(GameObject());
    final bad = live.addComponent(Logs('bad later', load: Future.error('again')));
    await pumpEventQueue();
    expect(live.isDestroyed, isFalse);
    expect(live.has<Logs>(), isFalse);
    expect(bad.isRemoved, isTrue);
    expect(errors.map((e) => e.error), ['no', 'again']);
  });

  test('a disabled behaviour waits to start, and gets no updates or taps', () {
    final behaviour = Logs('b')..enabled = false;
    world.add(GameObject(components: [behaviour]));
    world.update(1 / 60);
    expect(log, ['load b']);
    behaviour.enabled = true;
    world.update(1 / 60);
    expect(log, ['load b', 'start b', 'update b']);
    log.clear();
    behaviour.enabled = false;
    world.update(1 / 60);
    expect(log, isEmpty);
  });

  test("taps reach an object's behaviours, which can take them, before its parent", () {
    world.add(GameObject(components: [Camera()]));
    final parent = world.add(GameObject(components: [Clicks('parent', takes: true)]));
    final child = parent.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, -5)),
        components: [cube(), Clicks('first'), Clicks('second', takes: true), Clicks('third')],
      ),
    );
    tap(world, centre);
    world.update(1 / 60);
    expect(log, ['tap first', 'tap second']);

    log.clear();
    child.get<Clicks>()!.gameObject.getAll<Clicks>()[1].enabled = false;
    tap(world, centre);
    world.update(1 / 60);
    expect(log, ['tap first', 'tap third', 'tap parent']);
  });

  test('removing a behaviour destroys it: no more calls, and it can be read but not used', () {
    final object = world.add(GameObject(components: [Logs('b')]));
    world.update(1 / 60);
    final b = object.get<Logs>()!;
    log.clear();
    object.removeComponent(b);
    expect(log, ['destroy b']);
    expect(b.isRemoved, isTrue);
    expect(b.gameObject, same(object), reason: 'still says where it was');
    world.update(1 / 60);
    expect(log, ['destroy b']);
    expect(() => object.removeComponent(b), throwsArgumentError);
  });

  test("a behaviour on the part of a model, which the game didn't add, is updated", () async {
    final truck = await Model.load(File('../../tests/assets/gltf/CesiumMilkTruck.glb').readAsBytesSync());
    addTearDown(truck.dispose);
    final part = world.add(truck.instantiate()).find('Wheels')!;
    expect(part.get<MeshRenderer>()?.mesh, isNotNull, reason: "the engine's renderer, read once");
    part.addComponent(Logs('wheel'));
    world.update(1 / 60);
    expect(log, ['load wheel', 'start wheel', 'update wheel']);
  });

  test("a behaviour on a model's part is enabled, and disabled with the model", () async {
    final truck = await Model.load(File('../../tests/assets/gltf/CesiumMilkTruck.glb').readAsBytesSync());
    addTearDown(truck.dispose);
    final model = world.add(truck.instantiate());
    final part = model.find('Wheels')!;
    final enables = part.addComponent(Enables());
    world.update(1 / 60);
    expect(enables.log, ['enable']);
    model.active = false;
    world.update(1 / 60);
    expect(enables.log, ['enable', 'disable']);
  });

  test('a behaviour put on an object while it loads holds the object back until its own load is done', () async {
    final objectLoad = Completer<void>(), behaviourLoad = Completer<void>();
    final object = world.add(GameObject(components: [Logs('first', load: objectLoad.future)]));
    object.addComponent(Logs('second', load: behaviourLoad.future));
    objectLoad.complete();
    await pumpEventQueue();
    expect(object.activeInHierarchy, isFalse);
    behaviourLoad.complete();
    await object.loaded;
    expect(object.activeInHierarchy, isTrue);
  });

  test('a behaviour removed during an update gets no update that frame', () {
    final b = Logs('b');
    world.add(GameObject(components: [Logs('a'), Remover(b), b]));
    world.update(1 / 60);
    expect(log, ['load a', 'load b', 'start a', 'start b', 'update a', 'destroy b']);
  });

  test('a component refused on a live object is not put on, and can go elsewhere', () {
    final object = world.add(GameObject());
    // Its values are checked where they're set (stage 14, C2): what's left
    // to refuse at the add is what it uses, such as a disposed material.
    final renderer = MeshRenderer(Mesh.cube(), Material()..dispose());
    expect(() => object.addComponent(renderer), throwsStateError);
    expect(object.has<MeshRenderer>(), isFalse);
    renderer.material = Material();
    world.add(GameObject(components: [renderer]));
    expect(world.first<MeshRenderer>(), same(renderer));
  });

  test('getInParent and getInChildren look at the object, then up or down', () {
    final root = world.add(
      GameObject(
        components: [Health(1)],
        children: [
          GameObject(
            name: 'a',
            children: [
              GameObject(name: 'b', components: [Armour(2)]),
            ],
          ),
        ],
      ),
    );
    final b = root.find('b')!;
    expect(b.getInParent<Armour>()!.hp, 2, reason: 'itself first');
    expect(root.find('a')!.getInParent<Health>()!.hp, 1);
    expect(root.getInChildren<Armour>()!.hp, 2);
    expect(root.getInChildren<Health>()!.hp, 1, reason: 'itself first');
    expect(root.find('a')!.getInChildren<Camera>(), isNull);
  });

  test("a destroyed object's components can still be found and read, not changed; one taken off is gone", () {
    final enemy = world.add(GameObject(components: [Health(3), Armour(1), Light.sun()]));
    final health = enemy.get<Health>()!, light = enemy.get<Light>()!;
    enemy.removeComponent(enemy.get<Armour>()!);
    enemy.destroy();
    expect(enemy.get<Health>(), same(health));
    expect(enemy.get<Health>()!.hp, 3);
    expect(enemy.getAll<Damageable>(), [health]);
    expect(enemy.get<Light>(), same(light));
    expect(enemy.has<Armour>(), isFalse, reason: 'taken off before');
    expect(health.isRemoved, isTrue);
    expect(() => light.intensity = 2, throwsStateError);
    world.update(1 / 60);
    expect(enemy.get<Health>(), same(health), reason: 'still there after the next frame tidies');
    expect(world.all<Health>(), isEmpty, reason: 'the world forgets it');
  });

  test('a behaviour finds what is above and below its object: ancestor, getInParent, getInChildren', () {
    final b = Logs('b');
    final level = world.add(
      Logged(
        'level',
        components: [Health(1)],
        children: [
          GameObject(
            name: 'a',
            components: [b],
            children: [
              GameObject(components: [Health(5)]),
            ],
          ),
        ],
      ),
    );
    expect(b.ancestor<Logged>(), same(level));
    expect(b.getInParent<Health>()!.hp, 1);
    expect(b.getInParent<Logs>(), same(b), reason: 'its own object first');
    expect(b.getInChildren<Health>()!.hp, 5);
    expect(b.ancestor<GameObject>(), same(level), reason: 'above, never its own object');
  });

  test("a behaviour's shortcuts reach its object, and touching the world before it's added says so", () {
    final health = Health(3);
    final object = GameObject(
      transform: Transform(position: Vec3(1, 2, 3)),
      components: [health],
    );
    expect(health.transform.position, Vec3(1, 2, 3));
    expect(() => health.world, throwsA(isA<StateError>().having((e) => e.message, 'message', contains('onLoad'))));
    world.add(object);
    expect(health.world, same(world));
    health.transform = Transform(position: Vec3(4, 5, 6));
    expect(object.transform.position, Vec3(4, 5, 6));
  });

  group('a behaviour as it goes', () {
    for (final (how, end) in [
      ('taken off', (GameObject o, Behaviour b) => o.removeComponent(b)),
      ('destroyed with its object', (GameObject o, Behaviour b) => o.destroy()),
    ]) {
      test("can't listen or start a routine in its onDestroy, $how", () {
        final signal = Signal<int>();
        final object = world.add(GameObject());
        final late = object.addComponent(LateStarter(signal));
        world.update(1 / 60);
        end(object, late);
        world.update(1 / 60);
        signal.emit(1);
        world.update(1 / 60);
        expect(late.refused, 2);
        expect(late.heard, 0);
        expect(late.ran, isFalse);
      });
    }
  });
}

class Enables extends Behaviour {
  final log = <String>[];

  @override
  void onEnable() {
    super.onEnable();
    log.add('enable');
  }

  @override
  void onDisable() {
    super.onDisable();
    log.add('disable');
  }
}

/// Tries to listen and start a routine as it goes.
class LateStarter extends Behaviour {
  LateStarter(this.signal);
  final Signal<int> signal;
  var heard = 0, refused = 0;
  var ran = false;

  @override
  void onDestroy() {
    super.onDestroy();
    try {
      on(signal, (_) => heard++);
    } on StateError {
      refused++;
    }
    try {
      startRoutine(() async => ran = true);
    } on StateError {
      refused++;
    }
  }
}
