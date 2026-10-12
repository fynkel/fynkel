// Step 5: input, on its own. Fingers, gestures and keys as signals and
// polled state: a dial to pinch and twist, a puck driven by the 'move'
// axis and the 'jump' action, and a log of every gesture.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class InputScene extends LabScene {
  @override
  String get title => 'Input: fingers and keys';
  @override
  String get step => 'step 5';
  @override
  String get about =>
      'Tap, double tap, long press, drag and swipe anywhere: the log says what input saw. Pinch and twist with '
      'two fingers (or a trackpad) to size and turn the dial; scroll sizes it too. WASD or the arrows move the '
      'puck (input.axis2), space makes it hop (input.action). The readout shows every finger, the mouse too.';

  late final GameObject _dial;
  double _size = 1, _turn = 0;
  var _logMoves = false;

  @override
  void build() {
    _dial = add(shape('dial', Mesh.cube(), const Color(0xFF4CA6E0), at: Vec3(0, 0.6, 0), size: Vec3(1.2, 1.2, 1.2)));
    _dial.add(shape('notch', Mesh.cube(), const Color(0xFFFFD040), at: Vec3(0, 0, 0.55), size: Vec3(0.2, 0.6, 0.1)));
    add(Puck());
  }

  @override
  void onStart() {
    super.onStart();
    on(input.onTap, (f) => log.add('tap ×${f.tapCount} (${f.kind.name}, ${f.button.name}) at ${_at(f.position)}'));
    on(input.onLongPress, (f) => log.add('long press at ${_at(f.position)}'));
    on(input.onDragStart, (f) => log.add('drag start from ${_at(f.startPosition)}'));
    on(input.onDragEnd, (f) => log.add('drag end: ${_vec(f.totalDelta)} in ${secs(f.age)}'));
    on(input.onSwipe, (s) => log.add('swipe ${s.side.name} at ${s.speed.round()} px/s over ${s.distance.round()} px'));
    on(input.onKeyDown, (k) => log.add('key down $k'));
    on(input.onKeyUp, (k) => log.add('key up ${k.key.name}'));
    on(input.onFingerMove, (f) {
      if (_logMoves) log.add('move ${f.id}: ${_vec(f.delta)}');
    });
    on(input.onHover, (f) {
      if (_logMoves) log.add('hover at ${_at(f.position)}');
    });
    on(input.onPinch, (p) => _size = (_size * p.scale).clamp(0.3, 3.0));
    on(input.onTwist, (p) => _turn += p.twist);
    on(input.onScroll, (s) => _size = (_size * math.exp(s.y * 0.003)).clamp(0.3, 3.0));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _dial.transform = Transform(
      position: Vec3(0, 0.6 * _size, 0),
      // Clockwise on the view, seen from above.
      rotation: Quat.axisAngle(Vec3.up, -_turn),
      scale: Vec3(1.2, 1.2, 1.2) * _size,
    );
    final fingers = input.fingers;
    readout.value = [
      if (fingers.isEmpty) 'no fingers',
      for (final f in fingers)
        '${f.kind.name} ${f.id}${f.isPressed ? ' down ${secs(f.age)}' : ''} at ${_at(f.position)}'
            '${f.isDragging ? ', dragging ${f.velocity.length.round()} px/s' : ''}',
      'move ${_vec(input.axis2('move'))}   jump ${input.action('jump').isPressed ? 'held' : '–'}   '
          'dial ×${_size.toStringAsFixed(2)}, ${(_turn * 180 / math.pi).round()}°',
    ].join('\n');
  }

  static String _at(ViewPoint p) => '(${p.x.round()}, ${p.y.round()})';
  static String _vec(Vec2 v) => '(${v.x.toStringAsFixed(1)}, ${v.y.toStringAsFixed(1)})';

  @override
  List<Control> get controls => [
    Toggle('log moves and hovers', () => _logMoves, (on) => _logMoves = on),
    Slider(
      'long press time',
      () => input.settings.longPressTime,
      (v) => input.settings.longPressTime = v,
      min: 0.2,
      max: 1.5,
    ),
    Slider('swipe speed', () => input.settings.swipeSpeed, (v) => input.settings.swipeSpeed = v, min: 300, max: 2000),
    Button('reset the dial', () {
      _size = 1;
      _turn = 0;
    }),
    Button('clear log', log.clear),
  ];
}

/// Moved by the 'move' axis, and hops on 'jump'.
class Puck extends GameObject {
  Puck()
    : super(
        name: 'puck',
        transform: Transform(position: Vec3(-3, 0.3, 1.5)),
      );

  var _x = -3.0, _z = 1.5;
  double _hop = 0;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFE0613A))));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final move = input.axis2('move');
    // Up on the axis is away from the camera: -Z.
    _x = (_x + move.x * 4 * time.delta).clamp(-6.0, 6.0);
    _z = (_z - move.y * 4 * time.delta).clamp(-4.0, 4.0);
    if (input.action('jump').wasPressed && _hop == 0) {
      _hop = 0.6;
      ancestor<LabScene>()?.log.add('jump: action(\'jump\').wasPressed');
    }
    _hop = math.max(0, _hop - time.delta);
    final height = 0.3 + 6 * _hop * (0.6 - _hop);
    transform = Transform(position: Vec3(_x, height, _z), scale: Vec3(0.6, 0.6, 0.6));
  }
}
