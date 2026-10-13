part of 'world.dart';

// Motion (docs/stage-4.md §3.7): tweens, and groups of them, that the
// world moves on at the start of its routines step.
//
// Every motion is a function of its own time: [Motion._seek] puts what it
// moves where it is at that time, forwards or back, so a group plays its
// parts, and plays them back for a yoyo, by seeking them. What a helper
// moves from is read when it begins (after its delay), so the second
// part of a sequence starts where the first ended.

/// How a motion goes from its start to its end: given how far along it is
/// in time (0 to 1), how far along the value is (0 at the start, 1 at the
/// end, and beyond them for the ones that overshoot). Any function will
/// do, so Flutter's curves work too: `ease: Curves.easeOut.transform`.
typedef EaseFunction = double Function(double t);

/// The usual eases, as functions (Robert Penner's, as in easings.net and
/// DOTween). `in` starts slow, `out` ends slow, `inOut` does both;
/// `Back` overshoots a little, `Elastic` springs and `Bounce` bounces.
abstract final class Ease {
  /// The same speed all the way: t.
  static double linear(double t) => t;

  /// Starts slow and speeds up, on a sine curve, the gentlest.
  static double inSine(double t) => 1 - math.cos(t * math.pi / 2);

  /// Starts fast and slows to the end, on a sine curve, the gentlest.
  static double outSine(double t) => math.sin(t * math.pi / 2);

  /// Slow at both ends, fast in the middle, on a sine curve, the gentlest.
  static double inOutSine(double t) => -(math.cos(math.pi * t) - 1) / 2;

  /// Starts slow and speeds up, on t².
  static double inQuad(double t) => t * t;

  /// Starts fast and slows to the end, on t²: every tween's default.
  static double outQuad(double t) => 1 - (1 - t) * (1 - t);

  /// Slow at both ends, fast in the middle, on t².
  static double inOutQuad(double t) => t < 0.5 ? 2 * t * t : 1 - _pow(-2 * t + 2, 2) / 2;

  /// Starts slow and speeds up, on t³.
  static double inCubic(double t) => t * t * t;

  /// Starts fast and slows to the end, on t³.
  static double outCubic(double t) => 1 - _pow(1 - t, 3);

  /// Slow at both ends, fast in the middle, on t³.
  static double inOutCubic(double t) => t < 0.5 ? 4 * t * t * t : 1 - _pow(-2 * t + 2, 3) / 2;

  /// Starts slow and speeds up, on t⁴.
  static double inQuart(double t) => t * t * t * t;

  /// Starts fast and slows to the end, on t⁴.
  static double outQuart(double t) => 1 - _pow(1 - t, 4);

  /// Slow at both ends, fast in the middle, on t⁴.
  static double inOutQuart(double t) => t < 0.5 ? 8 * _pow(t, 4) : 1 - _pow(-2 * t + 2, 4) / 2;

  /// Starts slow and speeds up, on t⁵, the sharpest of the powers.
  static double inQuint(double t) => _pow(t, 5);

  /// Starts fast and slows to the end, on t⁵, the sharpest of the powers.
  static double outQuint(double t) => 1 - _pow(1 - t, 5);

  /// Slow at both ends, fast in the middle, on t⁵, the sharpest of the powers.
  static double inOutQuint(double t) => t < 0.5 ? 16 * _pow(t, 5) : 1 - _pow(-2 * t + 2, 5) / 2;

  /// Starts slow and speeds up, on 2 to the power of t, very sharp.
  static double inExpo(double t) => t == 0 ? 0 : _pow(2, 10 * t - 10);

  /// Starts fast and slows to the end, on 2 to the power of t, very sharp.
  static double outExpo(double t) => t == 1 ? 1 : 1 - _pow(2, -10 * t);

  /// Slow at both ends, fast in the middle, on 2 to the power of t, very sharp.
  static double inOutExpo(double t) => t == 0
      ? 0
      : t == 1
      ? 1
      : t < 0.5
      ? _pow(2, 20 * t - 10) / 2
      : (2 - _pow(2, -20 * t + 10)) / 2;

  /// Starts slow and speeds up, on a quarter circle.
  static double inCirc(double t) => 1 - math.sqrt(1 - t * t);

  /// Starts fast and slows to the end, on a quarter circle.
  static double outCirc(double t) => math.sqrt(1 - (t - 1) * (t - 1));

