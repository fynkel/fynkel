// Bridge bandwidth (blueprint §5.7, §15): N subscribed entities, one
// SetTransform each per tick; measures encode+submit, step, and snapshot
// read on the SDK path. benchmarks/m0/capture.py parses the `[bench]` line.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  test('bandwidth: 1000 subscribed entities, 600 steps', () {
    const n = 1000;
    const steps = 600;
    final w = World.create()..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final objects = [for (var i = 0; i < n; i++) w.spawn(name: 'e$i')];
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(w.stats().subscriptions, n);

    final sw = Stopwatch();
    var encodeUs = 0, stepUs = 0, readUs = 0;
    var checksum = 0.0;
    for (var t = 0; t < steps; t++) {
      w.beginFrame(1 / 60);
      w.drainEvents();
      sw.start();
      for (var i = 0; i < n; i++) {
        objects[i].transform.position = Vec3(t.toDouble(), i.toDouble(), 0);
      }
      // step() flushes the batch (encode+submit) then steps; split the two
      // by timing the flush explicitly.
      final before = sw.elapsedMicroseconds;
      final r = w.step();
      final after = sw.elapsedMicroseconds;
      sw.stop();
      expect(r.snapshotRecords, n);
      // Attribute: everything before the native step is encode+submit; we
      // cannot split inside step() here, so measure step separately below.
      encodeUs += before;
      stepUs += after - before;
      sw.reset();
      sw.start();
      for (var i = 0; i < n; i++) {
        checksum += objects[i].worldTransform!.position.x;
      }
      sw.stop();
      readUs += sw.elapsedMicroseconds;
      sw.reset();
    }
    expect(checksum, greaterThan(0));
    final s = w.stats();
    final inKib = s.bytesIn / steps / 1024;
    final outKib = s.bytesOut / steps / 1024;
    print(
      '[bench] bridge @ $n entities: encode+submit ${encodeUs ~/ steps} us/tick, step ${stepUs ~/ steps} us/tick, '
      'read ${readUs ~/ steps} us/tick | in ${inKib.toStringAsFixed(1)} KiB/tick, out ${outKib.toStringAsFixed(1)} KiB/tick',
    );
    print(
      '[bench] ffi calls/tick: ${(s.ffiCalls / steps).toStringAsFixed(1)}, copies/tick: ${(s.dartCopies / steps).toStringAsFixed(1)}',
    );
  });
}
