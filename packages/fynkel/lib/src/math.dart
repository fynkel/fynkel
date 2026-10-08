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
/// with [GameObject.rayThrough] to find what's under a point in a view, and
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

/// A point in a view, in logical pixels from its top-left corner, with
/// the view's size: what [GameObject.rayThrough] needs to aim a ray.
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
