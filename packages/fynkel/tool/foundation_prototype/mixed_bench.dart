import 'dart:convert';

import 'mixed_world.dart';
import 'slice.dart';

Map<String, Object?> run(bool adapter, bool dense) {
  final world = MixedWorld(adapter: adapter);
  const count = 128;
  final refs = <MixedRef>[];
  try {
    world.transaction(
      writes: mixedFields,
      run: (access) {
        for (var i = 0; i < count; i++) {
          final ref = access.spawn('population-$i');
          refs.add(ref);
          access.object(ref).health = const Health(poisonPerSecond: 2);
        }
      },
    );
    final changed = dense ? count : 1;
    final samples = <int>[];
    late int calls, sent, received;
    for (var round = 0; round < 15; round++) {
      if (round == 5) {
        calls = world.calls;
        sent = world.requestBytes;
        received = world.responseBytes;
      }
      final watch = Stopwatch()..start();
      world.transaction(
        writes: ['Health'],
        run: (access) {
          for (final ref in refs.take(changed)) {
            access.object(ref).health = Health(
              hp: 100 - (round + 1) / 30,
              poisonPerSecond: 2,
            );
          }
        },
      );
      final publication = world.publish();
      if ((publication['render'] as List).length != count) {
        throw StateError('Lost extracted entity');
      }
      watch.stop();
      if (round >= 5) samples.add(watch.elapsedMicroseconds);
    }
    final measuredCalls = world.calls - calls;
    final requestBytes = world.requestBytes - sent,
        responseBytes = world.responseBytes - received;
    var checksum = 0.0;
    world.transaction(
      run: (access) =>
          access.forEach('Health', (object) => checksum += object.health!.hp),
    );
    if ((checksum - (count * 100 - changed * .5)).abs() > .0001) {
      throw StateError('Wrong full-frame checksum');
    }
    samples.sort();
    return {
      'strategy': adapter ? 'preflight-adapter' : 'shared-transaction-view',
      'entities': count,
      'changedPerFrame': changed,
      'measuredFrames': 10,
      'warmupFrames': 5,
      'p50Us': samples[5],
      'p95Us': samples[9],
      'timedFfiCalls': measuredCalls,
      'requestBytes': requestBytes,
      'responseBytes': responseBytes,
      'checksum': checksum,
      'workload': 'Dart JIT staging + native transaction + core tick + render extraction + JSON publication decode; excludes actual GPU/physics/assets',
      'limitation': 'Diagnostic JSON RPC includes per-record calls and full publication. Shipping bridge must use generated batched views/deltas.',
    };
  } finally {
    world.dispose();
  }
}

void main() => print(
  const JsonEncoder.withIndent('  ').convert({
    'bridgeFrames': [
      for (final dense in [false, true])
        for (final adapter in [false, true]) run(adapter, dense),
    ],
  }),
);
