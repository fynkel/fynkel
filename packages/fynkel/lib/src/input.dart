part of 'world.dart';

// Input (docs/stage-4.md §3.6): raw events in, fingers, gestures, signals
// and polled state out. This part knows nothing about objects: what
// needs the scene (what's under a finger, the objects' handlers) goes
// through [_Targets], which the world gives it (pointing.dart).

/// What the player does: fingers (touches, the mouse, pens), gestures,
/// keys and named controls. Every world has one, as [World.input], and
/// every object and behaviour reads it as `input`.
///
/// **Listening**: its signals tell what happened, in the input step at
/// the start of [World.update] (before any start or update):
/// `on(input.onTap, (finger) => ...)`. **Polling**, as Unity's `Input`:
/// [isPressed], [wasPressed] and [wasReleased] for keys and mouse buttons,
/// [fingers], [mousePosition], [scroll], [axis2] and [action], in any
/// update.
///
/// **Flutter controls**: a button or a stick drawn by Flutter drives the
/// same actions and axes as keys do: [InputAction.press] and
/// [InputAction.release], and [setAxis2].
///
/// **Feeding it**: package:fynkel_flutter gives it Flutter's events, and a
/// test its own, through [simulate]. They're queued, and the next frame's
/// input step handles them in order.
///
/// While the world is paused ([World.paused]) it takes nothing: what was
/// held is let go when the pause starts, and what comes meanwhile is
/// dropped.
final class Input {
  /// An input of its own, with no world: for tests of input alone, driven
  /// by [update]. A world makes its own.
  Input({InputSettings? settings}) : settings = settings ?? InputSettings() {
    _bindDefaults();
  }

  /// The tap's slop and time, the double tap's interval, the long press's
  /// time and the swipe's speed.
  final InputSettings settings;

  /// The layers a finger can touch ([GameObject.layer]): taps, presses and
  /// drags go through objects on the others, as do the layers the camera
  /// doesn't draw ([Camera.cullingMask]). Unity's `eventMask`.
  LayerMask layers = LayerMask.all;

  // The world, through which the input step reaches objects; null for an
  // input of its own.
  _Targets? _targets;

  double _width = 0, _height = 0;

  /// The view's width, in logical pixels: 0 until a view gives it.
  double get width => _width;

  /// The view's height, in logical pixels: 0 until a view gives it.
  double get height => _height;

  // The unscaled seconds since the first frame, and frames so far.
  double _now = 0;
  int _frame = 0;

  final _queue = <_Raw>[];

  final _fingers = <Finger>[];
  late final _fingersView = UnmodifiableListView(_fingers);

  final _keys = <KeyCode>{};
  final _pressedKeys = <KeyCode>{};
  final _releasedKeys = <KeyCode>{};

  // Mouse buttons as Flutter's bits (left 1, right 2, middle 4); a touch
  // or a pen down holds the left one.
  int _buttons = 0, _pressedButtons = 0, _releasedButtons = 0;

  double _scrollX = 0, _scrollY = 0;
  Vec2 _scroll = Vec2.zero;

  // A trackpad's pinch and twist since the last frame.
  double _padScale = 1, _padTwist = 0;

  // The last tap, for double taps.
  double _lastTapAt = double.negativeInfinity;
  double _lastTapX = 0, _lastTapY = 0;
  int _lastTapCount = 0;
  FingerKind? _lastTapKind;
  MouseButton? _lastTapButton;

  final _actions = <String, InputAction>{};
  final _axes = <String, _Axis2>{};

  // The keys the game uses: bound, or asked about. The view claims these
  // from Flutter and lets it have the rest too.
  final _used = List<bool>.filled(KeyCode.values.length, false);

  // Set by the world: whether it's paused, and whether the pause has yet
  // to let go of what's held.
  bool _paused = false;
  bool _pausing = false;

  /// What feeds this input: the view's events, or a test's.
  late final InputSimulation simulate = InputSimulation._(this);

  // --- Signals ---

  /// A finger went down: a touch, a pen, or a mouse button pressed while
  /// none was.
  final onFingerDown = Signal<Finger>();

  /// A finger went up (or was cancelled: [Finger.wasCancelled]). It's no
  /// longer in [fingers], except the mouse, which stays while it's in the
  /// view.
  final onFingerUp = Signal<Finger>();

  /// Each frame, for each finger that's down, after its moves.
  final onFingerHeld = Signal<Finger>();

  /// A finger that's down moved this frame: once a frame, with
  /// [Finger.delta] the whole move.
  final onFingerMove = Signal<Finger>();

  /// The mouse (or a pen) moved over the view with nothing pressed.
  final onHover = Signal<Finger>();

  /// A tap: a finger that went up within the tap's slop and time
  /// ([InputSettings.tapSlop], [InputSettings.tapTime]) and wasn't long
  /// pressed. [Finger.tapCount] is 2 for the second tap of a double tap,
  /// 3 for a triple; each tap comes, so a double tap is a tap of 1 then a
  /// tap of 2. Every mouse button taps ([Finger.button]).
  final onTap = Signal<Finger>();

  /// A finger held still for [InputSettings.longPressTime]; it won't tap
  /// when it goes up.
  final onLongPress = Signal<Finger>();

  /// A finger moved further than the tap's slop: a drag starts. It won't
  /// tap.
  final onDragStart = Signal<Finger>();

  /// A dragging finger moved: once a frame, with [Finger.delta] the
  /// frame's move (the first takes in the slop, so a drag's deltas add up
  /// to the finger's whole move).
  final onDrag = Signal<Finger>();

