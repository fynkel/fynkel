// A `transform` write in `update` shows in that frame (docs/foundation.md
// 1.2, ADR-0008 amendment 2026-09-27): what the frame draws is checked
// through the projection, which reads the frame's poses as the renderer
// does — on a frame that runs no fixed tick, as every other frame of a
// 120 Hz display does.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

/// Sets its object's x each update (the frame's), as a Unity `Update` would.
class UpdateMover extends Behaviour {
  double x = 2;

  @override
  void update(UpdateContext context) {
    x += 1;
    final p = transform.position;
    transform.position = Vec3(x, p.y, p.z);
  }
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-update-writes-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  /// The fixture's Main scene: the camera at (0, 2, 6) looking down -Z,
  /// fov 1 rad; Crate A (no body) at (2, 0.5, 0); Crate B a dynamic body.
  World loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(fixture, cacheDir: cache);
    w.frame(1 / 60);
    return w;
  }

  double completedX(World w, GameObject object) {
    final snapshot = w.native.snapshot;
    for (var i = 0; i < snapshot.recordCount; i++) {
      final record = SnapshotTransformRecord.decode(
        snapshot.records,
        i * SnapshotTransformRecord.wireSize,
        snapshot.tail,
      );
      if (record.entity == object.entity) return record.world.position.x;
    }
    throw StateError('Object missing from completed snapshot');
  }

  /// Screen x of a world point for the fixture camera in 800 × 600.
  double screenX(Vec3 p) {
    final halfH = (6.0 - p.z) * math.tan(0.5);
    final halfW = halfH * 800 / 600;
    return (p.x / halfW * 0.5 + 0.5) * 800;
  }

  // Failed before the amendment: the frame drew the value from before
  // update, ~183 px off on a frame with no tick.
  test('an update write shows in the same frame, also on a frame with no tick', () {
    final w = loaded();
    final crate = w.find('Crate A')!;
    final mover = crate.addBehaviour(UpdateMover());
    final anchor = w.projection.add(crate);
    w.projection.setViewport(800, 600);
    w.frame(1 / 60); // the behaviour starts and moves once

    for (var i = 0; i < 6; i++) {
      final plan = w.frame(1 / 240); // most of these run no tick
      w.projection.update();
      expect(
        anchor.x,
        closeTo(screenX(Vec3(mover.x, 0.5, 0)), 0.01),
        reason: 'frame $i (${plan.steps} ticks): the frame draws the x update wrote',
      );
      expect(crate.transform.position.x, mover.x, reason: 'and reads return it');
    }

    // Logical state includes this update; the completed snapshot remains at the tick.
    w.frame(1 / 60);
    expect(crate.worldTransform!.position.x, closeTo(mover.x, 1e-6));
    expect(completedX(w, crate), closeTo(mover.x - 1, 1e-6), reason: 'the tick ran before this update');
  });

  test("a dynamic body's update write teleports it at the next tick", () {
    final w = loaded();
    final body = w.find('Crate B')!;
    body.addBehaviour(UpdateMover()..x = 10);
    for (var i = 0; i < 3; i++) {
      w.frame(1 / 60);
    }
    expect(
      body.worldTransform!.position.x,
      closeTo(13, 1e-3),
      reason: 'logical gameplay includes the latest update',
    );
    expect(completedX(w, body), closeTo(12, 1e-3), reason: 'physics completed before the latest update');
    expect(body.bodyState(), isNotNull);
  });
}
