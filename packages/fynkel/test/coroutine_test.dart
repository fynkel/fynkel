// Coroutines and Invoke (docs/unity-parity.md § Tier 2): a `sync*`
// generator resumed once per frame after `update`, the Unity waits, nested
// routines, Invoke / InvokeRepeating / CancelInvoke, and when they stop —
// over the real native library.

import 'package:fynkel/advanced.dart';
import 'package:test/test.dart';

/// Runs whatever the test hands it and counts frames through `update`.
class Host extends Behaviour {
  Host({this.onStart});

  final void Function(Host)? onStart;
  int frame = 0;
  final List<String> log = [];

  @override
  void start() => onStart?.call(this);

  @override
  void update(UpdateContext context) {
    frame++;
    log.add('update $frame');
  }

  @override
  void lateUpdate(UpdateContext context) => log.add('late $frame');
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  /// A world with one resolved object carrying [host].
  (World, GameObject) withHost(Host host) {
    final w = World.create(library: lib, fixedDt: 1 / 20)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final go = w.spawn(name: 'Host')..addBehaviour(host);
    w.frame(1 / 20);
    return (w, go);
  }

  test('a coroutine starts after commit at the scheduler, one step a frame', () {
    final host = Host();
    final (w, _) = withHost(host);
    Iterable<Object?> steps() sync* {
      host.log.add('step 1');
      yield null;
      host.log.add('step 2');
      yield null;
      host.log.add('step 3');
    }

    host.log.clear();
    final c = host.startCoroutine(steps());
    expect(host.log, isEmpty, reason: 'scheduling never executes user code');
    expect(c.isRunning, isTrue);
    w.frame(1 / 20);
    w.frame(1 / 20);
    w.frame(1 / 20);
    expect(host.log, [
      'update 2',
      'step 1',
      'late 2',
      'update 3',
      'step 2',
      'late 3',
      'update 4',
      'step 3',
      'late 4',
    ], reason: 'between update and lateUpdate');
    expect(c.isRunning, isFalse);
  });

  test('WaitForSeconds, WaitUntil / WaitWhile, a nested routine and waiting for another', () {
    final host = Host();
    final (w, _) = withHost(host);
    final at = <String, int>{};
    var open = false;
    Iterable<Object?> inner() sync* {
      yield null;
      at['inner'] = host.frame;
    }

    late Coroutine other;
    Iterable<Object?> sub() sync* {
      yield const WaitForSeconds(0.3);
      at['other'] = host.frame;
    }

    Iterable<Object?> main() sync* {
      yield const WaitForSeconds(0.2); // 0.05 s frames: four of them
      at['seconds'] = host.frame;
      yield WaitUntil(() => open);
      at['until'] = host.frame;
      yield inner();
      at['after inner'] = host.frame;
      other = host.startCoroutine(sub());
      yield other;
      at['after other'] = host.frame;
      yield WaitWhile(() => open);
      at['while'] = host.frame;
    }

    final c = host.startCoroutine(main());
    for (var i = 0; i < 6; i++) {
      w.frame(1 / 20);
    }
    expect(at, {'seconds': 6}, reason: 'first execution at frame 2; 0.2 s later is frame 6');
    open = true;
    w.frame(1 / 20);
    expect(at['until'], 8);
    expect(at['inner'], isNull, reason: 'a nested generator runs in place, from its first yield');
    w.frame(1 / 20);
    expect(at['inner'], 9);
    expect(at['after inner'], 9);
    for (var i = 0; i < 6; i++) {
      w.frame(1 / 20);
    }
    expect(at['other'], isNull, reason: 'the child starts after the scheduling callback commits');
    w.frame(1 / 20);
    expect(at['other'], 16, reason: '0.3 s after its first execution at frame 10');
    w.frame(1 / 20);
    expect(at['after other'], 17, reason: 'the frame after the other one ended');
    open = false;
    w.frame(1 / 20);
    expect(at['while'], 18);
    expect(c.isRunning, isFalse);
  });

  test('WaitForFixedUpdate goes on after a tick, with afterPhysics', () {
    final host = Host();
    final (w, _) = withHost(host);
    final ticks = <int>[];
    Iterable<Object?> fixed() sync* {
      for (var i = 0; i < 3; i++) {
        yield const WaitForFixedUpdate();
        ticks.add(w.tick);
      }
    }

    host.startCoroutine(fixed());
    w.frame(0); // Acquire the iterator at the variable scheduler boundary.
    final start = w.tick;
    w.frame(3 / 20); // three ticks in one frame
    expect(ticks, [start + 1, start + 2, start + 3]);
  });

  test('invoke, invokeRepeating, cancelInvoke; invokes run while disabled', () {
    final host = Host();
    final (w, _) = withHost(host);
    final calls = <String>[];
    host.invoke(() => calls.add('once @${host.frame}'), 0.1);
    final every = host.invokeRepeating(() => calls.add('every @${host.frame}'), 0, 0.1);
    expect(host.isInvoking, isTrue);
    for (var i = 0; i < 5; i++) {
      w.frame(1 / 20);
    }
    expect(calls, ['every @2', 'once @3', 'every @4', 'every @6']);
    host.enabled = false;
    w.frame(1 / 20);
    w.frame(1 / 20);
    expect(calls.last, startsWith('every'), reason: 'still called while disabled, as in Unity');
    final n = calls.length;
    host.cancelInvoke(every);
    expect(host.isInvoking, isFalse);
    w.frame(1 / 20);
    w.frame(1 / 20);
    expect(calls, hasLength(n));
  });

  test(
    'stopped by stopAllCoroutines, by leaving the active hierarchy and by destruction; a throw stops only it',
    () {
      final host = Host();
      final (w, go) = withHost(host);
      var ticks = 0;
      Iterable<Object?> forever() sync* {
        while (true) {
          ticks++;
          yield null;
        }
      }

      final a = host.startCoroutine(forever());
      host.enabled = false;
      w.frame(1 / 20);
      expect(a.isRunning, isTrue, reason: 'a disabled behaviour keeps its coroutines');
      host.enabled = true;
      go.setActive(false);
      w.frame(1 / 20);
      w.frame(1 / 20);
      expect(a.isRunning, isFalse, reason: 'an inactive object stops them');
      go.setActive(true);
      w.frame(1 / 20);

      final b = host.startCoroutine(forever());
      final kept = host.invoke(() {}, 100);
      host.stopAllCoroutines();
      expect((b.isRunning, kept.isRunning), (false, true), reason: 'invokes are not coroutines');

      final errors = <BehaviourError>[];
      w.onBehaviourError = errors.add;
      Iterable<Object?> broken() sync* {
        yield null;
        throw StateError('broken');
      }

      final c = host.startCoroutine(broken()), d = host.startCoroutine(forever());
      w.frame(1 / 20); // First execution yields; it cannot throw until resumed.
      final before = ticks;
      w.frame(1 / 20);
      expect(c.isRunning, isFalse);
      expect(errors.single.callback, 'coroutine');
      expect(errors.single.message, contains('broken'));
      expect(d.isRunning, isTrue);
      expect(ticks, before + 1, reason: 'the others went on');

      host.destroy();
      w.frame(1 / 20);
      expect(
        (d.isRunning, kept.isRunning),
        (false, false),
        reason: 'destroying the behaviour stops everything',
      );
      expect(() => host.startCoroutine(forever()), throwsStateError);
    },
  );
}
