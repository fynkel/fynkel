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
  Float64List _until = Float64List(0);
  int _count = 0;
  int _capacity = 0;
  // How many the engine has: 0 means there's nothing to clear.
  int _sent = 0;

  static final _free = NativeFinalizer(malloc.nativeFree);

  /// A line from [a] to [b] (Unity's `Debug.DrawLine`).
  void line(Vec3 a, Vec3 b, {Color color = Color.white, double seconds = 0}) {
    if (!enabled) return;
    _add(a, b, color, _untilAfter(seconds));
  }

  /// A line from [origin] along [direction], as long as it is (Unity's
  /// `Debug.DrawRay`).
  void ray(Vec3 origin, Vec3 direction, {Color color = Color.white, double seconds = 0}) {
    if (!enabled) return;
    _add(origin, origin + direction, color, _untilAfter(seconds));
  }

  /// The edges of a box [size] across, its middle at [center], turned by
  /// [rotation] (Unity's `Gizmos.DrawWireCube`).
  void box(Vec3 center, Vec3 size, {Quat rotation = Quat.identity, Color color = Color.white, double seconds = 0}) {
    if (!enabled) return;
    final until = _untilAfter(seconds);
    final h = size * 0.5;
    // Each corner to the ones that differ from it in one axis, once.
    for (var i = 0; i < 8; i++) {
      for (final bit in const [1, 2, 4]) {
        if (i & bit == 0) _add(_corner(center, h, rotation, i), _corner(center, h, rotation, i | bit), color, until);
      }
    }
  }

  // A box's corner [i], its bits the axes on the plus side. Not a closure
  // in [box]: the JIT would allocate its captured variables at every call.
  static Vec3 _corner(Vec3 center, Vec3 h, Quat rotation, int i) =>
      center + rotation.rotate(Vec3(i & 1 == 0 ? -h.x : h.x, i & 2 == 0 ? -h.y : h.y, i & 4 == 0 ? -h.z : h.z));

  /// A sphere of [radius] around [center], as three circles: one around
  /// each axis (Unity's `Gizmos.DrawWireSphere`).
  void sphere(Vec3 center, double radius, {Color color = Color.white, double seconds = 0}) {
    if (!enabled) return;
    final until = _untilAfter(seconds);
    const segments = 32;
    for (var axis = 0; axis < 3; axis++) {
      for (var i = 0; i < segments; i++) {
        _add(_around(center, radius, axis, i, segments), _around(center, radius, axis, i + 1, segments), color, until);
      }
    }
  }

  // Point [i] of [segments] on the circle of [radius] around [axis].
  static Vec3 _around(Vec3 center, double radius, int axis, int i, int segments) {
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
  void path(List<Vec3> points, {bool closed = false, Color color = Color.white, double seconds = 0}) {
    if (!enabled) return;
    final until = _untilAfter(seconds);
    for (var i = 1; i < points.length; i++) {
      _add(points[i - 1], points[i], color, until);
    }
    if (closed && points.length > 2) _add(points.last, points.first, color, until);
  }

  /// Takes every line away now, those with seconds left too.
  void clear() => _count = 0;

  /// How many lines there are to draw: this frame's, and those with
  /// seconds left.
  int get count => _count;

  double _untilAfter(double seconds) =>
      seconds > 0 ? _world.time._unscaledSinceStart + seconds : double.negativeInfinity;

  void _add(Vec3 a, Vec3 b, Color color, double until) {
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
    _until[_count] = until;
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
    _free.attach(this, lines.cast(), detach: this);
    _until = Float64List(capacity)..setRange(0, _count, _until);
    _capacity = capacity;
  }

  void _release() {
    if (_lines == nullptr) return;
    _free.detach(this);
    malloc.free(_lines);
    _lines = nullptr;
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
    _now = _world.time._unscaledSinceStart;
    _keep(oneFrame: true);
    if (_count == 0 && _sent == 0) return;
    check(fy_debug_lines_set(_world.pointer, _lines.cast(), _count));
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
