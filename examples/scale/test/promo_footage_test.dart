// The promo film's footage of Scale: 10,000 objects on the beat, the
// camera circling them as the app's does, and one flying low over them.
// FYNKEL_FOOTAGE=<dir> flutter test test/promo_footage_test.dart

import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:scale/main.dart';

import 'promo_recorder.dart';

void main() {
  const timeout = Timeout(Duration(minutes: 10));

  Future<World> start() async {
    final world = World()..quality = Quality.high;
    addTearDown(world.dispose);
    world.add(Scale(objects: 10000));
    for (var i = 0; i < 120; i++) {
      world.update(1 / 60);
    }
    return world;
  }

  test('scale', () async {
    final world = await start();
    await record(world, 'scale', 8);
  }, skip: skipFootage, timeout: timeout);

  test('scale_low', () async {
    final world = await start();
    final camera = world.camera!.gameObject;
    final eye = camera.position;
    await record(world, 'scale_low', 8, after: (t) {
      final k = smooth(0, 8, t);
      final a = (10 + 30 * k) * math.pi / 180;
      final r = eye.flat.length * (0.55 - 0.15 * k);
      final at = Vec3(math.sin(a) * r, eye.y * (0.25 + 0.1 * k), math.cos(a) * r);
      camera
        ..position = at
        ..rotation = Quat.lookRotation(Vec3(0, 0, 0) - at);
    });
  }, skip: skipFootage, timeout: timeout);
}
