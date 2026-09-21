// The Inspector's small conversions: a unit quaternion to and from Euler
// degrees, linear RGB to and from sRGB / hex for the colour picker, and
// the number formatting a document round-trips byte-exactly.
import 'dart:math' as math;

/// Euler angles in degrees, applied as intrinsic X, then Y, then Z
/// (`q = qz · qy · qx`, i.e. the entity pitches about its own X, yaws about
/// the resulting Y, rolls about the resulting Z) — the Unity/Godot reading
/// of "rotation X/Y/Z". Stored as a quaternion `[x, y, z, w]`.
final class Euler {
  const Euler(this.x, this.y, this.z);

  final double x;
  final double y;
  final double z;

  static const zero = Euler(0, 0, 0);

  /// The quaternion `[x, y, z, w]` of these angles (normalised).
  List<double> toQuaternion() {
    final cx = math.cos(_rad(x) / 2), sx = math.sin(_rad(x) / 2);
    final cy = math.cos(_rad(y) / 2), sy = math.sin(_rad(y) / 2);
    final cz = math.cos(_rad(z) / 2), sz = math.sin(_rad(z) / 2);
    // qz * qy * qx
    final qx = sx * cy * cz - cx * sy * sz;
    final qy = cx * sy * cz + sx * cy * sz;
    final qz = cx * cy * sz - sx * sy * cz;
    final qw = cx * cy * cz + sx * sy * sz;
    return normalizeQuaternion([qx, qy, qz, qw]);
  }

  /// The angles of a quaternion `[x, y, z, w]`; at the pole (|pitch about
  /// Y| = 90°) roll is folded into yaw so the result is still exact.
  static Euler fromQuaternion(List<num> q) {
    final n = normalizeQuaternion([for (final v in q) v.toDouble()]);
    final x = n[0], y = n[1], z = n[2], w = n[3];
    // Rotation matrix elements of R = Rz·Ry·Rx.
    final m20 = 2 * (x * z - w * y);
    final m21 = 2 * (y * z + w * x);
    final m22 = 1 - 2 * (x * x + y * y);
    final m10 = 2 * (x * y + w * z);
    final m00 = 1 - 2 * (y * y + z * z);
    final m01 = 2 * (x * y - w * z);
    final sy = -m20.clamp(-1.0, 1.0);
    final ey = math.asin(sy);
    double ex, ez;
    if (sy.abs() < 0.9999999) {
      ex = math.atan2(m21, m22);
      ez = math.atan2(m10, m00);
    } else {
      // Gimbal lock: roll folds into yaw.
      ex = 0;
      ez = math.atan2(-m01, 1 - 2 * (x * x + z * z));
    }
    return Euler(_deg(ex), _deg(ey), _deg(ez));
  }

  /// Degrees rounded to 1e-4, with −0 dropped (what the rows show).
  Euler get clean => Euler(_cleanDeg(x), _cleanDeg(y), _cleanDeg(z));

  Euler withComponent(int i, double v) => switch (i) {
    0 => Euler(v, y, z),
    1 => Euler(x, v, z),
    _ => Euler(x, y, v),
  };

  double operator [](int i) => switch (i) {
    0 => x,
    1 => y,
    _ => z,
  };

  @override
  bool operator ==(Object other) => other is Euler && other.x == x && other.y == y && other.z == z;

  @override
  int get hashCode => Object.hash(x, y, z);

  @override
  String toString() => 'Euler($x°, $y°, $z°)';
}

double _rad(double d) => d * math.pi / 180;
double _deg(double r) => r * 180 / math.pi;
double _cleanDeg(double d) {
  final r = (d * 1e4).roundToDouble() / 1e4;
  return r == 0 ? 0 : r;
}

List<double> normalizeQuaternion(List<double> q) {
  final len = math.sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
  if (len < 1e-12 || !len.isFinite) return [0, 0, 0, 1];
  return [q[0] / len, q[1] / len, q[2] / len, q[3] / len];
}

/// Same rotation (q and −q), within [eps].
bool sameRotation(List<num> a, List<num> b, {double eps = 1e-6}) {
  bool close(int sign) {
    for (var i = 0; i < 4; i++) {
      if ((a[i] - sign * b[i]).abs() > eps) return false;
    }
    return true;
  }

  return a.length == 4 && b.length == 4 && (close(1) || close(-1));
}

// ── Colour ────────────────────────────────────────────────────────────────

/// Linear light `0..1` → sRGB-encoded `0..1`.
double linearToSrgb(double c) {
  final v = c.clamp(0.0, 1.0);
  return v <= 0.0031308 ? v * 12.92 : 1.055 * math.pow(v, 1 / 2.4) - 0.055;
}

/// sRGB-encoded `0..1` → linear light `0..1`.
double srgbToLinear(double c) {
  final v = c.clamp(0.0, 1.0);
  return v <= 0.04045 ? v / 12.92 : math.pow((v + 0.055) / 1.055, 2.4).toDouble();
}

/// A linear RGB(A) list as `#RRGGBB` (or `#RRGGBBAA` when four components).
String linearRgbToHex(List<num> rgb) {
  String h(double v) => (linearToSrgb(v) * 255).round().clamp(0, 255).toRadixString(16).padLeft(2, '0');
  final r = h(rgb[0].toDouble()), g = h(rgb[1].toDouble()), b = h(rgb[2].toDouble());
  final a = rgb.length > 3 ? (rgb[3].toDouble().clamp(0.0, 1.0) * 255).round().toRadixString(16).padLeft(2, '0') : '';
  return '#$r$g$b$a'.toUpperCase();
}

