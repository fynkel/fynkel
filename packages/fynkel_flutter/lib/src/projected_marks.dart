// Many small marks over objects (ADR-0062 amendment 1, step 2): health
// bars and dots for a crowd, drawn by one painter at their objects'
// projected points. A `ProjectionLayer` child is a widget (text, buttons,
// anything) and costs a widget; a mark costs an anchor and two rectangles,
// so hundreds fit in a frame. Values, colours and positions repaint; they
// never rebuild.

import 'package:flutter/widgets.dart';
import 'package:fynkel/gameplay.dart';

/// How the marks of a [WorldMarks] look: all the same shape and size, each
/// with its own value and (optionally) colour.
class MarkStyle {
  /// A bar [size] logical pixels: [background] across, [color] filled to
  /// the mark's value (0 to 1) from the left. [cornerRadius] defaults to a
  /// quarter of the height.
  const MarkStyle.bar({
    required Size this.size,
    required this.color,
    this.background,
    this.cornerRadius,
    this.alignment = Alignment.bottomCenter,
    this.nudge = Offset.zero,
    this.maxDistance,
  }) : radius = null;

  /// A dot of [radius] logical pixels centred on the point (the value is
  /// not drawn).
  const MarkStyle.dot({required double this.radius, required this.color, this.nudge = Offset.zero, this.maxDistance})
    : size = null,
      background = null,
      cornerRadius = null,
      alignment = Alignment.center;

  final Size? size;
  final double? radius;
  final Color color;
  final Color? background;
  final double? cornerRadius;

  /// Which point of a bar sits on the projected point
  /// ([Alignment.bottomCenter]: the bar floats above it).
  final Alignment alignment;

  /// Shifts every mark by logical pixels.
  final Offset nudge;

  /// Marks farther than this from the camera (along its view, in metres)
  /// are not drawn; null draws them at any distance.
  final double? maxDistance;

  bool get isBar => size != null;
}

/// One mark of a [WorldMarks].
class WorldMark {
  WorldMark._(this._owner, this.anchor, double value, Color? color) : _value = value, _color = color;

  final WorldMarks _owner;

  /// Where the mark's point is (its `ScreenAnchor` in `World.projection`).
  final ScreenAnchor anchor;

  /// The object (or entity handle) the mark is over.
  Object get target => anchor.target;

  double _value;
  Color? _color;
  bool _hidden = false;

  /// How full the bar is, 0 to 1.
  double get value => _value;
  set value(double v) {
    final c = v.clamp(0.0, 1.0);
    if (c == _value) return;
    _value = c;
    _owner._changed();
  }

  /// This mark's colour instead of the style's (null: the style's).
  Color? get color => _color;
  set color(Color? c) {
    if (c == _color) return;
    _color = c;
    _owner._changed();
  }

  /// Not drawn, while keeping its anchor.
  bool get hidden => _hidden;
  set hidden(bool h) {
    if (h == _hidden) return;
    _hidden = h;
    _owner._changed();
  }

  /// Takes the mark (and its anchor) away.
  void remove() => _owner.remove(this);
}

/// A set of marks over objects of [world], drawn by a [ProjectedMarks].
///
/// Each [add] registers one anchor in `world.projection` (bounded to
/// `ProjectionView.maxAnchors` in all, shared with `ProjectionLayer`s);
/// [dispose] removes them. Marks over destroyed objects are not drawn;
/// [prune] takes them away.
class WorldMarks extends ChangeNotifier {
  WorldMarks(this.world, {required this.style, this.offset = Vec3.zero, this.occlusionMask = 0}) {
    world.projection.addListener(notifyListeners);
  }

  final World world;
  final MarkStyle style;

  /// The point over each object, in its local frame (a mark's own offset
  /// replaces it).
  final Vec3 offset;

  /// With bits set, a collider on those layers between the camera and the
  /// point hides the mark (0 never hides).
  final int occlusionMask;

  final List<WorldMark> _marks = [];
  bool _disposed = false;

  List<WorldMark> get marks => List.unmodifiable(_marks);
  int get length => _marks.length;

  /// A mark over [target] (a `GameObject` or an entity handle).
  WorldMark add(Object target, {double value = 1, Color? color, Vec3? offset}) {
    final a = world.projection.add(target, offset: offset ?? this.offset, occlusionMask: occlusionMask);
    final m = WorldMark._(this, a, value.clamp(0.0, 1.0), color);
    _marks.add(m);
    _changed();
    return m;
  }

  void remove(WorldMark mark) {
    if (!_marks.remove(mark)) return;
    mark.anchor.remove();
    _changed();
  }

  /// Takes away the marks whose objects are gone.
  void prune() {
    final gone = _marks.where((m) => m.anchor.missing || !m.anchor.isAttached).toList();
    for (final m in gone) {
      _marks.remove(m);
      m.anchor.remove();
    }
    if (gone.isNotEmpty) _changed();
  }

  void clear() {
    for (final m in _marks) {
      m.anchor.remove();
    }
    _marks.clear();
    _changed();
  }

  void _changed() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    world.projection.removeListener(notifyListeners);
    for (final m in _marks) {
      m.anchor.remove();
    }
    _marks.clear();
    super.dispose();
  }
}

/// Draws [marks] where their points land in the viewport of their world's
/// projection view, in one paint. Size it like the viewport it overlays
/// (in a `FynkelGameView` overlay, a `Positioned.fill`); it takes no taps.
class ProjectedMarks extends StatelessWidget {
  const ProjectedMarks({super.key, required this.marks});

  final WorldMarks marks;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final size = constraints.biggest;
        marks.world.projection.setViewport(size.width, size.height);
        return CustomPaint(size: size, painter: _MarksPainter(marks));
      },
    );
  }
}

class _MarksPainter extends CustomPainter {
  _MarksPainter(this.marks) : super(repaint: marks);

  final WorldMarks marks;
  final Paint _paint = Paint();

  @override
  void paint(Canvas canvas, Size size) {
    final style = marks.style;
    final max = style.maxDistance;
    final bar = style.size;
    final radius = Radius.circular(style.cornerRadius ?? (bar == null ? 0 : bar.height / 4));
    canvas.save();
    canvas.clipRect(Offset.zero & size);
    for (final m in marks._marks) {
      final a = m.anchor;
      if (m._hidden || !a.isAttached || !a.visible) continue;
      if (max != null && a.depth > max) continue;
      final color = m._color ?? style.color;
      if (bar != null) {
        final left = a.x - bar.width * (style.alignment.x + 1) / 2 + style.nudge.dx;
        final top = a.y - bar.height * (style.alignment.y + 1) / 2 + style.nudge.dy;
        if (style.background case final bg?) {
          _paint.color = bg;
          canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromLTWH(left, top, bar.width, bar.height), radius), _paint);
        }
        if (m._value > 0) {
          _paint.color = color;
          canvas.drawRRect(
            RRect.fromRectAndRadius(Rect.fromLTWH(left, top, bar.width * m._value, bar.height), radius),
            _paint,
          );
        }
      } else {
        _paint.color = color;
        canvas.drawCircle(Offset(a.x + style.nudge.dx, a.y + style.nudge.dy), style.radius!, _paint);
      }
    }
    canvas.restore();
  }

  @override
  bool? hitTest(Offset position) => false;

  @override
  bool shouldRepaint(_MarksPainter old) => !identical(old.marks, marks);
}
