```dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A ball that falls on the ground.
class Falling extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(components: [BoxCollider(size: Vec3(10, 1, 10))]));
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 5, 0)),
        components: [MeshRenderer(Mesh.sphere(), Material()), SphereCollider(), Rigidbody()],
      ),
    );
  }
}
```