  /// A dragging finger went up, or was cancelled.
  final onDragEnd = Signal<Finger>();

  /// A drag let go at [InputSettings.swipeSpeed] or faster, right after
  /// its [onDragEnd].
  final onSwipe = Signal<Swipe>();

  /// Two or more fingers (or a trackpad) moved apart or together this
  /// frame.
  final onPinch = Signal<Pinch>();

  /// Two or more fingers (or a trackpad) turned this frame. The same
  /// [Pinch] as [onPinch]'s when both happen.
  final onTwist = Signal<Pinch>();

  /// A mouse wheel or a trackpad scrolled this frame: the frame's [scroll].
  final onScroll = Signal<Vec2>();

  /// A key went down. Held keys repeating don't count.
  final onKeyDown = Signal<KeyPress>();

  /// A key went up, including each held key when the view loses focus.
  final onKeyUp = Signal<KeyPress>();

  // --- Polling ---

  /// The fingers now: those down, in the order they went down, and the
  /// mouse (or a hovering pen) while it's over the view, pressed or not.
  List<Finger> get fingers => _fingersView;

  /// Whether a key or mouse button is held now. A touch or a pen down
  /// holds [MouseButton.left], as in Unity.
  ///
  /// Asking about a key claims it from Flutter: see [usesKey].
  bool isPressed(InputButton button) => switch (button) {
    final KeyCode key => _use(key) && _keys.contains(key),
    final MouseButton m => _buttons & m.bit != 0,
  };

  /// Whether a key or mouse button went down this frame.
  bool wasPressed(InputButton button) => switch (button) {
    final KeyCode key => _use(key) && _pressedKeys.contains(key),
    final MouseButton m => _pressedButtons & m.bit != 0,
  };

  /// Whether a key or mouse button went up this frame.
  bool wasReleased(InputButton button) => switch (button) {
    final KeyCode key => _use(key) && _releasedKeys.contains(key),
    final MouseButton m => _releasedButtons & m.bit != 0,
  };

  /// Whether the game uses [key]: it's bound to an axis or an action, or
  /// the game asked about it ([isPressed], [wasPressed], [wasReleased]).
  /// The view gives the game every key, and claims these: the others reach
  /// Flutter's shortcuts and focus too, so Escape can close a dialog and
  /// Tab move focus, unless the game asks about them. [onKeyDown] and
  /// [onKeyUp] hear every key. While the world is paused the view claims
  /// none, so a pause menu's shortcuts get them all.
  bool usesKey(KeyCode key) => _used[key.index];

  @pragma('vm:prefer-inline')
  bool _use(KeyCode key) {
    _used[key.index] = true;
    return true;
  }

  void _useAll(List<InputButton> buttons) {
    for (final b in buttons) {
      if (b is KeyCode) _use(b);
    }
  }

  /// Where the mouse is (or a hovering pen), or else the first finger
  /// down; null with neither.
  ViewPoint? get mousePosition {
    for (var i = 0; i < _fingers.length; i++) {
      if (_fingers[i].kind != FingerKind.touch) return _fingers[i].position;
    }
    return _fingers.isEmpty ? null : _fingers.first.position;
  }

  /// Mouse wheel and two-finger trackpad scrolling this frame, in logical
  /// pixels. Y is positive with the wheel turned away from you, as Unity's
  /// `mouseScrollDelta` (the other way from Flutter's `scrollDelta`); X is
  /// positive to the right.
  Vec2 get scroll => _scroll;

  // --- Axes and actions ---

  /// The named two-way control [name] now, as a [Vec2] of length at most
  /// 1: x to the right, y up. `'move'` reads W, A, S, D and the arrow
  /// keys; [bindAxis2] adds others, and [setAxis2] adds a Flutter stick's
  /// value to the keys'. Throws an [ArgumentError] for a name that isn't
  /// bound or set. Zero while the world is paused. It's the "2D Vector
  /// composite" of Unity's Input System; there's no one-way axis, so for
  /// Unity's `GetAxis("Horizontal")` read `axis2('move').x`.
  Vec2 axis2(String name) {
    final axis = _axes[name] ?? (throw ArgumentError.value(name, 'name', 'is no axis (bound: ${_axes.keys})'));
    if (_paused) return Vec2.zero;
    final x = (_anyDown(axis.right) ? 1 : 0) - (_anyDown(axis.left) ? 1 : 0);
    final y = (_anyDown(axis.up) ? 1 : 0) - (_anyDown(axis.down) ? 1 : 0);
    final set = axis.value;
    if (x == 0 && y == 0) return set;
    // Diagonals are no faster.
    final scale = x != 0 && y != 0 ? math.sqrt1_2 : 1.0;
    if (identical(set, Vec2.zero)) return Vec2(x * scale, y * scale);
    final sx = x * scale + set.x, sy = y * scale + set.y;
    final length = math.sqrt(sx * sx + sy * sy);
    return length > 1 ? Vec2(sx / length, sy / length) : Vec2(sx, sy);
  }

  /// Sets the two-way control [name] from outside the keys, such as an
  /// on-screen stick drawn by Flutter, until it's set again: back to
  /// [Vec2.zero] when the stick is let go. A longer [value] is cut to
  /// length 1. [axis2] adds it to what its keys say. An axis that isn't
  /// bound is made, with no keys. Takes effect at once.
  void setAxis2(String name, Vec2 value) {
    final axis = _axes[name] ??= _Axis2(const [], const [], const [], const []);
    final length = value.length;
    axis.value = length == 0 ? Vec2.zero : (length > 1 ? value * (1 / length) : value);
  }

