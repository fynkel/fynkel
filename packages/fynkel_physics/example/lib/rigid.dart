// Tumble with the package's own parts (stage 5 step 4, docs/stage-5.md):
// each cube a BoxCollider and a Rigidbody, as a game would make them,
// where lib/main.dart's Tumble drives the raw world itself.
//
//   flutter run --dart-define=TUMBLE_RIGIDBODIES=1 --dart-define=TUMBLE_CUBES=500
//     [--dart-define=TUMBLE_THREAD=0] [--dart-define=TUMBLE_INTERPOLATE=1]
//     [--dart-define=TUMBLE_RAYS=10] [--dart-define=TUMBLE_LISTEN=1]

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import 'main.dart' show cubeCount, rayCount, size, threadMode;

/// `--dart-define=TUMBLE_INTERPOLATE=1` draws each cube between its last
/// two steps.
const interpolated = bool.fromEnvironment('TUMBLE_INTERPOLATE');

/// `--dart-define=TUMBLE_LISTEN=1` gives each cube a behaviour with
/// collision callbacks (stage 5 step 5), so every contact it has is told.
const listening = bool.fromEnvironment('TUMBLE_LISTEN');

/// Counts the collision callbacks its cube gets.
class Touches extends Behaviour with Collisions {
  static var told = 0;

  @override
  void onCollisionEnter(Collision collision) => told++;

  @override
  void onCollisionStay(Collision collision) => told++;

  @override
  void onCollisionExit(Collision collision) => told++;
}

class RigidTumble extends GameObject {
  RigidTumble({this.count = cubeCount});

  final int count;
  final cubes = <GameObject>[];
  var _sinceDrop = 0.0;

  @override
  void onLoad() {
    super.onLoad();
    // TUMBLE_THREAD=0 steps in the frame; anything else between frames.
    physics.stepBetweenFrames = threadMode != 0;
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
    add(
      GameObject(
        transform: Transform(position: Vec3(0, -0.5, 0)),
        components: [BoxCollider(size: Vec3(40, 1, 40))],
      ),
    );
    final material = Material(color: Color(0xFFE0613A));
    for (var i = 0; i < count; i++) {
      cubes.add(
        add(
          GameObject(
            transform: Transform(scale: Vec3(size, size, size)),
            components: [
              MeshRenderer(Mesh.cube(), material),
              BoxCollider(),
              Rigidbody(interpolation: interpolated ? RigidbodyInterpolation.interpolate : RigidbodyInterpolation.none),
              if (listening) Touches(),
            ],
          ),
        ),
      );
    }
    drop();
  }

  /// Puts every cube back in the air, as lib/main.dart's Tumble does: a
  /// teleport, by setting its transform.
  void drop() {
    final side = math.sqrt(count / 10).ceil();
    for (var i = 0; i < count; i++) {
      final column = i ~/ 10;
      cubes[i].transform = Transform(
        position: Vec3(
          (column % side - side / 2) * size * 1.6,
          2 + (i % 10) * size * 1.4,
          (column ~/ side - side / 2) * size * 1.6,
        ),
        rotation: Quat.axisAngle(Vec3(1, 1, 0).normalized, i * 0.37),
        scale: Vec3(size, size, size),
      );
      cubes[i].get<Rigidbody>()!
        ..linearVelocity = Vec3.zero
        ..angularVelocity = Vec3.zero;
    }
    _sinceDrop = 0;
  }

  /// How many rays hit, so they aren't optimised away.
  var hits = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    _sinceDrop += time.delta;
    if (_sinceDrop > 8) drop();
    for (var i = 0; i < rayCount; i++) {
      final ray = Ray(Vec3(i * 0.7 - rayCount * 0.35, 12, i * 0.3 - 2), const Vec3(0, -1, 0));
      hits += physics.raycast(ray, maxDistance: 30) == null ? 0 : 1;
    }
  }
}
