import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

class _Script extends Behaviour {}

class _LazyRoutine extends Iterable<Object?> {
  _LazyRoutine(this.onIterator);
  final void Function() onIterator;
  @override
  Iterator<Object?> get iterator {
    onIterator();
    return <Object?>[null].iterator;
  }
}

void main() {
  late World world;
  setUp(() => world = World.create());
  tearDown(() => world.dispose());
  _Script attach() => world.edit(() => world.spawn().addBehaviour(_Script()));

  test('staged schedules execute no user iterator or callback before the scheduler phase', () {
    final log = <String>[];
    late Coroutine routine;
    world.edit(() {
      final b = world.spawn().addBehaviour(_Script());
      routine = b.startCoroutine(_LazyRoutine(() => log.add('iterator')));
      b.invoke(() => log.add('invoke'), 0);
      b.tween(0.0, 1.0, 0, (_) => log.add('script tween'));
      world.tween(0.0, 1.0, 0, (_) => log.add('world tween'));
      expect(b.isInvoking, isTrue);
      expect(routine.isRunning, isTrue);
      expect(log, isEmpty);
    });
    expect(log, isEmpty);
    world.frame(0);
    expect(log, ['iterator', 'invoke', 'script tween', 'world tween']);
    world.frame(0);
    expect(routine.isRunning, isFalse);
  });

  test('abort retires escaped schedules and completes their futures without executing user code', () async {
    var called = false;
    late Coroutine routine;
    late Coroutine tween;
    expect(
      () => world.edit(() {
        final b = world.spawn().addBehaviour(_Script());
        routine = b.startCoroutine(_LazyRoutine(() => called = true));
        tween = world.tween(0.0, 1.0, 0, (_) => called = true);
        b.invoke(() => called = true, 0);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(routine.isRunning, isFalse);
    expect(tween.isRunning, isFalse);
    await routine.done;
    await tween.done;
    world.frame(0);
    expect(called, isFalse);
    expect(world.objects, isEmpty);
  });

  test('stopping existing work is visible inside the edit and reversible until commit', () async {
    final b = attach();
    var count = 0;
    Iterable<Object?> run() sync* {
      while (true) {
        count++;
        yield null;
      }
    }

    final c = b.startCoroutine(run());
    expect(count, 0);
    world.frame(0);
    expect(count, 1);
    expect(
      () => world.edit(() {
        c.stop();
        expect(c.isRunning, isFalse);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(c.isRunning, isTrue);
    world.frame(0);
    expect(count, 2);
    world.edit(c.stop);
    expect(c.isRunning, isFalse);
    world.frame(0);
    await c.done;
    expect(count, 2);
  });

  test('bulk cancellation includes staged routines and invokes independently', () async {
    final b = attach();
    var calls = 0;
    final oldInvoke = b.invoke(() => calls++, 0);
    late Coroutine routine;
    late Coroutine invocation;
    world.edit(() {
      routine = b.startCoroutine(_LazyRoutine(() => calls++));
      invocation = b.invoke(() => calls++, 0);
      b.stopAllCoroutines();
      expect(routine.isRunning, isFalse);
      expect(b.isInvoking, isTrue);
      b.cancelInvoke();
      expect(b.isInvoking, isFalse);
    });
    await routine.done;
    await invocation.done;
    world.frame(0);
    await oldInvoke.done;
    expect(calls, 0);
  });

  test('owner and target destruction cancels newly scheduled work at commit', () async {
    late Coroutine c;
    late Coroutine t;
    var calls = 0;
    world.edit(() {
      final go = world.spawn();
      final b = go.addBehaviour(_Script());
      c = b.invoke(() => calls++, 0);
      t = go.transform.moveTo(const Vec3(1, 0, 0), 1);
      go.destroy();
    });
    expect(c.isRunning, isFalse);
    expect(t.isRunning, isFalse);
    await c.done;
    await t.done;
    world.frame(0);
    expect(calls, 0);
  });

  test('invalid schedules fail stickily and preserve existing invocation ownership', () {
    final b = attach();
    var called = false;
    final existing = b.invoke(() => called = true, 0);
    for (final invalid in <void Function()>[
      () => b.invoke(() {}, double.nan),
      () => world.tween('a', 'b', 1, (_) {}),
      () => _Script().tween(0.0, 1.0, 1, (_) {}),
      () => _Script().startCoroutine(const []),
    ]) {
      expect(
        () => world.edit(() {
          existing.stop();
          world.spawn(name: 'discard');
          try {
            invalid();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(existing.isRunning, isTrue);
      expect(world.find('discard'), isNull);
    }
    world.frame(0);
    expect(called, isTrue);
  });

  test('cross-world cancellation and scheduling abort prior changes', () {
    final other = World.create();
    addTearDown(other.dispose);
    final b = other.edit(() => other.spawn().addBehaviour(_Script()));
    final c = b.invoke(() {}, 100);
    for (final operation in <void Function()>[c.stop, () => b.invoke(() {}, 0)]) {
      expect(
        () => world.edit(() {
          world.spawn(name: 'discard');
          try {
            operation();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(world.objects, isEmpty);
      expect(c.isRunning, isTrue);
    }
  });

  test('invoke deadlines start at the request and simulation waits use the fixed phase', () {
    final b = attach();
    final log = <String>[];
    world.edit(() {
      b.invoke(() => log.add('gameplay'), 0.1);
      b.invoke(() => log.add('simulation'), 0, clock: TimeClock.simulation);
      b.tween(0.0, 1.0, 0, (_) => log.add('simulation tween'), clock: TimeClock.simulation);
    });
    world.frame(0);
    expect(log, isEmpty);
    world.frame(1 / 60);
    expect(log, ['simulation', 'simulation tween']);
    for (var i = 0; i < 5; i++) {
      world.frame(1 / 60);
    }
    expect(log, ['simulation', 'simulation tween', 'gameplay']);
  });

  test('iterator failures after commit are reported through the scheduler', () async {
    final errors = <BehaviourError>[];
    world.onBehaviourError = errors.add;
    final b = attach();
    final c = world.edit(() => b.startCoroutine(_LazyRoutine(() => throw StateError('iterator failed'))));
    expect(errors, isEmpty);
    world.frame(0);
    await c.done;
    expect(c.isRunning, isFalse);
    expect(errors.single.callback, 'coroutine');
    expect(b.isAlive, isTrue);
    world.edit(() => b.gameObject.name = 'still usable');
  });

  test('world disposal completes committed schedules and rejects later starts', () async {
    final b = attach();
    late Coroutine c;
    late Coroutine t;
    world.edit(() {
      c = b.invoke(() => fail('disposed invoke ran'), 100);
      t = world.tween(0.0, 1.0, 100, (_) => fail('disposed tween ran'));
    });
    final done = c.done;
    world.dispose();
    await done;
    await t.done;
    expect(c.isRunning, isFalse);
    expect(t.isRunning, isFalse);
    expect(() => world.tween(0.0, 1.0, 1, (_) {}), throwsStateError);
    expect(() => b.invoke(() {}, 0), throwsStateError);
  });

  test('disposal during a deferred callback cancels the rest of the scheduler pass', () async {
    late Coroutine first;
    late Coroutine second;
    world.edit(() {
      first = world.tween(0.0, 1.0, 0, (_) => world.dispose());
      second = world.tween(0.0, 1.0, 0, (_) => fail('callback ran after disposal'));
    });
    world.frame(0);
    await first.done;
    await second.done;
    expect(world.isDisposed, isTrue);
  });

  test('rejected async edit cancels schedules and expires later coroutine access', () async {
    final b = attach();
    late Coroutine c;
    late Future<void> continuation;
    var expired = false;
    expect(
      () => world.edit(() {
        c = b.invoke(() => fail('aborted invocation ran'), 0);
        continuation = () async {
          await Future<void>.value();
          try {
            c.stop();
          } on StateError {
            expired = true;
          }
        }();
        return continuation;
      }),
      throwsStateError,
    );
    await continuation;
    expect(expired, isTrue);
    expect(c.isRunning, isFalse);
    await c.done;
    world.frame(0);
    expect(b.isInvoking, isFalse);
  });

  test('inactive staged owners cancel coroutines while disabled scripts retain invokes', () {
    final b = attach();
    late Coroutine c;
    var called = false;
    world.edit(() {
      c = b.startCoroutine(_LazyRoutine(() => fail('inactive coroutine started')));
      b.gameObject.setActive(false);
      b.enabled = false;
      b.invoke(() => called = true, 0);
    });
    expect(c.isRunning, isFalse);
    world.frame(0);
    expect(called, isTrue);
  });
}
