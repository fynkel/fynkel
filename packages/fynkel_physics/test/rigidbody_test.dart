// Rigidbodies through the package (docs/stage-5.md §3.8, step 4): falling,
// stacking, tunnelling and frozen axes, the step between frames against
// the step in the fixed step, teleports, compound bodies, parents,
// interpolation, forces and materials.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

const step = 1 / 50;

World newWorld({bool betweenFrames = true}) {
  final world = World();
  addTearDown(world.dispose);
  world.physics.stepBetweenFrames = betweenFrames;
  return world;
}

/// A floor 100 × 100 whose top is at y 0.
GameObject floor(World world, {PhysicsMaterial material = PhysicsMaterial.standard}) => world.add(
  GameObject(
    name: 'floor',
    transform: Transform(position: const Vec3(0, -0.5, 0)),
    components: [BoxCollider(size: const Vec3(100, 1, 100), material: material)],
  ),
);

GameObject ball(World world, Vec3 at, {Rigidbody? body, double radius = 0.5, PhysicsMaterial? material}) => world.add(
  GameObject(
    name: 'ball',
    transform: Transform(position: at),
    components: [
      SphereCollider(radius: radius, material: material ?? PhysicsMaterial.standard),
      body ?? Rigidbody(),
    ],
  ),
);

GameObject cube(World world, Vec3 at, {Rigidbody? body}) => world.add(
  GameObject(
    name: 'cube',
    transform: Transform(position: at),
    components: [BoxCollider(), body ?? Rigidbody()],
  ),
);

void frames(World world, int count, [double dt = step]) {
  for (var i = 0; i < count; i++) {
    world.update(dt);
  }
}

Matcher near(Vec3 v, [double tolerance = 1e-3]) =>
    predicate<Vec3>((got) => (got - v).length < tolerance, 'within $tolerance of $v');

