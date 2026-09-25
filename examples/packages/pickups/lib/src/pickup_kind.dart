// A kind of pickup (ADR-0027, a data asset): what taking one gives, its
// colour and its sound. The package ships one, assets/data/gem.data.json
// (tool/assets.dart writes it); a game makes its own with Create ▸
// Pickups ▸ Pickup Kind.

import 'package:fynkel/fynkel.dart';

/// What a pickup gives and how it looks and sounds.
@FynkelDataAsset(id: 'pickups.PickupKind', menu: 'Pickups/Pickup Kind', icon: 'diamond')
class PickupKind extends DataAsset {
  PickupKind({this.points = 10, this.color = const Vec3(0.3, 0.9, 0.8), this.sound, this.pitch = 1});

  /// What taking one is worth; the game decides what points mean
  /// ([PickupSink]).
  @Expose()
  int points;

  /// Its colour, linear RGB.
  @Expose(kind: 'color')
  Vec3 color;

  /// What it plays when taken (none: silent).
  @Expose(kind: 'asset(audio)')
  AssetRef? sound;

  /// The sound's pitch.
  @Expose(min: 0.05, max: 8)
  double pitch;
}
