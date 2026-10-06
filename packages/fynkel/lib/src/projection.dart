// Projected world anchors (M3-13, blueprint §10, docs/hud.md): a bounded
// per-frame view mapping points attached to entities into a viewport's
// pixels — what a screen-space label is placed at. One FFI call per frame
// for every anchor (`fynkel_world_project`), answered from the frame's
// camera and interpolated poses with the renderer's own matrices.

part of 'world.dart';

/// Screen-space answers for world anchors, one [ScreenAnchor] per label.
///
/// Register anchors with [add], give the view the viewport it projects
/// into ([viewport]), and call [update] once per frame after `World.frame`
/// (`FynkelGameView` does; a headless test calls it by hand) — every
/// anchor's [ScreenAnchor.x]/[ScreenAnchor.y]/[ScreenAnchor.visible] then
/// hold that frame's answer and the listeners run once. At most
/// [maxAnchors] anchors project; beyond that the newest are
/// [ScreenAnchor.dropped] and `FYN-R0012` (`tooManyAnchors`) is reported
/// through `World.onDiagnostics` once per overflow.
class ProjectionView {
  ProjectionView._(this._world);

  /// The bound (blueprint §10): the engine projects at most this many
  /// anchors per request.
  static const int maxAnchors = 64;

  final World _world;
  final List<ScreenAnchor> _anchors = [];
  final RecordBatch _batch = RecordBatch(
    BufferKind.projectionAnchors,
    gen.ProjectionAnchorRecord.wireSize,
    64 * 32,
  );
  final _AnchorRecord _record = _AnchorRecord();
  final List<void Function()> _listeners = [];
  double _width = 0;
  double _height = 0;
  EntityRef? _camera;
  int _version = 0;
  bool _overflowReported = false;

  /// The registered anchors, registration order.
  List<ScreenAnchor> get anchors => List.unmodifiable(_anchors);
  int get length => _anchors.length;
  bool get isEmpty => _anchors.isEmpty;

  /// The viewport the anchors project into, in the caller's pixels
  /// (logical pixels for a Flutter overlay: the answers are then widget
  /// positions). The aspect ratio is what matters to the projection; the
  /// scale is the caller's.
  double get width => _width;
  double get height => _height;
  void setViewport(double width, double height) {
    _width = width;
    _height = height;
  }

  /// The camera entity the last [update] projected from (null: the world
  /// has no enabled camera and nothing is placed).
  EntityRef? get camera => _camera;

  /// Bumped by every [update]; a cheap "did anything change" for a painter.
  int get version => _version;

  /// Registers an anchor: [target] is a [GameObject] (pending ones are
  /// fine — they answer `missing` until spawned) or an entity handle,
  /// [offset] a point in the entity's local frame (a label above the head:
  /// `Vec3(0, 2, 0)`), [occlusionMask] the collider layers that hide the
  /// anchor when one sits between the camera and the point (0: never
  /// hidden by geometry — the explicit policy of blueprint §10).
  ScreenAnchor add(Object target, {Vec3 offset = Vec3.zero, int occlusionMask = 0}) {
    final a = ScreenAnchor._(this, target, offset, occlusionMask);
    _anchors.add(a);
    return a;
  }

  /// Unregisters an anchor (a no-op when already removed).
  void remove(ScreenAnchor anchor) {
    if (_anchors.remove(anchor)) anchor._attached = false;
  }

  /// Removes every anchor whose object is dead ([ScreenAnchor.missing]
  /// after the last update).
  void prune() {
    _anchors.removeWhere((a) {
      if (!a.missing) return false;
      a._attached = false;
      return true;
    });
  }

  void clear() {
    for (final a in _anchors) {
      a._attached = false;
    }
    _anchors.clear();
  }

  /// Listeners run after an [update] that moved, re-flagged or dropped an
  /// anchor (a Flutter layer repaints); an update that changed nothing —
  /// a still camera over still targets — runs none.
  void addListener(void Function() listener) => _listeners.add(listener);
  void removeListener(void Function() listener) => _listeners.remove(listener);

