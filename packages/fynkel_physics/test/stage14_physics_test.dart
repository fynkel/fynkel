// Stage 14's physics rows (docs/stage-14-findings.md, 2b): a collider
// changed or added on a body that has moved stays on it (PH1, PH2), a
// collider turned off and on in a frame stays in its trigger (PH3),
// movePosition and moveRotation on a moving body change only what they
// say (PH4), and a joint whose other body goes says so (PH10).

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

GameObject floor(World world) => world.add(
  GameObject(
    name: 'floor',
    transform: Transform(position: const Vec3(0, -0.5, 0)),
    components: [BoxCollider(size: const Vec3(100, 1, 100))],
  ),
);

void frames(World world, int count, [double dt = step]) {
  for (var i = 0; i < count; i++) {
    world.update(dt);
  }
}

final class Zone extends Behaviour with Triggers {
  final inside = <Collider>{};
  final log = <String>[];
  @override
  void onTriggerEnter(Collider other) {
    log.add('enter');
    inside.add(other);
  }

  @override
  void onTriggerExit(Collider other) {
    log.add('exit');
    inside.remove(other);
  }
}

void main() {
  for (final betweenFrames in [false, true]) {
    final tag = betweenFrames ? 'between frames' : 'in the fixed step';

    test('PH1 ($tag): changing a resting ball\'s radius keeps its collider on it', () {
      final world = newWorld(betweenFrames: betweenFrames);
      floor(world);
      final collider = SphereCollider(radius: 0.5);
      final rb = Rigidbody();
      final ball = world.add(
        GameObject(
          name: 'ball',
          transform: Transform(position: const Vec3(0, 5, 0)),
          components: [collider, rb],
        ),
      );
      frames(world, 150);
      expect(ball.position.y, closeTo(0.5, 0.02), reason: 'rests on the floor first');
      collider.radius = 0.6;
      world.update(0);
      final found = world.physics.overlapSphere(ball.position, 0.05);
      frames(world, 50);
      final y = ball.position.y;
      expect(found, contains(collider), reason: 'the collider is where the ball is; 50 frames later y = $y');
      expect(ball.position.y, closeTo(0.6, 0.03), reason: 'rests on the floor with its new radius');
    });

    test('PH4 ($tag): moveRotation on a falling dynamic body keeps its position', () {
      final world = newWorld(betweenFrames: betweenFrames);
      final rb = Rigidbody();
      final o = world.add(
        GameObject(
          transform: Transform(position: const Vec3(3, 5, 0)),
          components: [BoxCollider(), rb],
        ),
      );
      frames(world, 4);
      rb.moveRotation(Quat.axisAngle(Vec3.up, 0.5));
      frames(world, 3);
      expect(o.position.x, closeTo(3, 0.01));
    });

    test('PH4b ($tag): movePosition on a turned dynamic body keeps its rotation', () {
      final world = newWorld(betweenFrames: betweenFrames);
      final rb = Rigidbody();
      final turned = Quat.axisAngle(Vec3.up, 0.785);
      final o = world.add(
        GameObject(
          transform: Transform(position: Vec3.zero, rotation: turned),
          components: [BoxCollider(), rb],
        ),
      );
      frames(world, 4);
      rb.movePosition(o.position + const Vec3(1, 0, 0));
      frames(world, 3);
      final d = (o.rotation.x - turned.x).abs() + (o.rotation.y - turned.y).abs() + (o.rotation.w - turned.w).abs();
      expect(d, lessThan(0.01), reason: 'rotation ${o.rotation} kept $turned');
    });

    test('PH3 ($tag): a collider turned off and on in one frame inside a trigger stays inside', () {
      final world = newWorld(betweenFrames: betweenFrames);
      final zone = Zone();
      world.add(
        GameObject(
          name: 'zone',
          components: [
            BoxCollider(size: const Vec3(4, 4, 4), isTrigger: true),
            zone,
          ],
        ),
      );
      final cube = world.add(
        GameObject(
          name: 'cube',
          components: [
            BoxCollider(size: const Vec3(0.5, 0.5, 0.5)),
            Rigidbody(useGravity: false),
          ],
        ),
      );
      frames(world, 10);
      expect(zone.inside, hasLength(1));
      zone.log.clear();
      cube.active = false;
      cube.active = true;
      frames(world, 10);
      expect(zone.inside, hasLength(1), reason: 'log after the toggle: ${zone.log}');
    });
  }

  test('PH2: a collider added mid-frame to an interpolated moving body sits on the body', () {
    final world = newWorld(betweenFrames: false);
    final rb = Rigidbody(useGravity: false, interpolation: RigidbodyInterpolation.interpolate);
    final o = world.add(GameObject(name: 'mover', components: [SphereCollider(radius: 0.1), rb]));
    world.update(0);
    rb.linearVelocity = const Vec3(10, 0, 0);
    // Find a frame between steps where the drawn pose trails the stepped one.
    var gap = 0.0;
    for (var i = 0; i < 40; i++) {
      world.update(1 / 120);
      gap = (rb.position.x - o.position.x).abs();
      if (gap > 0.05) break;
    }
    expect(gap, greaterThan(0.05), reason: 'drawn and stepped poses apart');
    final box = BoxCollider(size: const Vec3(1, 1, 1));
    o.addComponent(box);
    frames(world, 6, 1 / 120);
    world.update(0);
    // A ray from far ahead along -x hits the box's front face at body.x + 0.5.
    final hit = world.physics.raycast(Ray(Vec3(rb.position.x + 50, 0, 0), const Vec3(-1, 0, 0)), maxDistance: 100);
    expect(hit, isNotNull);
    expect(hit!.collider, same(box));
    expect(hit.point.x - 0.5, closeTo(rb.position.x, 0.01), reason: 'box offset from the body');
  });

  test('PH10: a joint whose connected body is destroyed holds its body to the world where it is', () {
    final world = newWorld();
    final joint = FixedJoint();
    final anchor = world.add(
      GameObject(
        name: 'anchor',
        transform: Transform(position: const Vec3(0, 2, 0)),
        components: [BoxCollider(), Rigidbody(isKinematic: true)],
      ),
    );
    final hung = world.add(
      GameObject(
        name: 'hung',
        transform: Transform(position: const Vec3(0, 0.5, 0)),
        components: [BoxCollider(), Rigidbody(), joint..connectedBody = anchor.get<Rigidbody>()],
      ),
    );
    frames(world, 3);
    expect(joint.isJoined, isTrue);
    final y = hung.position.y;
    anchor.destroy();
    frames(world, 50);
    expect(joint.connectedBody, isNull);
    expect(joint.isJoined, isTrue);
    expect(hung.position.y, closeTo(y, 0.01), reason: 'held where it was');
  });
}
