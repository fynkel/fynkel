import 'package:fynkel/fynkel.dart';

class Sphere extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.sphere(), Material(color: Color(0xFF8805))));
  }

  var t = 0;
  var goingUp = true;

  @override
  void onUpdate() {
    super.onUpdate();
    print(transform.position.y);

    if (goingUp) {
      translate(Vec3(0, 1, 0) * time.delta);
      if (transform.position.y >= 1) {
        goingUp = false;
      }
    } else {
      translate(Vec3(0, -1, 0) * time.delta);
      if (transform.position.y <= -1) {
        goingUp = true;
      }
    }
  }
}
