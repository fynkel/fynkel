import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show Behaviour, Camera, GameObject, LayerMask, Ray, Vec3, World;
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
/// camera or off the view; nearer ones are drawn over farther ones. With
/// [hideBehind] it's hidden while something is in front of it too, and
/// with [keepOnScreen] it waits at the view's edge rather than leave. It
/// takes taps on itself from the game: a button in it works, and the game
/// doesn't see that tap. For a widget at a point rather than an object,
/// put it on an empty object there.
class WorldWidget extends Behaviour {
  /// [builder]'s widget, its [alignment] point at the object's position
  /// plus [offset]; shrunk beyond [shrinkBeyond], if given.
  WorldWidget(
    this.builder, {
    this.offset = Vec3.zero,
    this.alignment = Alignment.center,
    this.shrinkBeyond,
    this.hideBehind = false,
    this.hideLayers = LayerMask.all,
    this.keepOnScreen = false,
    this.edgeMargin = 8,
  }) : assert(
         shrinkBeyond == null || shrinkBeyond > 0,
         'WorldWidget.shrinkBeyond is $shrinkBeyond: give a distance above 0, or leave it out for full size always',
       ),
       assert(edgeMargin >= 0, 'WorldWidget.edgeMargin is $edgeMargin: give 0 or more logical pixels');

  /// Where the world widget whose builder [context] is in is, in its view:
  /// whether it's held at the edge ([keepOnScreen]) and which way its
  /// point is. Only from a [builder]'s context.
  static WorldWidgetPlace of(BuildContext context) {
    final scope = context.getInheritedWidgetOfExactType<_Scope>();
    if (scope == null) {
      throw FlutterError(
        'WorldWidget.of() was given a context that isn\'t in a WorldWidget\'s builder: call it from the builder, '
        'or from a widget the builder made.',
      );
    }
    return scope.place;
  }

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

  /// Hidden while something is between the camera and its point: a wall,
  /// a tree, anything [World.raycast] hits (the meshes as drawn), but its
  /// own object and what's under it. It fades out and in over 0.15 s.
  /// Each frame looks again for a share of these, so a hundred cost a few
  /// casts a frame, not a hundred.
  bool hideBehind;

  /// What [hideBehind] hides it behind: [LayerMask.all], or the layers
  /// given (`LayerMask.except([glass])` sees through glass).
  LayerMask hideLayers;

  /// Off the view or behind the camera, held inside the view's edge,
  /// [edgeMargin] in, on the line from the middle toward its point, rather
  /// than hidden: an arrow to where to go. With its point on the view, all
  /// of it is kept inside the edge too: a speech bubble over someone at
  /// the side of the screen doesn't hang off it. [WorldWidget.of] tells its
  /// builder when it's there and which way to point.
  bool keepOnScreen;

  /// How far in from the view's edge [keepOnScreen] holds it, in logical
  /// pixels.
  double edgeMargin;

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

/// Where a [WorldWidget] is in one view, for its builder:
/// `WorldWidget.of(context)`.
final class WorldWidgetPlace {
  WorldWidgetPlace._();

  /// True while it's held at the view's edge ([WorldWidget.keepOnScreen])
  /// because its point is off the view or behind the camera.
  ValueListenable<bool> get offScreen => _offScreen;
  final _offScreen = ValueNotifier(false);

  /// While [offScreen], which way its point is from the view's middle, in
  /// radians: 0 to the right, pi / 2 down, as `Transform.rotate` turns. An
  /// arrow drawn pointing right, turned by it, points there.
  ValueListenable<double> get angle => _angle;
  final _angle = ValueNotifier(0.0);

  // [WorldWidget.hideBehind]: whether something was in front at the last
  // look (null before the first), and how much of it is painted.
  bool? _behind;
  double _opacity = 1;

  void _dispose() {
    _offScreen.dispose();
    _angle.dispose();
  }
}

/// Gives a world widget's builder its place.
class _Scope extends InheritedWidget {
  const _Scope({super.key, required this.place, required super.child});

  final WorldWidgetPlace place;

  @override
  bool updateShouldNotify(_Scope old) => !identical(old.place, place);
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
  const WorldWidgetLayer({super.key, required this.world, required this.frames, this.camera, this.framesBehind});

  /// The world whose widgets it shows.
  final World world;

