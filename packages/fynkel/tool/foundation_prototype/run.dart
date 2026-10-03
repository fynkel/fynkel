import 'dart:convert';

import 'package:fynkel/advanced.dart' as engine;

import 'slice.dart';
import 'world.dart';

Map<String, Object?> slice(engine.NativeLibrary library) {
  final world = PrototypeWorld(library: library);
  try {
    world.register(healthType);
    world.register(doorType);
    final player = world.spawn('player');
    final door = world.spawn('door');
    final projectiles = List.generate(
      128,
      (index) => world.spawn('projectile-$index'),
    );
    final interaction = DoorBehaviour(player: player, door: door);
    world.addSystem(
      interaction.system,
    ); // Dependency resolution does not rely on registration order.
    world.addSystem(poisonSystem());
    world.initialize();
    world.transaction(
      writes: [healthType, doorType],
      run: (access) {
        access.object(player).set(healthType, const Health(poisonPerSecond: 2));
        access.object(door).set(doorType, const DoorState());
        for (final projectile in projectiles) {
          access.set(
            projectile,
            healthType,
            const Health(hp: 5, poisonPerSecond: 1),
          );
        }
      },
    );
    interaction.requested = true;
    world.frame(1 / 60);
    final health = world.save(player, healthType);
    final opened = world.save(door, doorType)['fields'] as Map<String, Object?>;
    if (opened['open'] != true) {
      throw StateError('Door failed to read post-poison Health');
    }
    world.edit(player, healthType, {'hp': 42.0});
    final authored = jsonDecode(
      jsonEncode(world.save(player, healthType)),
    ) as Map<String, Object?>;
    world.replaceScene();
    final replacement = world.spawn('replacement-player');
    world.initialize();
    world.load(replacement, healthType, authored);
    var retiredRejected = false;
    try {
      world.save(player, healthType);
    } on StateError {
      retiredRejected = true;
    }
    if (!retiredRejected) throw StateError('Retired reference was accepted');
    return {
      'population': 130,
      'order': world.systemOrder,
      'postPoisonPlayer': health,
      'doorOpened': true,
      'inspectorEditAndRoundTrip': world.save(replacement, healthType),
      'retiredReferenceRejected': retiredRejected,
      'limitations': [
        'No registered native custom-data extension yet',
        'No atomic built-in/custom edit',
        'No skeletal animation or runtime spawn during iteration',
      ],
    };
  } finally {
    world.dispose();
  }
}

Map<String, Object?> benchmark(engine.NativeLibrary library, int count) {
  final world = PrototypeWorld(library: library);
  try {
    world.register(healthType);
    final entities = List.generate(
      count,
      (index) => world.spawn('population-$index'),
    );
    world.initialize();
    world.transaction(
      writes: [healthType],
      run: (access) {
        for (final entity in entities) {
          access.set(entity, healthType, const Health(poisonPerSecond: 2));
        }
      },
    );
    final system = poisonSystem();
    const rounds = 120, dt = 1 / 60;
    // Warm the same transaction/query path, then reset before measuring.
    for (var step = 0; step < 20; step++) {
      world.transaction(
        writes: system.writes,
        run: (access) => system.run(access, dt),
      );
    }
    world.transaction(
      writes: [healthType],
      run: (access) {
        for (final entity in entities) {
          access.set(entity, healthType, const Health(poisonPerSecond: 2));
        }
      },
    );
    final writes = world.recordWrites;
    final times = <int>[];
    for (var step = 0; step < rounds; step++) {
      final clock = Stopwatch()..start();
      world.transaction(
        writes: system.writes,
        run: (access) => system.run(access, dt),
      );
      clock.stop();
      times.add(clock.elapsedMicroseconds);
    }
    var checksum = 0.0;
    world.transaction(
      reads: [healthType],
      run: (access) =>
          access.forEach(healthType, (_, health) => checksum += health.hp),
    );
    if ((checksum - count * 96).abs() > 0.0001 * count) {
      throw StateError('Incorrect benchmark output');
    }
    times.sort();
    return {
      'entities': count,
      'rounds': rounds,
      'recordWrites': world.recordWrites - writes,
      'p50Us': times[times.length ~/ 2],
      'p95Us': times[(times.length * .95).floor()],
      'checksum': checksum,
      'timedPath': 'Dart-owned immutable records, scoped dense query, validation, journal and atomic callback commit; excludes native frame/initialization',
      'bridgeCallsInsideTimedBulkLoop': 0,
      'allocationMeasurement':
          'Not captured; each changed record and journal entry can allocate',
    };
  } finally {
    world.dispose();
  }
}

void main() {
  final library = engine.NativeLibrary.open();
  try {
    print(
      const JsonEncoder.withIndent('  ').convert({
        'experiment':
            'F0.3 custom gameplay ownership and access, not a production API',
        'slice': slice(library),
        'dartStorage': [benchmark(library, 1000), benchmark(library, 10000)],
      }),
    );
  } finally {
    library.dispose();
  }
}