/// `#RGB`, `#RRGGBB`, `#RRGGBBAA` (with or without `#`) → linear RGB(A);
/// null when it is not a colour.
List<double>? hexToLinearRgb(String text) {
  var s = text.trim();
  if (s.startsWith('#')) s = s.substring(1);
  if (s.length == 3) s = s.split('').map((c) => '$c$c').join();
  if (s.length != 6 && s.length != 8) return null;
  final n = int.tryParse(s, radix: 16);
  if (n == null) return null;
  double ch(int shift) => srgbToLinear(((n >> shift) & 0xff) / 255);
  if (s.length == 6) return [ch(16), ch(8), ch(0)];
  return [ch(24), ch(16), ch(8), ((n) & 0xff) / 255];
}

/// sRGB `0..1` components → HSV (`h` in degrees `0..360`, `s`, `v` in `0..1`).
(double, double, double) rgbToHsv(double r, double g, double b) {
  final max = math.max(r, math.max(g, b)), min = math.min(r, math.min(g, b));
  final d = max - min;
  var h = 0.0;
  if (d > 1e-9) {
    if (max == r) {
      h = 60 * (((g - b) / d) % 6);
    } else if (max == g) {
      h = 60 * ((b - r) / d + 2);
    } else {
      h = 60 * ((r - g) / d + 4);
    }
  }
  if (h < 0) h += 360;
  final s = max <= 1e-9 ? 0.0 : d / max;
  return (h, s, max);
}

/// HSV → sRGB `0..1`.
(double, double, double) hsvToRgb(double h, double s, double v) {
  final c = v * s;
  final hh = (h % 360) / 60;
  final x = c * (1 - ((hh % 2) - 1).abs());
  final (r1, g1, b1) = switch (hh.floor()) {
    0 => (c, x, 0.0),
    1 => (x, c, 0.0),
    2 => (0.0, c, x),
    3 => (0.0, x, c),
    4 => (x, 0.0, c),
    _ => (c, 0.0, x),
  };
  final m = v - c;
  return (r1 + m, g1 + m, b1 + m);
}

// ── Numbers ───────────────────────────────────────────────────────────────

/// The JSON number to write for [v] where the document held [previous]:
/// an integral value stays an `int` when the field was one, and the
/// text form is rounded to 1e-9 so `0.1 + 0.2` does not leak. A
/// `number`-typed field that held `1` (no fraction) stays `1` — that is
/// how the document's canonical writer keeps the bytes stable.
Object jsonNumber(double v, {Object? previous, bool integer = false}) {
  final r = (v * 1e9).roundToDouble() / 1e9;
  final c = r == 0 ? 0.0 : r;
  if (integer) return c.round();
  if (c == c.roundToDouble() && c.abs() < 1e15 && (previous == null || previous is int)) return c.round();
  return c;
}

/// Parses what the user typed into a number field: a plain number, or a
/// short arithmetic expression (`+ - * /`, parentheses, unary minus).
double? parseNumber(String text) {
  final t = text.trim().replaceAll(',', '.');
  if (t.isEmpty) return null;
  final direct = double.tryParse(t);
  if (direct != null) return direct.isFinite ? direct : null;
  try {
    final v = _Expr(t).parse();
    return v != null && v.isFinite ? v : null;
  } catch (_) {
    return null;
  }
}

/// A tiny recursive-descent evaluator for `1 + 2 * (3 - 0.5)`.
final class _Expr {
  _Expr(this.s);
  final String s;
  int i = 0;

  double? parse() {
    final v = _sum();
    _ws();
    return i == s.length ? v : null;
  }

  void _ws() {
    while (i < s.length && s[i] == ' ') {
      i++;
    }
  }

  double _sum() {
    var v = _product();
    while (true) {
      _ws();
      if (i < s.length && (s[i] == '+' || s[i] == '-')) {
        final op = s[i++];
        final r = _product();
        v = op == '+' ? v + r : v - r;
      } else {
        return v;
      }
    }
  }

  double _product() {
    var v = _unary();
    while (true) {
      _ws();
      if (i < s.length && (s[i] == '*' || s[i] == '/')) {
        final op = s[i++];
        final r = _unary();
        v = op == '*' ? v * r : v / r;
      } else {
        return v;
      }
    }
  }

  double _unary() {
    _ws();
    if (i < s.length && s[i] == '-') {
      i++;
      return -_unary();
    }
    if (i < s.length && s[i] == '+') {
      i++;
      return _unary();
    }
    return _atom();
  }

  double _atom() {
    _ws();
    if (i < s.length && s[i] == '(') {
      i++;
      final v = _sum();
      _ws();
      if (i >= s.length || s[i] != ')') throw const FormatException('missing )');
      i++;
      return v;
    }
    final start = i;
    while (i < s.length &&
        (RegExp(r'[0-9.eE]').hasMatch(s[i]) ||
            ((s[i] == '-' || s[i] == '+') && i > start && (s[i - 1] == 'e' || s[i - 1] == 'E')))) {
      i++;
    }
    final v = double.tryParse(s.substring(start, i));
    if (v == null) throw FormatException('not a number at $start');
    return v;
  }
}
