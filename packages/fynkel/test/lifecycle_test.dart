// The lifecycle, time and errors (docs/stage-4.md §3.2, §3.3, §3.9, step 2).

import 'dart:async';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'tapping.dart';

/// Every call any object or behaviour gets, in order.
final log = <String>[];

class Logged extends GameObject {
  Logged(String name, {super.components, super.children}) : super(name: name);

  @override
  void onEnable() {
    super.onEnable();
    log.add('enable $name');
  }

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
  void onLateUpdate() {
    super.onLateUpdate();
    log.add('late $name');
  }

  @override
  void onDisable() {
    super.onDisable();
    log.add('disable $name');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    log.add('destroy $name');
  }
}

class Logs extends Behaviour {
  Logs(this.label, {this.load});

  final String label;
  final Future<void>? load;

  @override
  FutureOr<void> onLoad() {
    super.onLoad();
    return load;
  }

  @override
  void onEnable() {
    super.onEnable();
    log.add('enable $label');
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
  void onLateUpdate() {
    super.onLateUpdate();
    log.add('late $label');
  }

  @override
  void onDisable() {
    super.onDisable();
    log.add('disable $label');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    log.add('destroy $label');
  }
}

/// Destroys itself from [where], and logs every call it gets, as do its
/// behaviour and child.
class DestroysIn extends Logged {
  DestroysIn(this.where) : super('self', components: [Logs('mine')], children: [Logged('child')]);
  final String where;

  @override
  void onEnable() {
    super.onEnable();
    if (where == 'enable') destroy();
  }

  @override
  void onStart() {
    super.onStart();
    if (where == 'start') destroy();
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (where == 'update') destroy();
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    if (where == 'late') destroy();
  }

  @override
  void onDisable() {
    super.onDisable();
    if (where == 'disable') destroy();
  }
}

/// Throws from [where].
class Throws extends GameObject {
  Throws(this.where) : super(name: 'throws');
  final String where;

  @override
  void onEnable() {
    super.onEnable();
    if (where == 'enable') throw StateError('in enable');
  }

  @override
  void onStart() {
    super.onStart();
    if (where == 'start') throw StateError('in start');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (where == 'update') throw StateError('in update');
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    if (where == 'late') throw StateError('in late');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    if (where == 'destroy') throw StateError('in destroy');
  }
}

/// Throws from its onTap.
class TapThrows extends GameObject {
  TapThrows({super.transform, super.components});

  @override
  bool onClick(Finger finger) => throw StateError('in tap');
}

/// Throws from its onTap, or logs it.
class TapBehaviour extends Behaviour {
  TapBehaviour(this.label, {this.throws = false});
  final String label;
  final bool throws;

  @override
  bool onClick(Finger finger) {
    if (throws) throw StateError('in $label');
    log.add('tap $label');
    return true;
  }
}

/// Remembers what [time] said in its updates.
class Clock extends Behaviour {
  final deltas = <double>[];