  /// Ticks after each update of [world].
  final Listenable frames;

  /// The view's own camera, or null for the world's main one.
  final Camera? camera;

  /// How many frames after Flutter's picture of an update the engine's
  /// picture of it reaches the screen: the widgets are placed as that many
  /// updates ago, so they're on what they mark. Null: none.
  final int Function()? framesBehind;

  @override
  State<WorldWidgetLayer> createState() => _WorldWidgetLayerState();
}

class _WorldWidgetLayerState extends State<WorldWidgetLayer> {
  late _Shown _of;
  // What's built, as it was at the last build: the placer reads the same.
  var _built = const <WorldWidget>[];
  var _builtPlaces = const <WorldWidgetPlace>[];
  final _places = <WorldWidget, WorldWidgetPlace>{};

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
    for (final place in _places.values) {
      place._dispose();
    }
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
    // Places of widgets gone go once their builders are gone.
    final gone = [
      for (final w in _places.keys)
        if (!_built.contains(w)) w,
    ];
    if (gone.isNotEmpty) {
      final places = [for (final w in gone) _places.remove(w)!];
      SchedulerBinding.instance.addPostFrameCallback((_) {
        for (final place in places) {
          place._dispose();
        }
      });
    }
    _builtPlaces = [for (final w in _built) _places[w] ??= WorldWidgetPlace._()];
    if (_built.isEmpty) return const SizedBox.shrink();
    return _Placed(
      world: widget.world,
      widgets: _built,
      places: _builtPlaces,
      frames: widget.frames,
      camera: widget.camera,
      framesBehind: widget.framesBehind,
      children: [
        // Each painted once, and only moved after.
        for (var i = 0; i < _built.length; i++)
          _Scope(
            key: ObjectKey(_built[i]),
            place: _builtPlaces[i],
            child: RepaintBoundary(child: Builder(builder: _built[i].builder)),
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
    required this.places,
    required this.frames,
    required this.camera,
    required this.framesBehind,
    required super.children,
  });

  final World world;
  final Camera? camera;
  // One for each child, in order.
  final List<WorldWidget> widgets;
  final List<WorldWidgetPlace> places;
  final Listenable frames;
  final int Function()? framesBehind;

  @override
  _RenderPlaced createRenderObject(BuildContext context) => _RenderPlaced(world, widgets, places, frames)
    ..camera = camera
    ..framesBehind = framesBehind;

  @override
  void updateRenderObject(BuildContext context, _RenderPlaced placed) {
    placed
      ..world = world
      ..camera = camera
      ..widgets = widgets
      ..places = places
      ..frames = frames
      ..framesBehind = framesBehind;
    placed.markNeedsPaint();
  }
}

/// Where a child was painted, last time.
class _Place extends ContainerBoxParentData<RenderBox> {
  bool shown = false;
  // Its widget's, in [_RenderPlaced.widgets] and `places`.
  int index = 0;
  double scale = 1;
  double depth = 0;
}

class _RenderPlaced extends RenderBox
    with ContainerRenderObjectMixin<RenderBox, _Place>, RenderBoxContainerDefaultsMixin<RenderBox, _Place> {
  _RenderPlaced(this.world, this.widgets, this.places, this._frames);

  World world;
  Camera? camera;
  int Function()? framesBehind;
  List<WorldWidget> widgets;
  // One for each widget.
  List<WorldWidgetPlace> places;
  Listenable _frames;
  set frames(Listenable value) {
    if (identical(value, _frames)) return;
    if (attached) _frames.removeListener(_frame);
    _frames = value;
    if (attached) _frames.addListener(_frame);
  }

  /// How many [WorldWidget.hideBehind] looks a frame takes, at most, beyond
  /// a widget's first: a look into 3,000 objects is about 35 µs on a Mac,
  /// so 8 are 0.3 ms, and a hundred widgets each look again in 13 frames.
  static const castsPerFrame = 8;

  /// How long [WorldWidget.hideBehind]'s fade takes.
  static const fadeSeconds = 0.15;

  // Where the next frame's looks start, among the widgets that look.
  var _castFrom = 0;
  // The widgets that look this frame; reused.
  final _looking = <int>[];
  Duration? _lastFrame;
  // Laid out since the last placing: paint places them, without looking.
  var _stale = true;
  // The last [framesBehind] placings, oldest at [_next]: for each child,
  // shown (1 or 0), x, y, scale and depth.
  final _ring = <Float64List>[];
  var _next = 0;
  static const _stride = 5;

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
    _frames.addListener(_frame);
  }

