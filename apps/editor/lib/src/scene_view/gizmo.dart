// The handle framework (blueprint §19.2e item 9g, ADR-0019 §2): a handle
// is a value — a shape in world space, a hit test in screen space and a
// drag constraint that turns pointer motion into a number or a vector.
// Tools (`scene_tools.dart`) build handles for the selection and decide
// what a constrained value does; the view model hit-tests, drags and
// commits; the renderer's overlay draws them on top of the picture.
import 'dart:math' as math;
import 'dart:ui' show Offset;

import 'scene_math.dart';

/// What a handle looks like, in world space. [lines] and [triangles] are
/// drawn and hit; [back] (the far half of a ring) is drawn faint and never
/// hit.
final class HandleShape {
  const HandleShape({this.lines = const [], this.triangles = const [], this.back = const [], this.fill = 1});

  /// Polylines.
  final List<List<Vec3>> lines;

  /// Filled triangles, three points each.
  final List<List<Vec3>> triangles;
  final List<List<Vec3>> back;

  /// The triangles' alpha relative to the handle's colour (a plane
  /// square is translucent, an arrow's cone opaque).
  final double fill;
}

/// What a drag reads: where it started and where the pointer is now, in
/// pixels and as rays, and the camera's forward axis.
final class DragInput {
  const DragInput({
    required this.startPx,
    required this.px,
    required this.startRay,
    required this.ray,
    required this.pivotPx,
    required this.cameraForward,
  });

  final Offset startPx, px, pivotPx;
  final Ray startRay, ray;
  final Vec3 cameraForward;
}

/// A constrained drag's value: a scalar (metres along an axis, radians,
/// a factor, a radius) and a vector (a move in world space).
final class DragValue {
  const DragValue(this.scalar, [this.vector = const [0, 0, 0]]);
  final double scalar;
  final Vec3 vector;
}

/// How pointer motion becomes a value.
sealed class DragConstraint {
  const DragConstraint();

  /// Null when the pointer's ray misses (parallel to a plane or an axis).
  DragValue? solve(DragInput i);
}

/// Along a line: the distance moved along [axis] (metres), and that move.
final class AxisConstraint extends DragConstraint {
  const AxisConstraint(this.origin, this.axis);
  final Vec3 origin, axis;

  @override
  DragValue? solve(DragInput i) {
    final s0 = i.startRay.closestOnLine(origin, axis);
    final s1 = i.ray.closestOnLine(origin, axis);
    if (s0 == null || s1 == null) return null;
    final s = s1 - s0;
    return DragValue(s, vScale(vNormalize(axis), s));
  }
}

/// In a plane through [origin] with [normal] (null normal: the view
/// plane): the move, and its length.
final class PlaneConstraint extends DragConstraint {
  const PlaneConstraint(this.origin, [this.normal]);
  final Vec3 origin;
  final Vec3? normal;

  @override
  DragValue? solve(DragInput i) {
    final n = normal ?? i.cameraForward;
    final h0 = i.startRay.hitPlane(origin, n);
    final h1 = i.ray.hitPlane(origin, n);
    if (h0 == null || h1 == null) return null;
    final v = vSub(h1, h0);
    return DragValue(vLength(v), v);
  }
}

/// Around [axis] (null: the view axis): the angle swept about the pivot on
/// screen, in radians, positive counter-clockwise looking down the axis.
final class RingConstraint extends DragConstraint {
  const RingConstraint(this.axis, {required this.facingCamera});
  final Vec3? axis;

  /// Whether [axis] points towards the camera (screen angles grow
  /// clockwise, y down, so the sign flips).
  final bool facingCamera;

  @override
  DragValue? solve(DragInput i) {
    final a0 = math.atan2(i.startPx.dy - i.pivotPx.dy, i.startPx.dx - i.pivotPx.dx);
    final a1 = math.atan2(i.px.dy - i.pivotPx.dy, i.px.dx - i.pivotPx.dx);
    var angle = a1 - a0;
    while (angle > math.pi) {
      angle -= 2 * math.pi;
    }
    while (angle < -math.pi) {
      angle += 2 * math.pi;
    }
    if (axis != null && facingCamera) angle = -angle;
    return DragValue(angle, axis ?? i.cameraForward);
  }
}

