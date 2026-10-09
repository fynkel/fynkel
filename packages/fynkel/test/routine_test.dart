// Routines (docs/stage-4.md §3.4, step 3).

import 'dart:async';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Everything that happened, in order.
final log = <String>[];

/// Logs its updates and late updates, so a routine's place in the frame
/// shows.
class Frames extends GameObject {
  Frames() : super(name: 'frames');

  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update');
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    log.add('late');
  }
}

/// Starts [body] in its onStart, to show where a wait made in the frame
/// ends.
class StartsIn extends GameObject {
  StartsIn(this.body);
  final Future<void> Function(StartsIn self) body;

  @override
  void onStart() {
    super.onStart();
    start(() => body(this));
  }
}

class Plain extends Behaviour {}

void main() {
  late World world;
  late GameObject object;
  setUp(() {
    world = World();
    object = world.add(Frames());
    log.clear();
  });
  tearDown(() => world.dispose());

  /// Runs [n] frames of [dt] seconds.
  void frames(int n, [double dt = 0.25]) {
    for (var i = 0; i < n; i++) {
      world.update(dt);
    }
  }

  test('a routine runs at once up to its first wait, then in the routines step: after updates, before late', () {
    object.start(() async {
      log.add('a');
      await nextFrame;
      log.add('b');
      await nextFrame;
      log.add('c');
    });
    expect(log, ['a']);
    frames(2);
    expect(log, ['a', 'update', 'b', 'late', 'update', 'c', 'late']);
  });

  test('a wait made during a frame ends in a later one, even one made in that frame\'s updates', () {
    world.add(
      StartsIn((self) async {
        log.add('started ${self.time.frame}');
        await nextFrame;
        log.add('next ${self.time.frame}');
        await wait(0);
        log.add('wait(0) ${self.time.frame}');
      }),
    );
    frames(3);
    expect(log.where((l) => !l.startsWith('update') && l != 'late'), ['started 1', 'next 2', 'wait(0) 3']);
  });

  test('routines go on in the order their waits were made, with what they schedule, inside the step', () {
    for (final name in ['a', 'b']) {
      object.start(() async {
        await nextFrame;
        await Future<void>.value(); // a finished future: a microtask
        await null;
        log.add(name);
      });
    }
    frames(1);
    expect(log, ['update', 'a', 'b', 'late']);
  });

  test('what a routine schedules runs before the next routine goes on', () {
    object.start(() async {
      await nextFrame;
      await null;
      log.add('a');
    });
    object.start(() async {
      await nextFrame;
      log.add('b');
    });
    frames(1);
    expect(log, ['update', 'a', 'b', 'late']);
  });

  test('wait counts game time: scale slows it, 0 stops it; unscaled counts real time', () {
    final scaled = object.start(() async => wait(1));
    final real = object.start(() async => wait(1, unscaled: true));
    world.time.scale = 0.5;
    frames(4); // 1 s real, 0.5 s game
    expect((scaled.isRunning, real.isRunning), (true, false));
    world.time.scale = 0;
    frames(8);
    expect(scaled.isRunning, isTrue);
    world.time.scale = 1;
    frames(2);
    expect(scaled.isRunning, isFalse);
  });

  test('until ends when its test is true, whileTrue when it turns false; both from the next frame', () {
    var open = false;
    object.start(() async {
      await until(() => open);
      log.add('open');
      await whileTrue(() => open);
      log.add('closed');
    });
    Iterable<String> mine() => log.where((l) => l != 'update' && l != 'late');
    frames(2);
    open = true;
    frames(1);
    expect(mine(), ['open']);
    frames(2);
    open = false;
    frames(1);
    expect(mine(), ['open', 'closed']);
  });

  test('a routine gives back what it returns, to a routine awaiting it in the same step, or to anyone', () async {
    Future<int> answer() async {
      await wait(0.5);
      return 42;
    }

    final asked = object.start(answer);
    final outer = object.start(() async {
      final got = await object.start(answer);
      log.add('got $got');
      return got + 1;
    });
    frames(2);
    expect(log, ['update', 'late', 'update', 'got 42', 'late']);
    expect(await asked, 42);
    expect(await outer.done, 43);
  });

  test('stop unwinds a routine through its finally at once, and nothing of it runs after', () async {
    final routine = object.start(() async {
      try {
        await wait(10);
        log.add('never');
      } finally {
        log.add('finally');
      }
    });
    frames(1);
    routine.stop();
    expect(log, ['update', 'late', 'finally']);
    expect((routine.isRunning, routine.isStopped), (false, true));
    await expectLater(routine.done, throwsA(isA<RoutineStopped>()));
    frames(50);
    expect(log.where((l) => l == 'never'), isEmpty);
    routine.stop();
  });

  test('a routine stops with its object, and a behaviour\'s with the behaviour, finally first', () {
    final behaviour = object.addComponent(Plain());
    object.start(() async {
      try {
        await nextFrame;
        await wait(10);
      } finally {
        log.add('object finally');
      }
    });
    final b = behaviour.start(() async {
      try {
        await wait(10);
      } finally {
        log.add('behaviour finally');
      }
    });
    frames(1);
    log.clear();
    object.removeComponent(behaviour);
    expect(log, ['behaviour finally']);
    expect(b.isStopped, isTrue);
    final other = object.addComponent(Plain());
    other.start(() async {
      try {
        await wait(10);
      } finally {
        log.add('other finally');
      }
    });
    object.destroy();
    expect(log, ['behaviour finally', 'other finally', 'object finally']);
  });

  test('no routine code runs after its object is destroyed, even when an outside future completes later', () async {
    final outside = Completer<int>();
    final failing = Completer<int>();
    object.start(() async {
      final n = await outside.future;
      log.add('after outside $n');
    });
    object.start(() async {
      try {
        await failing.future;
      } catch (_) {
        log.add('caught');
      }
    });
    object.start(() async {
      unawaited(outside.future.then((n) => log.add('then $n')));
      await wait(10);
    });
    object.destroy();
    outside.complete(1);
    failing.completeError(StateError('late'));
    await pumpEventQueue();
    expect(log, isEmpty);
  });

  test('a routine pauses while its object is hidden, and goes on where it was', () {
    final routine = object.start(() async {
      await wait(1);
      log.add('done');
    });
    frames(2); // 0.5 s
    object.active = false;
    frames(10);
    expect(routine.isRunning, isTrue);
    object.active = true;
    frames(1);
    expect(routine.isRunning, isTrue, reason: '0.75 s of its 1 s');
    frames(1);
    expect(log, contains('done'));
  });

  test('a behaviour\'s routine pauses while it is disabled', () {
    final behaviour = object.addComponent(Plain());
    final routine = behaviour.start(() async => nextFrame);
    behaviour.enabled = false;
    frames(3);
    expect(routine.isRunning, isTrue);
    behaviour.enabled = true;
    frames(1);
    expect(routine.isRunning, isFalse);
  });

  test('routines started in a routine stop with it; one that ends lets them go on', () {
    late Routine<void> inner;
    late Routine<void> kept;
    final outer = object.start(() async {
      inner = object.start(() async {
        try {
          await wait(10);
        } finally {
          log.add('inner finally');
        }
      });
      try {
        await wait(10);
      } finally {
        log.add('outer finally');
      }
    });
    object.start(() async {
      kept = object.start(() async => wait(10));
    });
    frames(1);
    outer.stop();
    expect(log.where((l) => l.contains('finally')), ['inner finally', 'outer finally']);
    expect(inner.isStopped, isTrue);
    expect(kept.isRunning, isTrue);
  });

  test('stopping a routine that awaits its own routines (in a race) stops them, then it, once', () async {
    final outer = object.start(() async {
      try {
        await race([object.start(() async => wait(5)), object.start(() async => wait(5))]);
      } finally {
        log.add('outer finally');
      }
    });
    frames(1);
    outer.stop();
    expect(log.where((l) => l.contains('finally')), ['outer finally']);
    expect(outer.isStopped, isTrue);
    await expectLater(outer.done, throwsA(isA<RoutineStopped>()));
  });

  test('race gives the first to end and stops the rest; all gives each, in order', () {
    final loser = object.start(() async {
      try {
        await wait(5);
      } finally {
        log.add('loser finally');
      }
      return 'loser';
    });
    object.start(() async {
      final first = await race([wait(1).then((_) => 'timer'), loser]);
      log.add('race $first');
      final each = await all([object.start(() async => 1), Future.value(2), wait(0.5).then((_) => 3)]);
      log.add('all $each');
    });
    frames(4);
    expect(log.where((l) => l != 'update' && l != 'late'), ['loser finally', 'race timer']);
    expect(loser.isStopped, isTrue);
    frames(2);
    expect(log.last == 'late' ? log[log.length - 2] : log.last, 'all [1, 2, 3]');
  });

  test('race stops a losing routine and drops a losing wait, which then wakes nothing', () {
    late Routine<String> slow;
    var checks = 0;
    object.start(() async {
      slow = object.start(() async {
        await wait(5);
        return 'slow';
      });
      log.add(await race([slow, until(() => world.time.frame == 3).then((_) => 'until')]));
      await race([
        wait(1),
        until(() {
          checks++;
          return false;
        }),
      ]);
      log.add('wait');
    });
    frames(3);
    expect(log.where((l) => l != 'update' && l != 'late'), ['until']);
    expect(slow.isStopped, isTrue);
    frames(30);
    expect(log.where((l) => l != 'update' && l != 'late'), ['until', 'wait']);
    expect(checks, 3, reason: 'checked in frames 4 to 6; frame 7 ended the wait first, and then it was dropped');
  });

  test('all with a failure stops the rest and throws it', () async {
    late Routine<int> other;
    final routine = object.start(() async {
      other = object.start(() async {
        await wait(10);
        return 1;
      });
      return all([other, object.start<int>(() async => throw StateError('no'))]);
    });
    frames(1);
    await expectLater(routine.done, throwsStateError);
    expect(other.isStopped, isTrue);
  });

  test('an error a routine doesn\'t catch goes to world.errors, unless it is awaited; try catches one', () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    object.start(() async {
      await nextFrame;
      throw StateError('alone');
    });
    object.start(() async {
      try {
        await object.start(() async {
          await nextFrame;
          throw StateError('awaited');
        });
      } on StateError catch (e) {
        log.add('caught ${e.message}');
      }
    });
    object.start(() async {
      try {
        await Future<void>.error(StateError('outside'));
      } on StateError catch (e) {
        log.add('caught ${e.message}');
      }
    });
    frames(1);
    await pumpEventQueue();
    expect(errors.map((e) => (e.object, (e.error as StateError).message)), [(object, 'alone')]);
    expect(log, containsAll(['caught awaited', 'caught outside']));
  });

  test('a routine that catches its stop gets stopped again at its next wait', () {
    final routine = object.start(() async {
      try {
        await wait(10);
      } on RoutineStopped {
        log.add('caught stop');
      }
      try {
        await nextFrame;
      } on RoutineStopped {
        log.add('stopped again');
      }
    });
    routine.stop();
    expect(log, ['caught stop', 'stopped again']);
  });

  test('a routine that stops itself is stopped at its next wait, at once', () {
    late Routine<void> routine;
    routine = object.start(() async {
      await nextFrame;
      routine.stop();
      try {
        await nextFrame;
      } on RoutineStopped {
        log.add('stopped at once');
      }
    });
    frames(1);
    expect(log, ['update', 'stopped at once', 'late']);
  });

  test('waits work only inside a routine, and start only on a live object', () {
    expect(() => nextFrame, throwsStateError);
    expect(() => wait(1), throwsStateError);
    expect(() => GameObject().start(() async {}), throwsStateError);
    final gone = world.add(GameObject())..destroy();
    expect(() => gone.start(() async {}), throwsStateError);
    expect(object.start(() async => wait(-1)).done, throwsArgumentError);
  });
}
