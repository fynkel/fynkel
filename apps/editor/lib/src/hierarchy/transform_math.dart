// The little linear algebra "keep world transform" needs: a `fynkel.Transform`
// (position, rotation quaternion xyzw, scale) as a column-major 4×4 matrix,
// products, the affine inverse, and the decomposition back. Kept local so the
// editor has no math dependency for three functions.
import 'dart:math' as math;

/// A `fynkel.Transform`'s data: `position` [x, y, z], `rotation` [x, y, z, w], `scale` [x, y, z].
final class Trs {
  const Trs({required this.position, required this.rotation, required this.scale});

  static const identity = Trs(position: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1]);

  final List<double> position;
  final List<double> rotation;
  final List<double> scale;

  /// Reads a component's `data`; missing fields take the identity's values.
  static Trs fromData(Map<String, Object?>? data) {
    if (data == null) return identity;
    List<double> vec(String key, List<double> fallback) {
      final v = data[key];
      if (v is List && v.length == fallback.length && v.every((n) => n is num)) {
        return [for (final n in v) (n as num).toDouble()];
      }
      return fallback;
    }

    return Trs(
      position: vec('position', identity.position),
      rotation: vec('rotation', identity.rotation),
      scale: vec('scale', identity.scale),
    );
  }

  /// Column-major 4×4: translation × rotation × scale.
  List<double> toMatrix() {
    final q = _normalized(rotation);
    final x = q[0], y = q[1], z = q[2], w = q[3];
    final xx = x * x, yy = y * y, zz = z * z;
    final xy = x * y, xz = x * z, yz = y * z, wx = w * x, wy = w * y, wz = w * z;
    final r = [
      1 - 2 * (yy + zz), 2 * (xy + wz), 2 * (xz - wy), //
      2 * (xy - wz), 1 - 2 * (xx + zz), 2 * (yz + wx), //
      2 * (xz + wy), 2 * (yz - wx), 1 - 2 * (xx + yy),
    ];
    final sx = scale[0], sy = scale[1], sz = scale[2];
    return [
      r[0] * sx, r[1] * sx, r[2] * sx, 0, //
      r[3] * sy, r[4] * sy, r[5] * sy, 0, //
      r[6] * sz, r[7] * sz, r[8] * sz, 0, //
      position[0], position[1], position[2], 1,
    ];
  }

  /// The TRS whose matrix is [m] (rotation from the scale-normalised
  /// columns; a negative determinant flips the x scale). Values are
  /// rounded to 1e-9 so a round trip through this does not smear `0.5`
  /// into `0.49999999999`.
  static Trs fromMatrix(List<double> m) {
    var sx = _len(m[0], m[1], m[2]);
    final sy = _len(m[4], m[5], m[6]);
    final sz = _len(m[8], m[9], m[10]);
    final det =
        m[0] * (m[5] * m[10] - m[6] * m[9]) - m[4] * (m[1] * m[10] - m[2] * m[9]) + m[8] * (m[1] * m[6] - m[2] * m[5]);
    if (det < 0) sx = -sx;
    double c(int i, double s) => s == 0 ? 0 : m[i] / s;
    final r = [
      c(0, sx), c(1, sx), c(2, sx), //
      c(4, sy), c(5, sy), c(6, sy), //
      c(8, sz), c(9, sz), c(10, sz),
    ];
    // Column-major 3×3 → quaternion (Shepperd's method).
    final trace = r[0] + r[4] + r[8];
    double qx, qy, qz, qw;
    if (trace > 0) {
      final s = math.sqrt(trace + 1) * 2;
      qw = 0.25 * s;
      qx = (r[5] - r[7]) / s;
      qy = (r[6] - r[2]) / s;
      qz = (r[1] - r[3]) / s;
    } else if (r[0] > r[4] && r[0] > r[8]) {
      final s = math.sqrt(1 + r[0] - r[4] - r[8]) * 2;
      qw = (r[5] - r[7]) / s;
      qx = 0.25 * s;
      qy = (r[3] + r[1]) / s;
      qz = (r[6] + r[2]) / s;
    } else if (r[4] > r[8]) {
      final s = math.sqrt(1 + r[4] - r[0] - r[8]) * 2;
      qw = (r[6] - r[2]) / s;
      qx = (r[3] + r[1]) / s;
      qy = 0.25 * s;
      qz = (r[7] + r[5]) / s;
    } else {
      final s = math.sqrt(1 + r[8] - r[0] - r[4]) * 2;
      qw = (r[1] - r[3]) / s;
      qx = (r[6] + r[2]) / s;
      qy = (r[7] + r[5]) / s;
      qz = 0.25 * s;
    }
    var q = _normalized([qx, qy, qz, qw]);
    if (q[3] < 0) q = [-q[0], -q[1], -q[2], -q[3]]; // one canonical sign
    return Trs(
      position: [clean(m[12]), clean(m[13]), clean(m[14])],
      rotation: [for (final v in q) clean(v)],
      scale: [clean(sx), clean(sy), clean(sz)],
    );
  }

  /// The component `data` values to write: integers as integers (`1`, not `1.0`).
  Map<String, Object?> toData() => {
    'position': [for (final v in position) _num(v)],
    'rotation': [for (final v in rotation) _num(v)],
    'scale': [for (final v in scale) _num(v)],
  };

  bool sameAs(Trs o) => _same(position, o.position) && _sameRotation(rotation, o.rotation) && _same(scale, o.scale);

  static bool _same(List<double> a, List<double> b) {
    for (var i = 0; i < a.length; i++) {
      if ((a[i] - b[i]).abs() > 1e-7) return false;
    }
    return true;
  }

  /// q and −q are the same rotation.
  static bool _sameRotation(List<double> a, List<double> b) => _same(a, b) || _same(a, [for (final v in b) -v]);

  static Object _num(double v) {
    final c = clean(v);
    return c == c.roundToDouble() && c.abs() < 1e15 ? c.round() : c;
  }

  /// Rounds to 1e-9 and drops the sign of zero.
  static double clean(double v) {
    final r = (v * 1e9).roundToDouble() / 1e9;
    return r == 0 ? 0 : r;
  }

  static double _len(double x, double y, double z) => math.sqrt(x * x + y * y + z * z);

  static List<double> _normalized(List<double> q) {
    final n = math.sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
    return n == 0 ? const [0, 0, 0, 1] : [q[0] / n, q[1] / n, q[2] / n, q[3] / n];
  }

  @override
  String toString() => 'Trs($position, $rotation, $scale)';
}

