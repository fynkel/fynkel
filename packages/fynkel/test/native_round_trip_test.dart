// M0 backlog #11: one batched command buffer Dart -> Rust, one snapshot +
// event (+ diagnostics) buffer Rust -> Dart, through the ffigen bindings.
// Also measures bridge bandwidth and per-step cost at 1 000 subscribed
// entities (blueprint §5.7 "snapshot bandwidth is measurable").

import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  late FynkelNative native;
  setUpAll(() => native = FynkelNative.open(FynkelNative.defaultLibraryPath()));
  tearDownAll(() => native.dispose());

  test('spawn / subscribe / set / destroy round trip with diagnostics', () {
    final world = native.createWorld();
    addTearDown(world.dispose);
    final cmds = CommandBuffer();

    // Simulation lane: nothing is visible until step().
    cmds
      ..spawn(7, const Transform(position: [1, 2, 3]))
      ..spawn(8, const Transform(position: [4, 5, 6]));
    expect(world.submit(CommandLane.simulation, cmds), FynkelStatus.FYNKEL_STATUS_OK);
    expect(world.events.recordCount, 0);

    var r = world.step();
    expect(r.status, FynkelStatus.FYNKEL_STATUS_OK);
    expect((r.tick, r.applied, r.rejected, r.liveEntities), (1, 2, 0, 2));
    final spawned = world.events.events().where((e) => e.isSpawned).toList();
    expect(spawned.map((e) => e.a), [7, 8], reason: 'temp ids in submit order');
    final e7 = spawned[0].b, e8 = spawned[1].b;
    expect(e7, isNot(0));
    expect(e7, isNot(e8));

    // Subscription-based snapshot: only e7.
    expect(world.subscribeTransforms([e7]), FynkelStatus.FYNKEL_STATUS_OK);
    r = world.step();
    expect(r.snapshotRecords, 1);
    final snap = world.snapshot.transforms().single;
    expect(snap.entity, e7);
    expect(snap.transform.position, [1, 2, 3]);

    // Presentation lane applies immediately; simulation lane at next step.
    cmds
      ..clear()
      ..setTransform(e7, const Transform(position: [9, 9, 9]));
    expect(world.submit(CommandLane.presentation, cmds), FynkelStatus.FYNKEL_STATUS_OK);
    cmds
      ..clear()
      ..setTransform(e7, const Transform(position: [4, 4, 4]));
    expect(world.submit(CommandLane.simulation, cmds), FynkelStatus.FYNKEL_STATUS_OK);
    world.step();
    expect(world.snapshot.transforms().single.transform.position, [4, 4, 4]);

    // Destroy e8, then rejected commands produce indexed diagnostics.
    cmds
      ..clear()
      ..destroy(e8);
    world.submit(CommandLane.simulation, cmds);
    r = world.step();
    expect(world.events.events().single.isDestroyed, isTrue);
    cmds
      ..clear()
      ..setTransform(e7, const Transform())
      ..setTransform(e8, const Transform()) // stale
      ..spawn(0, const Transform()) // bad temp id
      ..setTransform(e7, const Transform(position: [double.nan, 0, 0])); // bad payload
    world.submit(CommandLane.simulation, cmds);
    r = world.step();
    expect(r.status, FynkelStatus.FYNKEL_STATUS_COMMANDS_REJECTED);
    expect((r.applied, r.rejected), (1, 3));
    final diags = world.diagnostics.diagnostics().toList();
    expect(diags.map((d) => d.commandIndex), [1, 2, 3]);
    expect(diags[0].code, 2, reason: 'STALE_ENTITY');
    expect(diags[0].entity, e8);
    expect(diags[0].message, 'set_transform: stale or unknown entity');
    expect(diags[1].code, 3, reason: 'INVALID_TEMP_ID');
    expect(diags[2].code, 4, reason: 'INVALID_PAYLOAD');
    expect(diags[2].component, 1, reason: 'TRANSFORM');
    expect(world.isPoisoned, isFalse);
  });

  test('protocol errors apply nothing', () {
    final world = native.createWorld();
    addTearDown(world.dispose);
    final cmds = CommandBuffer()..spawn(1, const Transform());
    final bytes = Uint8List.fromList(cmds.encoded());
    bytes[0] ^= 0xff; // corrupt magic
    expect(world.submitRaw(CommandLane.simulation, bytes), FynkelStatus.FYNKEL_STATUS_PROTOCOL);
    bytes[0] ^= 0xff;
    expect(world.submitRaw(CommandLane.simulation, bytes.sublist(0, bytes.length - 1)),
        FynkelStatus.FYNKEL_STATUS_PROTOCOL, reason: 'length mismatch');
    expect(world.step().liveEntities, 0, reason: 'nothing applied');
  });

  test('bandwidth: 1000 subscribed entities, 600 steps', () {
    const n = 1000, steps = 600;
    final world = native.createWorld(initialCapacity: n);
    addTearDown(world.dispose);
    final cmds = CommandBuffer(n);
    for (var i = 1; i <= n; i++) {
      cmds.spawn(i, Transform(position: [i.toDouble(), 0, 0]));
    }
    world.submit(CommandLane.simulation, cmds);
    world.step();
    final handles = world.events.events().map((e) => e.b).toList();
    expect(handles.length, n);
    world.subscribeTransforms(handles);

    // Steady state: every tick moves every entity (n SetTransform commands
    // in, n snapshot records out) — the "thousands of objects" shape.
    final sw = Stopwatch();
    var submitUs = 0, stepUs = 0, readUs = 0, bytesIn = 0, bytesOut = 0;
    var checksum = 0.0;
    for (var t = 0; t < steps; t++) {
      sw
        ..reset()
        ..start();
      cmds.clear();
      for (var i = 0; i < n; i++) {
        cmds.setTransform(handles[i], Transform(position: [i.toDouble(), t.toDouble(), 0]));
      }
      bytesIn += cmds.encoded().length;
      world.submit(CommandLane.simulation, cmds);
      submitUs += sw.elapsedMicroseconds;

      sw
        ..reset()
        ..start();
      final r = world.step();
      stepUs += sw.elapsedMicroseconds;
      expect(r.snapshotRecords, n);

      sw
        ..reset()
        ..start();
      final snap = world.snapshot;
      bytesOut += snap.byteLength;
      // Typical gameplay read: touch every record without allocating.
      for (var i = 0; i < n; i++) {
        checksum += snap.snapshotPositionX(i);
      }
      readUs += sw.elapsedMicroseconds;
    }
    expect(checksum, greaterThan(0));
    final line = 'bridge @ $n entities: encode+submit ${(submitUs / steps).toStringAsFixed(0)} us/tick, '
        'step ${(stepUs / steps).toStringAsFixed(0)} us/tick, read ${(readUs / steps).toStringAsFixed(0)} us/tick | '
        'in ${(bytesIn / steps / 1024).toStringAsFixed(1)} KiB/tick, out ${(bytesOut / steps / 1024).toStringAsFixed(1)} KiB/tick';
    stdout.writeln('[bench] $line');
  });
}
