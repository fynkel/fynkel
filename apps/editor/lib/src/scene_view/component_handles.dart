// Component handles (blueprint §19.2e item 9g, ADR-0019 §2): a component
// type registers handles for the numbers a user would rather drag in the
// picture than type — a point or spot light's range, a spot's angles, a
// box, sphere or capsule collider's size, a camera's field of view. They
// show behind the Inspector's Edit toggle (Unity's *Edit Collider*), in
// place of the tool's gizmo, on the lead entity; each drag is one
// transaction of the fields it changed.
//
// A handler reads the component's data as the document holds it and the
// entity's world matrix, returns handles (`gizmo.dart`) and turns a
// constrained drag value into field values. The world keeps drawing the
// committed shape until release; the handles and their guides draw the
// dragged one.
import 'dart:math' as math;

import '../theme/tokens.dart';
import 'gizmo.dart';
import 'scene_math.dart';
import 'scene_tools.dart' show GizmoFrame;

/// What a component handler sees: the entity's world matrix, the
/// component's data, the view.
final class ComponentContext {
  ComponentContext({
    required this.world,
    required this.data,
    required this.vp,
    this.pixelRatio = 1,
    this.aspect = 1,
  });

  final List<double> world;
  final Map<String, Object?> data;
  final ViewProjection vp;

  /// Physical pixels per point.
  final double pixelRatio;

  /// The picture's width over height (a camera's frustum guide).
  final double aspect;

  /// The same, over other data (a drag's current values).
  ComponentContext withData(Map<String, Object?> data) =>
      ComponentContext(world: world, data: data, vp: vp, pixelRatio: pixelRatio, aspect: aspect);

  /// The entity's origin.
  Vec3 get origin => matTranslation(world);

  /// Its unit axes (the rotation, scale removed).
  late final List<Vec3> axes = matAxes(world);

  /// The largest of its axes' scales (a sphere's radius scales by it).
  late final double maxScale = [
    for (var i = 0; i < 3; i++) vLength(transformDirection(world, [i == 0 ? 1.0 : 0, i == 1 ? 1.0 : 0, i == 2 ? 1.0 : 0])),
  ].reduce(math.max);

  /// Metres per point at [p].
  double point(Vec3 p) => vp.metresPerPixel(p) * pixelRatio;

  /// The camera's right and up axes.
  late final List<Vec3> camera = () {
    final a = matAxes(vp.world);
    return [a[0], a[1]];
  }();

  /// The unit direction from [p] to the camera.
  Vec3 toCamera(Vec3 p) => vp.camera.orthographic
      ? vNormalize(matAxes(vp.world)[2])
      : vNormalize(vSub(matTranslation(vp.world), p));

  double number(String field, double fallback) {
    final v = data[field];
    return v is num && v.isFinite ? v.toDouble() : fallback;
  }

  Vec3 vector(String field, Vec3 fallback) {
    final v = data[field];
    if (v is List && v.length == 3 && v.every((e) => e is num)) return [for (final e in v) (e as num).toDouble()];
    return fallback;
  }
}

/// A handle drag's answer: the fields it sets (top-level keys of the
/// component's data) and the readout under the picture.
final class FieldEdit {
  const FieldEdit(this.fields, this.readout);

  final Map<String, Object?> fields;
  final String readout;
}

/// The handles of one component type.
abstract class ComponentHandles {
  const ComponentHandles();

  /// The component type (`fynkel.PointLight`).
  String get type;

  /// Whether this component's data has anything to drag (a mesh
  /// collider has not).
  bool supports(Map<String, Object?> data) => true;

  /// The handles, in priority order.
  List<Handle> handles(ComponentContext c);

  /// What dragging [handle] by [value] sets; [c] holds the data at the
  /// drag's start. Null ignores the move.
  FieldEdit? edit(Handle handle, DragValue value, ComponentContext c);
}

/// The registry, by component type.
abstract final class ComponentHandlers {
  static final _byType = <String, ComponentHandles>{
    for (final h in const [
      PointLightHandles(),
      SpotLightHandles(),
      ColliderHandles(),
      CameraHandles(),
    ])
      h.type: h,
  };

  static const _builtIn = {'fynkel.PointLight', 'fynkel.SpotLight', 'fynkel.Collider', 'fynkel.Camera'};

  static ComponentHandles? of(String type) => _byType[type];

