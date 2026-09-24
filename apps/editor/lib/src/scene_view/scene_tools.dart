// The Scene view's tools (blueprint §19.2e item 9g, ADR-0019 §2): each is
// a registered object — an id, a label, an icon, a key and the handles it
// puts on the selection — instead of a case in every switch of the view
// model. A tool turns a handle's constrained drag value into an edit;
// the model previews it, and commits it as one transaction on release.
//
// Move, rotate, scale and rect are registrations over the handle
// framework (`gizmo.dart`); the transform tool is the first three
// composed — the proof that another tool costs no surgery.
import 'dart:math' as math;

import 'package:flutter/services.dart' show LogicalKeyboardKey;
import 'package:flutter/widgets.dart' show IconData;

import '../hierarchy/transform_math.dart' show matMul;
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'editor_state.dart' show SnapSettings;
import 'gizmo.dart';
import 'scene_math.dart';

/// The transform gizmo's frame: the pivot, its three unit axes (world or
/// the lead's local) and the length in metres of [screenSize] points
/// there.
final class GizmoFrame {
  const GizmoFrame({required this.pivot, required this.axes, required this.size, this.box});

  final Vec3 pivot;
  final List<Vec3> axes;
  final double size;

  /// The selection's bounds (children included) along [axes], relative
  /// to [pivot]: the lowest and highest coordinate on each axis. The Rect
  /// tool's rectangle; null when nothing shown has a position.
  final (Vec3, Vec3)? box;

  /// Points (logical pixels, so the same on a Retina screen) a handle
  /// spans on screen.
  static const screenSize = 80.0;

  static const worldAxes = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
  ];
}

/// What a tool sees when it builds handles or reads a drag.
final class ToolContext {
  ToolContext({
    required this.frame,
    required this.vp,
    this.snap = const SnapSettings(),
    this.snapping = false,
    this.uniform = false,
    this.fromCenter = false,
  });

  final GizmoFrame frame;
  final ViewProjection vp;
  final SnapSettings snap;

  /// Snapping is on for this drag (the setting, inverted by ⌘).
  final bool snapping;

  /// ⇧ held: the Rect tool keeps the rectangle's proportions.
  final bool uniform;

  /// ⌥ held: the Rect tool resizes about the rectangle's centre.
  final bool fromCenter;

  /// The camera's right, up and forward axes.
  late final List<Vec3> camera = () {
    final a = matAxes(vp.world);
    return [a[0], a[1], vScale(a[2], -1)];
  }();

  /// The unit direction from the pivot to the camera (the view's back
  /// axis in orthographic).
  late final Vec3 toCamera = vp.camera.orthographic
      ? vScale(camera[2], -1)
      : vNormalize(vSub(matTranslation(vp.world), frame.pivot));

  /// Metres per point at the pivot.
  double get pixel => frame.size / GizmoFrame.screenSize;

  /// Physical pixels per point.
  double get pixelRatio => frame.size / vp.metresPerPixel(frame.pivot) / GizmoFrame.screenSize;
}

/// Which parts of a transform a tool's edit changes: a move keeps the
/// rotation and scale bits exactly, a rotation the scale.
enum TransformChannel { move, rotate, scale }

/// A tool's answer to a drag: a transform about the pivot (a matrix
/// applied to every target's world matrix), the pivot after it and the
/// readout under the picture.
final class TransformEdit {
  const TransformEdit({
    required this.delta,
    required this.pivot,
    required this.readout,
    required this.channel,
    this.factors,
    this.movesObjects = false,
  });

  final List<double> delta;

  /// A scale's factors along the gizmo's axes: each object's own scale
  /// is multiplied by them, as Unity multiplies `localScale`.
  final Vec3? factors;

  /// A scale that moves the objects with the delta even in Pivot mode:
  /// the Rect tool resizes about the opposite edge, so what it resizes
  /// moves with the edge.
  final bool movesObjects;
  final Vec3 pivot;
  final String readout;
  final TransformChannel channel;
}

/// A Scene view tool.
abstract class SceneTool {
  const SceneTool();

  /// Stable: saved in `.fynkel/editor.json`, part of command ids.
  String get id;
  String get label;
  IconData get icon;

