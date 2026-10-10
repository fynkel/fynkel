// package:fynkel_physics's example, for now on the raw world of stage 5
// step 1 (docs/stage-5.md): a pile of cubes that fall, tumble and settle,
// dropped again every 8 seconds. Each fixed step's moved cubes are written
// to their objects. lib/measured.dart runs the same for the tool, with the
// step's timings.
//
//   flutter run --dart-define=TUMBLE_CUBES=500 [--dart-define=TUMBLE_PHYSICS=0]

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_physics/raw.dart';

void main() => runApp(FynkelApp(Tumble.new));

const cubeCount = int.fromEnvironment('TUMBLE_CUBES', defaultValue: 1000);

/// `--dart-define=TUMBLE_PHYSICS=0` leaves the cubes where they're dropped:
/// the same scene without the physics, to tell its cost from drawing's.
const stepping = bool.fromEnvironment('TUMBLE_PHYSICS', defaultValue: true);
const stepSeconds = 1 / 50;
const size = 0.5;

class Tumble extends GameObject {
  Tumble({this.count = cubeCount});

  final int count;
  final physics = RawPhysics();
  final cubes = <GameObject>[];
  final bodies = <int>[];
  var _sinceDrop = 0.0;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 14, 22), rotation: Quat.lookRotation(Vec3(0, -10, -22))),
        components: [Camera()],
      ),
    );
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.3))),
        components: [Light.sun()],
      ),
    );
    add(
      GameObject(
        transform: Transform(scale: Vec3(40, 1, 40)),
        components: [MeshRenderer(Mesh.plane(), Material(color: Color(0xFF3A4A5A)))],
      ),
    );
    physics.addBox(0, Vec3(20, 0.5, 20), position: Vec3(0, -0.5, 0));
    final material = Material(color: Color(0xFFE0613A));
    for (var i = 0; i < count; i++) {
      final cube = add(
        GameObject(
          transform: Transform(scale: Vec3(size, size, size)),
          components: [MeshRenderer(Mesh.cube(), material)],
        ),
      );
      final body = physics.addBody(BodyKind.dynamic, i);
      physics.addBox(body, Vec3(size / 2, size / 2, size / 2));
      cubes.add(cube);
      bodies.add(body);
    }
    drop();
  }

  @override
  void onDestroy() {
    physics.dispose();
    super.onDestroy();
  }

  /// Puts every cube back in the air: columns of 10 over a square, each
  /// a little turned so the pile tumbles.
  void drop() {
    final side = math.sqrt(count / 10).ceil();
    for (var i = 0; i < count; i++) {
      final column = i ~/ 10;
      final position = Vec3(
        (column % side - side / 2) * size * 1.6,
        2 + (i % 10) * size * 1.4,
        (column ~/ side - side / 2) * size * 1.6,
      );
      final rotation = Quat.axisAngle(Vec3(1, 1, 0).normalized, i * 0.37);
      physics.setBodyPose(bodies[i], position, rotation);
      cubes[i].transform = Transform(position: position, rotation: rotation, scale: Vec3(size, size, size));
    }
    _sinceDrop = 0;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _sinceDrop += time.delta;
    if (_sinceDrop > 8) drop();
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    if (stepping) stepAndWrite(physics, cubes);
  }
}

/// One fixed step, and each moved cube's pose written to its object.
void stepAndWrite(RawPhysics physics, List<GameObject> cubes) {
  final moved = physics.step(stepSeconds);
  for (var i = 0; i < moved; i++) {
    cubes[physics.movedKey(i)].transform = Transform(
      position: physics.movedPosition(i),
      rotation: physics.movedRotation(i),
      scale: Vec3(size, size, size),
    );
  }
}
