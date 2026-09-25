/// A data type (ADR-0027): `game.Weapon` is an asset type whose instances
/// are `.data.json` files, shared by every behaviour that names one.
/// `fynkel codegen` writes its `DataType` into
/// `lib/generated/behaviours.g.dart` (registered by `registerBehaviours`)
/// and its schema to `schemas/data/game.Weapon.schema.json`.
library;

import 'package:fynkel/fynkel.dart';

/// What a weapon does.
@FynkelDataAsset(id: 'game.Weapon', menu: 'Game/Weapon', icon: 'swords')
class Weapon extends DataAsset {
  Weapon({this.damage = 1, this.cooldown = 0.5, this.projectile, this.sounds = const [], this.upgrade});

  /// Hit points one hit takes.
  @Expose(min: 0)
  int damage;

  /// Seconds between two hits.
  @Expose(min: 0, unit: 's')
  double cooldown;

  /// What it fires, if anything.
  @Expose(kind: 'asset(prefab)')
  AssetRef? projectile;

  /// One is played per hit.
  @Expose(kind: 'asset(audio)')
  List<AssetRef> sounds;

  /// What it becomes when upgraded: another weapon asset.
  @Expose()
  Weapon? upgrade;
}

/// Holds a [Weapon] and spares: data-typed fields on a behaviour, stored as
/// `{ "asset": id }` and set to the shared instance at load.
@FynkelBehaviour(id: 'game.Wielder', icon: 'swords', category: 'Gameplay')
class Wielder extends Behaviour {
  /// The weapon in hand.
  @Expose()
  Weapon? weapon;

  /// The weapons carried.
  @Expose()
  List<Weapon> spares = [];

  /// Damage dealt so far (not authored).
  int dealt = 0;

  /// One hit with the weapon in hand.
  void hit() => dealt += weapon?.damage ?? 0;
}
