import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show Behaviour, Camera, Vec3, World;
import 'package:fynkel/platform.dart' show cameraObjectId;

/// A Flutter widget at its object's place in the world: a health bar over
/// an enemy, a name tag, a damage number, a "press E" hint. Every
/// `FynkelView` of the world shows it over the game, where its camera
/// sees the object, each frame:
///
/// ```dart
/// enemy.addComponent(WorldWidget((context) => HealthBar(enemy.health), offset: Vec3(0, 1.2, 0)));
/// ```
///
/// The widget is built once (and again when it rebuilds itself, from a
/// `ValueListenableBuilder` say) and only moved after that, so a hundred
/// cost little. It's hidden while its object is inactive, behind the
/// camera or off the view; nearer ones are drawn over farther ones. It
/// takes taps on itself from the game: a button in it works, and the game
/// doesn't see that tap. For a widget at a point rather than an object,
/// put it on an empty object there.
class WorldWidget extends Behaviour {
  /// [builder]'s widget, its [alignment] point at the object's position
  /// plus [offset]; shrunk beyond [shrinkBeyond], if given.
  WorldWidget(this.builder, {this.offset = Vec3.zero, this.alignment = Alignment.center, this.shrinkBeyond})
    : assert(shrinkBeyond == null || shrinkBeyond > 0);

  /// Builds the widget, once it's shown.
  final WidgetBuilder builder;

  /// Added to the object's position, in the world's directions (not
  /// turned with the object): `Vec3(0, 1.2, 0)` is above it.
  Vec3 offset;

  /// Which point of the widget is on the world point: [Alignment.center]
  /// is its middle, [Alignment.bottomCenter] puts it above the point.
  final Alignment alignment;

  /// Full size up to this distance from the camera, smaller beyond in
  /// proportion (half as big twice as far), as things in the world look.
  /// Null: always full size.
  final double? shrinkBeyond;

  /// The world point it's at.
  Vec3 get point => gameObject.worldTransform.position + offset;

  @override
  void onEnable() {
    super.onEnable();
    _shownIn(world).add(this);
  }

  @override
  void onDisable() {
    _shownIn(world).remove(this);
    super.onDisable();
  }
}

/// A world's world widgets that are on, in the order they came.
class _Shown extends ChangeNotifier {
  final widgets = <WorldWidget>[];

  void add(WorldWidget widget) {
    widgets.add(widget);
    notifyListeners();
  }

  void remove(WorldWidget widget) {
    if (widgets.remove(widget)) notifyListeners();
  }
}

final _shown = Expando<_Shown>('WorldWidget');

_Shown _shownIn(World world) => _shown[world] ??= _Shown();

/// The world widgets that are on in [world], for tests and tools.
List<WorldWidget> worldWidgetsOf(World world) => List.unmodifiable(_shownIn(world).widgets);

/// Ticks once a frame, after the world's update: what moves the widgets.
class FrameTicks extends ChangeNotifier {
  /// Tells the listeners a frame was updated.
  void tick() => notifyListeners();
}

/// Shows [world]'s world widgets over a view of it, placed by its camera
/// at each of [frames]. A view puts one over its game.
class WorldWidgetLayer extends StatefulWidget {
  /// The layer of [world]'s widgets, moved at each of [frames], placed by
  /// [camera] (the world's main one when null).
  const WorldWidgetLayer({super.key, required this.world, required this.frames, this.camera});

  /// The world whose widgets it shows.
  final World world;

  /// Ticks after each update of [world].
  final Listenable frames;

  /// The view's own camera, or null for the world's main one.
  final Camera? camera;

  @override
  State<WorldWidgetLayer> createState() => _WorldWidgetLayerState();
}

class _WorldWidgetLayerState extends State<WorldWidgetLayer> {
  late _Shown _of;
  // What's built, as it was at the last build: the placer reads the same.
  var _built = const <WorldWidget>[];

  @override
  void initState() {
    super.initState();
    _of = _shownIn(widget.world)..addListener(_changed);
  }

  @override
  void didUpdateWidget(WorldWidgetLayer oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.world, widget.world)) {
      _of.removeListener(_changed);
      _of = _shownIn(widget.world)..addListener(_changed);
    }
  }

  @override
  void dispose() {
    _of.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (!mounted) return;
    // From the game's update (a transient callback) it builds this frame;
    // from a build, the next.
    if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
      SchedulerBinding.instance.addPostFrameCallback((_) {
        if (mounted) setState(() {});
      });
    } else {
      setState(() {});
    }
  }

  @override
  Widget build(BuildContext context) {
    _built = List.of(_of.widgets);
    if (_built.isEmpty) return const SizedBox.shrink();
    return _Placed(
      world: widget.world,
      widgets: _built,
      frames: widget.frames,
      camera: widget.camera,
      children: [
        // Each painted once, and only moved after.
        for (final w in _built)
          RepaintBoundary(
            key: ObjectKey(w),
            child: Builder(builder: w.builder),
          ),
      ],
    );
  }
}

/// Lays out each of [world]'s shown widgets (the children, in order) at
/// its size, and paints it where the camera sees its point, nearest last.
/// Not a Flow: that makes a transform layer for each child each frame.
class _Placed extends MultiChildRenderObjectWidget {
  const _Placed({
    required this.world,
    required this.widgets,
    required this.frames,
    required this.camera,
    required super.children,
  });

  final World world;
  final Camera? camera;
  // One for each child, in order.
  final List<WorldWidget> widgets;
  final Listenable frames;

  @override
  _RenderPlaced createRenderObject(BuildContext context) => _RenderPlaced(world, widgets, frames)..camera = camera;

