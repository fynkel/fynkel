// Tweens (ADR-0026 §7, DOTween's everyday calls): a value moved from one
// end to the other over game time with an [Ease], on top of coroutines —
// each call returns the [Coroutine], so a script can yield it, `await
// tween.done`, or `stop()` it. `transform.moveTo` / `rotateTo` /
// `scaleTo` belong to the object (they end when it is destroyed);
// `Behaviour.tween` to the script (it stops with the script's
// coroutines); `World.tween` to nothing but the world.

part of 'world.dart';

/// How a tween's progress (0 → 1 over its time) maps to the share of the
/// way from its start to its end — the curves of easings.net, named as
/// DOTween names them. `in` starts slowly, `out` ends slowly, `inOut`
/// does both; `back` overshoots, `bounce` and `elastic` settle.
enum Ease {
  linear,
  inQuad,
  outQuad,
  inOutQuad,
  inCubic,
  outCubic,
  inOutCubic,
  inSine,
  outSine,
  inOutSine,
  inBack,
  outBack,
  inOutBack,
  outBounce,
  outElastic;

  /// The share of the way at progress [t] (0 → 0, 1 → 1).
  double call(double t) {
    const back = 1.70158, back2 = back * 1.525;
    return switch (this) {
      linear => t,
      inQuad => t * t,
      outQuad => 1 - (1 - t) * (1 - t),
      inOutQuad => t < 0.5 ? 2 * t * t : 1 - math.pow(-2 * t + 2, 2) / 2,
      inCubic => t * t * t,
      outCubic => 1 - math.pow(1 - t, 3).toDouble(),
      inOutCubic => t < 0.5 ? 4 * t * t * t : 1 - math.pow(-2 * t + 2, 3) / 2,
      inSine => 1 - math.cos(t * math.pi / 2),
      outSine => math.sin(t * math.pi / 2),
      inOutSine => -(math.cos(math.pi * t) - 1) / 2,
      inBack => (back + 1) * t * t * t - back * t * t,
      outBack => 1 + (back + 1) * math.pow(t - 1, 3) + back * math.pow(t - 1, 2),
      inOutBack =>
        t < 0.5
            ? math.pow(2 * t, 2) * ((back2 + 1) * 2 * t - back2) / 2
            : (math.pow(2 * t - 2, 2) * ((back2 + 1) * (t * 2 - 2) + back2) + 2) / 2,
      outBounce => _bounce(t),
      outElastic =>
        t == 0 || t == 1 ? t : math.pow(2, -10 * t) * math.sin((t * 10 - 0.75) * (2 * math.pi) / 3) + 1,
    };
  }

  static double _bounce(double t) {
    const n = 7.5625, d = 2.75;
    if (t < 1 / d) return n * t * t;
    if (t < 2 / d) return n * (t -= 1.5 / d) * t + 0.75;
    if (t < 2.5 / d) return n * (t -= 2.25 / d) * t + 0.9375;
    return n * (t -= 2.625 / d) * t + 0.984375;
  }
}

/// [a] to [b] at [t]: numbers (whole numbers rounded) and vectors in a
/// straight line, rotations
/// along the shorter arc. Anything else is an [ArgumentError].
Object _lerp(Object a, Object b, double t) => switch ((a, b)) {
  (final int a, final int b) => (a + (b - a) * t).round(),
  (final num a, final num b) => a + (b - a) * t,
  (final Vec2 a, final Vec2 b) => a.lerp(b, t),
  (final Vec3 a, final Vec3 b) => a.lerp(b, t),
  (final Vec4 a, final Vec4 b) => a.lerp(b, t),
  (final Quat a, final Quat b) => a.slerp(b, t),
  _ => throw ArgumentError(
    'a tween runs between two numbers, Vec2s, Vec3s, Vec4s or Quats, '
    'not ${a.runtimeType} and ${b.runtimeType}',
  ),
};

/// Calls [apply] with the eased share of the way every frame, from the
/// frame it starts (0) to the one [seconds] of game time later (1).
Iterable<Object?> _tweenRoutine(
  _BehaviourSystem system,
  double seconds,
  Ease ease,
  void Function(double) apply,
  TimeClock clock,
) sync* {
  final start = system._seconds(clock);
  while (true) {
    final elapsed = system._seconds(clock) - start;
    // Same deadline tolerance as WaitForSeconds, including f32 fixed ticks.
    final t = elapsed + 1e-6 >= seconds ? 1.0 : (elapsed / seconds).clamp(0.0, 1.0);
    apply(ease(t));
    if (t >= 1) return;
    yield clock == TimeClock.simulation ? const WaitForFixedUpdate() : null;
  }
}

void _checkSeconds(double seconds) {
  if (!seconds.isFinite || seconds < 0) {
    (Zone.current[_gameplayZoneKey] as _GameplayAdapter?)?.reject('duration must be finite and not negative');
    throw ArgumentError.value(seconds, 'seconds', 'must be finite and not negative');
  }
}

/// A setter for [T] values from [from] to [to], checked before anything
/// starts.
void Function(double) _between<T extends Object>(T from, T to, void Function(T value) onValue) {
  final edit = Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
  if (edit != null) {
    edit.guard(() => _lerp(from, to, 0));
  } else {
    _lerp(from, to, 0);
  }
  return (t) => onValue(_lerp(from, to, t) as T);
}

extension on World {
  Coroutine _tweenOn(
    GameObject? target,
    double seconds,
    Ease ease,
    void Function(double) apply,
    TimeClock clock,
  ) {
    _checkSeconds(seconds);
    return _behaviours._start(
      Coroutine._(null, _tweenRoutine(_behaviours, seconds, ease, apply, clock), false, target)
        .._initialWait = clock == TimeClock.simulation ? const WaitForFixedUpdate() : null,
    );
  }
}