/// Away from the pivot on screen: the factor of the pointer's distance
/// from it now over at the start (a uniform scale).
final class ScreenScaleConstraint extends DragConstraint {
  const ScreenScaleConstraint();

  @override
  DragValue? solve(DragInput i) {
    final l0 = (i.startPx - i.pivotPx).distance;
    final l1 = (i.px - i.pivotPx).distance;
    return DragValue(l0 < 1 ? 1.0 : l1 / l0);
  }
}

/// A radius from [center] along [direction]: [start] plus the distance
/// moved along it, never below zero (a light's range, a sphere's radius,
/// a box's half extent).
final class RadiusConstraint extends DragConstraint {
  const RadiusConstraint(this.center, this.direction, this.start);
  final Vec3 center, direction;
  final double start;

  @override
  DragValue? solve(DragInput i) {
    final v = AxisConstraint(center, direction).solve(i);
    if (v == null) return null;
    return DragValue(math.max(0, start + v.scalar), v.vector);
  }
}

/// A draggable thing in the Scene view.
final class Handle {
  const Handle({
    required this.id,
    required this.shape,
    required this.color,
    required this.constraint,
    required this.grab,
    this.width = HandleWidth.handle,
  });

  /// Unique among the handles on screen: `move.x`, `rotate.view`,
  /// `scale.uniform`, `light.range.+x`.
  final String id;
  final HandleShape shape;

  /// Linear RGBA.
  final List<double> color;
  final DragConstraint constraint;

  /// A world point on the handle that is always hit: where a test (or a
  /// keyboard-free script) grabs it.
  final Vec3 grab;
  final HandleWidth width;
}

/// Which of the design tokens' widths a handle's lines take.
enum HandleWidth { handle, line }

/// A handle projected to pixels for one frame: its polylines and
/// triangles in physical pixels, for the hit test.
final class ProjectedHandle {
  ProjectedHandle(this.handle, ViewProjection vp)
    : lines = [
        for (final l in handle.shape.lines) [for (final p in l) ?_px(vp, p)],
      ],
      triangles = [
        for (final t in handle.shape.triangles)
          if (t.map((p) => _px(vp, p)).toList() case [final Offset a, final Offset b, final Offset c])
            // Edge-on, a triangle is a line every point on which is
            // "inside": it would steal clicks from the handles along it.
            if (((b - a).dx * (c - a).dy - (b - a).dy * (c - a).dx).abs() > 1) [a, b, c],
      ];

  final Handle handle;
  final List<List<Offset>> lines;
  final List<List<Offset>> triangles;

  static Offset? _px(ViewProjection vp, Vec3 p) {
    final r = vp.project(p);
    return r == null ? null : Offset(r[0], r[1]);
  }

  /// Distance from [p] to the handle in pixels (0 inside a triangle).
  double distanceTo(Offset p) {
    for (final t in triangles) {
      if (_inside(p, t)) return 0;
    }
    var best = double.infinity;
    for (final l in lines) {
      for (var i = 0; i + 1 < l.length; i++) {
        best = math.min(best, _segmentDistance(p, l[i], l[i + 1]));
      }
    }
    return best;
  }

  static bool _inside(Offset p, List<Offset> t) {
    double side(Offset a, Offset b) => (b.dx - a.dx) * (p.dy - a.dy) - (b.dy - a.dy) * (p.dx - a.dx);
    final d0 = side(t[0], t[1]), d1 = side(t[1], t[2]), d2 = side(t[2], t[0]);
    final neg = d0 < 0 || d1 < 0 || d2 < 0, pos = d0 > 0 || d1 > 0 || d2 > 0;
    return !(neg && pos);
  }

  static double _segmentDistance(Offset p, Offset a, Offset b) {
    final ab = b - a;
    final len2 = ab.dx * ab.dx + ab.dy * ab.dy;
    if (len2 < 1e-9) return (p - a).distance;
    final t = (((p - a).dx * ab.dx + (p - a).dy * ab.dy) / len2).clamp(0.0, 1.0);
    return (p - (a + ab * t)).distance;
  }
}