  /// Binds the two-way control [name] to keys or buttons for each way,
  /// replacing what it had.
  void bindAxis2(
    String name, {
    required List<InputButton> up,
    required List<InputButton> down,
    required List<InputButton> left,
    required List<InputButton> right,
  }) {
    final kept = _axes[name]?.value ?? Vec2.zero;
    _axes[name] = _Axis2(List.of(up), List.of(down), List.of(left), List.of(right))..value = kept;
    _useAll(up);
    _useAll(down);
    _useAll(left);
    _useAll(right);
  }

  /// The named button [name], held by any of the keys and mouse buttons
  /// bound to it: `input.action('jump').wasPressed`. `'jump'` is the space
  /// bar; [bindAction] adds others. Throws an [ArgumentError] for a name
  /// that isn't bound.
  InputAction action(String name) =>
      _actions[name] ?? (throw ArgumentError.value(name, 'name', 'is no action (bound: ${_actions.keys})'));

  /// Binds the action [name] to [buttons], replacing what it had, and
  /// returns it. With no buttons, it's pressed only from outside
  /// ([InputAction.press]).
  InputAction bindAction(String name, List<InputButton> buttons) {
    _useAll(buttons);
    return _actions[name] = InputAction._(this, name, List.unmodifiable(buttons));
  }

  bool _anyDown(List<InputButton> buttons) {
    for (var i = 0; i < buttons.length; i++) {
      if (isPressed(buttons[i])) return true;
    }
    return false;
  }

  void _bindDefaults() {
    bindAxis2(
      'move',
      up: const [KeyCode.w, KeyCode.arrowUp],
      down: const [KeyCode.s, KeyCode.arrowDown],
      left: const [KeyCode.a, KeyCode.arrowLeft],
      right: const [KeyCode.d, KeyCode.arrowRight],
    );
    bindAction('jump', const [KeyCode.space]);
  }

  // --- The input step ---

  /// Runs the input step of an input of its own, [dt] unscaled seconds
  /// after the last: its events, then its gestures. A world's input runs
  /// in [World.update], so this throws a [StateError] for one.
  void update(double dt) {
    if (_targets != null) throw StateError('this Input is a world\'s, and runs in World.update');
    _now += dt;
    _step();
  }

  /// The input step, at [_now]. Takes no double, so a frame boxes none.
  void _step() {
    _frame++;
    if (_pressedKeys.isNotEmpty) _pressedKeys.clear();
    if (_releasedKeys.isNotEmpty) _releasedKeys.clear();
    _pressedButtons = 0;
    _releasedButtons = 0;
    _scroll = Vec2.zero;
    if (_paused) {
      if (_pausing) {
        _pausing = false;
        _letGo();
      }
      if (_queue.isNotEmpty) _queue.clear();
      _scrollX = _scrollY = 0;
      _padScale = 1;
      _padTwist = 0;
      return;
    }
    // Events a handler feeds meanwhile wait for the next frame.
    final n = _queue.length;
    try {
      for (var i = 0; i < n; i++) {
        _handle(_queue[i]);
      }
    } finally {
      if (n > 0) _queue.removeRange(0, n);
    }
    _gestures();
  }

  @pragma('vm:prefer-inline')
  void _emit<T>(Signal<T> signal, T value) {
    try {
      signal.emit(value);
    } catch (error, stack) {
      final targets = _targets;
      if (targets == null) rethrow;
      targets.report(error, stack);
    }
  }

  Finger? _finger(int id) {
    for (var i = 0; i < _fingers.length; i++) {
      if (_fingers[i].id == id) return _fingers[i];
    }
    return null;
  }

  void _handle(_Raw raw) {
    switch (raw.kind) {
      case _RawKind.down:
        _down(raw);
      case _RawKind.move:
        final finger = _finger(raw.id);
        if (finger == null || !finger._down) return;
        if (raw.pressure case final p?) finger._pressure = p;
        if (raw.buttons case final b? when finger.kind == FingerKind.mouse) {
          finger._buttons = b;
          _setButtons();
        }
        _move(finger, raw.x, raw.y);
      case _RawKind.up:
        final finger = _finger(raw.id);
        if (finger == null || !finger._down) return;
        _move(finger, raw.x, raw.y);
        _up(finger, cancelled: false);
      case _RawKind.cancel:
        final finger = _finger(raw.id);
        if (finger == null || !finger._down) return;
        _up(finger, cancelled: true);
      case _RawKind.hover:
        var finger = _finger(raw.id);
        if (finger == null) {
          finger = Finger._(this, raw.id, raw.fingerKind);
          finger._place(raw.x, raw.y);
          finger._lastX = raw.x;
          finger._lastY = raw.y;
          _fingers.add(finger);
          _targets?.moved(finger);
          return;
        }
        if (finger._down) return;
        _move(finger, raw.x, raw.y);
      case _RawKind.leave:
        final finger = _finger(raw.id);
        if (finger == null || finger._down) return;
        _flush(finger);
        _fingers.remove(finger);
        finger._gone = true;
        _targets?.left(finger);
      case _RawKind.scroll:
        _scrollX += raw.x;
        _scrollY += raw.y;
      case _RawKind.pad:
        _padScale *= raw.x;
        _padTwist += raw.y;
      case _RawKind.keyDown:
        final key = raw.key!;
        if (!_keys.add(key)) return;
        _pressedKeys.add(key);
        _emit(onKeyDown, KeyPress._(key, _keys));
      case _RawKind.keyUp:
        _keyUp(raw.key!);
      case _RawKind.release:
        for (final key in [..._keys]) {
          _keyUp(key);
        }
      case _RawKind.actionDown:
        final action = raw.action!;
        if (action._held) return;
        action._held = true;
        action._pressedAt = _frame;
      case _RawKind.actionUp:
        final action = raw.action!;
        if (!action._held) return;
        action._held = false;
        action._releasedAt = _frame;
    }
  }

