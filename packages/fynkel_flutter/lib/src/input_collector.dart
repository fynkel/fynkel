import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show Input, Vec2, ViewPoint;

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
/// [Input]: Flutter's events in package:fynkel's terms. Keys are Flutter's
/// [LogicalKeyboardKey]s. The view feeds it from its `Listener` and `Focus`.
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
  Input take() {
    final input = Input(
      width: size.width,
      height: size.height,
      pointers: {for (final MapEntry(:key, :value) in _pointers.entries) key: _point(value.at)},
      taps: [for (final t in _taps) _point(t)],
      drag: Vec2(_drag.dx, _drag.dy),
      scroll: Vec2(_scroll.dx, _scroll.dy),
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