/// The handle under [p] among [handles] (physical pixels): the first one
/// [p] is inside a triangle of — so the order is the priority, small
/// filled handles first — else the nearest line within [tolerance].
Handle? hitTestHandles(List<ProjectedHandle> handles, Offset p, {double tolerance = 8}) {
  Handle? best;
  var bestD = tolerance;
  for (final h in handles) {
    final d = h.distanceTo(p);
    if (d == 0) return h.handle;
    if (d < bestD) {
      best = h.handle;
      bestD = d;
    }
  }
  return best;
}

// ── Shapes ─────────────────────────────────────────────────────────────

/// Two unit vectors perpendicular to [axis] and to each other.
(Vec3, Vec3) perpendiculars(Vec3 axis) {
  final a = vNormalize(axis);
  final u = vNormalize(vCross(a, a[1].abs() < 0.9 ? const [0.0, 1.0, 0.0] : const [1.0, 0.0, 0.0]));
  return (u, vCross(a, u));
}

/// Points around a circle of radius [r] about [axis] through [center],
/// closed (the first point repeated).
List<Vec3> circle(Vec3 center, Vec3 axis, double r, {int segments = 64}) {
  final (u, v) = perpendiculars(axis);
  return [
    for (var i = 0; i <= segments; i++)
      vAdd(center, vAdd(vScale(u, r * math.cos(i * 2 * math.pi / segments)), vScale(v, r * math.sin(i * 2 * math.pi / segments)))),
  ];
}

/// A ring split into the half facing [toCamera] (drawn, hit) and the far
/// half (faint) — a rotate handle as Unity draws it. A ring facing the
/// camera is all front.
HandleShape ring(Vec3 center, Vec3 axis, double r, Vec3 toCamera) {
  final pts = circle(center, axis, r);
  final facing = vDot(vNormalize(axis), toCamera).abs() > 0.995;
  if (facing) return HandleShape(lines: [pts]);
  final front = <List<Vec3>>[], back = <List<Vec3>>[];
  List<Vec3>? run;
  bool? runFront;
  for (var i = 0; i + 1 < pts.length; i++) {
    final mid = vScale(vAdd(pts[i], pts[i + 1]), 0.5);
    final isFront = vDot(vSub(mid, center), toCamera) >= -1e-9 * r;
    if (isFront != runFront) {
      run = [pts[i]];
      (isFront ? front : back).add(run);
      runFront = isFront;
    }
    run!.add(pts[i + 1]);
  }
  return HandleShape(lines: front, back: back);
}

/// A cone from [base] to [tip] of base radius [r], as triangles.
List<List<Vec3>> cone(Vec3 base, Vec3 tip, double r, {int segments = 12}) {
  final (u, v) = perpendiculars(vSub(tip, base));
  final rim = [
    for (var i = 0; i < segments; i++)
      vAdd(base, vAdd(vScale(u, r * math.cos(i * 2 * math.pi / segments)), vScale(v, r * math.sin(i * 2 * math.pi / segments)))),
  ];
  return [
    for (var i = 0; i < segments; i++) ...[
      [rim[i], rim[(i + 1) % segments], tip],
      [rim[(i + 1) % segments], rim[i], base],
    ],
  ];
}

/// An axis-aligned (in [axes]) cube of half size [h] at [center], as
/// triangles.
List<List<Vec3>> cube(Vec3 center, List<Vec3> axes, double h) {
  Vec3 corner(int i) => vAdd(
    center,
    vAdd(
      vScale(axes[0], i & 1 == 0 ? -h : h),
      vAdd(vScale(axes[1], i & 2 == 0 ? -h : h), vScale(axes[2], i & 4 == 0 ? -h : h)),
    ),
  );
  const faces = [
    [0, 1, 3, 2],
    [4, 6, 7, 5],
    [0, 4, 5, 1],
    [2, 3, 7, 6],
    [0, 2, 6, 4],
    [1, 5, 7, 3],
  ];
  return [
    for (final f in faces) ...[
      [corner(f[0]), corner(f[1]), corner(f[2])],
      [corner(f[0]), corner(f[2]), corner(f[3])],
    ],
  ];
}

/// A quad [a] [b] [c] [d] as two triangles.
List<List<Vec3>> quad(Vec3 a, Vec3 b, Vec3 c, Vec3 d) => [
  [a, b, c],
  [a, c, d],
];