  /// The pause starts: every key, finger and action held goes up, the
  /// fingers as cancelled.
  void _letGo() {
    for (final key in [..._keys]) {
      _keyUp(key);
    }
    for (final finger in [..._fingers]) {
      if (finger._down) _up(finger, cancelled: true);
    }
    for (final action in _actions.values) {
      if (action._held) {
        action._held = false;
        action._releasedAt = _frame;
      }
    }
  }

  void _keyUp(KeyCode key) {
    if (!_keys.remove(key)) return;
    _releasedKeys.add(key);
    _emit(onKeyUp, KeyPress._(key, _keys));
  }

  void _down(_Raw raw) {
    var finger = _finger(raw.id);
    if (finger != null && finger._down) {
      // A down for a finger that's down: its up never came.
      _up(finger, cancelled: true);
      finger = _finger(raw.id);
    }
    if (finger == null) {
      finger = Finger._(this, raw.id, raw.fingerKind);
      _fingers.add(finger);
    } else {
      // The hovering mouse, pressed: it's last now, as the latest down.
      _flush(finger);
      _fingers
        ..remove(finger)
        ..add(finger);
    }
    finger._press(raw.x, raw.y, raw.buttons ?? 1, raw.pressure ?? 1);
    _setButtons();
    _emit(onFingerDown, finger);
    // What it went down on is entered first, as in Unity.
    _targets?.moved(finger);
    _targets?.down(finger);
  }

  void _move(Finger finger, double x, double y) {
    if (x == finger._x && y == finger._y) return;
    finger._place(x, y);
    finger._moved = true;
    if (!finger._down || finger._dragging) return;
    final dx = x - finger._startX, dy = y - finger._startY;
    if (dx * dx + dy * dy > finger._slop * finger._slop) {
      finger._dragging = true;
      finger._tapping = false;
      _emit(onDragStart, finger);
      _targets?.dragStart(finger);
    }
  }

  /// Tells this frame's move of [finger], if it hasn't been told.
  void _flush(Finger finger) {
    if (!finger._moved) return;
    finger._moved = false;
    if (!finger._down) {
      _emit(onHover, finger);
      _targets?.moved(finger);
      return;
    }
    _emit(onFingerMove, finger);
    _targets?.moved(finger);
    if (finger._dragging) {
      _emit(onDrag, finger);
      _targets?.drag(finger);
    }
  }

  void _up(Finger finger, {required bool cancelled}) {
    _flush(finger);
    finger._sample();
    finger._down = false;
    finger._upAt = _now;
    finger._cancelled = cancelled;
    if (cancelled || _now - finger._downAt > settings.tapTime) finger._tapping = false;
    final mouse = finger.kind == FingerKind.mouse;
    if (!mouse) {
      _fingers.remove(finger);
      finger._gone = true;
    }
    finger._buttons = 0;
    _setButtons();

    if (finger._tapping) {
      final dx = finger._startX - _lastTapX, dy = finger._startY - _lastTapY;
      final again =
          finger._downAt - _lastTapAt <= settings.doubleTapTime &&
          dx * dx + dy * dy <= settings.doubleTapSlop * settings.doubleTapSlop &&
          finger.kind == _lastTapKind &&
          finger.button == _lastTapButton;
      finger._tapCount = again ? _lastTapCount + 1 : 1;
      _lastTapAt = _now;
      _lastTapX = finger._x;
      _lastTapY = finger._y;
      _lastTapCount = finger._tapCount;
      _lastTapKind = finger.kind;
      _lastTapButton = finger.button;
      _emit(onTap, finger);
      _targets?.click(finger);
    }
    if (finger._dragging) {
      _emit(onDragEnd, finger);
      _targets?.dragEnd(finger);
      if (!cancelled) {
        final velocity = finger.velocity;
        final speed = velocity.length;
        if (speed >= settings.swipeSpeed) {
          _emit(onSwipe, Swipe._(finger, velocity * (1 / speed), speed, finger.totalDelta.length));
        }
      }
    }
    _emit(onFingerUp, finger);
    _targets?.up(finger);
    if (!mouse) _targets?.left(finger);
    finger._dragging = false;
  }

  /// Works out which mouse buttons are held, and which changed.
  void _setButtons() {
    var now = 0;
    for (var i = 0; i < _fingers.length; i++) {
      final f = _fingers[i];
      if (!f._down) continue;
      now |= f.kind == FingerKind.mouse ? f._buttons : MouseButton.left.bit;
    }
    _pressedButtons |= now & ~_buttons;
    _releasedButtons |= _buttons & ~now;
    _buttons = now;
  }

