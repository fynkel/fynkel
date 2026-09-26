// A blend tree's weights in the editor (ADR-0033 § 1), for the
// Inspector's map: the same methods as the runtime's
// (`crates/fynkel_animation/src/blend_tree.rs`), so the colour around a
// child is the weight the game gives it. The tests hold both to the same
// hand-worked values.
import 'dart:math' as math;

/// Unity's five blend tree types, as the document writes them.
const treeTypes = ['1d', 'simpleDirectional', 'freeformDirectional', 'freeformCartesian', 'direct'];

/// How many parameters a type reads.
int treeParameterCount(String type) => switch (type) {
  '1d' => 1,
  'direct' => 0,
  _ => 2,
};

bool treeIs2d(String type) => treeParameterCount(type) == 2;

/// Unity's names for the types.
String treeTypeName(String type) => switch (type) {
  '1d' => '1D',
  'simpleDirectional' => '2D Simple Directional',
  'freeformDirectional' => '2D Freeform Directional',
  'freeformCartesian' => '2D Freeform Cartesian',
  'direct' => 'Direct',
  _ => type,
};

/// Weights under this are not sampled.
const minWeight = 0.001;

const _eps = 1e-6;

/// Each child's weight at [input] (1D reads its x), for a type other than
/// `direct`. Sums to 1; empty for no children.
List<double> treeWeights(String type, List<(double, double)> positions, (double, double) input) {
  final out = List<double>.filled(positions.length, 0);
  if (positions.isEmpty) return out;
  if (positions.length == 1) return out..[0] = 1;
  switch (type) {
    case '1d':
      _oneD(positions, input.$1, out);
    case 'simpleDirectional':
      if (!_simpleDirectional(positions, input, out)) _gradientBand(positions, input, out);
    case 'freeformCartesian':
      _gradientBand(positions, input, out);
    case 'freeformDirectional':
      _polarGradientBand(positions, input, out);
    default:
      out.fillRange(0, out.length, 1);
  }
  _finish(positions, input, out);
  return out;
}

/// A direct tree's weights: each child's parameter value (negative counts
/// as 0), normalized; all zero gives the first child.
List<double> directWeights(List<double> values) {
  final out = [for (final v in values) v.isFinite ? math.max(0.0, v) : 0.0];
  final sum = out.fold(0.0, (a, b) => a + b);
  if (sum <= 0) {
    if (out.isNotEmpty) out[0] = 1;
    return out;
  }
  for (var i = 0; i < out.length; i++) {
    out[i] /= sum;
  }
  _dropSmall(out);
  return out;
}

void _oneD(List<(double, double)> positions, double x, List<double> out) {
  int? below, above;
  for (var i = 0; i < positions.length; i++) {
    final t = positions[i].$1;
    if (t <= x && (below == null || t > positions[below].$1)) below = i;
    if (t >= x && (above == null || t < positions[above].$1)) above = i;
  }
  if (below != null && above != null && above != below) {
    final (tb, ta) = (positions[below].$1, positions[above].$1);
    final f = (x - tb) / (ta - tb);
    out[below] = 1 - f;
    out[above] = f;
  } else {
    out[below ?? above ?? 0] = 1;
  }
}

double _len((double, double) v) => math.sqrt(v.$1 * v.$1 + v.$2 * v.$2);
(double, double) _sub((double, double) a, (double, double) b) => (a.$1 - b.$1, a.$2 - b.$2);
double _dot((double, double) a, (double, double) b) => a.$1 * b.$1 + a.$2 * b.$2;
double _signedAngle((double, double) a, (double, double) b) => math.atan2(a.$1 * b.$2 - a.$2 * b.$1, _dot(a, b));

const _tau = 2 * math.pi;

