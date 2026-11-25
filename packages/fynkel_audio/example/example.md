```dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

/// A thing that hums, louder the nearer the camera is.
class Humming extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    add(
      GameObject(
        components: [
          AudioSource(clip: await assets.audio('hum.ogg'), loop: true, spatialBlend: 1, maxDistance: 25),
        ],
      ),
    );
  }
}
```
