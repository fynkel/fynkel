import 'dart:math' as math;

/// A point or direction in 2D, such as a texture coordinate: (0, 0) is the
/// image's top-left corner, with y growing downwards.
final class Vec2 {
  const Vec2(this.x, this.y);

  final double x;
  final double y;

  static const zero = Vec2(0, 0);
  static const one = Vec2(1, 1);

  Vec2 operator +(Vec2 o) => Vec2(x + o.x, y + o.y);
  Vec2 operator -(Vec2 o) => Vec2(x - o.x, y - o.y);
  Vec2 operator *(double s) => Vec2(x * s, y * s);
  Vec2 operator /(double s) => Vec2(x / s, y / s);
  Vec2 operator -() => Vec2(-x, -y);

  double dot(Vec2 o) => x * o.x + y * o.y;
  double get length => math.sqrt(x * x + y * y);

  /// The point [t] of the way from [a] to [b]: [a] at 0, [b] at 1, and
  /// beyond them outside 0–1.
  static Vec2 lerp(Vec2 a, Vec2 b, double t) => Vec2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);

  /// [current] moved towards [target] by at most [maxDistance], never past
  /// it: `position = Vec2.moveTowards(position, goal, speed * time.delta)`.
  static Vec2 moveTowards(Vec2 current, Vec2 target, double maxDistance) {
    final d = target - current;
    final l = d.length;
    return l <= maxDistance || l == 0 ? target : current + d * (maxDistance / l);
  }

  @override
  bool operator ==(Object other) => other is Vec2 && other.x == x && other.y == y;

  @override
  int get hashCode => Object.hash(x, y);

  @override
  String toString() => 'Vec2($x, $y)';
}

/// A point or direction in 3D. Fynkel is right-handed with +Y up, and an
/// object looks along its -Z ([Vec3.forward]).
final class Vec3 {
  const Vec3(this.x, this.y, this.z);

  final double x;
  final double y;
  final double z;

  static const zero = Vec3(0, 0, 0);
  static const one = Vec3(1, 1, 1);
  static const up = Vec3(0, 1, 0);
  static const down = Vec3(0, -1, 0);
  static const right = Vec3(1, 0, 0);
  static const left = Vec3(-1, 0, 0);

  /// -Z: the way an object (and a camera) looks when it isn't rotated.
  static const forward = Vec3(0, 0, -1);
  static const back = Vec3(0, 0, 1);

  Vec3 operator +(Vec3 o) => Vec3(x + o.x, y + o.y, z + o.z);
  Vec3 operator -(Vec3 o) => Vec3(x - o.x, y - o.y, z - o.z);
  Vec3 operator -() => Vec3(-x, -y, -z);
  Vec3 operator *(double s) => Vec3(x * s, y * s, z * s);
  Vec3 operator /(double s) => Vec3(x / s, y / s, z / s);

  /// Each component times [o]'s: a scale applied to a point.
  Vec3 scaledBy(Vec3 o) => Vec3(x * o.x, y * o.y, z * o.z);

  double dot(Vec3 o) => x * o.x + y * o.y + z * o.z;
  Vec3 cross(Vec3 o) => Vec3(y * o.z - z * o.y, z * o.x - x * o.z, x * o.y - y * o.x);
  double get length => math.sqrt(dot(this));

  /// This direction with length 1. Throws for a zero vector, which has no
  /// direction.
  Vec3 get normalized {
    final l = length;
    if (l < 1e-12 || !l.isFinite) throw ArgumentError.value(this, 'vector', 'has no direction');
    return this / l;
  }

  /// Whether every component is within [tolerance] of [o]'s.
  bool closeTo(Vec3 o, [double tolerance = 1e-6]) =>
      (x - o.x).abs() <= tolerance && (y - o.y).abs() <= tolerance && (z - o.z).abs() <= tolerance;

  /// The point [t] of the way from [a] to [b]: [a] at 0, [b] at 1, and
  /// beyond them outside 0–1.
  static Vec3 lerp(Vec3 a, Vec3 b, double t) =>
      Vec3(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t);

  /// [current] moved towards [target] by at most [maxDistance], never past
  /// it: `position = Vec3.moveTowards(position, goal, speed * time.delta)`.
  static Vec3 moveTowards(Vec3 current, Vec3 target, double maxDistance) {
    final d = target - current;
    final l = d.length;
    return l <= maxDistance || l == 0 ? target : current + d * (maxDistance / l);
  }

