import 'package:fynkel/fynkel.dart';

class Sphere extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.sphere(), own(Material(color: Color(0xFF8805)))));
  }

  var t = 0;
  var up = true;

  @override
  void onUpdate() {
    super.onUpdate();
    print(transform.position.y);

    if (up) {
      translate(Vec3(0, 1, 0) * time.delta);
      if (transform.position.y >= 1) {
        up = false;
      }
    } else {
      translate(Vec3(0, -1, 0) * time.delta);
      if (transform.position.y <= -1) {
        up = true;
      }
    }
  }
}