  /// The key that picks it while the Scene view has focus.
  LogicalKeyboardKey? get key => null;

  String get name => id;

  /// Scale works in the lead's own axes whatever the space setting.
  bool get alwaysLocal => false;

  /// Whether the tool shows a gizmo on the selection at all.
  bool get hasGizmo => true;

  /// Whether its handles need the selection's bounds ([GizmoFrame.box]).
  bool get needsBox => false;

  /// The handles on the selection, in priority order (the first one the
  /// pointer is inside wins).
  List<Handle> handles(ToolContext c);

  /// What dragging [handle] by [value] does; null ignores the move.
  TransformEdit? edit(Handle handle, DragValue value, ToolContext c);

  static const select = SelectTool();
  static const move = MoveTool();
  static const rotate = RotateTool();
  static const scale = ScaleTool();
  static const rect = RectTool();
  static const transform = TransformTool();

  /// Every registered tool, in toolbar order.
  static List<SceneTool> get values => SceneTools.all;

  @override
  String toString() => 'SceneTool($id)';
}

/// The registry: the toolbar, the commands, the keys and `editor.json`
/// all read it.
abstract final class SceneTools {
  static final _all = <SceneTool>[
    SceneTool.select,
    SceneTool.move,
    SceneTool.rotate,
    SceneTool.scale,
    SceneTool.rect,
    SceneTool.transform,
  ];

  static List<SceneTool> get all => List.unmodifiable(_all);

  /// Adds [tool] after the built-in ones. Its id must be new.
  static void register(SceneTool tool) {
    if (_all.any((t) => t.id == tool.id)) throw ArgumentError('a tool "${tool.id}" is already registered');
    _all.add(tool);
  }

  /// Removes a tool [register] added (tests, a closing plug-in).
  static void unregister(String id) {
    if (const ['select', 'move', 'rotate', 'scale', 'rect', 'transform'].contains(id)) {
      throw ArgumentError('"$id" is built in');
    }
    _all.removeWhere((t) => t.id == id);
  }

  static SceneTool? byId(String? id) {
    for (final t in _all) {
      if (t.id == id) return t;
    }
    return null;
  }

  static SceneTool? byKey(LogicalKeyboardKey key) {
    for (final t in _all) {
      if (t.key == key) return t;
    }
    return null;
  }
}

List<double> _axisColor(int i) => GizmoTokens.linear(GizmoTokens.axisColors[i]);
const _axisNames = ['x', 'y', 'z'];

String _fmt(double v) {
  final r = (v * 1000).roundToDouble() / 1000;
  return r == r.roundToDouble() ? r.toInt().toString() : r.toStringAsFixed(r.abs() < 0.1 ? 3 : 2);
}

List<double> _translation(Vec3 v) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, v[0], v[1], v[2], 1];

/// [m] applied about [pivot].
List<double> _about(Vec3 pivot, List<double> m) =>
    matMul(_translation(pivot), matMul(m, _translation(vScale(pivot, -1))));

/// Select: no gizmo, clicks pick.
final class SelectTool extends SceneTool {
  const SelectTool();
  @override
  String get id => 'select';
  @override
  String get label => 'Select';
  @override
  IconData get icon => FIcons.nearMeOutlined;
  @override
  LogicalKeyboardKey get key => LogicalKeyboardKey.keyQ;
  @override
  bool get hasGizmo => false;
  @override
  List<Handle> handles(ToolContext c) => const [];
  @override
  TransformEdit? edit(Handle handle, DragValue value, ToolContext c) => null;
}

/// Move: an arrow per axis, a square per plane (on the camera's side of
/// the pivot) and the view-plane square in the middle.
final class MoveTool extends SceneTool {
  const MoveTool({this.viewSquare = true});

  /// The transform tool puts the uniform scale there instead.
  final bool viewSquare;

  @override
  String get id => 'move';
  @override
  String get label => 'Move';
  @override
  IconData get icon => FIcons.openWith;
  @override
  LogicalKeyboardKey get key => LogicalKeyboardKey.keyW;

