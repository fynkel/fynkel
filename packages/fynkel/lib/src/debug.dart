part of 'world.dart';

// Debug drawing (docs/stage-4.md §3.9), as Unity's Debug.DrawLine and its
// Gizmos' wire shapes: lines over the scene, for a frame or for some
// seconds. The lines drawn during a frame are kept here, in memory the
// engine reads, and sent in one call at the end of [World.update]; a frame
// that draws none, after one that drew none, makes no call at all.

/// Lines drawn over the scene, through everything, to see what the code
/// sees: a ray, a path, where a box is. Reached as [World.debug] (or
/// `debug` in an object or a behaviour).
///
/// A line lasts one frame, or `seconds` of unscaled time
/// ([Time.unscaledSinceStart]), so they pass while the game is paused:
/// draw it every frame to keep it. Lines drawn outside a frame show from
/// the next one.
final class Debug implements Finalizable {
  Debug._(this._world);

  final World _world;

  /// Whether drawing does anything. On, except in a release build; turn
  /// it on there to draw anyway, or off to stop (the lines go too).
  bool enabled = !const bool.fromEnvironment('dart.vm.product');

  // The lines, ten floats each as the engine's FyDebugLine (a, b, colour),
  // and when each goes, in unscaled time: -infinity after this frame.
  Pointer<Float> _lines = nullptr;
  // The same memory as the engine's lines, cast once.
  Pointer<FyDebugLine> _engineLines = nullptr;
  Float64List _until = Float64List(0);
  int _count = 0;
  int _capacity = 0;
  // How many the engine has: 0 means there's nothing to clear.
  int _sent = 0;

  static final _free = NativeFinalizer(malloc.nativeFree);

  /// A line from [a] to [b] (Unity's `Debug.DrawLine`).
  void line(Vec3 a, Vec3 b, {Color color = Color.white, double? seconds}) {
    if (!enabled) return;
    _addSeconds = seconds;
    _add(a, b, color);
  }

  /// A line from [origin] along [direction], as long as it is (Unity's
  /// `Debug.DrawRay`).
  void ray(Vec3 origin, Vec3 direction, {Color color = Color.white, double? seconds}) {
    if (!enabled) return;
    _addSeconds = seconds;
    _add(origin, origin + direction, color);
  }

  /// The edges of a box [size] across, its middle at [center], turned by
  /// [rotation] (Unity's `Gizmos.DrawWireCube`).
  void box(Vec3 center, Vec3 size, {Quat rotation = Quat.identity, Color color = Color.white, double? seconds}) {
    if (!enabled) return;
    _addSeconds = seconds;
    final h = size * 0.5;
    // Each corner to the ones that differ from it in one axis, once.
    for (var i = 0; i < 8; i++) {
      for (var bit = 1; bit < 8; bit <<= 1) {
        if (i & bit == 0) _add(_corner(center, h, rotation, i), _corner(center, h, rotation, i | bit), color);
      }
    }
  }

  // A box's corner [i], its bits the axes on the plus side. Not a closure
  // in [box]: the JIT would allocate its captured variables at every call.
  static Vec3 _corner(Vec3 center, Vec3 h, Quat rotation, int i) =>
      center + rotation.rotate(Vec3(i & 1 == 0 ? -h.x : h.x, i & 2 == 0 ? -h.y : h.y, i & 4 == 0 ? -h.z : h.z));

  /// A sphere of [radius] around [center], as three circles: one around
  /// each axis (Unity's `Gizmos.DrawWireSphere`).
  void sphere(Vec3 center, double radius, {Color color = Color.white, double? seconds}) {
    if (!enabled) return;
    _addSeconds = seconds;
    _radius = radius;
    const segments = 32;
    for (var axis = 0; axis < 3; axis++) {
      for (var i = 0; i < segments; i++) {
        _add(_around(center, axis, i, segments), _around(center, axis, i + 1, segments), color);
      }
    }
  }

  // The sphere's radius for [_around]: a field, as the JIT boxes a double
  // passed to a call.
  double _radius = 0;

