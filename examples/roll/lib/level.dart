import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A box that stays put: the floor, a wall, a pillar. Its collider takes
/// the object's scale, so one `BoxCollider()` fits however big it is.
GameObject block(Vec3 position, Vec3 size, Color color) => GameObject(
  transform: Transform(position: position, scale: size),
  components: [
    MeshRenderer(Mesh.cube(), Material(color: color)),
    BoxCollider(),
  ],
);

/// The floor, 20 m across, walls round it, and two pillars to hide behind.
GameObject level() {
  const stone = Color(0xFFB8B0A0);
  const wall = Color(0xFF8A7F70);
  return GameObject(
    name: 'level',
    children: [
      block(const Vec3(0, -0.5, 0), const Vec3(20, 1, 20), stone),
      block(const Vec3(0, 1, -10.5), const Vec3(22, 2, 1), wall),
      block(const Vec3(0, 1, 10.5), const Vec3(22, 2, 1), wall),
      block(const Vec3(-10.5, 1, 0), const Vec3(1, 2, 20), wall),
      block(const Vec3(10.5, 1, 0), const Vec3(1, 2, 20), wall),
      block(const Vec3(-4, 1.5, -3), const Vec3(1.5, 3, 1.5), wall),
      block(const Vec3(5, 1.5, 4), const Vec3(1.5, 3, 1.5), wall),
    ],
  );
}