  /// Whether [type] with [data] has handles to show.
  static bool supports(String type, Map<String, Object?> data) => _byType[type]?.supports(data) ?? false;

  static void register(ComponentHandles h) {
    if (_byType.containsKey(h.type)) throw ArgumentError('handles for "${h.type}" are already registered');
    _byType[h.type] = h;
  }

  static void unregister(String type) {
    if (_builtIn.contains(type)) throw ArgumentError('"$type" is built in');
    _byType.remove(type);
  }
}

// ── Shared pieces ──────────────────────────────────────────────────────

/// A handle's dot at [p]: a segment across it along the camera's right
/// axis, drawn [GizmoTokens.dotSize] wide (`HandleWidth.dot`), so a
/// square facing the camera.
HandleShape dot(ComponentContext c, Vec3 p, {List<List<Vec3>> guides = const []}) {
  final r = vScale(c.camera[0], c.point(p) * GizmoTokens.dotSize / 2);
  return HandleShape(
    lines: [
      [vSub(p, r), vAdd(p, r)],
    ],
    guides: guides,
  );
}

const _signs = ['+', '-'];
const _axisNames = ['x', 'y', 'z'];

String _fmt(double v) {
  final r = (v * 1000).roundToDouble() / 1000;
  return r == r.roundToDouble() ? r.toInt().toString() : r.toStringAsFixed(r.abs() < 0.1 ? 3 : 2);
}

String _degrees(double radians) => '${_fmt(radians * 180 / math.pi)}°';

/// Whether a dot along [dir] from a centre would sit on the centre itself
/// as seen from the camera (it is not offered).
bool _endOn(ComponentContext c, Vec3 at, Vec3 dir) => vDot(vNormalize(dir), c.toCamera(at)).abs() > 0.97;

List<double> get _lightColor => GizmoTokens.linear(GizmoTokens.componentHandle);
List<double> get _colliderColor => GizmoTokens.linear(GizmoTokens.colliderHandle);

/// The smallest length a handle leaves (the runtime refuses zero).
const _minLength = 0.001;

// ── Lights ─────────────────────────────────────────────────────────────

/// A point light: its range, a dot on each end of the three circles.
final class PointLightHandles extends ComponentHandles {
  const PointLightHandles();

  @override
  String get type => 'fynkel.PointLight';

  @override
  List<Handle> handles(ComponentContext c) {
    final o = c.origin, r = c.number('range', 10);
    final guides = [for (final axis in GizmoFrame.worldAxes) circle(o, axis, r)];
    final out = <Handle>[];
    for (var i = 0; i < 3; i++) {
      for (var s = 0; s < 2; s++) {
        final dir = vScale(GizmoFrame.worldAxes[i], s == 0 ? 1 : -1);
        final at = vAdd(o, vScale(dir, r));
        if (_endOn(c, o, dir)) continue;
        out.add(
          Handle(
            id: 'range.${_signs[s]}${_axisNames[i]}',
            shape: dot(c, at, guides: out.isEmpty ? guides : const []),
            color: _lightColor,
            constraint: RadiusConstraint(o, dir, r),
            grab: at,
            width: HandleWidth.dot,
          ),
        );
      }
    }
    return out;
  }

  @override
  FieldEdit? edit(Handle handle, DragValue value, ComponentContext c) {
    if (!handle.id.startsWith('range.')) return null;
    final r = math.max(_minLength, value.scalar);
    return FieldEdit({'range': r}, 'range ${_fmt(r)} m');
  }
}

/// A spot light: its range (a dot at the cone's end) and its outer and
/// inner angles (dots on the two circles there).
final class SpotLightHandles extends ComponentHandles {
  const SpotLightHandles();

  @override
  String get type => 'fynkel.SpotLight';

  /// The widest cone a handle drags to (the runtime takes up to π, but a
  /// cone drawn at its range stops making sense near a half turn).
  static const maxAngle = 89 * math.pi / 180;

