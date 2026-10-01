// Repeatable F2 callback-boundary qualification over the shipping SDK/native
// bridge. JIT host time includes reconciliation, journal and metadata commit;
// excludes rendering, assets, physics and behavioural scheduler dispatch.
import 'dart:convert';

import 'package:fynkel/fynkel.dart';

void main() {
  final results = <Map<String, Object>>[];
  for (final count in [128, 2048]) {
    for (final callbacks in [1, 32]) {
      final world = World.create();
      try {
        final objects = world.edit(() => List.generate(count, (i) => world.spawn(name: 'object-$i')));
        final native = world.native;
        final samples = <int>[];
        var calls = 0;
        for (var round = 0; round < 15; round++) {
          final before = native.ffiCalls;
          final watch = Stopwatch()..start();
          for (var callback = 0; callback < callbacks; callback++) {
            world.edit(() {
              objects[callback].transform.position = Vec3(round.toDouble(), 0, 0);
            });
          }
          watch.stop();
          if (round >= 5) {
            samples.add(watch.elapsedMicroseconds);
            calls += native.ffiCalls - before;
          }
        }
        for (final object in objects.take(callbacks)) {
          if (object.transform.position.x != 14) throw StateError('lost committed write');
        }
        samples.sort();
        results.add({
          'entities': count,
          'callbacksPerSample': callbacks,
          'warmupSamples': 5,
          'measuredSamples': 10,
          'p50Us': samples[5],
          'p95Us': samples[9],
          'ffiCallsPerSample': calls ~/ 10,
          'limitation': 'Entry reconciliation scans all live identities. F4 revision tracking remains required for sparse large worlds.',
        });
      } finally {
        world.dispose();
      }
    }
  }
  print(
    const JsonEncoder.withIndent('  ').convert({
      'workload': 'shipping SDK explicit callback-equivalent edits, transform writes, Dart JIT',
      'results': results,
    }),
  );
}
