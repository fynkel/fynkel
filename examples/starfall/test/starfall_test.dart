import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:starfall/falling.dart';
import 'package:starfall/game.dart';

void main() {
  late World world;
  late StarfallGame game;

  setUp(() {
    // A phone's portrait view: the finger's point on the field depends on it.
    world = World()..input.simulate.resize(390, 760);
    game = world.add(StarfallGame(seed: 1));
    world.run(0.1);
  });
  tearDown(() => world.dispose());

  /// Drops [kind] just above the basket.
  Falling dropOnBasket(Kind kind) =>
      game.falling.get((thing) => thing.launch(kind, game.basket.position + Vec3(0, 1, 0), 4));

  test('nothing falls on the title, and Play starts it falling', () {
    world.run(2);
    expect(game.falling.countActive, 0);
    game.start();
    world.run(2);
    expect(game.phase.value, Phase.playing);
    expect(game.falling.countActive, greaterThan(0));
  });

  test('a star caught scores, a rock caught costs a life', () {
    game.start();
    dropOnBasket(Kind.star);
    world.run(0.4);
    expect(game.score.value, 1);
    dropOnBasket(Kind.rock);
    world.run(0.4);
    expect(game.lives.value, 2);
  });

  test('the basket follows a finger held on the screen', () {
    game.start();
    // The right side of the screen, near the bottom.
    world.input.simulate.fingerDown(1, 330, 600);
    world.run(0.1);
    expect(game.basket.position.x, greaterThan(2));
    world.input.simulate.fingerMove(1, 60, 600);
    world.run(0.1);
    expect(game.basket.position.x, lessThan(-2));
    world.input.simulate.fingerUp(1, 60, 600);
  });

  test('three lives lost ends it, and the best score is kept', () {
    game.start();
    for (var i = 0; i < 5; i++) {
      dropOnBasket(Kind.star);
      world.run(0.4);
    }
    for (var i = 0; i < 3; i++) {
      dropOnBasket(Kind.rock);
      world.run(0.4);
    }
    expect(game.phase.value, Phase.over);
    expect(game.best.value, greaterThanOrEqualTo(5));
  });

  test('a long game makes no new objects: they come back to their pools', () {
    game.start();
    game.lives.value = 1000; // a minute of play, never over
    world.run(30);
    expect(game.phase.value, Phase.playing);
    expect(game.sparkles.countAll, greaterThan(0), reason: 'some were caught');
    final made = game.falling.countAll + game.sparkles.countAll;
    world.run(30);
    expect(game.falling.countAll + game.sparkles.countAll, made);
  });

  test('a picture, mid-game', () {
    final view = HeadlessView(world, width: 390, height: 760);
    addTearDown(view.dispose);
    game.start();
    world.run(4);
    Directory('build').createSync();
    File('build/starfall.png').writeAsBytesSync(view.png());
  });
}
