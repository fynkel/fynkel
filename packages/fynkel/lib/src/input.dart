// The input contract (blueprint §5.1, §19.2d item 7; docs/input.md): raw
// control events a host captures once per frame, the actions the project's
// `inputActions` bind them to, and the `Input` facade behaviours read.
//
// The binding table lives in the native world (compiled from the cooked
// project on load); this file is the Dart side of the wire — events in,
// action states out — plus the service a test drives by hand.

import 'dart:typed_data';

import '../generated/schema.g.dart' as gen;
import '../math.dart';
import 'context.dart';
import 'wire/buffers.dart';

/// The device a raw control belongs to (wire values 0..3).
enum InputDevice {
  keyboard,
  mouse,
  gamepad,
  touch;

  static InputDevice? parse(String s) {
    for (final d in values) {
      if (d.name == s) return d;
    }
    return null;
  }
}

/// What happened to a control (wire values 0..4).
enum InputPhase {
  /// A button went down (`x` 1).
  down,

  /// A button went up (`x` 0).
  up,

  /// An absolute axis value (`x`, or `x`/`y`): a stick, a trigger.
  axis,

  /// Relative movement (mouse delta, wheel): summed over the frame and,
  /// like an edge, consumed by exactly one tick.
  move,

  /// Every control released (focus lost, pause): the edges are seen by the
  /// next tick.
  reset;

  static InputPhase? parse(String s) {
    for (final p in values) {
      if (p.name == s) return p;
    }
    return null;
  }
}

/// What an action produces (wire values 0..2).
enum ActionKind {
  button,
  axis1d,
  axis2d;

  static ActionKind? parse(String s) {
    for (final k in values) {
      if (k.name == s) return k;
    }
    return null;
  }
}

/// One raw control event of a host frame, named as the binding grammar
/// names controls (`Space`, `W`, `left`, `delta`, `leftStick`,
/// `overlay/jump`).
class InputEvent {
  const InputEvent(this.device, this.control, this.phase, {this.x = 0, this.y = 0});

  const InputEvent.down(InputDevice device, String control) : this(device, control, InputPhase.down, x: 1);
  const InputEvent.up(InputDevice device, String control) : this(device, control, InputPhase.up);
  const InputEvent.axis(InputDevice device, String control, double x, [double y = 0])
      : this(device, control, InputPhase.axis, x: x, y: y);
  const InputEvent.move(InputDevice device, String control, double x, [double y = 0])
      : this(device, control, InputPhase.move, x: x, y: y);
  const InputEvent.reset() : this(InputDevice.keyboard, '', InputPhase.reset);

  final InputDevice device;
  final String control;
  final InputPhase phase;
  final double x, y;

  gen.InputEventRecord toRecord() =>
      gen.InputEventRecord(device: device.index, control: control, phase: phase.index, x: x, y: y);

  /// The recording form (`docs/input.md`): `{device, control, phase, x?, y?}`.
  Map<String, Object?> toJson() => {
        if (phase != InputPhase.reset) 'device': device.name,
        if (control.isNotEmpty) 'control': control,
        'phase': phase.name,
        if (x != 0) 'x': x,
        if (y != 0) 'y': y,
      };

  /// Throws [FormatException] for an unknown device or phase.
  factory InputEvent.fromJson(Map<String, Object?> json) {
    final phase = InputPhase.parse(json['phase'] as String? ?? '');
    if (phase == null) throw FormatException('`${json['phase']}` is not an input phase', json);
    if (phase == InputPhase.reset) return const InputEvent.reset();
    final device = InputDevice.parse(json['device'] as String? ?? '');
    if (device == null) throw FormatException('`${json['device']}` is not an input device', json);
    final control = json['control'] as String? ?? '';
    if (control.isEmpty) throw FormatException('an input event needs a control', json);
    final x = (json['x'] as num?)?.toDouble() ?? 0;
    final y = (json['y'] as num?)?.toDouble() ?? 0;
    return switch (phase) {
      InputPhase.down => InputEvent.down(device, control),
      InputPhase.up => InputEvent.up(device, control),
      _ => InputEvent(device, control, phase, x: x, y: y),
    };
  }

