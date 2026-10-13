import 'dart:io';

import 'package:flutter/material.dart' show Brightness;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show WorldTheme, WorldWidget;
import 'package:sheepdog/game.dart';
import 'package:sheepdog/palette.dart';

const width = 480, height = 320;

void main() {
  late World world;
  late SheepdogGame game;
  setUp(() {
    world = World();
    world.input.simulate.resize(width.toDouble(), height.toDouble());
    // As the app's light theme would: FynkelGame gives it from the app.
    world.theme = sheepdogTheme(Brightness.light);
    game = world.add(SheepdogGame(seed: 1));
  });
  tearDown(() => world.dispose());

  Round round() => game.round;
  Dog dog() => round().dog;

  /// [seconds] of frames at 60 a second.
  void run(double seconds) {
    for (var i = 0; i < (seconds * 60).round(); i++) {
      world.update(1 / 60);
    }
  }

  /// Every sheep but [keep] far off in a corner, out of the way.
  void alone(Sheep keep) {
    for (final sheep in round().flock) {
      if (sheep != keep) sheep.position = Vec3(10, 0.4, 10);
    }
  }

  /// With `SHEEPDOG_SHOT=<dir>`, a picture of the field as it is now, in
  /// [name].png there, to look at.
  void shot(String name) {
    final dir = Platform.environment['SHEEPDOG_SHOT'];
    if (dir == null) return;
    final view = HeadlessView(world, width: width, height: height);
    addTearDown(view.dispose);
    File('$dir/$name.png').writeAsBytesSync(view.png());
  }

  group('the field', () {
    test('a fence lets you into the pen only through its gate', () {
      final gate = Vec3(0, 0, penFront + 0.5), inside = Vec3(0, 0, penFront - 1);
      expect(walk(gate, inside), inside);
      expect(walk(gate, inside, intoPen: false), isNot(inside), reason: 'never, for a wolf');
      // From beside the pen, straight at its side: it stops at the fence.
      final beside = Vec3(penHalf + 0.5, 0, -8);
      expect(inPen(walk(beside, Vec3(penHalf - 1, 0, -8))), isFalse);
      // And out again only through the gate.
      expect(inPen(walk(Vec3(penHalf - 0.5, 0, -8), Vec3(penHalf + 1, 0, -8))), isTrue);
    });

    test('nothing leaves the field; along a fence it slides', () {
      expect(walk(Vec3(11, 0, 0), Vec3(14, 0, 1)), Vec3(size - 0.5, 0, 1));
    });

    test('a round starts with the flock in the field, the dog in front of it, and no wolves for a while', () {
      run(0.1);
      expect(round().flock, hasLength(names.length));
      expect(round().flock.every((s) => s.isFree && s.has<WorldWidget>()), isTrue, reason: 'each with its name');
      expect(dog().position.z, greaterThan(5));
      run(3.5);
      expect(world.all<Wolf>(), isEmpty);
      expect(game.wave.value, 0);
      shot('day');
    });
  });

  group('the flock', () {
    test('a sheep runs from the dog, and grazes slowly without it', () {
      run(0.1);
      final sheep = round().flock.first;
      alone(sheep);
      dog().position = Vec3(0, 0.35, 6);
      sheep.position = Vec3(0, 0.4, 4);
      run(0.5);
      expect(sheep.position.z, lessThan(3), reason: 'away from the dog, at a run');
      dog().position = Vec3(10, 0.35, -10);
      final from = sheep.position;
      run(1);
      expect((sheep.position - from).length, lessThanOrEqualTo(Sheep.grazing + 1e-6));
    });

    test('a sheep driven through the gate is home for good, further in, without its name', () {
      run(0.1);
      final sheep = round().flock.first;
      alone(sheep);
      sheep.position = Vec3(0, 0.4, penFront + 1.5);
      dog().position = Vec3(0, 0.35, penFront + 3.5);
      // The dog walks up behind it.
      world.input.simulate.keyDown(KeyCode.w);
      run(0.4);
      world.input.simulate.keyUp(KeyCode.w);
      run(0.6);
      expect(sheep.isHome, isTrue);
      expect(game.home.value, 1);
      expect(sheep.has<WorldWidget>(), isFalse);
      dog().position = Vec3(10, 0.35, 10);
      run(3);
      expect(inPen(sheep.position), isTrue);
      expect(sheep.position.z, lessThan(penFront - 2), reason: 'out of the gate');
    });

    test('a bark sends the sheep near it running', () {
      run(0.1);
      final sheep = round().flock.first;
      alone(sheep);
      dog().position = Vec3(0, 0.35, 8);
      sheep.position = Vec3(0, 0.4, 3);
      run(0.2);
      expect(sheep.position.z, closeTo(3, Sheep.grazing * 0.2 + 1e-6), reason: 'too far to mind the dog');
      dog().bark();
      run(0.4);
      expect(sheep.position.z, lessThan(2.2));
    });
  });

  group('the wolves', () {
    test('wave 1 is one wolf, after a pause; it goes for the nearest sheep, takes it and runs off', () {
      run(4.1);
      expect(game.wave.value, 1);
      final wolf = world.all<Wolf>().single;
      expect(wolf.has<WorldWidget>(), isTrue, reason: 'with its health bar');
      final sheep = round().flock.first;
      alone(sheep);
      dog().position = Vec3(-10, 0.35, -2);
      sheep.position = wolf.position + Vec3(0, 0, -3);
      run(0.5);
      expect(wolf.isHunting, isTrue);
      expect(sheep.isTaken, isFalse, reason: 'it runs, but not fast enough');
      run(5);
      expect(sheep.isTaken, isTrue);
      expect(game.lost.value, 1);
      expect(wolf.isHunting, isFalse);
      run(4);
      expect(wolf.isDestroyed, isTrue, reason: 'off the field and gone');
    });

    test('three barks send a wolf off with nothing; it never goes into the pen', () {
      run(4.5);
      final wolf = world.all<Wolf>().single;
      for (final sheep in round().flock) {
        sheep.position = Vec3(10, 0.4, 10);
      }
      run(0.5);
      wolf.position = dog().position + Vec3(0, 0.1, -2);
      for (var i = 0; i < 3; i++) {
        expect(wolf.isHunting, isTrue);
        wolf.position = dog().position + Vec3(0, 0.1, -2);
        dog().bark();
        run(0.6);
      }
      expect(wolf.health.isDead, isTrue);
      expect(wolf.isHunting, isFalse);
      expect(wolf.has<WorldWidget>(), isFalse);
      expect(game.lost.value, 0);

      // A wolf at the gate, after a sheep at home: it stays out.
      expect(walk(Vec3(0, 0, penFront + 0.2), Vec3(0, 0, penFront - 0.5), intoPen: false), Vec3(0, 0, penFront + 0.2));
    });
  });

  group('the round', () {
    test("is over once every sheep is home or taken: the result is how many came home, and the wolves go", () {
      run(4.5);
      final flock = round().flock;
      for (final sheep in flock.take(5)) {
        sheep.position = Vec3(0, 0.4, penFront - 1);
      }
      run(0.1);
      expect(game.home.value, 5);
      expect(game.result.value, isNull);
      for (final sheep in flock.skip(5)) {
        sheep.take();
      }
      run(0.1);
      expect(game.lost.value, 3);
      expect(game.result.value, 5);
      expect(world.all<Wolf>().every((w) => !w.isHunting), isTrue);
      run(10);
      expect(world.all<Wolf>(), isEmpty, reason: 'and no more come');
    });

    test('restart is a new round: the flock back, the counts at 0, the camera on the new dog', () async {
      run(0.1);
      final old = round();
      round().flock.first.take();
      run(0.5);
      await game.restart();
      run(0.1);
      expect(old.isDestroyed, isTrue);
      expect((game.home.value, game.lost.value, game.wave.value, game.result.value), (0, 0, 0, null));
      expect(round().free, hasLength(names.length));
      run(1);
      expect((world.camera!.gameObject.position - dog().position).length, lessThan(20));
    });
  });

  group('controls', () {
    test('the dog moves on the bound keys and the arrows, and barks on the bark key', () {
      run(0.1);
      final from = dog().position;
      world.input.simulate.keyDown(KeyCode.w);
      run(0.5);
      world.input.simulate.keyUp(KeyCode.w);
      expect(dog().position.z, closeTo(from.z - Dog.speed * 0.5, 0.2));
      game.controls.set('bark', KeyCode.f);
      run(0.1);
      final sheep = round().flock.first;
      alone(sheep);
      sheep.position = dog().position + Vec3(0, 0, -5);
      world.input.simulate.keyDown(KeyCode.f);
      run(0.3);
      world.input.simulate.keyUp(KeyCode.f);
      expect(sheep.position.z, lessThan(dog().position.z - 5.5), reason: 'it heard the bark');
    });

    test('a key another control has swaps with it; reset goes back', () {
      final controls = Controls()..set('up', KeyCode.d);
      expect((controls['up'], controls['right']), (KeyCode.d, KeyCode.w));
      controls.reset();
      expect((controls['up'], controls['right']), (KeyCode.w, KeyCode.d));
    });
  });

  test("the game wears the app's theme: a day in the light one, a night in the dark", () {
    run(0.1);
    final camera = world.camera!;
    world.theme = sheepdogTheme(Brightness.light);
    final day = (camera.clearColor, game.grass.color);
    world.theme = sheepdogTheme(Brightness.dark);
    final night = (camera.clearColor, game.grass.color);
    expect(night, isNot(day));
    expect(game.map.clearColor, camera.clearColor, reason: 'the minimap too');
    run(4.5);
    shot('night');
  });
}
