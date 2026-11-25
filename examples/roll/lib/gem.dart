import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import 'ball.dart';

/// A gem that spins where it is, and is taken when the ball rolls into it.
class Gem extends GameObject with Triggers {
  Gem(Vec3 at, {required this.onTaken})
    : super(
        name: 'gem',
        transform: Transform(position: at, scale: Vec3.all(0.5)),
        components: [
          MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF3AC0E0), emissive: const Color(0xFF105060))),
          BoxCollider(isTrigger: true),
        ],
      );

  /// What it tells the game when it's taken.
  final void Function() onTaken;

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, 2 * time.delta);
  }

  @override
  void onTriggerEnter(Collider other) {
    if (other.gameObject is! Ball) return;
    onTaken();
    destroy();
  }
}
