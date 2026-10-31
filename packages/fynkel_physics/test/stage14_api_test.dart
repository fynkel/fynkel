// Stage 14's pass over the physics API (docs/stage-14-audit/api/physics.md):
// a joint value that's refused leaves the joint as it was (P1), query lists
// are read-only hit or miss (P2), a joint with no rigidbody says so (P3),
// rigidbody errors name the call made (P4), a rigidbody's position works
// off the world (P5), bad centres and meshes say which collider (P8), the
// status of a PhysicsException is in the main library (P9), the value
// classes have copyWith (P10), and ignoreCollision's errors name the
// colliders (P13).

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:fynkel_physics/raw.dart' show RawPhysics;
import 'package:test/test.dart';

const step = 1 / 50;

World newWorld() {
  final world = World();
  addTearDown(world.dispose);
  return world;
}

void frames(World world, double seconds) {
  for (var i = 0; i < (seconds / step).round(); i++) {
    world.update(step);
  }
}

/// The errors [world] reports, as they come.
List<ObjectError> errorsOf(World world) {
  final errors = <ObjectError>[];
  final listening = world.errors.listen(errors.add);
  addTearDown(listening.cancel);
  return errors;
}

Matcher argumentErrorSaying(List<String> parts) => isA<ArgumentError>().having(
  (e) => e.toString(),
  'message',
  allOf([for (final p in parts) contains(p), isNot(contains('Instance of'))]),
);

Matcher stateErrorSaying(List<String> parts) =>
    isA<StateError>().having((e) => e.message, 'message', allOf([for (final p in parts) contains(p)]));

