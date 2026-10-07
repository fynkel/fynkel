// The game. It imports only package:fynkel, so Fynkel's Transform,
// Material and Color never meet Flutter's: main.dart, the Flutter side,
// doesn't import package:fynkel at all.

import 'package:fynkel/fynkel.dart';

/// The stage 1 program (docs/stage-1.md), as written there.
class Hello {
  Hello() {
    world.spawn(Camera(), Transform(position: Vec3(0, 1.5, 4), lookAt: Vec3.zero));
    world.spawn(Light.sun(), Transform(forward: Vec3(-0.4, -1, -0.3)));
    cube = world.spawn(Transform(), MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A))));
  }

  final world = World();
  late final GameObject cube;

  void onFrame(double dt) => cube.rotate(Vec3.up, dt);
}
