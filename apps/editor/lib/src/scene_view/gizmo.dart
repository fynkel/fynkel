// The transform gizmo's geometry: built in world space around the pivot
// (constant screen size), projected to pixels once per frame, hit-tested
// by the model and painted by the panel from the same projection — so
// what the user grabs is what they see. The gizmo is 2D on top of the
// picture (never hidden inside a mesh); the selection outline and the
// icons are debug lines in the picture.
import 'dart:math' as math;
import 'dart:ui' show Offset;

import 'editor_state.dart';
import 'scene_math.dart';

/// A handle of the gizmo.
enum GizmoHandle {
  x,
  y,
  z,
  xy,
  yz,
  xz,

  /// Move: the view-plane square; rotate: the ring about the view axis;
  /// scale: uniform.
  center;

  bool get isAxis => this == x || this == y || this == z;
  bool get isPlane => this == xy || this == yz || this == xz;

  /// The axis index of an axis handle.
  int get axisIndex => switch (this) {
    x => 0,
    y => 1,
    z => 2,
    _ => -1,
  };

  /// The two axis indices of a plane handle.
  (int, int) get planeAxes => switch (this) {
    xy => (0, 1),
    yz => (1, 2),
    xz => (0, 2),
    _ => (0, 0),
  };

  /// The axis a plane handle is perpendicular to.
  int get planeNormalIndex => switch (this) {
    xy => 2,
    yz => 0,
    xz => 1,
    _ => -1,
  };
}

/// The gizmo in world space.
final class GizmoGeometry {
  const GizmoGeometry({required this.tool, required this.pivot, required this.axes, required this.size});

  final SceneTool tool;
  final Vec3 pivot;

  /// Unit axes of the gizmo's frame (world or the pivot entity's local).
  final List<Vec3> axes;

  /// The handle length in metres (≈ 90 px on screen).
  final double size;

  /// Pixels the gizmo spans on screen.
  static const screenSize = 90.0;

  /// Points along a ring of radius [r] about [axis] through the pivot.
  List<Vec3> ring(Vec3 axis, double r, {int segments = 64}) {
    final a = vNormalize(axis);
    var u = vCross(a, a[1].abs() < 0.9 ? [0, 1, 0] : [1, 0, 0]);
    u = vNormalize(u);
    final v = vCross(a, u);
    return [
      for (var i = 0; i <= segments; i++)
        vAdd(
          pivot,
          vAdd(
            vScale(u, r * math.cos(i * 2 * math.pi / segments)),
            vScale(v, r * math.sin(i * 2 * math.pi / segments)),
          ),
        ),
    ];
  }
}

/// A handle projected to pixels: a polyline (an axis, a ring) or a polygon
/// (a plane square, the centre) with its hit test.
final class ProjectedHandle {
  const ProjectedHandle(this.handle, this.points, {this.closed = false, this.tip});

  final GizmoHandle handle;

  /// Physical pixels.
  final List<Offset> points;

  /// A closed polygon (filled) rather than a stroke.
  final bool closed;

  /// An axis handle's tip (the arrow head / the cube).
  final Offset? tip;

  /// Distance from [p] to the handle in pixels (0 inside a polygon).
  double distanceTo(Offset p) {
    if (closed && _inside(p)) return 0;
    var best = double.infinity;
    for (var i = 0; i + 1 < points.length; i++) {
      best = math.min(best, _segmentDistance(p, points[i], points[i + 1]));
    }
    if (closed && points.length > 2) best = math.min(best, _segmentDistance(p, points.last, points.first));
    return best;
  }

  bool _inside(Offset p) {
    var inside = false;
    for (var i = 0, j = points.length - 1; i < points.length; j = i++) {
      final a = points[i], b = points[j];
      if ((a.dy > p.dy) != (b.dy > p.dy) && p.dx < (b.dx - a.dx) * (p.dy - a.dy) / (b.dy - a.dy) + a.dx) {
        inside = !inside;
      }
    }
    return inside;
  }

  static double _segmentDistance(Offset p, Offset a, Offset b) {
    final ab = b - a;
    final len2 = ab.dx * ab.dx + ab.dy * ab.dy;
    if (len2 < 1e-9) return (p - a).distance;
    final t = (((p - a).dx * ab.dx + (p - a).dy * ab.dy) / len2).clamp(0.0, 1.0);
    return (p - (a + ab * t)).distance;
  }
}