  /// After the events: each finger's move, long press and hold, then the
  /// pinch and twist, and the scroll.
  void _gestures() {
    for (var i = 0; i < _fingers.length; i++) {
      final finger = _fingers[i];
      _flush(finger);
      if (!finger._down) continue;
      final age = _now - finger._downAt;
      if (finger._tapping && !finger._longPressed && age >= settings.longPressTime) {
        finger._longPressed = true;
        finger._tapping = false;
        _emit(onLongPress, finger);
        _targets?.longPress(finger);
      }
      if (!finger._down) continue;
      if (finger._tapping && age > settings.tapTime) finger._tapping = false;
      _emit(onFingerHeld, finger);
    }

    _pinch();

    if (_scrollX != 0 || _scrollY != 0) {
      _scroll = Vec2(_scrollX, -_scrollY);
      _scrollX = 0;
      _scrollY = 0;
      _emit(onScroll, _scroll);
    }

    for (var i = 0; i < _fingers.length; i++) {
      final finger = _fingers[i];
      finger._lastX = finger._x;
      finger._lastY = finger._y;
      if (finger._down) finger._sample();
    }
  }

  /// The pinch and twist of the touches and pens down since before this
  /// frame, and the trackpad's.
  @pragma('vm:never-inline')
  void _pinch() {
    // Most frames have nothing to pinch: they return before holding a
    // double, which the JIT may box (it did, once a frame, on CI's runner).
    if (_padScale == 1 && _padTwist == 0 && !_pinching()) return;
    var n = 0;
    var moved = false;
    double cx = 0, cy = 0, px = 0, py = 0;
    for (var i = 0; i < _fingers.length; i++) {
      final f = _fingers[i];
      if (!_pinches(f)) continue;
      n++;
      cx += f._x;
      cy += f._y;
      px += f._lastX;
      py += f._lastY;
      if (f._x != f._lastX || f._y != f._lastY) moved = true;
    }
    var scale = _padScale, twist = _padTwist;
    _padScale = 1;
    _padTwist = 0;
    ViewPoint? centre;
    if (n >= 2 && moved) {
      cx /= n;
      cy /= n;
      px /= n;
      py /= n;
      double now = 0, before = 0, turned = 0;
      var turning = 0;
      for (var i = 0; i < _fingers.length; i++) {
        final f = _fingers[i];
        if (!_pinches(f)) continue;
        final ax = f._x - cx, ay = f._y - cy, bx = f._lastX - px, by = f._lastY - py;
        final a = math.sqrt(ax * ax + ay * ay), b = math.sqrt(bx * bx + by * by);
        now += a;
        before += b;
        if (a > 0 && b > 0) {
          var angle = math.atan2(ay, ax) - math.atan2(by, bx);
          if (angle > math.pi) angle -= 2 * math.pi;
          if (angle < -math.pi) angle += 2 * math.pi;
          turned += angle;
          turning++;
        }
      }
      if (before > 0 && now > 0) scale *= now / before;
      if (turning > 0) twist += turned / turning;
      centre = _point(cx, cy);
    }
    if (scale == 1 && twist == 0) return;
    final pinch = Pinch._(
      scale,
      twist,
      centre ?? mousePosition ?? _point(_width / 2, _height / 2),
      n >= 2 && moved ? n : 0,
    );
    if (scale != 1) _emit(onPinch, pinch);
    if (twist != 0) _emit(onTwist, pinch);
  }

  bool _pinches(Finger f) => f._down && f.kind != FingerKind.mouse && f._downFrame < _frame;

  /// Whether two or more fingers [_pinches]: only then can they pinch.
  bool _pinching() {
    var n = 0;
    for (var i = 0; i < _fingers.length; i++) {
      if (_pinches(_fingers[i]) && ++n >= 2) return true;
    }
    return false;
  }

  ViewPoint _point(double x, double y) => ViewPoint(x, y, width: _width, height: _height);

  @override
  String toString() => 'Input(${_width}x$_height, fingers: $_fingers, keys: $_keys)';
}

/// The numbers [Input]'s gestures go by.
final class InputSettings {
  /// How far, in logical pixels, a touch or pen may move and still tap;
  /// further, it drags. Flutter's `kTouchSlop`.
  double tapSlop = 18;

  /// The same for the mouse. Flutter's `kPrecisePointerHitSlop`.
  double mouseTapSlop = 1;

  /// How long, in seconds, a finger may be down and still tap.
  double tapTime = 0.5;

  /// The most seconds from one tap's up to the next one's down for a
  /// double tap. Flutter's `kDoubleTapTimeout`.
  double doubleTapTime = 0.3;

  /// How far apart, in logical pixels, a double tap's taps may be.
  /// Flutter's `kDoubleTapSlop`.
  double doubleTapSlop = 100;

  /// How long, in seconds, a finger held still takes to long press.
  /// Flutter's `kLongPressTimeout`.
  double longPressTime = 0.5;

  /// The least speed, in logical pixels a second, a drag lets go at to
  /// swipe.
  double swipeSpeed = 800;

  /// Over how many seconds before now a finger's [Finger.velocity] is
  /// measured.
  double velocityTime = 0.1;
}

/// What a [Finger] is.
enum FingerKind {
  /// A finger on a touch screen.
  touch,

  /// The mouse, or a trackpad's pointer.
  mouse,

  /// A stylus.
  pen,
}

/// A key or a mouse button: what [Input.isPressed] and the others take.
sealed class InputButton {}

/// A mouse button. A touch counts as [left] in [Input.isPressed].
enum MouseButton implements InputButton {
  /// The main button; a touch or a pen counts as it.
  left(1),

  /// The second button: a right click.
  right(2),

  /// The wheel's button.
  middle(4);

  const MouseButton(this.bit);

