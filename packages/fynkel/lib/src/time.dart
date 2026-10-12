part of 'world.dart';

/// The world's clock, as Unity's `Time`: read it from any object or
/// behaviour as `time` (`time.delta`). [World.update] moves it on once a
/// frame, before anything runs.
///
/// The game's time is also cut into fixed steps of [fixedDelta] (50 a
/// second by default), whatever the frame rate: each frame runs as many as
/// its time holds, maybe none ([GameObject.onFixedUpdate]). Inside one,
/// [delta] is [fixedDelta] and [sinceStart] the steps' own time.
final class Time {
  Time._();

  double _delta = 0;
  double _unscaledDelta = 0;
  double _sinceStart = 0;
  double _unscaledSinceStart = 0;
  int _frame = 0;
  double _scale = 1;
  double _maxDelta = 1 / 3;
  double _fixedDelta = 1 / 50;
  double _fixedTime = 0;
  int _fixedSteps = 0;
  bool _inFixed = false;

  /// The seconds this frame counts for: the frame's time, at most
  /// [maxDelta], times [scale]. Move by `speed * time.delta`. Inside a
  /// fixed step, [fixedDelta].
  double get delta => _inFixed ? _fixedDelta : _delta;

  /// This frame's seconds without [scale] (still at most [maxDelta]): for
  /// what keeps moving while the game is paused, such as a pause menu.
  double get unscaledDelta => _unscaledDelta;

  /// The sum of every frame's [delta]: the game's time since the world
  /// was made. Inside a fixed step, the time at its end: [fixedDelta] times
  /// the steps so far.
  double get sinceStart => _inFixed ? _fixedTime : _sinceStart;

  /// The sum of every frame's [unscaledDelta].
  double get unscaledSinceStart => _unscaledSinceStart;

  /// How many frames have run: 0 before the first [World.update].
  int get frame => _frame;

  /// How fast the game's time runs: 1 is real time, 0.5 slow motion, 0
  /// stopped (objects still get their updates, with a [delta] of 0). To
  /// pause the game, input and all, use [World.paused].
  /// Throws an [ArgumentError] unless it's finite and 0 or more.
  double get scale => _scale;
  set scale(double value) {
    if (!(value >= 0 && value.isFinite)) throw ArgumentError.value(value, 'scale', 'must be finite and 0 or more');
    _scale = value;
  }

  /// The most one frame counts for, in seconds (1/3 by default, as
  /// Unity's): after a hitch (loading, the app in the background) the game
  /// steps this far rather than jumping. Throws an [ArgumentError] unless
  /// it's more than 0.
  double get maxDelta => _maxDelta;
  set maxDelta(double value) {
    if (!(value > 0)) throw ArgumentError.value(value, 'maxDelta', 'must be more than 0');
    _maxDelta = value;
  }

  /// How long a fixed step is, in game seconds: 1/50 by default, as
  /// Unity's. Throws an [ArgumentError] unless it's finite and more than 0.
  double get fixedDelta => _fixedDelta;
  set fixedDelta(double value) {
    if (!(value > 0 && value.isFinite)) {
      throw ArgumentError.value(value, 'fixedDelta', 'must be finite and more than 0');
    }
    _fixedDelta = value;
  }

  /// Whether a fixed step is running: in [GameObject.onFixedUpdate], and
  /// in what runs with it.
  bool get inFixedStep => _inFixed;

  /// How far this frame is from the last fixed step towards the next, 0 to
  /// 1: to draw what the steps move between where the last two left it.
  double get fixedAlpha {
    final alpha = (_sinceStart - _fixedTime) / _fixedDelta;
    return alpha < 0 ? 0 : (alpha > 1 ? 1 : alpha);
  }

  /// Whether the frame's time holds another fixed step.
  bool get _fixedDue => _fixedTime + _fixedDelta <= _sinceStart + 1e-9;

  // Whether the world is paused, and whether the next frame is the first
  // after a pause, which counts no time.
  bool _paused = false;
  bool _skip = false;

  void _advance(double dt) {
    if (!(dt >= 0)) throw ArgumentError.value(dt, 'dt', 'must be 0 or more');
    var raw = dt < _maxDelta ? dt : _maxDelta;
    if (_skip) {
      _skip = false;
      raw = 0;
    }
    _unscaledDelta = raw;
    _delta = _paused ? 0 : raw * _scale;
    _sinceStart += _delta;
    _unscaledSinceStart += raw;
    _frame++;
  }

  @override
  String toString() => 'Time(frame $_frame, delta $_delta, scale $_scale)';
}
