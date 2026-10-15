import 'dart:math' as math;

/// A point or direction in 2D, such as a texture coordinate: (0, 0) is the
/// image's top-left corner, with y growing downwards.
final class Vec2 {
  /// The point ([x], [y]).
  const Vec2(this.x, this.y);

  /// The x component.
  final double x;

  /// The y component.
  final double y;

  /// (0, 0).
  static const zero = Vec2(0, 0);

  /// (1, 1).
  static const one = Vec2(1, 1);

  /// This plus [o], component by component.
  Vec2 operator +(Vec2 o) => Vec2(x + o.x, y + o.y);

  /// This minus [o], component by component.
  Vec2 operator -(Vec2 o) => Vec2(x - o.x, y - o.y);

  /// This times [s]: as long, [s] times.
  Vec2 operator *(double s) => Vec2(x * s, y * s);

  /// This divided by [s].
  Vec2 operator /(double s) => Vec2(x / s, y / s);

  /// This the other way round.
  Vec2 operator -() => Vec2(-x, -y);

  /// The dot product with [o].
  double dot(Vec2 o) => x * o.x + y * o.y;

  /// How long it is (Unity's `magnitude`).
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
  /// The point ([x], [y], [z]).
  const Vec3(this.x, this.y, this.z);

  /// ([s], [s], [s]): a uniform scale, as `scale: Vec3.all(2)`.
  const Vec3.all(double s) : x = s, y = s, z = s;

  /// The x component: +X is right.
  final double x;

  /// The y component: +Y is up.
  final double y;

  /// The z component: +Z is back, towards a camera that isn't rotated.
  final double z;

  /// (0, 0, 0).
  static const zero = Vec3(0, 0, 0);

  /// (1, 1, 1): the scale that leaves things as they are.
  static const one = Vec3(1, 1, 1);

  /// +Y.
  static const up = Vec3(0, 1, 0);

  /// -Y.
  static const down = Vec3(0, -1, 0);

  /// +X.
  static const right = Vec3(1, 0, 0);

  /// -X.
  static const left = Vec3(-1, 0, 0);

  /// -Z: the way an object (and a camera) looks when it isn't rotated.
  static const forward = Vec3(0, 0, -1);

  /// +Z: the opposite of [forward].
  static const back = Vec3(0, 0, 1);

  /// This plus [o], component by component.
  Vec3 operator +(Vec3 o) => Vec3(x + o.x, y + o.y, z + o.z);

  /// This minus [o], component by component.
  Vec3 operator -(Vec3 o) => Vec3(x - o.x, y - o.y, z - o.z);

  /// This the other way round.
  Vec3 operator -() => Vec3(-x, -y, -z);

  /// This times [s]: as long, [s] times.
  Vec3 operator *(double s) => Vec3(x * s, y * s, z * s);

  /// This divided by [s].
  Vec3 operator /(double s) => Vec3(x / s, y / s, z / s);

  /// Each component times [o]'s: a scale applied to a point.
  Vec3 scaledBy(Vec3 o) => Vec3(x * o.x, y * o.y, z * o.z);

  /// The dot product with [o]: the lengths multiplied by the cosine of
  /// the angle between them, so 0 when they're at right angles.
  double dot(Vec3 o) => x * o.x + y * o.y + z * o.z;

  /// The cross product with [o]: at right angles to both, right-handed
  /// (`Vec3.right.cross(Vec3.up)` is [back]).
  Vec3 cross(Vec3 o) => Vec3(y * o.z - z * o.y, z * o.x - x * o.z, x * o.y - y * o.x);

  /// How long it is (Unity's `magnitude`).
  double get length => math.sqrt(dot(this));

  /// It with [x] in place of its own.
  Vec3 withX(double x) => Vec3(x, y, z);

  /// It with [y] in place of its own: `position.withY(0.5)` keeps x and z.
  Vec3 withY(double y) => Vec3(x, y, z);

  /// It with [z] in place of its own.
  Vec3 withZ(double z) => Vec3(x, y, z);

  /// It on the ground, y = 0: `(target - position).flat` is the way to
  /// walk to a target above or below.
  Vec3 get flat => Vec3(x, 0, z);

  /// This direction with length 1. Throws for a zero vector, which has no
  /// direction, so a vector that should never be zero says so where it is;
  /// for one that can be, such as the way to a target the object may have
  /// reached, use [normalizedOrZero].
  Vec3 get normalized {
    final l = length;
    if (l < 1e-12 || !l.isFinite) throw ArgumentError('$this has no direction');
    return this / l;
  }

