import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show FingerKind, Input, KeyCode;

/// Gives a world's [Input] Flutter's events, as they come: package:fynkel
/// queues them for the world's next frame. The view feeds it from its
/// `Listener`, `MouseRegion` and `Focus`.
class InputFeed {
  /// A feed into the [Input] the given function returns, asked each time,
  /// as the world the view shows can change.
  InputFeed(this._input);

  /// The input of the world the view shows now.
  final Input Function() _input;

  // A trackpad gesture's scale and turn so far, for each event's share.
  double _padScale = 1, _padRotation = 0;

  /// The view's size in logical pixels, from its last layout.
  void resize(Size size) => _input().simulate.resize(size.width, size.height);

  /// Touches are fingers of their own; a mouse or a pen is one finger,
  /// by its device, hovering or pressed.
  static int _id(PointerEvent e) => _kind(e.kind) == FingerKind.touch ? e.pointer : -1 - e.device;

  static FingerKind _kind(PointerDeviceKind kind) => switch (kind) {
    PointerDeviceKind.mouse || PointerDeviceKind.trackpad => FingerKind.mouse,
    PointerDeviceKind.stylus || PointerDeviceKind.invertedStylus => FingerKind.pen,
    PointerDeviceKind.touch || PointerDeviceKind.unknown => FingerKind.touch,
  };

  static double _pressure(PointerEvent e) {
    final range = e.pressureMax - e.pressureMin;
    return range > 0 ? ((e.pressure - e.pressureMin) / range).clamp(0.0, 1.0) : 1.0;
  }

  /// A finger, mouse button or pen touches down.
  void pointerDown(PointerDownEvent e) => _input().simulate.fingerDown(
    _id(e),
    e.localPosition.dx,
    e.localPosition.dy,
    kind: _kind(e.kind),
    buttons: e.buttons,
    pressure: _pressure(e),
  );

  /// A pressed finger, mouse or pen moves.
  void pointerMove(PointerMoveEvent e) => _input().simulate.fingerMove(
    _id(e),
    e.localPosition.dx,
    e.localPosition.dy,
    buttons: e.buttons,
    pressure: _pressure(e),
  );

  /// A finger, mouse button or pen lifts.
  void pointerUp(PointerUpEvent e) => _input().simulate.fingerUp(_id(e), e.localPosition.dx, e.localPosition.dy);

  /// Flutter took a pointer away (a gesture won elsewhere, say): it ends
  /// without a lift.
  void pointerCancel(PointerCancelEvent e) => _input().simulate.fingerCancel(_id(e));

  /// A mouse or pen over the view with nothing pressed, or coming in.
  void hover(PointerEvent e) =>
      _input().simulate.hover(_id(e), e.localPosition.dx, e.localPosition.dy, kind: _kind(e.kind));

  /// A mouse or pen leaves the view.
  void exit(PointerExitEvent e) => _input().simulate.leave(_id(e));

  /// A mouse wheel or trackpad scrolled by [delta], in logical pixels.
  void scrolled(Offset delta) => _input().simulate.scrollBy(delta.dx, delta.dy);

  /// A trackpad's two fingers come down: their pinch and twist count from
  /// here.
  void panZoomStart(PointerPanZoomStartEvent e) {
    _padScale = 1;
    _padRotation = 0;
  }

  /// A trackpad's two fingers: moving together scrolls (the other way, as
  /// Flutter's scroll views take it), spreading pinches, turning twists.
  void panZoomUpdate(PointerPanZoomUpdateEvent e) {
    final input = _input().simulate;
    final pan = e.localPanDelta;
    if (pan != Offset.zero) input.scrollBy(-pan.dx, -pan.dy);
    final scale = e.scale / _padScale, twist = e.rotation - _padRotation;
    _padScale = e.scale;
    _padRotation = e.rotation;
    if (scale != 1 || twist != 0) input.trackpad(scale: scale, twist: twist);
  }