  @override
  bool operator ==(Object other) => other is Vec3 && other.x == x && other.y == y && other.z == z;

  @override
  int get hashCode => Object.hash(x, y, z);

  @override
  String toString() => 'Vec3($x, $y, $z)';
}

/// A rotation, as a unit quaternion. Angles are in radians.
final class Quat {
  /// The raw components. Prefer [Quat.axisAngle] and [Quat.lookRotation];
  /// the engine refuses a quaternion whose length isn't 1.
  const Quat(this.x, this.y, this.z, this.w);

  final double x;
  final double y;
  final double z;
  final double w;

  static const identity = Quat(0, 0, 0, 1);

  /// Turns [angle] radians around [axis] (counter-clockwise looking down
  /// the axis, so a positive turn around [Vec3.up] goes from -Z towards -X).
  factory Quat.axisAngle(Vec3 axis, double angle) {
    final n = axis.normalized;
    final s = math.sin(angle / 2);
    return Quat(n.x * s, n.y * s, n.z * s, math.cos(angle / 2));
  }

  /// The rotation that makes an object look along [forward] (its -Z turns
  /// to [forward]) with its +Y as close to [up] as possible. If [forward]
  /// is straight along [up], -Z is used as up instead, so looking straight
  /// down still works.
  factory Quat.lookRotation(Vec3 forward, {Vec3 up = Vec3.up}) {
    final back = -forward.normalized;
    var side = up.cross(back);
    if (side.length < 1e-6) side = Vec3.forward.cross(back);
    final right = side.normalized;
    final upright = back.cross(right);
    return _fromBasis(right, upright, back);
  }

  /// The rotation whose matrix has these (orthonormal, right-handed) columns.
  static Quat _fromBasis(Vec3 r, Vec3 u, Vec3 b) {
    final trace = r.x + u.y + b.z;
    if (trace > 0) {
      final s = math.sqrt(trace + 1) * 2;
      return Quat((u.z - b.y) / s, (b.x - r.z) / s, (r.y - u.x) / s, s / 4);
    } else if (r.x > u.y && r.x > b.z) {
      final s = math.sqrt(1 + r.x - u.y - b.z) * 2;
      return Quat(s / 4, (u.x + r.y) / s, (b.x + r.z) / s, (u.z - b.y) / s);
    } else if (u.y > b.z) {
      final s = math.sqrt(1 + u.y - r.x - b.z) * 2;
      return Quat((u.x + r.y) / s, s / 4, (b.y + u.z) / s, (b.x - r.z) / s);
    } else {
      final s = math.sqrt(1 + b.z - r.x - u.y) * 2;
      return Quat((b.x + r.z) / s, (b.y + u.z) / s, s / 4, (r.y - u.x) / s);
    }
  }

  /// Both rotations: `a * b` turns by `b` first, then by `a`, as with
  /// matrices.
  Quat operator *(Quat o) => Quat(
    w * o.x + x * o.w + y * o.z - z * o.y,
    w * o.y - x * o.z + y * o.w + z * o.x,
    w * o.z + x * o.y - y * o.x + z * o.w,
    w * o.w - x * o.x - y * o.y - z * o.z,
  );

  /// [v] turned by this rotation.
  Vec3 rotate(Vec3 v) {
    final q = Vec3(x, y, z);
    final t = q.cross(v) * 2;
    return v + t * w + q.cross(t);
  }

  /// The opposite rotation.
  Quat get inverse => Quat(-x, -y, -z, w);

  double dot(Quat o) => x * o.x + y * o.y + z * o.z + w * o.w;

  /// The angle between this rotation and [o], in radians: 0 to pi.
  double angleTo(Quat o) => 2 * math.acos(math.min(1, dot(o).abs()));

  /// The rotation [t] of the way from [a] to [b], turning the short way
  /// round at an even speed (spherical interpolation): [a] at 0, [b] at 1.
  static Quat slerp(Quat a, Quat b, double t) {
    var d = a.dot(b);
    // q and -q are the same rotation: take the one nearer, for the short way.
    final s = d < 0 ? -1.0 : 1.0;
    d *= s;
    double ka, kb;
    if (d > 0.9995) {
      // Nearly the same: a straight line, normalized, is as good.
      ka = 1 - t;
      kb = t;
    } else {
      final theta = math.acos(d);
      final sin = math.sin(theta);
      ka = math.sin((1 - t) * theta) / sin;
      kb = math.sin(t * theta) / sin;
    }
    kb *= s;
    return Quat(a.x * ka + b.x * kb, a.y * ka + b.y * kb, a.z * ka + b.z * kb, a.w * ka + b.w * kb).normalized;
  }

