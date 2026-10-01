// Engine services and the `GameContext` they live in (design/dart-sdk.md
// §21.5, blueprint §3.3): explicit, replaceable services underneath the
// static facades (`Time.delta`). A facade resolves through the context that
// is *current* — the one the world installs around every behaviour
// callback — never through hidden process-global state, so a test can run
// gameplay code against a `TestGameContext` with its own clock.

import 'dart:async' show Zone, ZoneDelegate, ZoneSpecification;

import 'editor_channel.dart';
import 'input.dart';
import 'settings.dart';

/// Explicit timeline for deadlines and tweens.
enum TimeClock { gameplay, real, simulation }

/// Frame and fixed-step timing.
abstract interface class TimeService {
  /// Seconds per call of the callback that reads it: [fixedDelta] in the
  /// fixed phase (`fixedUpdate`, `afterPhysics`, the contact callbacks),
  /// the current (or last) host frame's seconds everywhere else — Unity's
  /// `Time.deltaTime`.
  double get delta;

  /// The host frame's seconds before [timeScale] — Unity's
  /// `Time.unscaledDeltaTime`, for a pause menu's own animation.
  double get unscaledDelta;

  /// How fast game time runs (Unity's `Time.timeScale`): 1 is real time,
  /// 0.5 half speed, 0 paused — no fixed tick runs and [delta] is 0 in
  /// `update`, which still runs, so the game can read the input that
  /// resumes it. [fixedDelta] does not change: the ticks come less often.
  /// Must be finite and >= 0; sampled at frame begin.
  double get timeScale;
  set timeScale(double value);

  /// Seconds per fixed tick.
  double get fixedDelta;

  /// Fixed ticks completed so far.
  int get tick;

  /// Accepted scaled/clamped gameplay seconds, identical in every phase.
  double get elapsed;

  /// Accepted real host seconds, independent of gameplay scaling/clamping.
  double get realElapsed;

  /// Completed fixed ticks times fixed delta; Step advances only this clock.
  double get simulationElapsed;

  /// Host frames begun so far.
  int get frame;

  /// Interpolation factor between the last two ticks (`0..1`).
  double get alpha;
}

/// Reads elapsed seconds from an explicit timeline.
extension TimeClockReading on TimeService {
  double seconds(TimeClock clock) => switch (clock) {
    TimeClock.gameplay => elapsed,
    TimeClock.real => realElapsed,
    TimeClock.simulation => simulationElapsed,
  };
}

/// Logs, markers and counters. The seed has logs.
abstract interface class DiagnosticsService {
  void log(String message, {String source = 'game'});
}

/// Prints through `print` — silent under the editor channel, where the
/// facade already wrote the `log` record ([EditorChannel]) and a print
/// would show the line in the editor's Console twice.
class PrintDiagnostics implements DiagnosticsService {
  const PrintDiagnostics();

  @override
  void log(String message, {String source = 'game'}) {
    if (!EditorChannel.enabled) print('[$source] $message');
  }
}

/// The services of one running world (or one test). Facades resolve through
/// [GameContext.current]; a world makes its context current around every
/// behaviour callback ([run]).
class GameContext {
  GameContext({
    required this.time,
    this.diagnostics = const PrintDiagnostics(),
    InputService? input,
    SettingsService? settings,
  }) : input = input ?? NoInput(),
       settings = settings ?? SettingsStore();

  final TimeService time;
  final DiagnosticsService diagnostics;

  /// The project's actions ([Input] facade): the world's binding table in
  /// a game, a [TestInput] in a test, [NoInput] when neither.
  final InputService input;

  /// The user's settings (M3-12): a memory-only [SettingsStore] until the
  /// game installs one over its `settings.json` (`World.useSettings`).
  SettingsService settings;
  final Map<Type, Object> _services = {};

  /// Installs (or replaces) a service by its static type: a test installs
  /// a recorded input, a game its own systems.
  void install<T extends Object>(T service) => _services[T] = service;

  /// The installed service of type [T]; throws when none was installed.
  T service<T extends Object>() {
    final s = _services[T];
    if (s == null) {
      throw StateError('no $T service is installed in this GameContext');
    }
    return s as T;
  }

