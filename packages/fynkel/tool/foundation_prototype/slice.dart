import 'world.dart';

final class Health {
  const Health({this.hp = 100, this.poisonPerSecond = 0});
  final double hp, poisonPerSecond;
}

final healthType = ComponentType<Health>(
  id: 'prototype.Health',
  version: 1,
  fields: const [
    FieldSchema('hp', FieldKind.number),
    FieldSchema('poisonPerSecond', FieldKind.number),
  ],
  create: Health.new,
  encode: (value) => {'hp': value.hp, 'poisonPerSecond': value.poisonPerSecond},
  decode: (fields) => Health(
    hp: (fields['hp'] as num).toDouble(),
    poisonPerSecond: (fields['poisonPerSecond'] as num).toDouble(),
  ),
  validate: (value) {
    if (!value.hp.isFinite ||
        !value.poisonPerSecond.isFinite ||
        value.hp < 0 ||
        value.poisonPerSecond < 0) {
      throw ArgumentError('Health must be finite and nonnegative');
    }
  },
);

final class DoorState {
  const DoorState({this.open = false});
  final bool open;
}

final doorType = ComponentType<DoorState>(
  id: 'prototype.DoorState',
  version: 1,
  fields: const [FieldSchema('open', FieldKind.boolean)],
  create: DoorState.new,
  encode: (value) => {'open': value.open},
  decode: (fields) => DoorState(open: fields['open'] as bool),
  validate: (_) {},
);

SystemSpec poisonSystem() => SystemSpec(
  id: 'poison',
  writes: [healthType],
  run: (access, dt) => access.forEach(healthType, (entity, health) {
    if (health.poisonPerSecond == 0) return;
    final remaining = health.hp - health.poisonPerSecond * dt;
    access.set(
      entity,
      healthType,
      Health(
        hp: remaining < 0 ? 0 : remaining,
        poisonPerSecond: health.poisonPerSecond,
      ),
    );
  }),
);

/// Local interaction is scheduled like a system and reads the same Health record.
/// Prototype lifecycle adapter only; production Behaviour API is unchanged.
final class DoorBehaviour {
  DoorBehaviour({required this.player, required this.door});
  final EntityRef player, door;
  bool requested = false;

  SystemSpec get system => SystemSpec(
    id: 'door-interaction',
    after: ['poison'],
    reads: [healthType],
    writes: [doorType],
    run: (access, dt) {
      if (!requested) return;
      final healthy = access.object(player).get(healthType)!.hp > 0;
      access.object(door).set(doorType, DoorState(open: healthy));
      requested = false;
    },
  );
}
