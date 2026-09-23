/// Gizmos drawn by game code at run time (ADR-0019 §3, blueprint §19.2e
/// item 9h): an immediate-mode API a behaviour calls from any callback —
/// `Gizmos.wireSphere(transform.position, 2)` — whose lines the game's own
/// window draws over the picture through the renderer's overlay, faded
/// where they pass behind geometry.
///
/// They are on only when the view asks (`FynkelGameView(gizmos: true)`,
/// or `FYNKEL_GIZMOS=1` in the environment — what the editor's Play
/// passes when its Gizmos toggle is on): [World.gizmos] is then a
/// [GizmoBuffer] the world clears at the start of every frame. Otherwise
/// every call returns at once; in a release build [Gizmos.compiled] is a
/// `const false` and the compiler drops the calls' bodies.
library;

import 'dart:math' as math;
import 'dart:typed_data';

import '../generated/schema.g.dart' show Transform;
import '../math.dart';
import 'context.dart';

/// One frame's gizmo lines: ten floats each, `ax ay az bx by bz r g b a`
/// in world space, linear RGBA (the layout the viewport's overlay takes).
final class GizmoBuffer {
  static const lineFloats = 10;

  Float32List _data = Float32List(lineFloats * 256);
  int _count = 0;

  /// Lines queued this frame.
  int get lineCount => _count;
  bool get isEmpty => _count == 0;

  /// The queued lines (a view of the buffer, valid until the next change).
  Float32List get lines => Float32List.sublistView(_data, 0, _count * lineFloats);

  /// The colour and transform calls use; reset every frame.
  Vec4 color = Gizmos.defaultColor;
  Transform? matrix;

  void clear() {
    _count = 0;
    color = Gizmos.defaultColor;
    matrix = null;
  }

  /// A line in world space (no [matrix]).
  void addLine(Vec3 a, Vec3 b, Vec4 rgba) {
    if (!a.isFinite || !b.isFinite || !rgba.isFinite) return;
    if ((_count + 1) * lineFloats > _data.length) {
      _data = Float32List(_data.length * 2)..setRange(0, _count * lineFloats, _data);
    }
    final o = _count++ * lineFloats;
    _data
      ..[o] = a.x
      ..[o + 1] = a.y
      ..[o + 2] = a.z
      ..[o + 3] = b.x
      ..[o + 4] = b.y
      ..[o + 5] = b.z
      ..[o + 6] = rgba.x
      ..[o + 7] = rgba.y
      ..[o + 8] = rgba.z
      ..[o + 9] = rgba.w;
  }

  Vec3 _apply(Vec3 p) {
    final m = matrix;
    if (m == null) return p;
    return m.position + m.rotation.rotate(Vec3(p.x * m.scale.x, p.y * m.scale.y, p.z * m.scale.z));
  }

  /// A line through [matrix] in [color].
  void line(Vec3 a, Vec3 b) => addLine(_apply(a), _apply(b), color);

  void polyline(List<Vec3> points, {bool closed = false}) {
    for (var i = 0; i + 1 < points.length; i++) {
      line(points[i], points[i + 1]);
    }
    if (closed && points.length > 2) line(points.last, points.first);
  }
}

/// Draws debug shapes in the game's window. Every call is a no-op unless
/// the world's gizmos are on ([World.gizmos]), and free in release.
abstract final class Gizmos {
  /// False in a release (product) build: the calls compile to nothing.
  static const bool compiled = !bool.fromEnvironment('dart.vm.product');

  /// White, as Unity starts.
  static const Vec4 defaultColor = Vec4(1, 1, 1, 1);

  static const _segments = 32;

  static GizmoBuffer? get _buffer => compiled ? GameContext.current?.serviceOrNull<GizmoBuffer>() : null;

  /// Whether calls draw anything now.
  static bool get enabled => _buffer != null;

  /// The colour the next shapes take (linear RGBA); reset every frame.
  static Vec4 get color => _buffer?.color ?? defaultColor;
  static set color(Vec4 c) => _buffer?.color = c;

  /// The transform the next shapes are drawn through (a local space: a
  /// `GameObject.worldTransform`), null for world space; reset every frame.
  static Transform? get matrix => _buffer?.matrix;
  static set matrix(Transform? m) => _buffer?.matrix = m;

  /// Runs [draw] with [color] set, then restores the previous one.
  static void withColor(Vec4 color, void Function() draw) {
    final b = _buffer;
    if (b == null) return;
    final saved = b.color;
    b.color = color;
    try {
      draw();
    } finally {
      b.color = saved;
    }
  }

  /// Runs [draw] in the local space of [matrix], then restores the
  /// previous one.
  static void withMatrix(Transform matrix, void Function() draw) {
    final b = _buffer;
    if (b == null) return;
    final saved = b.matrix;
    b.matrix = matrix;
    try {
      draw();
    } finally {
      b.matrix = saved;
    }
  }