  @override
  bool operator ==(Object other) =>
      other is InputEvent &&
      other.device == device &&
      other.control == control &&
      other.phase == phase &&
      other.x == x &&
      other.y == y;
  @override
  int get hashCode => Object.hash(device, control, phase, x, y);
  @override
  String toString() => phase == InputPhase.reset ? 'InputEvent.reset' : 'InputEvent(${device.name}:$control ${phase.name} $x $y)';
}

/// One bound action as the current callback sees it. A live view the
/// world updates in place; copy the fields you keep.
class ActionState {
  ActionState(this.name, this.kind);

  final String name;
  final ActionKind kind;

  /// A button down / an axis away from zero.
  bool held = false;

  /// Went down (up) since the tick before this one — seen by exactly one
  /// tick; in `update`/`lateUpdate`, went down (up) during this frame.
  bool pressed = false;
  bool released = false;

  /// `axis1d` value; `axis2d` x; a button 1/0. A relative control
  /// (`mouse:delta`, `mouse:wheel`) is motion, scaled by its binding's
  /// `*<factor>`: in a tick it is the rate that covers the motion in that
  /// one tick (`x · dt` is the motion; a catch-up tick reads zero), in
  /// `update`/`lateUpdate` the frame's summed motion itself.
  double x = 0;
  double y = 0;

  Vec2 get vector => Vec2(x, y);

  @override
  String toString() => 'ActionState($name ${kind.name} held=$held pressed=$pressed released=$released $x $y)';
}

/// The actions of one world (or one test) as gameplay code reads them.
abstract interface class InputService {
  /// Down / away from zero now.
  bool held(String action);

  /// Went down since the last tick (or during this frame, in `update`).
  bool pressed(String action);
  bool released(String action);

  /// An `axis1d` action's value in `-1..1` (a `button`'s 1/0, an `axis2d`'s x).
  double axis(String action);

  /// An `axis2d` action's vector (`x` right, `y` up; inside the unit
  /// square for keys and sticks, unclamped for `mouse:delta` — see
  /// [ActionState.x]).
  Vec2 action2D(String action);

  /// The action's state, or null when the project does not declare it.
  ActionState? state(String action);

  /// Every action in declaration order.
  List<ActionState> get actions;
}

/// Static facade over the current context's [InputService].
abstract final class Input {
  static InputService get _s => GameContext.require('Input').input;

  static bool held(String action) => _s.held(action);
  static bool pressed(String action) => _s.pressed(action);
  static bool released(String action) => _s.released(action);
  static double axis(String action) => _s.axis(action);
  static Vec2 action2D(String action) => _s.action2D(action);
  static ActionState? state(String action) => _s.state(action);
  static List<ActionState> get actions => _s.actions;
}

/// Shared name → state lookup with the unknown-action policy: an action the
/// project does not declare reads as nothing (held false, zero), so a typo
/// is a silent no-op — `state()` tells the two apart.
abstract class ActionTable implements InputService {
  final List<ActionState> _states = [];
  final Map<String, ActionState> _byName = {};

  @override
  List<ActionState> get actions => List.unmodifiable(_states);

  @override
  ActionState? state(String action) => _byName[action];

  @override
  bool held(String action) => _byName[action]?.held ?? false;
  @override
  bool pressed(String action) => _byName[action]?.pressed ?? false;
  @override
  bool released(String action) => _byName[action]?.released ?? false;
  @override
  double axis(String action) => _byName[action]?.x ?? 0;
  @override
  Vec2 action2D(String action) {
    final s = _byName[action];
    return s == null ? Vec2.zero : Vec2(s.x, s.y);
  }

  void _reset(Iterable<ActionState> states) {
    _states
      ..clear()
      ..addAll(states);
    _byName
      ..clear()
      ..addEntries(_states.map((s) => MapEntry(s.name, s)));
  }
}