  @override
  void detach() {
    _frames.removeListener(_frame);
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
    _stale = true;
  }

  /// After each update of the world: places them, looks, fades, and tells
  /// the builders where they are, before this frame builds.
  void _frame() {
    if (!hasSize) return;
    final now = SchedulerBinding.instance.currentFrameTimeStamp;
    final last = _lastFrame;
    _lastFrame = now;
    final dt = last == null ? 0.0 : ((now - last).inMicroseconds / 1e6).clamp(0.0, 0.1);
    _place(dt: dt, look: true);
    markNeedsPaint();
  }

  /// Works out where each child goes, and the order to paint them in.
  /// Looking ([WorldWidget.hideBehind]), fading by [dt] seconds and
  /// telling the builders ([WorldWidgetPlace]) only when [look]: from the
  /// frame, not from paint.
  void _place({required double dt, required bool look}) {
    _stale = false;
    _order.clear();
    _looking.clear();
    for (var child = firstChild; child != null; child = childAfter(child)) {
      (child.parentData! as _Place).shown = false;
    }
    if (world.isDisposed) return;
    final own = this.camera;
    if (own != null && cameraObjectId(own, world) == -1) return;
    final camera = own ?? world.camera;
    if (camera == null || !camera.gameObject.activeInHierarchy) return;
    if (!(size.width > 0 && size.height > 0)) return;
    final turn = camera.gameObject.worldTransform.rotation;
    final eye = camera.gameObject.worldTransform.position;
    // The camera draws moved by its shake: moving each point back instead
    // keeps the widgets on what they mark.
    final shake = world.cameras.shakeOf(camera);
    // Camera.worldToScreenPoint's numbers, worked out once, so a hundred
    // widgets a frame make nothing each (stage 14's BR7): the point from
    // the eye, turned back by the camera's turn, over the view.
    final (ex, ey, ez) = (eye.x + shake.x, eye.y + shake.y, eye.z + shake.z);
    final (qx, qy, qz, qw) = (-turn.x, -turn.y, -turn.z, turn.w);
    final viewWidth = size.width, viewHeight = size.height, aspect = viewWidth / viewHeight;
    final orthographic = camera.orthographic, orthographicSize = camera.orthographicSize;
    final tanHalf = orthographic ? 0.0 : math.tan(camera.fovY / 2);
    var i = 0;
    for (var child = firstChild; child != null; child = childAfter(child), i++) {
      final place = child.parentData! as _Place..index = i;
      final w = i < widgets.length ? widgets[i] : null;
      final where = i < places.length ? places[i] : null;
      if (w == null || where == null || w.isRemoved) continue;
      // Its point, from the eye (both moved back by the shake).
      final own = w.gameObject.worldTransform.position, offset = w.offset;
      final px = own.x + offset.x - ex, py = own.y + offset.y - ey, pz = own.z + offset.z - ez;
      final distance = math.sqrt(px * px + py * py + pz * pz);
      // Turned into the camera's frame (Quat.rotate's numbers).
      final tx = (qy * pz - qz * py) * 2, ty = (qz * px - qx * pz) * 2, tz = (qx * py - qy * px) * 2;
      final cx = px + tx * qw + (qy * tz - qz * ty), cy = py + ty * qw + (qz * tx - qx * tz);
      final cz = pz + tz * qw + (qx * ty - qy * tx);
      var inFront = true;
      double across, up;
      if (orthographic) {
        across = cx / (orthographicSize * aspect);
        up = cy / orthographicSize;
      } else {
        final ahead = -cz;
        inFront = ahead > 0;
        across = inFront ? cx / (ahead * tanHalf * aspect) : 0;
        up = inFront ? cy / (ahead * tanHalf) : 0;
      }
      final atX = (across + 1) / 2 * viewWidth, atY = (1 - up) / 2 * viewHeight;
      final onView = inFront && atX >= 0 && atX <= viewWidth && atY >= 0 && atY <= viewHeight;
      if (w.keepOnScreen && !onView) {
        // Held at the edge, on the line from the middle toward the point:
        // which way it is from the camera, across and up, holds behind it
        // too.
        final to = Vec3(px, py, pz);
        var dx = to.dot(turn.rotate(Vec3.right)), dy = -to.dot(turn.rotate(Vec3.up));
        if (dx.abs() < 1e-9 && dy.abs() < 1e-9) dy = 1; // Straight behind: below.
        final width = child.size.width, height = child.size.height;
        final roomX = math.max(0.0, size.width / 2 - w.edgeMargin - width / 2);
        final roomY = math.max(0.0, size.height / 2 - w.edgeMargin - height / 2);
        final t = math.min(dx == 0 ? double.infinity : roomX / dx.abs(), dy == 0 ? double.infinity : roomY / dy.abs());
        place
          ..shown = true
          ..index = i
          ..offset = Offset(size.width / 2 + dx * t - width / 2, size.height / 2 + dy * t - height / 2)
          ..scale = 1
          ..depth = distance;
        // At the edge it shows whatever is in the way: an arrow is no use
        // hidden.
        where._opacity = 1;
        if (look) {
          where._offScreen.value = true;
          where._angle.value = math.atan2(dy, dx);
          // Back on the view, it looks again before it shows.
          where._behind = null;
        }
        _order.add(child);
        continue;
      }
      if (look && w.keepOnScreen) where._offScreen.value = false;
      if (!inFront) continue; // Behind the camera.
      final beyond = w.shrinkBeyond;
      // A release build doesn't check it: one that isn't above 0 keeps full size.
      final scale = beyond == null || !(beyond > 0) || distance <= beyond ? 1.0 : beyond / distance;
      final width = child.size.width * scale, height = child.size.height * scale;
      var x = atX - (w.alignment.x + 1) / 2 * width;
      var y = atY - (w.alignment.y + 1) / 2 * height;
      if (w.keepOnScreen) {
        // Its point on the view: all of it inside the edge too.
        x = x.clamp(w.edgeMargin, math.max(w.edgeMargin, size.width - w.edgeMargin - width));
        y = y.clamp(w.edgeMargin, math.max(w.edgeMargin, size.height - w.edgeMargin - height));
      }
      if (x > size.width || y > size.height || x + width < 0 || y + height < 0) continue;
      // Still, it keeps its offset: nothing made.
      if (place.offset.dx != x || place.offset.dy != y) place.offset = Offset(x, y);
      place
        ..shown = true
        ..index = i
        ..scale = scale
        ..depth = distance;
      if (!w.hideBehind) {
        where
          .._behind = null
          .._opacity = 1;
      } else if (look) {
        if (where._behind == null) {
          // Its first look is now, whatever the share, and it shows as it
          // is: no fade in from nothing.
          where._behind = _isBehind(w, eye, Vec3(eye.x + px, eye.y + py, eye.z + pz));
          where._opacity = where._behind! ? 0 : 1;
        } else {
          _looking.add(i);
        }
      }
      _order.add(child);
    }
    if (look) {
      _lookAndFade(eye, shake, dt);
      _delay(framesBehind?.call() ?? 0);
    }
    // Faded out: not painted, and not tapped.
    _order.removeWhere((child) {
      final place = child.parentData! as _Place;
      final hidden = _opacityOf(place) <= 0;
      if (hidden) place.shown = false;
      return hidden;
    });
    // Farthest first, so the nearest is over the rest.
    _order.sort(_farFirst);
  }

