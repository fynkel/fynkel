import 'dart:typed_data';

/// Opaque runtime identity. Scenes store authored UUIDs instead (ABI 2).
/// [world] is a u64 bit pattern; its high bit may make Dart's int negative.
final class EntityRef implements Comparable<EntityRef> {
  const EntityRef(this.world, this.slot, this.generation)
    : assert(slot >= 0 && slot <= 0xffffffff),
      assert(generation >= 0 && generation <= 0xffffffff);
  static const nil = EntityRef(0, 0, 0);
  // Transitional tick-command target, never a live ECS reference. F2 replaces
  // the old queued-command protocol with transaction-scoped tagged targets.
  const EntityRef.temp(int id) : world = 0, slot = id, generation = 0;
  final int world, slot, generation;
  bool get isNull => world == 0 && slot == 0 && generation == 0;
  bool get isLive => world != 0 && generation != 0;
  factory EntityRef.fromJson(Object? value) {
    if (value is! String || !RegExp(r'^[0-9a-fA-F]{32}$').hasMatch(value)) {
      throw FormatException(
        'Expected a 32-digit runtime entity reference',
        value,
      );
    }
    final high = int.parse(value.substring(16, 24), radix: 16);
    final low = int.parse(value.substring(24), radix: 16);
    return EntityRef(
      (high << 32) | low,
      int.parse(value.substring(8, 16), radix: 16),
      int.parse(value.substring(0, 8), radix: 16),
    );
  }
  String toJson() => toString();

  @override
  String toString() =>
      '${generation.toRadixString(16).padLeft(8, '0')}'
      '${slot.toRadixString(16).padLeft(8, '0')}'
      '${(world >>> 32).toRadixString(16).padLeft(8, '0')}'
      '${world.toUnsigned(32).toRadixString(16).padLeft(8, '0')}';
  @override
  bool operator ==(Object other) =>
      other is EntityRef &&
      world == other.world &&
      slot == other.slot &&
      generation == other.generation;
  @override
  int get hashCode => Object.hash(world, slot, generation);
  @override
  int compareTo(EntityRef other) {
    var comparison = (world >>> 32).compareTo(other.world >>> 32);
    if (comparison != 0) return comparison;
    comparison = world.toUnsigned(32).compareTo(other.world.toUnsigned(32));
    if (comparison != 0) return comparison;
    comparison = slot.compareTo(other.slot);
    return comparison != 0
        ? comparison
        : generation.compareTo(other.generation);
  }
}

void writeEntity(ByteData data, int offset, EntityRef entity) {
  if (entity.slot < 0 ||
      entity.slot > 0xffffffff ||
      entity.generation < 0 ||
      entity.generation > 0xffffffff) {
    throw FormatException('Entity reference field exceeds u32');
  }
  data.setUint64(offset, entity.world, Endian.little);
  data.setUint32(offset + 8, entity.slot, Endian.little);
  data.setUint32(offset + 12, entity.generation, Endian.little);
}

EntityRef readEntity(ByteData data, int offset) => EntityRef(
  data.getUint64(offset, Endian.little),
  data.getUint32(offset + 8, Endian.little),
  data.getUint32(offset + 12, Endian.little),
);
