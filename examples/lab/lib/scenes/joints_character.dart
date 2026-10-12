// Stage 5 step 8: a CharacterJoint, the basics. Two arms hang from the
// world, each on a CharacterJoint at its top: the left on Unity's limits
// (it twists -20° to 70° and swings 40° each way), the right with its
// limits at 180°, a free ball joint. Swing them both the same.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class JointsCharacterScene extends LabScene {
  @override
  String get title => 'Joints: a character joint';
  @override
  String get step => 'stage 5 step 8';
  @override
  String get about =>
      'Two arms, each on a CharacterJoint at its top: a ball in a socket, as a ragdoll\'s shoulder or a chain\'s '
      'link. The left has Unity\'s limits: it swings 40° each way, and twists -20° to 70° about its length. The '
      'right has its limits at 180°, so it swings and twists every way. Swing them both the same, and see where '
      'the left one stops.';

  @override
  Vec3 get eye => Vec3(0, 2.5, 7);
  @override
  Vec3 get target => Vec3(0, 2.5, 0);

  late final GameObject limited, free;
  late final CharacterJoint joint;

  @override
  void build() {
    add(floorCollider());
    joint = CharacterJoint(anchor: Vec3(0, 0.5, 0), axis: Vec3(0, 1, 0), swingAxis: Vec3(1, 0, 0));
    limited = arm('limited arm', -1.5, const Color(0xFF3A8EE0), joint);
    free = arm(
      'free arm',
      1.5,
      const Color(0xFFE0A030),
      CharacterJoint(
        anchor: Vec3(0, 0.5, 0),
        axis: Vec3(0, 1, 0),
        swingAxis: Vec3(1, 0, 0),
        lowTwistLimit: SoftJointLimit(limit: -180.deg),
        highTwistLimit: SoftJointLimit(limit: 180.deg),
        swing1Limit: SoftJointLimit(limit: 180.deg),
        swing2Limit: SoftJointLimit(limit: 180.deg),
      ),
    );
  }

  /// An arm 1.5 m long hanging at [x] from a post, on [joint] at its top.
  GameObject arm(String name, double x, Color color, CharacterJoint joint) {
    add(shape('$name\'s post', Mesh.cube(), const Color(0xFF7A8290), at: Vec3(x, 4.1, 0), size: Vec3(0.3, 0.2, 0.3)));
    return add(
      shape(
        name,
        Mesh.cube(),
        color,
        at: Vec3(x, 3.25, 0),
        size: Vec3(0.15, 1.5, 0.15),
        components: [
          BoxCollider(),
          Rigidbody(mass: 1, interpolation: RigidbodyInterpolation.interpolate),
          joint,
        ],
      ),
    );
  }

  /// How far [arm] has swung from hanging straight down, in degrees.
  static double swing(GameObject arm) => math.acos(arm.rotation.rotate(Vec3.up).y.clamp(-1, 1)) * 180 / math.pi;

  void push(Vec3 impulse) {
    for (final a in [limited, free]) {
      a.get<Rigidbody>()!.addForceAtPosition(impulse, a.transformPoint(Vec3(0, -0.5, 0)), mode: ForceMode.impulse);
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      'the limited arm has swung ${swing(limited).toStringAsFixed(0)}°',
      'the free arm has swung ${swing(free).toStringAsFixed(0)}°',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('swing them sideways', () {
      push(Vec3(2, 0, 0));
      log.add('swung both: the limited one stops at 40°');
    }),
    Button('swing them forward', () {
      push(Vec3(0, 0, 2));
      log.add('swung both forward');
    }),
    Button('twist them', () {
      for (final a in [limited, free]) {
        a.get<Rigidbody>()!.addTorque(Vec3(0, 0.1, 0), mode: ForceMode.impulse);
      }
      log.add('twisted both: the limited one stops at 70°');
    }),
    Slider(
      'the limited arm\'s swing (°)',
      () => joint.swing1Limit.limit.inDegrees,
      (v) {
        final limit = SoftJointLimit(limit: v.roundToDouble().deg);
        joint
          ..swing1Limit = limit
          ..swing2Limit = limit;
      },
      min: 0,
      max: 180,
    ),
  ];
}
