// Joints through the package (docs/stage-5.md, step 8): a fixed joint
// holds, a hinge swings with the formula's period and stops at its limits,
// turns with its motor and to its spring's target, a spring keeps its rest
// length, a chain hangs, and each breaks at its force and says so.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

const step = 1 / 50;

World newWorld({Vec3 gravity = const Vec3(0, -9.81, 0)}) {
  final world = World();
  addTearDown(world.dispose);
  world.physics.gravity = gravity;
  return world;
}

/// A box [size] across at [at], with a rigidbody of [mass], and [joints].
GameObject box(
  World world,
  Vec3 at, {
  Vec3 size = const Vec3(0.5, 0.5, 0.5),
  Quat rotation = Quat.identity,
  double mass = 1,
  List<Component> joints = const [],
  String name = 'box',
}) => world.add(
  GameObject(
    name: name,
    transform: Transform(position: at, rotation: rotation),
    components: [
      BoxCollider(size: size),
      Rigidbody(mass: mass, angularDamping: 0),
      ...joints,
    ],
  ),
);

void frames(World world, double seconds) {
  for (var i = 0; i < (seconds / step).round(); i++) {
    world.update(step);
  }
}

Matcher near(Vec3 v, [double tolerance = 1e-3]) =>
    predicate<Vec3>((got) => (got - v).length < tolerance, 'within $tolerance of $v');

/// A door 1 m wide, 2 m tall, of 10 kg, hinged on its left edge about Y,
/// with no gravity.
(GameObject, HingeJoint) door(World world, HingeJoint hinge) {
  world.physics.gravity = Vec3.zero;
  final door = box(world, const Vec3(0.5, 1, 0), size: const Vec3(1, 2, 0.1), mass: 10, joints: [hinge]);
  return (door, hinge);
}

HingeJoint doorHinge({
  bool useLimits = false,
  JointLimits limits = const JointLimits(),
  bool useMotor = false,
  JointMotor motor = const JointMotor(),
  bool useSpring = false,
  JointSpring spring = const JointSpring(),
}) => HingeJoint(
  anchor: const Vec3(-0.5, 0, 0),
  axis: const Vec3(0, 1, 0),
  useLimits: useLimits,
  limits: limits,
  useMotor: useMotor,
  motor: motor,
  useSpring: useSpring,
  spring: spring,
);

