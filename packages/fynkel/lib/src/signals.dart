part of 'world.dart';

// Signals (docs/stage-4.md §3.5): something that happens, for whoever
// wants to hear it.
//
// A signal holds its subscriptions; an object or behaviour that listens
// with `on` holds its own only weakly, so a long-lived listener doesn't
// keep the signals of destroyed emitters alive, and cancels them when
// it's destroyed or removed, so a destroyed listener is never called and
// nothing holds it.

/// Something that happens, for whoever wants to hear it, with a value of
/// type [T]: `final died = Signal<void>();`, then `died.emit(null)` where
/// it happens, and `on(health.died, (_) => score.value++)` in the object
/// or behaviour that cares. Plain Dart: it needs no world, and works in
/// tests and in Flutter code too.
///
/// Listeners hear it at once, in [emit], in the order they started
/// listening. A listener added while it emits hears the next one; one
/// cancelled while it emits, before its turn, doesn't hear this one.
final class Signal<T> {
  final _subscriptions = <SignalSubscription<T>>[];

  // How deep in [emit] it is, and whether a subscription was cancelled
  // meanwhile, to be taken out once it's done.
  var _emitting = 0;
  var _cancelledWhileEmitting = false;

  // [stream]'s, once asked for.
  StreamController<T>? _controller;

  /// Whether anything listens, with [listen], `on` or [stream].
  bool get hasListeners => _subscriptions.any((s) => !s._cancelled) || (_controller?.hasListener ?? false);

  /// The values emitted from now on, as a broadcast [Stream], for code
  /// outside the game such as a Flutter widget: `StreamBuilder(stream:
  /// game.over.stream, ...)`, or `game.over.stream.listen(...)` to show a
  /// dialog. Unlike [listen]'s handlers, which hear a value in [emit], a
  /// stream's listeners hear it a moment later, as Dart's streams do.
  Stream<T> get stream => (_controller ??= StreamController<T>.broadcast()).stream;

  /// Calls [handler] with each value emitted from now on, until the
  /// subscription is cancelled. In an object or behaviour, use its `on`
  /// instead, which ends by itself when it's destroyed.
  SignalSubscription<T> listen(void Function(T value) handler) => _add(SignalSubscription<T>._(this, handler, null));

  SignalSubscription<T> _add(SignalSubscription<T> subscription) {
    _subscriptions.add(subscription);
    return subscription;
  }

  /// Tells every listener [value], one after the other. A listener added
  /// with `on` that throws has its error go to [World.errors], with its
  /// object, and the rest still hear it, as when an object's update
  /// throws. One added with [listen] that throws doesn't stop the rest
  /// either: once they have all heard it, this throws the first such
  /// error.
  void emit(T value) {
    Object? error;
    StackTrace? stack;
    _emitting++;
    try {
      final n = _subscriptions.length;
      for (var i = 0; i < n; i++) {
        final subscription = _subscriptions[i];
        if (subscription._cancelled) continue;
        try {
          subscription._hear(value);
        } catch (e, s) {
          if (error == null) (error, stack) = (e, s);
        }
      }
    } finally {
      if (--_emitting == 0 && _cancelledWhileEmitting) {
        _cancelledWhileEmitting = false;
        _subscriptions.removeWhere((s) => s._cancelled);
      }
    }
    if (_controller case final controller? when controller.hasListener) controller.add(value);
    if (error != null) Error.throwWithStackTrace(error, stack!);
  }

  /// The next value emitted. In a routine it's one of the routine's waits:
  /// the code after it runs in the routines step (in the frame after the
  /// one it started waiting in, at the earliest), it holds while the
  /// routine is paused, and stopping the routine unwinds it from here,
  /// through its `finally` blocks. `race([died.next, wait(5)])` waits for
  /// at most 5 seconds.
  Future<T> get next {
    final routine = _currentRoutine;
    if (routine == null) {
      final completer = Completer<T>();
      late final SignalSubscription<T> subscription;
      subscription = listen((value) {
        subscription.cancel();
        completer.complete(value);
      });
      return completer.future;
    }
    if (routine._stopped) throw RoutineStopped._();
    final world = routine._world;
    final wait = _NextWait<T>(routine, world.time._frame);
    wait.subscription = listen(wait.fire);
    world._waits.add(wait);
    final future = wait.values.future;
    _waitOf[future] = wait;
    return future;
  }

  @override
  String toString() => 'Signal<$T>';
}

/// A listener of a [Signal]: what [Signal.listen] and `on` return.
final class SignalSubscription<T> {
  SignalSubscription._(this._signal, this._handler, this._owner);

  Signal<T>? _signal;
  void Function(T value)? _handler;

  // The GameObject or Behaviour that listens with `on`, or null.
  final Object? _owner;

  bool _cancelled = false;

  /// Whether it was cancelled, by [cancel] or with its object or
  /// behaviour.
  bool get isCancelled => _cancelled;

  /// Stops listening, and lets go of the handler. Cancelling it again
  /// does nothing.
  void cancel() {
    if (_cancelled) return;
    _cancelled = true;
    _handler = null;
    final signal = _signal!;
    _signal = null;
    if (signal._emitting > 0) {
      signal._cancelledWhileEmitting = true;
    } else {
      signal._subscriptions.remove(this);
    }
  }

  void _hear(T value) {
    final handler = _handler!;
    final object = switch (_owner) {
      final GameObject o => o,
      final Behaviour b => b._owner,
      _ => null,
    };
    final world = object?._world;
    if (world == null || world.isDisposed) return handler(value);
    try {
      handler(value);
    } catch (error, stack) {
      world._report(object, error, stack);
    }
  }
}

/// The subscriptions an object or behaviour made with `on`, held weakly:
/// its signal holds each, so one whose signal is gone goes too.
final class _Listening {
  final _subscriptions = <WeakReference<SignalSubscription<Object?>>>[];

  // Gone and cancelled ones are dropped when the list reaches this.
  var _tidyAt = 16;

  void add(SignalSubscription<Object?> subscription) {
    if (_subscriptions.length >= _tidyAt) {
      _subscriptions.removeWhere((w) => w.target?._cancelled ?? true);
      _tidyAt = math.max(16, _subscriptions.length * 2);
    }
    _subscriptions.add(WeakReference(subscription));
  }

  void cancelAll() {
    for (final w in _subscriptions) {
      w.target?.cancel();
    }
    _subscriptions.clear();
  }
}

SignalSubscription<T> _on<T>(Object owner, Signal<T> signal, void Function(T value) handler) {
  final subscription = signal._add(SignalSubscription<T>._(signal, handler, owner));
  switch (owner) {
    case final GameObject o:
      (o._listening ??= _Listening()).add(subscription);
    case final Behaviour b:
      (b._listening ??= _Listening()).add(subscription);
  }
  return subscription;
}

/// A routine waiting for a signal's next value.
final class _NextWait<T> extends _Wait {
  _NextWait(super.routine, super.made);

  final values = Completer<T>.sync();
  SignalSubscription<T>? subscription;
  var _fired = false;
  late T _value;

  void fire(T value) {
    if (_fired) return;
    _fired = true;
    _value = value;
    _stopListening();
  }

  void _stopListening() {
    subscription?.cancel();
    subscription = null;
  }

  @override
  set over(bool value) {
    super.over = value;
    if (value) _stopListening();
  }

  @override
  bool ends(Time time) => _fired;

  @override
  void end() => values.complete(_value);

  @override
  void fail(Object error, StackTrace stack) => values.completeError(error, stack);
}