  /// Where the world [point] lands in the viewport, now — a one-off beside
  /// the per-frame anchors (an effect at a hit point, a marker for one
  /// frame): one FFI call, nothing registered. [width]/[height] default
  /// to the view's viewport ([setViewport]); [occlusionMask] as for
  /// [add]. Null without an enabled camera.
  ScreenPoint? worldToScreen(Vec3 point, {double? width, double? height, int occlusionMask = 0}) {
    final (w, h) = _size(width, height);
    final batch = _single ??= RecordBatch(BufferKind.projectionAnchors, gen.ProjectionAnchorRecord.wireSize, 64);
    batch.clear();
    batch.add(
      _record
        ..entity = EntityRef.nil
        ..offset = point
        ..mask = occlusionMask,
    );
    final length = batch.finalize();
    final (b, camera, _, _) = _world._native.project(batch.backingStore, length, w, h);
    batch.clear();
    if (camera.isNull || b.recordCount < 1) return null;
    final d = b.records;
    final flags = d.getUint32(28, Endian.little);
    return ScreenPoint(
      x: d.getFloat32(16, Endian.little),
      y: d.getFloat32(20, Endian.little),
      depth: d.getFloat32(24, Endian.little),
      inFront: flags & ScreenAnchor._inFront != 0,
      onScreen: flags & ScreenAnchor._onScreen != 0,
      occluded: flags & ScreenAnchor._occluded != 0,
    );
  }

  RecordBatch? _single;

  /// The ray through the viewport point ([x], [y]) — pixels from the top
  /// left, the units of [setViewport] — from the frame's camera and pose:
  /// Unity's `Camera.ScreenPointToRay`. Use it to aim, or hand it to
  /// `World.raycast` ([pick] does). [width]/[height] default to the view's
  /// viewport. Null without an enabled camera.
  ScreenRay? screenRay(double x, double y, {double? width, double? height}) {
    final (w, h) = _size(width, height);
    // The native call takes whole pixels for the viewport; scale the point
    // with it so a fractional logical size keeps its aspect.
    final sx = width ?? _width, sy = height ?? _height;
    final (camera, origin, direction) = _world._native.screenRay(w, h, x * w / sx, y * h / sy);
    if (camera.isNull) return null;
    return ScreenRay(camera: camera, origin: origin, direction: direction);
  }

  /// The nearest collider under the viewport point ([x], [y]): [screenRay]
  /// into `World.raycast` (a tap on an object, the cursor over the ground).
  /// The options are the raycast's; null without a camera or a hit.
  PhysicsHit? pick(
    double x,
    double y, {
    double maxDistance = 1000,
    int mask = 0xffffffff,
    bool hitSensors = true,
    double? width,
    double? height,
  }) {
    final ray = screenRay(x, y, width: width, height: height);
    if (ray == null) return null;
    return _world.raycast(ray.origin, ray.direction, maxDistance: maxDistance, mask: mask, hitSensors: hitSensors);
  }

  (int, int) _size(double? width, double? height) {
    final w = width ?? _width, h = height ?? _height;
    if (w <= 0 || h <= 0) {
      throw StateError('no viewport: call setViewport (a ProjectionLayer does) or pass width and height');
    }
    return (w.round().clamp(1, 1 << 20), h.round().clamp(1, 1 << 20));
  }

