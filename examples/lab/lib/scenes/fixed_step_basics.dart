// Stage 5 step 2: the fixed step, the basics. Two cubes turned in
// onFixedUpdate, one drawn where its last step left it, one drawn between
// its last two steps with time.fixedAlpha.

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class FixedStepBasicsScene extends LabScene {
  @override
  String get title => 'Fixed step: the basics';
  @override
  String get step => 'stage 5 step 2';
  @override
  String get about =>
      'Both cubes turn in onFixedUpdate, which runs 50 times a second whatever the frame rate (physics will run '
      'there). The orange one is drawn where its last step left it; the blue one between its last two steps, with '
      'time.fixedAlpha. Slide the steps down to 5 a second: the orange one jumps, the blue one stays smooth.';

  var _frames = 0;
  var _steps = 0;
  var _counted = 0.0;
  var _rate = '';

  @override
  void build() {
    add(shape('at its steps', Mesh.cube(), _orange, at: Vec3(-1.5, 0.6, 0), components: [StepTurn(smooth: false)]));
    add(shape('between them', Mesh.cube(), _blue, at: Vec3(1.5, 0.6, 0), components: [StepTurn(smooth: true)]));
  }

  static const _orange = Color(0xFFE0613A);
  static const _blue = Color(0xFF4CA6E0);

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    _steps++;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _frames++;
    _counted += time.unscaledDelta;
    if (_counted >= 1) {
      _rate = 'last second: $_frames frames, $_steps fixed steps';
      _frames = _steps = 0;
      _counted = 0;
    }
    readout.value = [
      'fixedDelta ${secs(time.fixedDelta)} (${(1 / time.fixedDelta).round()} steps a second)',
      'fixedAlpha ${time.fixedAlpha.toStringAsFixed(2)}',
      _rate,
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Slider(
      'steps a second',
      () => 1 / time.fixedDelta,
      (v) => time.fixedDelta = 1 / v.roundToDouble(),
      min: 5,
      max: 60,
    ),
    Button('50 a second', () => time.fixedDelta = 1 / 50),
    Button('pause', () => time.scale = 0),
    Button('play', () => time.scale = 1),
  ];
}

/// Turns its object by the same angle every fixed step.
class StepTurn extends Behaviour {
  StepTurn({required this.smooth});

  /// Whether it's drawn between its last two steps, or at the last.
  final bool smooth;

  static const _speed = 1.5; // radians a second
  var _before = 0.0;
  var _now = 0.0;

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    _before = _now;
    _now += _speed * time.delta; // time.delta is time.fixedDelta here
    if (!smooth) _draw(_now);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (smooth) _draw(_before + (_now - _before) * time.fixedAlpha);
  }

  void _draw(double angle) => gameObject.rotation = Quat.axisAngle(Vec3.up, angle);
}
