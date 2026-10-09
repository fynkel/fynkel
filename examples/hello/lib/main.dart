// The stage 1 program (docs/stage-1.md), as a game: a camera, a sun and a
// turning cube. lib/measured.dart runs the same game for the tool.

import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() => runApp(FynkelApp(Hello()));

class Hello extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 1.5, 4), lookAt: Vec3.zero),
        components: [Camera()],
      ),
    );
    add(
      GameObject(
        transform: Transform(forward: Vec3(-0.4, -1, -0.3)),
        components: [Light.sun()],
      ),
    );
    add(Cube());
  }
}

class Cube extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    meshRenderer = MeshRenderer(Mesh.cube(), own(Material(color: Color(0xFFE0613A))));
  }

  @override
  void onUpdate(double dt) {
    super.onUpdate(dt);
    rotate(Vec3.up, dt);
  }
}
