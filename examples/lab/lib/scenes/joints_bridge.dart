// Stage 5 step 8: a rope bridge of planks, each on a HingeJoint to the
// next, the ends to the world. Tap to drop a crate on it; with a break
// force, the joints break under the load, and the planks hear of it.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class JointsBridgeScene extends LabScene {
  @override
  String get title => 'Joints: a bridge';
  @override
  String get step => 'stage 5 step 8';
  @override
  String get about =>
      'Twelve planks hang between two posts, each on a HingeJoint to the one before, and the ends to the world. '
      'Tap the bridge to drop a crate on it. Make the joints breakable, with a break force, and load it: a joint '
      'that holds harder than that breaks, and its plank hears of it (onJointBreak).';

  @override
  Vec3 get eye => Vec3(0, 4, 8);
  @override
  Vec3 get target => Vec3(0, 1.5, 0);

  late final Camera camera = find('camera')!.get<Camera>()!;
  final planks = <Plank>[];
  final crates = <GameObject>[];

  /// The joints' break force, in newtons: infinity for unbreakable.
  var breakForce = double.infinity;
  var broken = 0;

  static const count = 12;

  /// The posts' tops, where the bridge hangs from.
  static final leftEnd = Vec3(-3, 2.5, 0);
  static final rightEnd = Vec3(3, 2.5, 0);

  @override
  void build() {
    add(floorCollider());
    for (final end in [leftEnd, rightEnd]) {
      add(
        shape('post', Mesh.cube(), const Color(0xFF7A8290), at: Vec3(end.x, end.y / 2, 0), size: Vec3(0.3, end.y, 1.6)),
      );
    }
    rebuild();
  }

  /// Takes the planks and crates away, and hangs new planks, sagging.
  void rebuild() {
    for (final o in [...planks, ...crates]) {
      o.destroy();
    }
    planks.clear();
    crates.clear();
    broken = 0;
    final points = sag(leftEnd, rightEnd, count, 7.2);
    for (var i = 0; i < count; i++) {
      final (a, b) = (points[i], points[i + 1]);
      final plank = add(Plank(this, a, b, previous: planks.isEmpty ? null : planks.last));
      planks.add(plank);
    }
    // The last plank's far end, to the world too.
    planks.last.addComponent(HingeJoint(anchor: Vec3(0.5, 0, 0), axis: Vec3(0, 0, 1), breakForce: breakForce));
  }

  /// [count] + 1 points from [a] to [b], [length] apart in all, as a
  /// circle's arc that sags down between them: each segment the same.
  static List<Vec3> sag(Vec3 a, Vec3 b, int count, double length) {
    final chord = (b - a).length;
    // The arc's half angle θ: sin θ / θ = chord / length.
    var theta = 1.0;
    for (var i = 0; i < 30; i++) {
      final f = math.sin(theta) / theta - chord / length;
      final d = (theta * math.cos(theta) - math.sin(theta)) / (theta * theta);
      theta -= f / d;
    }
    final radius = chord / (2 * math.sin(theta));
    final middle = (a + b) * 0.5;
    final center = middle + Vec3(0, radius * math.cos(theta), 0);
    return [
      for (var i = 0; i <= count; i++)
        () {
          final angle = -theta + 2 * theta * i / count;
          return center + Vec3(radius * math.sin(angle), -radius * math.cos(angle), 0);
        }(),
    ];
  }

  void broke(Plank plank, double force) {
    broken++;
    log.add('a joint on ${plank.name} broke, holding with ${force.toStringAsFixed(0)} N');
  }

  @override
  void onStart() {
    super.onStart();
    on(input.onTap, (finger) {
      final hit = physics.raycast(camera.screenPointToRay(finger.position));
      if (hit == null) return;
      dropCrate(Vec3(hit.point.x, 4.5, 0));
    });
  }

  /// Drops a 20 kg crate from [at].
  void dropCrate(Vec3 at) {
    crates.add(
      add(
        shape(
          'crate',
          Mesh.cube(),
          const Color(0xFFC08A4A),
          at: at,
          size: Vec3(0.6, 0.6, 0.6),
          components: [BoxCollider(), Rigidbody(mass: 20)],
        ),
      ),
    );
    log.add('dropped a crate at x = ${at.x.toStringAsFixed(1)}');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final lowest = planks.map((p) => p.position.y).reduce(math.min);
    readout.value = [
      'the lowest plank is ${lowest.toStringAsFixed(2)} m up',
      breakForce.isFinite ? 'joints break at ${breakForce.toStringAsFixed(0)} N' : 'joints never break',
      '$broken broken',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('drop a crate in the middle', () => dropCrate(Vec3(0, 4.5, 0))),
    Toggle('breakable', () => breakForce.isFinite, (on) => setBreakForce(on ? 1500 : double.infinity)),
    Slider(
      'break force (N)',
      () => breakForce.isFinite ? breakForce : 3000,
      (v) => setBreakForce(v.roundToDouble()),
      min: 200,
      max: 3000,
    ),
    Button('build it again', () {
      rebuild();
      log.add('built it again');
    }),
  ];

  void setBreakForce(double force) {
    breakForce = force;
    for (final p in planks) {
      for (final j in p.getAll<HingeJoint>()) {
        j.breakForce = force;
      }
    }
  }
}

/// A plank from [a] to [b], on a hinge to [previous] (or the world), that
/// tells the scene when one of its joints breaks.
class Plank extends GameObject with JointBreaks {
  Plank(this.scene, Vec3 a, Vec3 b, {Plank? previous})
    : super(
        name: 'plank',
        transform: Transform(
          position: (a + b) * 0.5,
          rotation: Quat.axisAngle(Vec3(0, 0, 1), math.atan2(b.y - a.y, b.x - a.x)),
          scale: Vec3((b - a).length - 0.02, 0.08, 1.4),
        ),
        components: [
          MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF9A6A3A))),
          BoxCollider(),
          Rigidbody(mass: 2),
          HingeJoint(
            connectedBody: previous?.get<Rigidbody>(),
            anchor: Vec3(-0.5, 0, 0),
            axis: Vec3(0, 0, 1),
            breakForce: scene.breakForce,
          ),
        ],
      );

  final JointsBridgeScene scene;

  @override
  void onJointBreak(Joint joint, double breakForce) => scene.broke(this, breakForce);
}
