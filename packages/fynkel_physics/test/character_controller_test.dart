// The character controller through the package (docs/stage-5.md, step 7):
// up steps below its step offset and not above, up slopes below its slope
// limit, along walls, grounded, pushing rigidbodies, and its callbacks.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

World newWorld() {
  final world = World();
  addTearDown(world.dispose);
  // The floor, its top at y = 0.
  world.add(
    GameObject(
      name: 'floor',
      transform: Transform(position: const Vec3(0, -0.5, 0)),
      components: [BoxCollider(size: const Vec3(100, 1, 100))],
    ),
  );
  return world;
}

/// A box [size] across with its middle at [middle], turned [degrees] about Z.
GameObject block(World world, Vec3 size, Vec3 middle, {double degrees = 0, String name = 'block'}) => world.add(
  GameObject(
    name: name,
    transform: Transform(position: middle, rotation: Quat.axisAngle(const Vec3(0, 0, 1), degrees * math.pi / 180)),
    components: [BoxCollider(size: size)],
  ),
);

/// A player 2 m tall standing at [x], [z], walking with [Walk].
GameObject player(World world, {double x = 0, double z = 0, Vec3 velocity = Vec3.zero}) => world.add(
  GameObject(
    name: 'player',
    transform: Transform(position: Vec3(x, 1.08, z)),
    components: [CharacterController(), Walk(velocity)],
  ),
);

/// Walks its object at [velocity], with gravity, as Unity's examples do.
class Walk extends Behaviour {
  Walk(this.velocity);

  Vec3 velocity;
  var _fall = 0.0;
  late CollisionFlags last;

  @override
  void onUpdate() {
    super.onUpdate();
    final controller = get<CharacterController>()!;
    if (controller.isGrounded) _fall = -1;
    _fall -= 9.81 * time.delta;
    last = controller.move((velocity + Vec3(0, _fall, 0)) * time.delta);
  }
}

void run(World world, double seconds) {
  for (var t = 0.0; t < seconds; t += 1 / 60) {
    world.update(1 / 60);
  }
}

/// How high [player]'s feet are.
double feet(GameObject player) => player.position.y - 1.08;

