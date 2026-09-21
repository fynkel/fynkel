// The C3 heap probe (M3-16): connects to the test VM's service (`flutter
// test --enable-vmservice`; the plain runner has none and the probe
// reports so), samples a window, sees the allocation and the GC it forces.

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  test('samples heap usage, allocation and GC over the VM service', () async {
    final probe = await DartHeapProbe.connect();
    if (probe == null) {
      // A VM without a service (a release build, a test runner started
      // without one): the view's stats simply stay null.
      return;
    }
    addTearDown(probe.close);
    expect(probe.isOpen, isTrue);
    final first = await probe.sample();
    expect(first, isNotNull);
    expect(first!.heapUsage, greaterThan(0));
    expect(first.heapCapacity, greaterThanOrEqualTo(first.heapUsage));

    // Allocate ≈ 40 MB in 256-int lists: the heap accounting sees it
    // (within the kilobyte the events round to), and the scavenger runs.
    var kept = <List<int>>[];
    for (var i = 0; i < 20000; i++) {
      kept.add(List<int>.filled(256, i));
      if (kept.length > 500) kept = [];
    }
    final second = await probe.sample();
    expect(second, isNotNull);
    expect(second!.allocatedBytes, greaterThan(30 << 20), reason: '$second');
    expect(second.allocatedBytes, lessThan(80 << 20), reason: '$second');
    expect(second.gcCollections, greaterThan(0), reason: '$second');
    expect(second.newSpaceCollections, greaterThan(0), reason: '$second');
    expect(second.gcReasons.values.fold(0, (a, b) => a + b), second.gcCollections);
    expect(second.gcPauseMs, greaterThan(0), reason: '$second');
    expect(second.toString(), contains('gc '));

    // A quiet window allocates little and needs no collection.
    final quiet = await probe.sample();
    expect(quiet!.allocatedBytes, lessThan(1 << 20), reason: '$quiet');
    await probe.close();
    expect(probe.isOpen, isFalse);
    expect(await probe.sample(), isNull);
  });
}
