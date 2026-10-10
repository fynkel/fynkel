// A probe for hot reload (docs/stage-4.md §3.9), not part of the game:
// `flutter run -t lib/reload_probe.dart`, then change the v1s, `field`'s
// 1, or add a field, and press r (hot reload) or R (hot restart).

import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() => runApp(FynkelApp(Probe()));

class Probe extends GameObject {
  int field = 1;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 1, 4), lookAt: Vec3.zero),
        components: [Camera()],
      ),
    );
    add(GameObject(components: [Light.ambient()]));
    add(
      GameObject(
        components: [
          MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A))),
          Spinner(),
        ],
      ),
    );
    print('probe: onLoad v1');
    start(() async {
      while (true) {
        await wait(1);
        print('probe: routine v1');
      }
    });
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (time.frame % 60 == 0) print('probe: update ${say()} field=$field');
  }

  String say() => 'v1';
}

class Spinner extends Behaviour {
  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, time.delta);
    if (time.frame % 60 == 30) print('probe: spinner v1');
  }
}