void main() {
  group('P1: a joint value that is refused leaves the joint as it was', () {
    test('connectedBody set to its own rigidbody throws, and a good one after joins', () {
      final world = newWorld();
      final rb = Rigidbody();
      final hinge = HingeJoint();
      world.add(GameObject(name: 'door', components: [BoxCollider(), rb, hinge]));
      final other = Rigidbody(isKinematic: true);
      world.add(
        GameObject(
          transform: Transform(position: const Vec3(3, 0, 0)),
          components: [BoxCollider(), other],
        ),
      );
      frames(world, 0.1);
      expect(hinge.isJoined, isTrue);
      expect(() => hinge.connectedBody = rb, throwsA(argumentErrorSaying(['connectedBody', 'own'])));
      expect(hinge.connectedBody, isNull);
      expect(hinge.isJoined, isTrue, reason: 'still joined to the world');
      hinge.connectedBody = other;
      frames(world, 0.1);
      expect(hinge.isJoined, isTrue);
      expect(hinge.connectedBody, same(other));
    });

    test('a NaN anchor, connected anchor or axis throws and changes nothing', () {
      final world = newWorld();
      final hinge = HingeJoint();
      final ball = CharacterJoint();
      world.add(GameObject(components: [BoxCollider(), Rigidbody(), hinge]));
      world.add(
        GameObject(
          transform: Transform(position: const Vec3(3, 0, 0)),
          components: [BoxCollider(), Rigidbody(), ball],
        ),
      );
      frames(world, 0.1);
      const nan = Vec3(double.nan, 0, 0);
      expect(() => hinge.anchor = nan, throwsA(argumentErrorSaying(['HingeJoint.anchor', 'NaN'])));
      expect(() => hinge.connectedAnchor = nan, throwsA(argumentErrorSaying(['HingeJoint.connectedAnchor'])));
      expect(() => hinge.axis = nan, throwsA(argumentErrorSaying(['HingeJoint.axis'])));
      expect(() => hinge.axis = Vec3.zero, throwsA(argumentErrorSaying(['HingeJoint.axis'])));
      expect(() => ball.axis = nan, throwsA(argumentErrorSaying(['CharacterJoint.axis'])));
      expect(() => ball.swingAxis = nan, throwsA(argumentErrorSaying(['CharacterJoint.swingAxis'])));
      expect(hinge.anchor, Vec3.zero);
      expect(hinge.autoConfigureConnectedAnchor, isTrue);
      expect(hinge.axis, const Vec3(1, 0, 0));
      expect(ball.axis, const Vec3(1, 0, 0));
      expect(ball.swingAxis, const Vec3(0, 1, 0));
      expect(hinge.isJoined, isTrue);
      expect(ball.isJoined, isTrue);
      hinge.anchor = const Vec3(0.5, 0, 0);
      frames(world, 0.1);
      expect(hinge.isJoined, isTrue);
      expect(hinge.anchor, const Vec3(0.5, 0, 0));
    });

    test('a NaN anchor or axis in the constructor throws', () {
      const nan = Vec3(double.nan, 0, 0);
      expect(() => FixedJoint(anchor: nan), throwsA(argumentErrorSaying(['FixedJoint.anchor'])));
      expect(() => SpringJoint(connectedAnchor: nan), throwsA(argumentErrorSaying(['SpringJoint.connectedAnchor'])));
      expect(() => HingeJoint(axis: nan), throwsA(argumentErrorSaying(['HingeJoint.axis'])));
      expect(() => CharacterJoint(swingAxis: nan), throwsA(argumentErrorSaying(['CharacterJoint.swingAxis'])));
    });

    test('a joint made to its own rigidbody says so once, and joins once fixed', () async {
      final world = newWorld();
      final errors = errorsOf(world);
      final rb = Rigidbody();
      final o = world.add(GameObject(name: 'thing', components: [BoxCollider()]));
      final hinge = o.addComponent(HingeJoint(connectedBody: rb));
      o.addComponent(rb);
      frames(world, 0.5);
      await pumpEventQueue();
      expect(hinge.isJoined, isFalse);
      expect(errors, hasLength(1));
      expect('${errors.single.error}', allOf(contains('HingeJoint'), contains('thing'), contains('connectedBody')));
      expect(world.physics.rigidbodyCount, 1);
      hinge.connectedBody = null;
      frames(world, 0.1);
      await pumpEventQueue();
      expect(hinge.isJoined, isTrue);
      expect(errors, hasLength(1));
    });
  });

  group('P2: query lists are read-only, hit or miss', () {
    test('casts and overlaps', () {
      final world = newWorld();
      world.add(GameObject(components: [BoxCollider()]));
      final physics = world.physics;
      const down = Vec3(0, -1, 0);
      final lists = <String, List<Object>>{
        'raycastAll hit': physics.raycastAll(Ray(const Vec3(0, 5, 0), down)),
        'raycastAll miss': physics.raycastAll(Ray(const Vec3(9, 5, 0), down)),
        'sphereCastAll hit': physics.sphereCastAll(Ray(const Vec3(0, 5, 0), down), 0.2),
        'overlapSphere hit': physics.overlapSphere(Vec3.zero, 1),
        'overlapSphere miss': physics.overlapSphere(const Vec3(9, 0, 0), 1),
        'overlapBox hit': physics.overlapBox(Vec3.zero, Vec3.one),
      };
      for (final MapEntry(key: name, value: list) in lists.entries) {
        expect(list.isNotEmpty, !name.endsWith('miss'), reason: name);
        expect(() => list.removeWhere((_) => true), throwsUnsupportedError, reason: name);
        expect(() => list.sort((a, b) => 0), throwsUnsupportedError, reason: name);
      }
      // A copy can change.
      final sorted = physics.raycastAll(Ray(const Vec3(0, 5, 0), down)).toList()
        ..sort((a, b) => b.distance.compareTo(a.distance));
      expect(sorted, hasLength(1));
    });
  });

  group('P3: a joint on an object with no rigidbody', () {
    test('says so once, and joins once one is added', () async {
      final world = newWorld();
      final errors = errorsOf(world);
      final hinge = HingeJoint();
      final door = world.add(GameObject(name: 'door', components: [BoxCollider(), hinge]));
      frames(world, 1);
      await pumpEventQueue();
      expect(hinge.isJoined, isFalse);
      expect(errors, hasLength(1));
      expect(errors.single.gameObject, same(door));
      expect(
        '${errors.single.error}',
        allOf(contains('HingeJoint'), contains('door'), contains('Put a Rigidbody on door')),
      );
      door.addComponent(Rigidbody());
      frames(world, 0.1);
      await pumpEventQueue();
      expect(hinge.isJoined, isTrue);
      expect(errors, hasLength(1));
    });

    test('listed before its rigidbody, it says nothing', () async {
      final world = newWorld();
      final errors = errorsOf(world);
      final hinge = HingeJoint(axis: Vec3.up);
      world.add(GameObject(components: [BoxCollider(), hinge, Rigidbody()]));
      frames(world, 0.5);
      await pumpEventQueue();
      expect(hinge.isJoined, isTrue);
      expect(errors, isEmpty);
    });
  });

  group('P4: rigidbody errors name the call made', () {
    test('addRelativeForce and addRelativeTorque before it is added', () {
      final rb = Rigidbody();
      GameObject(components: [rb]);
      expect(() => rb.addRelativeForce(Vec3.up), throwsA(stateErrorSaying(['Rigidbody.addRelativeForce'])));
      expect(() => rb.addRelativeTorque(Vec3.up), throwsA(stateErrorSaying(['Rigidbody.addRelativeTorque'])));
    });
  });

  group('P5: a rigidbody\'s position and rotation off the world are its object\'s', () {
    test('before it is added', () {
      final rb = Rigidbody();
      final o = GameObject(
        transform: Transform(position: const Vec3(1, 2, 3)),
        components: [SphereCollider(), rb],
      );
      expect(rb.position, const Vec3(1, 2, 3));
      expect(rb.rotation, Quat.identity);
      rb.position = const Vec3(4, 5, 6);
      final turn = Quat.axisAngle(Vec3.up, 0.5);
      rb.rotation = turn;
      expect(o.position, const Vec3(4, 5, 6));
      expect(o.rotation, turn);
      final world = newWorld();
      world.physics.gravity = Vec3.zero;
      world.add(o);
      frames(world, 0.1);
      expect((rb.position - const Vec3(4, 5, 6)).length, lessThan(1e-4));
    });

    test('while disabled, as a pooled body is', () {
      final world = newWorld();
      world.physics.gravity = Vec3.zero;
      final rb = Rigidbody();
      final o = world.add(GameObject(components: [SphereCollider(), rb]));
      frames(world, 0.1);
      rb.enabled = false;
      expect(rb.position, o.position);
      rb.position = const Vec3(0, 7, 0);
      expect(o.position, const Vec3(0, 7, 0));
      rb.enabled = true;
      frames(world, 0.1);
      expect((rb.position - const Vec3(0, 7, 0)).length, lessThan(1e-4));
    });
  });

  group('P8: bad values say which collider', () {
    test('a NaN center throws where it is given', () {
      const nan = Vec3(double.nan, 0, 0);
      expect(() => BoxCollider(center: nan), throwsA(argumentErrorSaying(['BoxCollider.center', 'NaN'])));
      final sphere = SphereCollider();
      expect(() => sphere.center = nan, throwsA(argumentErrorSaying(['SphereCollider.center'])));
      expect(sphere.center, Vec3.zero);
      expect(() => CharacterController(center: nan), throwsA(argumentErrorSaying(['CharacterController.center'])));
    });

    test('a mesh with no triangles names its collider and object', () async {
      final world = newWorld();
      final errors = errorsOf(world);
      world.add(
        GameObject(
          name: 'rock',
          components: [MeshCollider(mesh: Mesh())],
        ),
      );
      frames(world, 0.1);
      await pumpEventQueue();
      expect(errors, hasLength(1));
      expect('${errors.single.error}', allOf(contains('MeshCollider'), contains('rock'), contains('triangle')));
    });
  });

  group('P9: the main library has what a game needs', () {
    test('PhysicsStatus, and RawPhysics.gravity reads back', () {
      final e = PhysicsException(PhysicsStatus.poisoned, 'test');
      expect(e.status == PhysicsStatus.poisoned, isTrue);
      final raw = RawPhysics();
      addTearDown(raw.dispose);
      expect(raw.gravity, const Vec3(0, -9.81, 0));
      raw.gravity = const Vec3(0, -1, 0);
      expect(raw.gravity, const Vec3(0, -1, 0));
    });
  });

  group('P10: copyWith', () {
    test('changes only what it is given', () {
      const motor = JointMotor(targetVelocity: 1, force: 100);
      expect(motor.copyWith(targetVelocity: 3), isA<JointMotor>().having((m) => m.force, 'force', 100));
      expect(motor.copyWith(targetVelocity: 3).targetVelocity, 3);
      expect(motor.copyWith(force: 5).targetVelocity, 1);
      const spring = JointSpring(spring: 20, damper: 8, targetPosition: 1);
      final s = spring.copyWith(targetPosition: 2);
      expect([s.spring, s.damper, s.targetPosition], [20, 8, 2]);
      expect(spring.copyWith(spring: 1).damper, 8);
      expect(spring.copyWith(damper: 1).spring, 20);
      const limits = JointLimits(min: -1, max: 1);
      expect([limits.copyWith(max: 2).min, limits.copyWith(max: 2).max], [-1, 2]);
      expect(limits.copyWith(min: -2).max, 1);
      expect(const SoftJointLimit(limit: 1).copyWith(limit: 2).limit, 2);
      expect(const SoftJointLimit(limit: 1).copyWith().limit, 1);
      const material = PhysicsMaterial(
        dynamicFriction: 0.2,
        bounciness: 0.5,
        frictionCombine: PhysicsMaterialCombine.minimum,
        bounceCombine: PhysicsMaterialCombine.maximum,
      );
      final m = material.copyWith(bounciness: 0.9);
      expect(m.dynamicFriction, 0.2);
      expect(m.bounciness, 0.9);
      expect(m.frictionCombine, PhysicsMaterialCombine.minimum);
      expect(m.bounceCombine, PhysicsMaterialCombine.maximum);
      expect(material.copyWith(dynamicFriction: 1).bounciness, 0.5);
      expect(material.copyWith(frictionCombine: PhysicsMaterialCombine.average).dynamicFriction, 0.2);
      expect(
        material.copyWith(bounceCombine: PhysicsMaterialCombine.average).frictionCombine,
        PhysicsMaterialCombine.minimum,
      );
    });
  });

  group('P13: ignoreCollision names the colliders', () {
    test('in another world, and the same one twice', () {
      final world = newWorld();
      final other = newWorld();
      final a = SphereCollider();
      final b = BoxCollider();
      world.add(GameObject(name: 'a', components: [a]));
      other.add(GameObject(name: 'b', components: [b]));
      expect(
        () => world.physics.ignoreCollision(a, b),
        throwsA(stateErrorSaying(['ignoreCollision', 'BoxCollider', 'another world'])),
      );
      expect(
        () => world.physics.getIgnoreCollision(a, b),
        throwsA(stateErrorSaying(['getIgnoreCollision', 'BoxCollider', 'another world'])),
      );
      expect(() => world.physics.ignoreCollision(a, a), throwsA(argumentErrorSaying(['SphereCollider', 'itself'])));
    });
  });
}
