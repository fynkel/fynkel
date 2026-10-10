// The stage 1 program (docs/stage-1.md), as a game: a camera, a sun and a
// turning cube. lib/measured.dart runs the same game for the tool.

import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:hello/sphere.dart';

void main() => runApp(FynkelApp(Hello.new));

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
    add(Sphere());
  }
}

class Cube extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A))));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, time.delta);
  }
}
