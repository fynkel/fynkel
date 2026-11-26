// The promo film's footage of the garden: the character walks among the
// trees from place to place, the camera circling with it.
// FYNKEL_FOOTAGE=<dir> flutter test test/promo_footage_test.dart

import 'dart:io';
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:garden/garden.dart';

import 'promo_recorder.dart';

void main() {
  test('garden', () async {
    final world = World(source: (asset) async => File('assets/$asset').readAsBytesSync())..quality = Quality.high;
    addTearDown(world.dispose);
    world.input.simulate.resize(1920, 1080);
    final garden = world.add(Garden());
    await garden.loaded;
    for (var i = 0; i < 30; i++) {
      world.update(1 / 60);
    }
    final character = world.first<Character>()!;
    final camera = world.camera!.gameObject;
    final op = Operator(camera);
    final stops = [const Vec3(2.5, 0, -1.5), const Vec3(-2, 0, -2.5), const Vec3(-1, 0, 2)];
    var next = 0;
    await record(world, 'garden', 9, before: (t) {
      if (t > 1 + next * 2.8 && next < stops.length) character.walkTo(stops[next++]);
    }, after: (t) {
      final me = character.position;
      final a = (-30 + 70 * smooth(0, 9, t)) * math.pi / 180;
      op.frame(me + Vec3(math.sin(a) * 8.5, 3.4, math.cos(a) * 8.5), me + const Vec3(0, 1.2, 0), ease: 0.04);
    });
  }, skip: skipFootage, timeout: const Timeout(Duration(minutes: 10)));
}