  /// Shows the placing of [behind] updates ago, and keeps this one for
  /// later: the engine's picture of this update isn't on screen yet.
  void _delay(int behind) {
    if (behind <= 0) {
      _ring.clear();
      return;
    }
    if (_ring.length != behind) {
      _ring.clear();
      _next = 0;
    }
    final count = childCount * _stride;
    if (_ring.length < behind) {
      // Until there are enough, each placing is shown as it is.
      _ring.add(_record(Float64List(count)));
      return;
    }
    var kept = _ring[_next];
    // Widgets came or went: what was kept is of other children.
    if (kept.length != count) {
      _ring[_next] = _record(Float64List(count));
      _next = (_next + 1) % behind;
      return;
    }
    var k = 0;
    for (var child = firstChild; child != null; child = childAfter(child), k += _stride) {
      final place = child.parentData! as _Place;
      final shown = kept[k] != 0, x = kept[k + 1], y = kept[k + 2], scale = kept[k + 3], depth = kept[k + 4];
      kept
        ..[k] = place.shown ? 1 : 0
        ..[k + 1] = place.offset.dx
        ..[k + 2] = place.offset.dy
        ..[k + 3] = place.scale
        ..[k + 4] = place.depth;
      place
        ..shown = shown
        ..offset = Offset(x, y)
        ..scale = scale
        ..depth = depth;
    }
    _next = (_next + 1) % behind;
    _order.clear();
    for (var child = firstChild; child != null; child = childAfter(child)) {
      if ((child.parentData! as _Place).shown) _order.add(child);
    }
  }

