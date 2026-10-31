part of 'world.dart';

// Routines (docs/stage-4.md §3.4): async functions whose waits the world
// ends in the routines step of its frame.
//
// A routine's body runs in a zone of its own. Its waits complete
// synchronously in [World._runRoutines], so the code after them runs
// there, in the order the waits were made. The microtasks it schedules
// (an `await` on a finished future, or on a routine that ended at once)
// go to the world's own queue, never Dart's: those scheduled while that
// step runs run before it ends, and the others in the next one. So a game
// test's plain loop of updates, with no event loop between, drains them. Once a routine is stopped its zone runs none of its
// callbacks but those carrying [RoutineStopped], which unwind it through
// its `finally` blocks: nothing else of it ever runs again, even when a
// future from outside it completes later.

const _routineKey = #fynkel.routine;
const _baseKey = #fynkel.routineBase;

/// The routine the code calling this runs in, or null.
Routine<Object?>? get _currentRoutine => Zone.current[_routineKey] as Routine<Object?>?;

/// The zone routines started from here are forked from: the one the
/// first routine of a chain was started in.
Zone get _baseZone => (Zone.current[_baseKey] as Zone?) ?? Zone.current;

final _routineZone = ZoneSpecification(
  runUnary: _runUnary,
  runBinary: _runBinary,
  scheduleMicrotask: _scheduleMicrotask,
);

// What a stopped routine's callbacks return instead of running: nothing,
// or a future that never completes, so what was chained on them waits
// forever rather than failing.
final Future<Never> _never = Completer<Never>().future;

R _runUnary<R, T>(Zone self, ZoneDelegate parent, Zone zone, R Function(T) f, T arg) {
  if ((self[_routineKey] as Routine<Object?>)._stopped) {
    if (null is R) return null as R;
    if (_never is R) return _never as R;
  }
  return parent.runUnary(zone, f, arg);
}

R _runBinary<R, T1, T2>(Zone self, ZoneDelegate parent, Zone zone, R Function(T1, T2) f, T1 a, T2 b) {
  if ((self[_routineKey] as Routine<Object?>)._stopped && a is! RoutineStopped) {
    if (null is R) return null as R;
    if (_never is R) return _never as R;
  }
  return parent.runBinary(zone, f, a, b);
}

void _scheduleMicrotask(Zone self, ZoneDelegate parent, Zone zone, void Function() f) {
  final world = (self[_routineKey] as Routine<Object?>)._world;
  // A disposed world's routines are over.
  if (world.isDisposed) return;
  (world._inRoutineStep ? world._microtasks : world._laterMicrotasks).add(zone.bindCallbackGuarded(f));
}

/// Thrown into a routine, at the `await` it waits at, when it's stopped,
/// so its `finally` blocks run; and what a stopped routine fails with
/// when awaited. Catching it doesn't keep the routine going: whatever it
/// waits for next ends at once the same way.
final class RoutineStopped implements Exception {
  RoutineStopped._();

  @override
  String toString() => 'RoutineStopped: the routine was stopped';
}

/// A running routine: what `startRoutine` returns. Await it for what its
/// body returns (`final choice = await startRoutine(askPlayer);`), or
/// [stop] it.
///
/// It stops by itself when the object or behaviour that started it is
/// destroyed or removed, and when the routine it was started in is
/// stopped. While its object is hidden (or its behaviour disabled) it's
/// paused: its waits don't count down and don't end.
///
/// What its body throws and doesn't catch goes to [World.errors], unless
/// something awaits the routine, which then gets the error.
final class Routine<T> implements Future<T> {
  Routine._(this._world, this._owner, this._object, this._parent) {
    _result.future.ignore();
  }

  final World _world;
  final _Moving _owner; // the GameObject or Behaviour that started it
  final GameObject _object;
  final Routine<Object?>? _parent;
  final _result = Completer<T>.sync();
  List<Routine<Object?>>? _children;
  // What it waits for and hasn't been given yet: one wait, mostly.
  List<_Wait>? _waits;
  var _running = true;
  var _stopped = false;
  var _awaited = false;

  /// Whether it's still going: it hasn't returned, failed or been stopped.
  bool get isRunning => _running;

  /// Whether it was stopped ([stop], or with its owner or parent).
  bool get isStopped => _stopped;

  /// Completes with what its body returns; fails with what it throws, or
  /// with [RoutineStopped] if it's stopped first.
  Future<T> get done {
    _awaited = true;
    return _result.future;
  }

