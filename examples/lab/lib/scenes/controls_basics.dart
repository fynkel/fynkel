// Stage 6: Flutter controls and pause, the basics. A cube the 'move' axis
// drives and the 'jump' action makes hop, from the keys or from the page's
// Flutter controls: two sliders are a stick (input.setAxis2), a button
// presses the action. A spinner shows the game's time; the pause switch
// stops it, and the game's input with it (world.paused).

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class ControlsBasicsScene extends LabScene {
  @override
  String get title => 'Input: Flutter controls and pause';
  @override
  String get step => 'stage 6';
  @override
  String get about =>
      'WASD or the arrows move the blue cube, space makes it hop. The two sliders are a stick for the same '
      "'move' axis (input.setAxis2), and the jump button presses the same 'jump' action (press, release). "
      'The pause switch sets world.paused: the spinner and the cube stop, and keys do nothing until it\'s off.';

  late final GameObject cube, spinner;
  double stickX = 0, stickY = 0;
  double _rise = 0;

  @override
  void build() {
    cube = add(shape('cube', Mesh.cube(), const Color(0xFF4CA6E0), at: Vec3(0, 0.5, 0)));
    spinner = add(shape('spinner', Mesh.cube(), const Color(0xFFFFD040), at: Vec3(3, 1, -2), size: Vec3(1, 0.2, 0.2)));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final dt = time.delta;
    spinner.rotate(Vec3.up, 3 * dt);
    final move = input.axis2('move');
    cube.position += Vec3(move.x, 0, -move.y) * (4 * dt);
    if (input.action('jump').wasPressed && cube.position.y <= 0.5) {
      _rise = 5;
      log.add('jump');
    }
    _rise -= 9.8 * dt;
    final y = cube.position.y + _rise * dt;
    cube.position = Vec3(cube.position.x, y < 0.5 ? 0.5 : y, cube.position.z);
    if (y < 0.5) _rise = 0;
    readout.value =
        'move ${move.x.toStringAsFixed(2)}, ${move.y.toStringAsFixed(2)}   '
        'game time ${secs(time.sinceStart)}   ${world.paused ? 'paused' : ''}';
  }

  void _stick() => input.setAxis2('move', Vec2(stickX, stickY));

  @override
  List<Control> get controls => [
    Slider('stick x', () => stickX, (v) {
      stickX = v;
      _stick();
    }, min: -1),
    Slider('stick y', () => stickY, (v) {
      stickY = v;
      _stick();
    }, min: -1),
    Button('jump', () {
      input.action('jump')
        ..press()
        ..release();
    }),
    Toggle('pause', () => world.paused, (on) {
      world.paused = on;
      log.add(on ? 'world.paused: time and input stop' : 'resumed: this frame counts no time');
    }),
  ];
}
