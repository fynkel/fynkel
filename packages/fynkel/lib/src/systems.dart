/// Per-system time (M3-10): what `World.systemTimes` decodes from
/// `FYNKEL_QUERY_SYSTEMS` — one entry per `World::step` phase, in tick
/// order, for the last step.
library;

import '../generated/schema.g.dart' as gen;

export '../generated/schema.g.dart' show SystemKind;

class SystemTime {
  const SystemTime({required this.system, required this.entities, required this.micros});

  factory SystemTime.fromRecord(gen.SystemTimeRecord r) =>
      SystemTime(system: r.system, entities: r.entities, micros: r.micros);

  /// The phase; `SystemKind.step` is the whole step.
  final gen.SystemKind system;

  /// What the phase touched in the last step: live entities for `step`,
  /// commands applied for `commands`, components ticked for a catalogue
  /// system, bodies + colliders + characters for `physics`, world
  /// matrices recomputed for `transforms`, snapshot records for `publish`.
  final int entities;

  /// Wall time of the phase in the last step, microseconds.
  final int micros;

  double get milliseconds => micros / 1000.0;

  @override
  String toString() => 'SystemTime(${system.name} $micros µs, $entities)';
}
