// The orientation gizmo (blueprint item 9e): in the Scene view's top right,
// the world's three axes as the camera sees them — the positive ends
// labelled X Y Z in the axis colours, the negative ends as dots, the far
// ends drawn first. A click on an end looks at the pivot from that side
// (again: from the opposite side), a drag orbits. The caption names the
// view ("Top · Ortho") when the camera looks straight along an axis, and a
// click on it switches perspective / orthographic; so does a click in the
// centre, unless an end covers it (looking along an axis, that axis's end
// sits there, and the click is the end's: what you see is what you click).
import 'dart:ui' show ImageFilter;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import '../theme/theme.dart' show uiFontFamily;
import '../theme/tokens.dart';
import 'scene_math.dart';

/// The axis colours of the Scene view: X, Y, Z (the move gizmo's too).
const sceneAxisColors = GizmoTokens.axisColors;

/// One end of an axis as the gizmo draws it.
final class GizmoAxisEnd {
  const GizmoAxisEnd(this.axis, this.positive, this.offset, this.depth);

  /// 0 X, 1 Y, 2 Z.
  final int axis;
  final bool positive;

  /// From the gizmo's centre, logical pixels, +Y down.
  final Offset offset;

  /// Along the view direction: positive ends point away from the viewer.
  final double depth;

  String get label => '${positive ? '' : '−'}${'XYZ'[axis]}';
}

/// What the pointer is over.
sealed class _Target {
  const _Target();
}

final class _End extends _Target {
  const _End(this.end);
  final GizmoAxisEnd end;

  @override
  bool operator ==(Object other) => other is _End && other.end.axis == end.axis && other.end.positive == end.positive;

  @override
  int get hashCode => Object.hash(end.axis, end.positive);
}

final class _Centre extends _Target {
  const _Centre();

  @override
  bool operator ==(Object other) => other is _Centre;

  @override
  int get hashCode => 0;
}

class OrientationGizmo extends StatefulWidget {
  const OrientationGizmo({
    super.key,
    required this.camera,
    required this.onLookFrom,
    required this.onToggleProjection,
    required this.onOrbit,
  });

  final EditorCamera camera;
  final void Function(int axis, bool positive) onLookFrom;
  final VoidCallback onToggleProjection;

  /// A drag's movement, logical pixels.
  final ValueChanged<Offset> onOrbit;

  /// The circle's diameter.
  static const diameter = 84.0;

  /// The widget's width, the caption's room ("Bottom · Ortho").
  static const width = 104.0;

  /// Centre to an end.
  static const arm = 29.0;
  static const endRadius = 8.0;
  static const dotRadius = 6.0;
  static const centreRadius = 9.0;

  /// The six ends for [camera], the far ones first (the order they are
  /// drawn in; hit tests go the other way).
  static List<GizmoAxisEnd> endsFor(EditorCamera camera) {
    final r = camera.right, u = camera.up, f = camera.forward;
    final ends = [
      for (var axis = 0; axis < 3; axis++)
        for (final positive in const [true, false])
          GizmoAxisEnd(
            axis,
            positive,
            Offset(r[axis], -u[axis]) * (positive ? arm : -arm),
            positive ? f[axis] : -f[axis],
          ),
    ];
    ends.sort((a, b) => b.depth.compareTo(a.depth));
    return ends;
  }

  /// The end under [local] (from the centre), the nearest to the viewer first.
  static GizmoAxisEnd? endAt(EditorCamera camera, Offset local) {
    for (final e in endsFor(camera).reversed) {
      if ((local - e.offset).distance <= (e.positive ? endRadius : dotRadius) + 2) return e;
    }
    return null;
  }

  @override
  State<OrientationGizmo> createState() => _OrientationGizmoState();
}

class _OrientationGizmoState extends State<OrientationGizmo> {
  _Target? _hover;
  Offset? _downAt;
  bool _dragging = false;

  static const _centre = Offset(OrientationGizmo.diameter / 2, OrientationGizmo.diameter / 2);

  _Target? _targetAt(Offset local) {
    final p = local - _centre;
    final end = OrientationGizmo.endAt(widget.camera, p);
    if (end != null) return _End(end);
    if (p.distance <= OrientationGizmo.centreRadius) return const _Centre();
    return null;
  }

  void _setHover(_Target? t) {
    if (t != _hover) setState(() => _hover = t);
  }

  void _down(PointerDownEvent e) {
    if (e.buttons != kPrimaryButton) return;
    _downAt = e.localPosition;
    _dragging = false;
  }

  void _move(PointerMoveEvent e) {
    final start = _downAt;
    if (start == null) return;
    if (!_dragging && (e.localPosition - start).distance < kTouchSlop / 2) return;
    _dragging = true;
    widget.onOrbit(e.delta);
  }