  T? serviceOrNull<T extends Object>() => _services[T] as T?;

  /// Removes the service of type [T] (none installed: nothing happens).
  void uninstall<T extends Object>() => _services.remove(T);

  bool has<T extends Object>() => _services.containsKey(T);

  static final List<GameContext> _stack = [];

  /// The context gameplay code is running in, or null outside one.
  static GameContext? get current => _stack.isEmpty ? null : _stack.last;

  /// [current], or a [StateError] naming the facade that needed it.
  static GameContext require([String what = 'this call']) {
    final c = current;
    if (c == null) {
      throw StateError(
        '$what needs a current GameContext: it runs inside a behaviour callback, '
        'or wrap it in GameContext.run(context, () { ... })',
      );
    }
    return c;
  }

  /// Runs [body] with this context current (nested runs restore the outer
  /// one), so `Time.delta` and friends resolve to it.
  ///
  /// Under an editor ([EditorChannel.enabled]) the body also runs in a zone
  /// that turns `print` into a Console row with its stack (source
  /// `print`) — Play in the editor runs the game in the editor's own
  /// process, where stdout reaches no Console.
  R run<R>(R Function() body) {
    _stack.add(this);
    try {
      return EditorChannel.enabled ? _printZone().run(body) : body();
    } finally {
      _stack.removeLast();
    }
  }

  static Zone? _zone, _zoneParent;

  /// One zone per parent zone (a steady frame forks nothing).
  static Zone _printZone() {
    final current = Zone.current;
    if (identical(current, _zone)) return current;
    if (!identical(current, _zoneParent)) {
      _zoneParent = current;
      _zone = current.fork(
        specification: const ZoneSpecification(print: _print),
      );
    }
    return _zone!;
  }

  static void _print(Zone self, ZoneDelegate parent, Zone zone, String line) {
    if (!EditorChannel.enabled) return parent.print(zone, line);
    EditorChannel.send('log', {
      'tick': current?.time.tick,
      'source': 'print',
      'severity': 'info',
      'message': line,
      'stack': StackTrace.current.toString(),
    });
  }
}

/// A clock a test drives by hand.
class TestTime implements TimeService {
  TestTime({
    this.delta = 1 / 60,
    double? unscaledDelta,
    double timeScale = 1,
    this.fixedDelta = 1 / 60,
    this.tick = 0,
    this.frame = 0,
    this.alpha = 0,
    double? elapsed,
    this.realElapsed = 0,
  }) : elapsed = elapsed ?? tick * fixedDelta,
       unscaledDelta = unscaledDelta ?? delta {
    this.timeScale = timeScale;
  }

  @override
  double delta;
  @override
  double unscaledDelta;
  @override
  double get timeScale => _timeScale;
  @override
  set timeScale(double value) {
    if (!value.isFinite || value < 0) {
      throw ArgumentError.value(value, 'timeScale');
    }
    _timeScale = value;
  }

  double _timeScale = 1;
  @override
  double fixedDelta;
  @override
  int tick;
  @override
  int frame;
  @override
  double alpha;

  @override
  double elapsed;
  @override
  double realElapsed;
  @override
  double get simulationElapsed => tick * fixedDelta;

  /// Drive a host frame explicitly; fixed ticks are advanced separately.
  void advanceFrame(double dt, {double maxGameDelta = 0.25}) {
    if (!timeScale.isFinite || timeScale < 0) {
      throw ArgumentError.value(timeScale, 'timeScale');
    }
    if (!maxGameDelta.isFinite || maxGameDelta <= 0) {
      throw ArgumentError.value(maxGameDelta, 'maxGameDelta');
    }
    unscaledDelta = dt.isFinite && dt > 0 ? dt : 0;
    final scaled = unscaledDelta * timeScale;
    delta = scaled < maxGameDelta ? scaled : maxGameDelta;
    realElapsed += unscaledDelta;
    elapsed += delta;
    frame++;
  }

  /// Advances one fixed tick.
  void advanceTick() => tick += 1;
}

/// Collects log lines instead of printing them.
class RecordingDiagnostics implements DiagnosticsService {
  final List<String> lines = [];

  @override
  void log(String message, {String source = 'game'}) =>
      lines.add('[$source] $message');
}