  /// Gives the game the key, and says whether the view keeps it: only
  /// those the game uses ([Input.usesKey]). The rest go on to Flutter's
  /// shortcuts and focus, so Escape can close a dialog and Tab move focus.
  KeyEventResult key(KeyEvent e) {
    final key = keyCodes[e.physicalKey];
    if (key == null) return KeyEventResult.ignored;
    final input = _input();
    final used = switch (e) {
      KeyDownEvent() => input.simulate.keyDown(key),
      KeyUpEvent() => input.simulate.keyUp(key),
      _ => input.simulate.claims(key),
    };
    return used ? KeyEventResult.handled : KeyEventResult.ignored;
  }

  /// The view lost focus: its keys won't come up here, so they're let go.
  void releaseKeys() => _input().simulate.releaseKeys();
}

/// Flutter's physical keys as Fynkel's: where they are, not what they
/// type.
final Map<PhysicalKeyboardKey, KeyCode> keyCodes = {
  PhysicalKeyboardKey.keyA: KeyCode.a,
  PhysicalKeyboardKey.keyB: KeyCode.b,
  PhysicalKeyboardKey.keyC: KeyCode.c,
  PhysicalKeyboardKey.keyD: KeyCode.d,
  PhysicalKeyboardKey.keyE: KeyCode.e,
  PhysicalKeyboardKey.keyF: KeyCode.f,
  PhysicalKeyboardKey.keyG: KeyCode.g,
  PhysicalKeyboardKey.keyH: KeyCode.h,
  PhysicalKeyboardKey.keyI: KeyCode.i,
  PhysicalKeyboardKey.keyJ: KeyCode.j,
  PhysicalKeyboardKey.keyK: KeyCode.k,
  PhysicalKeyboardKey.keyL: KeyCode.l,
  PhysicalKeyboardKey.keyM: KeyCode.m,
  PhysicalKeyboardKey.keyN: KeyCode.n,
  PhysicalKeyboardKey.keyO: KeyCode.o,
  PhysicalKeyboardKey.keyP: KeyCode.p,
  PhysicalKeyboardKey.keyQ: KeyCode.q,
  PhysicalKeyboardKey.keyR: KeyCode.r,
  PhysicalKeyboardKey.keyS: KeyCode.s,
  PhysicalKeyboardKey.keyT: KeyCode.t,
  PhysicalKeyboardKey.keyU: KeyCode.u,
  PhysicalKeyboardKey.keyV: KeyCode.v,
  PhysicalKeyboardKey.keyW: KeyCode.w,
  PhysicalKeyboardKey.keyX: KeyCode.x,
  PhysicalKeyboardKey.keyY: KeyCode.y,
  PhysicalKeyboardKey.keyZ: KeyCode.z,
  PhysicalKeyboardKey.digit0: KeyCode.digit0,
  PhysicalKeyboardKey.digit1: KeyCode.digit1,
  PhysicalKeyboardKey.digit2: KeyCode.digit2,
  PhysicalKeyboardKey.digit3: KeyCode.digit3,
  PhysicalKeyboardKey.digit4: KeyCode.digit4,
  PhysicalKeyboardKey.digit5: KeyCode.digit5,
  PhysicalKeyboardKey.digit6: KeyCode.digit6,
  PhysicalKeyboardKey.digit7: KeyCode.digit7,
  PhysicalKeyboardKey.digit8: KeyCode.digit8,
  PhysicalKeyboardKey.digit9: KeyCode.digit9,
  PhysicalKeyboardKey.f1: KeyCode.f1,
  PhysicalKeyboardKey.f2: KeyCode.f2,
  PhysicalKeyboardKey.f3: KeyCode.f3,
  PhysicalKeyboardKey.f4: KeyCode.f4,
  PhysicalKeyboardKey.f5: KeyCode.f5,
  PhysicalKeyboardKey.f6: KeyCode.f6,
  PhysicalKeyboardKey.f7: KeyCode.f7,
  PhysicalKeyboardKey.f8: KeyCode.f8,
  PhysicalKeyboardKey.f9: KeyCode.f9,
  PhysicalKeyboardKey.f10: KeyCode.f10,
  PhysicalKeyboardKey.f11: KeyCode.f11,
  PhysicalKeyboardKey.f12: KeyCode.f12,
  PhysicalKeyboardKey.space: KeyCode.space,
  PhysicalKeyboardKey.enter: KeyCode.enter,
  PhysicalKeyboardKey.escape: KeyCode.escape,
  PhysicalKeyboardKey.tab: KeyCode.tab,
  PhysicalKeyboardKey.backspace: KeyCode.backspace,
  PhysicalKeyboardKey.delete: KeyCode.delete,
  PhysicalKeyboardKey.insert: KeyCode.insert,
  PhysicalKeyboardKey.home: KeyCode.home,
  PhysicalKeyboardKey.end: KeyCode.end,
  PhysicalKeyboardKey.pageUp: KeyCode.pageUp,
  PhysicalKeyboardKey.pageDown: KeyCode.pageDown,
  PhysicalKeyboardKey.arrowUp: KeyCode.arrowUp,
  PhysicalKeyboardKey.arrowDown: KeyCode.arrowDown,
  PhysicalKeyboardKey.arrowLeft: KeyCode.arrowLeft,
  PhysicalKeyboardKey.arrowRight: KeyCode.arrowRight,
  PhysicalKeyboardKey.shiftLeft: KeyCode.shiftLeft,
  PhysicalKeyboardKey.shiftRight: KeyCode.shiftRight,
  PhysicalKeyboardKey.controlLeft: KeyCode.controlLeft,
  PhysicalKeyboardKey.controlRight: KeyCode.controlRight,
  PhysicalKeyboardKey.altLeft: KeyCode.altLeft,
  PhysicalKeyboardKey.altRight: KeyCode.altRight,
  PhysicalKeyboardKey.metaLeft: KeyCode.metaLeft,
  PhysicalKeyboardKey.metaRight: KeyCode.metaRight,
  PhysicalKeyboardKey.capsLock: KeyCode.capsLock,
  PhysicalKeyboardKey.minus: KeyCode.minus,
  PhysicalKeyboardKey.equal: KeyCode.equal,
  PhysicalKeyboardKey.bracketLeft: KeyCode.bracketLeft,
  PhysicalKeyboardKey.bracketRight: KeyCode.bracketRight,
  PhysicalKeyboardKey.backslash: KeyCode.backslash,
  PhysicalKeyboardKey.semicolon: KeyCode.semicolon,
  PhysicalKeyboardKey.quote: KeyCode.quote,
  PhysicalKeyboardKey.backquote: KeyCode.backquote,
  PhysicalKeyboardKey.comma: KeyCode.comma,
  PhysicalKeyboardKey.period: KeyCode.period,
  PhysicalKeyboardKey.slash: KeyCode.slash,
  PhysicalKeyboardKey.numpad0: KeyCode.numpad0,
  PhysicalKeyboardKey.numpad1: KeyCode.numpad1,
  PhysicalKeyboardKey.numpad2: KeyCode.numpad2,
  PhysicalKeyboardKey.numpad3: KeyCode.numpad3,
  PhysicalKeyboardKey.numpad4: KeyCode.numpad4,
  PhysicalKeyboardKey.numpad5: KeyCode.numpad5,
  PhysicalKeyboardKey.numpad6: KeyCode.numpad6,
  PhysicalKeyboardKey.numpad7: KeyCode.numpad7,
  PhysicalKeyboardKey.numpad8: KeyCode.numpad8,
  PhysicalKeyboardKey.numpad9: KeyCode.numpad9,
  PhysicalKeyboardKey.numpadAdd: KeyCode.numpadAdd,
  PhysicalKeyboardKey.numpadSubtract: KeyCode.numpadSubtract,
  PhysicalKeyboardKey.numpadMultiply: KeyCode.numpadMultiply,
  PhysicalKeyboardKey.numpadDivide: KeyCode.numpadDivide,
  PhysicalKeyboardKey.numpadDecimal: KeyCode.numpadDecimal,
  PhysicalKeyboardKey.numpadEnter: KeyCode.numpadEnter,
};