/// Column-major 4×4 product `a × b` (apply b, then a).
List<double> matMul(List<double> a, List<double> b) {
  final out = List<double>.filled(16, 0);
  for (var col = 0; col < 4; col++) {
    for (var row = 0; row < 4; row++) {
      var sum = 0.0;
      for (var k = 0; k < 4; k++) {
        sum += a[k * 4 + row] * b[col * 4 + k];
      }
      out[col * 4 + row] = sum;
    }
  }
  return out;
}

const List<double> identityMatrix = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

/// The inverse of an affine column-major matrix (last row 0 0 0 1); the
/// identity for a singular one (a zero scale has no inverse — the entity
/// keeps its local transform then).
List<double> matInverseAffine(List<double> m) {
  final a = m[0], b = m[4], c = m[8];
  final d = m[1], e = m[5], f = m[9];
  final g = m[2], h = m[6], i = m[10];
  final det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
  if (det.abs() < 1e-12) return List.of(identityMatrix);
  final inv = [
    (e * i - f * h) / det, -(d * i - f * g) / det, (d * h - e * g) / det, //
    -(b * i - c * h) / det, (a * i - c * g) / det, -(a * h - b * g) / det, //
    (b * f - c * e) / det, -(a * f - c * d) / det, (a * e - b * d) / det,
  ];
  // inv is row-major 3×3 here: inv[r*3+c]. Column-major output.
  final tx = m[12], ty = m[13], tz = m[14];
  final ntx = -(inv[0] * tx + inv[1] * ty + inv[2] * tz);
  final nty = -(inv[3] * tx + inv[4] * ty + inv[5] * tz);
  final ntz = -(inv[6] * tx + inv[7] * ty + inv[8] * tz);
  return [
    inv[0], inv[3], inv[6], 0, //
    inv[1], inv[4], inv[7], 0, //
    inv[2], inv[5], inv[8], 0, //
    ntx, nty, ntz, 1,
  ];
}
