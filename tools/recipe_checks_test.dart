// Copied into the guide's app by tools/check_guide.py: the recipes
// (website/src/content/docs/recipes) do what they say.
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:my_game/recipes/blast.dart';
import 'package:my_game/recipes/bob.dart';
import 'package:my_game/recipes/chase.dart';
import 'package:my_game/recipes/countdown.dart';
import 'package:my_game/recipes/damage_number.dart';
import 'package:my_game/recipes/day.dart';
import 'package:my_game/recipes/first_person.dart';
import 'package:my_game/recipes/game_over.dart';
import 'package:my_game/recipes/health_bar.dart';
import 'package:my_game/recipes/hearts.dart';
import 'package:my_game/recipes/jump.dart';
import 'package:my_game/recipes/patrol.dart';
import 'package:my_game/recipes/pick_up.dart';
import 'package:my_game/recipes/shoot.dart';
import 'package:my_game/recipes/sliding_door.dart';
import 'package:my_game/recipes/slow_motion.dart';
import 'package:my_game/recipes/tap_to_move.dart';
import 'package:my_game/recipes/third_person.dart';
import 'package:my_game/recipes/top_down.dart';
import 'package:my_game/recipes/walk.dart';
import 'package:my_game/recipes/waves.dart';

World newWorld() {
  final world = World();
  addTearDown(world.dispose);
  return world;
}

GameObject thing(World world, [List<Component> components = const [], Vec3 at = Vec3.zero]) => world.add(
  GameObject(
    transform: Transform(position: at),
    components: components,
  ),
);

void ground(World world) => thing(world, [BoxCollider(size: const Vec3(40, 1, 40))], const Vec3(0, -0.5, 0));

