// Host-side input capture (blueprint §5.1, §19.2d item 7; docs/input.md):
// turns Flutter key and pointer events, the platform's gamepad and
// captured-mouse events and the touch overlay's controls into the raw
// `InputEvent`s one `World.frame` receives. Flutter consumes its own
// interactions first (a focused text field keeps its keys); the viewport
// owns the focus rules: a focus loss, a pause, a hidden app is a `reset`.

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart';

/// Collects one frame's raw control events. The game view feeds it and
/// hands [take] to `World.frame`; a [TouchOverlay] and the platform side
/// (`FynkelInputChannel`) add to it; an [InputRecorder] can copy every
/// frame it produced.
class FynkelInputCapture {
  FynkelInputCapture({this.recorder});

  /// When set, every frame's dt and events are recorded (`*.input.json`).
  InputRecorder? recorder;

  final List<InputEvent> _events = [];
  int _buttons = 0;

  /// Whether mouse movement comes from the platform's captured-pointer
  /// deltas (Flutter's own move deltas are then ignored).
  bool pointerCaptured = false;

  /// Events collected since the last [take].
  List<InputEvent> get pending => List.unmodifiable(_events);

  void add(InputEvent e) => _events.add(e);
  void addAll(Iterable<InputEvent> events) => _events.addAll(events);

  /// Every control up, with edges for the next tick.
  void reset() {
    _buttons = 0;
    _events.add(const InputEvent.reset());
  }

  /// The frame's events (a fresh list; the capture starts over). With a
  /// [recorder], the frame is recorded as [dt] seconds.
  List<InputEvent> take(double dt) {
    if (_events.isEmpty) {
      recorder?.frame(dt, const []);
      return const [];
    }
    final out = List<InputEvent>.of(_events, growable: false);
    _events.clear();
    recorder?.frame(dt, out);
    return out;
  }

  /// A key from the viewport's `Focus.onKeyEvent`: physical keys, so WASD
  /// is the same four keys on every layout. Repeats are not edges.
  /// Returns whether the key is one the grammar names.
  bool handleKey(KeyEvent event) {
    final name = keyName(event.physicalKey);
    if (name == null) return false;
    switch (event) {
      case KeyDownEvent():
        _events.add(InputEvent.down(InputDevice.keyboard, name));
      case KeyUpEvent():
        _events.add(InputEvent.up(InputDevice.keyboard, name));
      case KeyRepeatEvent():
        break;
    }
    return true;
  }

  /// A pointer event from the viewport (mouse buttons, movement as
  /// `mouse:delta` in physical pixels, the wheel — and a trackpad's
  /// two-finger scroll — as `mouse:wheel`). Touch
  /// pointers are the overlay's business.
  void handlePointer(PointerEvent event, double devicePixelRatio) {
    if (event.kind != PointerDeviceKind.mouse && event.kind != PointerDeviceKind.trackpad) {
      return;
    }
    switch (event) {
      case PointerDownEvent() || PointerUpEvent() || PointerCancelEvent():
        final now = event is PointerCancelEvent ? 0 : event.buttons;
        _buttonsChanged(now);
      case PointerMoveEvent() || PointerHoverEvent():
        if (event is PointerMoveEvent) _buttonsChanged(event.buttons);
        if (!pointerCaptured && event.delta != Offset.zero) {
          _events.add(
            InputEvent.move(
              InputDevice.mouse,
              'delta',
              event.delta.dx * devicePixelRatio,
              -event.delta.dy * devicePixelRatio,
            ),
          );
        }
      case PointerScrollEvent():
        // Positive away from the user (up), in logical pixels of scroll.
        if (event.scrollDelta.dy != 0) {
          _events.add(InputEvent.move(InputDevice.mouse, 'wheel', -event.scrollDelta.dy));
        }
      case PointerPanZoomUpdateEvent():
        // A trackpad's two-finger scroll (macOS sends pan/zoom, not
        // scrolls; foundation 4.6): the wheel, as Unity's scroll delta.
        // Fingers moving down is a wheel turned away from the user.
        if (event.panDelta.dy != 0) {
          _events.add(InputEvent.move(InputDevice.mouse, 'wheel', event.panDelta.dy));
        }
      default:
        break;
    }
  }

