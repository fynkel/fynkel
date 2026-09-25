import 'dart:math' show cos, sin;

import 'package:fynkel/fynkel.dart';

import '../generated/assets.g.dart';
import 'flow.dart';
import 'game_state.dart';
import 'level.dart';

/// A coin (`coin` in level.dart): counts itself into the [GameState] at
/// `start`; when the player enters its `collect` zone it scores, plays the
/// pickup sound, leaves a [burst] of sparks in its own gold and destroys
/// itself.
@FynkelBehaviour(id: 'game.Collectible')
class Collectible extends Behaviour {
  /// Points this coin is worth.
  @Expose(min: 1)
  int value = 1;

  bool collected = false;

  GameState get state => context.service<GameState>();

  @override
  void start() {
    state.total.value += 1;
  }

  @override
  void onZoneEnter(ZoneEvent event) {
    if (event.event != 'collect' || collected || !isPlayer(world, event.other)) return;
    collected = true;
    state.collect();
    world.audio.play(Assets.audio.coin, volume: 0.7);

    // The sparks start where the coin is and fly apart.
    final here = (gameObject.worldTransform ?? transform.value).position;
    final gold = gameObject.getInChildren<MeshRenderer>()?.material ?? Primitive.defaultMaterial;
    final sparks = world.build(burst(gold, here));
    for (final (i, spark) in sparks.children.indexed) {
      final angle = i * 1.047; // six sparks, 60° apart
      spark.applyImpulse(Vec3(0.04 * cos(angle), 0.15, 0.04 * sin(angle)));
    }
    gameObject.destroy();
  }
}
