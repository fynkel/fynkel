// Stage 7: split screen. Two FynkelViews of the same world side by side,
// each drawn from the camera that follows its player. The world is
// updated once a frame for both, and each player's name tag (a
// WorldWidget) shows in both halves, placed by that half's camera. The
// left half takes the keys; the right takes no input of its own.

import 'package:flutter/material.dart' hide Color, Material, Transform;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';
import '../widgets/tags.dart';

class SplitScreenBasicsScene extends LabScene {
  @override
  String get title => 'Split screen: the basics';
  @override
  String get step => 'stage 7';
  @override
  String get about =>
      'WASD moves the blue cube, seen on the left; the arrows move the orange one, seen on the right. '
      'Each half is a view of the same world from its own camera, which follows its cube. The readout '
      'has each half\'s frames per second.';

  late final GameObject blue, orange;
  late final Camera leftCamera, rightCamera;
  late final GameObject _leftEye, _rightEye;
  var _left = '–', _right = '–';

  static const _behind = Vec3(0, 3, 6);

  @override
  void build() {
    input
      ..bindAxis2(
        'blue',
        up: const [KeyCode.w],
        down: const [KeyCode.s],
        left: const [KeyCode.a],
        right: const [KeyCode.d],
      )
      ..bindAxis2(
        'orange',
        up: const [KeyCode.arrowUp],
        down: const [KeyCode.arrowDown],
        left: const [KeyCode.arrowLeft],
        right: const [KeyCode.arrowRight],
      );
    blue = add(_player('blue', const Color(0xFF4CA6E0), Vec3(-2, 0.5, 0)));
    orange = add(_player('orange', const Color(0xFFE0613A), Vec3(2, 0.5, 0)));
    for (final (x, z) in [(-4.0, -4.0), (4.0, -4.0), (-4.0, 4.0), (4.0, 4.0), (0.0, -7.0)]) {
      add(shape('pillar', Mesh.cube(), const Color(0xFFFFD040), at: Vec3(x, 1, z), size: Vec3(0.6, 2, 0.6)));
    }
    leftCamera = Camera(clearColor: const Color(0xFF1C2230));
    rightCamera = Camera(clearColor: const Color(0xFF2C2220));
    _leftEye = add(GameObject(name: 'left camera', components: [leftCamera]));
    _rightEye = add(GameObject(name: 'right camera', components: [rightCamera]));
    _follow();
  }

  GameObject _player(String name, Color colour, Vec3 at) => shape(
    name,
    Mesh.cube(),
    colour,
    at: at,
    components: [WorldWidget((_) => NameTag(name), offset: Vec3(0, 0.8, 0), alignment: Alignment.bottomCenter)],
  );

  void _move(GameObject player, String axis) {
    final move = input.axis2(axis);
    final p = player.position + Vec3(move.x, 0, -move.y) * (5 * time.delta);
    player.position = Vec3(p.x.clamp(-9, 9), 0.5, p.z.clamp(-9, 9));
  }

  void _follow() {
    for (final (eye, player) in [(_leftEye, blue), (_rightEye, orange)]) {
      eye.position = player.position + _behind;
      eye.rotation = Quat.lookRotation(-_behind);
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _move(blue, 'blue');
    _move(orange, 'orange');
    _follow();
    readout.value = 'left $_left fps   right $_right fps';
  }

  @override
  List<Control> get controls => const [];

  @override
  Widget view(BuildContext context) => Row(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      Expanded(
        child: FynkelView(root: this, camera: leftCamera, onStats: (s) => _left = fps('left', s)),
      ),
      const SizedBox(width: 2, child: ColoredBox(color: Colors.white70)),
      Expanded(
        child: FynkelView(root: this, camera: rightCamera, input: false, onStats: (s) => _right = fps('right', s)),
      ),
    ],
  );
}