  @override
  List<Handle> handles(ToolContext c) {
    final f = c.frame, p = f.pivot, s = f.size;
    final out = <Handle>[];
    if (viewSquare) {
      final r = vScale(c.camera[0], 7 * c.pixel), u = vScale(c.camera[1], 7 * c.pixel);
      final corners = [
        vSub(vSub(p, r), u),
        vSub(vAdd(p, r), u),
        vAdd(vAdd(p, r), u),
        vAdd(vSub(p, r), u),
      ];
      out.add(
        Handle(
          id: 'move.view',
          shape: HandleShape(
            lines: [
              [...corners, corners.first],
            ],
            triangles: quad(corners[0], corners[1], corners[2], corners[3]),
            fill: 0.3,
          ),
          color: GizmoTokens.linear(GizmoTokens.neutral),
          constraint: PlaneConstraint(p),
          grab: p,
          width: HandleWidth.line,
        ),
      );
    }
    for (final (a, b, n) in const [(0, 1, 2), (1, 2, 0), (0, 2, 1)]) {
      // A plane seen edge-on is a sliver nobody can aim at: hidden.
      if (vDot(f.axes[n], c.toCamera).abs() < 0.1) continue;
      // On the camera's side of each axis, as Unity flips them.
      final da = vScale(f.axes[a], vDot(f.axes[a], c.toCamera) >= 0 ? 1 : -1);
      final db = vScale(f.axes[b], vDot(f.axes[b], c.toCamera) >= 0 ? 1 : -1);
      Vec3 at(double x, double y) => vAdd(p, vAdd(vScale(da, x * s), vScale(db, y * s)));
      const lo = 0.18, hi = 0.42;
      final corners = [at(lo, lo), at(hi, lo), at(hi, hi), at(lo, hi)];
      out.add(
        Handle(
          id: 'move.${_axisNames[a]}${_axisNames[b]}',
          shape: HandleShape(
            lines: [
              [...corners, corners.first],
            ],
            triangles: quad(corners[0], corners[1], corners[2], corners[3]),
            fill: 0.35,
          ),
          color: _axisColor(n),
          constraint: PlaneConstraint(p, f.axes[n]),
          grab: at((lo + hi) / 2, (lo + hi) / 2),
          width: HandleWidth.line,
        ),
      );
    }
    for (var i = 0; i < 3; i++) {
      final axis = f.axes[i];
      final base = vAdd(p, vScale(axis, s * 0.8)), tip = vAdd(p, vScale(axis, s));
      out.add(
        Handle(
          id: 'move.${_axisNames[i]}',
          shape: HandleShape(
            lines: [
              [p, base],
            ],
            triangles: cone(base, tip, s * 0.07),
          ),
          color: _axisColor(i),
          constraint: AxisConstraint(p, axis),
          grab: vAdd(p, vScale(axis, s * 0.88)),
        ),
      );
    }
    return out;
  }

  @override
  TransformEdit? edit(Handle handle, DragValue value, ToolContext c) {
    final f = c.frame;
    Vec3 move;
    String readout;
    switch (handle.id) {
      case 'move.x' || 'move.y' || 'move.z':
        final i = _axisNames.indexOf(handle.id.substring(5));
        var s = value.scalar;
        if (c.snapping) s = snapTo(s, c.snap.move);
        move = vScale(f.axes[i], s);
        readout = '${_fmt(s)} m';
      case 'move.xy' || 'move.yz' || 'move.xz':
        final a = _axisNames.indexOf(handle.id[5]), b = _axisNames.indexOf(handle.id[6]);
        move = value.vector;
        if (c.snapping) {
          final sa = snapTo(vDot(move, f.axes[a]), c.snap.move);
          final sb = snapTo(vDot(move, f.axes[b]), c.snap.move);
          move = vAdd(vScale(f.axes[a], sa), vScale(f.axes[b], sb));
        }
        readout = '${_fmt(vLength(move))} m';
      case 'move.view':
        move = value.vector;
        if (c.snapping) move = [for (final v in move) snapTo(v, c.snap.move)];
        readout = '${_fmt(vLength(move))} m';
      default:
        return null;
    }
    return TransformEdit(
      delta: _translation(move),
      pivot: vAdd(f.pivot, move),
      readout: readout,
      channel: TransformChannel.move,
    );
  }
}