  @override
  List<Handle> handles(ComponentContext c) {
    final o = c.origin, a = c.axes, fwd = vScale(a[2], -1);
    final range = c.number('range', 10);
    final outer = c.number('outer_angle', 0.6), inner = c.number('inner_angle', 0.4);
    final end = vAdd(o, vScale(fwd, range));
    final ro = range * math.tan(math.min(outer, maxAngle)), ri = range * math.tan(math.min(inner, maxAngle));
    final guides = [
      circle(end, fwd, ro),
      circle(end, fwd, ri),
      for (final d in [a[0], a[1], vScale(a[0], -1), vScale(a[1], -1)]) [o, vAdd(end, vScale(d, ro))],
    ];
    final out = <Handle>[
      Handle(
        id: 'range',
        shape: dot(c, end, guides: guides),
        color: _lightColor,
        constraint: RadiusConstraint(o, fwd, range),
        grab: end,
        width: HandleWidth.dot,
      ),
    ];
    for (final (name, radius, alpha) in [('outer', ro, 1.0), ('inner', ri, 0.6)]) {
      for (var i = 0; i < 2; i++) {
        for (var s = 0; s < 2; s++) {
          final dir = vScale(a[i], s == 0 ? 1 : -1);
          final at = vAdd(end, vScale(dir, radius));
          final color = List.of(_lightColor)..[3] *= alpha;
          out.add(
            Handle(
              id: '$name.${_signs[s]}${_axisNames[i]}',
              shape: dot(c, at),
              color: color,
              constraint: RadiusConstraint(end, dir, radius),
              grab: at,
            width: HandleWidth.dot,
            ),
          );
        }
      }
    }
    return out;
  }

  @override
  FieldEdit? edit(Handle handle, DragValue value, ComponentContext c) {
    final range = c.number('range', 10);
    final outer = c.number('outer_angle', 0.6), inner = c.number('inner_angle', 0.4);
    if (handle.id == 'range') {
      final r = math.max(_minLength, value.scalar);
      return FieldEdit({'range': r}, 'range ${_fmt(r)} m');
    }
    final angle = math.atan2(value.scalar, range).clamp(_minLength, maxAngle);
    if (handle.id.startsWith('outer.')) {
      // Narrowing the outer cone past the inner one takes the inner along.
      return FieldEdit({'outer_angle': angle, if (inner > angle) 'inner_angle': angle}, 'outer ${_degrees(angle)}');
    }
    if (handle.id.startsWith('inner.')) {
      final a = math.min(angle, outer);
      return FieldEdit({'inner_angle': a}, 'inner ${_degrees(a)}');
    }
    return null;
  }
}

// ── Colliders ──────────────────────────────────────────────────────────

/// A collider: a box's six faces (the opposite face stays where it is, so
/// the offset moves by half), a sphere's radius, a capsule's radius and
/// half height (about its centre). A mesh collider has no handles.
final class ColliderHandles extends ComponentHandles {
  const ColliderHandles();

  @override
  String get type => 'fynkel.Collider';

  @override
  bool supports(Map<String, Object?> data) => const {'box', 'sphere', 'capsule'}.contains(data['shape'] ?? 'box');

  @override
  List<Handle> handles(ComponentContext c) => switch (c.data['shape'] ?? 'box') {
    'box' => _box(c),
    'sphere' => _sphere(c),
    'capsule' => _capsule(c),
    _ => const [],
  };

  static Vec3 _unit(int i) => [i == 0 ? 1.0 : 0, i == 1 ? 1.0 : 0, i == 2 ? 1.0 : 0];

  List<Handle> _box(ComponentContext c) {
    final size = c.vector('size', const [0.5, 0.5, 0.5]), offset = c.vector('offset', const [0, 0, 0]);
    final corners = [
      for (var k = 0; k < 8; k++)
        transformPoint(c.world, [
          offset[0] + (k & 1 == 0 ? -size[0] : size[0]),
          offset[1] + (k & 2 == 0 ? -size[1] : size[1]),
          offset[2] + (k & 4 == 0 ? -size[2] : size[2]),
        ]),
    ];
    const edges = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    final guides = [
      for (final (a, b) in edges) [corners[a], corners[b]],
    ];
    final center = transformPoint(c.world, offset);
    final out = <Handle>[];
    for (var i = 0; i < 3; i++) {
      for (var s = 0; s < 2; s++) {
        final sign = s == 0 ? 1.0 : -1.0;
        final local = List.of(offset)..[i] += sign * size[i];
        final at = transformPoint(c.world, local);
        final dir = transformDirection(c.world, vScale(_unit(i), sign));
        if (_endOn(c, center, dir)) continue;
        out.add(
          Handle(
            id: 'size.${_signs[s]}${_axisNames[i]}',
            shape: dot(c, at, guides: out.isEmpty ? guides : const []),
            color: _colliderColor,
            constraint: AxisConstraint(at, dir),
            grab: at,
            width: HandleWidth.dot,
          ),
        );
      }
    }
    return out;
  }