  void _up(PointerUpEvent e) {
    final start = _downAt;
    _downAt = null;
    if (start == null || _dragging) {
      _dragging = false;
      return;
    }
    switch (_targetAt(e.localPosition)) {
      case _End(:final end):
        widget.onLookFrom(end.axis, end.positive);
      case _Centre():
        widget.onToggleProjection();
      case null:
        break;
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final camera = widget.camera;
    final view = camera.viewName;
    final projection = camera.orthographic ? 'Ortho' : 'Persp';
    // As wide as the longest caption: the circle stays put while the
    // caption changes under it.
    return SizedBox(
      width: OrientationGizmo.width,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Semantics(
            label: 'Orientation: click an axis to look from that side, drag to orbit',
            child: MouseRegion(
              cursor: _dragging ? SystemMouseCursors.grabbing : SystemMouseCursors.basic,
              onHover: (e) => _setHover(_targetAt(e.localPosition)),
              onExit: (_) => _setHover(null),
              child: Listener(
                behavior: HitTestBehavior.opaque,
                onPointerDown: _down,
                onPointerMove: _move,
                onPointerUp: _up,
                onPointerCancel: (_) => setState(() {
                  _downAt = null;
                  _dragging = false;
                }),
                child: ClipOval(
                  child: BackdropFilter(
                    filter: ImageFilter.blur(sigmaX: 12, sigmaY: 12),
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: t.overlay,
                        shape: BoxShape.circle,
                        border: Border.all(color: t.overlayEdge),
                      ),
                      child: CustomPaint(
                        key: const Key('scene.orient'),
                        size: const Size.square(OrientationGizmo.diameter),
                        painter: _GizmoPainter(
                          ends: OrientationGizmo.endsFor(camera),
                          hover: _hover,
                          text: t.text,
                          hoverFill: t.hover,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(height: 3),
          Tooltip(
            message: 'Perspective / orthographic (5)',
            child: MouseRegion(
              cursor: SystemMouseCursors.click,
              child: GestureDetector(
                key: const Key('scene.orient.projection'),
                behavior: HitTestBehavior.opaque,
                onTap: widget.onToggleProjection,
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                  decoration: BoxDecoration(
                    color: t.overlay,
                    borderRadius: BorderRadius.circular(4),
                    border: Border.all(color: t.overlayEdge),
                  ),
                  child: Text(
                    view == null ? projection : '$view · $projection',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontFamily: uiFontFamily, fontSize: 10.5, height: 1.3, color: t.textSecondary),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _GizmoPainter extends CustomPainter {
  _GizmoPainter({required this.ends, required this.hover, required this.text, required this.hoverFill});

  final List<GizmoAxisEnd> ends;
  final _Target? hover;
  final Color text;
  final Color hoverFill;

  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    if (hover is _Centre) {
      canvas.drawCircle(c, OrientationGizmo.centreRadius, Paint()..color = hoverFill);
    }
    for (final e in ends) {
      final colour = sceneAxisColors[e.axis];
      final at = c + e.offset;
      final lit = hover == _End(e);
      // Ends pointing away read dimmer, so the near half stands out.
      final fade = e.depth > 0.2 ? 0.75 : 1.0;
      if (e.positive) {
        canvas.drawLine(
          c,
          at,
          Paint()
            ..color = colour.withValues(alpha: fade)
            ..strokeWidth = 2
            ..strokeCap = StrokeCap.round,
        );
        canvas.drawCircle(at, OrientationGizmo.endRadius, Paint()..color = colour.withValues(alpha: fade));
      } else {
        canvas.drawCircle(
          at,
          OrientationGizmo.dotRadius,
          Paint()..color = colour.withValues(alpha: lit ? 0.9 : 0.35 * fade),
        );
        canvas.drawCircle(
          at,
          OrientationGizmo.dotRadius,
          Paint()
            ..color = colour.withValues(alpha: fade)
            ..style = PaintingStyle.stroke
            ..strokeWidth = 1.2,
        );
      }
      if (lit) {
        canvas.drawCircle(
          at,
          (e.positive ? OrientationGizmo.endRadius : OrientationGizmo.dotRadius) + 1.5,
          Paint()
            ..color = text
            ..style = PaintingStyle.stroke
            ..strokeWidth = 1.5,
        );
      }
      if (e.positive || lit) _label(canvas, at, e.label);
    }
    canvas.drawCircle(c, 2, Paint()..color = text.withValues(alpha: 0.7));
  }

  void _label(Canvas canvas, Offset at, String label) {
    final tp = TextPainter(
      text: TextSpan(
        text: label,
        style: TextStyle(
          fontFamily: uiFontFamily,
          fontSize: label.length > 1 ? 8 : 9.5,
          fontWeight: FontWeight.w700,
          height: 1,
          color: const Color(0xFFFFFFFF),
        ),
      ),
      textDirection: TextDirection.ltr,
    )..layout();
    tp.paint(canvas, at - Offset(tp.width / 2, tp.height / 2));
    tp.dispose();
  }

  @override
  bool shouldRepaint(_GizmoPainter old) {
    if (old.hover != hover || old.text != text || old.hoverFill != hoverFill || old.ends.length != ends.length) {
      return true;
    }
    for (var i = 0; i < ends.length; i++) {
      final a = old.ends[i], b = ends[i];
      if (a.axis != b.axis || a.positive != b.positive || a.offset != b.offset) return true;
    }
    return false;
  }
}
