// The game. It imports only package:fynkel, so Fynkel's Transform,
// Material and Color never meet Flutter's: main.dart, the Flutter side,
// doesn't import package:fynkel at all.

import 'package:fynkel/fynkel.dart';

/// The stage 1 program (docs/stage-1.md), as written there.
class Hello {
  Hello() {
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 1.5, 4), lookAt: Vec3.zero),
        components: [Camera()],
      ),
    );
    world.add(
      GameObject(
        transform: Transform(forward: Vec3(-0.4, -1, -0.3)),
        components: [Light.sun()],
      ),
    );
    cube = world.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A)))]));
  }

  final world = World();
  late final GameObject cube;

  void onFrame(double dt) => cube.rotate(Vec3.up, dt);
}
