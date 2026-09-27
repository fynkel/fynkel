// A kind of coin (ADR-0027): what one is worth, its colour, its size, its
// sound and its sparks, as a data asset type — Unity's ScriptableObject. The game has
// two, [CoinKinds]: the coin and the big coin worth five. tool/scenes.dart
// writes them as assets/data/coin.data.json and big_coin.data.json
// (`ProjectWriter.writeData`), and the coin prefab's `Collectible` names
// one; built live, the coins hold the same instances made in code. Every
// coin of a kind shares its one instance.

import 'package:fynkel/fynkel.dart';

import '../generated/assets.g.dart';

/// What a coin is worth and how it looks and sounds.
@FynkelDataAsset(id: 'game.CoinKind', menu: 'Game/Coin Kind', icon: 'paid')
class CoinKind extends DataAsset {
  CoinKind({
    this.value = 1,
    this.color = const Vec3(1, 0.78, 0.3),
    this.size = 1,
    this.sound,
    this.pitch = 1,
    Sparks? sparks,
  }) : sparks = sparks ?? Sparks();

  /// Points one is worth.
  @Expose(min: 1)
  int value;

  /// Its gold, linear RGB (its sparks too).
  @Expose(kind: 'color')
  Vec3 color;

  /// Its scale.
  @Expose(min: 0.1)
  double size;

  /// What it plays when taken.
  @Expose(kind: 'asset(audio)')
  AssetRef? sound;

  /// The sound's pitch.
  @Expose(min: 0.05, max: 8)
  double pitch;

  /// What it bursts into when taken.
  @Expose()
  Sparks sparks;
}

/// A coin's burst when taken: little cubes of its gold flying apart
/// (ADR-0027 §5, a record: Unity's `[Serializable]` class). The editor
/// draws it with the burst itself (`lib/editor/editor.dart`, a drawer).
@FynkelRecord()
class Sparks {
  Sparks({this.count = 6, this.size = 0.18, this.seconds = 1.2});

  /// How many, spread evenly around the coin.
  @Expose(min: 0, max: 24)
  int count;

  /// Each one's edge.
  @Expose(min: 0.02, max: 1, step: 0.01, unit: 'm')
  double size;

  /// How long they last.
  @Expose(min: 0.1, max: 10, step: 0.1, unit: 's')
  double seconds;
}

/// The game's kinds of coin, made once per world.
class CoinKinds {
  CoinKinds()
      : coin = CoinKind(sound: Assets.audio.coin),
        big = CoinKind(
          value: 5,
          color: const Vec3(1, 0.42, 0.12),
          size: 1.6,
          sound: Assets.audio.coin,
          pitch: 0.7,
          sparks: Sparks(count: 12, size: 0.24, seconds: 1.6),
        );

  /// The level's coins.
  final CoinKind coin;

  /// The last coin, behind the gate.
  final CoinKind big;
}
