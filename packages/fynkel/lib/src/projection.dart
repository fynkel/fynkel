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
  final RecordBatch _batch = RecordBatch(BufferKind.projectionAnchors, gen.ProjectionAnchorRecord.wireSize, 64 * 32);
  final List<void Function()> _listeners = [];
  double _width = 0;
  double _height = 0;
  int? _camera;
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
  int? get camera => _camera;

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

  /// Listeners run after every [update] (a Flutter layer repaints).
  void addListener(void Function() listener) => _listeners.add(listener);
  void removeListener(void Function() listener) => _listeners.remove(listener);

  /// Projects every anchor into the viewport: one FFI call. Call after
  /// `World.frame` (the same interpolation the frame renders with).
  /// Returns the number of anchors projected. With no anchors nothing is
  /// called and the listeners do not run.
  int update() {
    if (_anchors.isEmpty) return 0;
    final n = _anchors.length;
    final take = n < maxAnchors ? n : maxAnchors;
    _batch.clear();
    for (var i = 0; i < take; i++) {
      final a = _anchors[i];
      a._dropped = false;
      _batch.add(gen.ProjectionAnchorRecord(entity: a._entityOrZero(), offset: a.offset, mask: a.occlusionMask));
    }
    for (var i = take; i < n; i++) {
      _anchors[i]
        .._dropped = true
        .._flags = 0;
    }
    final (b, camera, projected, _) = _world._native.project(
      _batch.backingStore,
      _batch.finalize(),
      _width.round().clamp(1, 1 << 20),
      _height.round().clamp(1, 1 << 20),
    );
    _batch.clear();
    _camera = camera == 0 ? null : camera;
    var i = 0;
    for (final r in b.fixed(gen.ProjectedPointRecord.wireSize, gen.ProjectedPointRecord.decode)) {
      if (i >= take) break;
      final a = _anchors[i++];
      a._x = r.x;
      a._y = r.y;
      a._depth = r.depth;
      a._flags = r.flags;
    }
    if (n > maxAnchors) {
      if (!_overflowReported) {
        _overflowReported = true;
        _world.onDiagnostics([
          FynkelDiagnostic(
            commandIndex: 0xffffffff,
            code: DiagnosticCode.tooManyAnchors,
            rawCode: DiagnosticCode.tooManyAnchors.value,
            entity: 0,
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
    for (final l in _listeners.toList(growable: false)) {
      l();
    }
    return projected;
  }
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

  int _entityOrZero() {
    final t = target;
    if (t is GameObject) return t.isPending || !t.isAlive ? 0 : t.entity;
    if (t is int) return t;
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