  /// This direction with length 1, or [zero] for a zero vector (as Unity's
  /// `normalized`): `(target - position).normalizedOrZero * speed` stops
  /// the frame the two meet.
  Vec3 get normalizedOrZero {
    final l = length;
    return l < 1e-12 || !l.isFinite ? zero : this / l;
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

  /// The x component of the axis part: the axis times the sine of half
  /// the angle turned.
  final double x;

  /// The y component of the axis part.
  final double y;

  /// The z component of the axis part.
  final double z;

  /// The angle part: the cosine of half the angle turned.
  final double w;

  /// No rotation.
  static const identity = Quat(0, 0, 0, 1);

  /// Turns [angle] radians around [axis] (counter-clockwise looking down
  /// the axis, so a positive turn around [Vec3.up] goes from -Z towards -X).
  factory Quat.axisAngle(Vec3 axis, double angle) {
    final n = axis.normalized;
    final s = math.sin(angle / 2);
    return Quat(n.x * s, n.y * s, n.z * s, math.cos(angle / 2));
  }

  /// Turns by angles, in radians, each counter-clockwise around its axis
  /// as [Quat.axisAngle] does: [roll] around Z first, then [pitch] around
  /// X (positive looks up), then [yaw] around Y (positive turns left), the
  /// order of Unity's `Quaternion.Euler`. Radians, unlike Unity's, which
  /// takes degrees: write `Quat.euler(0, 90.deg, 0)`.
  factory Quat.euler(double pitch, double yaw, double roll) =>
      (Quat.axisAngle(Vec3.up, yaw) * Quat.axisAngle(Vec3.right, pitch) * Quat.axisAngle(Vec3.back, roll)).normalized;

  /// The rotation that makes an object look along [forward] (its -Z turns
  /// to [forward]) with its +Y as close to [up] as possible. If [forward]
  /// is straight along [up], -Z is used as up instead, so looking straight
  /// down still works.
  ///
  /// [forward] is a direction, not a point: to look from `eye` at
  /// `target`, give `target - eye`.
  factory Quat.lookRotation(Vec3 forward, {Vec3 up = Vec3.up}) {
    final l = forward.length;
    if (l < 1e-12 || !l.isFinite) {
      throw ArgumentError(
        'Quat.lookRotation($forward): $forward is no direction. It takes the way to face, not a point to '
        'look at: to look from eye at target, give it target - eye.',
      );
    }
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
  Vec3 rotate(Vec3 v) => _rotate(x, y, z, w, v);

  // v + w t + q × t, where t = 2 q × v, worked out in numbers so that only
  // the result is made: it runs for every move of a child.
  static Vec3 _rotate(double x, double y, double z, double w, Vec3 v) {
    final tx = (y * v.z - z * v.y) * 2, ty = (z * v.x - x * v.z) * 2, tz = (x * v.y - y * v.x) * 2;
    return Vec3(v.x + tx * w + (y * tz - z * ty), v.y + ty * w + (z * tx - x * tz), v.z + tz * w + (x * ty - y * tx));
  }

  /// The opposite rotation.
  Quat get inverse => Quat(-x, -y, -z, w);

  /// The dot product with [o], component by component: near 1 or -1 when
  /// the rotations are nearly the same.
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

  /// 1 for a rotation; see [normalized].
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
/// with [Camera.screenPointToRay] to find what's under a point in a view, and
/// cast it with [World.raycast].
final class Ray {
  /// [direction] is made unit length; throws for a zero one.
  factory Ray(Vec3 origin, Vec3 direction) => Ray._(origin, direction.normalized);

  const Ray._(this.origin, this.direction);

  /// Where it starts, in world space.
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
/// `normal.dot(p) + distance == 0`.
final class Plane {
  /// The plane through [point] facing [normal] (made unit length).
  factory Plane(Vec3 normal, Vec3 point) {
    final n = normal.normalized;
    return Plane._(n, -n.dot(point));
  }

  const Plane._(this.normal, this.distance);

  /// The ground: y = 0, facing up.
  static const ground = Plane._(Vec3.up, 0);

  /// Unit length.
  final Vec3 normal;

  /// How far the origin is above the plane, along [normal] (below,
  /// negative), as Unity's: the plane y = 3 facing up has -3.
  final double distance;

  /// How far [point] is above the plane (below, negative).
  double distanceTo(Vec3 point) => normal.dot(point) + distance;

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
/// the view's size: what [Camera.screenPointToRay] needs to aim a ray.
/// [Finger.position] and [Input.mousePosition] give pointer positions as
/// these.
final class ViewPoint {
  /// The point ([x], [y]) in a view [width] by [height], all in logical
  /// pixels.
  const ViewPoint(this.x, this.y, {required this.width, required this.height});

  /// Logical pixels from the view's left edge.
  final double x;

  /// Logical pixels from the view's top edge, growing downwards.
  final double y;

  /// The view's width, in the same logical pixels.
  final double width;

  /// The view's height, in the same logical pixels.
  final double height;

  @override
  bool operator ==(Object other) =>
      other is ViewPoint && other.x == x && other.y == y && other.width == width && other.height == height;

  @override
  int get hashCode => Object.hash(x, y, width, height);

  /// Its [x] and [y], as for a trail drawn on the screen.
  Vec2 toVec2() => Vec2(x, y);

  @override
  String toString() => 'ViewPoint($x, $y in ${width}x$height)';
}

/// Angles are radians everywhere in Fynkel, as in Dart's `math`: `90.deg`
/// writes one in degrees, as `Quat.euler(0, 90.deg, 0)` or
/// `JointLimits(min: -45.deg)`, and `angle.inDegrees` reads one back.
extension Degrees on num {
  /// This many degrees, in radians: `180.deg` is pi.
  double get deg => this * (math.pi / 180);

  /// This many radians, in degrees: `math.pi.inDegrees` is 180.
  double get inDegrees => this * (180 / math.pi);
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
  /// A follower that takes about [smoothTime] seconds to arrive, never
  /// faster than [maxSpeed] units a second (no limit unless given).
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
  /// A follower that takes about [smoothTime] seconds to arrive, never
  /// faster than `maxSpeed` units a second (no limit unless given).
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
