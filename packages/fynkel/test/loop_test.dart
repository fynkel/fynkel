// The loop and its order (docs/stage-3.md §3.2, step 3).

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'tapping.dart';

/// Every call any object gets, in order.
final log = <String>[];

/// Logs its starts, updates and destroys, and runs [onEachUpdate].
class Logged extends GameObject {
  Logged(String name, {super.transform, super.components, this.onEachUpdate, this.onStarting}) : super(name: name);

  final void Function(Logged self)? onEachUpdate;
  final void Function()? onStarting;

  @override
  void onStart() {
    super.onStart();
    log.add('start $name');
    onStarting?.call();
  }

  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update $name');
    onEachUpdate?.call(this);
  }

  @override
  void onDestroy() {
    super.onDestroy();
    log.add('destroy $name');
  }
}

/// Something that is part of an object: a mixin.
mixin Spins on GameObject {
  var turns = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    turns++;
  }
}

mixin Ages on GameObject {
  var age = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    age += time.delta;
  }
}

class Windmill extends GameObject with Spins, Ages {}

/// A behaviour that logs its starts and updates as its object's name.
class LoggedBehaviour extends Behaviour {
  @override
  void onStart() {
    super.onStart();
    log.add('start ${gameObject.name} behaviour');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update ${gameObject.name} behaviour');
  }
}

/// Takes taps, or passes them on, and logs both.
class Tappable extends GameObject {
  Tappable(String name, {this.takes = false, super.transform, super.components}) : super(name: name);

  final bool takes;
  final taps = <Finger>[];

  @override
  bool onClick(Finger finger) {
    taps.add(finger);
    log.add('tap $name');
    return takes;
  }
}

MeshRenderer cube() => MeshRenderer(Mesh.cube(), Material());

/// A camera at the origin looking down -Z, in a 100 × 100 view.
GameObject camera() => GameObject(components: [Camera()]);
const centre = ViewPoint(50, 50, width: 100, height: 100);