void main() {
  for (final betweenFrames in [false, true]) {
    group(betweenFrames ? 'between frames:' : 'in the fixed step:', () {
      test('a body dropped from 10 m lands within a step of √(2h/g) and rests', () {
        final world = newWorld(betweenFrames: betweenFrames);
        floor(world);
        final dropped = ball(world, const Vec3(0, 10.5, 0));
        final expected = math.sqrt(2 * 10 / 9.81);
        double? landed;
        for (var i = 1; i <= 200; i++) {
          world.update(step);
          // Between frames, a frame shows the step before its own.
          final shown = (betweenFrames ? i - 1 : i) * step;
          if (landed == null && dropped.position.y < 0.5 + 0.01) landed = shown;
        }
        expect(landed, closeTo(expected, step * 1.5));
        expect(dropped.position.y, closeTo(0.5, 0.02));
      });

      test('a stack of ten boxes stands for 10 s', () {
        final world = newWorld(betweenFrames: betweenFrames);
        floor(world);
        final boxes = [for (var i = 0; i < 10; i++) cube(world, Vec3(0, 0.5 + i, 0))];
        frames(world, 500);
        for (final (i, box) in boxes.indexed) {
          expect(box.position, near(Vec3(0, 0.5 + i, 0), 0.05), reason: 'box $i');
        }
      });
    });
  }

  test('a 0.1 m ball at 300 m/s with continuous detection doesn\'t pass a 0.1 m wall', () {
    // Physics sweeps any fast body against static colliders; continuous
    // detection sweeps it against moving ones too.
    for (final (moving, mode, passes) in [
      (false, CollisionDetectionMode.discrete, false),
      (true, CollisionDetectionMode.discrete, true),
      (true, CollisionDetectionMode.continuous, false),
    ]) {
      final world = newWorld();
      world.add(
        GameObject(
          transform: Transform(position: const Vec3(5, 0, 0)),
          components: [
            BoxCollider(size: const Vec3(0.1, 10, 10)),
            if (moving) Rigidbody(isKinematic: true),
          ],
        ),
      );
      final body = Rigidbody(useGravity: false, collisionDetectionMode: mode);
      final fast = ball(world, Vec3.zero, radius: 0.05, body: body);
      world.update(0);
      body.linearVelocity = const Vec3(300, 0, 0);
      frames(world, 20);
      expect(fast.position.x > 5, passes, reason: 'a moving wall $moving, ${mode.name}');
    }
  });

  test('frozen axes don\'t move', () {
    final world = newWorld();
    floor(world);
    final body = Rigidbody(constraints: RigidbodyConstraints.freezePositionX | RigidbodyConstraints.freezeRotation);
    final frozen = cube(world, const Vec3(0, 3, 0), body: body);
    world.update(0);
    body
      ..addForce(const Vec3(5, 0, 5), mode: ForceMode.impulse)
      ..addTorque(const Vec3(10, 10, 10), mode: ForceMode.impulse);
    frames(world, 100);
    expect(frozen.position.x, 0);
    expect(frozen.position.z, greaterThan(1));
    expect(frozen.position.y, closeTo(0.5, 0.02));
    expect(frozen.rotation, Quat.identity);
    expect(body.constraints.has(RigidbodyConstraints.freezeRotationY), isTrue);
    expect(body.constraints.has(RigidbodyConstraints.freezePositionY), isFalse);
  });

  test('the same input gives the same output, and between frames is the fixed step a frame later', () {
    List<Vec3> run(bool betweenFrames) {
      final world = newWorld(betweenFrames: betweenFrames);
      floor(world);
      final pile = [for (var i = 0; i < 8; i++) cube(world, Vec3(i * 0.3, 1 + i * 1.1, i * 0.2))];
      final seen = <Vec3>[];
      for (var i = 0; i < 120; i++) {
        world.update(step);
        seen.add(pile.last.position);
      }
      return seen;
    }

    final inline = run(false);
    expect(run(false), inline);
    final between = run(true);
    expect(between.skip(1).toList(), inline.take(inline.length - 1).toList());
  });

  test('a frame with two fixed steps runs the first in it and the second between frames', () {
    final inline = newWorld(betweenFrames: false);
    final between = newWorld();
    final a = ball(inline, const Vec3(0, 10, 0));
    final b = ball(between, const Vec3(0, 10, 0));
    inline.update(2 * step);
    between.update(2 * step);
    expect(b.position.y, lessThan(10), reason: 'the first step ran in the frame');
    expect(b.position.y, greaterThan(a.position.y), reason: 'the second runs after it');
    between.update(0);
    expect(b.position.y, a.position.y);
  });

  test('each fixed step sees the step before it done, and its forces go into its own step', () {
    List<double> run({required bool betweenFrames}) {
      final world = newWorld(betweenFrames: betweenFrames);
      final body = Rigidbody(useGravity: false);
      final pusher = _Pusher();
      world.add(
        GameObject(
          transform: Transform(position: const Vec3(0, 10, 0)),
          components: [SphereCollider(), body, pusher],
        ),
      );
      world.update(0);
      world.update(2 * step); // two fixed steps in one frame
      world.update(2 * step);
      world.update(0);
      return pusher.seen;
    }

    final inline = run(betweenFrames: false);
    expect(inline, [
      for (final steps in [0, 1, 3, 6]) closeTo(10 + steps * step, 1e-4),
    ]);
    expect(run(betweenFrames: true), inline);
  });

  test('what\'s asked between frames waits for the step, and sees where it left the body', () {
    final world = newWorld();
    final body = Rigidbody();
    final dropped = ball(world, const Vec3(0, 10, 0), body: body);
    world.update(step);
    final y = body.position.y; // the step started at the end of the frame
    expect(y, lessThan(10));
    world.update(0);
    expect(dropped.position.y, y);
    expect(world.physics.raycast(Ray(const Vec3(0, 20, 0), const Vec3(0, -1, 0)))!.point.y, closeTo(y + 0.5, 1e-4));
  });

  test('setting the transform teleports the body, and its velocity stays', () {
    for (final betweenFrames in [false, true]) {
      final world = newWorld(betweenFrames: betweenFrames);
      final body = Rigidbody(useGravity: false);
      final moved = ball(world, Vec3.zero, body: body);
      world.update(0);
      body.linearVelocity = const Vec3(1, 0, 0);
      frames(world, 10);
      moved.position = const Vec3(0, 50, 0);
      frames(world, 1);
      expect(body.position.y, 50, reason: 'between frames $betweenFrames');
      frames(world, 2);
      expect(moved.position.y, 50);
      expect(body.linearVelocity, near(const Vec3(1, 0, 0)));
      expect(moved.position.x, greaterThan(0));
    }
  });

  test('colliders on a rigidbody\'s object and below it are its shape, and go static when it goes', () {
    final world = newWorld();
    floor(world);
    final body = Rigidbody(mass: 2, useGravity: false);
    final left = BoxCollider();
    final right = SphereCollider();
    final dumbbell = world.add(
      GameObject(
        transform: Transform(position: const Vec3(0, 5, 0)),
        components: [body],
        children: [
          GameObject(
            transform: Transform(position: const Vec3(-2, 0, 0)),
            components: [left],
          ),
          GameObject(
            transform: Transform(position: const Vec3(2, 0, 0)),
            components: [right],
          ),
        ],
      ),
    );
    world.update(0);
    expect(left.attachedRigidbody, same(body));
    expect(right.attachedRigidbody, same(body));
    // The box (1 m³) outweighs the ball (0.52 m³): the center is nearer it.
    expect(body.centerOfMass.x, lessThan(-0.5));
    expect(body.worldCenterOfMass.y, closeTo(5, 1e-4));
    body.addForce(const Vec3(0, 2, 0), mode: ForceMode.impulse);
    frames(world, 10);
    expect(dumbbell.position.y, greaterThan(5));
    final ray = Ray(const Vec3(2, 20, 0), const Vec3(0, -1, 0));
    expect(world.physics.raycast(ray)!.collider, same(right), reason: 'it moves with the body');

    body.enabled = false;
    expect(left.attachedRigidbody, isNull);
    final y = dumbbell.position.y;
    frames(world, 10);
    expect(dumbbell.position.y, y, reason: 'nothing moves it now');
    expect(world.physics.raycast(ray)!.point.y, closeTo(y + 0.5, 1e-3));
  });

  test('a rigidbody under a parent puts its object where the body is, in the world', () {
    final world = newWorld();
    floor(world);
    final parent = world.add(
      GameObject(
        transform: Transform(position: const Vec3(10, 0, 0), scale: const Vec3(2, 2, 2)),
      ),
    );
    final child = parent.add(
      GameObject(
        transform: Transform(position: const Vec3(0, 2, 0)),
        components: [SphereCollider(), Rigidbody()],
      ),
    );
    frames(world, 150);
    final rest = child.worldTransform;
    // Scaled twice, the ball's radius is 1.
    expect(rest.position, near(const Vec3(10, 1, 0), 0.02));
    expect(child.localPosition.y, closeTo(0.5, 0.01), reason: 'in the parent\'s space');
    parent.position = const Vec3(20, 0, 0);
    frames(world, 2);
    expect(child.worldTransform.position.x, closeTo(20, 1e-3), reason: 'it went with its parent');
  });

  test('an interpolated body moves every frame, between where the steps left it', () {
    for (final betweenFrames in [false, true]) {
      final world = newWorld(betweenFrames: betweenFrames);
      final smooth = Rigidbody(useGravity: false, interpolation: RigidbodyInterpolation.interpolate);
      final stepped = Rigidbody(useGravity: false);
      final a = ball(world, Vec3.zero, body: smooth);
      final b = ball(world, const Vec3(0, 0, 5), body: stepped);
      world.update(0);
      smooth.linearVelocity = const Vec3(1, 0, 0);
      stepped.linearVelocity = const Vec3(1, 0, 0);
      frames(world, 10, 1 / 120);
      final (smoothXs, steppedXs) = (<double>[], <double>[]);
      for (var i = 0; i < 60; i++) {
        world.update(1 / 120);
        smoothXs.add(a.position.x);
        steppedXs.add(b.position.x);
      }
      final reason = 'between frames $betweenFrames';
      for (var i = 1; i < smoothXs.length; i++) {
        final move = smoothXs[i] - smoothXs[i - 1];
        expect(move, closeTo(1 / 120, 1e-3), reason: '$reason, frame $i');
      }
      expect(steppedXs.toSet().length, lessThan(30), reason: '$reason: it stands still between steps');
      expect(smoothXs.last, closeTo(steppedXs.last, step + 1e-3), reason: reason);
    }
  });

  test('a kinematic body moves where it\'s sent and pushes what\'s in the way; forces don\'t move it', () {
    final world = newWorld();
    final mover = Rigidbody(isKinematic: true);
    final pushed = Rigidbody(useGravity: false);
    final m = cube(world, Vec3.zero, body: mover);
    final p = cube(world, const Vec3(1.5, 0, 0), body: pushed);
    world.update(0);
    mover.addForce(const Vec3(100, 0, 0), mode: ForceMode.velocityChange);
    for (var i = 1; i <= 50; i++) {
      mover.movePosition(Vec3(i * 0.04, 0, 0));
      world.update(step);
    }
    world.update(0);
    expect(m.position, near(const Vec3(2, 0, 0)));
    expect(p.position.x, greaterThan(2.9));
    mover.moveRotation(Quat.axisAngle(Vec3.up, 0.5));
    frames(world, 2);
    expect(m.rotation, isNot(Quat.identity));
    expect(m.position, near(const Vec3(2, 0, 0)), reason: 'moveRotation keeps it where it was sent');
  });

  test('forces in Unity\'s four modes, torques, and an explosion', () {
    final world = newWorld();
    final bodies = [for (var i = 0; i < 4; i++) Rigidbody(mass: 2, useGravity: false, angularDamping: 0)];
    for (final (i, body) in bodies.indexed) {
      ball(world, Vec3(i * 3.0, 0, 0), body: body);
    }
    world.update(0);
    for (final (i, body) in bodies.indexed) {
      body.addForce(const Vec3(0, 0, 10), mode: ForceMode.values[i]);
    }
    world.update(step);
    world.update(0);
    final expected = [10 / 2 * step, 10 * step, 10 / 2, 10.0];
    for (final (i, body) in bodies.indexed) {
      expect(body.linearVelocity.z, closeTo(expected[i], 1e-4), reason: ForceMode.values[i].name);
    }
    bodies[0].addTorque(const Vec3(0, 3, 0), mode: ForceMode.velocityChange);
    bodies[1].addExplosionForce(10, const Vec3(3, 0, -2), 4, mode: ForceMode.velocityChange);
    bodies[2].addRelativeForce(const Vec3(1, 0, 0), mode: ForceMode.velocityChange);
    world.update(step);
    world.update(0);
    expect(bodies[0].angularVelocity.y, closeTo(3, 1e-4));
    expect(bodies[1].linearVelocity.z, greaterThan(10 * step + 1), reason: 'blown away from the explosion');
    expect(bodies[2].linearVelocity.x, closeTo(1, 1e-4));
  });

  test('a bouncy ball bounces, a frictionless box slides further, and gravity can change', () {
    final world = newWorld();
    floor(world, material: const PhysicsMaterial(dynamicFriction: 0, bounciness: 1));
    final bouncy = const PhysicsMaterial(bounciness: 1, bounceCombine: PhysicsMaterialCombine.maximum);
    final dead = ball(
      world,
      const Vec3(0, 5, 0),
      material: const PhysicsMaterial(bounceCombine: PhysicsMaterialCombine.minimum),
    );
    final live = ball(world, const Vec3(3, 5, 0), material: bouncy);
    var highest = 0.0;
    for (var i = 0; i < 150; i++) {
      world.update(step);
      if (i > 60) highest = math.max(highest, live.position.y);
    }
    expect(highest, greaterThan(3.5));
    expect(dead.position.y, closeTo(0.5, 0.02));

    // As in Unity, new gravity doesn't wake a sleeping body.
    world.physics.gravity = const Vec3(0, 9.81, 0);
    expect(world.physics.gravity, const Vec3(0, 9.81, 0));
    dead.get<Rigidbody>()!.wakeUp();
    frames(world, 50);
    expect(dead.position.y, greaterThan(2));
  });

  test('a resting body sleeps, wakes when pushed, and the settings come through', () {
    final world = newWorld();
    floor(world);
    final body = Rigidbody();
    cube(world, const Vec3(0, 0.5, 0), body: body);
    frames(world, 150);
    expect(body.isSleeping, isTrue);
    body.addForce(const Vec3(0, 1, 0), mode: ForceMode.impulse);
    expect(body.isSleeping, isFalse);
    body.sleep();
    expect(body.isSleeping, isTrue);
    body.wakeUp();
    expect(body.isSleeping, isFalse);

    final physics = world.physics;
    expect(physics.defaultSolverIterations, 6);
    expect(physics.sleepThreshold, closeTo(0.005, 1e-9));
    physics
      ..defaultSolverIterations = 10
      ..sleepThreshold = 0.1;
    expect(physics.defaultSolverIterations, 10);
    expect(physics.sleepThreshold, closeTo(0.1, 1e-7));
  });

  test('an object goes with its rigidbody and colliders in either order, hidden or destroyed', () {
    final world = newWorld();
    floor(world);
    final first = world.add(GameObject(components: [Rigidbody(), BoxCollider()]));
    final second = world.add(
      GameObject(
        transform: Transform(position: const Vec3(3, 0, 0)),
        components: [SphereCollider(), Rigidbody()],
      ),
    );
    frames(world, 2);
    final physics = world.physics;
    expect((physics.rigidbodyCount, physics.colliderCount), (2, 3));
    first.active = false;
    frames(world, 2);
    expect((physics.rigidbodyCount, physics.colliderCount), (1, 2));
    first.active = true;
    frames(world, 2);
    expect((physics.rigidbodyCount, physics.colliderCount), (2, 3));
    expect(first.get<Collider>()!.attachedRigidbody, same(first.get<Rigidbody>()));
    first.destroy();
    second.destroy();
    frames(world, 2);
    expect((physics.rigidbodyCount, physics.colliderCount), (0, 1));
  });

  test('bad values are refused, and a rigidbody out of the world says so', () {
    expect(() => Rigidbody(mass: 0), throwsArgumentError);
    expect(() => Rigidbody(linearDamping: -1), throwsArgumentError);
    expect(() => Rigidbody(constraints: const RigidbodyConstraints(1)), throwsArgumentError);
    expect(() => SphereCollider(material: const PhysicsMaterial(bounciness: 2)), throwsArgumentError);
    final world = newWorld();
    expect(() => world.physics.defaultSolverIterations = 0, throwsArgumentError);
    final body = Rigidbody();
    expect(() => body.position, throwsStateError);
    body.linearVelocity = const Vec3(1, 0, 0);
    expect(body.linearVelocity, const Vec3(1, 0, 0), reason: 'kept until it\'s in the world');
    final object = ball(world, Vec3.zero, body: body);
    world.update(0);
    expect(body.linearVelocity, near(const Vec3(1, 0, 0)), reason: 'it started at it');
    expect(() => body.mass = double.nan, throwsArgumentError);
    object.destroy();
    world.update(0);
    expect(() => body.addForce(Vec3.zero), throwsStateError);
    expect(world.physics.rigidbodyCount, 0);
  });
}

/// Pushes its body up by 1 m/s more in each fixed step, and keeps the
/// height it sees there.
final class _Pusher extends Behaviour {
  final seen = <double>[];

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    final body = gameObject.get<Rigidbody>()!;
    seen.add(body.position.y);
    body.addForce(const Vec3(0, 1, 0), mode: ForceMode.velocityChange);
  }
}
