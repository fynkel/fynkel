// Declared gizmos (blueprint §19.2e item 9h, ADR-0019 §3): the editor
// never runs game code, so a behaviour describes its gizmos with
// `@Gizmo.radius()` and the rest on the fields they draw, and the
// generator writes them into the field's `x-fynkel` (`FieldSpec.gizmo`);
// built-in components get theirs from `native_hints.dart`. The Scene view
// draws them for the selection (or for every carrier, `when: always`),
// and behind the Inspector's Edit toggle each is a handle whose drag sets
// its field — one transaction, as every component handle.
//
// A value is in the entity's space (moved, turned and scaled with it) or,
// `space: world`, in the world's: a point or path is then absolute, while
// a radius, box, direction or cone keeps the world's axes but still sits
// at the entity.
import 'dart:math' as math;
import 'dart:ui' show Color;

import '../hierarchy/transform_math.dart' show identityMatrix, matInverseAffine;
import '../inspector/field_spec.dart';
import '../theme/tokens.dart';
import 'component_handles.dart';
import 'gizmo.dart';
import 'scene_math.dart';

/// One polyline of a declared gizmo, in linear RGBA.
typedef GizmoPolyline = ({List<Vec3> points, List<double> color});

/// The declared gizmos of one component type: its fields with a `gizmo`
/// hint, as a [ComponentHandles] so the component-handle machinery drags
/// them.
final class DeclaredGizmos extends ComponentHandles {
  DeclaredGizmos(this.type, this.fields, {this.siblings = const {}});

  /// [spec]'s declared gizmos; null when it has none.
  static DeclaredGizmos? of(ComponentSpec spec) {
    final fields = [
      for (final f in spec.fields)
        if (f.gizmo != null) f,
    ];
    if (fields.isEmpty) return null;
    return DeclaredGizmos(spec.type, fields, siblings: {for (final f in spec.fields) f.name: f});
  }

  @override
  final String type;

  /// The fields with a gizmo, in the spec's order.
  final List<FieldSpec> fields;

  /// Every field of the type, by name (a cone's angle field's bounds).
  final Map<String, FieldSpec> siblings;

  /// Whether any is drawn on every carrier, not only the selection.
  bool get anyAlways => fields.any((f) => f.gizmo!.always);

  /// Whether any has a handle (a `lineTo` has none).
  bool get draggable => fields.any((f) => f.gizmo!.draggable);

  FieldSpec? _field(String name) {
    for (final f in fields) {
      if (f.name == name) return f;
    }
    return null;
  }

  @override
  bool supports(Map<String, Object?> data) => draggable;

  /// [c] with the schema defaults under the fields the data leaves out
  /// (the runtime's values for them).
  ComponentContext _defaulted(ComponentContext c) {
    final missing = {
      for (final f in siblings.values)
        if (f.hasDefault && !c.data.containsKey(f.name)) f.name: f.defaultValue,
    };
    return missing.isEmpty ? c : c.withData({...missing, ...c.data});
  }

  // ── Frames ─────────────────────────────────────────────────────────

