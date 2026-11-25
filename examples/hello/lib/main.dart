// The smallest game: a camera, a sun and a turning cube, as in
// https://fynkel.com/basics/hello/. tool/measured.dart runs the same game for the
// tool.

import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() => runApp(FynkelApp(Hello.new));

class Hello extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(transform: Transform.lookAt(Vec3(0, 1.5, 4), Vec3.zero), components: [Camera()]));
    add(GameObject(components: [Light.sun(direction: Vec3(-0.4, -1, -0.3))]));
    add(Cube());
  }
}

class Cube extends GameObject {
  Cube() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A)))]);

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, time.delta);
  }
}