  /// Stops it, and the routines started in it, theirs first: each is
  /// unwound from the `await` it waits at, so its `finally` blocks run now,
  /// and nothing else of it runs again. Stopping one that has ended does
  /// nothing.
  void stop() {
    if (!_running) return;
    // Over before its children stop: if it awaits one of them, it unwinds
    // while they do, and that mustn't end it a second time.
    _stopped = true;
    _running = false;
    for (final child in [...?_children].reversed) {
      child.stop();
    }
    _world._failWaits(this);
    _detach();
    _result.completeError(RoutineStopped._(), StackTrace.current);
  }

  bool get _paused => _owner._paused;

  void _finish(T value) {
    if (!_running) return;
    _running = false;
    _detach();
    _result.complete(value);
  }

  void _fail(Object error, StackTrace stack) {
    if (!_running) {
      // An error in a `finally` while it was being stopped.
      if (error is! RoutineStopped && !_world.isDisposed) _world._report(_object, error, stack);
      return;
    }
    if (error is RoutineStopped) {
      // It awaited a routine that was stopped, and didn't catch it.
      _stopped = true;
    } else if (!_awaited && !_world.isDisposed) {
      _world._report(_object, error, stack);
    }
    _running = false;
    _detach();
    _result.completeError(error, stack);
  }

  void _detach() {
    _owner._routines?.remove(this);
    _parent?._children?.remove(this);
  }

  @override
  Future<R> then<R>(FutureOr<R> Function(T value) onValue, {Function? onError}) => done.then(onValue, onError: onError);

  @override
  Future<T> catchError(Function onError, {bool Function(Object error)? test}) => done.catchError(onError, test: test);

  @override
  Future<T> whenComplete(FutureOr<void> Function() action) => done.whenComplete(action);

  @override
  Future<T> timeout(Duration timeLimit, {FutureOr<T> Function()? onTimeout}) =>
      done.timeout(timeLimit, onTimeout: onTimeout);

  @override
  Stream<T> asStream() => done.asStream();

  @override
  String toString() =>
      'Routine of $_owner${_running
          ? ''
          : _stopped
          ? ' (stopped)'
          : ' (ended)'}';
}

/// Something a routine waits for, ended by [World._runRoutines].
final class _Wait {
  _Wait(
    this.routine,
    this.made, {
    this.seconds = 0,
    this.timed = false,
    this.unscaled = false,
    this.test,
    this.until = true,
  });

  final Routine<Object?> routine;
  final int made; // the frame (or fixed step) it was made in: it ends in a later one
  double seconds; // left to wait, for a timed one
  final bool timed;
  final bool unscaled;
  final bool Function()? test;
  final bool until; // ends when [test] is true (else when it's false)
  final completer = Completer<void>.sync();

  /// Whether it's done with: ended, failed or given up. Once it is, its
  /// routine no longer holds it.
  bool get over => _over;
  set over(bool value) {
    _over = value;
    if (value) routine._waits?.remove(this);
  }

  bool _over = false;

  /// Ends it: the routine goes on.
  void end() => completer.complete();

  /// Throws [error] into the routine, at its `await`.
  void fail(Object error, StackTrace stack) => completer.completeError(error, stack);

  /// Whether it ends in this frame's routines step.
  bool ends(Time time) {
    if (timed) {
      seconds -= unscaled ? time._unscaledDelta : time._delta;
      return seconds <= 0;
    }
    if (test case final test?) return test() == until;
    return true;
  }
}

final _waitOf = Expando<_Wait>();

/// Says the wait [what] was asked for outside a routine, and how to start
/// one.
StateError _outsideRoutine(String what) {
  final example = switch (what) {
    'wait' => 'wait(1)',
    'waitUntil' => 'waitUntil(() => done)',
    'waitWhile' => 'waitWhile(() => busy)',
    _ => what,
  };
  final name = what.contains(' ') || !example.contains('(') ? what : '$what(...)';
  return StateError(
    '$name works only inside a routine, which waits for it: startRoutine(() async { await $example; }) on an '
    'object or a behaviour',
  );
}