  @override
  void updateRenderObject(BuildContext context, _RenderPlaced placed) {
    placed
      ..world = world
      ..camera = camera
      ..widgets = widgets
      ..frames = frames;
    placed.markNeedsPaint();
  }
}

/// Where a child was painted, last time.
class _Place extends ContainerBoxParentData<RenderBox> {
  bool shown = false;
  double scale = 1;
  double depth = 0;
}

class _RenderPlaced extends RenderBox
    with ContainerRenderObjectMixin<RenderBox, _Place>, RenderBoxContainerDefaultsMixin<RenderBox, _Place> {
  _RenderPlaced(this.world, this.widgets, this._frames);

  World world;
  Camera? camera;
  List<WorldWidget> widgets;
  Listenable _frames;
  set frames(Listenable value) {
    if (identical(value, _frames)) return;
    if (attached) _frames.removeListener(markNeedsPaint);
    _frames = value;
    if (attached) _frames.addListener(markNeedsPaint);
  }

  // The children shown, farthest first; reused.
  final _order = <RenderBox>[];
  final _clip = LayerHandle<ClipRectLayer>();

  @override
  void setupParentData(RenderBox child) {
    if (child.parentData is! _Place) child.parentData = _Place();
  }

  @override
  void attach(PipelineOwner owner) {
    super.attach(owner);
    _frames.addListener(markNeedsPaint);
  }

  @override
  void detach() {
    _frames.removeListener(markNeedsPaint);
    super.detach();
  }

  @override
  void dispose() {
    _clip.layer = null;
    super.dispose();
  }

  @override
  bool get sizedByParent => true;

  @override
  Size computeDryLayout(BoxConstraints constraints) => constraints.biggest;

  @override
  void performLayout() {
    // Off the view it's laid out still, so it's ready when it comes.
    final loose = constraints.loosen();
    for (var child = firstChild; child != null; child = childAfter(child)) {
      child.layout(loose, parentUsesSize: true);
    }
  }

  /// Works out where each child goes, and the order to paint them in.
  void _place() {
    _order.clear();
    for (var child = firstChild; child != null; child = childAfter(child)) {
      (child.parentData! as _Place).shown = false;
    }
    if (world.isDisposed) return;
    final own = this.camera;
    if (own != null && cameraObjectId(own, world) == -1) return;
    final camera = own ?? world.camera;
    if (camera == null || !camera.gameObject.activeInHierarchy) return;
    if (!(size.width > 0 && size.height > 0)) return;
    final eye = camera.gameObject.worldTransform.position;
    // The camera draws moved by its shake: moving each point back instead
    // keeps the widgets on what they mark.
    final shake = world.cameras.shakeOf(camera);
    var i = 0;
    for (var child = firstChild; child != null; child = childAfter(child), i++) {
      final place = child.parentData! as _Place;
      final w = i < widgets.length ? widgets[i] : null;
      if (w == null || w.isRemoved) continue;
      final point = w.point - shake;
      final at = camera.worldToScreenPoint(point, width: size.width, height: size.height);
      if (at == null) continue; // Behind the camera.
      final distance = (point - eye).length;
      final beyond = w.shrinkBeyond;
      final scale = beyond == null || distance <= beyond ? 1.0 : beyond / distance;
      final width = child.size.width * scale, height = child.size.height * scale;
      final x = at.x - (w.alignment.x + 1) / 2 * width;
      final y = at.y - (w.alignment.y + 1) / 2 * height;
      if (x > size.width || y > size.height || x + width < 0 || y + height < 0) continue;
      place
        ..shown = true
        ..offset = Offset(x, y)
        ..scale = scale
        ..depth = distance;
      _order.add(child);
    }
    // Farthest first, so the nearest is over the rest.
    _order.sort(_farFirst);
  }

  static int _farFirst(RenderBox a, RenderBox b) =>
      (b.parentData! as _Place).depth.compareTo((a.parentData! as _Place).depth);

  @override
  void paint(PaintingContext context, Offset offset) {
    _place();
    if (_order.isEmpty) {
      _clip.layer = null;
      return;
    }
    _clip.layer = context.pushClipRect(
      needsCompositing,
      offset,
      Offset.zero & size,
      _paintShown,
      oldLayer: _clip.layer,
    );
  }

  void _paintShown(PaintingContext context, Offset offset) {
    for (final child in _order) {
      final place = child.parentData! as _Place;
      final s = place.scale;
      if (s == 1) {
        context.paintChild(child, offset + place.offset);
      } else {
        context.pushTransform(
          needsCompositing,
          offset + place.offset,
          Matrix4.diagonal3Values(s, s, 1),
          (context, at) => context.paintChild(child, at),
        );
      }
    }
  }

  @override
  bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
    // Nearest first: the one painted over the rest takes the tap.
    for (var i = _order.length - 1; i >= 0; i--) {
      final child = _order[i];
      final place = child.parentData! as _Place;
      if (!place.shown) continue;
      final hit = result.addWithPaintTransform(
        transform: _transformOf(place),
        position: position,
        hitTest: (result, at) => child.hitTest(result, position: at),
      );
      if (hit) return true;
    }
    return false;
  }

  @override
  void applyPaintTransform(RenderBox child, Matrix4 transform) {
    transform.multiply(_transformOf(child.parentData! as _Place));
  }

  static Matrix4 _transformOf(_Place place) =>
      Matrix4.diagonal3Values(place.scale, place.scale, 1)..setTranslationRaw(place.offset.dx, place.offset.dy, 0);
}
