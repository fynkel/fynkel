// The promo film's footage of Bowling: a pocket hit that strikes, from the
// game's chase camera and from down by the pins.
// FYNKEL_FOOTAGE=<dir> flutter test test/promo_footage_test.dart

import 'dart:math' as math;

import 'package:bowling/bowling.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';

import 'promo_recorder.dart';

void main() {
  const timeout = Timeout(Duration(minutes: 10));

  Future<(World, BowlingGame)> start() async {
    final world = World()..quality = Quality.high;
    addTearDown(world.dispose);
    world.input.simulate.resize(1920, 1080);
    final game = world.add(BowlingGame());
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60);
    }
    return (world, game);
  }

  void bowl(BowlingGame game, {double x = 0.04, double aim = 0.08, double speed = 8.5}) {
    game.aimX = x;
    game.roll(speed: speed, angle: math.atan2(aim - x, headPin));
  }

  test('bowling_chase', () async {
    final (world, game) = await start();
    var rolled = false;
    await record(world, 'bowling_chase', 6, before: (t) {
      if (!rolled && t > 0.4) {
        bowl(game);
        rolled = true;
      }
    });
  }, skip: skipFootage, timeout: timeout);

  test('bowling_pins', () async {
    final (world, game) = await start();
    bowl(game);
    final camera = world.camera!.gameObject;
    await record(world, 'bowling_pins', 5.5, after: (t) {
      final k = smooth(0, 5.5, t);
      final at = Vec3(0.42 - 0.1 * k, 0.55 + 0.25 * k, -headPin + 1.6 + 0.8 * k);
      camera
        ..position = at
        ..rotation = Quat.lookRotation(Vec3(-0.08, 0.15, -headPin - 0.4) - at);
    });
  }, skip: skipFootage, timeout: timeout);
}