  @override
  void onUpdate() {
    super.onUpdate();
    deltas.add(time.delta);
  }
}

void main() {
  late World world;
  setUp(() {
    world = World();
    log.clear();
  });
  tearDown(() => world.dispose());

  test('a frame: enables when it appears, starts, updates, then late updates after every update', () {
    world.add(Logged('a', components: [Logs('a1')], children: [Logged('b')]));
    expect(log, ['enable a', 'enable a1', 'enable b'], reason: 'parents first, as soon as it appears');
    log.clear();
    world.update(1 / 60);
    expect(log, [
      'start a', 'start a1', 'start b', //
      'update a', 'update a1', 'update b',
      'late a', 'late a1', 'late b',
    ]);
  });

  test('turning an object off disables it and what is below, parents first; on again enables them', () {
    final a = world.add(Logged('a', components: [Logs('a1')], children: [Logged('b')]));
    final b = a.find('b')!;
    world.update(1 / 60);
    log.clear();

    a.active = false;
    expect(log, ['disable a', 'disable a1', 'disable b']);
    world.update(1 / 60);
    expect(log, ['disable a', 'disable a1', 'disable b'], reason: 'no frames while off');

    log.clear();
    b.active = false;
    expect(log, isEmpty, reason: 'b was already off by its parent');
    a.active = true;
    expect(log, ['enable a', 'enable a1'], reason: 'b keeps its own flag');
  });

  test("a behaviour's enabled flag calls onEnable and onDisable while its object is shown", () {
    final object = world.add(GameObject(components: [Logs('b')]));
    final b = object.get<Logs>()!;
    log.clear();
    b.enabled = false;
    b.enabled = false;
    expect(log, ['disable b']);
    object.active = false;
    b.enabled = true;
    expect(log, ['disable b'], reason: 'its object is hidden');
    object.active = true;
    expect(log, ['disable b', 'enable b']);
  });

  test('an object that loads is enabled when it appears, with everything added during its load', () async {
    final load = Completer<void>();
    world.add(
      Logged(
        'a',
        components: [Logs('a1', load: load.future)],
        children: [Logged('b')],
      ),
    );
    expect(log, isEmpty);
    load.complete();
    await pumpEventQueue();
    expect(log, ['enable a', 'enable a1', 'enable b']);
  });

  test('a behaviour put on a shown object is enabled when its own load is done', () async {
    final object = world.add(GameObject());
    object.addComponent(Logs('now'));
    expect(log, ['enable now']);
    final load = Completer<void>();
    object.addComponent(Logs('later', load: load.future));
    expect(log, ['enable now']);
    load.complete();
    await pumpEventQueue();
    expect(log, ['enable now', 'enable later']);

    final removed = Completer<void>();
    final gone = object.addComponent(Logs('gone', load: removed.future));
    object.removeComponent(gone);
    removed.complete();
    await pumpEventQueue();
    expect(log, ['enable now', 'enable later', 'destroy gone'], reason: 'removed before its load was done');
  });

  test('destroying disables what is enabled, then destroys, children first', () {
    final a = world.add(Logged('a', components: [Logs('a1')], children: [Logged('b'), Logged('off')]));
    a.find('off')!.active = false;
    log.clear();
    a.destroy();
    expect(log, [
      'destroy off', //
      'disable b', 'destroy b',
      'disable a', 'disable a1', 'destroy a', 'destroy a1',
    ]);
  });

  test('removing a behaviour disables it, then destroys it', () {
    final object = world.add(GameObject(components: [Logs('b')]));
    log.clear();
    object.removeComponent(object.get<Logs>()!);
    expect(log, ['disable b', 'destroy b']);
  });

  test('moving an object below a hidden one disables it, and back out enables it', () {
    final hidden = world.add(Logged('hidden')..active = false);
    final moved = world.add(Logged('moved'));
    log.clear();
    moved.parent = hidden;
    expect(log, ['disable moved']);
    moved.parent = null;
    expect(log, ['disable moved', 'enable moved']);
  });

  test('time: delta follows the frame, scale and maxDelta; unscaled time does not follow scale', () {
    final clock = world.add(GameObject(components: [Clock()])).get<Clock>()!;
    expect(world.time.frame, 0);
    world.update(0.01);
    world.time.scale = 0.5;
    world.update(0.02);
    world.time.scale = 0;
    world.update(0.02);
    world.time.scale = 1;
    world.update(5);
    expect(clock.deltas, [0.01, 0.01, 0, closeTo(1 / 3, 1e-9)], reason: 'a 5 s hitch counts as maxDelta');
    expect(world.time.unscaledDelta, closeTo(1 / 3, 1e-9));
    expect(world.time.sinceStart, closeTo(0.01 + 0.01 + 1 / 3, 1e-9));
    expect(world.time.unscaledSinceStart, closeTo(0.01 + 0.02 + 0.02 + 1 / 3, 1e-9));
    expect(world.time.frame, 4);
    expect(clock.time, same(world.time));
    expect(() => world.time.scale = -1, throwsArgumentError);
    expect(() => world.time.maxDelta = 0, throwsArgumentError);
    world.time.maxDelta = 0.1;
    world.update(1);
    expect(clock.deltas.last, closeTo(0.1, 1e-9));
    expect(() => world.update(-0.01), throwsArgumentError);
    expect(() => world.update(double.nan), throwsArgumentError);
    expect(world.time.frame, 5, reason: 'a refused frame counts for nothing');
  });

  // From the call that destroys it on, everything the object, its behaviour
  // and its child get: what was enabled is disabled, then all are
  // destroyed, children first, and nothing after.
  const destroyed = ['destroy child', 'destroy self', 'destroy mine'];
  final fromThere = {
    'enable': ['enable self', 'destroy child', 'disable self', 'destroy self', 'destroy mine'],
    'start': ['start self', 'disable child', 'destroy child', 'disable self', 'disable mine', ...destroyed.skip(1)],
    'update': ['update self', 'disable child', 'destroy child', 'disable self', 'disable mine', ...destroyed.skip(1)],
    'late': ['late self', 'disable child', 'destroy child', 'disable self', 'disable mine', ...destroyed.skip(1)],
    'disable': ['disable self', 'disable child', 'destroy child', 'disable mine', ...destroyed.skip(1)],
  };
  for (final MapEntry(key: where, value: expected) in fromThere.entries) {
    test('destroying itself in on$where ends it there, and the others go on', () {
      final self = world.add(DestroysIn(where));
      final other = world.add(Logged('other'));
      if (where == 'disable') self.active = false;
      world.update(1 / 60);
      final mine = log.where((line) => !line.endsWith('other')).toList();
      expect(mine.sublist(mine.indexOf(expected.first)), expected);
      expect(log, containsAllInOrder(['start other', 'update other', 'late other']));
      expect(self.isDestroyed, isTrue);
      expect(other.isDestroyed, isFalse);
    });
  }

  for (final where in ['enable', 'start', 'update', 'late', 'destroy']) {
    test("an error in one object's on${where[0].toUpperCase()}${where.substring(1)} goes to world.errors, "
        'and the rest go on', () async {
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final thrower = world.add(Throws(where));
      final other = world.add(Logged('other'));
      world.update(1 / 60);
      thrower.destroy();
      other.destroy();
      await pumpEventQueue();
      expect(errors.map((e) => (e.object, (e.error as StateError).message)), [(thrower, 'in $where')]);
      expect(log, containsAllInOrder(['start other', 'update other', 'late other', 'destroy other']));
      expect(thrower.isDestroyed, isTrue);
    });
  }

  test("an error in an onTap goes to world.errors, and the tap goes on as if it wasn't taken", () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    world.add(GameObject(components: [Camera()]));
    final parent = world.add(GameObject(components: [TapBehaviour('parent')]));
    final hit = parent.add(
      TapThrows(
        transform: Transform(position: Vec3(0, 0, -5)),
        components: [MeshRenderer(Mesh.cube(), Material()), TapBehaviour('mine', throws: true)],
      ),
    );
    tap(world, const ViewPoint(50, 50, width: 100, height: 100));
    world.update(1 / 60);
    await pumpEventQueue();
    expect(errors.map((e) => (e.object, (e.error as StateError).message)), [(hit, 'in tap'), (hit, 'in mine')]);
    expect(log, ['tap parent']);
  });

  test('a behaviour still loading is not enabled when its object is turned off and on', () async {
    final object = world.add(GameObject());
    final load = Completer<void>();
    object.addComponent(Logs('slow', load: load.future));
    object.active = false;
    object.active = true;
    expect(log, isEmpty);
    load.complete();
    await pumpEventQueue();
    expect(log, ['enable slow']);
  });

  test('with no one listening, an error is uncaught in the zone that called update', () {
    final uncaught = <Object>[];
    runZonedGuarded(() {
      world.add(Throws('update'));
      world.update(1 / 60);
    }, (error, _) => uncaught.add(error));
    expect(uncaught, [isA<StateError>()]);
  });
}