  List<Handle> _sphere(ComponentContext c) {
    final size = c.vector('size', const [0.5, 0.5, 0.5]);
    final center = transformPoint(c.world, c.vector('offset', const [0, 0, 0]));
    final r = size[0] * c.maxScale;
    final guides = [for (final axis in GizmoFrame.worldAxes) circle(center, axis, r)];
    final out = <Handle>[];
    for (var i = 0; i < 3; i++) {
      for (var s = 0; s < 2; s++) {
        final dir = vScale(GizmoFrame.worldAxes[i], s == 0 ? 1 : -1);
        if (_endOn(c, center, dir)) continue;
        final at = vAdd(center, vScale(dir, r));
        out.add(
          Handle(
            id: 'radius.${_signs[s]}${_axisNames[i]}',
            shape: dot(c, at, guides: out.isEmpty ? guides : const []),
            color: _colliderColor,
            constraint: RadiusConstraint(center, dir, r),
            grab: at,
            width: HandleWidth.dot,
          ),
        );
      }
    }
    return out;
  }

  List<Handle> _capsule(ComponentContext c) {
    final size = c.vector('size', const [0.5, 0.5, 0.5]);
    final center = transformPoint(c.world, c.vector('offset', const [0, 0, 0]));
    final a = c.axes, up = a[1], ms = c.maxScale;
    final r = size[0] * ms, h = size[1] * ms;
    final top = vAdd(center, vScale(up, h)), bottom = vSub(center, vScale(up, h));
    final guides = <List<Vec3>>[
      circle(top, up, r),
      circle(bottom, up, r),
      for (final d in [a[0], a[2], vScale(a[0], -1), vScale(a[2], -1)])
        [vAdd(bottom, vScale(d, r)), vAdd(top, vScale(d, r))],
      for (final side in [a[0], a[2]]) ...[
        _arc(top, side, up, r),
        _arc(bottom, side, vScale(up, -1), r),
      ],
    ];
    final out = <Handle>[];
    for (var s = 0; s < 2; s++) {
      final dir = vScale(up, s == 0 ? 1 : -1);
      if (_endOn(c, center, dir)) continue;
      final at = vAdd(center, vScale(dir, h + r));
      out.add(
        Handle(
          id: 'height.${_signs[s]}y',
          shape: dot(c, at, guides: out.isEmpty ? guides : const []),
          color: _colliderColor,
          constraint: RadiusConstraint(center, dir, h + r),
          grab: at,
          width: HandleWidth.dot,
        ),
      );
    }
    for (final i in const [0, 2]) {
      for (var s = 0; s < 2; s++) {
        final dir = vScale(a[i], s == 0 ? 1 : -1);
        if (_endOn(c, center, dir)) continue;
        final at = vAdd(center, vScale(dir, r));
        out.add(
          Handle(
            id: 'radius.${_signs[s]}${_axisNames[i]}',
            shape: dot(c, at, guides: out.isEmpty ? guides : const []),
            color: _colliderColor,
            constraint: RadiusConstraint(center, dir, r),
            grab: at,
            width: HandleWidth.dot,
          ),
        );
      }
    }
    return out;
  }

  /// A half circle from −[side] over [pole] to +[side].
  static List<Vec3> _arc(Vec3 center, Vec3 side, Vec3 pole, double r, {int segments = 32}) => [
    for (var i = 0; i <= segments; i++)
      vAdd(
        center,
        vAdd(vScale(side, -r * math.cos(i * math.pi / segments)), vScale(pole, r * math.sin(i * math.pi / segments))),
      ),
  ];

