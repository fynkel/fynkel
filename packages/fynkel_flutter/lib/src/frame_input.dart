import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show ViewPoint;

/// What the player did since the last frame: [FynkelView]'s `onFrame`
/// gets one each frame. Positions are [ViewPoint]s, in the view's logical
/// pixels from its top-left corner, so `camera.rayThrough(point)` aims
/// straight at what's under them.
///
/// Mouse, touch, pen and trackpad all arrive here, and keys while the view
/// has focus (it takes focus when it's tapped, and at first with
/// `autofocus`).
@immutable
final class FrameInput {
  FrameInput({
    required this.size,
    Map<int, ViewPoint> pointers = const {},
    List<ViewPoint> taps = const [],
    this.drag = Offset.zero,
    this.scroll = Offset.zero,
    Set<LogicalKeyboardKey> keys = const {},
    Set<LogicalKeyboardKey> pressed = const {},
    Set<LogicalKeyboardKey> released = const {},
  }) : pointers = Map.unmodifiable(pointers),
       taps = List.unmodifiable(taps),
       keys = Set.unmodifiable(keys),
       pressed = Set.unmodifiable(pressed),
       released = Set.unmodifiable(released);

  /// The view's size, in logical pixels.
  final Size size;

  /// The pointers down now (fingers, pens, mice with a button pressed), by
  /// Flutter's pointer id, and where they are.
  final Map<int, ViewPoint> pointers;

  /// Where each tap that ended this frame was, oldest first. A tap is a
  /// pointer that went down and up again without moving further than
  /// Flutter's tap slop (18 logical pixels for touch, 1 for a mouse), as
  /// Flutter's own `onTap` counts it. Mice tap with their primary button
  /// only.
  final List<ViewPoint> taps;

  /// The last of [taps], or null when there were none.
  ViewPoint? get tapped => taps.isEmpty ? null : taps.last;

  /// How far the first pointer still down moved this frame, in logical
  /// pixels, once it has moved further than a tap may. The move that takes
  /// it past the slop counts in full, so the drags of a whole gesture add
  /// up to the pointer's whole movement. [Offset.zero] otherwise.
  final Offset drag;

  /// Mouse wheel and two-finger trackpad scrolling this frame, in logical
  /// pixels, positive the way Flutter's `scrollDelta` is (wheel down or
  /// fingers up: +y). The view claims the wheel, so a scroll view around
  /// it doesn't scroll as well.
  final Offset scroll;

  /// The keys held now.
  final Set<LogicalKeyboardKey> keys;

  /// The keys that went down this frame. Key repeats don't count.
  final Set<LogicalKeyboardKey> pressed;

  /// The keys that went up this frame, including every held key when the
  /// view loses focus.
  final Set<LogicalKeyboardKey> released;

  bool isDown(LogicalKeyboardKey key) => keys.contains(key);
  bool wasPressed(LogicalKeyboardKey key) => pressed.contains(key);
  bool wasReleased(LogicalKeyboardKey key) => released.contains(key);

  @override
  String toString() =>
      'FrameInput(pointers: $pointers, taps: $taps, drag: $drag, scroll: $scroll, keys: $keys, '
      'pressed: $pressed, released: $released)';
}

/// A pointer that's down.
class _Pointer {
  _Pointer(this.down, this.at, {required this.canTap, required this.slop});

  final Offset down;
  Offset at;

  /// Still within the tap slop of [down], and able to tap at all.
  bool canTap;
  final double slop;

  /// Moved further than [slop] from [down] at some point.
  bool dragging = false;
}

/// Gathers the events between two frames and makes each frame's
/// [FrameInput]. The view feeds it from its `Listener` and `Focus`.
class InputCollector {
  /// The view's size in logical pixels, from its last layout.
  Size size = Size.zero;

  DeviceGestureSettings? gestureSettings;

  // Pointers in the order they went down, so the first is first.
  final _pointers = <int, _Pointer>{};
  final _taps = <Offset>[];
  var _drag = Offset.zero;
  var _scroll = Offset.zero;
  final _keys = <LogicalKeyboardKey>{};
  final _pressed = <LogicalKeyboardKey>{};
  final _released = <LogicalKeyboardKey>{};

  ViewPoint _point(Offset p) => ViewPoint(p.dx, p.dy, width: size.width, height: size.height);

  /// This frame's input. The per-frame parts start again empty.
  FrameInput take() {
    final input = FrameInput(
      size: size,
      pointers: {for (final MapEntry(:key, :value) in _pointers.entries) key: _point(value.at)},
      taps: [for (final t in _taps) _point(t)],
      drag: _drag,
      scroll: _scroll,
      keys: _keys,
      pressed: _pressed,
      released: _released,
    );
    _taps.clear();
    _drag = Offset.zero;
    _scroll = Offset.zero;
    _pressed.clear();
    _released.clear();
    return input;
  }

  void pointerDown(PointerDownEvent e) {
    final mouse = e.kind == PointerDeviceKind.mouse;
    _pointers[e.pointer] = _Pointer(
      e.localPosition,
      e.localPosition,
      canTap: !mouse || e.buttons & kPrimaryButton != 0,
      slop: computeHitSlop(e.kind, gestureSettings),
    );
  }

  void pointerMove(PointerMoveEvent e) {
    final p = _pointers[e.pointer];
    if (p == null) return;
    final from = p.at;
    p.at = e.localPosition;
    if (!p.dragging && (p.at - p.down).distance > p.slop) {
      p.dragging = true;
      p.canTap = false;
      // The whole way from where it went down, so nothing is lost to the
      // slop.
      if (_isFirst(e.pointer)) _drag += p.at - p.down;
      return;
    }
    if (p.dragging && _isFirst(e.pointer)) _drag += p.at - from;
  }

  void pointerUp(PointerUpEvent e) {
    final p = _pointers.remove(e.pointer);
    if (p != null && p.canTap) _taps.add(p.at);
  }

  void pointerCancel(PointerCancelEvent e) => _pointers.remove(e.pointer);

  bool _isFirst(int pointer) => _pointers.keys.first == pointer;

  void scrolled(Offset delta) => _scroll += delta;

  /// Whether the view used the key.
  KeyEventResult key(KeyEvent e) {
    switch (e) {
      case KeyDownEvent():
        if (_keys.add(e.logicalKey)) _pressed.add(e.logicalKey);
      case KeyUpEvent():
        if (_keys.remove(e.logicalKey)) _released.add(e.logicalKey);
      case KeyRepeatEvent():
        break;
    }
    return KeyEventResult.handled;
  }

  /// The view lost focus: its keys won't come up here, so they're let go.
  void releaseKeys() {
    _released.addAll(_keys);
    _keys.clear();
  }
}
