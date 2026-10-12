// Stage 6: moving, the basics. A cube on a turned table: where it is in the
// world and on the table, moving and turning in its own space or the
// world's, and taking it off the table and putting it back.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class MovingBasicsScene extends LabScene {
  @override
  String get title => 'Moving: the basics';
  @override
  String get step => 'stage 6';
  @override
  String get about =>
      'A cube on a table turned 45°. position is where the cube is in the world, localPosition where it is on '
      'the table. translate and rotate go by the cube\'s own axes, or the world\'s with Space.world. Take it '
      'off the table and put it back: it stays where it is (parent = keeps the world position). Then spin the '
      'table.';

  late final GameObject table, cube;

  @override
  void build() {
    table = add(
      GameObject(
        name: 'table',
        transform: Transform(position: Vec3(0, 0.25, 0), rotation: Quat.axisAngle(Vec3.up, math.pi / 4)),
      ),
    );
    // The top has the size, so the cube on the table isn't stretched.
    table.add(
      GameObject(
        name: 'top',
        transform: Transform(scale: Vec3(4, 0.5, 2)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF8A6A4A)))],
      ),
    );
    cube = table.add(
      GameObject(
        name: 'cube',
        transform: Transform(position: Vec3(0, 0.75, 0)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF4CA6E0)))],
      ),
    );
  }

  void _put(GameObject? parent) {
    cube.parent = parent ?? this;
    log.add(parent == null ? 'cube.parent = scene: off the table, where it was' : 'cube.parent = table: back on it');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value =
        'position ${_vec(cube.position)}   localPosition ${_vec(cube.localPosition)}   on the ${cube.parent!.name}';
  }

  static String _vec(Vec3 v) => '(${v.x.toStringAsFixed(1)}, ${v.y.toStringAsFixed(1)}, ${v.z.toStringAsFixed(1)})';

  @override
  List<Control> get controls => [
    Button('forward', () => cube.translate(Vec3.forward)),
    Button('forward in the world', () => cube.translate(Vec3.forward, relativeTo: Space.world)),
    Button('turn', () => cube.rotate(Vec3.up, math.pi / 8)),
    Button('tip over in the world', () => cube.rotate(Vec3.right, math.pi / 8, relativeTo: Space.world)),
    Button('off the table', () => _put(null)),
    Button('on the table', () => _put(table)),
    Button('spin the table', () => table.rotateBy(Vec3.up, math.pi / 2, seconds: 0.6)),
    Button('start over', () {
      cube.setParent(table, worldPositionStays: false);
      cube.transform = Transform(position: Vec3(0, 0.75, 0));
    }),
  ];
}