bool _simpleDirectional(List<(double, double)> positions, (double, double) p, List<double> out) {
  final centerAt = positions.indexWhere((q) => _len(q) < _eps);
  final center = centerAt < 0 ? null : centerAt;
  if (_len(p) < _eps) {
    if (center == null) return false;
    out[center] = 1;
    return true;
  }
  final angle = math.atan2(p.$2, p.$1);
  (int, double)? a, b;
  for (var i = 0; i < positions.length; i++) {
    if (i == center) continue;
    final q = positions[i];
    final d = (math.atan2(q.$2, q.$1) - angle) % _tau;
    final back = (_tau - d) % _tau;
    if (a == null || back < a.$2) a = (i, back);
    if (d > 0 && (b == null || d < b.$2)) b = (i, d);
  }
  if (a == null) return false;
  final (ia, da) = a;
  final qa = positions[ia];
  double alpha, beta;
  int? ib;
  if (b != null && b.$1 != ia && da + b.$2 < math.pi - _eps) {
    ib = b.$1;
    final qb = positions[ib];
    final det = qa.$1 * qb.$2 - qa.$2 * qb.$1;
    if (det.abs() < _eps) return false;
    alpha = math.max(0, (p.$1 * qb.$2 - p.$2 * qb.$1) / det);
    beta = math.max(0, (qa.$1 * p.$2 - qa.$2 * p.$1) / det);
  } else if (da < _eps) {
    alpha = _dot(p, qa) / _dot(qa, qa);
    beta = 0;
  } else {
    return false;
  }
  final sum = alpha + beta;
  final rest = center != null && sum < 1;
  final scale = rest ? 1.0 : 1 / sum;
  out[ia] = alpha * scale;
  if (ib != null) out[ib] = beta * scale;
  if (rest) out[center] = 1 - sum;
  return true;
}

void _gradientBand(List<(double, double)> positions, (double, double) p, List<double> out) {
  for (var i = 0; i < positions.length; i++) {
    var h = double.infinity;
    for (var j = 0; j < positions.length; j++) {
      if (i == j) continue;
      final ij = _sub(positions[j], positions[i]);
      final ip = _sub(p, positions[i]);
      final lsq = _dot(ij, ij);
      if (lsq < _eps * _eps) continue;
      h = math.min(h, 1 - _dot(ip, ij) / lsq);
    }
    out[i] = h.isFinite ? math.max(0, h) : 1;
  }
}

/// Johansen's weight of an angle against a relative change of magnitude.
const _directionScale = 2.0;

void _polarGradientBand(List<(double, double)> positions, (double, double) p, List<double> out) {
  final pm = _len(p);
  for (var i = 0; i < positions.length; i++) {
    final pi = positions[i];
    final im = _len(pi);
    var h = double.infinity;
    for (var j = 0; j < positions.length; j++) {
      if (i == j) continue;
      final pj = positions[j];
      final jm = _len(pj);
      final avg = (im + jm) * 0.5;
      if (avg < _eps) continue;
      var angleIj = _signedAngle(pi, pj);
      var angleIp = _signedAngle(pi, p);
      if (im < _eps) {
        angleIp = 0;
        angleIj = pm < _eps ? 0 : _signedAngle(p, pj);
      } else if (jm < _eps) {
        angleIj = 0;
      }
      if (pm < _eps) angleIp = 0;
      final ij = ((jm - im) / avg, angleIj * _directionScale);
      final ip = ((pm - im) / avg, angleIp * _directionScale);
      final lsq = _dot(ij, ij);
      if (lsq < _eps * _eps) continue;
      h = math.min(h, 1 - _dot(ip, ij) / lsq);
    }
    out[i] = h.isFinite ? math.max(0, h) : 1;
  }
}

void _finish(List<(double, double)> positions, (double, double) p, List<double> out) {
  final sum = out.fold(0.0, (a, b) => a + b);
  if (sum <= 0 || !sum.isFinite) {
    out.fillRange(0, out.length, 0);
    var nearest = 0;
    for (var i = 1; i < positions.length; i++) {
      if (_len(_sub(positions[i], p)) < _len(_sub(positions[nearest], p))) nearest = i;
    }
    out[nearest] = 1;
    return;
  }
  for (var i = 0; i < out.length; i++) {
    out[i] /= sum;
  }
  _dropSmall(out);
}

void _dropSmall(List<double> out) {
  var sum = 0.0;
  for (var i = 0; i < out.length; i++) {
    if (out[i] < minWeight) out[i] = 0;
    sum += out[i];
  }
  if (sum > 0 && sum != 1) {
    for (var i = 0; i < out.length; i++) {
      out[i] /= sum;
    }
  }
}