/// The gizmo on screen this frame.
final class ProjectedGizmo {
  const ProjectedGizmo({required this.tool, required this.pivot, required this.handles, required this.axisDepth});

  final SceneTool tool;

  /// The pivot's pixel.
  final Offset pivot;
  final List<ProjectedHandle> handles;

  /// Whether each gizmo axis points towards the camera (the rotate drag's
  /// sign).
  final List<bool> axisDepth;

  /// The handle under [p] (physical pixels), the closest within [tolerance].
  GizmoHandle? hitTest(Offset p, {double tolerance = 8}) {
    GizmoHandle? best;
    var bestD = tolerance;
    // Polygons first: the centre and the planes are small and sit over
    // the axis lines.
    for (final h in handles.where((h) => h.closed)) {
      final d = h.distanceTo(p);
      if (d < bestD || (d == 0 && best == null)) {
        best = h.handle;
        bestD = d;
      }
      if (d == 0) return h.handle;
    }
    for (final h in handles.where((h) => !h.closed)) {
      final d = h.distanceTo(p);
      if (d < bestD) {
        best = h.handle;
        bestD = d;
      }
    }
    return best;
  }

  /// Projects [g] through [vp]; null when the pivot is behind the camera.
  static ProjectedGizmo? project(GizmoGeometry g, ViewProjection vp) {
    final pivotPx = vp.project(g.pivot);
    if (pivotPx == null) return null;
    final pivot = Offset(pivotPx[0], pivotPx[1]);
    Offset? px(Vec3 p) {
      final r = vp.project(p);
      return r == null ? null : Offset(r[0], r[1]);
    }

    List<Offset> pts(List<Vec3> ps) => [for (final p in ps) ?px(p)];
    final camPos = matTranslation(vp.world);
    final toCam = vNormalize(vSub(camPos, g.pivot));
    final depth = [for (final a in g.axes) vDot(a, toCam) >= 0];
    final handles = <ProjectedHandle>[];
    final axisHandles = [GizmoHandle.x, GizmoHandle.y, GizmoHandle.z];
    switch (g.tool) {
      case SceneTool.select:
        break;
      case SceneTool.move || SceneTool.scale:
        for (var i = 0; i < 3; i++) {
          final tip = vAdd(g.pivot, vScale(g.axes[i], g.size));
          final tipPx = px(tip);
          if (tipPx == null) continue;
          handles.add(ProjectedHandle(axisHandles[i], [pivot, tipPx], tip: tipPx));
        }
        if (g.tool == SceneTool.move) {
          for (final h in [GizmoHandle.xy, GizmoHandle.yz, GizmoHandle.xz]) {
            final (a, b) = h.planeAxes;
            final lo = g.size * 0.35, hi = g.size * 0.65;
            final quad = pts([
              vAdd(g.pivot, vAdd(vScale(g.axes[a], lo), vScale(g.axes[b], lo))),
              vAdd(g.pivot, vAdd(vScale(g.axes[a], hi), vScale(g.axes[b], lo))),
              vAdd(g.pivot, vAdd(vScale(g.axes[a], hi), vScale(g.axes[b], hi))),
              vAdd(g.pivot, vAdd(vScale(g.axes[a], lo), vScale(g.axes[b], hi))),
            ]);
            if (quad.length == 4) handles.add(ProjectedHandle(h, quad, closed: true));
          }
        }
        // The centre: a square of 12 px (move: the view plane; scale: uniform).
        const c = 7.0;
        handles.add(
          ProjectedHandle(GizmoHandle.center, [
            pivot + const Offset(-c, -c),
            pivot + const Offset(c, -c),
            pivot + const Offset(c, c),
            pivot + const Offset(-c, c),
          ], closed: true),
        );
      case SceneTool.rotate:
        for (var i = 0; i < 3; i++) {
          handles.add(ProjectedHandle(axisHandles[i], pts(g.ring(g.axes[i], g.size))));
        }
        // The outer ring, about the view axis, always a circle on screen.
        final forward = vScale(matAxes(vp.world)[2], -1);
        handles.add(ProjectedHandle(GizmoHandle.center, pts(g.ring(forward, g.size * 1.2))));
    }
    return ProjectedGizmo(tool: g.tool, pivot: pivot, handles: handles, axisDepth: depth);
  }
}
