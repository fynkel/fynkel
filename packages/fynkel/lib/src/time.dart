part of 'world.dart';

/// The world's clock, as Unity's `Time`: read it from any object or
/// behaviour as `time` (`time.delta`). [World.update] moves it on once a
/// frame, before anything runs.
final class Time {
  Time._();

  double _delta = 0;
  double _unscaledDelta = 0;
  double _sinceStart = 0;
  double _unscaledSinceStart = 0;
  int _frame = 0;
  double _scale = 1;
  double _maxDelta = 1 / 3;

  /// The seconds this frame counts for: the frame's time, at most
  /// [maxDelta], times [scale]. Move by `speed * time.delta`.
  double get delta => _delta;

  /// This frame's seconds without [scale] (still at most [maxDelta]): for
  /// what keeps moving while the game is paused, such as a pause menu.
  double get unscaledDelta => _unscaledDelta;

  /// The sum of every frame's [delta]: the game's time since the world
  /// was made.
  double get sinceStart => _sinceStart;

  /// The sum of every frame's [unscaledDelta].
  double get unscaledSinceStart => _unscaledSinceStart;

  /// How many frames have run: 0 before the first [World.update].
  int get frame => _frame;

  /// How fast the game's time runs: 1 is real time, 0.5 slow motion, 0
  /// paused (objects still get their updates, with a [delta] of 0).
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

  void _advance(double dt) {
    if (!(dt >= 0)) throw ArgumentError.value(dt, 'dt', 'must be 0 or more');
    final raw = dt < _maxDelta ? dt : _maxDelta;
    _unscaledDelta = raw;
    _delta = raw * _scale;
    _sinceStart += _delta;
    _unscaledSinceStart += raw;
    _frame++;
  }

  @override
  String toString() => 'Time(frame $_frame, delta $_delta, scale $_scale)';
}
