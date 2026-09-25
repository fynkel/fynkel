import 'package:fynkel/fynkel.dart';

import 'pickup_kind.dart';

/// What a game hands the pickups: `world.context.install<PickupSink>(…)`.
/// A pickup taken with none installed only goes.
abstract interface class PickupSink {
  /// [pickup], of [kind], was taken by the object tagged its
  /// [Pickup.takenBy].
  void picked(Pickup pickup, PickupKind? kind);
}

/// A pickup: takes its [kind]'s colour at `start`, and when the object
/// tagged [takenBy] enters its `pickup` zone tells the game's
/// [PickupSink], plays the kind's sound and goes. The package's prefab
/// (assets/prefabs/pickup.prefab.json) spins and bobs it.
@FynkelBehaviour(id: 'pickups.Pickup')
class Pickup extends Behaviour {
  /// What it gives and how it looks and sounds.
  @Expose()
  PickupKind? kind;

  /// The tag of the object that takes it (the game's, `player` by
  /// default).
  @Expose()
  String takenBy = 'player';

  bool taken = false;

  /// A material per colour, made once for the world's life.
  static final _materials = <World, Map<Vec3, AssetRef>>{};

  @override
  void start() {
    final k = kind;
    if (k == null) return;
    final c = k.color;
    final material = (_materials[world] ??= {})[c] ??= world.createMaterial(
      PbrMaterial(baseColor: Vec4(c.x, c.y, c.z, 1), metallic: 0.1, roughness: 0.2, emissive: c, emissiveStrength: 0.6),
    );
    for (final part in gameObject.children) {
      final renderer = part.get<MeshRenderer>();
      if (renderer != null) part.set(renderer.copyWith(material: material));
    }
  }

  @override
  void onZoneEnter(ZoneEvent event) {
    if (event.event != 'pickup' || world.findWithTag(takenBy)?.entity != event.other) return;
    take();
  }

  /// Taken: the game is told, the sound plays, the pickup goes.
  void take() {
    if (taken) return;
    taken = true;
    world.context.serviceOrNull<PickupSink>()?.picked(this, kind);
    final sound = kind?.sound;
    if (sound != null) world.audio.play(sound, volume: 0.7, pitch: kind!.pitch);
    gameObject.destroy();
  }
}
