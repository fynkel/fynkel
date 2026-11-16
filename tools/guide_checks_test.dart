// Copied into the guide's app by tools/check_guide.py, beside the test it
// makes: that test runs each of the guide's objects for a second, and
// these check that a few of them do what the guide says they do. Each was
// copied into a game by a playtest (docs/playtests.md) and didn't.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:my_game/game.dart';
import 'package:my_game/player.dart';

World newWorld() {
  final world = World(source: (name) => File('assets/$name').readAsBytes());
  addTearDown(world.dispose);
  return world;
}

/// Where [object]'s model faces: glTF models face +Z, on their own axes.
Vec3 facing(GameObject object) => object.transform.rotation.rotate(const Vec3(0, 0, 1));

void main() {
  test("§13's Farmer faces the way it walks", () async {
    final world = newWorld();
    final farmer = world.add(Farmer());
    await farmer.loaded;
    farmer.walkTo(const Vec3(5, 0, 0));
    world.run(0.5);
    expect(farmer.position.x, greaterThan(0.1), reason: 'it walks');
    expect(facing(farmer).x, closeTo(1, 0.01), reason: 'its face is on its +Z: ${facing(farmer)}');
  });

  test("§13's Farmer and Hero play clips their character has", () async {
    final world = newWorld();
    final farmer = world.add(Farmer());
    final hero = world.add(Hero());
    await Future.wait([farmer.loaded, hero.loaded]);
    world.run(0.1);
    // An unknown clip throws, naming the ones there are.
    farmer.nod();
    hero.pickUp();
    world.run(0.5);
  });

  test("§16's Walk jumps on space", () async {
    final world = newWorld();
    world.add(
      GameObject(
        transform: Transform(position: const Vec3(0, -0.5, 0)),
        components: [BoxCollider(size: const Vec3(20, 1, 20))],
      ),
    );
    final walker = world.add(player());
    world.run(0.5);
    final standing = walker.position.y;
    world.input.simulate.keyDown(KeyCode.space);
    world.update(1 / 60);
    world.input.simulate.keyUp(KeyCode.space);
    var peak = standing;
    for (var frame = 0; frame < 60; frame++) {
      world.update(1 / 60);
      if (walker.position.y > peak) peak = walker.position.y;
    }
    // 5 m/s under 9.81 m/s²: 1.27 m.
    expect(peak - standing, greaterThan(1.1));
  });
}