/// A context for tests: a hand-driven clock and recorded logs, plus whatever
/// services the test installs.
class TestGameContext extends GameContext {
  factory TestGameContext({
    TestTime? time,
    RecordingDiagnostics? diagnostics,
    TestInput? input,
  }) => TestGameContext._(
    time ?? TestTime(),
    diagnostics ?? RecordingDiagnostics(),
    input ?? TestInput(),
  );

  TestGameContext._(
    TestTime time,
    RecordingDiagnostics diagnostics,
    TestInput input,
  ) : super(time: time, diagnostics: diagnostics, input: input);

  @override
  TestTime get time => super.time as TestTime;

  @override
  RecordingDiagnostics get diagnostics =>
      super.diagnostics as RecordingDiagnostics;

  /// The hand-driven actions (`press`, `release`, `setAxis`, `endTick`).
  @override
  TestInput get input => super.input as TestInput;
}

/// Static facade over the current context's [TimeService].
abstract final class Time {
  static double get delta => GameContext.require('Time.delta').time.delta;
  static double get unscaledDelta =>
      GameContext.require('Time.unscaledDelta').time.unscaledDelta;
  static double get timeScale =>
      GameContext.require('Time.timeScale').time.timeScale;
  static set timeScale(double value) =>
      GameContext.require('Time.timeScale').time.timeScale = value;
  static double get fixedDelta =>
      GameContext.require('Time.fixedDelta').time.fixedDelta;
  static int get tick => GameContext.require('Time.tick').time.tick;
  static double get elapsed => GameContext.require('Time.elapsed').time.elapsed;
  static double get realElapsed =>
      GameContext.require('Time.realElapsed').time.realElapsed;
  static double get simulationElapsed =>
      GameContext.require('Time.simulationElapsed').time.simulationElapsed;
  static int get frame => GameContext.require('Time.frame').time.frame;
  static double get alpha => GameContext.require('Time.alpha').time.alpha;
}

/// Static facade over the current context's [DiagnosticsService]. Every
/// log also goes out on the editor channel when one is listening (a `log`
/// record with the tick, the source and the message), whatever service the
/// game installed.
abstract final class Diagnostics {
  static void log(String message, {String source = 'game'}) {
    final context = GameContext.require('Diagnostics.log');
    if (EditorChannel.enabled) {
      EditorChannel.send('log', {
        'tick': context.time.tick,
        'source': source,
        'severity': 'info',
        'message': message,
        'stack': StackTrace.current.toString(),
      });
    }
    context.diagnostics.log(message, source: source);
  }
}

/// Unity's `Debug.Log` family: a Console row at the level asked for, with
/// the caller's stack — the row shows the calling line and a double-click
/// opens it in the IDE. Works in and outside a callback (the tick is the
/// current context's, when there is one). Without an editor the line goes
/// to the current context's [DiagnosticsService] (`print` by default; a
/// test's [RecordingDiagnostics]), prefixed by its level.
abstract final class Debug {
  static void log(Object? message) =>
      _send('info', '$message', StackTrace.current);
  static void logWarning(Object? message) =>
      _send('warning', '$message', StackTrace.current);
  static void logError(Object? message) =>
      _send('error', '$message', StackTrace.current);

  /// An error you caught, with its own stack (the red row of an exception).
  static void logException(Object error, [StackTrace? stack]) {
    final context = GameContext.current;
    if (EditorChannel.enabled) {
      EditorChannel.error(
        '$error',
        stack: (stack ?? StackTrace.current).toString(),
        tick: context?.time.tick,
      );
    }
    (context?.diagnostics ?? const PrintDiagnostics()).log(
      'error: $error',
      source: 'game',
    );
  }

  static void _send(String severity, String message, StackTrace stack) {
    final context = GameContext.current;
    if (EditorChannel.enabled) {
      EditorChannel.send('log', {
        'tick': context?.time.tick,
        'source': 'game',
        'severity': severity,
        'message': message,
        'stack': stack.toString(),
      });
    }
    (context?.diagnostics ?? const PrintDiagnostics()).log(
      severity == 'info' ? message : '$severity: $message',
      source: 'game',
    );
  }
}