  double get length => math.sqrt(x * x + y * y + z * z + w * w);

  /// This rotation with length exactly 1 again. Many multiplied rotations
  /// drift from 1; the engine refuses a drift above 1e-3.
  Quat get normalized {
    final l = length;
    return Quat(x / l, y / l, z / l, w / l);
  }

  /// Whether this and [o] are the same rotation within [tolerance]
  /// (q and -q are the same rotation).
  bool closeTo(Quat o, [double tolerance = 1e-6]) {
    final sign = (x * o.x + y * o.y + z * o.z + w * o.w) < 0 ? -1 : 1;
    return (x - sign * o.x).abs() <= tolerance &&
        (y - sign * o.y).abs() <= tolerance &&
        (z - sign * o.z).abs() <= tolerance &&
        (w - sign * o.w).abs() <= tolerance;
  }

  @override
  bool operator ==(Object other) => other is Quat && other.x == x && other.y == y && other.z == z && other.w == w;

  @override
  int get hashCode => Object.hash(x, y, z, w);

  @override
  String toString() => 'Quat($x, $y, $z, $w)';
}

/// A half-line in the world: from [origin], along [direction]. Make one
/// with [GameObject.screenToRay] to find what's under a point in a view, and
/// cast it with [World.raycast].
final class Ray {
  /// [direction] is made unit length; throws for a zero one.
  factory Ray(Vec3 origin, Vec3 direction) => Ray._(origin, direction.normalized);

  const Ray._(this.origin, this.direction);

  final Vec3 origin;

  /// Unit length.
  final Vec3 direction;

  /// The point [distance] along the ray.
  Vec3 at(double distance) => origin + direction * distance;

  @override
  bool operator ==(Object other) => other is Ray && other.origin == origin && other.direction == direction;

  @override
  int get hashCode => Object.hash(origin, direction);

  @override
  String toString() => 'Ray($origin, $direction)';
}

/// A flat surface without end, as Unity's `Plane`: the points [p] where
/// `normal.dot(p) == distance`.
final class Plane {
  /// The plane through [point] facing [normal] (made unit length).
  factory Plane(Vec3 normal, Vec3 point) {
    final n = normal.normalized;
    return Plane._(n, n.dot(point));
  }

  const Plane._(this.normal, this.distance);

  /// The ground: y = 0, facing up.
  static const ground = Plane._(Vec3.up, 0);

  /// Unit length.
  final Vec3 normal;

  /// From the origin along [normal] to the plane.
  final double distance;

  /// How far [point] is above the plane (below, negative).
  double distanceTo(Vec3 point) => normal.dot(point) - distance;

  /// How far along [ray] it meets the plane; null when it runs along the
  /// plane or away from it.
  double? raycast(Ray ray) {
    final towards = normal.dot(ray.direction);
    if (towards.abs() < 1e-12) return null;
    final along = -distanceTo(ray.origin) / towards;
    return along >= 0 ? along : null;
  }

  @override
  bool operator ==(Object other) => other is Plane && other.normal == normal && other.distance == distance;

  @override
  int get hashCode => Object.hash(normal, distance);

  @override
  String toString() => 'Plane($normal, $distance)';
}

/// A point in a view, in logical pixels from its top-left corner, with
/// the view's size: what [GameObject.screenToRay] needs to aim a ray.
/// package:fynkel_flutter's `FrameInput` gives pointer positions as these.
final class ViewPoint {
  const ViewPoint(this.x, this.y, {required this.width, required this.height});

  final double x;
  final double y;

  /// The view's size, in the same logical pixels.
  final double width;
  final double height;

  @override
  bool operator ==(Object other) =>
      other is ViewPoint && other.x == x && other.y == y && other.width == width && other.height == height;

  @override
  int get hashCode => Object.hash(x, y, width, height);

  @override
  String toString() => 'ViewPoint($x, $y in ${width}x$height)';
}

// Unity's Mathf, for moving things by hand.

/// The value [t] of the way from [a] to [b]: [a] at 0, [b] at 1, and
/// beyond them outside 0–1 (Unity's `LerpUnclamped`; clamp [t] for its
/// `Lerp`).
double lerp(double a, double b, double t) => a + (b - a) * t;

