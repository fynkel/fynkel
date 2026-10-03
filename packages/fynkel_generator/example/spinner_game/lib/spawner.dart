/// Records (ADR-0027 §5): `Wave` and `Spread` are `@FynkelRecord` classes,
/// nested values with `@Expose` fields and no identity — stored as JSON
/// objects inside `game.Spawner`'s data, alone or in a list, and drawn by
/// the Inspector as foldouts.
library;

import 'package:fynkel/gameplay.dart';

/// Where a wave's enemies appear around the spawner.
@FynkelRecord()
class Spread {
  /// Distance from the spawner.
  @Expose(min: 0, unit: 'm')
  double radius = 2;

  /// On a circle, rather than anywhere inside it.
  @Expose()
  bool ring = false;
}

/// One wave of enemies.
@FynkelRecord()
class Wave {
  Wave({this.count = 3});

  /// What spawns.
  @Expose(kind: 'asset(prefab)')
  AssetRef? enemy;

  /// How many.
  @Expose(min: 1)
  int count;

  /// Seconds after the previous wave.
  @Expose(min: 0, unit: 's')
  double delay = 1;

  @Expose()
  Spread spread = Spread();
}

/// Spawns its waves in order, then the boss wave.
@FynkelBehaviour(id: 'game.Spawner', icon: 'group_add', category: 'Gameplay')
class Spawner extends Behaviour {
  @Expose()
  List<Wave> waves = [];

  /// The last wave.
  @Expose()
  Wave boss = Wave(count: 1);

  /// Enemies spawned so far (not authored).
  int spawned = 0;

  /// Every enemy the waves hold.
  int get total => waves.fold(boss.count, (n, w) => n + w.count);
}
