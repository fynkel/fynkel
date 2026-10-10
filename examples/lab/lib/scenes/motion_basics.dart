// Step 6: motion, the basics. One cube, and one button for each thing a
// tween can do to it.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class MotionBasicsScene extends LabScene {
  @override
  String get title => 'Motion: the basics';
  @override
  String get step => 'step 6';
  @override
  String get about =>
      'One cube. Each button is one line: cube.moveTo(...), rotateBy, scaleTo, colorTo. "All in a row" puts four '
      'of them in a sequence. Tap the cube: punchScale.';

  late final GameObject cube;
  var _right = false;
  var _colour = 0;
  static const _colours = [Color(0xFFE0613A), Color(0xFF7BC86C), Color(0xFFFFC020), Color(0xFF4CA6E0)];

  @override
  void build() {
    cube = add(Cube());
  }

  void _move() {
    _right = !_right;
    cube.moveTo(Vec3(_right ? 2 : -2, 0.5, 0), seconds: 0.6);
  }

  void _allInARow() {
    cube.sequence([
      cube.moveTo(Vec3(0, 1.5, 0), seconds: 0.4),
      cube.rotateBy(Vec3.up, math.pi, seconds: 0.4),
      cube.colorTo(const Color(0xFFE0613A), seconds: 0.3),
      cube.moveTo(Vec3(0, 0.5, 0), seconds: 0.5, ease: Ease.outBounce),
    ]);
    _right = false;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = 'position ${_vec(cube.position)}   scale ${cube.scale.x.toStringAsFixed(2)}';
  }

  static String _vec(Vec3 v) => '(${v.x.toStringAsFixed(1)}, ${v.y.toStringAsFixed(1)}, ${v.z.toStringAsFixed(1)})';

  @override
  List<Control> get controls => [
    Button('move', _move),
    Button('spin', () => cube.rotateBy(Vec3.up, math.pi / 2, seconds: 0.5)),
    Button('grow', () => cube.scaleTo(Vec3.one * 1.5, seconds: 0.4)),
    Button('shrink', () => cube.scaleTo(Vec3.one, seconds: 0.4)),
    Button('colour', () => cube.colorTo(_colours[_colour++ % _colours.length], seconds: 0.5)),
    Button('all in a row', _allInARow),
  ];
}

/// Tap it: it swells and settles back.
class Cube extends GameObject {
  Cube()
    : super(
        name: 'cube',
        transform: Transform(position: Vec3(0, 0.5, 0)),
      );

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF4CA6E0))));
  }

  @override
  bool onClick(Finger finger) {
    punchScale(Vec3.one * 0.3, seconds: 0.4);
    return true;
  }
}