  static void line(Vec3 from, Vec3 to) {
    if (!compiled) return;
    _buffer?.line(from, to);
  }

  /// From [from] along [direction] (its length is the ray's).
  static void ray(Vec3 from, Vec3 direction) {
    if (!compiled) return;
    _buffer?.line(from, from + direction);
  }

  /// A circle of [radius] about [normal] through [center].
  static void wireCircle(Vec3 center, double radius, {Vec3 normal = Vec3.up}) {
    if (!compiled) return;
    final b = _buffer;
    if (b == null) return;
    b.polyline(_circle(center, normal, radius), closed: true);
  }

  /// Three circles, one per axis.
  static void wireSphere(Vec3 center, double radius) {
    if (!compiled) return;
    final b = _buffer;
    if (b == null) return;
    for (final n in const [Vec3(1, 0, 0), Vec3(0, 1, 0), Vec3(0, 0, 1)]) {
      b.polyline(_circle(center, n, radius), closed: true);
    }
  }

  /// A box of [size] (full extents) centred on [center].
  static void wireBox(Vec3 center, Vec3 size) {
    if (!compiled) return;
    final b = _buffer;
    if (b == null) return;
    final h = size * 0.5;
    Vec3 corner(int i) =>
        center + Vec3(i & 1 == 0 ? -h.x : h.x, i & 2 == 0 ? -h.y : h.y, i & 4 == 0 ? -h.z : h.z);
    const edges = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    for (final (i, j) in edges) {
      b.line(corner(i), corner(j));
    }
  }

  /// A capsule along Y: [height] end to end (at least two radii), [radius].
  static void wireCapsule(Vec3 center, double radius, double height) {
    if (!compiled) return;
    final b = _buffer;
    if (b == null) return;
    final half = math.max(0.0, height / 2 - radius);
    final top = center + Vec3.up * half, bottom = center - Vec3.up * half;
    b
      ..polyline(_circle(top, Vec3.up, radius), closed: true)
      ..polyline(_circle(bottom, Vec3.up, radius), closed: true);
    for (final d in const [Vec3(1, 0, 0), Vec3(-1, 0, 0), Vec3(0, 0, 1), Vec3(0, 0, -1)]) {
      b.line(bottom + d * radius, top + d * radius);
    }
    for (final side in const [Vec3(1, 0, 0), Vec3(0, 0, 1)]) {
      b
        ..polyline(_arc(top, side, Vec3.up, radius))
        ..polyline(_arc(bottom, side, -Vec3.up, radius));
    }
  }

  /// A line from [from] to [to] with a head.
  static void arrow(Vec3 from, Vec3 to, {double head = 0.2}) {
    if (!compiled) return;
    final b = _buffer;
    if (b == null) return;
    b.line(from, to);
    final d = to - from;
    final len = d.length;
    if (len == 0) return;
    final dir = d / len;
    final h = math.min(head, len * 0.5);
    final (u, v) = _perpendiculars(dir);
    for (final w in [u, -u, v, -v]) {
      b.line(to, to - dir * h + w * (h * 0.4));
    }
  }

  /// A perspective frustum from [origin] looking along [rotation]'s −Z:
  /// [fovY] radians, [aspect], from [near] to [far].
  static void frustum(Vec3 origin, Quat rotation, double fovY, double aspect, double near, double far) {
    if (!compiled) return;
    final b = _buffer;
    if (b == null) return;
    List<Vec3> rect(double d) {
      final hh = d * math.tan(fovY / 2), hw = hh * aspect;
      return [
        for (final (x, y) in const [(-1, -1), (1, -1), (1, 1), (-1, 1)])
          origin + rotation.rotate(Vec3(x * hw, y * hh, -d)),
      ];
    }

    final n = rect(near), f = rect(far);
    b
      ..polyline(n, closed: true)
      ..polyline(f, closed: true);
    for (var i = 0; i < 4; i++) {
      b.line(n[i], f[i]);
    }
  }

  static (Vec3, Vec3) _perpendiculars(Vec3 n) {
    final a = n.normalized;
    final u = a.cross(a.y.abs() < 0.9 ? Vec3.up : const Vec3(1, 0, 0)).normalized;
    return (u, a.cross(u));
  }

  static List<Vec3> _circle(Vec3 c, Vec3 normal, double r) {
    final (u, v) = _perpendiculars(normal);
    return [
      for (var i = 0; i < _segments; i++)
        c + u * (r * math.cos(i * 2 * math.pi / _segments)) + v * (r * math.sin(i * 2 * math.pi / _segments)),
    ];
  }

  static List<Vec3> _arc(Vec3 c, Vec3 side, Vec3 pole, double r) => [
        for (var i = 0; i <= _segments ~/ 2; i++)
          c +
              side * (-r * math.cos(i * math.pi / (_segments ~/ 2))) +
              pole * (r * math.sin(i * math.pi / (_segments ~/ 2))),
      ];
}