/// No input at all (a world before its project loaded, a context without
/// an input service): every read is false or zero.
class NoInput extends ActionTable {}

/// An input a test drives by hand: declare the actions, then press,
/// release and set axes between the ticks you run. Edges behave as in a
/// running game — [endTick] consumes them.
class TestInput extends ActionTable {
  TestInput({Iterable<(String, ActionKind)> actions = const []}) {
    _reset([for (final (name, kind) in actions) ActionState(name, kind)]);
  }

  /// Declares an action (idempotent).
  ActionState declare(String name, ActionKind kind) {
    final existing = _byName[name];
    if (existing != null) return existing;
    final s = ActionState(name, kind);
    _states.add(s);
    _byName[name] = s;
    return s;
  }

  ActionState _require(String name) =>
      _byName[name] ?? (throw StateError('TestInput does not declare the action `$name`'));

  /// A button (or axis) went down: held, and pressed until the next
  /// [endTick].
  void press(String action) {
    final s = _require(action);
    if (!s.held) s.pressed = true;
    s.held = true;
    if (s.kind == ActionKind.button) s.x = 1;
  }

  void release(String action) {
    final s = _require(action);
    if (s.held) s.released = true;
    s.held = false;
    s.x = 0;
    s.y = 0;
  }

  /// Sets an axis: non-zero is held, zero is not. Like the runtime, an
  /// axis action has no edges — only button actions are pressed/released.
  void setAxis(String action, double x, [double y = 0]) {
    final s = _require(action);
    s.held = x != 0 || y != 0;
    s.x = x;
    s.y = y;
  }

  /// A tick ran: its edges are consumed.
  void endTick() {
    for (final s in _states) {
      s.pressed = false;
      s.released = false;
    }
  }

  /// Everything up with its edges (a focus loss).
  void reset() {
    for (final s in _states) {
      release(s.name);
    }
  }
}

/// The actions of a native world, decoded from its input view in place.
/// [refresh] runs before a tick's callbacks (the edges that tick consumes)
/// and, with [frameEdges], before `update` (the frame's raw edges); the
/// names are decoded once per binding table ([invalidate] after a load),
/// the values every refresh, allocation-free.
class WorldInput extends ActionTable {
  WorldInput(this._view);

  final WireBuffer Function() _view;
  bool _stale = true;

  /// The binding table may have changed (a scene or project load).
  void invalidate() => _stale = true;

  /// Re-reads the world's view. With [frameEdges], `pressed`/`released`
  /// are the current frame's raw edges and a relative control's value the
  /// frame's summed motion (for `update`/`lateUpdate`); else the edges
  /// and the motion the next tick consumes.
  void refresh({bool frameEdges = false}) {
    final b = _view();
    final n = b.recordCount;
    final d = b.records;
    const size = gen.InputActionRecord.wireSize;
    if (_stale || n != _states.length) {
      _reset([
        for (final r in b.fixed(size, gen.InputActionRecord.decode)) ActionState(r.name, ActionKind.values[r.kind.clamp(0, 2)]),
      ]);
      _stale = false;
    }
    final pressedAt = frameEdges ? 15 : 13;
    final releasedAt = frameEdges ? 16 : 14;
    final xAt = frameEdges ? 28 : 20;
    final yAt = frameEdges ? 32 : 24;
    for (var i = 0; i < n; i++) {
      final o = i * size;
      final s = _states[i];
      s.pressed = d.getUint8(o + pressedAt) != 0;
      s.released = d.getUint8(o + releasedAt) != 0;
      s.x = d.getFloat32(o + xAt, Endian.little);
      s.y = d.getFloat32(o + yAt, Endian.little);
      // An axis is held while away from zero in the view being read.
      s.held = s.kind == ActionKind.button ? d.getUint8(o + 12) != 0 : (s.x != 0 || s.y != 0);
    }
  }
}
