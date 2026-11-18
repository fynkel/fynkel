import 'package:fynkel/fynkel.dart';

/// The basket at the bottom. It goes to the point under the finger (or
/// the mouse, held down), or left and right with A, D or the arrow keys.
class Basket extends GameObject {
  Basket()
    : super(
        name: 'basket',
        components: [MeshRenderer(Mesh.cylinder(), Material(color: const Color(0xFF4FB3FF), roughness: 0.4))],
      ) {
    localScale = Vec3(1.8, 0.3, 1.8);
  }

  /// How far it may go from the middle, either way.
  static const reach = 3.2;

  /// Metres a second, with the keys.
  static const keySpeed = 9.0;

  /// The plane the stars fall in, for the point under a finger.
  static final _field = Plane(Vec3.forward, Vec3.zero);

  @override
  void onUpdate() {
    super.onUpdate();
    var x = position.x + input.axis2('move').x * keySpeed * time.delta;
    for (final finger in input.fingers) {
      if (!finger.isPressed) continue;
      final point = world.camera?.screenPointToPlane(finger.position, _field);
      if (point != null) x = point.x;
      break;
    }
    position = Vec3(x.clamp(-reach, reach), position.y, position.z);
  }
}