void main() {
  test('a fixed joint holds its body where it was, and says how hard', () {
    final world = newWorld();
    final joint = FixedJoint();
    final held = box(world, const Vec3(1, 5, 0), joints: [joint]);
    expect(joint.isJoined, isTrue);
    frames(world, 2);
    expect(held.position, near(const Vec3(1, 5, 0), 0.01));
    expect(joint.currentForce, near(const Vec3(0, 9.81, 0), 0.05));
    expect(joint.currentTorque, near(Vec3.zero, 0.05));
    expect(joint.connectedAnchor, near(const Vec3(1, 5, 0)));
    expect(joint.autoConfigureConnectedAnchor, isTrue);
  });

  test('a joint listed before its rigidbody waits for it, and joins two bodies as they are', () {
    final world = newWorld();
    world.add(
      GameObject(
        name: 'floor',
        transform: Transform(position: const Vec3(0, -0.5, 0)),
        components: [BoxCollider(size: const Vec3(100, 1, 100))],
      ),
    );
    final a = box(world, const Vec3(0, 3, 0));
    final joint = FixedJoint(connectedBody: a.get<Rigidbody>());
    final b = world.add(
      GameObject(
        transform: Transform(position: const Vec3(0.7, 3.4, 0), rotation: Quat.axisAngle(Vec3.up, 0.5)),
        components: [
          joint,
          BoxCollider(size: const Vec3(0.5, 0.5, 0.5)),
          Rigidbody(),
        ],
      ),
    );
    expect(joint.isJoined, isTrue);
    final before = a.rotation.inverse.rotate(b.position - a.position);
    frames(world, 3);
    expect(a.position.y, lessThan(1), reason: 'they fell');
    expect(a.rotation.inverse.rotate(b.position - a.position), near(before, 0.02));
  });

  test('a hinged pendulum swings with the period 2π√(L/g)', () {
    final world = newWorld();
    final hinge = HingeJoint(anchor: const Vec3(0, 2, 0), axis: const Vec3(0, 0, 1));
    final ball = box(world, const Vec3(0, 3, 0), size: const Vec3(0.1, 0.1, 0.1), joints: [hinge]);
    // Pulled 10° aside, about the pivot at (0, 5, 0): it joined as it was,
    // so its angle starts at 10°.
    ball.get<Rigidbody>()!.isKinematic = true;
    world.update(step);
    expect(hinge.angle, closeTo(0, 1e-3));
    hinge.enabled = false;
    final pull = Quat.axisAngle(const Vec3(0, 0, 1), 10 * math.pi / 180);
    ball.transform = Transform(position: const Vec3(0, 5, 0) + pull.rotate(const Vec3(0, -2, 0)), rotation: pull);
    ball.get<Rigidbody>()!.isKinematic = false;
    hinge
      ..connectedAnchor = const Vec3(0, 5, 0)
      ..enabled = true;
    expect(hinge.angle, closeTo(0, 1e-3), reason: 'joined as it is now');
    // Measured from the bottom, with the connected anchor at the pivot.
    final crossings = <double>[];
    var last = hinge.angle;
    for (var i = 1; i <= 300; i++) {
      world.update(step);
      final a = hinge.angle;
      if (last > -10 && a <= -10) crossings.add(i * step);
      last = a;
    }
    final period = crossings[1] - crossings[0];
    expect(period, closeTo(2 * math.pi * math.sqrt(2 / 9.81), 0.05));
    expect(hinge.velocity.abs(), lessThan(100));
  });

  test("a hinge stops at its limits, and they change on the joint as it is", () {
    final world = newWorld();
    final (d, hinge) = door(world, doorHinge(useLimits: true, limits: const JointLimits(min: -30, max: 90)));
    final body = d.get<Rigidbody>()!;
    body
      ..angularVelocity = const Vec3(0, 3, 0)
      ..linearVelocity = const Vec3(0, 0, -1.5);
    frames(world, 1);
    expect(hinge.angle, closeTo(90, 3));
    hinge.limits = const JointLimits(min: -30, max: 45);
    frames(world, 1);
    expect(hinge.angle, closeTo(45, 3), reason: 'pushed back to the new limit');
    hinge.useLimits = false;
    body
      ..angularVelocity = const Vec3(0, 3, 0)
      ..linearVelocity = d.rotation.rotate(const Vec3(0, 0, -1.5));
    frames(world, 0.5);
    expect(hinge.angle.abs(), greaterThan(90));
  });

  test("a hinge's motor turns it at its target velocity", () {
    final world = newWorld();
    final (_, hinge) = door(world, doorHinge(useMotor: true, motor: const JointMotor(targetVelocity: 90, force: 100)));
    frames(world, 1);
    expect(hinge.velocity, closeTo(90, 1));
    hinge.motor = const JointMotor(targetVelocity: -45, force: 100);
    frames(world, 1);
    expect(hinge.velocity, closeTo(-45, 1));
    hinge.useMotor = false;
    hinge.motor = const JointMotor(targetVelocity: 90, force: 0);
    hinge.useMotor = true;
    final before = hinge.velocity;
    frames(world, 0.5);
    expect(hinge.velocity, closeTo(before, 1), reason: 'no force: it keeps turning as it was');
  });

  test("a hinge's spring turns it to its target position", () {
    final world = newWorld();
    final spring = JointSpring(spring: 50, damper: 10, targetPosition: 45);
    final (_, hinge) = door(world, doorHinge(useSpring: true, spring: spring));
    frames(world, 6);
    expect(hinge.angle, closeTo(45, 1));
  });

  test('a spring joint keeps its bodies as far apart as they were, and stretches by its weight', () {
    final world = newWorld();
    final top = box(world, const Vec3(0, 5, 0), joints: [FixedJoint()]);
    final spring = SpringJoint(connectedBody: top.get<Rigidbody>(), spring: 100, damper: 5);
    final hanging = box(world, const Vec3(0, 3, 0), joints: [spring]);
    expect(spring.connectedAnchor, near(const Vec3(0, -2, 0)), reason: "where its anchor was, in the top's space");
    frames(world, 10);
    expect(hanging.position.y, closeTo(3 - 9.81 / 100, 0.01));
    expect(spring.currentForce, near(const Vec3(0, 9.81, 0), 0.5));
  });

  test("a spring joint doesn't pull within its distances", () {
    final world = newWorld(gravity: Vec3.zero);
    final spring = SpringJoint(connectedAnchor: Vec3.zero, spring: 20, damper: 2, minDistance: 1.5, maxDistance: 3);
    final ball = box(world, const Vec3(1, 0, 0), joints: [spring]);
    frames(world, 10);
    expect(ball.position.length, inInclusiveRange(1.48, 3));
    expect(spring.autoConfigureConnectedAnchor, isFalse);
  });

  test('a chain of hinged links hangs from the world and stays joined', () {
    final world = newWorld();
    final links = <GameObject>[];
    for (var i = 0; i < 6; i++) {
      final hinge = HingeJoint(
        connectedBody: links.isEmpty ? null : links.last.get<Rigidbody>(),
        anchor: const Vec3(0, 0.25, 0),
        axis: const Vec3(0, 0, 1),
      );
      // Each link lies along X, turned so its top (+Y) end points back.
      final along = Quat.axisAngle(const Vec3(0, 0, 1), math.pi / 2);
      links.add(box(world, Vec3(i * 0.5, 5, 0), size: const Vec3(0.1, 0.5, 0.1), rotation: along, joints: [hinge]));
    }
    // Joined as laid out: each link's top end on the one before's bottom.
    frames(world, 10);
    for (var i = 1; i < links.length; i++) {
      final top = links[i].position + links[i].rotation.rotate(const Vec3(0, 0.25, 0));
      final bottom = links[i - 1].position + links[i - 1].rotation.rotate(const Vec3(0, -0.25, 0));
      expect(top, near(bottom, 0.05), reason: 'link $i');
    }
    // It swings below the first link's hinge, no further than its 3 m.
    const pivot = Vec3(-0.25, 5, 0);
    expect(links.first.position + links.first.rotation.rotate(const Vec3(0, 0.25, 0)), near(pivot, 0.02));
    expect((links.last.position - pivot).length, lessThan(2.8));
    expect(links.last.position.y, lessThan(3.5));
  });

  test('a joint breaks at its force: its object hears of it, and it is taken off', () {
    final world = newWorld();
    final heard = <String>[];
    final joint = FixedJoint(breakForce: 5);
    final lamp = world.add(
      Lamp(
        heard,
        transform: Transform(position: const Vec3(0, 5, 0)),
        components: [
          BoxCollider(size: const Vec3(0.5, 0.5, 0.5)),
          Rigidbody(),
          joint,
          Listener(heard),
        ],
      ),
    );
    frames(world, 0.2);
    expect(heard, hasLength(2));
    expect(heard, containsAll([startsWith('object FixedJoint'), startsWith('behaviour FixedJoint')]));
    final force = double.parse(heard.first.split(' ').last);
    expect(force, closeTo(9.81, 0.5));
    expect(joint.isRemoved, isTrue);
    expect(joint.isJoined, isFalse);
    expect(lamp.get<FixedJoint>(), isNull);
    expect(world.physics.jointCount, 0);
    frames(world, 0.5);
    expect(lamp.position.y, lessThan(4.5), reason: 'it fell');
  });

  test('a joint holds up to its break torque', () {
    for (final (torque, breaks) in [(20.0, false), (5.0, true)]) {
      final world = newWorld();
      final joint = FixedJoint(anchor: const Vec3(-1, 0, 0), breakTorque: torque);
      final held = box(world, const Vec3(1, 5, 0), joints: [joint]);
      frames(world, 1);
      expect(held.position.y < 4, breaks, reason: 'break torque $torque');
      expect(joint.isRemoved, breaks);
      if (!breaks) expect(joint.currentTorque, near(const Vec3(0, 0, 9.81), 0.1));
    }
  });

  test('the break force of a swinging, turned and scaled joint changes as it is', () {
    final world = newWorld();
    final joint = HingeJoint(anchor: const Vec3(-0.5, 0, 0), axis: const Vec3(0, 0, 1));
    final plank = world.add(
      GameObject(
        transform: Transform(
          position: const Vec3(0.6, 5, 0),
          rotation: Quat.axisAngle(const Vec3(0, 0, 1), 0.3),
          scale: const Vec3(0.5792683, 0.08, 1.4),
        ),
        components: [BoxCollider(), Rigidbody(), joint],
      ),
    );
    frames(world, 0.5);
    expect(plank.position.y, lessThan(5), reason: 'swinging down');
    joint.breakForce = 1;
    frames(world, 0.2);
    expect(joint.isRemoved, isTrue, reason: 'it broke at its new force');
  });

  test('a joint waits while a rigidbody is away, and joins them again when it is back', () {
    final world = newWorld();
    final top = box(world, const Vec3(0, 5, 0), joints: [FixedJoint()]);
    final joint = FixedJoint(connectedBody: top.get<Rigidbody>());
    final hanging = box(world, const Vec3(0, 4, 0), joints: [joint]);
    expect(world.physics.jointCount, 2);
    top.get<Rigidbody>()!.enabled = false;
    expect(joint.isJoined, isFalse);
    frames(world, 0.5);
    expect(hanging.position.y, lessThan(3.5), reason: 'it fell');
    // Back, it joins them as they are now.
    top.get<Rigidbody>()!.enabled = true;
    frames(world, 0.02);
    expect(joint.isJoined, isTrue);
    final gap = hanging.position.y - top.position.y;
    frames(world, 1);
    expect(hanging.position.y - top.position.y, closeTo(gap, 0.01));
    // Disabled, it lets go; removed, it's gone.
    joint.enabled = false;
    expect(world.physics.jointCount, 1);
    joint.enabled = true;
    expect(joint.isJoined, isTrue);
    hanging.removeComponent(joint);
    expect(world.physics.jointCount, 1);
    expect(joint.isJoined, isFalse);
  });

  test('bad values are refused', () {
    expect(() => FixedJoint(breakForce: 0), throwsArgumentError);
    expect(() => FixedJoint(breakTorque: double.nan), throwsArgumentError);
    expect(() => HingeJoint(axis: Vec3.zero), throwsArgumentError);
    expect(() => HingeJoint(limits: const JointLimits(min: 10, max: -10)), throwsArgumentError);
    expect(() => HingeJoint(limits: const JointLimits(min: -200)), throwsArgumentError);
    expect(() => HingeJoint(motor: const JointMotor(force: -1)), throwsArgumentError);
    expect(() => HingeJoint(spring: const JointSpring(damper: -1)), throwsArgumentError);
    expect(() => SpringJoint(spring: -1), throwsArgumentError);
    expect(() => SpringJoint(minDistance: 2, maxDistance: 1), throwsArgumentError);
    final hinge = HingeJoint();
    expect(() => hinge.axis = Vec3.zero, throwsArgumentError);
    expect(() => hinge.limits = const JointLimits(max: 181), throwsArgumentError);
    expect(hinge.angle, 0);
    expect(hinge.currentForce, Vec3.zero);
  });
}

class Lamp extends GameObject with JointBreaks {
  Lamp(this.heard, {super.transform, super.components});

  final List<String> heard;

  @override
  void onJointBreak(Joint joint, double breakForce) => heard.add('object ${joint.runtimeType} $breakForce');
}

class Listener extends Behaviour with JointBreaks {
  Listener(this.heard);

  final List<String> heard;

  @override
  void onJointBreak(Joint joint, double breakForce) => heard.add('behaviour ${joint.runtimeType} $breakForce');
}
