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
}
