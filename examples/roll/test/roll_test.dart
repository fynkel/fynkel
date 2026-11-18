import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:roll/game.dart';

void main() {
  late World world;
  late RollGame game;

  setUp(() {
    world = World();
    game = world.add(RollGame());
  });
  tearDown(() => world.dispose());

  test('the ball lands on the floor and stays there', () {
    world.run(2);
    expect(game.ball.position.y, closeTo(0.5, 0.05));
  });

  test('W rolls the ball away from the camera', () {
    world.run(1);
    final start = game.ball.position;
    world.input.simulate.keyDown(KeyCode.w);
    world.run(1);
    // The camera looks along -Z, so away from it is -Z.
    expect(game.ball.position.z, lessThan(start.z - 1));
  });

  test('rolling into a gem takes it, and the count goes down', () {
    world.run(1);
    expect(game.gemsLeft.value, 5);
    game.ball.position = RollGame.gemPlaces.first;
    world.run(0.2);
    expect(game.gemsLeft.value, 4);
  });

  test('a picture of the start', () {
    final view = HeadlessView(world, width: 800, height: 500);
    addTearDown(view.dispose);
    world.run(1);
    File('build/roll.png').writeAsBytesSync(view.png());
  });
}
