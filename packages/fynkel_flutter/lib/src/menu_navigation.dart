// Menus by controller (ADR-0062 amendment 1): while a menu is open the
// game view hands the controller's events here, and they become the keys
// Flutter already gives a meaning to — the arrow keys move the focus (or
// adjust a focused slider), `gameButtonA` activates — sent through the
// normal focus chain, so every Material and Cupertino control works as it
// does with a keyboard or a TV remote.

import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/gameplay.dart' show InputDevice, InputEvent, InputPhase;

/// Turns gamepad [InputEvent]s into key events for Flutter's focus.
///
/// * `dpadUp/Down/Left/Right` and the `leftStick` (pushed past [press],
///   let go below [release]) are the arrow keys. A held direction repeats
///   after [repeatDelay], every [repeatInterval].
/// * `south` is `gameButtonA` (Flutter's activate key), `east` is
///   `gameButtonB`, `west` `gameButtonX`, `north` `gameButtonY`, the
///   shoulders `gameButtonLeft1`/`Right1`, `menu` `gameButtonStart`,
///   `options` `gameButtonSelect`.
/// * Everything else (triggers, the right stick, the per-axis rows) is
///   dropped.
class MenuNavigator {
  MenuNavigator({this.send = dispatchKey});

  /// Where the key events go (the focus chain; a test can collect them).
  final bool Function(KeyEvent event) send;

  static const Duration repeatDelay = Duration(milliseconds: 400);
  static const Duration repeatInterval = Duration(milliseconds: 100);

  /// Stick deflection that counts as a push, and the one below which it
  /// is let go (hysteresis, so a stick resting near the edge does not
  /// chatter).
  static const double press = 0.5, release = 0.3;

  static const Map<String, (PhysicalKeyboardKey, LogicalKeyboardKey)> _buttons = {
    'south': (PhysicalKeyboardKey.gameButtonA, LogicalKeyboardKey.gameButtonA),
    'east': (PhysicalKeyboardKey.gameButtonB, LogicalKeyboardKey.gameButtonB),
    'west': (PhysicalKeyboardKey.gameButtonX, LogicalKeyboardKey.gameButtonX),
    'north': (PhysicalKeyboardKey.gameButtonY, LogicalKeyboardKey.gameButtonY),
    'leftShoulder': (PhysicalKeyboardKey.gameButtonLeft1, LogicalKeyboardKey.gameButtonLeft1),
    'rightShoulder': (PhysicalKeyboardKey.gameButtonRight1, LogicalKeyboardKey.gameButtonRight1),
    'menu': (PhysicalKeyboardKey.gameButtonStart, LogicalKeyboardKey.gameButtonStart),
    'options': (PhysicalKeyboardKey.gameButtonSelect, LogicalKeyboardKey.gameButtonSelect),
  };

  static const Map<String, (PhysicalKeyboardKey, LogicalKeyboardKey)> _dpad = {
    'dpadUp': (PhysicalKeyboardKey.arrowUp, LogicalKeyboardKey.arrowUp),
    'dpadDown': (PhysicalKeyboardKey.arrowDown, LogicalKeyboardKey.arrowDown),
    'dpadLeft': (PhysicalKeyboardKey.arrowLeft, LogicalKeyboardKey.arrowLeft),
    'dpadRight': (PhysicalKeyboardKey.arrowRight, LogicalKeyboardKey.arrowRight),
  };

  /// Keys this navigator holds down, so [releaseAll] can let them go.
  final Set<(PhysicalKeyboardKey, LogicalKeyboardKey)> _held = {};

  /// The stick's current direction (one of [_dpad]'s keys), if pushed.
  (PhysicalKeyboardKey, LogicalKeyboardKey)? _stick;

  /// The direction that repeats, and its timer.
  (PhysicalKeyboardKey, LogicalKeyboardKey)? _repeating;
  Timer? _repeat;

  /// The highlight strategy before the controller was used ([releaseAll]
  /// puts it back).
  FocusHighlightStrategy? _savedHighlight;