  Float64List _record(Float64List into) {
    var k = 0;
    for (var child = firstChild; child != null; child = childAfter(child), k += _stride) {
      final place = child.parentData! as _Place;
      into
        ..[k] = place.shown ? 1 : 0
        ..[k + 1] = place.offset.dx
        ..[k + 2] = place.offset.dy
        ..[k + 3] = place.scale
        ..[k + 4] = place.depth;
    }
    return into;
  }

  /// Looks for a share of the widgets that look, from where the last frame
  /// stopped, and fades each toward what it saw.
  void _lookAndFade(Vec3 eye, Vec3 shake, double dt) {
    final n = _looking.length;
    if (n > 0) {
      final from = _castFrom % n;
      final count = math.min(n, castsPerFrame);
      for (var k = 0; k < count; k++) {
        final i = _looking[(from + k) % n];
        places[i]._behind = _isBehind(widgets[i], eye, widgets[i].point - shake);
      }
      _castFrom = from + count;
    }
    final step = dt / fadeSeconds;
    for (final i in _looking) {
      final where = places[i];
      final to = where._behind! ? 0.0 : 1.0;
      where._opacity = where._opacity < to ? math.min(to, where._opacity + step) : math.max(to, where._opacity - step);
    }
  }

  // Not past the point: what's at it, or just behind, doesn't hide it.
  static const _margin = 0.05;

  /// Whether something on [w]'s layers is between [eye] and [point], other
  /// than [w]'s own object and what's under it.
  bool _isBehind(WorldWidget w, Vec3 eye, Vec3 point) {
    final to = point - eye;
    final distance = to.length;
    if (!(distance > _margin)) return false;
    final direction = to / distance;
    var from = eye;
    var left = distance - _margin;
    // Through its own object's faces, a few at most.
    for (var tries = 0; tries < 4 && left > 0; tries++) {
      final hit = world.raycast(Ray(from, direction), maxDistance: left, layers: w.hideLayers);
      if (hit == null) return false;
      if (!_under(hit.gameObject, w.gameObject)) return true;
      final step = hit.distance + 1e-3;
      from = from + direction * step;
      left -= step;
    }
    return false;
  }

  static bool _under(GameObject? object, GameObject root) {
    for (var o = object; o != null; o = o.parent) {
      if (identical(o, root)) return true;
    }
    return false;
  }

  /// How much of it is painted: less while [WorldWidget.hideBehind] fades it.
  double _opacityOf(_Place place) {
    final i = place.index;
    if (i >= widgets.length || i >= places.length || !widgets[i].hideBehind) return 1;
    return places[i]._opacity;
  }

  /// Whether it takes taps: not while hidden, or fading out.
  bool _tappable(_Place place) {
    final i = place.index;
    if (i >= widgets.length || i >= places.length || !widgets[i].hideBehind) return true;
    return places[i]._behind != true;
  }

  static int _farFirst(RenderBox a, RenderBox b) =>
      (b.parentData! as _Place).depth.compareTo((a.parentData! as _Place).depth);

  @override
  void paint(PaintingContext context, Offset offset) {
    if (_stale) _place(dt: 0, look: false);
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
      final opacity = _opacityOf(place);
      if (opacity < 1) {
        context.pushOpacity(offset, (opacity * 255).round(), (context, at) => _paintOne(context, at, child, place));
      } else {
        _paintOne(context, offset, child, place);
      }
    }
  }

  void _paintOne(PaintingContext context, Offset offset, RenderBox child, _Place place) {
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

  @override
  bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
    // Nearest first: the one painted over the rest takes the tap.
    for (var i = _order.length - 1; i >= 0; i--) {
      final child = _order[i];
      final place = child.parentData! as _Place;
      if (!place.shown || !_tappable(place)) continue;
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
