// Projected world labels (M3-13, blueprint §10, docs/hud.md): screen-space
// widgets pinned to points in the 3D scene. `ProjectionLayer` owns the
// anchors of its children in the world's bounded `ProjectionView`, gives
// the view its viewport from layout, and moves the children with a `Flow`
// that repaints (transforms only) when the view updates — no widget in the
// tree rebuilds per frame. A child whose anchor is not `visible` is not
// painted and not hit-testable.

import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart';

/// A widget pinned to a point of the 3D scene inside a [ProjectionLayer].
///
/// [target] is a `GameObject` or an entity handle and [offset] a point in
/// its local frame; the layer registers the anchor in `World.projection`
/// while this child is in its list and removes it after. [alignment] says
/// which point of the child sits on the projected point
/// ([Alignment.bottomCenter]: the child floats above it), [nudge] shifts
/// it by logical pixels. With [occlusionMask] set, a collider on those
/// layers between the camera and the point hides the child (the explicit
/// policy of blueprint §10; 0 never hides). [clampToEdges] keeps a child
/// whose point is in front of the camera but outside the viewport on the
/// nearest edge (a marker) instead of hiding it.
class ProjectedAnchor extends StatelessWidget {
  const ProjectedAnchor({
    super.key,
    required this.target,
    required this.child,
    this.offset = Vec3.zero,
    this.occlusionMask = 0,
    this.alignment = Alignment.bottomCenter,
    this.nudge = Offset.zero,
    this.clampToEdges = false,
  });

  final Object target;
  final Vec3 offset;
  final int occlusionMask;
  final Alignment alignment;
  final Offset nudge;
  final bool clampToEdges;
  final Widget child;

  /// The same target: the layer keeps the `ScreenAnchor` it already has
  /// (offset and mask are updated in place).
  bool sameTarget(ProjectedAnchor other) => identical(target, other.target) || target == other.target;

  @override
  Widget build(BuildContext context) => child;
}

/// A text label pinned to a point of the 3D scene: a [ProjectedAnchor]
/// with a small pill of [text].
class WorldLabel extends ProjectedAnchor {
  WorldLabel({
    super.key,
    required super.target,
    required String text,
    super.offset,
    super.occlusionMask,
    super.alignment,
    super.nudge,
    super.clampToEdges,
    TextStyle? style,
    Color? background,
    EdgeInsetsGeometry padding = const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
    BorderRadiusGeometry borderRadius = const BorderRadius.all(Radius.circular(6)),
  }) : super(
         child: DecoratedBox(
           decoration: BoxDecoration(color: background ?? const Color(0xB3000000), borderRadius: borderRadius),
           child: Padding(
             padding: padding,
             child: Text(
               text,
               style: style ?? const TextStyle(color: Color(0xFFFFFFFF), fontSize: 13, decoration: TextDecoration.none),
               softWrap: false,
             ),
           ),
         ),
       );
}

/// Places its [ProjectedAnchor] children where their points land in the
/// viewport of [world]'s projection view. Size it like the viewport it
/// overlays (an `overlay` of `FynkelGameView` fills it): the layer reports
/// its own size to the view in logical pixels and the answers come back
/// as widget positions.
///
/// The view is updated once per frame by `FynkelGameView` after
/// `World.frame`; a host with its own loop calls
/// `world.projection.update()` itself. The layer repaints on every update
/// and rebuilds only when its children change.
class ProjectionLayer extends StatefulWidget {
  const ProjectionLayer({super.key, required this.world, this.children = const []});

  final World world;
  final List<ProjectedAnchor> children;

  @override
  State<ProjectionLayer> createState() => _ProjectionLayerState();
}

class _ProjectionLayerState extends State<ProjectionLayer> {
  final List<ScreenAnchor> _anchors = [];
  late final _ProjectionRepaint _repaint = _ProjectionRepaint(widget.world.projection);

  @override
  void initState() {
    super.initState();
    _sync(const []);
  }

  @override
  void didUpdateWidget(ProjectionLayer old) {
    super.didUpdateWidget(old);
    if (!identical(old.world, widget.world)) {
      for (final a in _anchors) {
        a.remove();
      }
      _anchors.clear();
      _repaint.rebind(widget.world.projection);
      _sync(const []);
    } else {
      _sync(old.children);
    }
  }

  /// Keeps one `ScreenAnchor` per child, in order: unchanged children keep
  /// theirs, changed ones are re-registered.
  void _sync(List<ProjectedAnchor> old) {
    final view = widget.world.projection;
    final next = <ScreenAnchor>[];
    for (var i = 0; i < widget.children.length; i++) {
      final c = widget.children[i];
      ScreenAnchor? keep;
      if (i < old.length && i < _anchors.length && old[i].sameTarget(c) && _anchors[i].isAttached) {
        keep = _anchors[i];
        keep
          ..offset = c.offset
          ..occlusionMask = c.occlusionMask;
      }
      next.add(keep ?? view.add(c.target, offset: c.offset, occlusionMask: c.occlusionMask));
    }
    for (final a in _anchors) {
      if (!next.contains(a)) a.remove();
    }
    _anchors
      ..clear()
      ..addAll(next);
  }

  @override
  void dispose() {
    for (final a in _anchors) {
      a.remove();
    }
    _repaint.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final size = constraints.biggest;
        widget.world.projection.setViewport(size.width, size.height);
        return Flow(
          delegate: _AnchorFlowDelegate(widget.children, _anchors, size, repaint: _repaint),
          clipBehavior: Clip.hardEdge,
          children: widget.children,
        );
      },
    );
  }
}

/// A `Listenable` over a projection view's updates.
class _ProjectionRepaint extends ChangeNotifier {
  _ProjectionRepaint(this._view) {
    _view.addListener(notifyListeners);
  }

  ProjectionView _view;

  void rebind(ProjectionView view) {
    _view.removeListener(notifyListeners);
    _view = view;
    _view.addListener(notifyListeners);
  }

  @override
  void dispose() {
    _view.removeListener(notifyListeners);
    super.dispose();
  }
}

class _AnchorFlowDelegate extends FlowDelegate {
  _AnchorFlowDelegate(this.children, this.anchors, this.size, {required Listenable repaint}) : super(repaint: repaint);

  final List<ProjectedAnchor> children;
  final List<ScreenAnchor> anchors;
  final Size size;

  @override
  BoxConstraints getConstraintsForChild(int i, BoxConstraints constraints) => BoxConstraints.loose(constraints.biggest);

  @override
  void paintChildren(FlowPaintingContext context) {
    for (var i = 0; i < context.childCount && i < anchors.length; i++) {
      final a = anchors[i];
      final c = children[i];
      final placed = a.visible || (c.clampToEdges && a.inFront && !a.occluded && !a.dropped);
      if (!placed) continue;
      final child = context.getChildSize(i) ?? Size.zero;
      var left = a.x - child.width * (c.alignment.x + 1) / 2 + c.nudge.dx;
      var top = a.y - child.height * (c.alignment.y + 1) / 2 + c.nudge.dy;
      if (c.clampToEdges) {
        left = left.clamp(0.0, (size.width - child.width).clamp(0.0, double.infinity));
        top = top.clamp(0.0, (size.height - child.height).clamp(0.0, double.infinity));
      }
      context.paintChild(i, transform: Matrix4.translationValues(left, top, 0));
    }
  }

  @override
  bool shouldRepaint(_AnchorFlowDelegate old) =>
      !identical(old.children, children) || !identical(old.anchors, anchors) || old.size != size;

  @override
  bool shouldRelayout(_AnchorFlowDelegate old) => old.size != size;
}
