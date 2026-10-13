part of 'world.dart';

// What runs a motion (motion.dart has the public half): a tween of one
// value, a group of motions, and how a value type is blended.

enum _MotionState { running, completed, stopped }

/// One value going from one end to the other.
final class _Tween<T> extends Motion {
  _Tween(
    super._world,
    super._owner, {
    required T Function() from,
    required T Function(T from) to,
    required this.lerp,
    required this.apply,
    required double? seconds,
    required double? speed,
    this.distance,
    required this.ease,
    required super.delay,
    required super.loops,
    required super.yoyo,
    required super.unscaled,
    super.onComplete,
  }) : _getFrom = from,
       _getTo = to,
       _seconds = seconds,
       _speed = speed,
       super._() {
    if ((seconds == null) == (speed == null)) throw ArgumentError('give one of seconds and speed');
    if (seconds != null && !(seconds >= 0 && seconds.isFinite)) {
      throw ArgumentError.value(seconds, 'seconds', 'must be finite and 0 or more');
    }
    if (speed != null) {
      if (!(speed > 0 && speed.isFinite)) throw ArgumentError.value(speed, 'speed', 'must be finite and more than 0');
      if (distance == null) throw ArgumentError('speed works for double, Vec2, Vec3 and Quat values only');
    }
  }

  final T Function() _getFrom;
  final T Function(T from) _getTo;
  final T Function(T a, T b, double t) lerp;
  final double Function(T a, T b)? distance;
  final void Function(T value) apply;
  final EaseFunction ease;
  final double? _seconds;
  final double? _speed;
  double? _length;
  late T _from;
  late T _to;

  @override
  double? get _pass => _length;

  @override
  void _begin() {
    _from = _getFrom();
    _to = _getTo(_from);
    _length = _seconds ?? distance!(_from, _to) / _speed!;
  }

  @override
  void _seekPass(double into) {
    final pass = _length!;
    apply(lerp(_from, _to, ease(pass == 0 ? 1 : into / pass)));
  }

  @override
  String toString() => 'tween${_begun ? ' from $_from to $_to' : ''}';
}

/// Motions played one after another, or all at once.
final class _Group extends Motion {
  _Group(
    super._world,
    super._owner,
    Iterable<Motion> parts, {
    required this.together,
    required super.delay,
    required super.loops,
    required super.yoyo,
    required super.unscaled,
    super.onComplete,
  }) : _parts = parts.toList(),
       super._() {
    if (_parts.isEmpty) throw ArgumentError.value(parts, 'parts', 'is empty');
    for (final part in _parts) {
      if (!identical(part._world, _world)) throw ArgumentError('$part is in another world');
      if (part._group != null) throw StateError('$part is in a group already; a motion can be in one');
      if (!part.isRunning || part._made != _made) {
        throw StateError('a group takes motions made in the same frame, before they move');
      }
      if (part.loops < 0) throw ArgumentError('a motion that loops for ever never ends, so it can\'t be in a group');
    }
    for (final part in _parts) {
      part._group = this;
    }
  }

  final List<Motion> _parts;
  final bool together;
  double? _length;

  // Where its parts were last put, so it knows which way it goes.
  double _at = -1;

  @override
  double? get _pass => _length;

  @override
  void _begin() {}

  @override
  void _startOver(double pass) {
    _seekPass(pass);
    _restart();
  }

  void _restart() {
    _length = null;
    _at = -1;
    for (final part in _parts) {
      part._rewind();
    }
  }

  @override
  void _seekPass(double into) {
    final backwards = into < _at;
    _at = into;
    final n = _parts.length;
    if (together) {
      var longest = 0.0;
      var known = true;
      // Forwards the later parts win where two move the same thing;
      // backwards the earlier ones.
      for (var k = 0; k < n; k++) {
        final part = _parts[backwards ? n - 1 - k : k];
        final total = part._total;
        part._seekPart(total != null && into > total ? total : into);
        final now = part._total;
        if (now == null) {
          known = false;
        } else if (now > longest) {
          longest = now;
        }
      }
      if (known) _length ??= longest;
      return;
    }
    if (!backwards) {
      var offset = 0.0;
      for (var i = 0; i < n; i++) {
        final part = _parts[i];
        final local = into - offset;
        // The parts after this point wait their turn.
        if (local < 0) return;
        final total = part._total;
        part._seekPart(total != null && local > total ? total : local);
        final now = part._total;
        if (now == null) return;
        offset += now;
      }
      _length ??= offset;
      return;
    }
    // Backwards, every part has played, so each one's length is known:
    // those after this point go back to their starts, the last first.
    var offset = _length!;
    for (var i = n - 1; i >= 0; i--) {
      final part = _parts[i];
      final total = part._total!;
      offset -= total;
      final local = into - offset;
      if (local >= total) return;
      part._seekPart(local < 0 ? 0 : local);
    }
  }

  @override
  String toString() => together ? 'together(${_parts.length} motions)' : 'sequence(${_parts.length} motions)';
}

/// The built-in lerp and distance for [T], or an [ArgumentError].
(T Function(T a, T b, double t), double Function(T a, T b)?) _builtIn<T>(T sample, String type) {
  final Object l, d;
  switch (sample) {
    case double():
      l = lerp;
      d = (double a, double b) => (b - a).abs();
    case Vec2():
      l = Vec2.lerp;
      d = (Vec2 a, Vec2 b) => (b - a).length;
    case Vec3():
      l = Vec3.lerp;
      d = (Vec3 a, Vec3 b) => (b - a).length;
    case Quat():
      l = Quat.slerp;
      d = (Quat a, Quat b) => a.angleTo(b);
    case Color():
      l = Color.lerp;
      d = Object();
    default:
      throw ArgumentError(
        'tween knows double, Vec2, Vec3, Quat and Color; give a lerp for $type'
        '${sample is int ? ' (or write 0.0, not 0)' : ''}',
      );
  }
  if (l is! T Function(T, T, double)) {
    throw ArgumentError('tween knows double, Vec2, Vec3, Quat and Color, not $type; give a lerp for it');
  }
  return (l, d is double Function(T, T) ? d : null);
}

double _angle(Quat a, Quat b) => a.angleTo(b);