  /// Flutter's bit for it (`kPrimaryButton`, `kSecondaryButton`,
  /// `kMiddleMouseButton`).
  final int bit;
}

/// One touch, the mouse or a pen: from down to up, as Lean Touch's
/// `LeanFinger`. The mouse is one finger for as long as it's over the
/// view, pressed or not ([isPressed]); each press starts it over.
final class Finger {
  Finger._(this._input, this.id, this.kind);

  final Input _input;

  /// The pointer's id: one per touch, and one that stays for the mouse.
  final int id;

  /// A touch, the mouse or a pen.
  final FingerKind kind;

  double _x = 0, _y = 0;
  double _startX = 0, _startY = 0;
  double _lastX = 0, _lastY = 0;
  double _pressure = 1;
  double _downAt = 0;
  double? _upAt;
  int _downFrame = 0;
  int _buttons = 0;
  int _buttonsDown = 0;
  int _tapCount = 0;
  bool _down = false;
  bool _moved = false;
  bool _tapping = false;
  bool _dragging = false;
  bool _longPressed = false;
  bool _cancelled = false;
  bool _gone = false;

  // The last few (time, x, y), for [velocity].
  final _samples = Float64List(_sampleCount * 3);
  int _sampled = 0;
  static const _sampleCount = 8;

  // The world's: what it was under at the start and now, who took its
  // down and its drag, and the objects it's over.
  RayHit? _startedOver;
  RayHit? _over;
  int _overFrame = -1;
  int _overVersion = -1;
  int _version = 0;
  Object? _taker;
  Object? _captor;
  List<GameObject>? _chain;

  void _place(double x, double y) {
    _x = x;
    _y = y;
    _version++;
  }

  void _press(double x, double y, int buttons, double pressure) {
    final input = _input;
    _place(x, y);
    _startX = _lastX = x;
    _startY = _lastY = y;
    _downAt = input._now;
    _upAt = null;
    _downFrame = input._frame;
    _buttons = kind == FingerKind.mouse ? buttons : MouseButton.left.bit;
    _buttonsDown = _buttons;
    _pressure = pressure;
    _tapCount = 0;
    _down = true;
    _moved = false;
    _tapping = true;
    _dragging = false;
    _longPressed = false;
    _cancelled = false;
    _sampled = 0;
    _startedOver = null;
    _taker = null;
    _captor = null;
    _sample();
  }

  void _sample() {
    final at = _input._now;
    // A second sample in the same frame replaces the first.
    final last = (_sampled - 1) % _sampleCount;
    final i = _sampled > 0 && _samples[last * 3] == at ? last : _sampled++ % _sampleCount;
    _samples
      ..[i * 3] = at
      ..[i * 3 + 1] = _x
      ..[i * 3 + 2] = _y;
  }

  /// The button that pressed it: the mouse's first, [MouseButton.left] for
  /// a touch or a pen.
  MouseButton get button {
    final b = _buttonsDown;
    if (kind != FingerKind.mouse || b & MouseButton.left.bit != 0) return MouseButton.left;
    if (b & MouseButton.right.bit != 0) return MouseButton.right;
    if (b & MouseButton.middle.bit != 0) return MouseButton.middle;
    return MouseButton.left;
  }

  /// How hard it presses, 0 to 1 (1 for what can't tell).
  double get pressure => _pressure;

  /// Where it is.
  ViewPoint get position => _input._point(_x, _y);

  /// Where it went down.
  ViewPoint get startPosition => _input._point(_startX, _startY);

  /// How far it moved since the last frame.
  Vec2 get delta => Vec2(_x - _lastX, _y - _lastY);

  /// How far it is from where it went down.
  Vec2 get totalDelta => Vec2(_x - _startX, _y - _startY);

  /// How fast it moves, in logical pixels a second, over the last
  /// [InputSettings.velocityTime].
  Vec2 get velocity {
    final n = _sampled < _sampleCount ? _sampled : _sampleCount;
    if (n < 2) return Vec2.zero;
    final newest = (_sampled - 1) % _sampleCount;
    final t = _samples[newest * 3];
    var from = -1;
    for (var k = 1; k < n; k++) {
      final i = (_sampled - 1 - k) % _sampleCount;
      from = i;
      if (_samples[i * 3] < t - _input.settings.velocityTime) break;
    }
    final dt = t - _samples[from * 3];
    if (dt <= 0) return Vec2.zero;
    return Vec2(
      (_samples[newest * 3 + 1] - _samples[from * 3 + 1]) / dt,
      (_samples[newest * 3 + 2] - _samples[from * 3 + 2]) / dt,
    );
  }

  /// Seconds since it went down (until it went up).
  double get age => (_upAt ?? _input._now) - _downAt;

  /// For a tap: 1, or 2 for a double tap's second, and so on.
  int get tapCount => _tapCount;

  /// Whether it's down: a touch until it lifts, the mouse while a button
  /// is held.
  bool get isPressed => _down;

  /// While down: whether it may still tap (it's within the tap's slop and
  /// time, and wasn't long pressed). After up: whether it tapped.
  bool get isTap => _tapping;

  /// Whether it moved further than the tap's slop while down.
  bool get isDragging => _dragging;

  /// Whether it was long pressed.
  bool get isLongPress => _longPressed;

  /// Whether it went without an up of its own (the system or a widget
  /// took it).
  bool get wasCancelled => _cancelled;

  double get _slop => kind == FingerKind.mouse ? _input.settings.mouseTapSlop : _input.settings.tapSlop;