  void _buttonsChanged(int now) {
    final was = _buttons;
    if (now == was) return;
    _buttons = now;
    for (final (bit, name) in _mouseButtons) {
      final wasDown = was & bit != 0;
      final isDown = now & bit != 0;
      if (isDown && !wasDown) _events.add(InputEvent.down(InputDevice.mouse, name));
      if (wasDown && !isDown) _events.add(InputEvent.up(InputDevice.mouse, name));
    }
  }

  static const _mouseButtons = [
    (kPrimaryMouseButton, 'left'),
    (kSecondaryMouseButton, 'right'),
    (kMiddleMouseButton, 'middle'),
    (kBackMouseButton, 'back'),
    (kForwardMouseButton, 'forward'),
  ];

  /// The grammar's name for a physical key (`A`, `Space`, `ArrowUp`,
  /// `ShiftLeft`, `F1`), or null for a key the grammar does not name.
  static String? keyName(PhysicalKeyboardKey key) => _keyNames[key];

  static final Map<PhysicalKeyboardKey, String> _keyNames = {
    PhysicalKeyboardKey.keyA: 'A',
    PhysicalKeyboardKey.keyB: 'B',
    PhysicalKeyboardKey.keyC: 'C',
    PhysicalKeyboardKey.keyD: 'D',
    PhysicalKeyboardKey.keyE: 'E',
    PhysicalKeyboardKey.keyF: 'F',
    PhysicalKeyboardKey.keyG: 'G',
    PhysicalKeyboardKey.keyH: 'H',
    PhysicalKeyboardKey.keyI: 'I',
    PhysicalKeyboardKey.keyJ: 'J',
    PhysicalKeyboardKey.keyK: 'K',
    PhysicalKeyboardKey.keyL: 'L',
    PhysicalKeyboardKey.keyM: 'M',
    PhysicalKeyboardKey.keyN: 'N',
    PhysicalKeyboardKey.keyO: 'O',
    PhysicalKeyboardKey.keyP: 'P',
    PhysicalKeyboardKey.keyQ: 'Q',
    PhysicalKeyboardKey.keyR: 'R',
    PhysicalKeyboardKey.keyS: 'S',
    PhysicalKeyboardKey.keyT: 'T',
    PhysicalKeyboardKey.keyU: 'U',
    PhysicalKeyboardKey.keyV: 'V',
    PhysicalKeyboardKey.keyW: 'W',
    PhysicalKeyboardKey.keyX: 'X',
    PhysicalKeyboardKey.keyY: 'Y',
    PhysicalKeyboardKey.keyZ: 'Z',
    PhysicalKeyboardKey.digit0: '0',
    PhysicalKeyboardKey.digit1: '1',
    PhysicalKeyboardKey.digit2: '2',
    PhysicalKeyboardKey.digit3: '3',
    PhysicalKeyboardKey.digit4: '4',
    PhysicalKeyboardKey.digit5: '5',
    PhysicalKeyboardKey.digit6: '6',
    PhysicalKeyboardKey.digit7: '7',
    PhysicalKeyboardKey.digit8: '8',
    PhysicalKeyboardKey.digit9: '9',
    PhysicalKeyboardKey.space: 'Space',
    PhysicalKeyboardKey.enter: 'Enter',
    PhysicalKeyboardKey.escape: 'Escape',
    PhysicalKeyboardKey.tab: 'Tab',
    PhysicalKeyboardKey.backspace: 'Backspace',
    PhysicalKeyboardKey.delete: 'Delete',
    PhysicalKeyboardKey.insert: 'Insert',
    PhysicalKeyboardKey.home: 'Home',
    PhysicalKeyboardKey.end: 'End',
    PhysicalKeyboardKey.pageUp: 'PageUp',
    PhysicalKeyboardKey.pageDown: 'PageDown',
    PhysicalKeyboardKey.capsLock: 'CapsLock',
    PhysicalKeyboardKey.arrowUp: 'ArrowUp',
    PhysicalKeyboardKey.arrowDown: 'ArrowDown',
    PhysicalKeyboardKey.arrowLeft: 'ArrowLeft',
    PhysicalKeyboardKey.arrowRight: 'ArrowRight',
    PhysicalKeyboardKey.shiftLeft: 'ShiftLeft',
    PhysicalKeyboardKey.shiftRight: 'ShiftRight',
    PhysicalKeyboardKey.controlLeft: 'ControlLeft',
    PhysicalKeyboardKey.controlRight: 'ControlRight',
    PhysicalKeyboardKey.altLeft: 'AltLeft',
    PhysicalKeyboardKey.altRight: 'AltRight',
    PhysicalKeyboardKey.metaLeft: 'MetaLeft',
    PhysicalKeyboardKey.metaRight: 'MetaRight',
    PhysicalKeyboardKey.minus: 'Minus',
    PhysicalKeyboardKey.equal: 'Equal',
    PhysicalKeyboardKey.bracketLeft: 'BracketLeft',
    PhysicalKeyboardKey.bracketRight: 'BracketRight',
    PhysicalKeyboardKey.backslash: 'Backslash',
    PhysicalKeyboardKey.semicolon: 'Semicolon',
    PhysicalKeyboardKey.quote: 'Quote',
    PhysicalKeyboardKey.backquote: 'Backquote',
    PhysicalKeyboardKey.comma: 'Comma',
    PhysicalKeyboardKey.period: 'Period',
    PhysicalKeyboardKey.slash: 'Slash',
    PhysicalKeyboardKey.f1: 'F1',
    PhysicalKeyboardKey.f2: 'F2',
    PhysicalKeyboardKey.f3: 'F3',
    PhysicalKeyboardKey.f4: 'F4',
    PhysicalKeyboardKey.f5: 'F5',
    PhysicalKeyboardKey.f6: 'F6',
    PhysicalKeyboardKey.f7: 'F7',
    PhysicalKeyboardKey.f8: 'F8',
    PhysicalKeyboardKey.f9: 'F9',
    PhysicalKeyboardKey.f10: 'F10',
    PhysicalKeyboardKey.f11: 'F11',
    PhysicalKeyboardKey.f12: 'F12',
  };
}

