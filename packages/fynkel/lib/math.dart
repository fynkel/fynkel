/// Fynkel-owned immutable math value types (ADR-0013). Right-handed, Y-up,
/// metres, radians (Euler angles alone are degrees, as the Inspector shows
/// them); quaternions are `[x, y, z, w]`, matching `glam`.
///
/// M1 provides `Vec3` and `Quat` (what the wire records need), M3 `Vec4`
/// (material colours) and `Vec2` (input axes); `Mat4` and `Transform3`
/// follow with the gameplay API.
library;

import 'dart:math' as math;

class Vec2 {
  const Vec2(this.x, this.y);
  const Vec2.all(double v) : this(v, v);

  static const Vec2 zero = Vec2(0, 0);
  static const Vec2 one = Vec2(1, 1);

  final double x, y;

  Vec2 operator +(Vec2 o) => Vec2(x + o.x, y + o.y);
  Vec2 operator -(Vec2 o) => Vec2(x - o.x, y - o.y);
  Vec2 operator -() => Vec2(-x, -y);
  Vec2 operator *(double s) => Vec2(x * s, y * s);
  Vec2 operator /(double s) => Vec2(x / s, y / s);

  double dot(Vec2 o) => x * o.x + y * o.y;
  double get lengthSquared => dot(this);
  double get length => math.sqrt(lengthSquared);
  Vec2 get normalized {
    final l = length;
    return l == 0 ? zero : this / l;
  }

  /// This vector, or its direction when it is longer than one (a stick
  /// pushed diagonally, two keys held).
  Vec2 get clamped => lengthSquared > 1 ? normalized : this;

  Vec2 lerp(Vec2 o, double t) => this + (o - this) * t;
  bool get isFinite => x.isFinite && y.isFinite;

  @override
  bool operator ==(Object other) => other is Vec2 && other.x == x && other.y == y;
  @override
  int get hashCode => Object.hash(x, y);
  @override
  String toString() => 'Vec2($x, $y)';
}

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

  /// Component by component (Unity's `Vector3.Scale`).
  Vec3 scaled(Vec3 o) => Vec3(x * o.x, y * o.y, z * o.z);
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

  /// Euler angles in **degrees** (the one place angles are not radians:
  /// they are what the Inspector's Rotation row shows, ADR-0013 amendment
  /// 2026-09-24), applied as the Inspector applies them — about X, then
  /// the resulting Y, then the resulting Z (`qz * qy * qx`).
  factory Quat.euler(Vec3 degrees) {
    const r = math.pi / 360; // half-angle, in radians per degree
    final cx = math.cos(degrees.x * r), sx = math.sin(degrees.x * r);
    final cy = math.cos(degrees.y * r), sy = math.sin(degrees.y * r);
    final cz = math.cos(degrees.z * r), sz = math.sin(degrees.z * r);
    return Quat(
      sx * cy * cz - cx * sy * sz,
      cx * sy * cz + sx * cy * sz,
      cx * cy * sz - sx * sy * cz,
      cx * cy * cz + sx * sy * sz,
    );
  }

  /// The rotation whose forward ([Vec3.forward], −Z) points along
  /// [forward] with its up as near [up] as it can be (Unity's
  /// `Quaternion.LookRotation`). A zero [forward] is [identity]; an [up]
  /// along [forward] falls back to another axis.
  factory Quat.lookRotation(Vec3 forward, [Vec3 up = Vec3.up]) {
    final f = forward.normalized;
    if (f == Vec3.zero) return identity;
    final back = -f;
    var right = up.cross(back);
    if (right.lengthSquared < 1e-12) {
      right = (back.y.abs() < 0.9 ? Vec3.up : Vec3.right).cross(back);
    }
    right = right.normalized;
    final u = back.cross(right);
    // The matrix with columns right, u, back, to a quaternion.
    final m00 = right.x, m11 = u.y, m22 = back.z;
    final trace = m00 + m11 + m22;
    if (trace > 0) {
      final s = math.sqrt(trace + 1) * 2;
      return Quat((u.z - back.y) / s, (back.x - right.z) / s, (right.y - u.x) / s, s / 4);
    }
    if (m00 > m11 && m00 > m22) {
      final s = math.sqrt(1 + m00 - m11 - m22) * 2;
      return Quat(s / 4, (u.x + right.y) / s, (back.x + right.z) / s, (u.z - back.y) / s);
    }
    if (m11 > m22) {
      final s = math.sqrt(1 + m11 - m00 - m22) * 2;
      return Quat((u.x + right.y) / s, s / 4, (back.y + u.z) / s, (back.x - right.z) / s);
    }
    final s = math.sqrt(1 + m22 - m00 - m11) * 2;
    return Quat((back.x + right.z) / s, (back.y + u.z) / s, s / 4, (right.y - u.x) / s);
  }

  final double x, y, z, w;

  /// The Euler angles of [Quat.euler], in degrees within `[0, 360)` as
  /// Unity reports them; at the pole (Y = ±90°) the X turn folds into Z.
  Vec3 get eulerAngles {
    final n = normalized;
    final x = n.x, y = n.y, z = n.z, w = n.w;
    final m20 = 2 * (x * z - w * y);
    final sy = (-m20).clamp(-1.0, 1.0);
    final ey = math.asin(sy);
    double ex, ez;
    if (sy.abs() < 0.9999999) {
      ex = math.atan2(2 * (y * z + w * x), 1 - 2 * (x * x + y * y));
      ez = math.atan2(2 * (x * y + w * z), 1 - 2 * (y * y + z * z));
    } else {
      ex = 0;
      ez = math.atan2(-2 * (x * y - w * z), 1 - 2 * (x * x + z * z));
    }
    double deg(double r) {
      final d = r * 180 / math.pi % 360;
      return d >= 360 - 1e-9 ? 0 : d;
    }

    return Vec3(deg(ex), deg(ey), deg(ez));
  }

  /// Hamilton product: `this * o` applies `o` first, then `this`.
  Quat operator *(Quat o) => Quat(
        w * o.x + x * o.w + y * o.z - z * o.y,
        w * o.y - x * o.z + y * o.w + z * o.x,
        w * o.z + x * o.y - y * o.x + z * o.w,
        w * o.w - x * o.x - y * o.y - z * o.z,
      );

  Quat get conjugate => Quat(-x, -y, -z, w);

  /// The opposite rotation ([conjugate] of the normalised quaternion).
  Quat get inverse => normalized.conjugate;
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