  /// Slow at both ends, fast in the middle, on a quarter circle.
  static double inOutCirc(double t) =>
      t < 0.5 ? (1 - math.sqrt(1 - 4 * t * t)) / 2 : (math.sqrt(1 - _pow(-2 * t + 2, 2)) + 1) / 2;

  static const _c1 = 1.70158;
  static const _c2 = _c1 * 1.525;
  static const _c3 = _c1 + 1;

  /// Pulls back a little below 0 first, then speeds to the end.
  static double inBack(double t) => _c3 * t * t * t - _c1 * t * t;

  /// Shoots a little past 1, then comes back to it.
  static double outBack(double t) => 1 + _c3 * _pow(t - 1, 3) + _c1 * _pow(t - 1, 2);

  /// Pulls back below 0 at the start and shoots past 1 at the end.
  static double inOutBack(double t) => t < 0.5
      ? (_pow(2 * t, 2) * ((_c2 + 1) * 2 * t - _c2)) / 2
      : (_pow(2 * t - 2, 2) * ((_c2 + 1) * (t * 2 - 2) + _c2) + 2) / 2;

  static const _c4 = 2 * math.pi / 3;
  static const _c5 = 2 * math.pi / 4.5;

  /// Springs back and forth with growing swings, then snaps to the end.
  static double inElastic(double t) => t == 0 || t == 1 ? t : -_pow(2, 10 * t - 10) * math.sin((t * 10 - 10.75) * _c4);

  /// Snaps past the end and springs back and forth until it settles.
  static double outElastic(double t) => t == 0 || t == 1 ? t : _pow(2, -10 * t) * math.sin((t * 10 - 0.75) * _c4) + 1;

  /// Springs at the start and at the end.
  static double inOutElastic(double t) => t == 0 || t == 1
      ? t
      : t < 0.5
      ? -(_pow(2, 20 * t - 10) * math.sin((20 * t - 11.125) * _c5)) / 2
      : (_pow(2, -20 * t + 10) * math.sin((20 * t - 11.125) * _c5)) / 2 + 1;

  /// Bounces at the start, as a ball bouncing in reverse.
  static double inBounce(double t) => 1 - outBounce(1 - t);

  /// Bounces at the end, as a dropped ball.
  static double outBounce(double t) {
    const n = 7.5625, d = 2.75;
    if (t < 1 / d) return n * t * t;
    if (t < 2 / d) return n * (t -= 1.5 / d) * t + 0.75;
    if (t < 2.5 / d) return n * (t -= 2.25 / d) * t + 0.9375;
    return n * (t -= 2.625 / d) * t + 0.984375;
  }

  /// Bounces at the start and at the end.
  static double inOutBounce(double t) => t < 0.5 ? (1 - outBounce(1 - 2 * t)) / 2 : (1 + outBounce(2 * t - 1)) / 2;

  static double _pow(double x, num n) => math.pow(x, n).toDouble();
}

/// A running tween, or a [GameObject.sequence] or [GameObject.together] of
/// them: what [GameObject.tween], [GameObject.moveTo] and the rest return.
///
/// It's a future too: `await moveTo(...)` in a routine goes on in the
/// routines step of the frame it ends in (or of the next frame, for one
/// that ends in the frame it was made). It completes when the motion
/// ends, however it ends: done, [stop]ped, or with its owner; it never
/// fails.
///
/// It belongs to the object or behaviour that made it: it stops when that
/// one is destroyed or removed, and holds still while it's hidden or
/// disabled, as routines do. It moves from the frame after the one it was
/// made in, in the world's routines step: after every update, before the
/// routines' waits end (docs/stage-4.md §3.2).
sealed class Motion implements Future<void> {
  Motion._(
    this._world,
    this._owner, {
    required this.delay,
    required this.loops,
    required this.yoyo,
    required this.unscaled,
    this.onComplete,
  }) : _made = _world.time._frame {
    if (!(delay >= 0 && delay.isFinite)) throw ArgumentError.value(delay, 'delay', 'must be finite and 0 or more');
    if (loops == 0 || loops < -1) throw ArgumentError.value(loops, 'loops', 'must be 1 or more, or -1 for ever');
  }

  final World _world;
  final _Moving _owner; // the GameObject or Behaviour that made it
  final int _made;

  /// Seconds before it starts moving, and reads what it moves from.
  final double delay;