void main() {
  late World world;
  setUp(() {
    world = World();
    log.clear();
  });
  tearDown(() => world.dispose());

  test('starts come once, before the first update, parents first; updates parents first in the order added', () {
    final a = world.add(Logged('a'));
    a.add(Logged('a1')).add(Logged('a1x'));
    a.add(Logged('a2'));
    world.add(Logged('b'));
    expect(log, isEmpty, reason: 'nothing runs until the world updates');

    world.update(1 / 60);
    expect(log, [
      'start a', 'start a1', 'start a1x', 'start a2', 'start b', //
      'update a', 'update a1', 'update a1x', 'update a2', 'update b',
    ]);
    log.clear();
    world.update(1 / 60);
    expect(log, ['update a', 'update a1', 'update a1x', 'update a2', 'update b'], reason: 'no second start');
  });

  test('an object added during an update starts next frame; one added during a start starts at once', () {
    var spawned = false;
    world.add(
      Logged(
        'spawner',
        onEachUpdate: (self) {
          if (spawned) return;
          spawned = true;
          self.add(Logged('late'));
        },
      ),
    );
    world.update(0.1);
    expect(log, ['start spawner', 'update spawner'], reason: 'late is live, but waits for the next frame');
    expect(world.first<Logged>()!.find('late')!.isAlive, isTrue);
    log.clear();
    world.update(0.1);
    expect(log, ['start late', 'update spawner', 'update late']);

    log.clear();
    world.add(_StartsAChild());
    world.update(0.1);
    expect(log, ['start parent', 'start child', 'update spawner', 'update late', 'update child']);
  });

  test('an object destroyed during an update gets no more calls, even this frame', () {
    late Logged b;
    world.add(Logged('a', onEachUpdate: (_) => b.destroy()));
    b = world.add(Logged('b'));
    b.add(Logged('b1'));
    world.add(Logged('c'));
    world.update(0.1);
    expect(log, [
      'start a', 'start b', 'start b1', 'start c', //
      'update a', 'destroy b1', 'destroy b', 'update c',
    ]);
    log.clear();
    world.update(0.1);
    expect(log, ['update a', 'update c']);
  });

  test('onDestroy comes children first, while each is still there to read, and only once', () {
    final where = <String, Vec3>{};
    final root = world.add(_RemembersWhere('root', where, Transform(position: Vec3(1, 0, 0))));
    root.add(_RemembersWhere('child', where, Transform(position: Vec3(0, 2, 0))));
    root.destroy();
    root.destroy();
    expect(log, ['destroy child', 'destroy root']);
    expect(where, {'child': Vec3(1, 2, 0), 'root': Vec3(1, 0, 0)});
    expect(world.all<GameObject>(), isEmpty);
  });

  test(
    "an object destroyed before its first frame never starts, and an object can't be added to a dying one",
    () async {
      final gone = world.add(Logged('gone'));
      gone.destroy();
      world.update(0.1);
      expect(log, ['destroy gone']);

      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final dying = world.add(_AddsWhileDying());
      dying.destroy();
      await pumpEventQueue();
      expect(errors.single.gameObject, dying);
      expect(errors.single.error, isA<StateError>());
      expect(dying.isDestroyed, isTrue);
      expect(dying.isAlive, isFalse, reason: 'it goes even when its onDestroy throws');
    },
  );

  test("two mixins' updates both run, in order, with the class's own", () {
    final windmill = world.add(Windmill());
    world
      ..update(0.25)
      ..update(0.25);
    expect(windmill.turns, 2);
    expect(windmill.age, 0.5);
  });

  test('a tap goes to the object hit, then up through its parents until one takes it', () {
    world.add(camera());
    final level = world.add(Tappable('level', takes: true));
    final robot = level.add(Tappable('robot', transform: Transform(position: Vec3(0, 0, -5))));
    final arm = robot.add(Tappable('arm', components: [cube()]));
    tap(world, centre);
    world.update(0.1);
    expect(log, ['tap arm', 'tap robot', 'tap level']);
    expect(robot.taps.single.over!.gameObject, same(arm), reason: 'it learns which part was hit');
    expect(robot.taps.single.position, centre);
    expect(robot.taps.single.over!.point.z, closeTo(-4.5, 1e-5));
    expect(robot.taps.single.over!.distance, closeTo(4.4, 1e-5), reason: 'from the near plane, 0.1 ahead');

    log.clear();
    tap(world, const ViewPoint(0, 0, width: 100, height: 100));
    world.update(0.1);
    expect(log, isEmpty, reason: 'a tap on nothing goes nowhere');
    tap(world, centre);
    tap(world, centre);
    world.update(0.1);
    expect(log, hasLength(6), reason: 'every tap of the frame');
  });

  test('a tap stops where it is taken; one that destroys its object still goes on up', () {
    world.add(camera());
    final level = world.add(Tappable('level'));
    final taker = level.add(Tappable('taker', takes: true, transform: Transform(position: Vec3(0, 0, -5))));
    taker.add(Tappable('arm', components: [cube()]));
    tap(world, centre);
    world.update(0.1);
    expect(log, ['tap arm', 'tap taker']);

    log.clear();
    taker.destroy();
    final quitter = level.add(
      _DestroysOnTap(
        transform: Transform(position: Vec3(0, 0, -5)),
        components: [cube()],
      ),
    );
    tap(world, centre);
    world.update(0.1);
    expect(log, ['tap quitter', 'tap level'], reason: 'only taking it stops it');
    expect(quitter.isAlive, isFalse);
  });

  test('a tap is cast from the camera the view draws from, and needs one', () {
    world.add(
      Tappable(
        'box',
        components: [cube()],
        transform: Transform(position: Vec3(0, 0, -5)),
      ),
    );
    tap(world, centre);
    world.update(0.1);
    expect(log, isEmpty, reason: 'no camera, no tap');
    // A hidden camera first, looking away; the one drawn from looks at the box.
    world.add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(0, 0, 1))),
        components: [Camera()],
      )..active = false,
    );
    world.add(camera());
    tap(world, centre);
    world.update(0.1);
    expect(log, ['tap box']);
  });

  test('hidden objects are not started, updated or tapped, and start when first shown', () {
    world.add(camera());
    final off = world.add(Logged('off')..active = false);
    off.add(
      Logged(
        'below',
        components: [cube()],
        transform: Transform(position: Vec3(0, 0, -5)),
      ),
    );
    final on = world.add(Logged('on'));
    tap(world, centre);
    world.update(0.1);
    expect(log, ['start on', 'update on']);

    log.clear();
    off.active = true;
    world.update(0.1);
    expect(log, ['start off', 'start below', 'update off', 'update below', 'update on']);

    log.clear();
    on.active = false;
    off.find('below')!.active = false;
    world.update(0.1);
    expect(log, ['update off']);
  });

  test('an object shown during an update starts next frame, never updating before it starts', () {
    final hidden = Logged('hidden')..active = false;
    world.add(Logged('switch', onEachUpdate: (_) => hidden.active = true));
    world.add(hidden);
    world.update(0.1);
    expect(log, ['start switch', 'update switch']);
    log.clear();
    world.update(0.1);
    expect(log, ['start hidden', 'update switch', 'update hidden']);
  });

  test('destroying an inactive object, or one below it, leaves the rest running', () {
    final off = world.add(Logged('off')..active = false);
    final also = world.add(Logged('also')..active = false);
    also.add(Logged('below'));
    world.add(Logged('on'));
    off.destroy();
    also.find('below')!.destroy();
    world.update(0.1);
    expect(log, ['destroy off', 'destroy below', 'start on', 'update on']);
    also.destroy();
    world.update(0.1);
    expect(log.last, 'update on');
  });

  test('a new parent counts from the next frame: the moved object and what is below it update after it', () {
    final a = world.add(Logged('a'));
    final moved = a.add(Logged('moved'));
    moved.add(Logged('below'));
    final b = world.add(Logged('b'));
    world.update(0.1);
    log.clear();

    moved.parent = b;
    world.update(0.1);
    expect(log, ['update a', 'update b', 'update moved', 'update below']);
  });

  test('plain objects, with nothing of the game\'s to call, cost the updates nothing (DC4)', () {
    world.time.fixedDelta = 1 / 60;
    for (var i = 0; i < 20000; i++) {
      world.add(GameObject(components: [cube()]));
    }
    world.add(Logged('one'));
    final calls = <int>[];
    for (var i = 0; i < 9; i++) {
      world.update(1 / 60);
      final p = world.profile;
      calls.add((p.updates + p.lateUpdates + p.fixedUpdates).inMicroseconds);
    }
    calls.sort();
    // Visited, they were 0.3 ms on a Mac, three walks over 20,000.
    expect(calls[4], lessThan(50), reason: '${world.profile}');
    expect(log.where((l) => l == 'update one'), hasLength(9));
  });

  test('a plain object given a behaviour updates in its place: after its parent, before its children', () {
    final top = world.add(GameObject(name: 'top'));
    final middle = top.add(GameObject(name: 'middle'));
    middle.add(Logged('child'));
    world.update(0.1);
    log.clear();

    middle.addComponent(LoggedBehaviour());
    top.addComponent(LoggedBehaviour());
    world.update(0.1);
    // Started in the order they were put on; updated in the objects'.
    expect(log, [
      'start middle behaviour',
      'start top behaviour',
      'update top behaviour',
      'update middle behaviour',
      'update child',
    ]);
  });

  test('a behaviour put on a plain object during a start starts and updates that frame', () {
    final plain = world.add(GameObject(name: 'plain'));
    world.update(0.1);
    log.clear();
    world.add(Logged('adder', onStarting: () => plain.addComponent(LoggedBehaviour())));
    world.update(0.1);
    expect(log, ['start adder', 'start plain behaviour', 'update plain behaviour', 'update adder']);
  });

  test('a dispose from inside an update waits for it: the phases left are skipped, then the world is freed', () {
    final world = World();
    final quitter = world.add(_Quits());
    final after = world.add(_Counts());
    world.update(1 / 60);
    expect(world.isDisposed, isFalse);
    quitter.quit = true;
    world.update(1 / 60); // must not throw
    expect(world.isDisposed, isTrue);
    expect(after.updates, 2, reason: 'the update phase ends, then stops');
    expect(after.lates, 1, reason: 'no late update after the dispose');
  });

  test('an update from inside an update is refused, and the frame goes on', () async {
    final world = World();
    addTearDown(world.dispose);
    final errors = <Object>[];
    world.errors.listen((e) => errors.add(e.error));
    final nested = world.add(_UpdatesAgain());
    world.update(1 / 60);
    world.update(1 / 60);
    await pumpEventQueue();
    expect(nested.updates, 2);
    expect(errors, everyElement(isA<StateError>()));
    expect(errors, hasLength(2));
  });

  test('every object reads the same input', () {
    final reader = world.add(_ReadsInput());
    world.input.simulate.keyDown(KeyCode.w);
    world.update(0.1);
    expect(reader.seen, same(world.input));
    expect(reader.wasDown, isTrue, reason: 'the key went down in the input step, before the updates');
  });
}

