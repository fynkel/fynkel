import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// The player: a ball that W, A, S, D or the arrows roll, away from the
/// camera and across it, whichever way the camera looks.
class Ball extends GameObject {
  Ball()
    : super(
        name: 'ball',
        transform: Transform(position: const Vec3(0, 1, 3)),
        components: [
          MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFE0613A))),
          SphereCollider(),
          Rigidbody(mass: 1, interpolation: RigidbodyInterpolation.interpolate),
        ],
      );

  late final Rigidbody body = get<Rigidbody>()!;

  /// Pushes are worked out in a fixed step, as physics runs, so the ball
  /// rolls the same at 60 or 120 frames a second.
  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    final move = input.axis2('move');
    final camera = world.camera?.gameObject;
    if (camera == null || move.length == 0) return;
    // The camera's forward and right, flat on the floor.
    final forward = Vec3(camera.forward.x, 0, camera.forward.z).normalized;
    final right = Vec3(camera.right.x, 0, camera.right.z).normalized;
    body.addForce((forward * move.y + right * move.x) * 12);
  }
}