  /// One gamepad event (anything else is ignored).
  void handle(InputEvent e) {
    if (e.device != InputDevice.gamepad) return;
    if (e.phase == InputPhase.down || e.phase == InputPhase.axis) _showFocus();
    if (_buttons[e.control] case final key?) {
      if (e.phase == InputPhase.down) _down(key);
      if (e.phase == InputPhase.up) _up(key);
      return;
    }
    if (_dpad[e.control] case final key?) {
      if (e.phase == InputPhase.down) _down(key, repeats: true);
      if (e.phase == InputPhase.up) _up(key);
      return;
    }
    if (e.control == 'leftStick' && e.phase == InputPhase.axis) _stickMoved(e.x, e.y);
  }

  void _stickMoved(double x, double y) {
    final current = _stick;
    final magnitude = x.abs() > y.abs() ? x.abs() : y.abs();
    if (current != null && magnitude >= release && _direction(x, y) == current) return;
    if (current != null) {
      _stick = null;
      _up(current);
    }
    if (magnitude >= press) {
      final next = _direction(x, y);
      _stick = next;
      _down(next, repeats: true);
    }
  }

  /// The dominant axis's arrow (the stick's y is up).
  static (PhysicalKeyboardKey, LogicalKeyboardKey) _direction(double x, double y) => x.abs() > y.abs()
      ? (x > 0 ? _dpad['dpadRight']! : _dpad['dpadLeft']!)
      : (y > 0 ? _dpad['dpadUp']! : _dpad['dpadDown']!);

  void _down((PhysicalKeyboardKey, LogicalKeyboardKey) key, {bool repeats = false}) {
    if (!_held.add(key)) return;
    send(KeyDownEvent(physicalKey: key.$1, logicalKey: key.$2, timeStamp: _now()));
    if (repeats) {
      _repeat?.cancel();
      _repeating = key;
      _repeat = Timer(repeatDelay, () {
        _repeat = Timer.periodic(repeatInterval, (_) {
          if (_repeating == key && _held.contains(key)) {
            send(KeyRepeatEvent(physicalKey: key.$1, logicalKey: key.$2, timeStamp: _now()));
          }
        });
        if (_held.contains(key)) {
          send(KeyRepeatEvent(physicalKey: key.$1, logicalKey: key.$2, timeStamp: _now()));
        }
      });
    }
  }

  void _up((PhysicalKeyboardKey, LogicalKeyboardKey) key) {
    if (!_held.remove(key)) return;
    if (_repeating == key) {
      _repeat?.cancel();
      _repeat = null;
      _repeating = null;
    }
    send(KeyUpEvent(physicalKey: key.$1, logicalKey: key.$2, timeStamp: _now()));
  }

  /// A controller is navigating: show where the focus is, as Flutter does
  /// for a keyboard (touch and mouse hide the highlights).
  void _showFocus() {
    final manager = FocusManager.instance;
    if (_savedHighlight != null || manager.highlightMode == FocusHighlightMode.traditional) return;
    _savedHighlight = manager.highlightStrategy;
    manager.highlightStrategy = FocusHighlightStrategy.alwaysTraditional;
  }

  /// Every held key up and the focus highlights as they were (the menu
  /// closed, the view went away).
  void releaseAll() {
    if (_savedHighlight case final saved?) {
      FocusManager.instance.highlightStrategy = saved;
      _savedHighlight = null;
    }
    _stick = null;
    for (final key in _held.toList()) {
      _up(key);
    }
    _repeat?.cancel();
    _repeat = null;
    _repeating = null;
  }

  static Duration _now() => Duration(microseconds: DateTime.now().microsecondsSinceEpoch);

  /// Sends [event] through the focus chain from the primary focus up, as
  /// Flutter sends a key from the keyboard: each node's `onKeyEvent` in
  /// turn until one handles it or stops it. The hardware keyboard's state
  /// is untouched: these keys were never pressed on it.
  static bool dispatchKey(KeyEvent event) {
    final primary = FocusManager.instance.primaryFocus;
    if (primary == null) return false;
    for (final node in [primary, ...primary.ancestors]) {
      final result = node.onKeyEvent?.call(node, event) ?? KeyEventResult.ignored;
      switch (result) {
        case KeyEventResult.ignored:
          continue;
        case KeyEventResult.handled:
          return true;
        case KeyEventResult.skipRemainingHandlers:
          return false;
      }
    }
    return false;
  }
}