  @override
  FieldEdit? edit(Handle handle, DragValue value, ComponentContext c) {
    final size = c.vector('size', const [0.5, 0.5, 0.5]);
    final id = handle.id;
    switch (c.data['shape'] ?? 'box') {
      case 'box' when id.startsWith('size.'):
        final i = _axisNames.indexOf(id[6]);
        final sign = id[5] == '+' ? 1.0 : -1.0;
        final scale = vLength(transformDirection(c.world, _unit(i)));
        if (scale < 1e-9) return null;
        // The face moves by the drag, the opposite one stays: the half
        // extent grows by half of it and the centre follows.
        final half = math.max(_minLength, size[i] + value.scalar / scale / 2);
        final offset = c.vector('offset', const [0, 0, 0]);
        final newSize = List.of(size)..[i] = half;
        final newOffset = List.of(offset)..[i] += sign * (half - size[i]);
        return FieldEdit({'size': newSize, 'offset': newOffset}, '${_axisNames[i]} ${_fmt(2 * half * scale)} m');
      case 'sphere' when id.startsWith('radius.'):
        final r = math.max(_minLength, value.scalar / c.maxScale);
        return FieldEdit({'size': List.of(size)..[0] = r}, 'radius ${_fmt(r * c.maxScale)} m');
      case 'capsule' when id.startsWith('radius.'):
        final r = math.max(_minLength, value.scalar / c.maxScale);
        return FieldEdit({'size': List.of(size)..[0] = r}, 'radius ${_fmt(r * c.maxScale)} m');
      case 'capsule' when id.startsWith('height.'):
        // The dot sits on the cap: the half height is what is left after
        // the radius.
        final h = math.max(0.0, value.scalar / c.maxScale - size[0]);
        return FieldEdit({'size': List.of(size)..[1] = h}, 'height ${_fmt(2 * (h + size[0]) * c.maxScale)} m');
    }
    return null;
  }
}

// ── Camera ─────────────────────────────────────────────────────────────

/// A camera: its field of view (or orthographic height) from dots on the
/// top and bottom of its frustum where the world draws it
/// (`CAMERA_OUTLINE_DEPTH` past the near plane).
final class CameraHandles extends ComponentHandles {
  const CameraHandles();

  @override
  String get type => 'fynkel.Camera';

  /// Where the frustum's outline stops past the near plane (the
  /// authoring crate's `CAMERA_OUTLINE_DEPTH`).
  static const outlineDepth = 4.0;

  static const minFov = math.pi / 180, maxFov = 179 * math.pi / 180;

  bool _ortho(ComponentContext c) => c.data['projection'] == 'orthographic';

  @override
  List<Handle> handles(ComponentContext c) {
    final o = c.origin, a = c.axes, fwd = vScale(a[2], -1), up = a[1];
    final near = math.max(1e-3, c.number('near', 0.1));
    final depth = math.min(c.number('far', 1000), near + outlineDepth);
    double half(double d) =>
        _ortho(c) ? c.number('ortho_height', 10) / 2 : d * math.tan(c.number('fov_y', 1) / 2);
    List<Vec3> rect(double d) {
      final h = half(d), w = h * c.aspect, at = vAdd(o, vScale(fwd, d));
      return [
        for (final (x, y) in const [(-1, -1), (1, -1), (1, 1), (-1, 1), (-1, -1)])
          vAdd(at, vAdd(vScale(a[0], x * w), vScale(up, y * h))),
      ];
    }

    final nearRect = rect(near), farRect = rect(depth);
    final guides = [
      nearRect,
      farRect,
      for (var i = 0; i < 4; i++) [nearRect[i], farRect[i]],
    ];
    final at = vAdd(o, vScale(fwd, depth)), h = half(depth);
    return [
      for (var s = 0; s < 2; s++)
        Handle(
          id: _ortho(c) ? 'height.${_signs[s]}y' : 'fov.${_signs[s]}y',
          shape: dot(c, vAdd(at, vScale(up, s == 0 ? h : -h)), guides: s == 0 ? guides : const []),
          color: _lightColor,
          constraint: RadiusConstraint(at, vScale(up, s == 0 ? 1 : -1), h),
          grab: vAdd(at, vScale(up, s == 0 ? h : -h)),
          width: HandleWidth.dot,
        ),
    ];
  }

  @override
  FieldEdit? edit(Handle handle, DragValue value, ComponentContext c) {
    if (handle.id.startsWith('height.')) {
      final h = math.max(_minLength, 2 * value.scalar);
      return FieldEdit({'ortho_height': h}, 'height ${_fmt(h)} m');
    }
    if (!handle.id.startsWith('fov.')) return null;
    final near = math.max(1e-3, c.number('near', 0.1));
    final depth = math.min(c.number('far', 1000), near + outlineDepth);
    final fov = (2 * math.atan2(value.scalar, depth)).clamp(minFov, maxFov);
    return FieldEdit({'fov_y': fov}, 'field of view ${_degrees(fov)}');
  }
}