Future<void> _addWait(_Wait Function(Routine<Object?> routine, int frame) make, String what, {bool fixed = false}) {
  final routine = _currentRoutine ?? (throw _outsideRoutine(what));
  if (routine._stopped) throw RoutineStopped._();
  final world = routine._world;
  final wait = make(routine, fixed ? world.time._fixedSteps : world.time._frame);
  (fixed ? world._fixedWaits : world._waits).add(wait);
  (routine._waits ??= []).add(wait);
  final future = wait.completer.future;
  _waitOf[future] = wait;
  return future;
}

/// Ends in the routines step of the next frame: Unity's `yield return null`.
/// Only inside a routine.
Future<void> get nextFrame => _addWait((r, frame) => _Wait(r, frame), 'nextFrame');

/// Ends right after the next fixed step's [GameObject.onFixedUpdate]
/// calls, still inside the step: Unity's `yield return new
/// WaitForFixedUpdate()`. A frame with no step doesn't end it; one with
/// several ends it in the first. Only inside a routine.
Future<void> get fixedStep => _addWait((r, step) => _Wait(r, step), 'fixedStep', fixed: true);

/// Ends once [seconds] of game time ([Time.delta], so [Time.scale] slows
/// it) have passed, counting from the next frame, in that frame's
/// routines step; with [unscaled], real time ([Time.unscaledDelta]).
/// `wait(0)` ends next frame. Only inside a routine.
Future<void> wait(double seconds, {bool unscaled = false}) {
  if (!(seconds >= 0)) throw ArgumentError.value(seconds, 'seconds', 'must be 0 or more');
  return _addWait((r, frame) => _Wait(r, frame, seconds: seconds, timed: true, unscaled: unscaled), 'wait');
}

/// Ends in the first routines step, from the next frame on, where [test]
/// returns true. Only inside a routine.
Future<void> waitUntil(bool Function() test) => _addWait((r, frame) => _Wait(r, frame, test: test), 'waitUntil');

/// Ends in the first routines step, from the next frame on, where [test]
/// returns false. Only inside a routine.
Future<void> waitWhile(bool Function() test) =>
    _addWait((r, frame) => _Wait(r, frame, test: test, until: false), 'waitWhile');

/// What the first of [racers] to end gives, or throws; the rest are
/// stopped (routines, with their `finally` blocks run first) or dropped
/// (waits). `await race([wait(3), startRoutine(fight)])` fights for at most 3
/// seconds. Only routines and waits given as they are can be stopped:
/// another future, such as `wait(3).then(...)`, still ends, into nothing.
Future<T> race<T>(Iterable<Future<T>> racers) {
  final all = racers.toList();
  if (all.isEmpty) {
    throw ArgumentError('race was given nothing to race: give it a wait or a routine at least', 'racers');
  }
  final result = Completer<T>.sync();
  // Set before the rest are stopped, as a stopped routine ends at once.
  var settled = false;
  bool won(Future<T> winner) {
    if (settled) return false;
    settled = true;
    for (final racer in all) {
      if (!identical(racer, winner)) _cancel(racer);
    }
    return true;
  }

  _baseZone.run(() {
    for (final racer in all) {
      racer.then(
        (value) {
          if (won(racer)) result.complete(value);
        },
        onError: (Object error, StackTrace stack) {
          if (won(racer)) result.completeError(error, stack);
        },
      );
    }
  });
  return result.future;
}

/// What each of [futures] gives, in order, once all have ended. If one
/// throws, the rest are stopped or dropped (as [race] does) and this
/// throws that.
Future<List<T>> waitAll<T>(Iterable<Future<T>> futures) {
  final list = futures.toList();
  final result = Completer<List<T>>.sync();
  if (list.isEmpty) {
    result.complete(<T>[]);
    return result.future;
  }
  final values = List<T?>.filled(list.length, null);
  var left = list.length;
  var failed = false;
  _baseZone.run(() {
    for (final (i, future) in list.indexed) {
      future.then(
        (value) {
          if (failed) return;
          values[i] = value;
          if (--left == 0) result.complete([for (final v in values) v as T]);
        },
        onError: (Object error, StackTrace stack) {
          if (failed) return;
          failed = true;
          for (final other in list) {
            if (!identical(other, future)) _cancel(other);
          }
          result.completeError(error, stack);
        },
      );
    }
  });
  return result.future;
}

void _cancel(Future<Object?> future) {
  if (future is Routine<Object?>) {
    future.stop();
  } else if (future is Motion) {
    future.stop();
  } else if (_waitOf[future] case final wait?) {
    wait.over = true;
  }
}
