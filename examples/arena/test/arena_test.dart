import 'dart:io';

import 'package:arena/arena.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show WorldWidget;

const width = 320.0, height = 200.0;

void main() {
  late World world;
  late ArenaGame game;
  setUp(() {
    world = World();
    world.input.simulate.resize(width, height);
    game = world.add(ArenaGame(seed: 1));
  });
  tearDown(() => world.dispose());

  Player player() => game.round.player;

  /// Wave 1's enemies, all in and grown.
  List<Enemy> firstWave() {
    world.run(1.5 + 3 * 0.5 + 0.5);
    return world.all<Enemy>().toList();
  }

  /// Puts [enemy] [distance] metres in front of the player, still.
  void ahead(Enemy enemy, double distance) {
    for (final other in world.all<Enemy>()) {
      other.position = Vec3(-8, 0.5, 8);
    }
    enemy.position = player().position + player().facing * distance;
  }

  test('a round starts with the player in the middle; wave 1 brings three enemies, one by one', () {
    world.run(0.1);
    expect(player().position, Vec3(0, 0.5, 0));
    expect(game.wave.value, 1);
    expect(world.all<Enemy>(), isEmpty, reason: 'a pause first');
    world.run(1.5);
    expect(world.all<Enemy>(), hasLength(1));
    world.run(1);
    expect(world.all<Enemy>(), hasLength(3));
    expect(world.all<Enemy>().every((e) => e.has<WorldWidget>()), isTrue, reason: 'each with its health bar');
    final shot = Platform.environment['ARENA_SHOT'];
    if (shot != null) {
      world.run(1);
      final view = HeadlessView(world, width: width.toInt(), height: height.toInt());
      addTearDown(view.dispose);
      File(shot).writeAsBytesSync(view.png());
    }
  });

  test('W, A, S, D move the player, inside the walls; it faces the way it goes', () {
    world.run(0.1);
    world.input.simulate.keyDown(KeyCode.d);
    world.run(0.5);
    expect(player().position.x, closeTo(Player.speed * 0.5, 0.1));
    expect(player().facing, Vec3.right);
    world.run(5);
    expect(player().position.x, size - 0.5);
    world.input.simulate
      ..keyUp(KeyCode.d)
      ..keyDown(KeyCode.w);
    world.run(0.2);
    expect(player().position.z, lessThan(0), reason: 'up is away from the camera, -Z');
  });

  test('a finger dragging alone is a stick', () {
    world.run(0.1);
    world.input.simulate.fingerDown(1, 100, 100);
    world.input.simulate.fingerMove(1, 100, 160);
    world.run(0.5);
    expect(player().position.z, closeTo(Player.speed * 0.5, 0.2), reason: 'down the view is +Z');
    expect(player().position.x, closeTo(0, 1e-9));
  });

  test('space dashes ahead; a dash through an enemy kills it, scores, and leaves sparks', () {
    final enemy = firstWave().first;
    ahead(enemy, 3);
    world.input.simulate.keyDown(KeyCode.space);
    world.run(0.1);
    world.input.simulate.keyUp(KeyCode.space);
    expect(player().dashing, isTrue);
    world.run(0.2);
    expect(player().dashing, isFalse);
    expect(player().position.z, closeTo(-5, 1e-6));
    expect(enemy.health.isDead, isTrue);
    expect(enemy.has<WorldWidget>(), isFalse, reason: 'its health bar goes as it dies');
    expect(game.score.value, 1);
    expect(world.all<Sparks>(), hasLength(1));
    world.run(0.6);
    expect(enemy.isDestroyed, isTrue);
    expect(world.all<Sparks>(), isEmpty);
  });

  test('a swipe dashes the way it went', () {
    world.run(0.1);
    world.input.simulate.fingerDown(1, 100, 100);
    for (var i = 1; i <= 6; i++) {
      world.input.simulate.fingerMove(1, 100 + 20.0 * i, 100);
      world.run(1 / 60);
    }
    world.input.simulate.fingerUp(1, 220, 100);
    world.run(0.4);
    expect(player().position.x, greaterThan(4.9), reason: 'a dash is 5 metres; the drag moved it a little too');
  });

  test('an enemy that reaches the player hurts it, once a second; the HUD and the camera hear', () {
    final enemy = firstWave().first;
    ahead(enemy, 0.5);
    world.run(1 / 60);
    expect(player().health.left, Player.hearts - 1);
    expect(game.health.value, Player.hearts - 1);
    expect(world.cameras.isShaking, isTrue);
    world.run(0.5);
    expect(player().health.left, Player.hearts - 1, reason: 'guarded for a second after a hit');
    world.run(0.6);
    expect(player().health.left, Player.hearts - 2);
  });

  test('pause stops the game\'s time and input: enemies stand still, a dash waits, keys go to Flutter', () {
    final enemy = firstWave().first;
    world.paused = true; // As the pause menu does.
    final at = enemy.position;
    world.run(1);
    expect(enemy.position, at);
    player().dash(Vec3.right);
    expect(player().dashing, isFalse);
    expect(world.input.simulate.keyDown(KeyCode.w), isFalse, reason: 'the pause menu gets every key');
    world.input.simulate.keyUp(KeyCode.w);
    world.paused = false;
    world.run(0.1);
    expect(enemy.position, isNot(at));
  });

  test('a tap dashes towards what it hits, through the sparks: they are on the effects layer', () {
    world.run(0.1);
    final target = Vec3(4, 0, 0);
    final at = world.camera!.worldToScreenPoint(target, width: width, height: height)!;
    // Sparks right where the tap goes, between the camera and the floor.
    final sparks = game.round.add(Sparks(Vec3(3, 1, 0)));
    world.run(1 / 60);
    world.tap(at.x, at.y);
    world.run(1 / 60);
    expect(player().dashing, isTrue);
    expect(player().facing.x, closeTo(1, 0.05));
    expect(sparks.layer, effects);
  });

  test('losing every heart ends the round; again starts wave 1 with full hearts', () async {
    final enemy = firstWave().first;
    ahead(enemy, 0.5);
    world.run(Player.hearts + 0.5);
    expect(player().health.isDead, isTrue);
    expect(game.over.value, isTrue);
    final score = game.score.value, old = game.round;
    expect(score, 0);
    await game.restart();
    expect(old.isDestroyed, isTrue);
    expect([game.over.value, game.health.value, game.score.value], [false, Player.hearts, 0]);
    world.run(0.1);
    expect(world.all<Enemy>(), isEmpty);
    expect(game.wave.value, 1);
  });
}