  // Point [i] of [segments] on the circle of [_radius] around [axis].
  Vec3 _around(Vec3 center, int axis, int i, int segments) {
    final radius = _radius;
    final angle = 2 * math.pi * i / segments;
    final (c, s) = (math.cos(angle) * radius, math.sin(angle) * radius);
    return center +
        switch (axis) {
          0 => Vec3(0, c, s),
          1 => Vec3(c, 0, s),
          _ => Vec3(c, s, 0),
        };
  }

  /// Lines through [points] in order, and back to the first if [closed].
  void path(List<Vec3> points, {bool closed = false, Color color = Color.white, double? seconds}) {
    if (!enabled) return;
    _addSeconds = seconds;
    for (var i = 1; i < points.length; i++) {
      _add(points[i - 1], points[i], color);
    }
    if (closed && points.length > 2) _add(points.last, points.first, color);
  }

  /// Draws each particle system's shape where its object is, each frame,
  /// and the planes it collides with (as Unity's gizmos show them in the
  /// Scene view): where its particles come from, and what they bounce
  /// off. Off unless set.
  bool drawShapes = false;

  static const _shapeColor = Color(0xFF66CCFF);
  static const _planeColor = Color(0xFFFFCC33);

  /// The frame's shapes, with [drawShapes].
  void _drawShapes() {
    _addSeconds = null;
    for (final system in _world._particleSystems.values) {
      if (system.isRemoved || !system.enabled) continue;
      final object = system.gameObject;
      if (!object.activeInHierarchy) continue;
      final world = object.worldTransform;
      final (at, turn) = (world.position, world.rotation);
      final shape = system.shape;
      switch (shape._kind) {
        case _ShapeKind.point:
          for (final axis in const [Vec3(0.1, 0, 0), Vec3(0, 0.1, 0), Vec3(0, 0, 0.1)]) {
            final d = turn.rotate(axis);
            _add(at - d, at + d, _shapeColor);
          }
        case _ShapeKind.sphere || _ShapeKind.hemisphere:
          for (var axis = 0; axis < 3; axis++) {
            _ring(at, turn, shape.radius, axis, shape._kind == _ShapeKind.hemisphere && axis != 1);
          }
        case _ShapeKind.cone:
          // Its base, and where its sides are a metre up.
          final top = shape.radius + math.tan(shape.angle);
          final up = turn.rotate(const Vec3(0, 1, 0));
          _ring(at, turn, shape.radius, 1, false);
          _ring(at + up, turn, top, 1, false);
          for (final side in const [Vec3(1, 0, 0), Vec3(-1, 0, 0), Vec3(0, 0, 1), Vec3(0, 0, -1)]) {
            _add(at + turn.rotate(side * shape.radius), at + up + turn.rotate(side * top), _shapeColor);
          }
        case _ShapeKind.box:
          final h = shape.size * 0.5;
          for (var i = 0; i < 8; i++) {
            for (var bit = 1; bit < 8; bit <<= 1) {
              if (i & bit == 0) _add(_corner(at, h, turn, i), _corner(at, h, turn, i | bit), _shapeColor);
            }
          }
        case _ShapeKind.circle:
          _ring(at, turn, shape.radius, 1, false);
        case _ShapeKind.edge:
          final d = turn.rotate(Vec3(shape.radius, 0, 0));
          _add(at - d, at + d, _shapeColor);
      }
      for (final plane in system.collision?.planes ?? const <GameObject>[]) {
        if (plane.isDestroyed || !identical(plane._world, _world)) continue;
        final p = plane.worldTransform;
        // A square two metres across, its up the way it faces.
        final (a, b) = (p.rotation.rotate(const Vec3(1, 0, 0)), p.rotation.rotate(const Vec3(0, 0, 1)));
        final corners = [p.position + a + b, p.position - a + b, p.position - a - b, p.position + a - b];
        for (var i = 0; i < 4; i++) {
          _add(corners[i], corners[(i + 1) % 4], _planeColor);
        }
        _add(p.position, p.position + p.rotation.rotate(const Vec3(0, 0.5, 0)), _planeColor);
      }
    }
  }