  /// How many times it plays: 1 by default, -1 for ever.
  final int loops;

  /// Whether every other play goes back, from the end to the start, rather
  /// than starting over.
  final bool yoyo;

  /// Whether it runs on real time ([Time.unscaledDelta]), as a pause
  /// menu's does, rather than the game's. A part of a group runs on its
  /// group's.
  final bool unscaled;

  /// Called when it ends by itself or by [complete], not when it's
  /// [stop]ped; for a part of a group, each time the group plays it to
  /// its end.
  final void Function()? onComplete;

  _Group? _group;
  var _state = _MotionState.running;
  final _done = Completer<void>.sync();

  // Its time, for one the world moves on; the play it's in and the time
  // that play started at; whether it has begun (read what it moves from)
  // and whether it's at its very end.
  double _time = 0;
  int _cycle = 0;
  double _cycleStart = 0;
  bool _begun = false;
  bool _atEnd = false;
  bool _lastPlay = false;

  // As a part of a group: the time it was last put at, and whether it
  // broke (it threw, or its owner went) so the group skips it.
  double _last = -1;
  bool _broken = false;

  /// Whether it's still going.
  bool get isRunning => _state == _MotionState.running;

  /// Whether it ended by itself, or by [complete].
  bool get isCompleted => _state == _MotionState.completed;

  /// Whether it was stopped, by [stop] or with its owner.
  bool get isStopped => _state == _MotionState.stopped;

  /// How long one play lasts, in seconds; null until it's known (one that
  /// moves at a speed knows when it begins).
  double? get _pass;

  /// Its whole length, with its delay and every play: null while that
  /// isn't known, infinity for one that loops for ever.
  double? get _total {
    final pass = _pass;
    if (pass == null) return null;
    if (loops < 0) return pass == 0 ? delay : double.infinity;
    return delay + pass * loops;
  }

  /// Stops it where it is. It completes, without [onComplete]. Stopping a
  /// part of a group stops the group; stopping one that has ended does
  /// nothing.
  void stop() {
    if (_group != null) return _root.stop();
    if (!isRunning) return;
    _state = _MotionState.stopped;
    _ended();
  }

  /// Jumps it to its end, as if its time had run out, and calls
  /// [onComplete]; one that loops for ever ends at the end of the play
  /// it's in. Completing a part of a group completes the group.
  void complete() {
    if (_group != null) return _root.complete();
    if (!isRunning) return;
    _lastPlay = true;
    try {
      _seek(double.infinity);
    } catch (error, stack) {
      _world._report(_owner._ownerObject, error, stack);
    }
    _finish();
  }

  Motion get _root => _group?._root ?? this;

  /// Stops it because its owner went: a part of a group just drops out.
  void _halt() {
    if (_group == null) return stop();
    _broken = true;
    _owner._motions?.remove(this);
  }

  /// Moves its time on by [dt]: for one the world moves on.
  void _step(double dt) {
    _time += dt;
    _seek(_time);
    if (_atEnd) _finish();
  }

  void _finish() {
    if (!isRunning) return;
    _state = _MotionState.completed;
    _call(onComplete);
    _ended();
  }

  void _call(void Function()? callback) {
    if (callback == null) return;
    try {
      callback();
    } catch (error, stack) {
      _world._report(_owner._ownerObject, error, stack);
    }
  }

  void _ended() {
    _owner._motions?.remove(this);
    if (!_done.isCompleted) _done.complete();
    if (this case final _Group g) {
      for (final part in g._parts) {
        part._state = _state;
        part._ended();
      }
    }
  }

  /// Puts what it moves where it is [t] seconds in (its delay included):
  /// the play it's in, forwards or, for a yoyo's, back.
  void _seek(double t) {
    if (!_begun) {
      if (t < delay) return;
      _begun = true;
      _begin();
    }
    final local = t - delay;
    while (true) {
      final pass = _pass;
      final into = local - _cycleStart;
      if (pass == null) {
        // Its first play, while its length isn't known: forwards.
        _seekPass(into);
        if (_pass == null) return;
        continue;
      }
      if (into < 0 && _cycle > 0) {
        // Back into an earlier play (a part played back).
        _cycle--;
        _cycleStart -= pass;
        continue;
      }
      final back = yoyo && _cycle.isOdd;
      // A play of no length ends as it begins, however many loops it has.
      final last = _lastPlay || (loops >= 0 && _cycle >= loops - 1) || pass == 0;
      // The very end of a play belongs to it, not to the next.
      if (into < pass || (into == pass && !last)) {
        _atEnd = false;
        final at = into < 0 ? 0.0 : into;
        _seekPass(back ? pass - at : at);
        return;
      }
      if (last) {
        _seekPass(back ? 0 : pass);
        _atEnd = true;
        return;
      }
      _cycle++;
      _cycleStart += pass;
      // Starting over (not a yoyo's turn): a group ends this play, and its
      // parts begin again from where they are.
      if (!yoyo) _startOver(pass);
    }
  }