/// Rotate: a ring per axis (the far half faint), a ring about the view
/// axis around them and, inside, Unity's trackball: a drag anywhere on
/// the sphere the rings draw turns freely.
final class RotateTool extends SceneTool {
  const RotateTool({this.radius = 1, this.viewRing = true, this.trackball = true});

  /// The rings' radius in gizmo sizes (the transform tool's sit outside
  /// the move arrows).
  final double radius;
  final bool viewRing;

  /// The free rotation inside the rings (the transform tool has its move
  /// handles there instead).
  final bool trackball;

  @override
  String get id => 'rotate';
  @override
  String get label => 'Rotate';
  @override
  IconData get icon => FIcons.rotateRight;
  @override
  LogicalKeyboardKey get key => LogicalKeyboardKey.keyE;

  @override
  List<Handle> handles(ToolContext c) {
    final f = c.frame, p = f.pivot, r = f.size * radius;
    final out = <Handle>[];
    for (var i = 0; i < 3; i++) {
      final axis = f.axes[i];
      // The grab point: halfway between the ring's side and its point
      // nearest the camera — on the front half, never on the pivot.
      final along = vDot(c.toCamera, axis);
      var front = vSub(c.toCamera, vScale(axis, along));
      front = vLength(front) < 1e-3 ? perpendiculars(axis).$1 : vNormalize(front);
      final side = vCross(axis, front);
      out.add(
        Handle(
          id: 'rotate.${_axisNames[i]}',
          shape: ring(p, axis, r, c.toCamera),
          color: _axisColor(i),
          constraint: RingConstraint(axis, facingCamera: along >= 0),
          grab: vAdd(p, vScale(vNormalize(vAdd(front, side)), r)),
        ),
      );
    }
    if (viewRing) {
      out.add(
        Handle(
          id: 'rotate.view',
          shape: HandleShape(lines: [circle(p, c.camera[2], r * 1.2)]),
          color: GizmoTokens.linear(GizmoTokens.neutral),
          constraint: const RingConstraint(null, facingCamera: false),
          grab: vAdd(p, vScale(c.camera[0], r * 1.2)),
        ),
      );
    }
    if (trackball) {
      out.add(
        Handle(
          id: 'rotate.free',
          // The sphere's silhouette, faintly shaded, as Unity draws it.
          shape: HandleShape(triangles: disc(p, c.camera[2], r), guides: [circle(p, c.camera[2], r)], fill: 0.08),
          color: GizmoTokens.linear(GizmoTokens.neutral),
          constraint: TrackballConstraint(
            right: c.camera[0],
            up: c.camera[1],
            toCamera: c.toCamera,
            pixelRatio: c.pixelRatio,
          ),
          grab: vAdd(p, vScale(vAdd(c.camera[0], c.camera[1]), r * 0.3)),
          background: true,
        ),
      );
    }
    return out;
  }

  @override
  TransformEdit? edit(Handle handle, DragValue value, ToolContext c) {
    if (!handle.id.startsWith('rotate.')) return null;
    var angle = value.scalar;
    if (c.snapping) angle = snapTo(angle, c.snap.angle * math.pi / 180);
    return TransformEdit(
      delta: _about(c.frame.pivot, quatToMatrix(quatFromAxisAngle(value.vector, angle))),
      pivot: List.of(c.frame.pivot),
      readout: '${_fmt(angle * 180 / math.pi)}°',
      channel: TransformChannel.rotate,
    );
  }
}

/// Scale: a cube-tipped line per local axis and a cube in the middle for
/// a uniform scale.
final class ScaleTool extends SceneTool {
  const ScaleTool({this.axisHandles = true});

  /// The transform tool keeps only the uniform cube.
  final bool axisHandles;

  @override
  String get id => 'scale';
  @override
  String get label => 'Scale';
  @override
  IconData get icon => FIcons.aspectRatio;
  @override
  LogicalKeyboardKey get key => LogicalKeyboardKey.keyR;
  @override
  bool get alwaysLocal => true;