/// How far [value] is from [a] to [b]: 0 at [a], 1 at [b], beyond outside
/// them; 0 when [a] and [b] are the same.
double inverseLerp(double a, double b, double value) => a == b ? 0 : (value - a) / (b - a);

/// [value] kept within 0 and 1.
double clamp01(double value) => value < 0 ? 0 : (value > 1 ? 1 : value);

/// [current] moved towards [target] by at most [maxDelta], never past it.
double moveTowards(double current, double target, double maxDelta) {
  final d = target - current;
  return d.abs() <= maxDelta ? target : current + (d < 0 ? -maxDelta : maxDelta);
}

/// [t] wrapped into 0 up to [length]: 0, 1, 2, 0, 1, 2... for a length of
/// 3, as a clock's hand goes round. Negative [t] wraps too.
double repeat(double t, double length) => t - (t / length).floorToDouble() * length;

/// [t] going from 0 up to [length] and back again: 0, 1, 2, 1, 0, 1...
double pingPong(double t, double length) {
  final r = repeat(t, length * 2);
  return length - (r - length).abs();
}

final _random = math.Random();

/// A point anywhere inside the sphere of radius 1, each as likely.
Vec3 randomInSphere([math.Random? random]) {
  final r = random ?? _random;
  while (true) {
    final p = Vec3(r.nextDouble() * 2 - 1, r.nextDouble() * 2 - 1, r.nextDouble() * 2 - 1);
    if (p.dot(p) <= 1) return p;
  }
}

/// A point on the circle of radius 1, each direction as likely.
Vec2 randomOnCircle([math.Random? random]) {
  final a = (random ?? _random).nextDouble() * 2 * math.pi;
  return Vec2(math.cos(a), math.sin(a));
}

/// Follows a moving target smoothly, as a spring that never overshoots
/// (Unity's `Mathf.SmoothDamp`), keeping its own speed from frame to
/// frame:
///
/// ```dart
/// final follow = SmoothDamp(smoothTime: 0.3);
/// // each frame:
/// height = follow.step(height, target, time.delta);
/// ```
final class SmoothDamp {
  SmoothDamp({required this.smoothTime, this.maxSpeed = double.infinity});

  /// About how long it takes to reach the target, in seconds.
  double smoothTime;

  /// The fastest it moves, in units a second.
  double maxSpeed;

  /// How fast it's moving now, in units a second.
  double velocity = 0;

  /// [current] moved towards [target] over [dt] seconds.
  double step(double current, double target, double dt) {
    // Unity's: a critically damped spring, by Game Programming Gems 4.
    final st = math.max(0.0001, smoothTime);
    final omega = 2 / st;
    final x = omega * dt;
    final exp = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x);
    var change = current - target;
    final goal = target;
    final maxChange = maxSpeed * st;
    change = change.clamp(-maxChange, maxChange);
    final to = current - change;
    final temp = (velocity + omega * change) * dt;
    velocity = (velocity - omega * temp) * exp;
    var out = to + (change + temp) * exp;
    // Never past the target.
    if ((goal - current > 0) == (out > goal)) {
      out = goal;
      velocity = dt > 0 ? (out - goal) / dt : 0;
    }
    return out;
  }
}

/// [SmoothDamp] for a point: follows a moving target in 3D.
final class SmoothDamp3 {
  SmoothDamp3({required double smoothTime, this._maxSpeed = double.infinity})
    : _x = SmoothDamp(smoothTime: smoothTime),
      _y = SmoothDamp(smoothTime: smoothTime),
      _z = SmoothDamp(smoothTime: smoothTime);

  final SmoothDamp _x, _y, _z;
  final double _maxSpeed;

  /// About how long it takes to reach the target, in seconds.
  double get smoothTime => _x.smoothTime;
  set smoothTime(double value) => _x.smoothTime = _y.smoothTime = _z.smoothTime = value;

  /// How fast it's moving now, in units a second.
  Vec3 get velocity => Vec3(_x.velocity, _y.velocity, _z.velocity);

  /// [current] moved towards [target] over [dt] seconds, at most
  /// `maxSpeed` units a second.
  Vec3 step(Vec3 current, Vec3 target, double dt) {
    var goal = target;
    if (_maxSpeed.isFinite) {
      final reach = _maxSpeed * smoothTime;
      final d = target - current;
      if (d.length > reach) goal = current + d * (reach / d.length);
    }
    return Vec3(_x.step(current.x, goal.x, dt), _y.step(current.y, goal.y, dt), _z.step(current.z, goal.z, dt));
  }
}
