import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import 'ball.dart';
import 'gem.dart';
import 'level.dart';

/// The game's root: the level, the ball, the gems and a camera.
class RollGame extends GameObject {
  static const gemPlaces = [Vec3(-6, 0.6, -6), Vec3(6, 0.6, -6), Vec3(-6, 0.6, 6), Vec3(7, 0.6, 7), Vec3(0, 0.6, -8)];

  /// How many gems are still out: the HUD shows it.
  final gemsLeft = ValueNotifier(gemPlaces.length);

  late final Ball ball;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(const Vec3(-0.4, -1, -0.3))),
        components: [Light.sun()],
      ),
    );
    add(level());
    ball = add(Ball());
    for (final place in gemPlaces) {
      add(Gem(place, onTaken: () => gemsLeft.value--));
    }
    add(
      GameObject(
        name: 'camera',
        components: [
          Camera(),
          Orbit(ball, distance: 7, pitch: 0.6, offset: const Vec3(0, 0.5, 0)),
          CameraCollision(),
        ],
      ),
    );
  }
}
