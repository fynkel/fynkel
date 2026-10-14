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

  /// A new round at [level].
  Future<void> at(int level) async {
    await game.restart(level: level);
    run(0.1);
  }

  /// Every sheep but [keep] far off in a corner, out of the way.
  void alone(Sheep keep) {
    for (final sheep in round().flock) {
      if (sheep != keep) sheep.position = Vec3(10, 0.4, 10);
    }
  }

  /// The middle of the sheep still in the field.
  Vec3 middle() {
    final free = round().free.toList();
    return free.fold(Vec3.zero, (sum, s) => sum + flat(s.position)) / free.length.toDouble();
  }

  /// The widest a sheep in the field is from [middle].
  double spread() => round().free.map((s) => flat(s.position - middle()).length).reduce((a, b) => a > b ? a : b);

  /// The wolf of a round at level 2, come in after the calm.
  Future<Wolf> firstWolf() async {
    await at(2);
    run(calm + 0.5);
    final wolf = world.all<Wolf>().single;
    dog().position = Vec3(-size + 1, 0.35, -size + 1);
    return wolf;
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

    test('a round starts with the flock together mid-field, the dog behind it, the clock full', () {
      run(0.1);
      expect(round().flock, hasLength(names.length));
      expect(round().flock.every((s) => s.isFree && s.has<WorldWidget>()), isTrue, reason: 'each with its name');
      expect(spread(), lessThan(3));
      expect(round().flock.every((s) => s.position.z < dog().position.z - 3), isTrue);
      expect((game.level.value, game.clock.value), (1, levelOf(1).seconds.ceil()));
      shot('day');
    });
  });

  group('the flock', () {
    test('a sheep walks away from the dog, faster the nearer it is, and settles once it backs off', () {
      run(0.1);
      final sheep = round().flock.first;
      alone(sheep);
      double pace(double far) {
        sheep.position = Vec3(0, 0.4, 0);
        sheep.velocity = Vec3.zero;
        dog().position = Vec3(0, 0.35, far);
        run(1);
        // Back as far again each frame would be fairer; the dog standing
        // still is near enough for a second.
        return -sheep.position.z;
      }

      final walking = pace(4), running = pace(1.5);
      expect(walking, inInclusiveRange(0.3, 1.5), reason: 'a walk, not a bolt');
      expect(running, greaterThan(walking * 1.5));
      dog().position = Vec3(10, 0.35, -10);
      run(2);
      final from = sheep.position;
      run(1);
      expect((sheep.position - from).length, lessThanOrEqualTo(Sheep.grazing + 1e-6), reason: 'grazing again');
    });

    test('the flock keeps together: driven from behind, it goes the way the dog goes, as one', () {
      run(0.1);
      final from = middle();
      // A step at a time towards a spot behind the flock, at a trot.
      for (var i = 0; i < 3 * 60; i++) {
        final to = flat(middle() + Vec3(0, 0, 3.5) - dog().position);
        if (to.length > 0.05) dog().position += to.normalized * (Dog.speed * 0.6 / 60);
        world.update(1 / 60);
      }
      expect(from.z - middle().z, greaterThan(3), reason: 'it went forward');
      expect((middle().x - from.x).abs(), lessThan(1.5), reason: 'straight');
      expect(spread(), lessThan(4), reason: 'together');
    });

    test('a sheep at the gate is drawn in: home for good, further in, without its name', () {
      run(0.1);
      final sheep = round().flock.first;
      alone(sheep);
      sheep.position = Vec3(1, 0.4, penFront + 1.5);
      dog().position = Vec3(10, 0.35, 10);
      run(3);
      expect(sheep.isHome, isTrue);
      expect(game.home.value, 1);
      expect(sheep.has<WorldWidget>(), isFalse);
      run(3);
      expect(inPen(sheep.position), isTrue);
      expect(sheep.position.z, lessThan(penFront - 2), reason: 'out of the gate');
    });

    test('a bark sends the sheep near it running', () {
      run(0.1);
      final sheep = round().flock.first;
      alone(sheep);
      dog().position = Vec3(0, 0.35, 8);
      sheep.position = Vec3(0, 0.4, 2.5);
      run(0.5);
      final from = sheep.position;
      expect((from.z - 2.5).abs(), lessThan(Sheep.grazing * 0.5 + 1e-6), reason: 'too far to mind the dog');
      dog().bark();
      run(0.5);
      expect(sheep.position.z, lessThan(from.z - 0.8));
    });
  });

  group('the wolves', () {
    test('level 1 has none; at level 2 the first comes after the calm, its mark out first', () async {
      run(calm + 15);
      expect(world.all<Wolf>(), isEmpty);
      await at(2);
      run(calm - warning - 0.5);
      expect(world.all<WolfMark>(), isEmpty);
      expect(game.coming.value, isFalse);
      run(1);
      final mark = world.all<WolfMark>().single, marked = mark.position;
      expect(game.coming.value, isTrue);
      run(warning);
      final wolf = world.all<Wolf>().single;
      expect(mark.isDestroyed, isTrue);
      expect(game.coming.value, isFalse);
      expect(flat(wolf.position - marked).length, lessThan(1), reason: 'where its mark was');
      expect(wolf.has<WorldWidget>(), isTrue, reason: 'with its nerve over it');
    });

    test('a sheep outruns a wolf; one it gets near, it lunges at and takes, then runs off', () async {
      final wolf = await firstWolf();
      final sheep = round().flock.first;
      alone(sheep);
      wolf.position = Vec3(0, 0.45, 6);
      sheep.position = Vec3(0, 0.4, 2.5);
      run(0.5);
      final first = flat(sheep.position - wolf.position).length;
      run(1.5);
      expect(flat(sheep.position - wolf.position).length, greaterThan(first + 0.5), reason: 'it runs faster');
      // Cornered against the fence, near it.
      sheep.position = Vec3(wolf.position.x, 0.4, wolf.position.z - 1.5);
      run(1.5);
      expect(sheep.isTaken, isTrue);
      expect(game.lost.value, 1);
      expect(wolf.isHunting, isFalse);
      run(5);
      expect(wolf.isDestroyed, isTrue, reason: 'off the field and gone');
    });

    test('a wolf keeps away from the dog; near it, its nerve wears down and it runs off with nothing', () async {
      final wolf = await firstWolf();
      final sheep = round().flock.first;
      alone(sheep);
      sheep.position = Vec3(0, 0.4, 0);
      wolf.position = Vec3(0, 0.45, 9);
      // The dog stands between, then keeps near the wolf.
      dog().position = Vec3(0, 0.35, 4.5);
      var nearest = double.infinity;
      for (var i = 0; i < 4 * 60 && wolf.isHunting; i++) {
        final far = flat(wolf.position - dog().position).length;
        if (far < nearest) nearest = far;
        if (far > Wolf.fear + 1) dog().position += flat(wolf.position - dog().position).normalized * (Dog.speed / 60);
        world.update(1 / 60);
      }
      expect(nearest, greaterThan(Wolf.fear - 0.5), reason: 'it never came close');
      expect(wolf.health.isDead, isTrue);
      expect(wolf.isHunting, isFalse);
      expect(wolf.has<WorldWidget>(), isFalse);
      expect(sheep.isFree, isTrue);
      expect(game.lost.value, 0);
    });

    test('one bark sends a wolf off; it never goes into the pen', () async {
      final wolf = await firstWolf();
      for (final sheep in round().flock) {
        sheep.position = Vec3(10, 0.4, 10);
      }
      dog().position = wolf.position + Vec3(4, -0.1, 0);
      dog().bark();
      run(0.1);
      expect(wolf.health.isDead, isTrue);
      expect(wolf.isHunting, isFalse);

      // A wolf at the gate, after a sheep at home: it stays out.
      expect(walk(Vec3(0, 0, penFront + 0.2), Vec3(0, 0, penFront - 0.5), intoPen: false), Vec3(0, 0, penFront + 0.2));
    });
  });

  group('the round', () {
    test('is over once every sheep is home or taken: the result is how many came home, and the wolves go', () async {
      await at(3);
      run(calm + 1);
      final flock = round().flock;
      for (final sheep in flock.take(5)) {
        sheep.position = Vec3(0, 0.4, penFront - 1);
      }
      run(0.1);
      expect(game.home.value, 5);
      expect(game.result.value, isNull);
      for (final sheep in flock.skip(5).where((s) => s.isFree)) {
        sheep.take();
      }
      run(0.1);
      expect(game.home.value + game.lost.value, names.length);
      expect(game.result.value, 5);
      expect(world.all<Wolf>().every((w) => !w.isHunting), isTrue);
      run(30);
      expect(world.all<Wolf>(), isEmpty, reason: 'and no more come');
    });

    test('is over when the clock runs out, the sheep left in the field neither home nor lost', () {
      run(30);
      expect(game.clock.value, closeTo(levelOf(1).seconds - 30, 1));
      expect(game.result.value, isNull);
      round().flock.first.position = Vec3(0, 0.4, penFront - 1);
      run(levelOf(1).seconds - 30 + 0.1);
      expect(game.clock.value, 0);
      expect(game.result.value, 1);
      expect(round().free, hasLength(names.length - 1));
    });

    test('stars: one for half the flock, two for six, three for all of it', () {
      expect([for (var home = 0; home <= 8; home++) starsFor(home)], [0, 0, 0, 0, 1, 1, 2, 2, 3]);
    });

    test('restart is a new round: the flock back, the counts at 0, the camera on the new dog', () async {
      run(0.1);
      final old = round();
      round().flock.first.take();
      run(0.5);
      await game.restart(level: 2);
      run(0.1);
      expect(old.isDestroyed, isTrue);
      expect((game.home.value, game.lost.value, game.result.value), (0, 0, null));
      expect((game.level.value, game.clock.value), (2, levelOf(2).seconds.ceil()));
      expect(round().free, hasLength(names.length));
      run(1);
      expect((world.camera!.gameObject.position - dog().position).length, lessThan(20));
      await game.restart();
      expect(game.level.value, 2, reason: 'the same level again');
    });
  });

  group('is fun', () {
    /// Plays a round at [level] with [seed] to the end, with a dog that
    /// only walks behind the flock (to the far side of its middle from the
    /// gate, round it rather than through it, at a trot) or with [noDog]
    /// one in a far corner; how many came home, and how many were lost.
    Future<(int, int)> play(int level, int seed, {bool noDog = false}) async {
      final world = World();
      addTearDown(world.dispose);
      world.input.simulate.resize(width.toDouble(), height.toDouble());
      world.theme = sheepdogTheme(Brightness.light);
      final game = world.add(SheepdogGame(seed: seed, level: level));
      world.update(1 / 60);
      final dog = game.round.dog;
      if (noDog) dog.position = Vec3(size - 0.5, 0.35, -size + 0.5);
      while (game.result.value == null) {
        final free = game.round.free.toList();
        if (!noDog && free.isNotEmpty) {
          final middle = free.fold(Vec3.zero, (sum, s) => sum + flat(s.position)) / free.length.toDouble();
          final behind = middle + (middle - Vec3(0, 0, penFront - 1)).normalized * 3.5;
          var to = flat(behind - dog.position);
          final fromMiddle = flat(dog.position - middle);
          if (fromMiddle.length < 3 && to.length > 0.5) to = to.normalized + fromMiddle.normalized * 1.5;
          if (to.length > 0.05) {
            dog.position = walk(dog.position, dog.position + to.normalized * (Dog.speed * 0.6 / 60));
          }
        }
        world.update(1 / 60);
      }
      return (game.home.value, game.lost.value);
    }

    test('a dog that only walks behind the flock brings six or more home, every round, at every level', () async {
      for (var level = 1; level <= 4; level++) {
        for (var seed = 1; seed <= 4; seed++) {
          final (home, _) = await play(level, seed);
          expect(home, greaterThanOrEqualTo(6), reason: 'level $level, seed $seed');
        }
      }
    });

    test('with no dog the wolves win: every one takes a sheep, and most rounds get no star', () async {
      var starless = 0;
      for (var seed = 1; seed <= 6; seed++) {
        final (home, lost) = await play(3, seed, noDog: true);
        expect(lost, greaterThan(0), reason: 'seed $seed');
        if (starsFor(home) == 0) starless++;
      }
      expect(starless, greaterThanOrEqualTo(4));
      final (home, _) = await play(1, 1, noDog: true);
      expect(home, 0, reason: 'nor do they walk home by themselves');
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
      sheep.position = dog().position + Vec3(0, 0, -5.5);
      world.input.simulate.keyDown(KeyCode.f);
      run(0.5);
      world.input.simulate.keyUp(KeyCode.f);
      expect(sheep.position.z, lessThan(dog().position.z - 6.3), reason: 'it heard the bark');
    });

    test('a key another control has swaps with it; reset goes back', () {
      final controls = Controls()..set('up', KeyCode.d);
      expect((controls['up'], controls['right']), (KeyCode.d, KeyCode.w));
      controls.reset();
      expect((controls['up'], controls['right']), (KeyCode.w, KeyCode.d));
    });
  });

  test("the game wears the app's theme: a day in the light one, a night in the dark", () async {
    run(0.1);
    final camera = world.camera!;
    world.theme = sheepdogTheme(Brightness.light);
    final day = (camera.clearColor, game.grass.color);
    world.theme = sheepdogTheme(Brightness.dark);
    final night = (camera.clearColor, game.grass.color);
    expect(night, isNot(day));
    expect(game.map.clearColor, camera.clearColor, reason: 'the minimap too');
    await at(2);
    run(calm + 1);
    shot('night');
  });
}
