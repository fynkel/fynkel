part of 'world.dart';

/// What the player did since the last frame. [World.update] takes one,
/// and every object reads it as [GameObject.input]. package:fynkel_flutter
/// fills it from Flutter's events; a test makes its own.
///
/// Positions are [ViewPoint]s, in the view's logical pixels from its
/// top-left corner, so `camera.rayThrough(point)` aims straight at what's
/// under them.
final class Input {
  Input({
    this.width = 0,
    this.height = 0,
    Map<int, ViewPoint> pointers = const {},
    List<ViewPoint> taps = const [],
    this.drag = Vec2.zero,
    this.scroll = Vec2.zero,
    Set<Object> keys = const {},
    Set<Object> pressed = const {},
    Set<Object> released = const {},
  }) : pointers = Map.unmodifiable(pointers),
       taps = List.unmodifiable(taps),
       keys = Set.unmodifiable(keys),
       pressed = Set.unmodifiable(pressed),
       released = Set.unmodifiable(released);

  /// Nothing happened.
  static final none = Input();

  /// The view's size, in logical pixels.
  final double width;
  final double height;

  /// The pointers down now (fingers, pens, mice with a button pressed), by
  /// pointer id, and where they are.
  final Map<int, ViewPoint> pointers;

  /// Where each tap that ended this frame was, oldest first. A tap is a
  /// pointer that went down and up again without moving further than
  /// Flutter's tap slop (18 logical pixels for touch, 1 for a mouse), as
  /// Flutter's own `onTap` counts it. Mice tap with their primary button
  /// only. [World.update] also sends each to the object under it
  /// ([GameObject.onTap]).
  final List<ViewPoint> taps;

  /// The last of [taps], or null when there were none.
  ViewPoint? get tapped => taps.isEmpty ? null : taps.last;

  /// How far the first pointer still down moved this frame, in logical
  /// pixels, once it has moved further than a tap may. The move that takes
  /// it past the slop counts in full, so the drags of a whole gesture add
  /// up to the pointer's whole movement. [Vec2.zero] otherwise.
  final Vec2 drag;

  /// Mouse wheel and two-finger trackpad scrolling this frame, in logical
  /// pixels, positive the way Flutter's `scrollDelta` is (wheel down or
  /// fingers up: +y).
  final Vec2 scroll;

  /// The keys held now. With package:fynkel_flutter they're Flutter's
  /// `LogicalKeyboardKey`s: `input.isDown(LogicalKeyboardKey.keyW)`.
  final Set<Object> keys;

  /// The keys that went down this frame. Key repeats don't count.
  final Set<Object> pressed;

  /// The keys that went up this frame, including every held key when the
  /// view loses focus.
  final Set<Object> released;

  bool isDown(Object key) => keys.contains(key);
  bool wasPressed(Object key) => pressed.contains(key);
  bool wasReleased(Object key) => released.contains(key);

  @override
  String toString() =>
      'Input(${width}x$height, pointers: $pointers, taps: $taps, drag: $drag, scroll: $scroll, keys: $keys, '
      'pressed: $pressed, released: $released)';
}

/// A tap that hit something, as [GameObject.onTap] gets it: first the
/// object hit, then each object above it, until one returns true.
final class Tap {
  Tap._(this.at, this.hit);

  /// Where on the view.
  final ViewPoint at;

  /// What the ray from the camera through [at] hit first: the object
  /// (which may be a part below the one being asked), the point, the
  /// normal and the distance.
  final RayHit hit;

  /// The object hit: the one asked first.
  GameObject get object => hit.object;

  /// Where in the world.
  Vec3 get point => hit.point;

  @override
  String toString() => 'Tap($at on ${hit.object} at ${hit.point})';
}
