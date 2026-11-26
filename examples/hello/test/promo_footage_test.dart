// The promo film's footage of Hello: the cube, turning, as its code says.
// FYNKEL_FOOTAGE=<dir> flutter test test/promo_footage_test.dart

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:hello/main.dart';

import 'promo_recorder.dart';

void main() {
  test('hello', () async {
    final world = World()..quality = Quality.high;
    addTearDown(world.dispose);
    world.add(Hello());
    world.update(1 / 60);
    await record(world, 'hello', 8);
    // Square, for a card beside the code.
  }, skip: skipFootage, timeout: const Timeout(Duration(minutes: 10)));

  test('hello_square', () async {
    final world = World()..quality = Quality.high;
    addTearDown(world.dispose);
    world.add(Hello());
    world.update(1 / 60);
    await record(world, 'hello_square', 8, width: 1080, height: 1080);
  }, skip: skipFootage, timeout: const Timeout(Duration(minutes: 10)));
}
