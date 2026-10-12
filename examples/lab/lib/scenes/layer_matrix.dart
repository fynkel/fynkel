// Stage 5 step 5: the layer matrix. Two balls fall: the red one is on the
// ghost layer, which can be made to pass through the floor; the blue one
// can be made to ignore the floor by itself.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

/// The red ball's layer. Name layers with constants like this.
const ghostLayer = 8;

class LayerMatrixScene extends LabScene {
  @override
  String get title => 'The layer matrix';
  @override
  String get step => 'stage 5 step 5';
  @override
  String get about =>
      'The red ball is on layer 8, the ghost layer. physics.ignoreLayerCollision(8, 0) makes layer 8 pass '
      'through layer 0, the floor\'s, so it falls through. physics.ignoreCollision(blue, floor) does the same for '
      'one pair. A ball that falls far comes back to the top.';

  late final GameObject red, blue, floor;
  var blueIgnoresFloor = false;

  @override
  void build() {
    floor = add(floorCollider());
    red = add(shape('red ball', Mesh.sphere(), const Color(0xFFE0613A), components: [SphereCollider(), Rigidbody()]))
      ..layer = ghostLayer;
    blue = add(shape('blue ball', Mesh.sphere(), const Color(0xFF3A8EE0), components: [SphereCollider(), Rigidbody()]));
    drop(red, -1);
    drop(blue, 1);
  }

  void drop(GameObject ball, double x) {
    ball.transform = Transform(position: Vec3(x, 4, 0));
    ball.get<Rigidbody>()!.linearVelocity = Vec3.zero;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (red.position.y < -6) drop(red, -1);
    if (blue.position.y < -6) drop(blue, 1);
    readout.value = [
      'layer 8 meets layer 0: ${!physics.getIgnoreLayerCollision(ghostLayer, 0)}',
      'blue ignores the floor: ${physics.getIgnoreCollision(blue.get<SphereCollider>()!, floor.get<BoxCollider>()!)}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Toggle('layer 8 meets the floor', () => !physics.getIgnoreLayerCollision(ghostLayer, 0), (on) {
      physics.ignoreLayerCollision(ghostLayer, 0, ignore: !on);
      log.add('physics.ignoreLayerCollision($ghostLayer, 0, ignore: ${!on})');
    }),
    Toggle('blue meets the floor', () => !physics.getIgnoreCollision(_blue, _floor), (on) {
      physics.ignoreCollision(_blue, _floor, ignore: !on);
      log.add('physics.ignoreCollision(blue, floor, ignore: ${!on})');
    }),
    Button('drop them again', () {
      drop(red, -1);
      drop(blue, 1);
      log.add('dropped');
    }),
  ];

  SphereCollider get _blue => blue.get<SphereCollider>()!;
  BoxCollider get _floor => floor.get<BoxCollider>()!;
}