  /// A circle of [radius] round [center], turned by [turn], round [axis]
  /// (0 X, 1 Y, 2 Z); only its upper half with [upper].
  void _ring(Vec3 center, Quat turn, double radius, int axis, bool upper) {
    _radius = radius;
    const segments = 32;
    for (var i = 0; i < segments; i++) {
      final (a, b) = (_around(Vec3.zero, axis, i, segments), _around(Vec3.zero, axis, i + 1, segments));
      if (upper && (a.y < -1e-6 || b.y < -1e-6)) continue;
      _add(center + turn.rotate(a), center + turn.rotate(b), _shapeColor);
    }
  }

  /// Takes every line away now, those with seconds left too.
  void clear() => _count = 0;

  /// How many lines there are to draw: this frame's, and those with
  /// seconds left.
  int get count => _count;

  // How long the lines being added stay. The public calls take a nullable
  // double and keep it so: the JIT boxes a double with a default, or one
  // stored in a double field, at every call too big to inline.
  double? _addSeconds;

  void _add(Vec3 a, Vec3 b, Color color) {
    if (_world.isDisposed) throw StateError('this Debug belongs to a disposed World, so it can\'t draw');
    if (_count == _capacity) _grow();
    final f = _lines;
    final at = _count * 10;
    f
      ..[at] = a.x
      ..[at + 1] = a.y
      ..[at + 2] = a.z
      ..[at + 3] = b.x
      ..[at + 4] = b.y
      ..[at + 5] = b.z
      ..[at + 6] = color.r
      ..[at + 7] = color.g
      ..[at + 8] = color.b
      ..[at + 9] = color.a;
    final seconds = _addSeconds;
    _until[_count] = seconds != null && seconds > 0
        ? _world.time._unscaledSinceStart + seconds
        : double.negativeInfinity;
    _count++;
  }

  void _grow() {
    final capacity = _capacity == 0 ? 64 : _capacity * 2;
    final lines = malloc<Float>(capacity * 10);
    for (var i = 0; i < _count * 10; i++) {
      lines[i] = _lines[i];
    }
    _release();
    _lines = lines;
    _engineLines = lines.cast();
    _free.attach(this, lines.cast(), detach: this);
    _until = Float64List(capacity)..setRange(0, _count, _until);
    _capacity = capacity;
  }

  void _release() {
    if (_lines == nullptr) return;
    _free.detach(this);
    malloc.free(_lines);
    _lines = nullptr;
    _engineLines = nullptr;
  }

  /// Frees the lines for good, as the world is disposed.
  void _dispose() {
    _release();
    _count = 0;
    _capacity = 0;
  }

  /// Sends the lines to the engine, at the end of a frame: those drawn
  /// this frame, and those whose seconds haven't run out. Only those with
  /// seconds left are kept.
  void _flush() {
    if (!enabled) _count = 0;
    if (enabled && drawShapes) _drawShapes();
    _now = _world.time._unscaledSinceStart;
    _keep(oneFrame: true);
    if (_count == 0 && _sent == 0) return;
    check(fy_debug_lines_set(_world.pointer, _engineLines, _count));
    _sent = _count;
    _keep(oneFrame: false);
  }

  // The time [_flush] keeps lines until: a field, as the JIT boxes a double
  // passed along with a named argument.
  double _now = 0;

  /// Keeps the lines whose time hasn't run out at [_now], and the one-frame
  /// lines if [oneFrame], in order.
  void _keep({required bool oneFrame}) {
    final now = _now;
    var kept = 0;
    for (var i = 0; i < _count; i++) {
      final until = _until[i];
      if (!(until > now || (oneFrame && until == double.negativeInfinity))) continue;
      if (kept != i) {
        for (var j = 0; j < 10; j++) {
          _lines[kept * 10 + j] = _lines[i * 10 + j];
        }
        _until[kept] = until;
      }
      kept++;
    }
    _count = kept;
  }
}
