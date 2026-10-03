// M3-16 (blueprint §15.4, C5): the frame's timers, the memory counters
// in the bridge stats, the subscription budget diagnostic and the load
// report's timing.

import 'dart:io';

import 'package:fynkel/gameplay.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

class Spinner extends Behaviour {
  int fixed = 0, updates = 0;
  @override
  void fixedUpdate(FixedUpdateContext context) {
    fixed++;
    // Some Dart work the behaviour timer must see.
    var x = 0.0;
    for (var i = 0; i < 20000; i++) {
      x += i * 0.5;
    }
    if (x < 0) throw StateError('unreachable');
  }

  @override
  void update(UpdateContext context) => updates++;
}

void main() {
  test('frameTimes split a frame into behaviours, native and presentation', () {
    final w = World.create(fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final spinner = Spinner();
    final go = w.spawn(name: 'spinner');
    go.addBehaviour(spinner);
    w.frame(1 / 60);
    w.frame(3 / 60);
    final t = w.frameTimes;
    expect(t.ticks, greaterThanOrEqualTo(2));
    expect(spinner.fixed, greaterThanOrEqualTo(2));
    expect(t.behavioursUs, greaterThan(0));
    expect(t.nativeUs, greaterThan(0));
    expect(t.totalUs, greaterThanOrEqualTo(t.behavioursUs + t.nativeUs + t.presentationUs));
    expect(identical(t, w.frameTimes), isTrue, reason: 'one object, overwritten per frame');
    expect(t.toString(), contains('ticks='));
  });

  test('stats carry the snapshot size and memory; the budget diagnostic fires once per crossing', () {
    final w = World.create(fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = false;
    addTearDown(w.dispose);
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    final objects = [for (var i = 0; i < 8; i++) w.spawn(name: 'e$i')];
    for (final o in objects) {
      o.localTransform; // read: it rides the snapshot (foundation 4.4)
    }
    w.frame(1 / 60);
    w.frame(1 / 60);
    var s = w.stats();
    expect(s.subscriptions, 8);
    expect(s.snapshotBytes, greaterThan(0));
    expect(s.snapshotPeakBytes, s.snapshotBytes);
    expect(s.snapshotBudget, 0);
    expect(s.nativeBytes, greaterThan(0), reason: 'the counting allocator');
    expect(s.residentBytes, greaterThan(0), reason: 'the resident set on Apple');
    expect(s.residencyBytes, 0, reason: 'nothing loaded');

    // A budget below the current snapshot: reported once, the snapshot
    // still whole, the objects still readable.
    w.snapshotBudget = s.snapshotBytes - 1;
    expect(w.snapshotBudget, s.snapshotBytes - 1);
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(seen.map((d) => d.code), [DiagnosticCode.snapshotBudget]);
    expect(seen.single.message, startsWith('FYN-R0013'));
    expect(objects.last.worldTransform, isNotNull);
    s = w.stats();
    expect(s.snapshotBudget, s.snapshotBytes - 1);
    seen.clear();

    // Under again (fewer subscriptions), then over: a second report.
    for (final o in objects.skip(2)) {
      o.destroy();
    }
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(seen, isEmpty);
    for (var i = 0; i < 8; i++) {
      w.spawn(name: 'again$i').localTransform;
    }
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(seen.map((d) => d.code), [DiagnosticCode.snapshotBudget]);
    seen.clear();
    w.snapshotBudget = 0;
    w.frame(1 / 60);
    expect(seen, isEmpty);
    expect(() => w.snapshotBudget = -1, throwsArgumentError);
  });

  test('a load report carries the native call time; the stats the retained assets', () {
    final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
    final cache = Directory.systemTemp.createTempSync('fynkel-instrumentation-').path;
    addTearDown(() => Directory(cache).deleteSync(recursive: true));
    cookProject(fixture, cacheDir: cache);
    final w = World.create(fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    final report = w.loadProject(fixture, cacheDir: cache);
    expect(report.ok, isTrue);
    expect(report.loadMs, greaterThan(0));
    expect(report.loadMs, lessThan(5000));
    expect(report.residency.retained, greaterThan(0));
    w.frame(1 / 60);
    // Payloads become resident on first use (a renderer, a mesh collider);
    // headless the fixture keeps none, so the count is what is checked.
    expect(w.stats().residencyBytes, report.residency.bytes);
  });
}
