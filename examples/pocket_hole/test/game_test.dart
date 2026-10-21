import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:pocket_hole/game.dart';

void main() {
  test('all patterns stay inside reachable board bounds', () {
    for (var level = 1; level <= 40; level++) {
      final points = layout(level);
      expect(points, isNotEmpty);
      for (final p in points) {
        expect(p.x.abs(), lessThan(2.55));
        expect(p.z.abs(), lessThan(5.2));
      }
    }
  });
  test('collect entire board, persist unlock, then start fresh next level', () {
    final world = World()..input.simulate.resize(430, 932);
    addTearDown(world.dispose);
    var unlocked = 0;
    final game = world.add(PocketGame(onCompleted: (n) => unlocked = n));
    world.run(.1);
    game.start();
    for (final p in layout(1)) {
      game.target = Vec3(p.x, 0, p.z);
      world.run(1.2);
    }
    expect(game.collected, game.total);
    expect(game.state, PlayState.cleared);
    expect(unlocked, 2);
    game.next();
    world.run(.1);
    expect(game.level, 2);
    expect(game.collected, 0);
    expect(game.state, PlayState.playing);
  });
  test('floor supports blocks but has an actual moving circular opening', () {
    final world = World()..input.simulate.resize(430, 932);
    addTearDown(world.dispose);
    final game = world.add(PocketGame());
    world.run(.1);
    game.start();
    world.run(3);
    expect(game.collected, 0);
    expect(game.blocks.every((b) => b.position.y > .1), isTrue);
    final down = Vec3(0, -1, 0);
    expect(game.physics.raycast(Ray(Vec3(0, 2, 4.5), down)), isNull);
    expect(game.physics.raycast(Ray(Vec3(1.2, 2, 4.5), down))?.gameObject, game.floor);
    game.target = Vec3(0, 0, 2.5);
    world.run(1);
    expect(game.physics.raycast(Ray(Vec3(0, 2, 2.5), down)), isNull);
    expect(game.physics.raycast(Ray(Vec3(0, 2, 4.5), down))?.gameObject, game.floor);
  });
  test('all four physical layouts can be swept clear', () {
    for (var level = 1; level <= 4; level++) {
      final world = World()..input.simulate.resize(430, 932);
      try {
        final game = world.add(PocketGame(level: level));
        world.run(.1);
        game.start();
        world.run(2);
        for (var pass = 0; pass < 3 && game.state == PlayState.playing; pass++) {
          for (final block in game.blocks) {
            if (block.done) continue;
            game.target = Vec3(block.position.x.clamp(-2.55, 2.55), 0, block.position.z.clamp(-5.2, 5.2));
            world.run(1.5);
          }
        }
        expect(game.state, PlayState.cleared, reason: 'level $level: ${game.collected}/${game.total}');
      } finally {
        world.dispose();
      }
    }
  });
  test('pause freezes movement and collection; restart clears progress', () {
    final world = World()..input.simulate.resize(430, 932);
    addTearDown(world.dispose);
    final game = world.add(PocketGame());
    world.run(.1);
    game.start();
    game.pause();
    final before = game.target;
    final positions = game.blocks.map((b) => b.position).toList();
    game.move(100, -100);
    world.run(2);
    expect(game.target, before);
    expect(game.elapsed, 0);
    expect(game.blocks.map((b) => b.position).toList(), positions);
    game.resume();
    game.move(100, -100);
    expect(game.hole.position, game.target);
    world.run(1 / 60);
    expect(game.floor.position, game.target);
    expect(game.target.x, 2.55);
    expect(game.target.z, -5.2);
    game.start();
    expect(game.collected, 0);
    expect(game.elapsed, 0);
  });
}