/// The platform side's events (`fynkel_flutter/input`): gamepad state
/// changes from the GameController framework and captured-mouse deltas,
/// batched per platform callback as `[device, control, phase, x, y]`
/// rows. Every live [FynkelInputCapture] that [attach]ed receives them.
abstract final class FynkelInputChannel {
  static const channel = MethodChannel('fynkel_flutter/input');
  static final Set<FynkelInputCapture> _captures = {};
  static bool _installed = false;

  /// Received batches (tests, HUDs).
  static int batches = 0;

  static void attach(FynkelInputCapture capture) {
    _captures.add(capture);
    if (!_installed) {
      _installed = true;
      channel.setMethodCallHandler(_handle);
    }
  }

  static void detach(FynkelInputCapture capture) => _captures.remove(capture);

  static Future<Object?> _handle(MethodCall call) async {
    if (call.method != 'events') return null;
    batches++;
    final rows = call.arguments as List?;
    if (rows == null) return null;
    for (final row in rows) {
      final r = row as List;
      final device = InputDevice.values[(r[0] as int).clamp(0, InputDevice.values.length - 1)];
      final phase = InputPhase.values[(r[2] as int).clamp(0, InputPhase.values.length - 1)];
      final e = InputEvent(device, r[1] as String, phase, x: (r[3] as num).toDouble(), y: (r[4] as num).toDouble());
      for (final c in _captures) {
        c.add(e);
      }
    }
    return null;
  }

  /// Hides the cursor and routes its movement to `mouse:delta` (macOS;
  /// a no-op elsewhere). The game view calls this through
  /// `FynkelGameViewState.capturePointer`.
  static Future<bool> capturePointer(bool capture) async {
    if (kIsWeb) return false;
    try {
      return await channel.invokeMethod<bool>('capturePointer', {'capture': capture}) ?? false;
    } on MissingPluginException {
      return false;
    }
  }

  /// Whether the platform currently reports a connected gamepad.
  static Future<int> gamepadCount() async {
    if (kIsWeb) return 0;
    try {
      return await channel.invokeMethod<int>('gamepads') ?? 0;
    } on MissingPluginException {
      return 0;
    }
  }
}
