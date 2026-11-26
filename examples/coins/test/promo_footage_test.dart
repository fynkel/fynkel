// The promo film's footage of Coins: the player walks from coin to coin
// through the motes, plain and then in the toon style, and at dusk.
// FYNKEL_FOOTAGE=<dir> flutter test test/promo_footage_test.dart

import 'dart:io';
import 'dart:math' as math;

import 'package:coins/coins.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';

import 'promo_recorder.dart';

void main() {
  const timeout = Timeout(Duration(minutes: 10));

  Future<(World, CoinsGame)> start({bool dusk = false, bool toon = false}) async {
    final world = World(source: (asset) async => File('assets/$asset').readAsBytesSync())..quality = Quality.high;
    addTearDown(world.dispose);
    world.input.simulate.resize(1920, 1080);
    final game = world.add(CoinsGame());
    await game.loaded;
    if (dusk) await game.nextLevel();
    game.toon.value = toon;
    for (var i = 0; i < 30; i++) {
      world.update(1 / 60);
      await Future<void>.delayed(const Duration(milliseconds: 2));
    }
    return (world, game);
  }

  /// Walks to the nearest coin left, then the next.
  void Function(double) collect(World world, CoinsGame game) => (t) {
    final me = game.player.position;
    final coins = world.all<Coin>().where((c) => !c.isDestroyed).toList()
      ..sort((a, b) => (a.position - me).length.compareTo((b.position - me).length));
    if (coins.isNotEmpty) game.player.walkTo(coins.first.position);
  };

  Future<void> shot(String name, {bool dusk = false, bool toon = false, double seconds = 8}) async {
    final (world, game) = await start(dusk: dusk, toon: toon);
    final camera = world.camera!.gameObject;
    final op = Operator(camera);
    await record(world, name, seconds, before: collect(world, game), after: (t) {
      final me = game.player.position;
      final a = (25 + 50 * smooth(0, seconds, t)) * math.pi / 180;
      op.frame(me + Vec3(math.sin(a) * 6.5, 3.2, math.cos(a) * 6.5), me + const Vec3(0, 1, 0), ease: 0.05);
    });
  }

  test('coins_meadow', () => shot('coins_meadow'), skip: skipFootage, timeout: timeout);
  test('coins_toon', () => shot('coins_toon', toon: true), skip: skipFootage, timeout: timeout);
  test('coins_dusk', () => shot('coins_dusk', dusk: true), skip: skipFootage, timeout: timeout);
}