  /// What was under it when it went down: the object, the point and the
  /// normal, from the camera the view draws from. Null with no world.
  RayHit? get startedOver => _startedOver;

  /// What's under it now, cast when first asked in a frame (or after it
  /// moves). Null with no world, or over nothing.
  RayHit? get over {
    final targets = _input._targets;
    if (targets == null || _gone) return _over;
    if (_overFrame != _input._frame || _overVersion != _version) {
      _over = targets.hitTest(_x, _y);
      _overFrame = _input._frame;
      _overVersion = _version;
    }
    return _over;
  }

  @override
  String toString() =>
      'Finger($id ${kind.name} at ($_x, $_y)${_down ? ', down' : ''}${_dragging ? ', dragging' : ''}'
      '${_tapCount > 0 ? ', tap $_tapCount' : ''})';
}

/// A drag let go fast: [Input.onSwipe].
final class Swipe {
  Swipe._(this.finger, this.direction, this.speed, this.distance);

  /// The finger that swiped, now up.
  final Finger finger;

  /// Which way it was going as it let go, unit length (+y is down the
  /// view).
  final Vec2 direction;

  /// How fast, in logical pixels a second.
  final double speed;

  /// How far from where it went down, in logical pixels.
  final double distance;

  /// Which of the four ways [direction] is nearest.
  SwipeSide get side => direction.x.abs() >= direction.y.abs()
      ? (direction.x < 0 ? SwipeSide.left : SwipeSide.right)
      : (direction.y < 0 ? SwipeSide.up : SwipeSide.down);

  @override
  String toString() => 'Swipe(${side.name}, $speed px/s, $distance px)';
}

/// Which of the four ways a [Swipe] went: [Swipe.side].
enum SwipeSide {
  /// Towards the view's left.
  left,

  /// Towards the view's right.
  right,

  /// Towards the view's top.
  up,

  /// Towards the view's bottom.
  down,
}

/// A frame's pinch and twist: [Input.onPinch], [Input.onTwist].
final class Pinch {
  Pinch._(this.scale, this.twist, this.center, this.fingers);

  /// How much further apart the fingers are than last frame: above 1
  /// spreading, below 1 pinching in. Zoom by it.
  final double scale;

  /// How far they turned since last frame, in radians, clockwise on the
  /// view.
  final double twist;

  /// Between the fingers, or where the mouse is for a trackpad.
  final ViewPoint center;

  /// How many fingers; 0 for a trackpad.
  final int fingers;

  @override
  String toString() => 'Pinch(scale $scale, twist $twist, at (${center.x}, ${center.y}))';
}

/// A key going down or up, with the modifier keys held then.
final class KeyPress {
  KeyPress._(this.key, Set<KeyCode> held)
    : shift = held.contains(KeyCode.shiftLeft) || held.contains(KeyCode.shiftRight),
      control = held.contains(KeyCode.controlLeft) || held.contains(KeyCode.controlRight),
      alt = held.contains(KeyCode.altLeft) || held.contains(KeyCode.altRight),
      meta = held.contains(KeyCode.metaLeft) || held.contains(KeyCode.metaRight);

  /// The key that went down or up.
  final KeyCode key;

  /// Whether either shift key was held.
  final bool shift;

  /// Whether either control key was held.
  final bool control;

  /// Whether either alt key (option on a Mac) was held.
  final bool alt;

  /// Command on a Mac, the Windows key elsewhere.
  final bool meta;

  @override
  String toString() =>
      'KeyPress(${[if (control) 'control', if (alt) 'alt', if (shift) 'shift', if (meta) 'meta', key.name].join('+')})';
}

/// A named button ([Input.action]), held by any of its [buttons], or from
/// outside ([press]), such as by a Flutter button.
final class InputAction {
  InputAction._(this._input, this.name, this.buttons);

  final Input _input;

  /// The name it was made with, as [Input.action] takes it.
  final String name;

  /// The keys and mouse buttons that hold it; any one of them does.
  final List<InputButton> buttons;

  // Held from outside, and the input frames that pressed and let go of it.
  bool _held = false;
  int _pressedAt = -1, _releasedAt = -1;

  /// Holds it down from outside the keys, as a Flutter button does when
  /// pressed, until [release]: from the next frame's input step, as a
  /// key's press is. Pressing and releasing between two frames still
  /// makes one [wasPressed] and [wasReleased]. Does nothing while the
  /// world is paused.
  void press() => _input._queue.add(_Raw(_RawKind.actionDown, 0, 0, 0, action: this));

  /// Lets go of what [press] held.
  void release() => _input._queue.add(_Raw(_RawKind.actionUp, 0, 0, 0, action: this));

  /// Whether it's held now: by any of its [buttons], or by [press].
  bool get isPressed => _held || _input._anyDown(buttons);

  /// Whether it went down this frame: one of its buttons did, or [press],
  /// and nothing already held it.
  bool get wasPressed {
    var pressed = _pressedAt == _input._frame;
    if (_held && !pressed) return false;
    for (var i = 0; i < buttons.length; i++) {
      final b = buttons[i];
      if (_input.wasPressed(b)) {
        pressed = true;
      } else if (_input.isPressed(b)) {
        return false;
      }
    }
    return pressed;
  }

  /// Whether it went up this frame: one of its buttons did, or [release],
  /// and nothing holds it.
  bool get wasReleased {
    if (_held) return false;
    var released = _releasedAt == _input._frame;
    for (var i = 0; i < buttons.length; i++) {
      final b = buttons[i];
      if (_input.isPressed(b)) return false;
      if (_input.wasReleased(b)) released = true;
    }
    return released;
  }

