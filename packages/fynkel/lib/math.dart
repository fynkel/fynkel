/// Fynkel-owned immutable math value types (ADR-0013). Right-handed, Y-up,
/// metres, radians; quaternions are `[x, y, z, w]`, matching `glam`.
///
/// M1 provides `Vec3` and `Quat` (what the wire records need), M3 `Vec4`
/// (material colours); `Vec2`, `Mat4` and `Transform3` follow with the
/// gameplay API.
library;

import 'dart:math' as math;

class Vec3 {
  const Vec3(this.x, this.y, this.z);
  const Vec3.all(double v) : this(v, v, v);

  static const Vec3 zero = Vec3(0, 0, 0);
  static const Vec3 one = Vec3(1, 1, 1);
  static const Vec3 up = Vec3(0, 1, 0);
  static const Vec3 right = Vec3(1, 0, 0);
  static const Vec3 forward = Vec3(0, 0, -1);

  final double x, y, z;

  Vec3 operator +(Vec3 o) => Vec3(x + o.x, y + o.y, z + o.z);
  Vec3 operator -(Vec3 o) => Vec3(x - o.x, y - o.y, z - o.z);
  Vec3 operator -() => Vec3(-x, -y, -z);
  Vec3 operator *(double s) => Vec3(x * s, y * s, z * s);
  Vec3 operator /(double s) => Vec3(x / s, y / s, z / s);

  double dot(Vec3 o) => x * o.x + y * o.y + z * o.z;
  Vec3 cross(Vec3 o) => Vec3(y * o.z - z * o.y, z * o.x - x * o.z, x * o.y - y * o.x);
  double get lengthSquared => dot(this);
  double get length => math.sqrt(lengthSquared);
  Vec3 get normalized {
    final l = length;
    return l == 0 ? zero : this / l;
  }

  Vec3 lerp(Vec3 o, double t) => this + (o - this) * t;
  bool get isFinite => x.isFinite && y.isFinite && z.isFinite;

  @override
  bool operator ==(Object other) => other is Vec3 && other.x == x && other.y == y && other.z == z;
  @override
  int get hashCode => Object.hash(x, y, z);
  @override
  String toString() => 'Vec3($x, $y, $z)';
}

/// Four components; also an RGBA colour (linear, `x y z w` = `r g b a`).
class Vec4 {
  const Vec4(this.x, this.y, this.z, this.w);
  const Vec4.all(double v) : this(v, v, v, v);

  static const Vec4 zero = Vec4(0, 0, 0, 0);
  static const Vec4 one = Vec4(1, 1, 1, 1);

  final double x, y, z, w;

  Vec4 operator +(Vec4 o) => Vec4(x + o.x, y + o.y, z + o.z, w + o.w);
  Vec4 operator -(Vec4 o) => Vec4(x - o.x, y - o.y, z - o.z, w - o.w);
  Vec4 operator -() => Vec4(-x, -y, -z, -w);
  Vec4 operator *(double s) => Vec4(x * s, y * s, z * s, w * s);
  Vec4 operator /(double s) => Vec4(x / s, y / s, z / s, w / s);

  double dot(Vec4 o) => x * o.x + y * o.y + z * o.z + w * o.w;
  Vec3 get xyz => Vec3(x, y, z);
  Vec4 lerp(Vec4 o, double t) => this + (o - this) * t;
  bool get isFinite => x.isFinite && y.isFinite && z.isFinite && w.isFinite;

  @override
  bool operator ==(Object other) =>
      other is Vec4 && other.x == x && other.y == y && other.z == z && other.w == w;
  @override
  int get hashCode => Object.hash(x, y, z, w);
  @override
  String toString() => 'Vec4($x, $y, $z, $w)';
}

class Quat {
  const Quat(this.x, this.y, this.z, this.w);

  static const Quat identity = Quat(0, 0, 0, 1);

  /// Rotation of [radians] about unit [axis].
  factory Quat.axisAngle(Vec3 axis, double radians) {
    final h = radians / 2;
    final s = math.sin(h);
    return Quat(axis.x * s, axis.y * s, axis.z * s, math.cos(h));
  }

  final double x, y, z, w;

  /// Hamilton product: `this * o` applies `o` first, then `this`.
  Quat operator *(Quat o) => Quat(
        w * o.x + x * o.w + y * o.z - z * o.y,
        w * o.y - x * o.z + y * o.w + z * o.x,
        w * o.z + x * o.y - y * o.x + z * o.w,
        w * o.w - x * o.x - y * o.y - z * o.z,
      );

  Quat get conjugate => Quat(-x, -y, -z, w);
  double get length => math.sqrt(x * x + y * y + z * z + w * w);
  Quat get normalized {
    final l = length;
    return l == 0 ? identity : Quat(x / l, y / l, z / l, w / l);
  }

  Vec3 rotate(Vec3 v) {
    // v' = q * v * q^-1 for unit q, expanded.
    final u = Vec3(x, y, z);
    final t = u.cross(v) * 2;
    return v + t * w + u.cross(t);
  }

  bool get isFinite => x.isFinite && y.isFinite && z.isFinite && w.isFinite;

  @override
  bool operator ==(Object other) =>
      other is Quat && other.x == x && other.y == y && other.z == z && other.w == w;
  @override
  int get hashCode => Object.hash(x, y, z, w);
  @override
  String toString() => 'Quat($x, $y, $z, $w)';
}
