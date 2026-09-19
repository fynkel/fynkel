// Engine services and the `GameContext` they live in (design/dart-sdk.md
// §21.5, blueprint §3.3): explicit, replaceable services underneath the
// static facades (`Time.delta`). A facade resolves through the context that
// is *current* — the one the world installs around every behaviour
// callback — never through hidden process-global state, so a test can run
// gameplay code against a `TestGameContext` with its own clock.

/// Frame and fixed-step timing.
abstract interface class TimeService {
  /// Seconds of the current (or last) host frame.
  double get delta;

  /// Seconds per fixed tick.
  double get fixedDelta;

  /// Fixed ticks completed so far.
  int get tick;

  /// Simulated seconds: `tick * fixedDelta`.
  double get elapsed;

  /// Host frames begun so far.
  int get frame;

  /// Interpolation factor between the last two ticks (`0..1`).
  double get alpha;
}

/// Logs, markers and counters. The seed has logs.
abstract interface class DiagnosticsService {
  void log(String message, {String source = 'game'});
}

/// Prints through `print`.
class PrintDiagnostics implements DiagnosticsService {
  const PrintDiagnostics();

  @override
  void log(String message, {String source = 'game'}) => print('[$source] $message');
}

/// The services of one running world (or one test). Facades resolve through
/// [GameContext.current]; a world makes its context current around every
/// behaviour callback ([run]).
class GameContext {
  GameContext({required this.time, this.diagnostics = const PrintDiagnostics()});

  final TimeService time;
  final DiagnosticsService diagnostics;
  final Map<Type, Object> _services = {};

  /// Installs (or replaces) a service by its static type: a test installs
  /// a recorded input, a game its own systems.
  void install<T extends Object>(T service) => _services[T] = service;

  /// The installed service of type [T]; throws when none was installed.
  T service<T extends Object>() {
    final s = _services[T];
    if (s == null) throw StateError('no $T service is installed in this GameContext');
    return s as T;
  }

  T? serviceOrNull<T extends Object>() => _services[T] as T?;

  bool has<T extends Object>() => _services.containsKey(T);

  static final List<GameContext> _stack = [];

  /// The context gameplay code is running in, or null outside one.
  static GameContext? get current => _stack.isEmpty ? null : _stack.last;

  /// [current], or a [StateError] naming the facade that needed it.
  static GameContext require([String what = 'this call']) {
    final c = current;
    if (c == null) {
      throw StateError('$what needs a current GameContext: it runs inside a behaviour callback, '
          'or wrap it in GameContext.run(context, () { ... })');
    }
    return c;
  }

  /// Runs [body] with this context current (nested runs restore the outer
  /// one), so `Time.delta` and friends resolve to it.
  R run<R>(R Function() body) {
    _stack.add(this);
    try {
      return body();
    } finally {
      _stack.removeLast();
    }
  }
}

/// A clock a test drives by hand.
class TestTime implements TimeService {
  TestTime({this.delta = 1 / 60, this.fixedDelta = 1 / 60, this.tick = 0, this.frame = 0, this.alpha = 0});

  @override
  double delta;
  @override
  double fixedDelta;
  @override
  int tick;
  @override
  int frame;
  @override
  double alpha;

  @override
  double get elapsed => tick * fixedDelta;

  /// Advances one fixed tick.
  void advanceTick() => tick += 1;
}

/// Collects log lines instead of printing them.
class RecordingDiagnostics implements DiagnosticsService {
  final List<String> lines = [];

  @override
  void log(String message, {String source = 'game'}) => lines.add('[$source] $message');
}

/// A context for tests: a hand-driven clock and recorded logs, plus whatever
/// services the test installs.
class TestGameContext extends GameContext {
  factory TestGameContext({TestTime? time, RecordingDiagnostics? diagnostics}) =>
      TestGameContext._(time ?? TestTime(), diagnostics ?? RecordingDiagnostics());

  TestGameContext._(TestTime time, RecordingDiagnostics diagnostics) : super(time: time, diagnostics: diagnostics);

  @override
  TestTime get time => super.time as TestTime;

  @override
  RecordingDiagnostics get diagnostics => super.diagnostics as RecordingDiagnostics;
}

/// Static facade over the current context's [TimeService].
abstract final class Time {
  static double get delta => GameContext.require('Time.delta').time.delta;
  static double get fixedDelta => GameContext.require('Time.fixedDelta').time.fixedDelta;
  static int get tick => GameContext.require('Time.tick').time.tick;
  static double get elapsed => GameContext.require('Time.elapsed').time.elapsed;
  static int get frame => GameContext.require('Time.frame').time.frame;
  static double get alpha => GameContext.require('Time.alpha').time.alpha;
}

/// Static facade over the current context's [DiagnosticsService].
abstract final class Diagnostics {
  static void log(String message, {String source = 'game'}) =>
      GameContext.require('Diagnostics.log').diagnostics.log(message, source: source);
}