  @override
  String toString() => 'InputAction($name: $buttons)';
}

final class _Axis2 {
  _Axis2(this.up, this.down, this.left, this.right);
  final List<InputButton> up, down, left, right;

  // What [Input.setAxis2] set.
  Vec2 value = Vec2.zero;
}

enum _RawKind { down, move, up, cancel, hover, leave, scroll, pad, keyDown, keyUp, release, actionDown, actionUp }

/// A queued event.
final class _Raw {
  _Raw(
    this.kind,
    this.id,
    this.x,
    this.y, {
    this.fingerKind = FingerKind.touch,
    this.buttons,
    this.pressure,
    this.key,
    this.action,
  });

  final _RawKind kind;
  final int id;
  final double x, y;
  final FingerKind fingerKind;
  final int? buttons;
  final double? pressure;
  final KeyCode? key;
  final InputAction? action;
}

/// What feeds an [Input] ([Input.simulate]): package:fynkel_flutter's view
/// gives it Flutter's events, and a test its own, as a player would:
/// `world.input.simulate.fingerDown(1, 50, 50)`. Each is queued, and the
/// next frame's input step handles them in order.
///
/// Positions are in the view's logical pixels from its top-left corner
/// ([resize] gives its size); times are the world's unscaled frame times,
/// so a test's `world.update(0.6)` holds a finger down for 0.6 seconds.
final class InputSimulation {
  InputSimulation._(this._input);

  final Input _input;

  /// The view is [width] by [height] logical pixels. Takes effect at once.
  void resize(double width, double height) {
    _input._width = width;
    _input._height = height;
  }

  /// Finger [id] went down at ([x], [y]). For the mouse, [buttons] are
  /// Flutter's bits (`kPrimaryButton` and the rest); a mouse that's
  /// hovering keeps its id.
  void fingerDown(
    int id,
    double x,
    double y, {
    FingerKind kind = FingerKind.touch,
    int buttons = 1,
    double pressure = 1,
  }) => _input._queue.add(_Raw(_RawKind.down, id, x, y, fingerKind: kind, buttons: buttons, pressure: pressure));

  /// Finger [id], down, moved to ([x], [y]). A mouse's [buttons] may change
  /// while it's down.
  void fingerMove(int id, double x, double y, {int? buttons, double? pressure}) =>
      _input._queue.add(_Raw(_RawKind.move, id, x, y, buttons: buttons, pressure: pressure));

  /// Finger [id] went up at ([x], [y]).
  void fingerUp(int id, double x, double y) => _input._queue.add(_Raw(_RawKind.up, id, x, y));

  /// Finger [id] was taken away (by the system, or a widget over the
  /// view): it goes without tapping or swiping.
  void fingerCancel(int id) => _input._queue.add(_Raw(_RawKind.cancel, id, 0, 0));

  /// The mouse (or a pen) [id] moved over the view with nothing pressed.
  void hover(int id, double x, double y, {FingerKind kind = FingerKind.mouse}) =>
      _input._queue.add(_Raw(_RawKind.hover, id, x, y, fingerKind: kind));

  /// The mouse (or a pen) [id] left the view.
  void leave(int id) => _input._queue.add(_Raw(_RawKind.leave, id, 0, 0));

  /// A mouse wheel or a trackpad scrolled by ([dx], [dy]), as Flutter's
  /// `scrollDelta` (+y for the wheel turned towards you): [Input.scroll]
  /// says it the other way up, as Unity's.
  void scrollBy(double dx, double dy) => _input._queue.add(_Raw(_RawKind.scroll, 0, dx, dy));

  /// A trackpad pinched by [scale] (2 is twice as far apart) and turned by
  /// [twist] radians since its last event.
  void trackpad({double scale = 1, double twist = 0}) => _input._queue.add(_Raw(_RawKind.pad, 0, scale, twist));

  /// [key] went down. Returns whether the view should claim it from
  /// Flutter: the game uses it ([Input.usesKey]) and isn't paused.
  bool keyDown(KeyCode key) {
    _input._queue.add(_Raw(_RawKind.keyDown, 0, 0, 0, key: key));
    return claims(key);
  }

  /// [key] went up. Returns whether the view should claim it, as [keyDown].
  bool keyUp(KeyCode key) {
    _input._queue.add(_Raw(_RawKind.keyUp, 0, 0, 0, key: key));
    return claims(key);
  }

  /// Whether the view should claim [key] from Flutter now, as [keyDown]
  /// returns: for a key held down and repeating.
  bool claims(KeyCode key) => !_input._paused && _input.usesKey(key);

  /// The view lost focus: its keys won't come up here, so each held one
  /// goes up.
  void releaseKeys() => _input._queue.add(_Raw(_RawKind.release, 0, 0, 0));
}

/// What the input step reaches through the world: what's under a finger,
/// and the objects' handlers (pointing.dart).
abstract interface class _Targets {
  RayHit? hitTest(double x, double y);
  void down(Finger finger);
  void up(Finger finger);
  void click(Finger finger);
  void longPress(Finger finger);
  void dragStart(Finger finger);
  void drag(Finger finger);
  void dragEnd(Finger finger);

  /// It moved, or appeared: what it's over may have changed.
  void moved(Finger finger);

  /// It's gone: it's over nothing now.
  void left(Finger finger);

  void report(Object error, StackTrace stack);
}