  /// Reads what it moves from: once, when it first begins.
  void _begin();

  /// Puts what it moves where it is [into] seconds into one play.
  void _seekPass(double into);

  /// Plays again from the start, after a play [pass] seconds long: a
  /// group's parts begin again.
  void _startOver(double pass) {}

  /// Puts a part of a group at [t], once for each time.
  void _seekPart(double t) {
    if (_broken || t == _last) return;
    _last = t;
    final wasAtEnd = _atEnd;
    try {
      _seek(t);
    } catch (error, stack) {
      _world._report(_owner._ownerObject, error, stack);
      _broken = true;
      return;
    }
    if (_atEnd && !wasAtEnd) {
      _call(onComplete);
    }
  }

  /// Back to before it began, for a group that starts over.
  void _rewind() {
    _begun = false;
    _atEnd = false;
    _cycle = 0;
    _cycleStart = 0;
    _last = -1;
    if (this case final _Group g) g._restart();
  }

  @override
  Future<R> then<R>(FutureOr<R> Function(void value) onValue, {Function? onError}) {
    final routine = _currentRoutine;
    if (routine != null && isRunning && identical(routine._world, _world)) {
      // One of the routine's waits: it goes on in the routines step, and a
      // stopped routine unwinds from it.
      return _addWait(
        (r, frame) => _Wait(r, frame, test: () => !isRunning),
        'a motion',
      ).then(onValue, onError: onError);
    }
    return _done.future.then(onValue, onError: onError);
  }

  @override
  Future<void> catchError(Function onError, {bool Function(Object error)? test}) =>
      _done.future.catchError(onError, test: test);

  @override
  Future<void> whenComplete(FutureOr<void> Function() action) => then((_) => action());

  @override
  Future<void> timeout(Duration timeLimit, {FutureOr<void> Function()? onTimeout}) =>
      _done.future.timeout(timeLimit, onTimeout: onTimeout);

  @override
  Stream<void> asStream() => _done.future.asStream();
}

