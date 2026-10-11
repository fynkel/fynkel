// Stage 5 step 4: rigidbodies, the basics. A cube and a ball with a
// Rigidbody fall onto the floor; push the cube, or turn gravity off.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class RigidbodyBasicsScene extends LabScene {
  @override
  String get title => 'Rigidbodies: the basics';
  @override
  String get step => 'stage 5 step 4';
  @override
  String get about =>
      'The cube and the ball each have a collider and a Rigidbody, so physics moves them: they fall, land on the '
      'floor (a BoxCollider under it) and roll or rest. Push the cube with addForce, drop them again, or turn '
      'gravity off.';

  late final GameObject cube, ball;
  late final Rigidbody cubeBody, ballBody;

  @override
  void build() {
    add(floorCollider());
    cubeBody = Rigidbody();
    ballBody = Rigidbody();
    cube = add(shape('cube', Mesh.cube(), const Color(0xFFE0613A), components: [BoxCollider(), cubeBody]));
    ball = add(shape('ball', Mesh.sphere(), const Color(0xFF3A8EE0), components: [SphereCollider(), ballBody]));
    drop();
  }

  void drop() {
    cube.transform = Transform(position: Vec3(-1, 3, 0), rotation: Quat.axisAngle(Vec3(1, 0, 1).normalized, 0.6));
    ball.transform = Transform(position: Vec3(1, 4, 0));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      'the ball is ${ball.position.y.toStringAsFixed(2)} m up, at ${ballBody.linearVelocity.length.toStringAsFixed(2)} m/s',
      'the cube is ${cubeBody.isSleeping ? 'asleep' : 'awake'}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('drop them again', () {
      drop();
      log.add('dropped');
    }),
    Button('push the cube', () {
      cubeBody.addForce(Vec3(2, 4, 0), mode: ForceMode.impulse);
      log.add('pushed: addForce(Vec3(2, 4, 0), mode: ForceMode.impulse)');
    }),
    Toggle('gravity', () => cubeBody.useGravity, (on) {
      cubeBody.useGravity = on;
      ballBody.useGravity = on;
      if (!on) ballBody.addForce(Vec3(0, 2, 0), mode: ForceMode.velocityChange);
    }),
  ];
}
