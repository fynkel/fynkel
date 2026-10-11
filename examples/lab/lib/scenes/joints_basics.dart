// Stage 5 step 8: joints, the basics. A door on a HingeJoint that a spring
// swings shut, a chain of links hinged to each other, and a crate hanging
// on a SpringJoint.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class JointsBasicsScene extends LabScene {
  @override
  String get title => 'Joints: the basics';
  @override
  String get step => 'stage 5 step 8';
  @override
  String get about =>
      'Each has a Rigidbody and a joint. The door turns on a HingeJoint about its left edge, between its limits '
      '(-90° to 90°), and its spring swings it shut. The chain is six links, each on a HingeJoint to the one above, '
      'the top one to the world. The crate hangs on a SpringJoint, and bobs.';

  @override
  Vec3 get eye => Vec3(0, 3, 9);
  @override
  Vec3 get target => Vec3(0, 1.8, 0);

  late final GameObject door, crate, rope;
  late final HingeJoint hinge;
  late final SpringJoint spring;
  final links = <GameObject>[];

  /// Where the crate's spring hangs from.
  static final hook = Vec3(3, 4, 0);

  @override
  void build() {
    add(floorCollider());
    // The door, hinged on its left edge to a post.
    add(shape('post', Mesh.cube(), const Color(0xFF7A8290), at: Vec3(-3.6, 1.1, 0), size: Vec3(0.2, 2.2, 0.2)));
    hinge = HingeJoint(
      anchor: Vec3(-0.5, 0, 0),
      axis: Vec3(0, 1, 0),
      useLimits: true,
      limits: const JointLimits(min: -90, max: 90),
      useSpring: true,
      spring: const JointSpring(spring: 20, damper: 8),
    );
    door = add(
      shape(
        'door',
        Mesh.cube(),
        const Color(0xFFC08A4A),
        at: Vec3(-3, 1.1, 0),
        size: Vec3(1, 2, 0.1),
        components: [BoxCollider(), Rigidbody(mass: 10), hinge],
      ),
    );
    // The chain, hanging from (0, 4, 0).
    for (var i = 0; i < 6; i++) {
      links.add(
        add(
          shape(
            'link ${i + 1}',
            Mesh.cube(),
            i.isEven ? const Color(0xFFB0B8C8) : const Color(0xFF8A90A0),
            at: Vec3(0, 3.8 - i * 0.4, 0),
            size: Vec3(0.12, 0.4, 0.12),
            components: [
              BoxCollider(),
              Rigidbody(mass: 0.5),
              HingeJoint(
                connectedBody: links.isEmpty ? null : links.last.get<Rigidbody>(),
                anchor: Vec3(0, 0.5, 0),
                axis: Vec3(0, 0, 1),
              ),
            ],
          ),
        ),
      );
    }
    // The crate, on a spring from the hook.
    spring = SpringJoint(connectedAnchor: hook, spring: 40, damper: 1, maxDistance: 1.5);
    crate = add(
      shape(
        'crate',
        Mesh.cube(),
        const Color(0xFF3A8EE0),
        at: Vec3(3, 2.5, 0),
        size: Vec3(0.6, 0.6, 0.6),
        components: [BoxCollider(), Rigidbody(mass: 2), spring],
      ),
    );
    rope = add(shape('spring', Mesh.cube(), const Color(0xFFFFD040), size: Vec3(0.04, 0.04, 1)));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // The spring, drawn from the hook to the crate.
    final to = crate.position - hook;
    rope.transform = Transform(
      position: hook + to * 0.5,
      rotation: Quat.lookRotation(to, up: Vec3(1, 0, 0)),
      scale: Vec3(0.04, 0.04, to.length),
    );
    readout.value = [
      'the door is at ${hinge.angle.toStringAsFixed(0)}°',
      'the crate is ${to.length.toStringAsFixed(2)} m below the hook',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('push the door', () {
      door.get<Rigidbody>()!.addForceAtPosition(
        Vec3(0, 0, -60),
        door.position + Vec3(0.5, 0, 0),
        mode: ForceMode.impulse,
      );
      log.add('pushed the door: its spring swings it shut');
    }),
    Button('swing the chain', () {
      links.last.get<Rigidbody>()!.addForce(Vec3(4, 0, 0), mode: ForceMode.impulse);
      log.add('swung the chain');
    }),
    Button('pull the crate down', () {
      crate.get<Rigidbody>()!.addForce(Vec3(0, -20, 0), mode: ForceMode.impulse);
      log.add('pulled the crate: past its 1.5 m maxDistance, the spring pulls it back');
    }),
    Toggle('the door\'s spring', () => hinge.useSpring, (on) => hinge.useSpring = on),
  ];
}