class _StartsAChild extends GameObject {
  _StartsAChild() : super(name: 'parent');

  @override
  void onStart() {
    super.onStart();
    log.add('start parent');
    add(Logged('child'));
  }
}

class _RemembersWhere extends GameObject {
  _RemembersWhere(String name, this.where, Transform transform) : super(name: name, transform: transform);

  final Map<String, Vec3> where;

  @override
  void onDestroy() {
    super.onDestroy();
    log.add('destroy $name');
    where[name!] = worldTransform.position;
  }
}

class _AddsWhileDying extends GameObject {
  @override
  void onDestroy() {
    super.onDestroy();
    add(GameObject());
  }
}

class _DestroysOnTap extends Tappable {
  _DestroysOnTap({super.transform, super.components}) : super('quitter');

  @override
  bool onClick(Finger finger) {
    super.onClick(finger);
    destroy();
    return false;
  }
}

class _ReadsInput extends GameObject {
  Input? seen;
  bool? wasDown;

  @override
  void onUpdate() {
    super.onUpdate();
    seen = input;
    wasDown = input.isPressed(KeyCode.w);
  }
}

class _Quits extends GameObject {
  var quit = false;

  @override
  void onUpdate() {
    super.onUpdate();
    if (quit) world.dispose();
  }
}

class _UpdatesAgain extends GameObject {
  var updates = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    updates++;
    world.update(1 / 60);
  }
}

class _Counts extends GameObject {
  var updates = 0, lates = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    updates++;
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    lates++;
  }
}