  @override
  List<Handle> handles(ToolContext c) {
    final f = c.frame, p = f.pivot, s = f.size;
    final out = <Handle>[
      Handle(
        id: 'scale.uniform',
        shape: HandleShape(triangles: cube(p, f.axes, s * 0.07)),
        color: GizmoTokens.linear(GizmoTokens.neutral),
        constraint: ScreenScaleConstraint(pixelRatio: c.pixelRatio),
        grab: p,
      ),
    ];
    if (!axisHandles) return out;
    for (var i = 0; i < 3; i++) {
      final end = vAdd(p, vScale(f.axes[i], s * 0.92));
      out.add(
        Handle(
          id: 'scale.${_axisNames[i]}',
          shape: HandleShape(
            lines: [
              [p, end],
            ],
            triangles: cube(end, f.axes, s * 0.055),
          ),
          color: _axisColor(i),
          constraint: AxisConstraint(p, f.axes[i]),
          grab: end,
        ),
      );
    }
    return out;
  }

  @override
  TransformEdit? edit(Handle handle, DragValue value, ToolContext c) {
    final f = c.frame;
    List<double> factors;
    double factor;
    switch (handle.id) {
      case 'scale.x' || 'scale.y' || 'scale.z':
        factor = 1 + value.scalar / f.size;
        if (c.snapping) factor = snapTo(factor, c.snap.scale);
        factor = math.max(factor, 0.001);
        factors = [1, 1, 1];
        factors[_axisNames.indexOf(handle.id.substring(6))] = factor;
      case 'scale.uniform':
        factor = value.scalar;
        if (c.snapping) factor = snapTo(factor, c.snap.scale);
        factor = math.max(factor, 0.001);
        factors = [factor, factor, factor];
      default:
        return null;
    }
    return TransformEdit(
      delta: _about(f.pivot, _alongAxes(f.axes, factors)),
      pivot: List.of(f.pivot),
      readout: '×${_fmt(factor)}',
      channel: TransformChannel.scale,
      factors: factors,
    );
  }
}

/// A scale by [factors] along [axes]: A·diag(f)·Aᵀ.
List<double> _alongAxes(List<Vec3> axes, List<double> factors) {
  final m = List<double>.filled(16, 0);
  for (var r = 0; r < 3; r++) {
    for (var col = 0; col < 3; col++) {
      var v = 0.0;
      for (var k = 0; k < 3; k++) {
        v += axes[k][r] * factors[k] * axes[k][col];
      }
      m[col * 4 + r] = v;
    }
  }
  m[15] = 1;
  return m;
}

/// Unity's Rect tool (T) for 3D objects: a rectangle around the selection
/// (children included) in the plane of its two local axes most across the
/// view. A corner resizes both ways and an edge one, about the opposite
/// side (⌥: about the centre; ⇧: keeping the proportions) — the objects
/// move with the edge, their own scale multiplied; a drag inside moves
/// them in the plane; just outside a corner turns them about the plane's
/// normal.
final class RectTool extends SceneTool {
  const RectTool();

  @override
  String get id => 'rect';
  @override
  String get label => 'Rect';
  @override
  IconData get icon => FIcons.rectTool;
  @override
  LogicalKeyboardKey get key => LogicalKeyboardKey.keyT;
  @override
  bool get alwaysLocal => true;
  @override
  bool get needsBox => true;

  /// The rectangle for [c]: the frame's axes in its plane (a, b), its
  /// normal, the low and high coordinates along a and b, the plane's depth.
  static ({int a, int b, int n, double loA, double hiA, double loB, double hiB, double depth})? _rect(ToolContext c) {
    final box = c.frame.box;
    if (box == null) return null;
    final axes = c.frame.axes;
    var n = 0;
    for (var i = 1; i < 3; i++) {
      if (vDot(axes[i], c.toCamera).abs() > vDot(axes[n], c.toCamera).abs()) n = i;
    }
    final (a, b) = switch (n) {
      0 => (1, 2),
      1 => (0, 2),
      _ => (0, 1),
    };
    final (lo, hi) = box;
    return (a: a, b: b, n: n, loA: lo[a], hiA: hi[a], loB: lo[b], hiB: hi[b], depth: (lo[n] + hi[n]) / 2);
  }

