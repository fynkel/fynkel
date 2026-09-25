import 'dart:math' show cos, pi, sin;

import 'package:fynkel/fynkel.dart';

import '../generated/assets.g.dart';
import 'coin_kind.dart';
import 'flow.dart';
import 'game_state.dart';
import 'level.dart';

/// A coin (`coin` in level.dart): counts itself into the [GameState] at
/// `start` and takes its [kind]'s colour and size; when the player enters
/// its `collect` zone it scores the kind's value, plays its sound, leaves
/// a [burst] of sparks in its colour and destroys itself.
@FynkelBehaviour(id: 'game.Collectible')
class Collectible extends Behaviour {
  /// What it is worth and how it looks and sounds: a data asset
  /// (coin_kind.dart), shared by every coin of the kind.
  @Expose()
  CoinKind? kind;

  bool collected = false;

  GameState get state => context.service<GameState>();

  /// A material per kind and colour, made once for the world's life.
  static final _materials = Expando<(Vec3, AssetRef)>();

  @override
  void start() {
    state.total.value += 1;
    final k = kind;
    if (k == null) return;
    transform.scale = Vec3.all(k.size);
    final made = _materials[k];
    final material = made != null && made.$1 == k.color ? made.$2 : _material(k);
    for (final part in gameObject.children) {
      final renderer = part.get<MeshRenderer>();
      if (renderer != null) part.set(renderer.copyWith(material: material));
    }
  }

  AssetRef _material(CoinKind k) {
    final c = k.color;
    final ref = world.createMaterial(PbrMaterial(
      baseColor: Vec4(c.x, c.y, c.z, 1),
      metallic: 0.4,
      roughness: 0.35,
      emissive: c,
      emissiveStrength: 0.35,
    ));
    _materials[k] = (c, ref);
    return ref;
  }

  @override
  void onZoneEnter(ZoneEvent event) {
    if (event.event != 'collect' || !isPlayer(world, event.other)) return;
    collect();
  }

  /// Taken: scores the kind's value, plays its sound, bursts into sparks
  /// and goes (the player touching it; the editor's Level Tools too).
  void collect() {
    if (collected) return;
    collected = true;
    state.collect(kind?.value ?? 1);
    world.audio.play(kind?.sound ?? Assets.audio.coin, volume: 0.7, pitch: kind?.pitch ?? 1);

    // The sparks start where the coin is and fly apart.
    final here = (gameObject.worldTransform ?? transform.value).position;
    final gold = gameObject.getInChildren<MeshRenderer>()?.material ?? Primitive.defaultMaterial;
    final sparks = world.build(burst(gold, here, kind?.sparks));
    final count = sparks.children.length;
    for (final (i, spark) in sparks.children.indexed) {
      final angle = i * 2 * pi / count; // evenly apart
      spark.applyImpulse(Vec3(0.04 * cos(angle), 0.15, 0.04 * sin(angle)));
    }
    gameObject.destroy();
  }
}