  /// The matrix [g]'s values are in: the entity's, or (world space) the
  /// world's — at the entity for a shape about it, the origin for a point
  /// or path.
  static List<double> frameOf(ComponentContext c, GizmoHint g) {
    if (!g.world) return c.world;
    if (g.shape == 'point' || g.shape == 'path') return identityMatrix;
    final o = c.origin;
    return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, o[0], o[1], o[2], 1];
  }

  static const _unit = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
  ];

  List<double> _color(FieldSpec f) =>
      GizmoTokens.linear(f.gizmo!.color == null ? GizmoTokens.declaredGizmo : Color(f.gizmo!.color!));

  // ── Drawing ────────────────────────────────────────────────────────

  /// The polylines of the gizmos [selected] decides on (`when: always`
  /// ones whatever it is). A `lineTo`'s target is an object reference
  /// (`{entity: id}`, ADR-0035/0036), whose position [findEntity] answers
  /// by entity id.
  List<GizmoPolyline> lines(
    ComponentContext context, {
    required bool selected,
    Vec3? Function(String id)? findEntity,
  }) {
    final c = _defaulted(context);
    final out = <GizmoPolyline>[];
    for (final f in fields) {
      final g = f.gizmo!;
      if (!selected && !g.always) continue;
      final color = _color(f);
      for (final l in _shape(c, f, findEntity)) {
        out.add((points: l, color: color));
      }
    }
    return out;
  }

  List<List<Vec3>> _shape(
    ComponentContext c,
    FieldSpec f,
    Vec3? Function(String id)? findEntity,
  ) {
    final g = f.gizmo!;
    final m = frameOf(c, g);
    final o = matTranslation(m);
    switch (g.shape) {
      case 'radius':
        final r = c.number(f.name, 0) * _maxScale(m);
        return [for (final a in matAxes(m)) circle(c.origin, a, r)];
      case 'box':
        final h = vScale(c.vector(f.name, const [1, 1, 1]), 0.5);
        final corners = [
          for (var i = 0; i < 8; i++)
            transformPoint(m, [i & 1 == 0 ? -h[0] : h[0], i & 2 == 0 ? -h[1] : h[1], i & 4 == 0 ? -h[2] : h[2]]),
        ];
        const edges = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
        return [
          for (final (i, j) in edges) [corners[i], corners[j]],
        ];
      case 'point':
        final p = transformPoint(m, c.vector(f.name, const [0, 0, 0]));
        return [..._cross(c, p), if (vDistance(p, c.origin) > 1e-9) [c.origin, p]];
      case 'direction':
        final tip = vAdd(o, transformDirection(m, c.vector(f.name, const [0, 0, -1])));
        return _arrow(c, o, tip);
      case 'cone':
        final (end, fwd, length) = _coneEnd(c, f, m);
        final r = length * math.tan(math.min(c.number(g.angle!, 0.5), _maxConeAngle));
        final a = matAxes(m);
        return [
          circle(end, fwd, r),
          for (final d in [a[0], a[1], vScale(a[0], -1), vScale(a[1], -1)]) [o, vAdd(end, vScale(d, r))],
        ];
      case 'lineTo':
        final target = switch (c.data[f.name]) {
          {'entity': final String id} => findEntity?.call(id),
          _ => null,
        };
        return target == null
            ? const []
            : [
                [c.origin, target],
              ];
      case 'path':
        final points = [for (final p in _points(c, f)) transformPoint(m, p)];
        return [
          if (points.length > 1) points,
          for (final p in points) ..._cross(c, p),
        ];
    }
    return const [];
  }

  /// A small three-axis cross at [p], a few points across on screen.
  List<List<Vec3>> _cross(ComponentContext c, Vec3 p) {
    final s = c.point(p) * GizmoTokens.dotSize / 2;
    return [
      for (final a in _unit) [vSub(p, vScale(a, s)), vAdd(p, vScale(a, s))],
    ];
  }

  List<List<Vec3>> _arrow(ComponentContext c, Vec3 from, Vec3 to) {
    final d = vSub(to, from);
    final len = vLength(d);
    if (len < 1e-9) return const [];
    final dir = vScale(d, 1 / len);
    final h = math.min(c.point(to) * 12, len * 0.5);
    final (u, v) = perpendiculars(dir);
    final back = vSub(to, vScale(dir, h));
    return [
      [from, to],
      for (final w in [u, vScale(u, -1), v, vScale(v, -1)]) [to, vAdd(back, vScale(w, h * 0.4))],
    ];
  }

  static const _maxConeAngle = 89 * math.pi / 180;

  /// A cone's end, axis (unit, −Z) and length in world metres.
  (Vec3, Vec3, double) _coneEnd(ComponentContext c, FieldSpec f, List<double> m) {
    final axis = transformDirection(m, const [0, 0, -1]);
    final scale = vLength(axis);
    final fwd = scale < 1e-12 ? const [0.0, 0.0, -1.0] : vScale(axis, 1 / scale);
    final length = c.number(f.name, 1) * scale;
    return (vAdd(matTranslation(m), vScale(fwd, length)), fwd, length);
  }

  static double _maxScale(List<double> m) => [for (final a in _unit) vLength(transformDirection(m, a))].reduce(math.max);

  List<Vec3> _points(ComponentContext c, FieldSpec f) {
    final v = c.data[f.name];
    if (v is! List) return const [];
    return [
      for (final p in v)
        if (p is List && p.length == 3 && p.every((e) => e is num)) [for (final e in p) (e as num).toDouble()],
    ];
  }

  // ── Handles ────────────────────────────────────────────────────────

  @override
  List<Handle> handles(ComponentContext context) {
    final c = _defaulted(context);
    final out = <Handle>[];
    for (final f in fields) {
      final g = f.gizmo!;
      if (!g.draggable) continue;
      final m = frameOf(c, g);
      final o = matTranslation(m);
      final color = _color(f);
      Handle dotAt(String id, Vec3 p, DragConstraint constraint) =>
          Handle(id: '${f.name}.$id', shape: dot(c, p), color: color, constraint: constraint, grab: p, width: HandleWidth.dot);
      switch (g.shape) {
        case 'radius':
          final r = c.number(f.name, 0) * _maxScale(m);
          final axes = matAxes(m);
          for (var i = 0; i < 3; i++) {
            for (final s in const [1.0, -1.0]) {
              final dir = vScale(axes[i], s);
              if (_facing(c, o, dir)) continue;
              out.add(dotAt('${s > 0 ? '+' : '-'}${'xyz'[i]}', vAdd(o, vScale(dir, r)), RadiusConstraint(o, dir, r)));
            }
          }
        case 'box':
          final h = vScale(c.vector(f.name, const [1, 1, 1]), 0.5);
          for (var i = 0; i < 3; i++) {
            for (final s in const [1.0, -1.0]) {
              final local = [for (var k = 0; k < 3; k++) k == i ? s * h[i] : 0.0];
              final at = transformPoint(m, local);
              final dir = vNormalize(transformDirection(m, vScale(_unit[i], s)));
              if (_facing(c, o, dir)) continue;
              out.add(dotAt('${s > 0 ? '+' : '-'}${'xyz'[i]}', at, AxisConstraint(at, dir)));
            }
          }
        case 'point':
          final p = transformPoint(m, c.vector(f.name, const [0, 0, 0]));
          out.add(dotAt('at', p, PlaneConstraint(p)));
        case 'direction':
          final tip = vAdd(o, transformDirection(m, c.vector(f.name, const [0, 0, -1])));
          out.add(dotAt('tip', tip, PlaneConstraint(tip)));
        case 'cone':
          final (end, fwd, length) = _coneEnd(c, f, m);
          out.add(dotAt('length', end, RadiusConstraint(o, fwd, length)));
          final r = length * math.tan(math.min(c.number(g.angle!, 0.5), _maxConeAngle));
          final axes = matAxes(m);
          for (var i = 0; i < 2; i++) {
            for (final s in const [1.0, -1.0]) {
              final dir = vScale(axes[i], s);
              out.add(dotAt('angle.${s > 0 ? '+' : '-'}${'xy'[i]}', vAdd(end, vScale(dir, r)), RadiusConstraint(end, dir, r)));
            }
          }
        case 'path':
          final points = _points(c, f);
          for (var i = 0; i < points.length; i++) {
            final p = transformPoint(m, points[i]);
            out.add(dotAt('$i', p, PlaneConstraint(p)));
          }
      }
    }
    return out;
  }

  /// A dot along [dir] from [at] would sit on [at] as the camera sees it.
  static bool _facing(ComponentContext c, Vec3 at, Vec3 dir) => vDot(vNormalize(dir), c.toCamera(at)).abs() > 0.97;

  @override
  FieldEdit? edit(Handle handle, DragValue value, ComponentContext context) {
    final c = _defaulted(context);
    final dot = handle.id.indexOf('.');
    if (dot < 0) return null;
    final f = _field(handle.id.substring(0, dot));
    final part = handle.id.substring(dot + 1);
    if (f == null) return null;
    final g = f.gizmo!;
    final m = frameOf(c, g);
    final o = matTranslation(m);
    final inverse = matInverseAffine(m);
    switch (g.shape) {
      case 'radius':
        final ms = _maxScale(m);
        final r = _clamp(f, ms < 1e-12 ? 0 : value.scalar / ms);
        return FieldEdit({f.name: r}, '${f.label} ${fmtAmount(r.toDouble())}${_suffix(f)}');
      case 'box':
        final i = 'xyz'.indexOf(part[1]);
        final size = List.of(c.vector(f.name, const [1, 1, 1]));
        final scale = vLength(transformDirection(m, _unit[i]));
        if (scale < 1e-12) return null;
        size[i] = math.max(_minLength, _clamp(f, size[i] + 2 * value.scalar / scale).toDouble());
        return FieldEdit({f.name: size}, '${'xyz'[i]} ${fmtAmount(size[i])}${_suffix(f)}');
      case 'point':
        final p = transformPoint(m, c.vector(f.name, const [0, 0, 0]));
        final local = transformPoint(inverse, vAdd(p, value.vector));
        return FieldEdit({f.name: local}, '${f.label} ${_vec(local)}');
      case 'direction':
        final tip = vAdd(o, transformDirection(m, c.vector(f.name, const [0, 0, -1])));
        final d = transformDirection(inverse, vSub(vAdd(tip, value.vector), o));
        return FieldEdit({f.name: d}, '${f.label} ${_vec(d)}${_suffix(f)}');
      case 'cone':
        final (_, _, length) = _coneEnd(c, f, m);
        final scale = vLength(transformDirection(m, const [0, 0, -1]));
        if (part == 'length') {
          final l = _clamp(f, scale < 1e-12 ? 0 : value.scalar / scale);
          return FieldEdit({f.name: l}, '${f.label} ${fmtAmount(l.toDouble())}${_suffix(f)}');
        }
        final angleField = siblings[g.angle!];
        final a = math.atan2(math.max(0.0, value.scalar), math.max(length, 1e-9)).clamp(_minLength, _maxConeAngle);
        final clamped = angleField == null ? a : _clamp(angleField, a);
        return FieldEdit({g.angle!: clamped}, '${angleField?.label ?? g.angle!} ${fmtDegrees(clamped.toDouble())}');
      case 'path':
        final i = int.tryParse(part);
        final points = _points(c, f).map(List<double>.of).toList();
        if (i == null || i >= points.length) return null;
        final p = transformPoint(m, points[i]);
        points[i] = transformPoint(inverse, vAdd(p, value.vector));
        return FieldEdit({f.name: points}, 'point $i ${_vec(points[i])}');
    }
    return null;
  }

  static const _minLength = 0.001;

  /// [v] within the field's bounds, whole for an integer field.
  static num _clamp(FieldSpec f, double v) {
    var x = v;
    if (f.min != null) x = math.max(x, f.min!.toDouble());
    if (f.max != null) x = math.min(x, f.max!.toDouble());
    if (f.min == null) x = math.max(x, 0);
    return f.kind == FieldKind.integer ? x.round() : x;
  }

  static String _suffix(FieldSpec f) => f.unit == null ? '' : ' ${f.unit}';

  static String _vec(Vec3 v) => '(${v.map(fmtAmount).join(', ')})';
}