/// Motion on objects and behaviours (docs/stage-4.md §3.7): tweens and
/// their helpers, and moving things by hand.
mixin _Moving {
  /// The object moved.
  GameObject get _self;

  /// Who owns its motions: the object, or the behaviour.
  _Moving get _motionOwner => this;

  // What it made and started that's still running, and the signals it
  // listens to with `on`: a GameObject's or a Behaviour's.
  List<Motion>? _motions;
  List<Routine<Object?>>? _routines;
  _Listening? _listening;

  /// Its object: itself, or the behaviour's (null before it has one).
  GameObject? get _ownerObject;

  /// Whether what it runs holds still: the object hidden, or the
  /// behaviour disabled.
  bool get _holdsStill;

  M _own<M extends Motion>(M motion) {
    (_motions ??= []).add(motion);
    motion._world._motions.add(motion);
    return motion;
  }

  World get _motionWorld {
    final self = _self;
    final owner = _motionOwner;
    if (owner is Behaviour && owner._removed) throw StateError('$owner was removed, so it can\'t start a motion');
    if (self._destroyed) throw StateError('$self was destroyed, so it can\'t start a motion');
    return self.world;
  }

  // --- Moving by hand ---

  /// Where the object is in the world, as Unity's `transform.position`.
  /// Setting it moves the object there, whatever its parents are.
  /// [localPosition] is the same, counted from its parent.
  Vec3 get position => _self.worldTransform.position;
  set position(Vec3 value) => _setPosition(value);

  /// How the object is turned in the world: its parents' rotations and
  /// its own. Setting it turns the object so, whatever its parents are.
  /// [localRotation] is the same, counted from its parent.
  Quat get rotation => _self.worldTransform.rotation;
  set rotation(Quat value) => _setRotation(value);

  /// Where the object is relative to its parent (the world, for a root):
  /// its [GameObject.transform]'s position.
  Vec3 get localPosition => _self.transform.position;
  set localPosition(Vec3 value) => _setLocalPosition(value);

  /// How the object is turned relative to its parent.
  Quat get localRotation => _self.transform.rotation;
  set localRotation(Quat value) => _setLocalRotation(value);

  /// How big the object is relative to its parent. There is no world
  /// scale to set: see [lossyScale].
  Vec3 get localScale => _self.transform.scale;
  set localScale(Vec3 value) => _setScale(value);

  /// How big the object is in the world: its parents' scales times its
  /// own. Approximate when a parent with an uneven scale has a rotated
  /// child, as Unity's.
  Vec3 get lossyScale => _self.worldTransform.scale;

  /// Turns the object [angle] radians around [axis]: an axis of its own
  /// by default (`rotate(Vec3.up, a)` turns it about its own up), or of the
  /// world's with `relativeTo: Space.world`.
  void rotate(Vec3 axis, double angle, {Space relativeTo = Space.self}) {
    final turn = Quat.axisAngle(axis.normalized, angle);
    switch (relativeTo) {
      case Space.self:
        _setLocalRotation((_self.transform.rotation * turn).normalized);
      case Space.world:
        _setRotation((turn * _self.worldTransform.rotation).normalized);
    }
  }

  /// Moves the object by [offset]: along its own axes by default
  /// (`translate(Vec3.forward)` moves it a metre the way it looks, its
  /// scale aside), or the world's with `relativeTo: Space.world`.
  void translate(Vec3 offset, {Space relativeTo = Space.self}) {
    final world = _self.worldTransform;
    _setPosition(world.position + (relativeTo == Space.self ? world.rotation.rotate(offset) : offset));
  }

  /// The way the object looks, in the world: its -Z.
  Vec3 get forward => _self.worldTransform.rotation.rotate(Vec3.forward);

  /// The object's right, in the world: its +X.
  Vec3 get right => _self.worldTransform.rotation.rotate(Vec3.right);

  /// The object's up, in the world: its +Y.
  Vec3 get up => _self.worldTransform.rotation.rotate(Vec3.up);

  /// Turns the object to look at [point], in the world, with its up as
  /// close to [worldUp] as it can be. Throws an [ArgumentError] when it's
  /// at [point] already.
  void lookAt(Vec3 point, {Vec3 worldUp = Vec3.up}) => _setRotation(_rotationToLookAt(point, worldUp));

  Quat _rotationToLookAt(Vec3 point, Vec3 worldUp) =>
      Quat.lookRotation(point - _self.worldTransform.position, up: worldUp);

  /// [point], given in the object's own space, in the world's: where a
  /// point on it is.
  Vec3 transformPoint(Vec3 point) {
    final t = _self.worldTransform;
    return t.position + t.rotation.rotate(point.scaledBy(t.scale));
  }

  /// [point], given in the world, in the object's own space: the other
  /// way from [transformPoint].
  Vec3 inverseTransformPoint(Vec3 point) {
    final Transform(position: o, rotation: r, scale: s) = _self.worldTransform;
    // r.inverse.rotate(point - o) / s, in numbers so that only the result
    // is made: it runs for every move of a child (see Quat.rotate).
    final vx = point.x - o.x, vy = point.y - o.y, vz = point.z - o.z;
    final x = -r.x, y = -r.y, z = -r.z, w = r.w;
    final tx = (y * vz - z * vy) * 2, ty = (z * vx - x * vz) * 2, tz = (x * vy - y * vx) * 2;
    return Vec3(
      (vx + tx * w + (y * tz - z * ty)) / s.x,
      (vy + ty * w + (z * tx - x * tz)) / s.y,
      (vz + tz * w + (x * ty - y * tx)) / s.z,
    );
  }

  // --- Tweens ---

  /// Takes a value from [from] to [to] over [seconds], or at [speed] units
  /// a second (give one), calling [onUpdate] with it each frame:
  ///
  /// ```dart
  /// tween(0.0, 1.0, seconds: 0.5, onUpdate: (v) => light.intensity = v);
  /// ```
  ///
  /// It knows `double`, `Vec2`, `Vec3`, `Quat` (the short way round) and
  /// `Color`; give [lerp] for anything else. [ease] shapes it (see [Ease]:
  /// out-quad by default, as DOTween's). It waits [delay] seconds first,
  /// plays [loops] times (-1 for ever), every other one back for a
  /// [yoyo], on real time when [unscaled]; [onComplete] is called when it
  /// ends. It's owned by this object or behaviour: see [Motion].
  Motion tween<T>(
    T from,
    T to, {
    required void Function(T value) onUpdate,
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    T Function(T a, T b, double t)? lerp,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) {
    final (l, d) = lerp != null ? (lerp, null) : _builtIn<T>(from, '$T');
    return _value<T>(
      () => from,
      (_) => to,
      l,
      d,
      onUpdate,
      seconds: seconds,
      speed: speed,
      ease: ease,
      delay: delay,
      loops: loops,
      yoyo: yoyo,
      unscaled: unscaled,
      onComplete: onComplete,
    );
  }

  Motion _value<T>(
    T Function() from,
    T Function(T from) to,
    T Function(T a, T b, double t) lerp,
    double Function(T a, T b)? distance,
    void Function(T value) apply, {
    required double? seconds,
    required double? speed,
    required EaseFunction ease,
    required double delay,
    required int loops,
    required bool yoyo,
    required bool unscaled,
    required void Function()? onComplete,
  }) => _own(
    _Tween<T>(
      _motionWorld,
      _motionOwner,
      from: from,
      to: to,
      lerp: lerp,
      apply: apply,
      distance: distance,
      seconds: seconds,
      speed: speed,
      ease: ease,
      delay: delay,
      loops: loops,
      yoyo: yoyo,
      unscaled: unscaled,
      onComplete: onComplete,
    ),
  );

  /// Plays [motions] one after another: each starts where the last left
  /// what it moves. They must be made in this frame, as the arguments of
  /// this call usually are, and none may loop for ever. The sequence is a
  /// [Motion] of its own, with its own [delay], [loops], [yoyo] (plays
  /// them all back, the last first) and [onComplete]:
  ///
  /// ```dart
  /// sequence([moveTo(a, seconds: 1), scaleTo(Vec3.one * 2, seconds: 0.3)], loops: -1, yoyo: true);
  /// ```
  Motion sequence(
    Iterable<Motion> motions, {
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _own(
    _Group(
      _motionWorld,
      _motionOwner,
      motions,
      together: false,
      delay: delay,
      loops: loops,
      yoyo: yoyo,
      unscaled: unscaled,
      onComplete: onComplete,
    ),
  );

  /// Plays [motions] all at once, as one [Motion] that lasts as long as
  /// the longest: see [sequence].
  Motion together(
    Iterable<Motion> motions, {
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _own(
    _Group(
      _motionWorld,
      _motionOwner,
      motions,
      together: true,
      delay: delay,
      loops: loops,
      yoyo: yoyo,
      unscaled: unscaled,
      onComplete: onComplete,
    ),
  );

  // --- Helpers ---

  /// Moves the object to [position] in the world over [seconds], or at
  /// [speed] units a second, from wherever it is when it begins (Unity's
  /// `DOMove`). The rest is as [tween]'s. [localMoveTo] counts from the
  /// parent, so it moves with it.
  Motion moveTo(
    Vec3 position, {
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _value<Vec3>(
    () => _self.worldTransform.position,
    (_) => position,
    Vec3.lerp,
    _distance3,
    _setPosition,
    seconds: seconds,
    speed: speed,
    ease: ease,
    delay: delay,
    loops: loops,
    yoyo: yoyo,
    unscaled: unscaled,
    onComplete: onComplete,
  );

  /// Moves the object by [offset], in the world, from wherever it is when
  /// it begins: see [moveTo].
  Motion moveBy(
    Vec3 offset, {
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _value<Vec3>(
    () => _self.worldTransform.position,
    (from) => from + offset,
    Vec3.lerp,
    _distance3,
    _setPosition,
    seconds: seconds,
    speed: speed,
    ease: ease,
    delay: delay,
    loops: loops,
    yoyo: yoyo,
    unscaled: unscaled,
    onComplete: onComplete,
  );

  /// Turns the object to [rotation] in the world the short way round, over
  /// [seconds] or at [speed] radians a second: see [moveTo].
  /// [localRotateTo] counts from the parent.
  Motion rotateTo(
    Quat rotation, {
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _value<Quat>(
    () => _self.worldTransform.rotation,
    (_) => rotation,
    Quat.slerp,
    _angle,
    _setRotation,
    seconds: seconds,
    speed: speed,
    ease: ease,
    delay: delay,
    loops: loops,
    yoyo: yoyo,
    unscaled: unscaled,
    onComplete: onComplete,
  );

  /// Moves the object to [position] relative to its parent, as
  /// [localPosition] is: see [moveTo].
  Motion localMoveTo(
    Vec3 position, {
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _value<Vec3>(
    () => _self.transform.position,
    (_) => position,
    Vec3.lerp,
    _distance3,
    _setLocalPosition,
    seconds: seconds,
    speed: speed,
    ease: ease,
    delay: delay,
    loops: loops,
    yoyo: yoyo,
    unscaled: unscaled,
    onComplete: onComplete,
  );

  /// Turns the object to [rotation] relative to its parent, as
  /// [localRotation] is: see [rotateTo].
  Motion localRotateTo(
    Quat rotation, {
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _value<Quat>(
    () => _self.transform.rotation,
    (_) => rotation,
    Quat.slerp,
    _angle,
    _setLocalRotation,
    seconds: seconds,
    speed: speed,
    ease: ease,
    delay: delay,
    loops: loops,
    yoyo: yoyo,
    unscaled: unscaled,
    onComplete: onComplete,
  );

  /// Turns the object [angle] radians around [axis] (one of its own, as
  /// [rotate]'s), any number of turns, over [seconds] or at
  /// [speed] radians a second. A coin that spins for ever:
  /// `rotateBy(Vec3.up, 2 * pi, seconds: 1, ease: Ease.linear, loops: -1)`.
  Motion rotateBy(
    Vec3 axis,
    double angle, {
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) {
    final along = axis.normalized;
    var start = Quat.identity;
    return _value<double>(
      () {
        start = _self.transform.rotation;
        return 0.0;
      },
      (_) => angle,
      lerp,
      (a, b) => (b - a).abs(),
      (a) => _setLocalRotation((start * Quat.axisAngle(along, a)).normalized),
      seconds: seconds,
      speed: speed,
      ease: ease,
      delay: delay,
      loops: loops,
      yoyo: yoyo,
      unscaled: unscaled,
      onComplete: onComplete,
    );
  }

  /// Turns the object to look at [point] in the world, as [lookAt] does,
  /// over [seconds] or at [speed] radians a second: see [moveTo]. Where to
  /// look is worked out when it begins.
  Motion lookAtOver(
    Vec3 point, {
    Vec3 worldUp = Vec3.up,
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _value<Quat>(
    () => _self.worldTransform.rotation,
    (_) => _rotationToLookAt(point, worldUp),
    Quat.slerp,
    _angle,
    _setRotation,
    seconds: seconds,
    speed: speed,
    ease: ease,
    delay: delay,
    loops: loops,
    yoyo: yoyo,
    unscaled: unscaled,
    onComplete: onComplete,
  );

  /// Sizes the object to [scale] relative to its parent, as [localScale]
  /// is (Unity's `DOScale`): see [moveTo].
  Motion scaleTo(
    Vec3 scale, {
    double? seconds,
    double? speed,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) => _value<Vec3>(
    () => _self.transform.scale,
    (_) => scale,
    Vec3.lerp,
    _distance3,
    _setScale,
    seconds: seconds,
    speed: speed,
    ease: ease,
    delay: delay,
    loops: loops,
    yoyo: yoyo,
    unscaled: unscaled,
    onComplete: onComplete,
  );

  /// Takes the object's colour to [color] over [seconds] (it has no
  /// `speed:`): see [moveTo].
  /// That's its [MeshRenderer]'s own [MeshRenderer.color], from the
  /// material's if it has none, so the other objects that share the
  /// material keep theirs. Give [material] to change that material
  /// instead, for everything that uses it. Throws a [StateError] when
  /// there's no renderer with a material.
  Motion colorTo(
    Color color, {
    required double seconds,
    Material? material,
    EaseFunction ease = Ease.outQuad,
    double delay = 0,
    int loops = 1,
    bool yoyo = false,
    bool unscaled = false,
    void Function()? onComplete,
  }) {
    if (material != null) {
      return _value<Color>(
        () => material.color,
        (_) => color,
        Color.lerp,
        null,
        (c) => material.color = c,
        seconds: seconds,
        speed: null,
        ease: ease,
        delay: delay,
        loops: loops,
        yoyo: yoyo,
        unscaled: unscaled,
        onComplete: onComplete,
      );
    }
    final renderer = _self.get<MeshRenderer>();
    final shared = renderer?.material ?? (throw StateError('$_self has no MeshRenderer with a material to colour'));
    return _value<Color>(
      () => renderer!.color ?? shared.color,
      (_) => color,
      Color.lerp,
      null,
      (c) => renderer!.color = c,
      seconds: seconds,
      speed: null,
      ease: ease,
      delay: delay,
      loops: loops,
      yoyo: yoyo,
      unscaled: unscaled,
      onComplete: onComplete,
    );
  }

  /// Swells the object's scale by [punch] and springs it back, [vibrato]
  /// swings that fade over [seconds], ending at the scale it began with:
  /// a hit, or a pickup. `punchScale(Vec3.one * 0.3, seconds: 0.4)`. It
  /// takes [seconds] only, no `speed:`.
  Motion punchScale(
    Vec3 punch, {
    required double seconds,
    int vibrato = 6,
    double delay = 0,
    int loops = 1,
    bool unscaled = false,
    void Function()? onComplete,
  }) {
    var base = Vec3.one;
    return _value<double>(
      () {
        base = _self.transform.scale;
        return 0.0;
      },
      (_) => 1.0,
      lerp,
      null,
      (p) => _setScale(base + punch * (math.sin(p * math.pi * vibrato) * (1 - p))),
      seconds: seconds,
      speed: null,
      ease: Ease.linear,
      delay: delay,
      loops: loops,
      yoyo: false,
      unscaled: unscaled,
      onComplete: onComplete,
    );
  }

  /// Shakes the object about where it is, up to [strength] along each
  /// axis, [frequency] times a second, fading over [seconds] and ending
  /// where it began. [seed] makes the same shake each time. It takes
  /// [seconds] only, no `speed:`.
  Motion shake(
    double strength, {
    required double seconds,
    double frequency = 20,
    int? seed,
    double delay = 0,
    int loops = 1,
    bool unscaled = false,
    void Function()? onComplete,
  }) {
    final random = math.Random(seed);
    final phase = Vec3(random.nextDouble(), random.nextDouble(), random.nextDouble()) * (2 * math.pi);
    var base = Vec3.zero;
    final turns = 2 * math.pi * frequency * seconds;
    return _value<double>(
      () {
        base = _self.transform.position;
        return 0.0;
      },
      (_) => 1.0,
      lerp,
      null,
      (p) {
        final fade = strength * (1 - p);
        final wobble = Vec3(
          math.sin(turns * p + phase.x),
          math.sin(turns * 1.13 * p + phase.y),
          math.sin(turns * 0.89 * p + phase.z),
        );
        _setLocalPosition(base + wobble * fade);
      },
      seconds: seconds,
      speed: null,
      ease: Ease.linear,
      delay: delay,
      loops: loops,
      yoyo: false,
      unscaled: unscaled,
      onComplete: onComplete,
    );
  }

  // What the helpers use, rather than [position] and the rest, which a
  // game's class may have overridden with its own. The world's position
  // and rotation are turned into the parent's here.
  void _setPosition(Vec3 v) {
    final self = _self;
    final parent = self._above;
    if (parent == null) return _setLocalPosition(v);
    final before = self._described == null && identical(self._worldLocal, self.transform) ? self._worldTransform : null;
    final above = self._worldAbove;
    _setLocalPosition(parent.inverseTransformPoint(v));
    // Its world transform is known, so the next read needn't work it out:
    // the position set, turned and sized as it was.
    if (before != null && identical(above, parent.worldTransform)) {
      self._worldLocal = self.transform;
      self._worldTransform = Transform._(v, before.rotation, before.scale);
    }
  }

  void _setRotation(Quat q) {
    final parent = _self._above;
    _setLocalRotation(parent == null ? q : (parent.worldTransform.rotation.inverse * q).normalized);
  }

  void _setLocalPosition(Vec3 v) => _self.transform = _self.transform.copyWith(position: v);
  void _setLocalRotation(Quat q) => _self.transform = _self.transform.copyWith(rotation: q);
  void _setScale(Vec3 v) => _self.transform = _self.transform.copyWith(scale: v);
}

double _distance3(Vec3 a, Vec3 b) => (b - a).length;