  /// Projects every anchor into the viewport: one FFI call. Call after
  /// `World.frame` (the same interpolation the frame renders with).
  /// Returns the number of anchors projected. With no anchors nothing is
  /// called and the listeners do not run.
  int update() {
    if (_anchors.isEmpty) return 0;
    final n = _anchors.length;
    final take = n < maxAnchors ? n : maxAnchors;
    _batch.clear();
    var dropped = false;
    for (var i = 0; i < take; i++) {
      final a = _anchors[i];
      if (a._dropped || a._fresh) dropped = true;
      a._dropped = false;
      a._fresh = false;
      final entity = a._entityOrZero();
      // A null entity asks for a world point (ADR-0062); a pending or dead
      // target is answered here instead (missing), in the same row order.
      a._gone = entity.isNull;
      _batch.add(
        _record
          ..entity = entity
          ..offset = a.offset
          ..mask = a.occlusionMask,
      );
    }
    for (var i = take; i < n; i++) {
      final a = _anchors[i];
      if (!a._dropped || a._flags != 0) dropped = true;
      a
        .._dropped = true
        .._flags = 0;
    }
    final length = _batch.finalize();
    final (b, camera, projected, _) = _world._native.project(
      _batch.backingStore,
      length,
      _width.round().clamp(1, 1 << 20),
      _height.round().clamp(1, 1 << 20),
    );
    _batch.clear();
    _camera = camera.isNull ? null : camera;
    // Read in place (no record objects) and notice whether anything moved:
    // a listener is a Flutter layer's repaint, which an idle frame with a
    // still camera and still targets should not cost.
    var changed = dropped;
    final d = b.records;
    final rows = b.recordCount < take ? b.recordCount : take;
    for (var i = 0; i < rows; i++) {
      final o = i * gen.ProjectedPointRecord.wireSize;
      final a = _anchors[i];
      final gone = a._gone;
      final x = gone ? 0.0 : d.getFloat32(o + 16, Endian.little);
      final y = gone ? 0.0 : d.getFloat32(o + 20, Endian.little);
      final depth = gone ? 0.0 : d.getFloat32(o + 24, Endian.little);
      final flags = !gone
          ? d.getUint32(o + 28, Endian.little)
          : camera.isNull
          ? ScreenAnchor._noCamera
          : ScreenAnchor._missing;
      if (x != a._x || y != a._y || depth != a._depth || flags != a._flags) {
        changed = true;
      }
      a._x = x;
      a._y = y;
      a._depth = depth;
      a._flags = flags;
    }
    if (n > maxAnchors) {
      if (!_overflowReported) {
        _overflowReported = true;
        _world.onDiagnostics([
          FynkelDiagnostic(
            commandIndex: 0xffffffff,
            code: DiagnosticCode.tooManyAnchors,
            rawCode: DiagnosticCode.tooManyAnchors.value,
            entity: EntityRef.nil,
            component: 0,
            lane: 1,
            message:
                'FYN-R0012: projection view holds $n anchors; at most $maxAnchors are projected, '
                '${n - maxAnchors} dropped',
          ),
        ]);
      }
    } else {
      _overflowReported = false;
    }
    _version++;
    if (changed) {
      for (final l in _listeners.toList(growable: false)) {
        l();
      }
    }
    return projected;
  }
}

/// A world point on screen ([ProjectionView.worldToScreen]).
class ScreenPoint {
  const ScreenPoint({
    required this.x,
    required this.y,
    required this.depth,
    required this.inFront,
    required this.onScreen,
    required this.occluded,
  });

  /// Viewport pixels (origin top-left, y down) — meaningful only while
  /// [inFront].
  final double x, y;

  /// Metres in front of the camera along its view axis.
  final double depth;

  /// In front of the near plane.
  final bool inFront;

  /// In front and inside the viewport.
  final bool onScreen;

  /// A collider on the request's `occlusionMask` sits between the camera
  /// and the point.
  final bool occluded;

  /// [onScreen] and not [occluded].
  bool get visible => onScreen && !occluded;

  @override
  String toString() =>
      'ScreenPoint(x=${x.toStringAsFixed(1)} y=${y.toStringAsFixed(1)} depth=${depth.toStringAsFixed(2)}'
      '${onScreen ? '' : inFront ? ' off screen' : ' behind'}${occluded ? ' occluded' : ''})';
}

/// The ray through a viewport point ([ProjectionView.screenRay]): Unity's
/// `Camera.ScreenPointToRay`.
class ScreenRay {
  const ScreenRay({required this.camera, required this.origin, required this.direction});

