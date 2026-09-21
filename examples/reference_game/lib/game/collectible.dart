import 'dart:math' show cos, sin;

import 'package:fynkel/fynkel.dart';

import 'game.dart';
import 'game_state.dart';

/// A coin (assets/prefabs/coin.prefab.json): counts itself into the
/// [GameState] at `start`, and when the player enters its `collect` zone
/// (a `fynkel.TriggerZone` with `once`) it scores, plays [pickup] once
/// (M3-11: a fire-and-forget `world.audio.play` — the coin is gone a tick
/// later, so no source on it could finish), leaves a burst of sparks
/// (the `burst` prefab instantiated at runtime, its pieces kicked apart
/// and despawned by their `LifetimeDespawn`) and destroys its prefab
/// instance — the anchor entity the scene spawned it under, so nothing of
/// the coin stays.
@FynkelBehaviour(id: 'game.Collectible')
class Collectible extends Behaviour {
  /// Points this coin is worth (the HUD counts coins, not points; here for
  /// the schema's sake and a future scoreboard).
  @Expose(min: 1)
  int value = 1;

  /// The pickup sound (assets/audio/coin.ogg; silent when unset).
  @Expose()
  AssetRef pickup = AssetRef.none;

  bool collected = false;

  GameState get state => context.service<GameState>();

  @override
  void start() {
    state.total.value += 1;
  }

  @override
  void onZoneEnter(ZoneEvent event) {
    if (event.event != 'collect' || collected) return;
    collected = true;
    state.collect();
    if (pickup != AssetRef.none) world.audio.play(pickup, volume: 0.7);

    // The sparks start where the coin is (its world pose as the last tick
    // left it) and fly apart.
    final here = (gameObject.worldTransform ?? transform.value).position;
    final burst = world.instantiate(
      burstPrefab,
      transform: Transform(position: here, rotation: Quat.identity, scale: const Vec3(1, 1, 1)),
      name: 'Burst',
    );
    var i = 0;
    for (final child in burst.children) {
      final spark = world.attach(child);
      final angle = i * 1.047; // six sparks, 60° apart
      spark.applyImpulse(Vec3(0.6 * cos(angle), 1.2, 0.6 * sin(angle)));
      i++;
    }

    // The prefab instance's anchor (our root's parent) goes, and we with it.
    final anchor = gameObject.parent;
    (anchor == null ? gameObject : (world.byEntity(anchor) ?? world.attach(anchor))).destroy();
  }
}