void main() {
  test('W walks away from the camera, and faces that way', () {
    final world = newWorld();
    world.add(GameObject(transform: Transform.lookAt(const Vec3(8, 4, 0), Vec3.zero), components: [Camera()]));
    final walker = thing(world, [WalkByCamera()]);
    world.run(0.1);
    world.input.simulate.keyDown(KeyCode.w);
    world.run(1);
    expect(walker.position.x, lessThan(-3));
    expect(walker.position.z.abs(), lessThan(0.1));
    expect(walker.forward.x, closeTo(-1, 0.05));
  });

  test('jumps once, pressed just before landing too', () {
    final world = newWorld();
    ground(world);
    final one = world.add(jumper());
    world.run(0.5);
    final standing = one.position.y;
    double jump() {
      world.input.simulate.keyDown(KeyCode.space);
      world.run(1 / 30);
      world.input.simulate.keyUp(KeyCode.space);
      var peak = 0.0;
      for (var i = 0; i < 30; i++) {
        world.update(1 / 60);
        if (one.position.y - standing > peak) peak = one.position.y - standing;
      }
      return peak;
    }

    expect(jump(), inInclusiveRange(0.9, 1.5));
    world.run(1);
    expect(one.position.y, closeTo(standing, 0.05));
    expect(jump(), inInclusiveRange(0.9, 1.5));
  });

  test('patrols, chases, bobs', () {
    final world = newWorld();
    final guard = thing(world, [
      Patrol([const Vec3(2, 0, 0), const Vec3(-2, 0, 0)]),
    ]);
    final prey = thing(world, const [], const Vec3(5, 0, 5));
    final hunter = thing(world, [Chase(prey)], const Vec3(0, 0, 5));
    final coin = thing(world, [Bob()]);
    var right = 0.0, left = 0.0, high = 0.0;
    for (var i = 0; i < 300; i++) {
      world.update(1 / 60);
      right = guard.position.x > right ? guard.position.x : right;
      left = guard.position.x < left ? guard.position.x : left;
      high = coin.position.y > high ? coin.position.y : high;
    }
    expect([right, left], [closeTo(2, 0.02), closeTo(-2, 0.02)]);
    expect((prey.position - hunter.position).length, closeTo(1, 0.1));
    expect(high, closeTo(0.15, 0.01));
  });

  test('cameras look at the player', () {
    for (final make in [thirdPerson, topDown]) {
      final world = newWorld();
      final player = thing(world, const [], const Vec3(3, 0, 0));
      final camera = world.add(make(player));
      world.run(2);
      final to = (player.position + const Vec3(0, 0.75, 0) - camera.position).normalized;
      expect(camera.forward.dot(to), greaterThan(0.98));
    }
  });

  test('drag right looks right, drag up looks up', () {
    final world = newWorld();
    final camera = world.add(firstPerson(thing(world)));
    world.run(0.1);
    world.input.simulate.fingerDown(1, 100, 100);
    world.update(1 / 60);
    for (var i = 1; i <= 10; i++) {
      world.input.simulate.fingerMove(1, 100 + i * 10, 100 - i * 5);
      world.update(1 / 60);
    }
    world.input.simulate.fingerUp(1, 200, 50);
    world.run(0.2);
    expect(camera.forward.x, greaterThan(0.2));
    expect(camera.forward.y, greaterThan(0.1));
  });

  test('goes where the tap is', () {
    final world = newWorld();
    world.add(GameObject(transform: Transform.lookAt(const Vec3(0, 10, 0.01), Vec3.zero), components: [Camera()]));
    world.input.simulate.resize(800, 600);
    final one = thing(world, [TapToMove()]);
    world.run(0.1);
    final (x, y) = (world.input.width * 0.75, world.input.height / 2);
    world.input.simulate.fingerDown(1, x, y);
    world.update(1 / 60);
    world.input.simulate.fingerUp(1, x, y);
    world.run(3);
    expect(one.position.x, greaterThan(1));
  });

  test('shots take hearts, and the last one kills', () {
    final world = newWorld();
    final foe = world.add(enemy())..addComponent(CapsuleCollider());
    foe.position = const Vec3(0, 0, -5);
    thing(world, [Gun()]);
    world.run(0.1);
    world.input.simulate.keyDown(KeyCode.f);
    world.run(0.1);
    expect(foe.get<Hearts>()!.left.value, 4);
    world.run(2);
    expect(foe.isDestroyed, isTrue);
  });

  test('a blast throws bodies and hurts', () {
    final world = newWorld();
    ground(world);
    final box = thing(world, [BoxCollider(), Rigidbody(), Hearts(3)], const Vec3(2, 0.5, 0));
    final bomb = thing(world);
    world.run(0.2);
    blast(bomb);
    world.run(0.3);
    expect(box.position.x, greaterThan(2.3));
    expect(box.get<Hearts>()!.left.value, 1);
  });

  test('a wave comes, and the next when it is gone', () {
    final world = newWorld();
    final waves = world.add(Waves());
    world.run(2);
    final first = waves.children.single;
    expect(first.children, hasLength(3));
    for (final foe in [...first.children]) {
      foe.destroy();
    }
    world.run(4);
    expect(waves.children.single.name, 'wave 2');
  });

  test('a damage number goes by itself', () {
    final world = newWorld();
    final before = world.all<GameObject>().length;
    damageNumber(world, Vec3.zero, 3);
    world.run(0.2);
    expect(world.all<GameObject>().length, before + 1);
    world.run(1);
    expect(world.all<GameObject>().length, before);
  });

  test('counts down', () {
    final world = newWorld();
    final countdown = world.add(Countdown());
    var went = 0;
    countdown.on(countdown.go, (_) => went++);
    world.run(1.5);
    expect((countdown.text.value, went), ('2', 0));
    world.run(2);
    expect((countdown.text.value, went), ('', 1));
  });

  test('starts over', () {
    final world = newWorld();
    final game = world.add(RoundGame());
    final first = game.round;
    game.again();
    world.run(0.1);
    expect((first.isDestroyed, game.round.isDestroyed), (true, false));
  });

  test('a gem is taken once', () {
    final world = newWorld();
    ground(world);
    final walker = world.add(jumper());
    var taken = 0;
    final gem = world.add(Gem(onTaken: () => taken++));
    gem.position = walker.position;
    world.run(1);
    expect((taken, gem.isDestroyed), (1, true));
  });

  test('a door opens near, and shuts', () {
    final world = newWorld();
    final player = thing(world, const [], const Vec3(0, 0, 10));
    final door = world.add(SlidingDoor(player));
    world.run(1);
    expect(door.position.x, 0);
    player.position = const Vec3(0, 0, 2);
    world.run(1);
    expect(door.position.x, closeTo(1.2, 0.01));
    player.position = const Vec3(0, 0, 10);
    world.run(1);
    expect(door.position.x, 0);
  });

  test('noon is bright from above, midnight dark', () {
    final world = newWorld();
    final day = world.add(Day(seconds: 4));
    world.run(1);
    expect(day.get<Light>()!.intensity, greaterThan(1.8));
    expect(day.forward.y, lessThan(-0.9));
    world.run(2);
    expect(day.get<Light>()!.intensity, 0);
  });

  test('slow, and back', () {
    final world = newWorld();
    final slow = SlowMotion();
    thing(world, [slow]);
    world.run(0.1);
    slow.play();
    world.run(0.3);
    expect(world.time.scale, 0.2);
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60);
    }
    expect(world.time.scale, 1);
  });
}