  static Vec3 _at(ToolContext c, ({int a, int b, int n, double loA, double hiA, double loB, double hiB, double depth}) r,
      double x, double y) {
    final f = c.frame;
    return vAdd(
      f.pivot,
      vAdd(vScale(f.axes[r.a], x), vAdd(vScale(f.axes[r.b], y), vScale(f.axes[r.n], r.depth))),
    );
  }

  @override
  List<Handle> handles(ToolContext c) {
    final r = _rect(c);
    if (r == null) return const [];
    final f = c.frame;
    final plane = PlaneConstraint(_at(c, r, 0, 0), f.axes[r.n]);
    final neutral = GizmoTokens.linear(GizmoTokens.neutral);
    final xs = [r.loA, r.hiA], ys = [r.loB, r.hiB];
    Vec3 corner(int i, int j) => _at(c, r, xs[i], ys[j]);
    final out = <Handle>[];
    // The corners: small squares facing the camera.
    final hr = vScale(c.camera[0], 4 * c.pixel), hu = vScale(c.camera[1], 4 * c.pixel);
    for (final (i, j) in const [(0, 0), (1, 0), (1, 1), (0, 1)]) {
      final p = corner(i, j);
      final q = [vSub(vSub(p, hr), hu), vSub(vAdd(p, hr), hu), vAdd(vAdd(p, hr), hu), vAdd(vSub(p, hr), hu)];
      out.add(
        Handle(
          id: 'rect.corner.$i$j',
          shape: HandleShape(
            lines: [
              [...q, q.first],
            ],
            triangles: quad(q[0], q[1], q[2], q[3]),
          ),
          color: neutral,
          constraint: plane,
          grab: p,
          width: HandleWidth.line,
        ),
      );
    }
    // The edges: along b at each end of a, along a at each end of b.
    for (final k in const [0, 1]) {
      out.add(
        Handle(
          id: 'rect.edge.a$k',
          shape: HandleShape(
            lines: [
              [corner(k, 0), corner(k, 1)],
            ],
          ),
          color: neutral,
          constraint: plane,
          grab: _at(c, r, xs[k], (r.loB + r.hiB) / 2),
          width: HandleWidth.line,
        ),
      );
      out.add(
        Handle(
          id: 'rect.edge.b$k',
          shape: HandleShape(
            lines: [
              [corner(0, k), corner(1, k)],
            ],
          ),
          color: neutral,
          constraint: plane,
          grab: _at(c, r, (r.loA + r.hiA) / 2, ys[k]),
          width: HandleWidth.line,
        ),
      );
    }
    // Just outside each corner: turn about the normal (unseen, as in Unity).
    final zone = 14 * c.pixel;
    for (final (i, j) in const [(0, 0), (1, 0), (1, 1), (0, 1)]) {
      final p = corner(i, j);
      final da = vScale(f.axes[r.a], i == 0 ? -1 : 1), db = vScale(f.axes[r.b], j == 0 ? -1 : 1);
      Vec3 at(double u, double v) => vAdd(p, vAdd(vScale(da, u), vScale(db, v)));
      out.add(
        Handle(
          id: 'rect.rotate.$i$j',
          shape: HandleShape(triangles: quad(at(0, 0), at(zone, 0), at(zone, zone), at(0, zone)), fill: 0),
          color: neutral,
          constraint: RingConstraint(f.axes[r.n], facingCamera: vDot(f.axes[r.n], c.toCamera) >= 0),
          grab: at(zone * 0.6, zone * 0.6),
        ),
      );
    }
    // The inside: move in the plane.
    out.add(
      Handle(
        id: 'rect.move',
        shape: HandleShape(triangles: quad(corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)), fill: 0),
        color: neutral,
        constraint: plane,
        grab: _at(c, r, (r.loA + r.hiA) / 2, (r.loB + r.hiB) / 2),
        background: true,
      ),
    );
    return out;
  }

  @override
  TransformEdit? edit(Handle handle, DragValue value, ToolContext c) {
    final r = _rect(c);
    if (r == null) return null;
    final f = c.frame;
    final id = handle.id;
    if (id == 'rect.move') {
      var move = value.vector;
      if (c.snapping) move = [for (final v in move) snapTo(v, c.snap.move)];
      return TransformEdit(
        delta: _translation(move),
        pivot: vAdd(f.pivot, move),
        readout: '${_fmt(vLength(move))} m',
        channel: TransformChannel.move,
      );
    }
    if (id.startsWith('rect.rotate.')) {
      var angle = value.scalar;
      if (c.snapping) angle = snapTo(angle, c.snap.angle * math.pi / 180);
      return TransformEdit(
        delta: _about(f.pivot, quatToMatrix(quatFromAxisAngle(value.vector, angle))),
        pivot: List.of(f.pivot),
        readout: '${_fmt(angle * 180 / math.pi)}°',
        channel: TransformChannel.rotate,
      );
    }
    // Which side of a and of b the handle drags (null: that way stays).
    final (int? sideA, int? sideB) = switch (id) {
      'rect.corner.00' => (0, 0),
      'rect.corner.10' => (1, 0),
      'rect.corner.11' => (1, 1),
      'rect.corner.01' => (0, 1),
      'rect.edge.a0' => (0, null),
      'rect.edge.a1' => (1, null),
      'rect.edge.b0' => (null, 0),
      'rect.edge.b1' => (null, 1),
      _ => (null, null),
    };
    if (sideA == null && sideB == null) return null;
    final v = value.vector;
    // The new extent over the old along one axis, and the coordinate that stays.
    (double, double) resize(double lo, double hi, int? side, double d) {
      final e = hi - lo;
      if (side == null || e.abs() < 1e-9) return (1, lo);
      final grow = (side == 1 ? d : -d) * (c.fromCenter ? 2 : 1);
      var next = e + grow;
      if (c.snapping) next = snapTo(next, c.snap.move);
      return (math.max(next / e, 0.001), c.fromCenter ? (lo + hi) / 2 : (side == 1 ? lo : hi));
    }

    var (fa, anchorA) = resize(r.loA, r.hiA, sideA, vDot(v, f.axes[r.a]));
    var (fb, anchorB) = resize(r.loB, r.hiB, sideB, vDot(v, f.axes[r.b]));
    if (c.uniform) {
      final u = sideA == null
          ? fb
          : sideB == null
          ? fa
          : ((fa - 1).abs() >= (fb - 1).abs() ? fa : fb);
      fa = u;
      fb = u;
      // An edge keeps the other way centred.
      if (sideA == null) anchorA = (r.loA + r.hiA) / 2;
      if (sideB == null) anchorB = (r.loB + r.hiB) / 2;
    }
    final factors = [1.0, 1.0, 1.0];
    factors[r.a] = fa;
    factors[r.b] = fb;
    final delta = _about(_at(c, r, anchorA, anchorB), _alongAxes(f.axes, factors));
    return TransformEdit(
      delta: delta,
      pivot: transformPoint(delta, f.pivot),
      readout: '${_fmt((r.hiA - r.loA) * fa)} × ${_fmt((r.hiB - r.loB) * fb)} m',
      channel: TransformChannel.scale,
      factors: factors,
      movesObjects: true,
    );
  }
}

/// Move, rotate and scale at once (Unity's transform tool): the move
/// arrows and planes, the rotate rings outside them, the uniform scale
/// cube in the middle. Built from the three, so it has no drag code of its
/// own.
final class TransformTool extends SceneTool {
  const TransformTool();

  static const _parts = [
    ScaleTool(axisHandles: false),
    MoveTool(viewSquare: false),
    RotateTool(radius: 1.25, viewRing: false, trackball: false),
  ];

  @override
  String get id => 'transform';
  @override
  String get label => 'Transform';
  @override
  IconData get icon => FIcons.transformTool;
  @override
  LogicalKeyboardKey get key => LogicalKeyboardKey.keyY;

  @override
  List<Handle> handles(ToolContext c) => [for (final t in _parts) ...t.handles(c)];

  @override
  TransformEdit? edit(Handle handle, DragValue value, ToolContext c) {
    for (final t in _parts) {
      final e = t.edit(handle, value, c);
      if (e != null) return e;
    }
    return null;
  }
}
