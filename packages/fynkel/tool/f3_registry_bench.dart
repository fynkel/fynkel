import 'dart:convert';
import 'dart:developer' as dev;
import 'dart:isolate' as iso;

import 'package:fynkel/gameplay.dart';
import 'package:vm_service/vm_service.dart';
import 'package:vm_service/vm_service_io.dart';

final health = ComponentType<(double, bool)>(
  id: 'bench.Health',
  fields: const [
    ComponentField('hp', ComponentFieldKind.number, 100.0),
    ComponentField('invulnerable', ComponentFieldKind.boolean, false),
  ],
  encode: (v) => [v.$1, v.$2],
  decode: (v) => (v[0] as double, v[1] as bool),
);
Future<void> main(List<String> args) async {
  VmService? service;
  String? isolate;
  if (!args.contains('--aot')) {
    final info = await dev.Service.controlWebServer(
      enable: true,
      silenceOutput: true,
    );
    if (info.serverUri case final uri?) {
      service = await vmServiceConnectUri(
        uri.replace(scheme: 'ws', path: '${uri.path}ws').toString(),
      );
      isolate = dev.Service.getIsolateId(iso.Isolate.current);
    }
  }
  final results = <Map<String, Object?>>[];
  for (final count in [1, 1000, 10000, 50000]) {
    final w = World.create();
    final native = w.native;
    final schemaWatch = Stopwatch()..start();
    w.registerComponent(health);
    schemaWatch.stop();
    final registration = Stopwatch()..start();
    w.edit(
      () => List.generate(count, (_) {
        final p = w.spawn();
        p.addData(health);
        return p;
      }),
    );
    registration.stop();
    int bulkCalls = 0, bulkBytes = 0, bulkCopies = 0, bulkUs = 0;
    w.registerSystems([
      GameplaySystem(
        id: 'bench.Damage',
        phase: GameplayPhase.fixedUpdate,
        writes: ['bench.Health'],
        run: (_) {
          final bulkWatch = Stopwatch()..start();
          final calls = native.ffiCalls,
              bytes = native.dartBytesCopied,
              copies = native.dartCopies;
          w.updateComponents(
            health,
            (_, v) => (v.$1 == 0 ? 100.0 : v.$1 - 1, v.$2),
          );
          bulkUs = bulkWatch.elapsedMicroseconds;
          bulkCalls = native.ffiCalls - calls;
          bulkBytes = native.dartBytesCopied - bytes;
          bulkCopies = native.dartCopies - copies;
        },
      ),
    ]);
    for (var i = 0; i < 40; i++) {
      w.frame(w.fixedDt);
    }
    final samples = <int>[], bulkSamples = <int>[];
    final before = native.ffiCalls;
    for (var i = 0; i < 60; i++) {
      final watch = Stopwatch()..start();
      w.frame(w.fixedDt);
      samples.add(watch.elapsedMicroseconds);
      bulkSamples.add(bulkUs);
    }
    final callsPerFrame = (native.ffiCalls - before) / 60;
    int? countedObjects, countedBytes;
    if (service != null && isolate != null) {
      await service.getAllocationProfile(isolate, reset: true, gc: true);
      for (var i = 0; i < 10; i++) {
        w.frame(w.fixedDt);
      }
      final profile = await service.getAllocationProfile(isolate);
      countedObjects = 0;
      countedBytes = 0;
      for (final m in profile.members ?? <ClassHeapStats>[]) {
        countedObjects = countedObjects! + (m.instancesAccumulated ?? 0);
        countedBytes = countedBytes! + (m.accumulatedSize ?? 0);
      }
    }
    final rawSamples = List<int>.of(samples);
    samples.sort();
    bulkSamples.sort();
    if (bulkCalls != 2) {
      throw StateError(
        'population uses $bulkCalls calls: ${w.drainSystemFailures().map((e) => '${e.error} ${e.stack}').take(1).join()}',
      );
    }
    results.add({
      'entities': count,
      'schema_registration_us': schemaWatch.elapsedMicroseconds,
      'bulk_p50_us': bulkSamples[30],
      'bulk_p95_us': bulkSamples[57],
      'create_us': registration.elapsedMicroseconds,
      'full_frame_p50_us': samples[30],
      'full_frame_p95_us': samples[57],
      'full_frame_p99_us': samples[59],
      'samples_us': rawSamples,
      'ffi_calls_per_frame': callsPerFrame,
      'bulk_ffi_calls': bulkCalls,
      'bulk_buffer_copies': bulkCopies,
      'bulk_bytes_copied': bulkBytes,
      'vm_counted_objects_per_frame': countedObjects == null
          ? null
          : countedObjects / 10,
      'vm_counted_allocation_bytes_per_frame': countedBytes == null
          ? null
          : countedBytes / 10,
    });
    w.dispose();
  }
  print(
    jsonEncode({
      'mode': args.contains('--aot') ? 'aot' : 'jit',
      'samples': results,
      'allocation_scope': 'VM allocation counters include service overhead and may undercount optimized allocations; not a zero-allocation claim. Native heap allocations are not counted.',
      'timing_scope': 'Whole World.frame including F4 identity reconciliation, native tick and publication; 40 warmup + 60 measured frames.',
    }),
  );
  await service?.dispose();
  if (service != null) {
    await dev.Service.controlWebServer(enable: false, silenceOutput: true);
  }
}
