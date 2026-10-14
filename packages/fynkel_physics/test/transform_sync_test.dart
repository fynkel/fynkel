// Colliders and rigidbodies follow the object hierarchy (docs/hardening.md
// phase 2): a collider added or enabled after its body moved, a rigidbody
// under a rigidbody, a teleport after a rotation, and a character
// controller made inside a trigger.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

const step = 1 / 50;

World newWorld() {
  final world = World();
  addTearDown(world.dispose);
  return world;
}

/// Where along x a collider is: which of the points 0, 10 and 20 it
/// overlaps.
List<double> foundAt(World world) => [
  for (final x in [0.0, 10.0, 20.0])
    if (world.physics.overlapSphere(Vec3(x, 0, 0), 0.1).isNotEmpty) x,
];

void main() {
  group('P1: a collider added after its body moved is where its object is', () {
    test('on the rigidbody\'s own object', () {
      final world = newWorld();
      final o = world.add(GameObject(components: [Rigidbody(useGravity: false)]));
      world.update(step);
      o.position = const Vec3(10, 0, 0);
      o.addComponent(BoxCollider());
      expect(foundAt(world), [10.0]);
      world.update(step);
      expect(foundAt(world), [10.0]);
    });

    test('on a child of the rigidbody\'s object', () {
      final world = newWorld();
      final o = world.add(GameObject(components: [Rigidbody(useGravity: false)]));
      final child = o.add(GameObject(name: 'child'));
      world.update(step);
      o.position = const Vec3(10, 0, 0);
      child.addComponent(BoxCollider());
      expect(foundAt(world), [10.0]);
    });

    test('enabled again', () {
      final world = newWorld();
      final collider = BoxCollider();
      final o = world.add(GameObject(components: [collider, Rigidbody(useGravity: false)]));
      collider.enabled = false;
      world.update(step);
      o.position = const Vec3(10, 0, 0);
      collider.enabled = true;
      expect(foundAt(world), [10.0]);
    });

    test('on a kinematic body', () {
      final world = newWorld();
      final o = world.add(GameObject(components: [Rigidbody(isKinematic: true)]));
      world.update(step);
      o.position = const Vec3(10, 0, 0);
      o.addComponent(BoxCollider());
      expect(foundAt(world), [10.0]);
    });

    test('moved to the rigidbody above when its own is removed', () {
      final world = newWorld();
      final parent = world.add(GameObject(components: [Rigidbody(useGravity: false)]));
      final body = Rigidbody(useGravity: false);
      final child = parent.add(GameObject(name: 'child', components: [BoxCollider(), body]));
      world.update(step);
      parent.position = const Vec3(10, 0, 0);
      child.removeComponent(body);
      expect(foundAt(world), [10.0]);
    });
  });

  group('P2: a rigidbody under a rigidbody keeps its own motion', () {
    for (final interpolation in RigidbodyInterpolation.values) {
      test('${interpolation.name}: a child pushed sideways goes as far as a lone body', () {
        final world = newWorld();
        final lone = Rigidbody(interpolation: interpolation);
        world.add(
          GameObject(
            name: 'lone',
            transform: Transform(position: const Vec3(0, 0, 5)),
            components: [lone],
          ),
        );
        final parent = world.add(
          GameObject(
            name: 'parent',
            components: [Rigidbody(interpolation: interpolation)],
          ),
        );
        final child = Rigidbody(interpolation: interpolation);
        final childObject = parent.add(GameObject(name: 'child', components: [child]));
        lone.linearVelocity = const Vec3(5, 0, 0);
        child.linearVelocity = const Vec3(5, 0, 0);
        for (var i = 0; i < 50; i++) {
          world.update(step);
        }
        expect(lone.position.x, closeTo(5, 0.2));
        expect(child.position.x, closeTo(lone.position.x, 1e-3), reason: 'the body');
        expect(childObject.worldTransform.position.x, closeTo(lone.gameObject.position.x, 1e-3), reason: 'its object');
        expect(child.position.y, closeTo(lone.position.y, 1e-3), reason: 'it falls as the lone one does');
      });
    }

    test('a forearm under an upper arm swings at the elbow, and stays on it', () {
      final world = newWorld();
      CharacterJoint free({Rigidbody? to}) => CharacterJoint(
        anchor: const Vec3(0, 0.5, 0),
        axis: const Vec3(0, 1, 0),
        swingAxis: const Vec3(1, 0, 0),
        connectedBody: to,
        lowTwistLimit: SoftJointLimit(limit: -180.deg),
        highTwistLimit: SoftJointLimit(limit: 180.deg),
        swing1Limit: SoftJointLimit(limit: 180.deg),
        swing2Limit: SoftJointLimit(limit: 180.deg),
      );
      final upperBody = Rigidbody(interpolation: RigidbodyInterpolation.interpolate);
      final upper = world.add(
        GameObject(
          name: 'upper arm',
          transform: Transform(position: const Vec3(0, 3, 0)),
          components: [
            BoxCollider(size: const Vec3(0.15, 1, 0.15)),
            upperBody,
            free(),
          ],
        ),
      );
      final forearmBody = Rigidbody(interpolation: RigidbodyInterpolation.interpolate);
      final forearm = upper.add(
        GameObject(
          name: 'forearm',
          transform: Transform(position: const Vec3(0, -1, 0)),
          components: [
            BoxCollider(size: const Vec3(0.15, 1, 0.15)),
            forearmBody,
            free(to: upperBody),
          ],
        ),
      );
      world.update(step);
      forearmBody.addForceAtPosition(
        const Vec3(1, 0, 0),
        forearm.transformPoint(const Vec3(0, -0.5, 0)),
        mode: ForceMode.impulse,
      );
      var most = 0.0;
      for (var i = 0; i < 25; i++) {
        world.update(step);
        final bend = upper.worldTransform.rotation.angleTo(forearm.worldTransform.rotation);
        if (bend > most) most = bend;
        final elbow = upper.transformPoint(const Vec3(0, -0.5, 0));
        final top = forearm.transformPoint(const Vec3(0, 0.5, 0));
        expect((elbow - top).length, lessThan(0.05), reason: 'the forearm stays on the elbow');
      }
      expect(most, greaterThan(20.deg), reason: 'the elbow bends');
    });
  });
}