void main() {
  test('it walks up a step below its step offset, and not one above', () {
    for (final (height, climbs) in [(0.2, true), (0.28, true), (0.4, false)]) {
      final world = newWorld();
      block(world, Vec3(4, height, 4), Vec3(3, height / 2, 0));
      final walker = player(world, velocity: const Vec3(2, 0, 0));
      run(world, 2);
      if (climbs) {
        expect(walker.position.x, greaterThan(2), reason: 'a $height m step');
        expect(feet(walker), closeTo(height, 0.05), reason: 'a $height m step');
      } else {
        expect(walker.position.x, inInclusiveRange(0.3, 0.5), reason: 'a $height m step');
        expect(feet(walker), closeTo(0, 0.05), reason: 'a $height m step');
      }
    }
  });

  test('it walks up a slope below its slope limit, and not one above', () {
    for (final (degrees, climbs) in [(30.0, true), (60.0, false)]) {
      final world = newWorld();
      // A ramp rising along +X from x = 2, 10 m long, its bottom edge under the floor.
      final slope = degrees * math.pi / 180;
      final middle =
          const Vec3(2, 0, 0) +
          Vec3(math.cos(slope), math.sin(slope), 0) * 5 +
          Vec3(math.sin(slope), -math.cos(slope), 0) * 0.5;
      block(world, const Vec3(10, 1, 4), middle, degrees: degrees);
      final walker = player(world, velocity: const Vec3(2, 0, 0));
      run(world, 3);
      expect(feet(walker), climbs ? greaterThan(1) : lessThan(0.5), reason: 'a $degrees° slope');
    }
  });

  test('it slides along a wall and says it hit its sides', () {
    final world = newWorld();
    block(world, const Vec3(40, 4, 1), const Vec3(0, 2, 1.5));
    final walker = player(world, velocity: const Vec3(2, 0, 2));
    run(world, 2);
    expect(walker.position.x, greaterThan(3.5));
    expect(walker.position.z, inInclusiveRange(0.35, 0.5));
    final flags = walker.get<Walk>()!.last;
    expect(flags.has(CollisionFlags.sides), isTrue);
    expect(flags.has(CollisionFlags.below), isTrue);
    expect(flags.has(CollisionFlags.sides | CollisionFlags.below), isTrue);
    expect(flags.has(CollisionFlags.above), isFalse);
  });

  test('it knows when it is grounded, and how fast it went', () {
    final world = newWorld();
    final walker = player(world);
    final controller = walker.get<CharacterController>()!;
    run(world, 0.5);
    expect(controller.isGrounded, isTrue);
    walker.position = const Vec3(0, 5, 0);
    walker.get<Walk>()!.velocity = const Vec3(3, 0, 0);
    world.update(1 / 60);
    expect(controller.isGrounded, isFalse);
    expect(controller.collisionFlags, CollisionFlags.none);
    expect(controller.velocity.x, closeTo(3, 1e-3));
    run(world, 2);
    expect(controller.isGrounded, isTrue);
    expect(feet(walker), closeTo(0, 0.02));
    expect(controller.velocity.y, closeTo(0, 0.5), reason: 'on the ground, it doesn\'t fall');
  });

  test('simpleMove walks it along the ground and lets it fall off a ledge', () {
    final world = newWorld();
    // A ledge 2 m up, ending at x = 2.
    block(world, const Vec3(4, 2, 4), const Vec3(0, 1, 0), name: 'ledge');
    final walker = world.add(
      GameObject(
        transform: Transform(position: const Vec3(0, 3.08, 0)),
        components: [CharacterController()],
      ),
    );
    final controller = walker.get<CharacterController>()!;
    var grounded = <bool>[];
    for (var i = 0; i < 120; i++) {
      grounded.add(controller.simpleMove(const Vec3(2, 5, 0)));
      world.update(1 / 60);
    }
    expect(walker.position.x, greaterThan(3.5));
    expect(feet(walker), closeTo(0, 0.05), reason: 'it fell to the floor');
    expect(grounded.first || grounded[1], isTrue);
    expect(grounded.contains(false), isTrue, reason: 'it fell');
    expect(grounded.last, isTrue);
  });

  test('it pushes a rigidbody it walks into, unless it weighs nothing', () {
    for (final mass in [70.0, 0.0]) {
      final world = newWorld();
      final crate = world.add(
        GameObject(
          transform: Transform(position: const Vec3(2, 0.5, 0)),
          components: [BoxCollider(), Rigidbody()],
        ),
      );
      final walker = player(world, velocity: const Vec3(1.5, 0, 0));
      walker.get<CharacterController>()!.mass = mass;
      run(world, 3);
      if (mass > 0) {
        expect(crate.position.x, greaterThan(3), reason: 'pushed');
      } else {
        expect(crate.position.x, closeTo(2, 0.01), reason: 'not pushed');
        expect(walker.position.x, lessThan(1));
      }
    }
  });

  test('each move tells its object and behaviours what it hit, once each', () {
    final world = newWorld();
    block(world, const Vec3(40, 4, 1), const Vec3(0, 2, 1.5), name: 'wall');
    final hits = <String>[];
    final walker = world.add(
      Hitting(hits, components: [CharacterController(), HitBehaviour(hits), Walk(const Vec3(0, 0, 2))]),
    );
    run(world, 1);
    expect(hits, containsAll(['object floor', 'behaviour floor', 'object wall', 'behaviour wall']));
    hits.clear();
    world.update(1 / 60);
    expect(hits..sort(), ['behaviour floor', 'behaviour wall', 'object floor', 'object wall']);
    expect(walker.get<CharacterController>()!.collisionFlags.has(CollisionFlags.sides), isTrue);
  });

  test('a hit says where, which way, and what it hit', () {
    final world = newWorld();
    final crate = world.add(
      GameObject(
        name: 'crate',
        transform: Transform(position: const Vec3(2, 0.5, 0)),
        components: [BoxCollider(), Rigidbody()],
      ),
    );
    final seen = <ControllerColliderHit>[];
    final walker = world.add(
      Recorder(
        seen,
        transform: Transform(position: const Vec3(0, 1.08, 0)),
        components: [CharacterController()],
      ),
    );
    world.update(1 / 60);
    walker.get<CharacterController>()!.move(const Vec3(2, 0, 0));
    final hit = seen.singleWhere((h) => h.gameObject == crate);
    expect(hit.collider, crate.get<BoxCollider>());
    expect(hit.rigidbody, crate.get<Rigidbody>());
    expect(hit.controller, walker.get<CharacterController>());
    expect(hit.normal.x, closeTo(-1, 1e-3));
    expect(hit.point.x, closeTo(1.5, 0.01));
    expect(hit.moveDirection.x, closeTo(1, 1e-9));
    expect(hit.moveLength, closeTo(2, 1e-9));
  });

  test('it enters a trigger as a kinematic rigidbody would', () {
    final world = newWorld();
    final coin = world.add(
      Coin(
        transform: Transform(position: const Vec3(2, 1, 0)),
        components: [SphereCollider(isTrigger: true)],
      ),
    );
    player(world, velocity: const Vec3(2, 0, 0));
    run(world, 2);
    expect(coin.entered, 1);
  });

  test('bad settings are refused, and moving out of the world throws', () {
    expect(() => CharacterController(slopeLimit: -1), throwsArgumentError);
    expect(() => CharacterController(stepOffset: double.nan), throwsArgumentError);
    expect(() => CharacterController(skinWidth: 0), throwsArgumentError);
    expect(() => CharacterController(mass: -1), throwsArgumentError);
    expect(() => CharacterController(height: 0), throwsArgumentError);
    final controller = CharacterController();
    expect(() => controller.move(const Vec3(1, 0, 0)), throwsStateError);
  });
}

class Hitting extends GameObject with ControllerHits {
  Hitting(this.hits, {super.components}) : super(transform: Transform(position: const Vec3(0, 1.08, 0)));

  final List<String> hits;

  @override
  void onControllerColliderHit(ControllerColliderHit hit) => hits.add('object ${hit.gameObject.name}');
}

class HitBehaviour extends Behaviour with ControllerHits {
  HitBehaviour(this.hits);

  final List<String> hits;

  @override
  void onControllerColliderHit(ControllerColliderHit hit) => hits.add('behaviour ${hit.gameObject.name}');
}

class Recorder extends GameObject with ControllerHits {
  Recorder(this.seen, {super.transform, super.components});

  final List<ControllerColliderHit> seen;

  @override
  void onControllerColliderHit(ControllerColliderHit hit) => seen.add(hit);
}

class Coin extends GameObject with Triggers {
  Coin({super.transform, super.components});

  var entered = 0;

  @override
  void onTriggerEnter(Collider other) {
    if (other is CharacterController) entered++;
  }
}