  /// The camera the frame is rendered from.
  final EntityRef camera;

  /// On the camera's near plane, under the point.
  final Vec3 origin;

  /// Unit length, away from the camera.
  final Vec3 direction;

  /// The point [distance] metres along the ray.
  Vec3 at(double distance) => origin + direction * distance;

  @override
  String toString() => 'ScreenRay($origin → $direction)';
}

/// One projected point: where an entity's local [offset] lands in the
/// viewport after the last [ProjectionView.update].
class ScreenAnchor {
  ScreenAnchor._(this._view, this.target, this.offset, this.occlusionMask);

  static const int _inFront = 1;
  static const int _onScreen = 2;
  static const int _occluded = 4;
  static const int _missing = 8;
  static const int _noCamera = 16;

  final ProjectionView _view;

  /// A [GameObject] or an entity handle.
  final Object target;
  Vec3 offset;
  int occlusionMask;
  bool _attached = true;
  double _x = 0;
  double _y = 0;
  double _depth = 0;
  int _flags = 0;
  bool _dropped = false;

  /// The last update sent no entity (a pending or dead target).
  bool _gone = false;

  /// Not projected yet: the first [ProjectionView.update] that places it
  /// notifies whatever its result is.
  bool _fresh = true;

  /// Still registered with its view.
  bool get isAttached => _attached;

  /// Viewport pixels (origin top-left, y down) — meaningful only while
  /// [inFront].
  double get x => _x;
  double get y => _y;

  /// Metres in front of the camera along its view axis (negative behind).
  double get depth => _depth;

  /// In front of the near plane: [x]/[y] are a place on the screen plane.
  bool get inFront => _flags & _inFront != 0;

  /// In front and inside the viewport rectangle.
  bool get onScreen => _flags & _onScreen != 0;

  /// A collider on [occlusionMask] sits between the camera and the point.
  bool get occluded => _flags & _occluded != 0;

  /// The entity is dead, not spawned yet, or has no transform.
  bool get missing => _flags & _missing != 0 || (target is GameObject && (target as GameObject).isPending);

  /// The world had no enabled camera at the last update.
  bool get noCamera => _flags & _noCamera != 0;

  /// Beyond the view's 64-anchor bound: never projected.
  bool get dropped => _dropped;

  /// Show the label here: on screen, not occluded, not dropped.
  bool get visible => onScreen && !occluded && !_dropped;

  /// The raw `PROJECTED_*` bits of the last update.
  int get flags => _flags;

  EntityRef _entityOrZero() {
    final t = target;
    if (t is GameObject) {
      // The native batch validates liveness; avoid one FFI lookup per anchor.
      return t.isPending || !t._alive ? EntityRef.nil : t.entity;
    }
    if (t is EntityRef) return t;
    throw ArgumentError.value(t, 'target', 'expected a GameObject or an entity handle');
  }

  /// Unregisters this anchor from its view.
  void remove() => _view.remove(this);

  @override
  String toString() =>
      'ScreenAnchor(${target is GameObject ? (target as GameObject).name : target} '
      '${visible ? 'visible' : 'hidden'} x=${x.toStringAsFixed(1)} y=${y.toStringAsFixed(1)} depth=${depth.toStringAsFixed(2)}'
      '${occluded ? ' occluded' : ''}${missing ? ' missing' : ''}${dropped ? ' dropped' : ''})';
}

/// A reusable `fynkel.rec.ProjectionAnchor` writer (the generated record
/// is immutable; one of these per view keeps [ProjectionView.update]
/// free of allocation).
class _AnchorRecord implements WireRecord {
  EntityRef entity = EntityRef.nil;
  Vec3 offset = Vec3.zero;
  int mask = 0;

  @override
  void encode(ByteData d, int offset, WireStrings strings) {
    writeEntity(d, offset, entity);
    writeVec3(d, offset + 16, this.offset);
    d.setUint32(offset + 28, mask, Endian.little);
  }
}
